//! Tests for the Cargo adapter.

use super::cargo::*;

fn manifest(text: &str) -> CargoManifest {
    parse(text).expect("valid Cargo.toml")
}

fn path_dep<'a>(m: &'a CargoManifest, name: &str) -> &'a PathDependency {
    m.path_dependencies
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no path dependency named {name} in {:?}",
                m.path_dependencies
            )
        })
}

// ---------------------------------------------------------------------------
// Packages and workspaces
// ---------------------------------------------------------------------------

#[test]
fn reads_the_package_name_from_a_plain_crate_manifest() {
    let m = manifest(
        r#"
        [package]
        name = "cb-core"
        version = "0.1.0"
        edition = "2021"
        "#,
    );

    assert_eq!(m.package_name.as_deref(), Some("cb-core"));
    assert!(!m.is_workspace_root);
    assert!(!m.is_virtual_manifest());
    assert!(m.workspace_members.is_empty());
}

#[test]
fn a_virtual_manifest_is_a_workspace_root_and_not_a_package() {
    // This repository's own root Cargo.toml, verbatim at the time of writing.
    let m = manifest(
        r#"
        [workspace]
        members = ["crates/core", "src-tauri"]
        resolver = "2"

        [workspace.package]
        version = "0.1.0"
        edition = "2021"
        "#,
    );

    assert!(m.is_workspace_root);
    assert_eq!(m.package_name, None);
    assert!(
        m.is_virtual_manifest(),
        "a [workspace] with no [package] is a virtual manifest and must not be scanned as a project"
    );
    assert_eq!(m.workspace_members, vec!["crates/core", "src-tauri"]);
}

#[test]
fn a_root_crate_is_both_a_workspace_root_and_a_package() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [workspace]
        members = ["crates/*"]
        "#,
    );

    assert!(m.is_workspace_root);
    assert_eq!(m.package_name.as_deref(), Some("app"));
    assert!(!m.is_virtual_manifest());
}

#[test]
fn workspace_member_globs_are_kept_exactly_as_written() {
    let m = manifest(
        r#"
        [workspace]
        members = ["crates/*", "src-tauri"]
        "#,
    );

    // Not expanded here: matching a glob onto real directories needs the list
    // of projects the scan already found, which this parser does not hold.
    assert_eq!(m.workspace_members, vec!["crates/*", "src-tauri"]);
}

#[test]
fn workspace_exclude_patterns_are_read_alongside_the_members() {
    let m = manifest(
        r#"
        [workspace]
        members = ["crates/*"]
        exclude = ["crates/legacy", "fixtures/*"]
        "#,
    );

    assert_eq!(m.workspace_members, vec!["crates/*"]);
    assert_eq!(m.workspace_exclude, vec!["crates/legacy", "fixtures/*"]);
}

#[test]
fn a_non_string_member_entry_is_dropped_rather_than_failing_the_parse() {
    let m = manifest(
        r#"
        [workspace]
        members = ["crates/core", 7, "src-tauri"]
        "#,
    );

    assert!(m.is_workspace_root);
    assert_eq!(m.workspace_members, vec!["crates/core", "src-tauri"]);
}

#[test]
fn an_inherited_package_name_is_reported_as_absent_rather_than_guessed() {
    // `name.workspace = true` resolves against the workspace root, which this
    // file does not contain. No answer beats inventing one.
    let m = manifest(
        r#"
        [package]
        name.workspace = true
        version = "0.1.0"
        "#,
    );

    assert_eq!(m.package_name, None);
}

// ---------------------------------------------------------------------------
// Path dependencies
// ---------------------------------------------------------------------------

#[test]
fn an_inline_table_path_dependency_records_the_path_as_written() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies]
        foo = { path = "../foo" }
        "#,
    );

    let dep = path_dep(&m, "foo");
    assert_eq!(dep.path, "../foo");
    assert_eq!(dep.kind, DependencyKind::Normal);
}

#[test]
fn a_dotted_section_path_dependency_is_recorded_identically_to_the_inline_form() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies.foo]
        path = "../foo"
        features = ["derive"]
        "#,
    );

    assert_eq!(
        m.path_dependencies,
        vec![PathDependency {
            name: "foo".into(),
            path: "../foo".into(),
            kind: DependencyKind::Normal,
        }]
    );
}

#[test]
fn dev_and_build_path_dependencies_carry_their_own_kind() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies]
        runtime = { path = "../runtime" }

        [dev-dependencies]
        harness = { path = "../harness" }

        [build-dependencies]
        codegen = { path = "../codegen" }
        "#,
    );

    assert_eq!(path_dep(&m, "runtime").kind, DependencyKind::Normal);
    assert_eq!(path_dep(&m, "harness").kind, DependencyKind::Dev);
    assert_eq!(path_dep(&m, "codegen").kind, DependencyKind::Build);
    assert_eq!(m.path_dependencies.len(), 3);
}

