//! Windows program-name resolution.
//!
//! `CreateProcess` resolves a bare program name against PATH by appending
//! `.exe` — and nothing else. Node package managers install as batch shims
//! (`pnpm.cmd`, `npm.cmd`, `yarn.cmd`), so spawning `pnpm` by name fails with
//! "program not found" even though it is plainly on PATH. The shell papers
//! over this with PATHEXT; a spawned process gets no such courtesy.
//!
//! `resolve_program` reproduces the PATHEXT walk: on Windows a bare,
//! extension-less name is searched across PATH trying each PATHEXT extension
//! in order, and the first hit is returned as a full path (Rust's `Command`
//! knows how to launch a `.cmd`/`.bat` by full path — it wraps it in
//! `cmd.exe` itself). Everything else — names with extensions, paths,
//! anything not found — passes through unchanged so the spawn error still
//! names what the configuration asked for. On non-Windows platforms this is
//! an identity function.

use std::path::{Path, PathBuf};

/// Resolve a program name for spawning. Identity on non-Windows.
pub fn resolve_program(program: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        let pathext = std::env::var("PATHEXT").ok();
        let exts = parse_pathext(pathext.as_deref());
        if let Some(found) = search(program, &dirs, &exts) {
            return found;
        }
    }
    PathBuf::from(program)
}

/// Split a PATHEXT value into extensions, falling back to the Windows
/// default when unset. Entries not starting with `.` are discarded.
fn parse_pathext(raw: Option<&str>) -> Vec<String> {
    const DEFAULT: &str = ".COM;.EXE;.BAT;.CMD";
    raw.unwrap_or(DEFAULT)
        .split(';')
        .filter(|e| e.starts_with('.'))
        .map(str::to_string)
        .collect()
}

/// The PATHEXT walk: for a bare, extension-less name, try every extension in
/// every directory — extension order decides within a directory, directory
/// order decides overall, matching what the shell would pick.
fn search(program: &str, dirs: &[PathBuf], exts: &[String]) -> Option<PathBuf> {
    if program.contains('/') || program.contains('\\') {
        return None;
    }
    if Path::new(program).extension().is_some() {
        return None;
    }
    for dir in dirs {
        for ext in exts {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            std::fs::write(dir.path().join(f), "").unwrap();
        }
        dir
    }

    fn exts(list: &[&str]) -> Vec<String> {
        list.iter().map(|e| e.to_string()).collect()
    }

    #[test]
    fn a_cmd_shim_is_found_by_appending_pathext_extensions() {
        let dir = dir_with(&["pnpm.cmd"]);
        let found = search(
            "pnpm",
            &[dir.path().to_path_buf()],
            &exts(&[".exe", ".cmd"]),
        );
        assert_eq!(found, Some(dir.path().join("pnpm.cmd")));
    }

    #[test]
    fn extension_order_decides_within_a_directory() {
        let dir = dir_with(&["tool.exe", "tool.cmd"]);
        let found = search(
            "tool",
            &[dir.path().to_path_buf()],
            &exts(&[".exe", ".cmd"]),
        );
        assert_eq!(found, Some(dir.path().join("tool.exe")));
    }

    #[test]
    fn an_earlier_path_directory_wins_over_a_better_extension_later() {
        let first = dir_with(&["tool.cmd"]);
        let second = dir_with(&["tool.exe"]);
        let found = search(
            "tool",
            &[first.path().to_path_buf(), second.path().to_path_buf()],
            &exts(&[".exe", ".cmd"]),
        );
        assert_eq!(found, Some(first.path().join("tool.cmd")));
    }

    #[test]
    fn a_name_with_a_path_separator_is_never_searched() {
        let dir = dir_with(&["pnpm.cmd"]);
        let dirs = [dir.path().to_path_buf()];
        assert_eq!(search("tools/pnpm", &dirs, &exts(&[".cmd"])), None);
        assert_eq!(search("tools\\pnpm", &dirs, &exts(&[".cmd"])), None);
    }

    #[test]
    fn a_name_that_already_has_an_extension_is_never_searched() {
        let dir = dir_with(&["pnpm.cmd", "pnpm.cmd.cmd"]);
        let found = search("pnpm.cmd", &[dir.path().to_path_buf()], &exts(&[".cmd"]));
        assert_eq!(found, None);
    }

    #[test]
    fn a_directory_entry_is_not_mistaken_for_a_program() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("pnpm.cmd")).unwrap();
        let found = search("pnpm", &[dir.path().to_path_buf()], &exts(&[".cmd"]));
        assert_eq!(found, None);
    }

    #[test]
    fn missing_directories_are_skipped_not_errors() {
        let real = dir_with(&["pnpm.cmd"]);
        let found = search(
            "pnpm",
            &[
                PathBuf::from("Z:\\definitely\\not\\here"),
                real.path().to_path_buf(),
            ],
            &exts(&[".cmd"]),
        );
        assert_eq!(found, Some(real.path().join("pnpm.cmd")));
    }

    #[test]
    fn pathext_falls_back_to_the_windows_default_when_unset() {
        assert_eq!(parse_pathext(None), exts(&[".COM", ".EXE", ".BAT", ".CMD"]));
    }

    #[test]
    fn pathext_entries_without_a_leading_dot_are_discarded() {
        assert_eq!(
            parse_pathext(Some(".EXE;EXE;;.CMD")),
            exts(&[".EXE", ".CMD"])
        );
    }

    #[test]
    fn an_unresolvable_name_passes_through_unchanged() {
        // Cross-platform: a nonsense name resolves to itself so the spawn
        // error still names what the configuration asked for.
        assert_eq!(
            resolve_program("definitely-not-a-real-program-xyz"),
            PathBuf::from("definitely-not-a-real-program-xyz")
        );
    }

    #[cfg(windows)]
    #[test]
    fn pnpm_resolves_to_its_cmd_shim_on_a_machine_that_has_it() {
        // Real-environment sanity check in the spirit of
        // `evaluates_a_real_project_when_the_sdk_is_available`: only asserts
        // when a shim-only pnpm install is present.
        let resolved = resolve_program("pnpm");
        if resolved != Path::new("pnpm") {
            let ext = resolved
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_lowercase);
            assert!(
                matches!(
                    ext.as_deref(),
                    Some("cmd") | Some("bat") | Some("exe") | Some("com")
                ),
                "resolved to {} which has no executable extension",
                resolved.display()
            );
        }
    }
}
