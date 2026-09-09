//! What build is this? — the data behind Help → About.
//!
//! A bridge with no decision in it, which is why the struct is **local to this
//! file** rather than living in `cb_core`: nothing in the core crate needs to
//! know what an About dialog renders, and there is nothing here to unit test
//! beyond the wire shape. Like [`crate::commands::notes`] it takes no
//! `AppState` — process metadata belongs to the process, not to a workspace.
//!
//! The application and Tauri versions are deliberately **not** here: the
//! frontend reads those from `@tauri-apps/api/app`, which already ships in the
//! bundle and is already permitted by `core:app:default`. This command exists
//! only for the facts that API cannot answer — the host platform and the build
//! provenance stamped in by `build.rs`.

use serde::Serialize;

/// The literal every field falls back to when it could not be established.
///
/// A build from a source tarball has no `.git`, and a build machine may have no
/// `git` on `PATH`. Both are ordinary, and both must read as *we do not know*
/// rather than as a blank the reader fills in themselves — a version report
/// that quietly omits its provenance is worse than one that admits it.
const UNKNOWN: &str = "unknown";

/// Everything about this build that only the process itself can answer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    /// `cb-app`'s crate version, which the Cargo workspace couples to every
    /// other manifest's. Carried alongside the frontend's `getVersion()` (which
    /// reads `tauri.conf.json`) so a drift between the two is *visible* in a
    /// bug report rather than silently picking one of them to believe.
    pub app_version: String,
    /// `std::env::consts::OS` — the target this binary was compiled for, not a
    /// user-agent string the webview might rewrite.
    pub os: String,
    /// `std::env::consts::ARCH`.
    pub arch: String,
    /// The short commit, suffixed `-dirty` when the working tree carried
    /// uncommitted changes at build time, or [`UNKNOWN`]. The suffix is not
    /// decoration: an unmarked sha on a modified tree is a *wrong* answer about
    /// which code is running.
    pub git_sha: String,
    /// UNIX epoch **seconds** as a decimal string, or [`UNKNOWN`].
    ///
    /// Seconds rather than a formatted date on purpose. Turning an instant into
    /// a calendar date needs real date arithmetic, and a build script is the one
    /// place in this tree that no test can reach — so the formatting lives in
    /// `aboutLogic.formatBuildDate`, where a test pins it.
    pub build_date: String,
}

/// Read this build's provenance. Cannot fail: every field has an established
/// [`UNKNOWN`] answer.
#[tauri::command]
pub async fn about_info() -> Result<AboutInfo, String> {
    Ok(AboutInfo {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        git_sha: option_env!("CB_GIT_SHA").unwrap_or(UNKNOWN).to_string(),
        build_date: option_env!("CB_BUILD_DATE").unwrap_or(UNKNOWN).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog reads these keys by name, and every one of them is two words,
    /// so a missing `rename_all` would ship `git_sha` to a UI asking for
    /// `gitSha` and render a blank where the commit should be. `types.ts`
    /// mirrors this list by hand — there is no codegen — so this is the only
    /// thing holding the two sides together.
    #[test]
    fn about_info_serialises_with_the_keys_the_ui_reads() {
        let info = AboutInfo {
            app_version: "1.3.0".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            git_sha: "abc1234-dirty".into(),
            build_date: "1756900000".into(),
        };
        let json = serde_json::to_value(&info).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["appVersion", "arch", "buildDate", "gitSha", "os"]);
    }

    /// An absent build stamp must reach the UI as the literal `unknown`, never
    /// as an empty string that would render as a blank line.
    #[tokio::test]
    async fn a_build_with_no_git_stamp_reports_unknown_rather_than_a_blank() {
        let info = about_info().await.unwrap();
        assert!(!info.git_sha.is_empty());
        assert!(!info.build_date.is_empty());
        assert_eq!(info.app_version, env!("CARGO_PKG_VERSION"));
    }
}
