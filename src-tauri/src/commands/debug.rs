//! Launching applications under a Debug Adapter Protocol adapter.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use cb_core::dap::model::{DebugEvent, DebugState};
use cb_core::dap::protocol::{self, Capabilities, Message, Request, Response};
use cb_core::dap::registry::{self, AdapterSpec, Debuggee, Resolution};
use cb_core::lsp::framing::{self, Decoder};
use cb_core::model::{Invocation, RunConfig, RunKind};
use cb_core::process::{configure_process_group, Stream};
use cb_core::running::{observe, RunMeta, RunningStore};
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::state::{AppState, WorkspaceSlot};

type Reader = Box<dyn AsyncRead + Unpin + Send>;
type Writer = Box<dyn AsyncWrite + Unpin + Send>;

struct Adapter {
    child: tokio::process::Child,
    reader: Reader,
    writer: Writer,
    pid: u32,
    program: String,
}

struct Prepared {
    config: RunConfig,
    debuggee: Debuggee,
    spec: AdapterSpec,
    launch: Value,
}

fn emit(channel: &Channel<DebugEvent>, event: DebugEvent) {
    let _ = channel.send(event);
}

fn emit_state(channel: &Channel<DebugEvent>, value: DebugState) {
    emit(channel, DebugEvent::State { state: value });
}

fn output(channel: &Channel<DebugEvent>, stream: Stream, text: impl Into<String>) {
    emit(
        channel,
        DebugEvent::Output {
            stream,
            text: text.into(),
        },
    );
}

/// Where the adapters vendored by `pnpm debuggers:fetch` live, when they were
/// vendored at all.
///
/// `None` is an ordinary answer, exactly as it is for the inspector sidecar in
/// `inspect.rs`: `cargo build` produces no resource directory, and a build made
/// with no network produces one with nothing in it. `dap::registry` falls
/// through to PATH either way.
fn bundled_debuggers(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resolve("debuggers", tauri::path::BaseDirectory::Resource)
        .ok()
        .filter(|dir| dir.is_dir())
}

fn adapter_for(
    config: &RunConfig,
    bundled: Option<&Path>,
) -> Result<(Debuggee, AdapterSpec), DebugState> {
    let Some(debuggee) = Debuggee::for_ecosystem(&config.ecosystem) else {
        return Err(DebugState::Failed {
            detail: format!("{} configurations cannot be debugged", config.ecosystem),
        });
    };
    match registry::resolve(debuggee, &cb_core::lsp::registry::RealProbe, bundled) {
        Resolution::Found(spec) => Ok((debuggee, spec)),
        Resolution::NotFound { looked_for, hint } => {
            Err(DebugState::NotInstalled { looked_for, hint })
        }
        Resolution::Misconfigured { detail } => Err(DebugState::Failed { detail }),
    }
}

async fn pump_build(
    mut events: tokio::sync::mpsc::Receiver<cb_core::process::ProcessEvent>,
    channel: Channel<DebugEvent>,
) {
    while let Some(event) = events.recv().await {
        match event {
            cb_core::process::ProcessEvent::Output { stream, text } => {
                output(&channel, stream, text)
            }
            cb_core::process::ProcessEvent::Failed { message } => output(
                &channel,
                Stream::Stderr,
                format!("[code-basics] {message}\n"),
            ),
            _ => {}
        }
    }
}

