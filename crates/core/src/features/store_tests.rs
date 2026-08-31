use super::*;

use std::collections::BTreeMap;
use std::fs;

use crate::features::FeatureId;

/// A temp file path of this test's own, cleared before use so a leftover from a
/// failed run does not leak in.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cb-features-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("features.json")
}

fn file_with(pairs: &[(&str, bool)]) -> FeaturesFile {
    let mut file = FeaturesFile::default();
    file.enabled = pairs
        .iter()
        .map(|(id, on)| ((*id).to_string(), *on))
        .collect::<BTreeMap<_, _>>();
    file
}

#[test]
fn a_missing_file_loads_as_the_defaults() {
    let path = scratch("missing").with_file_name("does-not-exist.json");
    assert_eq!(load(&path), FeaturesFile::default());
}

#[test]
fn a_corrupt_file_loads_as_the_defaults_rather_than_erroring() {
    let path = scratch("corrupt");
    fs::write(&path, "not json at all {{{").unwrap();
    assert_eq!(load(&path), FeaturesFile::default());
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn load_existing_tells_no_store_apart_from_an_empty_one() {
    let path = scratch("existing");
    assert_eq!(load_existing(&path), None, "no file yet");

    save(&path, &FeaturesFile::default()).unwrap();
    assert_eq!(
        load_existing(&path),
        Some(FeaturesFile::default()),
        "a written-but-empty store is not the same as no store"
    );
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_then_load_round_trips() {
    let path = scratch("round-trip");
    let file = file_with(&[("sqlConsole", false), ("askCodebase", true)]);
    save(&path, &file).unwrap();
    assert_eq!(load(&path), file);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn an_unknown_feature_id_survives_a_round_trip() {
    // A downgrade must not silently discard a choice a later build made, or the
    // next upgrade re-enables something the user had turned off.
    let path = scratch("unknown-id");
    let file = file_with(&[("sqlConsole", false), ("timeMachine", false)]);
    save(&path, &file).unwrap();

    let loaded = load(&path);
    assert_eq!(loaded.enabled.get("timeMachine"), Some(&false));
    assert!(!loaded.is_enabled(FeatureId::SqlConsole));
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_leaves_no_temp_file_behind() {
    let path = scratch("no-temp");
    save(&path, &file_with(&[("sqlConsole", true)])).unwrap();
    let left: Vec<String> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["features.json".to_string()]);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn save_creates_the_parent_directory() {
    let path = scratch("parent")
        .with_file_name("nested")
        .join("features.json");
    save(&path, &FeaturesFile::default()).unwrap();
    assert!(path.exists());
    let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

// --- the seed rule ---------------------------------------------------------

#[test]
fn a_seed_applies_when_there_is_no_user_file() {
    let seed = file_with(&[("sqlConsole", false)]);
    let merged = merge_seed(None, seed.clone());
    assert_eq!(merged, seed);
    assert!(!merged.is_enabled(FeatureId::SqlConsole));
}

#[test]
fn a_seed_is_ignored_once_the_user_has_a_file() {
    // The bug this prevents: a repair install re-enabling a feature the user
    // deliberately turned off.
    let user = file_with(&[("sqlConsole", false)]);
    let seed = file_with(&[("sqlConsole", true), ("askCodebase", true)]);

    let merged = merge_seed(Some(user.clone()), seed);
    assert_eq!(merged, user, "the user's file wins whole");
    assert!(!merged.is_enabled(FeatureId::SqlConsole));
}

#[test]
fn an_empty_user_file_still_counts_as_a_user_file() {
    // Someone who opened the picker and changed nothing has still decided.
    let merged = merge_seed(
        Some(FeaturesFile::default()),
        file_with(&[("sqlConsole", false)]),
    );
    assert!(merged.enabled.is_empty(), "the seed did not leak in");
    assert!(
        merged.is_enabled(FeatureId::SqlConsole),
        "so the default applies"
    );
}

// --- where an installer leaves its seed ------------------------------------

#[test]
fn on_windows_the_seed_sits_beside_the_executable() {
    let dir = Path::new("C:/Program Files/code-basics");
    assert_eq!(
        seed_path_for(dir, Platform::Windows),
        Some(dir.join("features.json")),
        "the NSIS page writes the seed into $INSTDIR"
    );
}

#[test]
fn on_linux_the_seed_is_a_shared_path_not_the_exe_directory() {
    // /usr/bin is not a place to write app data, and an AppImage's mount point
    // is read-only and different on every launch.
    let seed = seed_path_for(Path::new("/usr/bin"), Platform::Linux).unwrap();
    assert_eq!(seed, PathBuf::from("/usr/share/code-basics/features.json"));
}

#[test]
fn a_platform_with_no_feature_installer_has_no_seed_path() {
    assert_eq!(
        seed_path_for(Path::new("/Applications"), Platform::Other),
        None
    );
}

// --- applying the seed on first run ----------------------------------------

#[test]
fn first_run_adopts_the_seed_and_writes_it_through() {
    let path = scratch("seed-adopt");
    let seed = path.with_file_name("seed.json");
    fs::write(&seed, r#"{"version":1,"enabled":{"sqlConsole":false}}"#).unwrap();

    let features = ensure_seeded(&path, Some(&seed)).unwrap();
    assert!(!features.is_enabled(FeatureId::SqlConsole));
    assert!(
        path.exists(),
        "the seed is written through, so the next run needs no installer file"
    );
    assert_eq!(load(&path), features);
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_second_run_ignores_the_seed() {
    let path = scratch("seed-second-run");
    let seed = path.with_file_name("seed.json");
    fs::write(&seed, r#"{"version":1,"enabled":{"sqlConsole":true}}"#).unwrap();

    save(&path, &file_with(&[("sqlConsole", false)])).unwrap();

    let features = ensure_seeded(&path, Some(&seed)).unwrap();
    assert!(
        !features.is_enabled(FeatureId::SqlConsole),
        "the user's own store wins over a reinstall's seed"
    );
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn no_seed_at_all_yields_the_defaults_and_writes_nothing() {
    let path = scratch("seed-none");
    let features = ensure_seeded(&path, None).unwrap();
    assert_eq!(features, FeaturesFile::default());
    assert!(
        !path.exists(),
        "an ordinary launch must not create a file it has nothing to say in"
    );
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_corrupt_seed_is_ignored_rather_than_failing_the_launch() {
    let path = scratch("seed-corrupt");
    let seed = path.with_file_name("seed.json");
    fs::write(&seed, "}{ not json").unwrap();

    let features = ensure_seeded(&path, Some(&seed)).unwrap();
    assert_eq!(features, FeaturesFile::default());
    assert!(!path.exists(), "a bad seed is not adopted");
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_seed_path_that_does_not_exist_is_not_an_error() {
    // The normal case for a dev checkout: the platform has a seed convention,
    // but nothing was installed there.
    let path = scratch("seed-absent");
    let seed = path.with_file_name("nothing-here.json");
    assert_eq!(
        ensure_seeded(&path, Some(&seed)).unwrap(),
        FeaturesFile::default()
    );
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

// --- the installer -> app contract -----------------------------------------
//
// Everything above tests the store against JSON this file wrote itself. These
// tests instead read the *packaging* artefacts off disk and check that what an
// installer actually emits is what `FeaturesFile` actually parses.
//
// They exist because `load` is deliberately tolerant: a seed with a typo'd key,
// a renamed feature id or a wrong destination path is not an error anywhere --
// it degrades silently to the defaults, and the user's installer choice
// vanishes with no diagnostic. That tolerance is right (a bad preferences file
// must never stop the app starting) and is exactly why the contract has to be
// pinned somewhere else.
//
// What they do NOT establish: that any installer was ever built. `pnpm tauri
// build` is not run here. These pin the contract, not the artefact.

/// The repository root, derived from this crate's manifest directory so the
/// tests find the packaging files wherever the checkout lives.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn read_repo_file(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

const NSI_PATH: &str = "src-tauri/installer/windows/installer.nsi";
const LINUX_SEED_PATH: &str = "src-tauri/resources/linux/features.json";
const TAURI_CONF_PATH: &str = "src-tauri/tauri.conf.json";

/// The NSIS variable holding the page's answer for `feature`.
///
/// Derived from the stable id rather than looked up in a table, so a feature
/// added to `FeatureId::ALL` without a matching checkbox fails here -- the
/// variable will not be declared and the seed will not mention it -- instead of
/// shipping a switch nobody can reach.
fn nsis_var(feature: FeatureId) -> String {
    let mut chars = feature.id().chars();
    let head = chars.next().expect("a feature id is never empty");
    format!("$Feature{}{}", head.to_ascii_uppercase(), chars.as_str())
}

/// The lines of `Function <name>` in an NSIS script, exclusive of its header
/// and its `FunctionEnd`.
fn nsis_function<'a>(script: &'a str, name: &str) -> Vec<&'a str> {
    let header = format!("Function {name}");
    let lines = script.lines().skip_while(|l| l.trim() != header).skip(1);
    let mut body = Vec::new();
    for line in lines {
        if line.trim() == "FunctionEnd" {
            return body;
        }
        body.push(line);
    }
    panic!("{name} not found (or unterminated) in the NSIS script");
}

/// The single-quoted literal on a `FileWrite $9 '...'` line.
fn nsis_literal(rest: &str) -> &str {
    let start = rest
        .find('\'')
        .unwrap_or_else(|| panic!("FileWrite argument is not a single-quoted literal: {rest}"));
    let end = rest
        .rfind('\'')
        .expect("a literal that opened must also close");
    assert!(end > start, "empty FileWrite literal: {rest}");
    &rest[start + 1..end]
}

/// Replay `Function WriteFeaturesSeed` for one set of checkbox answers and
/// return the exact bytes it writes.
///
/// This interprets the script rather than restating the JSON we hope it emits,
/// so a typo in any `FileWrite` literal changes the result and the parse below
/// is the thing that catches it.
fn nsis_seed_json(script: &str, choices: &[(FeatureId, bool)]) -> String {
    let values: BTreeMap<String, &'static str> = choices
        .iter()
        .map(|(f, on)| (nsis_var(*f), if *on { "1" } else { "0" }))
        .collect();

    let mut out = String::new();
    let mut emitting: Vec<bool> = Vec::new();
    let mut started = false;

    for line in nsis_function(script, "WriteFeaturesSeed") {
        let trimmed = line.trim();
        // Everything before the first FileWrite is the FileOpen error guard,
        // whose If/EndIf is balanced and closed before we start.
        if !started && !trimmed.starts_with("FileWrite") {
            continue;
        }
        started = true;

        if let Some(cond) = trimmed.strip_prefix("${If} ") {
            let parts: Vec<&str> = cond.split_whitespace().collect();
            assert_eq!(
                parts.len(),
                3,
                "unhandled NSIS condition in WriteFeaturesSeed: {trimmed}"
            );
            assert_eq!(parts[1], "==", "unhandled comparison: {trimmed}");
            let have = values.get(parts[0]).unwrap_or_else(|| {
                panic!(
                    "WriteFeaturesSeed branches on {}, which is no FeatureId's page variable",
                    parts[0]
                )
            });
            emitting.push(*have == parts[2].trim_matches('"'));
        } else if trimmed == "${Else}" {
            let branch = emitting.last_mut().expect("Else outside an If");
            *branch = !*branch;
        } else if trimmed == "${EndIf}" {
            emitting.pop().expect("EndIf outside an If");
        } else if let Some(rest) = trimmed.strip_prefix("FileWrite $9 ") {
            if emitting.iter().all(|b| *b) {
                out.push_str(nsis_literal(rest));
            }
        }
    }

    assert!(emitting.is_empty(), "unbalanced If in WriteFeaturesSeed");
    out
}

/// Every combination of the two checkboxes, so neither an inverted branch nor a
/// missing comma can hide behind the all-on case.
fn every_choice() -> Vec<(bool, bool)> {
    vec![(true, true), (false, false), (true, false), (false, true)]
}

#[test]
fn the_windows_installer_writes_json_the_app_can_parse() {
    let nsi = read_repo_file(NSI_PATH);

    for (sql, ask) in every_choice() {
        let json = nsis_seed_json(
            &nsi,
            &[(FeatureId::SqlConsole, sql), (FeatureId::AskCodebase, ask)],
        );

        let parsed: FeaturesFile = serde_json::from_str(&json).unwrap_or_else(|e| {
            panic!("the NSIS page writes {json:?}, which FeaturesFile cannot parse: {e}")
        });

        assert_eq!(parsed.version, 1, "seed {json}");
        assert_eq!(
            parsed.is_enabled(FeatureId::SqlConsole),
            sql,
            "SQL console state lost or inverted in {json}"
        );
        assert_eq!(
            parsed.is_enabled(FeatureId::AskCodebase),
            ask,
            "Ask the codebase state lost or inverted in {json}"
        );
        assert_eq!(
            parsed.enabled.len(),
            FeatureId::ALL.len(),
            "the seed records exactly the known features: {json}"
        );
    }
}

#[test]
fn first_launch_adopts_the_bytes_the_windows_installer_writes() {
    // The parse above proves the JSON is readable; this proves the whole path
    // the installer's file actually takes -- adopted once, then written through.
    let nsi = read_repo_file(NSI_PATH);
    let dir = scratch("nsis-adopt");

    for (i, (sql, ask)) in every_choice().into_iter().enumerate() {
        let store = dir.with_file_name(format!("store-{i}.json"));
        let seed = dir.with_file_name(format!("seed-{i}.json"));
        fs::write(
            &seed,
            nsis_seed_json(
                &nsi,
                &[(FeatureId::SqlConsole, sql), (FeatureId::AskCodebase, ask)],
            ),
        )
        .unwrap();

        let features = ensure_seeded(&store, Some(&seed)).unwrap();
        assert_eq!(features.is_enabled(FeatureId::SqlConsole), sql);
        assert_eq!(features.is_enabled(FeatureId::AskCodebase), ask);
        assert_eq!(load(&store), features, "the seed was written through");
    }

    let _ = fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn the_shipped_linux_seed_parses_and_is_all_on() {
    // Read the real file rather than a copy of its text: the .deb ships this
    // exact byte sequence, and nothing else would notice it drifting.
    let text = read_repo_file(LINUX_SEED_PATH);
    let parsed: FeaturesFile = serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("{LINUX_SEED_PATH} is not a FeaturesFile: {e}\ncontents: {text}")
    });

    assert_eq!(parsed.version, 1);
    for feature in FeatureId::ALL {
        assert!(
            parsed.is_enabled(feature),
            "a .deb cannot ask the question, so its seed must be the defaults; {} is off",
            feature.id()
        );
    }

    // And it survives the same adoption path as the Windows seed.
    let store = scratch("linux-seed");
    let seed = store.with_file_name("shipped.json");
    fs::write(&seed, &text).unwrap();
    assert_eq!(ensure_seeded(&store, Some(&seed)).unwrap(), parsed);
    let _ = fs::remove_dir_all(store.parent().unwrap());
}

#[test]
fn the_windows_seed_path_matches_what_the_nsis_opens() {
    // A silently-wrong path is the likeliest failure of the whole scheme: the
    // installer writes a real file, the app reads a real default, and nothing
    // anywhere reports a mismatch.
    let nsi = read_repo_file(NSI_PATH);
    let open = nsis_function(&nsi, "WriteFeaturesSeed")
        .into_iter()
        .map(str::trim)
        .find(|l| l.starts_with("FileOpen $9 "))
        .expect("WriteFeaturesSeed opens a file");

    let quoted = open
        .split('"')
        .nth(1)
        .expect("the FileOpen target is a quoted path");
    assert_eq!(
        quoted,
        format!("$INSTDIR\\{SEED_FILE}"),
        "the NSIS target and features::store::SEED_FILE have diverged"
    );

    let exe_dir = Path::new("C:/Program Files/code-basics");
    assert_eq!(
        seed_path_for(exe_dir, Platform::Windows),
        Some(exe_dir.join(SEED_FILE)),
        "$INSTDIR is where the main binary lands, so the seed sits beside the exe"
    );
}

#[test]
fn the_linux_seed_path_matches_the_deb_files_mapping() {
    let conf: serde_json::Value =
        serde_json::from_str(&read_repo_file(TAURI_CONF_PATH)).expect("tauri.conf.json is JSON");
    let files = conf["bundle"]["linux"]["deb"]["files"]
        .as_object()
        .expect("bundle.linux.deb.files declares the seed");

    let expected = seed_path_for(Path::new("/usr/bin"), Platform::Linux)
        .expect("Linux has a seed convention")
        .to_string_lossy()
        .replace('\\', "/");

    let source = files.get(&expected).unwrap_or_else(|| {
        panic!(
            "the .deb installs no file at {expected}; it installs {:?}",
            files.keys().collect::<Vec<_>>()
        )
    });
    assert_eq!(
        source.as_str(),
        Some("resources/linux/features.json"),
        "the .deb must ship the seed these tests read"
    );

    // A deb `files` source is relative to src-tauri/.
    assert!(
        repo_root()
            .join("src-tauri")
            .join(source.as_str().unwrap())
            .is_file(),
        "the declared source file does not exist"
    );
}

#[test]
fn every_feature_appears_in_both_installers() {
    // Adding a third feature without touching the installers should fail here,
    // rather than shipping a checkbox nobody can see and a Linux seed that
    // silently says nothing about it.
    let nsi = read_repo_file(NSI_PATH);
    let all_on: Vec<(FeatureId, bool)> = FeatureId::ALL.into_iter().map(|f| (f, true)).collect();
    let windows_seed: FeaturesFile =
        serde_json::from_str(&nsis_seed_json(&nsi, &all_on)).expect("the NSIS seed parses");
    let linux_seed: FeaturesFile =
        serde_json::from_str(&read_repo_file(LINUX_SEED_PATH)).expect("the Linux seed parses");

    for feature in FeatureId::ALL {
        let id = feature.id();
        assert!(
            windows_seed.enabled.contains_key(id),
            "the NSIS seed never mentions {id}"
        );
        assert!(
            linux_seed.enabled.contains_key(id),
            "{LINUX_SEED_PATH} never mentions {id}"
        );

        // The page must be able to answer for it, and must describe it the way
        // the in-app picker does.
        let var = nsis_var(feature);
        assert!(
            nsi.lines()
                .any(|l| l.trim() == format!("Var {}", &var[1..])),
            "the NSIS script declares no {var} for {id}"
        );
        assert!(
            nsi.contains(&format!("\"{}\"", feature.label())),
            "the installer page does not use the label {:?}",
            feature.label()
        );
        assert!(
            nsi.contains(&format!("\"{}\"", feature.description())),
            "the installer page does not use the description {:?}",
            feature.description()
        );
    }

    // And nothing extra: an id in a seed that this build cannot toggle is a
    // choice the user can make and never unmake from the picker.
    let known: Vec<&str> = FeatureId::ALL.iter().map(|f| f.id()).collect();
    for id in windows_seed.enabled.keys().chain(linux_seed.enabled.keys()) {
        assert!(
            known.contains(&id.as_str()),
            "a seed offers {id}, which is not a FeatureId"
        );
    }
}
