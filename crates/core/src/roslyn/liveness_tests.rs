use super::*;

use crate::roslyn::instances::{Listener, RoslynInstance, PROTOCOL_VERSION};
use std::path::PathBuf;

fn entry(pid: u32, exe: &str) -> RoslynInstance {
    RoslynInstance {
        pid,
        exe: exe.to_string(),
        protocol: PROTOCOL_VERSION,
        listener: Listener {
            pipe: crate::roslyn::instances::pipe_name(pid),
            token: "t".to_string(),
        },
        workspaces: vec![r"C:\code\repo".to_string()],
    }
}

#[test]
fn the_same_path_matches() {
    assert!(same_executable(
        &PathBuf::from(r"C:\apps\code-basics\cb-app.exe"),
        r"C:\apps\code-basics\cb-app.exe"
    ));
}

#[test]
fn case_and_separators_do_not_make_it_a_different_executable() {
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
    assert!(!same_executable(
        &PathBuf::from(r"C:\Windows\System32\notepad.exe"),
        r"C:\apps\code-basics\cb-app.exe"
    ));
}

#[test]
fn an_empty_recorded_path_matches_nothing() {
    assert!(!same_executable(&PathBuf::from(r"C:\anything.exe"), ""));
    assert!(!same_executable(&PathBuf::from(r"C:\anything.exe"), "   "));
}

#[test]
fn a_pid_that_cannot_exist_is_not_alive() {
    // pid 0 is the System Idle Process and is never this application.
    assert!(!alive(&entry(0, r"C:\apps\code-basics\cb-app.exe")));
}

#[test]
fn this_process_is_alive_under_its_own_executable_and_not_under_another() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mine = entry(std::process::id(), &exe.display().to_string());
    assert!(alive(&mine), "this process must probe as alive");

    let impostor = RoslynInstance {
        exe: r"C:\Windows\System32\notepad.exe".to_string(),
        ..mine
    };
    assert!(
        !alive(&impostor),
        "a live pid running a different executable must not be accepted"
    );
}
