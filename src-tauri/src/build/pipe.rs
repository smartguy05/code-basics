//! The Build control pipe: the one way an agent's `mcp-build` server reaches this
//! application to run and read a workspace's build.
//!
//! Windows only, and the module is `#[cfg(windows)]` rather than internally
//! branched — every pure layer beneath it ([`cb_core::build::mcp::wire`],
//! `instances`, `tools`, `render`, `answer`) stays compiled and tested on any
//! machine, which is what keeps `cargo test -p cb-core` meaningful off Windows.
//! The `mcp-build` server itself also builds everywhere and reports *unsupported*.
//!
//! # The name carries the pid, and that is not decoration
//!
//! `\\.\pipe\code-basics.build.<pid>`. A fixed name would be wrong three ways: two
//! applications could not both bind it, the first would then answer for a workspace
//! it does not have open, and the failure would be silent. With the pid in the
//! name, two applications are two pipes and the registry reports
//! [`cb_core::build::mcp::instances::InstanceError::Ambiguous`] — which **refuses**
//! rather than picking.
//!
//! # The listener is process-global, opened once at startup
//!
//! Like the Roslyn and editor pipes (and unlike the browser pipe, tied to a
//! panel), this one lives for the whole process: a build is per-workspace but the
//! pipe is one per application, and every call resolves its own `--workspace`
//! afresh through [`AppState::slot_for_root`].
//!
//! # The feature is re-checked on every call
//!
//! Whether the `BuildMcp` feature is on is read from the features file here, per
//! call, and handed to [`agent::answer`]. Unlike the editor context there is no
//! frontend push to stop, but a build is a side effect: running one while the user
//! has switched the feature off would be worse than reading stale state, so the
//! feature flag decides before anything runs.
//!
//! # What the DACL does, and what it does not
//!
//! Identical to the Roslyn and editor pipes', and for the same reason: a named pipe
//! created with a NULL security descriptor grants read access to *Everyone* and to
//! the anonymous account. [`user_only_descriptor`] builds `D:P(A;;GA;;;<this user's
//! SID>)`. That stops **other users**, and that is *all* it stops — another program
//! running as this user can open the pipe, and the per-launch token in the registry
//! reduces that to "another program that can read this user's `%APPDATA%`". Here
//! the `--workspace` scope baked into the install is the actual boundary.
//!
//! # `first_pipe_instance` is a fail-closed check
//!
//! The first instance is created with it set, so if something on the machine has
//! already created this exact name — which it could only do by predicting this
//! process's id before this process existed — creation **fails** and no listener is
//! published.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cb_core::build::mcp::instances::Listener;
use cb_core::build::mcp::tools;
use cb_core::build::mcp::wire::{self, RequestProblem, ToolAnswer};
use cb_core::features::{self, FeatureId};
use cb_core::lsp::jsonrpc::{self, Incoming};
use cb_core::mcp::ndjson;
use serde_json::Value;
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

use super::agent;

/// The pipe this process listens on.
pub fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\code-basics.build.{pid}")
}

/// A running listener. Dropping the handle does not stop it — call [`stop`].
///
/// [`stop`]: PipeListener::stop
pub struct PipeListener {
    listener: Listener,
    running: Arc<AtomicBool>,
    acceptor: tokio::task::JoinHandle<()>,
}

impl PipeListener {
    /// What to publish in the registry.
    pub fn published(&self) -> Listener {
        self.listener.clone()
    }

    /// Stop accepting, and stop answering on connections already open.
    pub fn stop(self) {
        self.running.store(false, Ordering::SeqCst);
        self.acceptor.abort();
    }
}

/// Start listening. Returns the listener to publish, or the reason there is none.
///
/// Must be called from inside the Tokio runtime — it spawns the accept loop.
pub fn start(app: AppHandle, token: String) -> Result<PipeListener, String> {
    let name = pipe_name(std::process::id());
    // The first instance claims the name; see the module docs on
    // `first_pipe_instance`.
    let server = create_instance(&name, true)?;
    let running = Arc::new(AtomicBool::new(true));

    let acceptor = tokio::spawn(accept_loop(
        name.clone(),
        server,
        app,
        token.clone(),
        running.clone(),
    ));

    Ok(PipeListener {
        listener: Listener { pipe: name, token },
        running,
        acceptor,
    })
}