#[test]
fn a_registry_dependency_yields_no_path_dependency() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies]
        serde = "1"
        serde_json = { version = "1", features = ["raw_value"] }
        "#,
    );

    assert!(m.path_dependencies.is_empty());
}

#[test]
fn a_git_dependency_yields_no_path_dependency() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies]
        thing = { git = "https://example.invalid/thing.git", branch = "main" }
        "#,
    );

    assert!(m.path_dependencies.is_empty());
}

#[test]
fn a_git_dependency_with_a_path_key_is_still_recorded_by_its_path() {
    // Cargo rejects this combination, but a half-edited manifest can contain
    // it. Recording the path is what the author most recently wrote down; the
    // caller resolves it and reports a miss if there is nothing there.
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies]
        thing = { git = "https://example.invalid/thing.git", path = "../thing" }
        "#,
    );

    assert_eq!(path_dep(&m, "thing").path, "../thing");
}

#[test]
fn a_renamed_path_dependency_is_recorded_under_the_real_crate_name() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies]
        alias = { path = "../foo", package = "foo" }
        "#,
    );

    assert_eq!(path_dep(&m, "foo").path, "../foo");
    assert!(
        m.path_dependencies.iter().all(|d| d.name != "alias"),
        "the rename is the local alias; the crate on disk is named by `package`"
    );
}

#[test]
fn target_specific_path_dependencies_are_recorded_with_their_section_kind() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [target.'cfg(unix)'.dependencies]
        plat = { path = "../plat-unix" }

        [target.'cfg(windows)'.dev-dependencies]
        winharness = { path = "../win-harness" }
        "#,
    );

    assert_eq!(path_dep(&m, "plat").path, "../plat-unix");
    assert_eq!(path_dep(&m, "plat").kind, DependencyKind::Normal);
    assert_eq!(path_dep(&m, "winharness").kind, DependencyKind::Dev);
}

#[test]
fn a_workspace_dependency_table_yields_no_path_dependency() {
    // `[workspace.dependencies]` declares what members *may* inherit. The edge
    // belongs to whichever member writes `foo.workspace = true`, which this
    // file does not know — and a virtual root is not a node to draw from.
    let m = manifest(
        r#"
        [workspace]
        members = ["crates/*"]

        [workspace.dependencies]
        foo = { path = "crates/foo" }
        "#,
    );

    assert!(m.path_dependencies.is_empty());
}

#[test]
fn an_inherited_dependency_yields_no_path_dependency() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies]
        foo.workspace = true
        "#,
    );

    assert!(m.path_dependencies.is_empty());
}

#[test]
fn a_crate_that_is_both_a_member_and_a_path_dependency_appears_once_in_each_list() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [workspace]
        members = ["crates/foo"]

        [dependencies]
        foo = { path = "crates/foo" }
        "#,
    );

    assert_eq!(m.workspace_members, vec!["crates/foo"]);
    assert_eq!(m.path_dependencies.len(), 1);
    assert_eq!(path_dep(&m, "foo").path, "crates/foo");
}

#[test]
fn the_same_crate_in_two_sections_is_recorded_once_per_section() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [dependencies]
        foo = { path = "../foo" }

        [dev-dependencies]
        foo = { path = "../foo", features = ["test-util"] }
        "#,
    );

    assert_eq!(m.path_dependencies.len(), 2);
    let kinds: Vec<_> = m.path_dependencies.iter().map(|d| d.kind).collect();
    assert_eq!(kinds, vec![DependencyKind::Normal, DependencyKind::Dev]);
}

// ---------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------

#[test]
fn explicit_bin_and_lib_sections_are_reported() {
    let m = manifest(
        r#"
        [package]
        name = "app"

        [lib]
        name = "app_lib"

        [[bin]]
        name = "app"
        path = "src/bin/app.rs"
        "#,
    );

    assert!(m.has_bin);
    assert!(m.has_lib);
}

#[test]
fn a_manifest_with_no_target_sections_reports_neither_bin_nor_lib() {
    // The implicit `src/main.rs` / `src/lib.rs` conventions are invisible from
    // the manifest text alone; the caller holds the directory and combines.
    let m = manifest(
        r#"
        [package]
        name = "app"
        "#,
    );

    assert!(!m.has_bin);
    assert!(!m.has_lib);
}

#[test]
fn an_empty_bin_array_reports_no_binary() {
    let m = manifest(
        r#"
        [package]
        name = "app"
        bin = []
        "#,
    );

    assert!(!m.has_bin);
}

// ---------------------------------------------------------------------------
// Abstaining
// ---------------------------------------------------------------------------

#[test]
fn malformed_toml_yields_none_rather_than_panicking() {
    assert!(parse("[package").is_none());
    assert!(parse("name = ").is_none());
    assert!(parse("[package]\nname = \"a\"\n[package]\nname = \"b\"").is_none());
}

#[test]
fn an_empty_manifest_parses_to_an_empty_result() {
    let m = manifest("");

    assert_eq!(m, CargoManifest::default());
    assert!(!m.is_virtual_manifest());
}