async fn build_dotnet_target(
    slot: &WorkspaceSlot,
    workspace: &cb_core::workspace::Workspace,
    config: &RunConfig,
    channel: &Channel<DebugEvent>,
) -> Result<PathBuf, String> {
    let project = config
        .project
        .as_ref()
        .ok_or_else(|| "a .NET debug configuration must target a project".to_string())?;
    let build = cb_core::adapters::dotnet::build_action_invocation(
        config,
        cb_core::adapters::dotnet::BuildAction::Build,
        &workspace.root,
    );
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    tokio::spawn(pump_build(rx, channel.clone()));
    let code = slot
        .supervisor
        .run(&format!("{}:debug-build", config.id), &build, tx)
        .await
        .map_err(|e| format!("debug build failed: {e:#}"))?;
    if code != Some(0) {
        return Err(format!("debug build exited with {code:?}"));
    }

    // Ask evaluated MSBuild for the real output. AssemblyName, OutputPath and
    // RuntimeIdentifier can all make a guessed bin/Debug path wrong.
    let absolute_project = workspace.root.join(project);
    let mut command = tokio::process::Command::new(cb_core::process::resolve_program("dotnet"));
    command
        .arg("msbuild")
        .arg(&absolute_project)
        .arg("-nologo")
        .arg("-getProperty:TargetPath")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(configuration) = &config.build_configuration {
        command.arg(format!("-property:Configuration={configuration}"));
    }
    if let Some(framework) = &config.framework {
        command.arg(format!("-property:TargetFramework={framework}"));
    }
    configure_process_group(&mut command);
    #[cfg(windows)]
    cb_core::process::no_window(command.as_std_mut());
    let result = command
        .output()
        .await
        .map_err(|e| format!("failed to ask MSBuild for TargetPath: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "MSBuild could not resolve TargetPath: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    let raw = String::from_utf8_lossy(&result.stdout);
    let value = raw
        .lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty())
        .ok_or_else(|| "MSBuild returned an empty TargetPath".to_string())?;
    let path = PathBuf::from(value);
    Ok(if path.is_absolute() {
        path
    } else {
        absolute_project
            .parent()
            .unwrap_or(&workspace.root)
            .join(path)
    })
}

fn dotnet_launch(
    workspace: &cb_core::workspace::Workspace,
    config: &RunConfig,
    target: &Path,
) -> Value {
    let mut env = BTreeMap::new();
    let mut args = Vec::new();
    let mut cwd = config
        .cwd
        .as_ref()
        .map(|p| workspace.root.join(p))
        .or_else(|| {
            config
                .project
                .as_ref()
                .and_then(|p| workspace.root.join(p).parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(|| workspace.root.clone());

    if !config.ignore_launch_settings {
        if let Some(project) = &config.project {
            let profiles = cb_core::workspace::launch_profiles(&workspace.root.join(project));
            let profile = config
                .launch_profile
                .as_deref()
                .and_then(|name| profiles.iter().find(|p| p.name == name))
                .or_else(|| profiles.iter().find(|p| p.launchable));
            if let Some(profile) = profile {
                env.extend(profile.env.clone());
                if let Some(url) = &profile.application_url {
                    env.insert("ASPNETCORE_URLS".into(), url.clone());
                }
                args.extend(profile.args.clone());
                if config.cwd.is_none() {
                    if let Some(dir) = &profile.working_directory {
                        let path = PathBuf::from(dir);
                        cwd = if path.is_absolute() {
                            path
                        } else {
                            workspace.root.join(path)
                        };
                    }
                }
            }
        }
    }
    env.extend(config.env.clone());
    args.extend(config.args.clone());
    json!({
        "name": config.name,
        "type": "coreclr",
        "request": "launch",
        "program": target,
        "cwd": cwd,
        "args": args,
        "env": env,
        "stopAtEntry": false,
        "justMyCode": true,
        "console": "internalConsole"
    })
}

fn node_launch(invocation: &Invocation, config: &RunConfig) -> Value {
    json!({
        "name": config.name,
        "type": "pwa-node",
        "request": "launch",
        "cwd": invocation.cwd,
        "runtimeExecutable": invocation.program,
        "runtimeArgs": invocation.args,
        "args": [],
        "env": invocation.env,
        "console": "internalConsole",
        "outputCapture": "std",
        "stopOnEntry": false,
        "autoAttachChildProcesses": false,
        "restart": false
    })
}

async fn spawn_adapter(
    debuggee: Debuggee,
    spec: &AdapterSpec,
    channel: &Channel<DebugEvent>,
) -> Result<Adapter, String> {
    let is_node_script = debuggee == Debuggee::Node
        && spec.program.extension().and_then(|e| e.to_str()) == Some("js");
    let program = if is_node_script {
        cb_core::process::resolve_program("node")
    } else {
        spec.program.clone()
    };
    let program_name = program.display().to_string();
    let mut command = tokio::process::Command::new(program);
    if is_node_script {
        command.arg(&spec.program);
    } else {
        command.args(&spec.args);
    }
    if debuggee == Debuggee::Node {
        command.args(["0", "127.0.0.1"]);
    }
    command
        .stdin(if debuggee == Debuggee::DotNet {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_process_group(&mut command);
    #[cfg(windows)]
    cb_core::process::no_window(command.as_std_mut());
    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to start {}: {e}", spec.description))?;
    let pid = child
        .id()
        .ok_or_else(|| "debug adapter started without a pid".to_string())?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "debug adapter has no stderr".to_string())?;
    let error_channel = channel.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            output(
                &error_channel,
                Stream::Stderr,
                format!("[debug adapter] {line}\n"),
            );
        }
    });

    if debuggee == Debuggee::DotNet {
        let reader = child
            .stdout
            .take()
            .ok_or_else(|| "debug adapter has no stdout".to_string())?;
        let writer = child
            .stdin
            .take()
            .ok_or_else(|| "debug adapter has no stdin".to_string())?;
        return Ok(Adapter {
            child,
            reader: Box::new(reader),
            writer: Box::new(writer),
            pid,
            program: program_name,
        });
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Node debug server has no stdout".to_string())?;
    let mut lines = BufReader::new(stdout).lines();
    let port = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            output(channel, Stream::Stderr, format!("[debug adapter] {line}\n"));
            if let Some(port) = line
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.parse::<u16>().ok())
                .next_back()
            {
                if port != 0 {
                    return Ok(port);
                }
            }
        }
        Err("Node debug server exited before announcing a port".to_string())
    })
    .await
    .map_err(|_| "Node debug server did not announce a port within 10 seconds".to_string())??;
    let socket = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| format!("could not connect to Node debug server on port {port}: {e}"))?;
    let (reader, writer) = socket.into_split();
    Ok(Adapter {
        child,
        reader: Box::new(reader),
        writer: Box::new(writer),
        pid,
        program: program_name,
    })
}