/// Accept connections until stopped.
///
/// The next instance is created **before** the connected one is handed off, the
/// standard Windows pattern: without it there is a window in which the name exists
/// with no free instance and a client's connect fails for no reason it could
/// report.
async fn accept_loop(
    name: String,
    mut server: NamedPipeServer,
    app: AppHandle,
    token: String,
    running: Arc<AtomicBool>,
) {
    while running.load(Ordering::SeqCst) {
        if let Err(error) = server.connect().await {
            eprintln!("code-basics build pipe: {error}");
            return;
        }
        if !running.load(Ordering::SeqCst) {
            return;
        }
        let connected = server;
        server = match create_instance(&name, false) {
            Ok(next) => next,
            Err(error) => {
                // Reported and then given up on, rather than looped: whatever
                // stopped a second instance being created will stop the next one
                // too, and a spin here would be a busy loop.
                eprintln!("code-basics build pipe: {error}");
                tokio::spawn(serve_connection(
                    connected,
                    app.clone(),
                    token.clone(),
                    running.clone(),
                ));
                return;
            }
        };
        tokio::spawn(serve_connection(
            connected,
            app.clone(),
            token.clone(),
            running.clone(),
        ));
    }
}

/// Answer one client until it goes away.
async fn serve_connection(
    mut pipe: NamedPipeServer,
    app: AppHandle,
    token: String,
    running: Arc<AtomicBool>,
) {
    let mut decoder = ndjson::LineDecoder::new();
    let mut chunk = [0u8; 8192];

    loop {
        let read = match pipe.read(&mut chunk).await {
            Ok(0) => return,
            Ok(count) => count,
            Err(_) => return,
        };
        // A malformed *line* is one bad message and the loop continues; an `Err`
        // is the stream itself, and where the next message begins is no longer
        // knowable. See `cb_core::mcp::ndjson`.
        let lines = match decoder.push(&chunk[..read]) {
            Ok(lines) => lines,
            Err(error) => {
                eprintln!("code-basics build pipe: {error}");
                return;
            }
        };
        for line in lines {
            if !running.load(Ordering::SeqCst) {
                return;
            }
            if let Some(reply) = answer_line(line, &app, &token).await {
                let encoded = ndjson::encode(&reply);
                if pipe.write_all(&encoded).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// The reply one line calls for, or [`None`] when it calls for none.
async fn answer_line(line: ndjson::Line, app: &AppHandle, token: &str) -> Option<Value> {
    let message = match line {
        ndjson::Line::Malformed { error, .. } => {
            return Some(malformed(&error));
        }
        ndjson::Line::Message(message) => message,
    };

    // Re-encoded so the one classifier in this tree reads it, exactly as the SQL,
    // browser, Roslyn and editor pipes do.
    let bytes = serde_json::to_vec(&message).ok()?;
    let (id, method, params) = match jsonrpc::classify(&bytes) {
        Ok(Incoming::Request { id, method, params }) => (id, method, params),
        // A notification must never be answered — a reply addressed to nothing.
        Ok(Incoming::Notification { .. }) => return None,
        // This side sends no requests, so nothing can be answering one.
        Ok(Incoming::Response { .. }) => return None,
        Err(error) => return Some(malformed(&error.to_string())),
    };
    let id = serde_json::to_value(&id).unwrap_or(Value::Null);

    let request = match wire::parse_request(&method, params.as_ref(), token) {
        Ok(request) => request,
        Err(problem) => return Some(wire::answer_value(&id, &problem.answer())),
    };

    let call = match tools::parse_call(&request.tool, request.arguments.as_object()) {
        Ok(call) => call,
        Err(error) => {
            // A tool name this build does not have, or arguments of the wrong
            // shape. Sent back as a refusal rather than a protocol error: the shim
            // is what speaks MCP, and it turns this into the tool execution error
            // the specification asks for.
            return Some(wire::answer_value(
                &id,
                &RequestProblem::Shape {
                    detail: error.message.to_string(),
                }
                .answer(),
            ));
        }
    };

    // A request that named no workspace reaches no build — the boundary is missing
    // entirely. This is the pipe-side guard; the shim refuses the same way before it
    // ever connects.
    if request.workspace.trim().is_empty() {
        return Some(wire::answer_value(
            &id,
            &ToolAnswer::refused(
                cb_core::build::mcp::answer::BuildRefusal::NoWorkspace.code(),
                cb_core::build::mcp::answer::BuildRefusal::NoWorkspace.sentence(),
            ),
        ));
    }

    // Read the feature flag fresh, then run or read the build for the request's own
    // `--workspace`. `agent::answer` turns feature-off into `Disabled`; see its
    // module docs on why the feature is re-checked here rather than trusted to the
    // frontend.
    let enabled = feature_enabled();
    let answer = agent::answer(app, &request.workspace, enabled, call).await;
    Some(wire::answer_value(&id, &answer))
}

/// Whether the Build MCP feature is switched on, read fresh from the user-global
/// features file.
fn feature_enabled() -> bool {
    features::load(&features::features_path()).is_enabled(FeatureId::BuildMcp)
}

fn malformed(detail: &str) -> Value {
    wire::answer_value(
        &Value::Null,
        &RequestProblem::Shape {
            detail: detail.to_string(),
        }
        .answer(),
    )
}

// ---------------------------------------------------------------------------
// Win32 — identical to the Roslyn and editor pipes'; a named pipe with no explicit
// DACL grants Everyone read access, so the descriptor is mandatory, not a nicety.
// ---------------------------------------------------------------------------

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// A security descriptor from SDDL, freed on drop.
struct LocalDescriptor(*mut c_void);

impl Drop for LocalDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

/// Create one instance of the pipe, with a descriptor granting only this user.
fn create_instance(name: &str, first: bool) -> Result<NamedPipeServer, String> {
    let descriptor = user_only_descriptor()?;
    let mut attributes = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };

    // SAFETY: the attributes (and the descriptor they point at) outlive the call,
    // which is the whole of tokio's requirement here.
    let server = unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .create_with_security_attributes_raw(
                name,
                &mut attributes as *mut _ as *mut std::ffi::c_void,
            )
    };
    server.map_err(|error| {
        format!(
            "the build control pipe could not be created at {name}: {error}. Agent access to the \
             build is unavailable; the application itself is unaffected."
        )
    })
}

/// `D:P(A;;GA;;;<this user's SID>)` — full access for this account, and a protected
/// DACL so nothing is inherited in.
fn user_only_descriptor() -> Result<LocalDescriptor, String> {
    let sid = current_user_sid()?;
    let sddl = format!("D:P(A;;GA;;;{sid})");
    let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out-pointer is
    // owned by `LocalDescriptor` afterwards.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 || descriptor.is_null() {
        return Err(format!(
            "the build control pipe's access rules could not be built ({}), so no pipe was \
             created. Rather than fall back to Windows' default — which grants read access to \
             Everyone — agent access to the build is unavailable.",
            std::io::Error::last_os_error()
        ));
    }
    Ok(LocalDescriptor(descriptor))
}

/// This account's SID, as a string.
fn current_user_sid() -> Result<String, String> {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(format!(
                "this process's own token could not be opened ({})",
                std::io::Error::last_os_error()
            ));
        }
        let closed = TokenHandle(token);

        let mut needed = 0u32;
        GetTokenInformation(closed.0, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            return Err("this process's own token reported no size for its user".to_string());
        }
        let mut buffer = vec![0u8; needed as usize];
        if GetTokenInformation(
            closed.0,
            TokenUser,
            buffer.as_mut_ptr() as *mut c_void,
            needed,
            &mut needed,
        ) == 0
        {
            return Err(format!(
                "this process's own user could not be read ({})",
                std::io::Error::last_os_error()
            ));
        }
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 || text.is_null() {
            return Err(format!(
                "this account's identifier could not be rendered ({})",
                std::io::Error::last_os_error()
            ));
        }
        let sid = wide_to_string(text);
        LocalFree(text as *mut c_void);
        Ok(sid)
    }
}

/// A token handle closed on drop.
struct TokenHandle(HANDLE);

impl Drop for TokenHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// A NUL-terminated wide string.
fn wide_to_string(text: *const u16) -> String {
    unsafe {
        let mut length = 0usize;
        while *text.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
    }
}

#[cfg(test)]
#[path = "pipe_tests.rs"]
mod tests;
