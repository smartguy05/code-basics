//! The Roslyn control pipe: the one way an agent's `mcp-roslyn` server reaches
//! this application's warm language-server session.
//!
//! Windows only, and the module is `#[cfg(windows)]` rather than internally
//! branched — every pure layer beneath it ([`cb_core::roslyn::wire`],
//! `instances`, `tools`, `render`, `symbol`) stays compiled and tested on any
//! machine, which is what keeps `cargo test -p cb-core` meaningful off Windows.
//! The `mcp-roslyn` server itself also builds everywhere and reports
//! *unsupported*.
//!
//! # The name carries the pid, and that is not decoration
//!
//! `\\.\pipe\code-basics.roslyn.<pid>`. A fixed name would be wrong three ways:
//! two applications could not both bind it, the first would then answer for a
//! workspace it does not have open, and the failure would be silent. With the pid
//! in the name, two applications are two pipes and the registry reports
//! [`cb_core::roslyn::instances::InstanceError::Ambiguous`] — which **refuses**
//! rather than picking.
//!
//! # The listener is process-global, opened once at startup
//!
//! Unlike the browser pipe (tied to an open panel), this one lives for the whole
//! process: the semantic model is per-workspace but the pipe is one per
//! application, and every call resolves its own `--workspace` afresh through
//! [`AppState::lsp_for_root`].
//!
//! # What the DACL does, and what it does not
//!
//! Identical to the browser pipe's, and for the same reason: a named pipe created
//! with a NULL security descriptor grants read access to *Everyone* and to the
//! anonymous account. [`user_only_descriptor`] builds `D:P(A;;GA;;;<this user's
//! SID>)`. That stops **other users**, and that is *all* it stops — another
//! program running as this user can open the pipe, and the per-launch token in the
//! registry reduces that to "another program that can read this user's
//! `%APPDATA%`". Here the `--workspace` scope baked into the install is the actual
//! boundary; there is no consent gate to grant, because the questions are
//! read-only over an already-open local repository.
//!
//! # `first_pipe_instance` is a fail-closed check
//!
//! The first instance is created with it set, so if something on the machine has
//! already created this exact name — which it could only do by predicting this
//! process's id before this process existed — creation **fails** and no listener
//! is published.

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cb_core::lsp::jsonrpc::{self, Incoming};
use cb_core::mcp::ndjson;
use cb_core::roslyn::instances::Listener;
use cb_core::roslyn::tools;
use cb_core::roslyn::wire::{self, RequestProblem};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

use super::agent::{self, Peer};
use crate::state::AppState;

/// The pipe this process listens on.
pub fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\code-basics.roslyn.{pid}")
}

/// A running listener. Dropping the handle does not stop it — call [`stop`].
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
            eprintln!("code-basics roslyn pipe: {error}");
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
                eprintln!("code-basics roslyn pipe: {error}");
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
    let peer = describe_peer(&pipe);
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
                eprintln!("code-basics roslyn pipe: {error}");
                return;
            }
        };
        for line in lines {
            if !running.load(Ordering::SeqCst) {
                return;
            }
            if let Some(reply) = answer_line(line, &app, &token, &peer).await {
                let encoded = ndjson::encode(&reply);
                if pipe.write_all(&encoded).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// The reply one line calls for, or [`None`] when it calls for none.
async fn answer_line(
    line: ndjson::Line,
    app: &AppHandle,
    token: &str,
    peer: &Peer,
) -> Option<Value> {
    let message = match line {
        ndjson::Line::Malformed { error, .. } => {
            return Some(malformed(&error));
        }
        ndjson::Line::Message(message) => message,
    };

    // Re-encoded so the one classifier in this tree reads it, exactly as
    // `mcp_sql` and the browser pipe do.
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

    // Resolve the session for the request's `--workspace`, per call. The `State`
    // guard is dropped before the await below — the handle is an owned clone.
    let handle = {
        let state = app.state::<AppState>();
        state.lsp_for_root(Path::new(&request.workspace))
    };
    let answer = agent::answer(handle, call, peer).await;
    Some(wire::answer_value(&id, &answer))
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

/// Who is on the other end, as the OS reports it.
fn describe_peer(pipe: &NamedPipeServer) -> Peer {
    let handle = pipe.as_raw_handle();
    let pid = client_process_id(handle);
    let program = pid.and_then(process_image_name);
    peer_label(pid, program)
}

/// Turn what the OS could say about the client into a label for a future banner.
///
/// Abstains rather than inventing: an unidentifiable caller is reported as
/// unidentified. The pure half, so the abstention is testable.
pub fn peer_label(pid: Option<u32>, program: Option<String>) -> Peer {
    Peer {
        pid: pid.unwrap_or(0),
        program: program
            .map(|path| {
                std::path::Path::new(&path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or(path)
            })
            .unwrap_or_else(|| "an unidentified program".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Win32 — identical to the browser pipe's; a named pipe with no explicit DACL
// grants Everyone read access, so the descriptor is mandatory, not a nicety.
// ---------------------------------------------------------------------------

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

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
            "the roslyn control pipe could not be created at {name}: {error}. Agent access to the \
             language server is unavailable; the application itself is unaffected."
        )
    })
}

/// `D:P(A;;GA;;;<this user's SID>)` — full access for this account, and a
/// protected DACL so nothing is inherited in.
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
            "the roslyn control pipe's access rules could not be built ({}), so no pipe was \
             created. Rather than fall back to Windows' default — which grants read access to \
             Everyone — agent access to the language server is unavailable.",
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

/// The pid on the other end of a connected pipe.
fn client_process_id(pipe: *mut c_void) -> Option<u32> {
    let mut pid = 0u32;
    // SAFETY: `pipe` is a connected pipe handle owned by the caller.
    let ok = unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) };
    (ok != 0 && pid != 0).then_some(pid)
}

/// A process's executable path.
fn process_image_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let owned = TokenHandle(handle);
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(owned.0, 0, buffer.as_mut_ptr(), &mut length);
        if ok == 0 || length == 0 {
            return None;
        }
        buffer.truncate(length as usize);
        Some(String::from_utf16_lossy(&buffer))
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