async fn send(writer: &mut Writer, message: &Message) -> Result<(), String> {
    let body = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    writer
        .write_all(&framing::encode(&body))
        .await
        .map_err(|e| format!("failed to write to debug adapter: {e}"))?;
    writer.flush().await.map_err(|e| e.to_string())
}

async fn receive(
    reader: &mut Reader,
    decoder: &mut Decoder,
    queued: &mut VecDeque<Message>,
) -> Result<Message, String> {
    loop {
        if let Some(message) = queued.pop_front() {
            return Ok(message);
        }
        let mut chunk = [0_u8; 8192];
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|e| format!("failed to read debug adapter: {e}"))?;
        if count == 0 {
            return Err("debug adapter closed its protocol stream".to_string());
        }
        for frame in decoder.push(&chunk[..count]).map_err(|e| e.to_string())? {
            queued.push_back(serde_json::from_slice(&frame).map_err(|e| e.to_string())?);
        }
    }
}

async fn run_protocol(
    adapter: &mut Adapter,
    debuggee: Debuggee,
    launch: Value,
    channel: &Channel<DebugEvent>,
) -> Result<Option<i64>, String> {
    let mut next_seq = 1_i64;
    let mut decoder = Decoder::new();
    let mut queued = VecDeque::new();
    let initialize_seq = next_seq;
    next_seq += 1;
    send(
        &mut adapter.writer,
        &Message::Request(Request {
            seq: initialize_seq,
            command: "initialize".into(),
            arguments: Some(protocol::initialize_arguments(
                "code-basics",
                debuggee.adapter_id(),
            )),
        }),
    )
    .await?;
    let capabilities: Capabilities = loop {
        match receive(&mut adapter.reader, &mut decoder, &mut queued).await? {
            Message::Response(response) if response.request_seq == initialize_seq => {
                if !response.success {
                    return Err(response.failure_text());
                }
                break response
                    .body
                    .and_then(|body| serde_json::from_value(body).ok())
                    .unwrap_or_default();
            }
            Message::Request(request) => {
                reply_unsupported(&mut adapter.writer, &mut next_seq, request).await?
            }
            _ => {}
        }
    };

    let launch_seq = next_seq;
    next_seq += 1;
    send(
        &mut adapter.writer,
        &Message::Request(Request {
            seq: launch_seq,
            command: "launch".into(),
            arguments: Some(launch),
        }),
    )
    .await?;

    let mut launch_answered = false;
    let mut configuration_seq = None;
    let mut exit_code = None;
    loop {
        match receive(&mut adapter.reader, &mut decoder, &mut queued).await? {
            Message::Response(response) => {
                if !response.success {
                    return Err(response.failure_text());
                }
                if response.request_seq == launch_seq {
                    launch_answered = true;
                    emit_state(channel, DebugState::Running);
                }
                if configuration_seq == Some(response.request_seq) && launch_answered {
                    emit_state(channel, DebugState::Running);
                }
            }
            Message::Event(event) if event.event == "initialized" => {
                if capabilities.supports_configuration_done_request {
                    let seq = next_seq;
                    next_seq += 1;
                    configuration_seq = Some(seq);
                    send(
                        &mut adapter.writer,
                        &Message::Request(Request {
                            seq,
                            command: "configurationDone".into(),
                            arguments: None,
                        }),
                    )
                    .await?;
                }
            }
            Message::Event(event) if event.event == "output" => {
                if let Some(body) = protocol::Output::from_body(event.body.as_ref()) {
                    let stream = if body.category == "stderr" {
                        Stream::Stderr
                    } else {
                        Stream::Stdout
                    };
                    output(channel, stream, body.output);
                }
            }
            Message::Event(event) if event.event == "process" => {
                emit_state(channel, DebugState::Running);
            }
            Message::Event(event) if event.event == "exited" => {
                exit_code = protocol::exited_code(event.body.as_ref());
            }
            Message::Event(event) if event.event == "terminated" => break,
            Message::Event(event) if event.event == "stopped" => {
                if let Some(stopped) = protocol::Stopped::from_body(event.body.as_ref()) {
                    emit_state(
                        channel,
                        DebugState::Paused {
                            reason: stopped.reason,
                            thread_id: stopped.thread_id,
                            description: stopped.description.or(stopped.text),
                        },
                    );
                }
            }
            Message::Event(event) if event.event == "continued" => {
                emit_state(channel, DebugState::Running)
            }
            Message::Request(request) => {
                reply_unsupported(&mut adapter.writer, &mut next_seq, request).await?
            }
            _ => {}
        }
    }
    Ok(exit_code)
}

