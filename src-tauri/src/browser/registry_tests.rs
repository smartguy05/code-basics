use super::*;

use cb_core::browser::liveness;
use std::sync::{Mutex, MutexGuard};

/// The registry path is a process-global environment variable, so the tests
/// that redirect it take turns.
static REDIRECT: Mutex<()> = Mutex::new(());

struct Redirected {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    previous: Option<std::ffi::OsString>,
}

impl Redirected {
    fn new() -> Self {
        let guard = REDIRECT.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CB_BROWSER_INSTANCES_PATH");
        std::env::set_var(
            "CB_BROWSER_INSTANCES_PATH",
            dir.path().join("browser-instances.json"),
        );
        Self {
            _guard: guard,
            _dir: dir,
            previous,
        }
    }
}

impl Drop for Redirected {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("CB_BROWSER_INSTANCES_PATH", value),
            None => std::env::remove_var("CB_BROWSER_INSTANCES_PATH"),
        }
    }
}

/// A listener for pid 1234, for the cases that publish a *fabricated* entry.
fn listener() -> Listener {
    listener_for(1234)
}

/// A listener whose pipe name is the one `pid` implies.
///
/// The pipe name is a function of the pid (`instances::pipe_name`), and
/// `choose_instance` refuses an entry whose stated name disagrees — that is how
/// a forged registry entry is caught. So any test that publishes an entry for a
/// *real* pid and then expects it to resolve has to build the listener for that
/// same pid; pairing a real pid with a hard-coded 1234 pipe is precisely the
/// tampering shape, and the refusal is correct.
fn listener_for(pid: u32) -> Listener {
    Listener {
        pipe: instances::pipe_name(pid),
        token: "a".repeat(64),
    }
}

// ---------------------------------------------------------------------------
// The token
// ---------------------------------------------------------------------------

#[test]
fn a_token_is_sixty_four_hex_characters_and_never_the_same_twice() {
    let first = mint_token();
    let second = mint_token();
    assert_eq!(first.len(), 64, "{first}");
    assert!(first.chars().all(|c| c.is_ascii_hexdigit()), "{first}");
    assert_ne!(first, second);
}

#[test]
fn a_token_is_not_derived_from_anything_a_prober_can_compute() {
    // The pid, the executable path and the time are all readable by any local
    // process, so a token containing them would be no barrier at all.
    let token = mint_token();
    assert!(!token.contains(&std::process::id().to_string()), "{token}");
}

// ---------------------------------------------------------------------------
// Composing the entry
// ---------------------------------------------------------------------------

#[test]
fn an_open_panel_publishes_its_pipe() {
    let entry = instance_for(
        1234,
        r"C:\apps\cb-app.exe".to_string(),
        true,
        Some(listener()),
        vec![r"C:\code\repo".to_string()],
    );
    assert_eq!(entry.pid, 1234);
    assert_eq!(entry.protocol, PROTOCOL_VERSION);
    assert!(entry.browser_feature);
    assert_eq!(entry.listener, Some(listener()));
    assert_eq!(entry.workspaces, vec![r"C:\code\repo".to_string()]);
}

#[test]
fn the_plugin_being_off_drops_the_listener_structurally() {
    // The two facts arrive from different places and a caller could pass a
    // stale pair. An entry advertising a pipe while claiming the browser is off
    // would make the two states indistinguishable, which is the one thing this
    // file exists to prevent.
    let entry = instance_for(
        1234,
        r"C:\apps\cb-app.exe".to_string(),
        false,
        Some(listener()),
        Vec::new(),
    );
    assert!(!entry.browser_feature);
    assert_eq!(entry.listener, None);
}

#[test]
fn the_plugin_on_with_no_panel_publishes_no_listener_and_that_is_the_panel_closed_answer() {
    let entry = instance_for(1234, "x".to_string(), true, None, Vec::new());
    assert!(entry.browser_feature);
    assert_eq!(entry.listener, None);

    // And that shape really does read back as PanelClosed rather than as the
    // plugin being off — the distinction the whole entry exists for.
    let file = instances::InstancesFile {
        version: 1,
        instances: vec![entry],
    };
    let error = instances::choose_instance(&file, None, &|_| true).unwrap_err();
    assert_eq!(error, instances::InstanceError::PanelClosed { pid: 1234 });
}

