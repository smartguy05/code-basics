use super::*;

use crate::browser::instances::{BrowserInstance, PROTOCOL_VERSION};
use std::path::PathBuf;

#[test]
fn the_same_path_matches() {
    assert!(same_executable(
        &PathBuf::from(r"C:\apps\code-basics\cb-app.exe"),
        r"C:\apps\code-basics\cb-app.exe"
    ));
}

#[test]
fn case_and_separators_do_not_make_it_a_different_executable() {
    // Windows paths are case-insensitive, and an entry written one way must
    // match a probe reporting it another.
    assert!(same_executable(
        &PathBuf::from(r"c:\apps\code-basics\cb-app.exe"),
        r"C:\Apps\Code-Basics\CB-App.exe"
    ));
    assert!(same_executable(
        &PathBuf::from("C:/apps/cb-app.exe"),
        r"C:\apps\cb-app.exe"
    ));
}

#[test]
fn a_different_executable_at_the_same_pid_does_not_match() {
    // The recycled-pid case this function exists for.
    assert!(!same_executable(
        &PathBuf::from(r"C:\Windows\System32\notepad.exe"),
        r"C:\apps\code-basics\cb-app.exe"
    ));
}

#[test]
fn a_different_install_of_the_same_application_does_not_match() {
    // Two copies of code-basics is an ordinary situation (an installed build
    // and a `cargo run` one), and they are different applications with
    // different panels.
    assert!(!same_executable(
        &PathBuf::from(r"C:\code\code-basics\target\debug\cb-app.exe"),
        r"C:\Program Files\code-basics\cb-app.exe"
    ));
}

#[test]
fn an_empty_recorded_path_matches_nothing() {
    // Otherwise an entry that failed to record its executable would match
    // whatever process later took its pid.
    assert!(!same_executable(&PathBuf::from(r"C:\anything.exe"), ""));
    assert!(!same_executable(&PathBuf::from(r"C:\anything.exe"), "   "));
}

#[test]
fn a_pid_that_cannot_exist_is_not_alive() {
    // pid 0 is the System Idle Process and is never this application.
    let entry = BrowserInstance {
        pid: 0,
        exe: r"C:\apps\code-basics\cb-app.exe".to_string(),
        protocol: PROTOCOL_VERSION,
        browser_feature: true,
        listener: None,
        workspaces: Vec::new(),
    };
    assert!(!alive(&entry));
}

#[test]
fn this_process_is_alive_under_its_own_executable_and_not_under_another() {
    // The only end-to-end check available without launching something: this
    // test binary really is running, and really is not `notepad.exe`.
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mine = BrowserInstance {
        pid: std::process::id(),
        exe: exe.display().to_string(),
        protocol: PROTOCOL_VERSION,
        browser_feature: true,
        listener: None,
        workspaces: Vec::new(),
    };
    assert!(alive(&mine), "this process must probe as alive");

    let impostor = BrowserInstance {
        exe: r"C:\Windows\System32\notepad.exe".to_string(),
        ..mine
    };
    assert!(
        !alive(&impostor),
        "a live pid running a different executable must not be accepted"
    );
}