async fn reply_unsupported(
    writer: &mut Writer,
    next_seq: &mut i64,
    request: Request,
) -> Result<(), String> {
    let response = Response {
        seq: *next_seq,
        request_seq: request.seq,
        success: false,
        command: request.command,
        message: Some("This client does not support that reverse request".into()),
        body: None,
    };
    *next_seq += 1;
    send(writer, &Message::Response(response)).await
}

async fn prepare_one(
    slot: &WorkspaceSlot,
    workspace: &cb_core::workspace::Workspace,
    mut config: RunConfig,
    env: Option<BTreeMap<String, String>>,
    build_configuration: Option<String>,
    channel: &Channel<DebugEvent>,
    bundled: Option<&Path>,
) -> Result<Prepared, String> {
    let (debuggee, spec) = adapter_for(&config, bundled).map_err(|failure| {
        emit_state(channel, failure.clone());
        match failure {
            DebugState::NotInstalled { hint, .. } => hint,
            DebugState::Failed { detail } => detail,
            _ => "debug adapter is unavailable".to_string(),
        }
    })?;
    config.env.extend(env.into_iter().flatten());
    if let Some(configuration) = build_configuration.filter(|c| !c.trim().is_empty()) {
        config.build_configuration = Some(configuration);
    }

    slot.supervisor.cancel(&config.id).await;
    slot.debug.cancel(&config.id).await;
    emit_state(channel, DebugState::Starting);
    let launch = match debuggee {
        Debuggee::DotNet => {
            let target = build_dotnet_target(slot, workspace, &config, channel).await?;
            dotnet_launch(workspace, &config, &target)
        }
        Debuggee::Node => {
            let invocation = cb_core::invocation::build(workspace, &config, None)?;
            node_launch(&invocation, &config)
        }
    };
    Ok(Prepared {
        config,
        debuggee,
        spec,
        launch,
    })
}