// ---------------------------------------------------------------------------
// Publish and withdraw
// ---------------------------------------------------------------------------

#[test]
fn publishing_then_choosing_finds_this_application() {
    let _redirect = Redirected::new();
    let entry = instance_for(
        std::process::id(),
        own_exe(),
        true,
        Some(listener_for(std::process::id())),
        Vec::new(),
    );
    publish(entry.clone()).unwrap();

    // Chosen with the **real** liveness probe: this process is running, and its
    // recorded executable is its own.
    let file = instances::load(&instances::instances_path());
    let chosen = instances::choose_instance(&file, None, &liveness::alive).unwrap();
    assert_eq!(chosen.pid, std::process::id());
    assert_eq!(chosen.listener, Some(listener_for(std::process::id())));
}

#[test]
fn republishing_replaces_this_applications_entry_rather_than_adding_one() {
    let _redirect = Redirected::new();
    let pid = std::process::id();
    publish(instance_for(pid, own_exe(), true, None, Vec::new())).unwrap();
    publish(instance_for(
        pid,
        own_exe(),
        true,
        Some(listener()),
        Vec::new(),
    ))
    .unwrap();

    let file = instances::load(&instances::instances_path());
    assert_eq!(file.instances.len(), 1);
    assert!(file.instances[0].listener.is_some());
}

#[test]
fn publishing_keeps_another_applications_entry() {
    // The file is user-global and every running application writes to it. A
    // whole-file write would silently unpublish somebody else's window.
    let _redirect = Redirected::new();
    let path = instances::instances_path();
    let stranger = instance_for(
        999_999,
        r"C:\other\cb-app.exe".to_string(),
        true,
        None,
        Vec::new(),
    );
    instances::save(
        &path,
        &instances::InstancesFile {
            version: 1,
            instances: vec![stranger.clone()],
        },
    )
    .unwrap();

    publish(instance_for(
        std::process::id(),
        own_exe(),
        true,
        None,
        Vec::new(),
    ))
    .unwrap();
    let file = instances::load(&path);
    assert_eq!(file.instances.len(), 2);
    assert!(file.instances.contains(&stranger));
}

#[test]
fn withdrawing_removes_only_this_application() {
    let _redirect = Redirected::new();
    let path = instances::instances_path();
    let stranger = instance_for(
        999_999,
        r"C:\other\cb-app.exe".to_string(),
        true,
        None,
        Vec::new(),
    );
    publish(stranger.clone()).unwrap();
    publish(instance_for(
        std::process::id(),
        own_exe(),
        true,
        None,
        Vec::new(),
    ))
    .unwrap();

    withdraw(std::process::id()).unwrap();
    let file = instances::load(&path);
    assert_eq!(file.instances, vec![stranger]);
}

#[test]
fn withdrawing_when_nothing_was_published_is_not_an_error() {
    // Called on shutdown regardless, including after a failed publish.
    let _redirect = Redirected::new();
    withdraw(std::process::id()).unwrap();
}

#[test]
fn a_corrupt_registry_does_not_stop_this_application_publishing() {
    // The tolerance rule, reached from this side: a damaged file must not make
    // the browser panel unable to announce itself.
    let _redirect = Redirected::new();
    let path = instances::instances_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ not json at all").unwrap();

    publish(instance_for(
        std::process::id(),
        own_exe(),
        true,
        Some(listener()),
        Vec::new(),
    ))
    .unwrap();
    let file = instances::load(&path);
    assert_eq!(file.instances.len(), 1);
}

#[test]
fn an_executable_that_cannot_be_identified_is_never_chosen() {
    // `own_exe` answers with an empty string rather than a guess, and an entry
    // that could not identify itself must not be driven.
    let _redirect = Redirected::new();
    publish(instance_for(
        std::process::id(),
        String::new(),
        true,
        Some(listener()),
        Vec::new(),
    ))
    .unwrap();
    let file = instances::load(&instances::instances_path());
    let error = instances::choose_instance(&file, None, &liveness::alive).unwrap_err();
    assert_eq!(error, instances::InstanceError::NoneRunning { hint: None });
}