async fn run_prepared(
    slot: std::sync::Arc<WorkspaceSlot>,
    prepared: Prepared,
    channel: Channel<DebugEvent>,
    running: RunningStore,
    terminal_events: bool,
) -> Result<(), String> {
    let Prepared {
        config,
        debuggee,
        spec,
        launch,
    } = prepared;
    let mut adapter = spawn_adapter(debuggee, &spec, &channel).await?;
    slot.debug.register(&config.id, adapter.pid).await;
    let root = slot.root.display().to_string();
    running.record(observe(
        adapter.pid,
        &config.id,
        RunMeta {
            root: root.clone(),
            label: format!("{} (debug)", config.name),
            kind: cb_core::running::RunKind::Run,
        },
        &adapter.program,
    ));
    let result = run_protocol(&mut adapter, debuggee, launch, &channel).await;
    let ended_on_its_own = slot.debug.finish(&config.id, adapter.pid).await;
    let _ = cb_core::process::kill_tree_async(adapter.pid).await;
    let _ = adapter.child.wait().await;
    running.remove_if_pid(&root, &config.id, adapter.pid);
    if !ended_on_its_own {
        if terminal_events {
            emit_state(&channel, DebugState::Exited { code: None });
        }
        return Ok(());
    }
    match result {
        Ok(code) => {
            if terminal_events {
                emit_state(&channel, DebugState::Exited { code });
            }
            Ok(())
        }
        Err(detail) => {
            if terminal_events {
                emit_state(
                    &channel,
                    DebugState::Failed {
                        detail: detail.clone(),
                    },
                );
            }
            Err(detail)
        }
    }
}

#[tauri::command]
pub async fn start_debug(
    app: AppHandle,
    state: State<'_, AppState>,
    config_id: String,
    channel: Channel<DebugEvent>,
    env: Option<BTreeMap<String, String>>,
    build_configuration: Option<String>,
) -> Result<(), String> {
    let slot = state.active_slot()?;
    let workspace = slot.workspace();
    let config = workspace
        .configs
        .iter()
        .find(|config| config.id == config_id)
        .cloned()
        .ok_or_else(|| format!("no configuration named {config_id}"))?;
    if config.kind != RunKind::App {
        return Err("only application configurations can be debugged".into());
    }

    let bundled = bundled_debuggers(&app);
    if config.compound.is_empty() {
        let prepared = prepare_one(
            &slot,
            &workspace,
            config,
            env,
            build_configuration,
            &channel,
            bundled.as_deref(),
        )
        .await?;
        return run_prepared(slot, prepared, channel, state.running.clone(), true).await;
    }

    // Resolve and prepare every member—including .NET builds and Node command
    // validation—before any adapter is launched. A bad member therefore leaves
    // the whole compound with no debuggee running.
    let mut prepared = Vec::new();
    for id in &config.compound {
        let member = workspace
            .configs
            .iter()
            .find(|candidate| candidate.id == *id)
            .cloned()
            .ok_or_else(|| format!("compound member `{id}` no longer exists"))?;
        if member.kind != RunKind::App {
            return Err(format!(
                "{} is not an application configuration",
                member.name
            ));
        }
        let member_name = member.name.clone();
        prepared.push(
            prepare_one(
                &slot,
                &workspace,
                member,
                env.clone(),
                build_configuration.clone(),
                &channel,
                bundled.as_deref(),
            )
            .await
            .map_err(|error| format!("{member_name}: {error}"))?,
        );
    }
    let mut handles = Vec::new();
    for member in prepared {
        handles.push(tokio::spawn(run_prepared(
            slot.clone(),
            member,
            channel.clone(),
            state.running.clone(),
            false,
        )));
    }
    let mut errors = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => errors.push(error),
            Err(error) => errors.push(error.to_string()),
        }
    }
    if errors.is_empty() {
        emit_state(&channel, DebugState::Exited { code: Some(0) });
        Ok(())
    } else {
        let detail = errors.join("; ");
        emit_state(
            &channel,
            DebugState::Failed {
                detail: detail.clone(),
            },
        );
        Err(detail)
    }
}

#[tauri::command]
pub async fn stop_debug(state: State<'_, AppState>, config_id: String) -> Result<bool, String> {
    let slot = state.active_slot()?;
    let workspace = slot.workspace();
    let mut stopped = slot.debug.cancel(&config_id).await;
    stopped |= slot
        .supervisor
        .cancel(&format!("{config_id}:debug-build"))
        .await;
    if let Some(compound) = workspace
        .configs
        .iter()
        .find(|config| config.id == config_id)
    {
        for member in &compound.compound {
            stopped |= slot.debug.cancel(member).await;
            stopped |= slot
                .supervisor
                .cancel(&format!("{member}:debug-build"))
                .await;
        }
    }
    Ok(stopped)
}

#[tauri::command]
pub async fn debug_ids(
    state: State<'_, AppState>,
    root: Option<String>,
) -> Result<Vec<String>, String> {
    let slot = match root {
        Some(root) => {
            state.slot(&dunce::canonicalize(&root).unwrap_or_else(|_| PathBuf::from(root)))
        }
        None => Some(state.active_slot()?),
    };
    Ok(match slot {
        Some(slot) => {
            let mut ids = slot.debug.ids().await;
            let workspace = slot.workspace();
            for config in &workspace.configs {
                if !config.compound.is_empty()
                    && config.compound.iter().any(|member| ids.contains(member))
                {
                    ids.push(config.id.clone());
                }
            }
            ids.sort();
            ids.dedup();
            ids
        }
        None => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_core::model::ConfigSource;

    fn workspace() -> cb_core::workspace::Workspace {
        cb_core::workspace::Workspace {
            root: PathBuf::from("/repo"),
            name: "repo".into(),
            projects: Vec::new(),
            configs: Vec::new(),
            solutions: Vec::new(),
            favorites: Vec::new(),
            order: Vec::new(),
        }
    }

    #[test]
    fn node_launch_keeps_the_resolved_command_environment_and_directory() {
        let invocation = Invocation {
            program: "pnpm".into(),
            args: vec!["run".into(), "dev".into()],
            cwd: PathBuf::from("/repo/web"),
            env: BTreeMap::from([("REDIS_STREAM".into(), "debug".into())]),
            report: None,
            coverage: None,
            warnings: Vec::new(),
        };
        let config = RunConfig::new(
            "web:dev",
            "Web",
            RunKind::App,
            "node",
            ConfigSource::UserFile,
        );

        let launch = node_launch(&invocation, &config);
        assert_eq!(launch["runtimeExecutable"], "pnpm");
        assert_eq!(launch["runtimeArgs"], json!(["run", "dev"]));
        assert_eq!(launch["cwd"], "/repo/web");
        assert_eq!(launch["env"]["REDIS_STREAM"], "debug");
    }

    #[test]
    fn dotnet_launch_applies_saved_environment_and_arguments() {
        let mut config =
            RunConfig::new("api", "Api", RunKind::App, "dotnet", ConfigSource::UserFile);
        config.ignore_launch_settings = true;
        config.cwd = Some("src/Api".into());
        config.args = vec!["--stream".into(), "debug".into()];
        config.env.insert("REDIS_STREAM".into(), "debug".into());

        let launch = dotnet_launch(&workspace(), &config, Path::new("/repo/Api.dll"));
        assert_eq!(launch["program"], "/repo/Api.dll");
        assert_eq!(
            launch["cwd"].as_str().unwrap().replace('\\', "/"),
            "/repo/src/Api"
        );
        assert_eq!(launch["args"], json!(["--stream", "debug"]));
        assert_eq!(launch["env"]["REDIS_STREAM"], "debug");
    }
}
