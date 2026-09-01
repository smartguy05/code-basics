//! .NET user secrets: per-project secrets stored *outside* the repository.
//!
//! Mirrors what `dotnet user-secrets` and Rider's "Manage .NET User Secrets"
//! do: a project names its store with a `<UserSecretsId>` property, and the
//! secrets themselves live in a `secrets.json` under the user profile —
//! `%APPDATA%\Microsoft\UserSecrets\<id>\` on Windows,
//! `~/.microsoft/usersecrets/<id>/` elsewhere — where the .NET configuration
//! system picks them up at runtime. Nothing secret ever touches the workspace.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::adapters::dotnet;

/// What the UI needs to show and edit a project's secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSecrets {
    /// The project's `<UserSecretsId>`, when it has one.
    pub secrets_id: Option<String>,
    /// Absolute path of the `secrets.json` the id resolves to.
    pub path: Option<PathBuf>,
    /// Contents of that file, when it exists.
    pub content: Option<String>,
}

/// Resolve a workspace-relative project path against the open workspace,
/// refusing anything that escapes it.
///
/// This is one of *two* traversal guards in this module, deliberately: this
/// one covers the path the frontend sends (a `RunConfig.project`, so
/// attacker-influenced only as far as the config file is), while
/// [`secrets_path`] covers the `<UserSecretsId>` read out of project XML.
/// They guard different inputs crossing different boundaries — an id that
/// never touches this function still becomes a path segment — so neither
/// subsumes the other and both must stay.
///
/// Containment is decided on the *canonical* path, after the OS has resolved
/// `..`, symlinks and (on Windows) short names, because only the canonical
/// form tells us where the path really lands. Note that `Path::join` with an
/// absolute `project` discards `root` entirely, so this check is the only
/// thing standing between an absolute path and the filesystem. `starts_with`
/// compares whole components, so a sibling directory whose name merely begins
/// with the root's name is correctly outside.
pub fn resolve_project_path(root: &Path, project: &str) -> Result<PathBuf, String> {
    // Canonicalise the root too: comparing a canonical path against a
    // non-canonical prefix can reject a legitimate path (or, worse, fail to
    // reject an illegitimate one) when the two spell the same directory
    // differently. Fall back to the root as given if it cannot be resolved,
    // so the failure is still reported against the project path below.
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let path = root.join(project);

    let canonical = dunce::canonicalize(&path)
        .map_err(|e| format!("{} does not exist: {e}", path.display()))?;
    if !canonical.starts_with(&root) {
        return Err(format!("{project} is outside the workspace"));
    }
    Ok(canonical)
}

/// Where a secrets id's `secrets.json` lives on this machine.
///
/// The id comes out of an XML property and becomes a path segment, so it is
/// validated rather than trusted. This is the second of the two guards
/// described on [`resolve_project_path`]: that one keeps the *project* inside
/// the workspace, this one keeps the *id* from steering writes out of the
/// user-secrets store. A project inside the workspace can still carry a
/// hostile id, so passing the first guard earns nothing here.
pub fn secrets_path(id: &str) -> Result<PathBuf> {
    if id.is_empty()
        || id
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        || id == "."
        || id == ".."
    {
        bail!("`{id}` is not a usable UserSecretsId");
    }

    let base = if cfg!(windows) {
        PathBuf::from(std::env::var("APPDATA").context("APPDATA is not set")?)
            .join("Microsoft")
            .join("UserSecrets")
    } else {
        PathBuf::from(std::env::var("HOME").context("HOME is not set")?)
            .join(".microsoft")
            .join("usersecrets")
    };

    Ok(base.join(id).join("secrets.json"))
}

/// Read a project's secrets id from its project file.
pub fn user_secrets_id(project_path: &Path) -> Result<Option<String>> {
    let content = std::fs::read_to_string(project_path)
        .with_context(|| format!("failed to read {}", project_path.display()))?;
    Ok(dotnet::parse_project_file(&content).user_secrets_id)
}

/// Everything needed to display a project's secrets: id, file location, and
/// the file's contents when it exists.
pub fn read(project_path: &Path) -> Result<ProjectSecrets> {
    let Some(id) = user_secrets_id(project_path)? else {
        return Ok(ProjectSecrets {
            secrets_id: None,
            path: None,
            content: None,
        });
    };

    read_with_id(&id)
}

/// Read a secrets store whose id was obtained from evaluated MSBuild
/// properties (for example through an imported props file).
pub fn read_with_id(id: &str) -> Result<ProjectSecrets> {
    let path = secrets_path(id)?;
    let content = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
    };

    Ok(ProjectSecrets {
        secrets_id: Some(id.to_string()),
        path: Some(path),
        content,
    })
}

/// Give a project a `<UserSecretsId>` if it does not have one, returning the
/// id either way. This is the one thing here that edits a file inside the
/// workspace, and it matches what `dotnet user-secrets init` writes.
pub fn ensure_id(project_path: &Path) -> Result<String> {
    if let Some(id) = user_secrets_id(project_path)? {
        return Ok(id);
    }

    let xml = std::fs::read_to_string(project_path)
        .with_context(|| format!("failed to read {}", project_path.display()))?;
    let id = uuid::Uuid::new_v4().to_string();

    let updated = insert_secrets_id(&xml, &id).with_context(|| {
        format!(
            "{} has no <PropertyGroup> or <Project> element to add a UserSecretsId to",
            project_path.display()
        )
    })?;

    std::fs::write(project_path, updated)
        .with_context(|| format!("failed to write {}", project_path.display()))?;
    Ok(id)
}

/// Insert `<UserSecretsId>` into project XML: inside the first
/// `<PropertyGroup>` when there is one, otherwise as a new group right after
/// the `<Project>` opening tag. Text manipulation rather than an XML rewrite,
/// so the rest of the file keeps its exact formatting.
fn insert_secrets_id(xml: &str, id: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();

    if let Some(open) = lower.find("<propertygroup") {
        let end = open + lower[open..].find('>')? + 1;
        // Match the file's indentation by reusing whatever precedes the tag.
        let line_start = xml[..open].rfind('\n').map_or(0, |i| i + 1);
        let indent = &xml[line_start..open];
        let insert = format!("\n{indent}  <UserSecretsId>{id}</UserSecretsId>");
        return Some(format!("{}{}{}", &xml[..end], insert, &xml[end..]));
    }

    let open = lower.find("<project")?;
    let end = open + lower[open..].find('>')? + 1;
    let insert =
        format!("\n  <PropertyGroup>\n    <UserSecretsId>{id}</UserSecretsId>\n  </PropertyGroup>");
    Some(format!("{}{}{}", &xml[..end], insert, &xml[end..]))
}

/// A UTF-8 byte-order mark, which .NET's JSON reader skips and `serde_json`
/// refuses. `dotnet user-secrets` and Rider both write one, so a `secrets.json`
/// this app never created is quite likely to start with it.
const BOM: &str = "\u{feff}";

/// Reduce the JSON dialect .NET's configuration loader accepts — a leading
/// byte-order mark, `//` and `/* */` comments, and trailing commas — to strict
/// JSON, for validation. Comments become spaces rather than disappearing, so
/// error positions still roughly line up with the original text.
///
/// Shared with [`crate::sql::discover`], which reads the same dialect out of
/// `appsettings*.json` and `secrets.json`. It is reused rather than copied
/// because there must be exactly one description of what .NET accepts.
pub(crate) fn strip_jsonc(text: &str) -> String {
    // The mark becomes spaces for the same reason a comment does: so a position
    // serde reports still points at the right place in what the user wrote.
    let text = match text.strip_prefix(BOM) {
        Some(rest) => format!("{}{rest}", " ".repeat(BOM.len())),
        None => text.to_string(),
    };
    let text = text.as_str();
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut in_string = false;

    while i < bytes.len() {
        let b = bytes[i];

        if in_string {
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if b == b'"' {
                in_string = false;
            }
            i += 1;
        } else if b == b'"' {
            in_string = true;
            out.push(b);
            i += 1;
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    out.extend_from_slice(b"  ");
                    i += 2;
                    closed = true;
                    break;
                }
                out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                i += 1;
            }
            if !closed {
                break;
            }
        } else if b == b',' {
            // A comma followed only by whitespace (or a comment, but those are
            // gone by the time we look ahead within `out`) and then a closing
            // bracket is a trailing comma; .NET allows it, serde_json does not.
            let mut j = i + 1;
            while j < bytes.len()
                && (bytes[j].is_ascii_whitespace()
                    || (bytes[j] == b'/' && matches!(bytes.get(j + 1), Some(b'/' | b'*'))))
            {
                if bytes[j] == b'/' {
                    if bytes[j + 1] == b'/' {
                        while j < bytes.len() && bytes[j] != b'\n' {
                            j += 1;
                        }
                    } else {
                        j += 2;
                        while j < bytes.len()
                            && !(bytes[j] == b'*' && bytes.get(j + 1) == Some(&b'/'))
                        {
                            j += 1;
                        }
                        j = (j + 2).min(bytes.len());
                    }
                } else {
                    j += 1;
                }
            }
            if matches!(bytes.get(j), Some(b'}' | b']')) {
                out.push(b' ');
            } else {
                out.push(b);
            }
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }

    String::from_utf8(out).expect("stripping only replaces ASCII bytes with ASCII")
}

/// Turn a `serde_json` failure over stripped text into a message that names the
/// line and shows it.
///
/// serde's own message ends in "at line L column C", which is true of the
/// *stripped* text — but stripping only ever replaces bytes with spaces, never
/// moves them, so the line number is the user's line number too. Quoting that
/// line is what turns "secrets are not valid JSON" from a dead end into
/// something the person reading it can act on; the original report of this bug
/// blamed the comments in the file, and the real cause was a byte-order mark
/// nothing on screen could show.
fn jsonc_error(original: &str, error: &serde_json::Error) -> anyhow::Error {
    /// Long enough for a connection string, short enough not to wrap an error
    /// banner into a wall.
    const MAX_QUOTED: usize = 200;

    let line = error.line();
    let quoted = original
        .lines()
        .nth(line.saturating_sub(1))
        .map(str::trim_end)
        .filter(|text| !text.is_empty());

    match quoted {
        // A position of 0 means serde could not place the failure at all.
        Some(text) if line > 0 => {
            let shown: String = if text.chars().count() > MAX_QUOTED {
                format!("{}…", text.chars().take(MAX_QUOTED).collect::<String>())
            } else {
                text.to_string()
            };
            anyhow!("secrets are not valid JSON: {error}\n  line {line}: {shown}")
        }
        _ => anyhow!("secrets are not valid JSON: {error}"),
    }
}

/// Save a project's secrets, adding a `<UserSecretsId>` to the project first
/// when it has none.
pub fn write(project_path: &Path, content: &str) -> Result<ProjectSecrets> {
    // The .NET configuration loader requires a JSON *object* but tolerates
    // comments and trailing commas; validate against that same dialect —
    // catching real syntax errors here beats a failure at application
    // startup, and rejecting the comments Rider writes would be worse.
    let parsed: serde_json::Value =
        serde_json::from_str(&strip_jsonc(content)).map_err(|e| jsonc_error(content, &e))?;
    if !parsed.is_object() {
        bail!("secrets must be a JSON object of key/value pairs");
    }

    let id = ensure_id(project_path)?;
    let path = secrets_path(&id)?;

    let dir = path.parent().expect("secrets path always has a parent");
    std::fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let text = if content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{content}\n")
    };
    std::fs::write(&path, &text).with_context(|| format!("failed to write {}", path.display()))?;

    Ok(ProjectSecrets {
        secrets_id: Some(id),
        path: Some(path),
        content: Some(text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_ID: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <UserSecretsId>aa11bb22-cc33-dd44-ee55-ff6677889900</UserSecretsId>
  </PropertyGroup>
</Project>"#;

    const WITHOUT_ID: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
  </PropertyGroup>
</Project>"#;

    fn project_with(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("App.csproj");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn reads_the_user_secrets_id() {
        let (_dir, path) = project_with(WITH_ID);
        assert_eq!(
            user_secrets_id(&path).unwrap().as_deref(),
            Some("aa11bb22-cc33-dd44-ee55-ff6677889900")
        );
    }

    #[test]
    fn a_project_without_an_id_reads_as_empty() {
        let (_dir, path) = project_with(WITHOUT_ID);
        let secrets = read(&path).unwrap();

        assert_eq!(secrets.secrets_id, None);
        assert_eq!(secrets.path, None);
        assert_eq!(secrets.content, None);
    }

    #[test]
    fn ensure_id_keeps_an_existing_id() {
        let (_dir, path) = project_with(WITH_ID);
        assert_eq!(
            ensure_id(&path).unwrap(),
            "aa11bb22-cc33-dd44-ee55-ff6677889900"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            WITH_ID,
            "the file must not change"
        );
    }

    #[test]
    fn ensure_id_adds_one_inside_the_existing_property_group() {
        let (_dir, path) = project_with(WITHOUT_ID);
        let id = ensure_id(&path).unwrap();

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(
            updated.contains(&format!("<UserSecretsId>{id}</UserSecretsId>")),
            "{updated}"
        );
        assert_eq!(
            updated.matches("<PropertyGroup>").count(),
            1,
            "must reuse the existing group, not add a second one: {updated}"
        );
        assert_eq!(
            user_secrets_id(&path).unwrap(),
            Some(id),
            "the parser must see it back"
        );
    }

    #[test]
    fn ensure_id_creates_a_property_group_when_there_is_none() {
        let (_dir, path) = project_with(
            r#"<Project Sdk="Microsoft.NET.Sdk">
</Project>"#,
        );
        let id = ensure_id(&path).unwrap();

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("<PropertyGroup>"), "{updated}");
        assert_eq!(user_secrets_id(&path).unwrap(), Some(id));
    }

    #[test]
    fn secrets_path_ends_with_the_id_and_file_name() {
        let path = secrets_path("some-id").unwrap();
        assert!(
            path.ends_with(Path::new("some-id").join("secrets.json")),
            "{}",
            path.display()
        );
    }

    #[test]
    fn a_path_traversal_id_is_rejected() {
        // The id comes out of project XML, which may not be trustworthy.
        assert!(secrets_path("../../etc").is_err());
        assert!(secrets_path("a/b").is_err());
        assert!(secrets_path("a\\b").is_err());
        assert!(secrets_path("").is_err());
        assert!(secrets_path("..").is_err());
    }

    #[test]
    fn writing_rejects_invalid_json() {
        let (_dir, path) = project_with(WITH_ID);
        assert!(write(&path, "{ not json").is_err());
        assert!(
            write(&path, "[1, 2]").is_err(),
            "an array is not a secrets object"
        );
    }

    #[test]
    fn validation_accepts_the_dialect_dotnet_accepts() {
        // .NET's configuration loader reads secrets.json with comments and
        // trailing commas enabled, and Rider writes comments into new files.
        let jsonc = r#"{
  // Local database
  "ConnectionStrings:DatabaseConnection": "Server=.;Database=app", /* dev only */
  "ConnectionStrings:RedisConnection": "localhost:6379",
}"#;

        let stripped = strip_jsonc(jsonc);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect(&stripped);
        assert_eq!(value.as_object().unwrap().len(), 2);
    }

    /// Every shape .NET's configuration loader accepts that a person actually
    /// writes into a `secrets.json`, checked one at a time so a failure names
    /// the shape rather than "something in this file".
    ///
    /// .NET reads this file with `JsonDocumentOptions { CommentHandling = Skip,
    /// AllowTrailingCommas = true }`, and both `dotnet user-secrets` and Rider
    /// write files this module then has to accept back.
    #[test]
    fn the_dialect_dotnet_accepts_round_trips_shape_by_shape() {
        let cases: &[(&str, &str)] = &[
            (
                "line comment on its own line",
                "{\n  // note\n  \"a\": \"1\"\n}",
            ),
            (
                "line comment at end of line",
                "{\n  \"a\": \"1\" // note\n}",
            ),
            (
                "line comment as the last line, no trailing newline",
                "{\n  \"a\": \"1\"\n}\n// note",
            ),
            (
                "line comment before the opening brace",
                "// note\n{ \"a\": \"1\" }",
            ),
            ("block comment inline", "{ /* note */ \"a\": \"1\" }"),
            (
                "block comment spanning lines",
                "{\n  /* one\n     two */\n  \"a\": \"1\"\n}",
            ),
            ("empty block comment", "{ /**/ \"a\": \"1\" }"),
            (
                "block comment ending in a double star",
                "{ /* note **/ \"a\": \"1\" }",
            ),
            (
                "comment containing a quote",
                "{\n  // do not say \"prod\"\n  \"a\": \"1\"\n}",
            ),
            (
                "comment containing a brace",
                "{\n  // } not the end\n  \"a\": \"1\"\n}",
            ),
            ("trailing comma", "{ \"a\": \"1\", }"),
            (
                "trailing comma then a line comment",
                "{\n  \"a\": \"1\", // note\n}",
            ),
            (
                "trailing comma then a block comment",
                "{ \"a\": \"1\", /* note */ }",
            ),
            ("CRLF line endings", "{\r\n  // note\r\n  \"a\": \"1\"\r\n}"),
            (
                "nested object with comments",
                "{\n  \"a\": {\n    // note\n    \"b\": \"1\"\n  }\n}",
            ),
            (
                "non-ASCII inside a comment",
                "{\n  // caf\u{e9} \u{2014} note\n  \"a\": \"1\"\n}",
            ),
            ("non-ASCII inside a value", "{ \"a\": \"caf\u{e9}\" }"),
            ("UTF-8 BOM", "\u{feff}{\n  // note\n  \"a\": \"1\"\n}"),
        ];

        let mut failures = Vec::new();
        for (name, text) in cases {
            let stripped = strip_jsonc(text);
            match serde_json::from_str::<serde_json::Value>(&stripped) {
                Ok(value) if value.is_object() => {}
                Ok(_) => failures.push(format!("{name}: parsed, but not into an object")),
                Err(e) => failures.push(format!("{name}: {e}")),
            }
        }
        assert!(
            failures.is_empty(),
            "shapes .NET accepts but this does not:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn comment_markers_inside_strings_are_left_alone() {
        let jsonc = r#"{ "url": "https://example.com/a", "note": "a // b /* c */" }"#;
        let value: serde_json::Value = serde_json::from_str(&strip_jsonc(jsonc)).unwrap();

        assert_eq!(value["url"], "https://example.com/a");
        assert_eq!(value["note"], "a // b /* c */");
    }

    #[test]
    fn a_byte_order_mark_is_tolerated_the_way_dotnet_tolerates_it() {
        // The bug this was reported as: "secrets.json with comments will not
        // save". The comments were never the problem — every comment shape
        // already round-tripped. The file simply began with a mark that
        // `dotnet user-secrets` had written and nothing on screen could show.
        let jsonc = "\u{feff}{\n  // Local database\n  \"a\": \"1\",\n}";
        let value: serde_json::Value = serde_json::from_str(&strip_jsonc(jsonc)).unwrap();

        assert_eq!(value["a"], "1");
    }

    #[test]
    fn only_a_leading_mark_is_stripped() {
        // Anywhere else it is an ordinary character, and a stray one mid-file is
        // a real error that must stay one.
        assert!(serde_json::from_str::<serde_json::Value>(&strip_jsonc("{\u{feff}}")).is_err());
        // Inside a string it is data the user typed, and must survive.
        let value: serde_json::Value =
            serde_json::from_str(&strip_jsonc("{ \"a\": \"x\u{feff}y\" }")).unwrap();
        assert_eq!(value["a"], "x\u{feff}y");
    }

    #[test]
    fn a_write_that_fails_validation_names_and_quotes_the_line() {
        let (_dir, path) = project_with(WITH_ID);
        let broken = "{\n  \"a\": \"1\"\n  \"b\": oops\n}";
        let message = format!("{:#}", write(&path, broken).unwrap_err());

        assert!(message.contains("line 3"), "{message}");
        assert!(message.contains("\"b\": oops"), "{message}");
    }

    #[test]
    fn a_failure_message_survives_a_line_it_cannot_quote() {
        let (_dir, path) = project_with(WITH_ID);
        // Nothing to quote: the failure is at the empty end of the input.
        let message = format!("{:#}", write(&path, "").unwrap_err());
        assert!(message.contains("secrets are not valid JSON"), "{message}");
    }

    #[test]
    fn stripping_does_not_make_broken_json_valid() {
        assert!(serde_json::from_str::<serde_json::Value>(&strip_jsonc("{ not json")).is_err());
        assert!(
            serde_json::from_str::<serde_json::Value>(&strip_jsonc("{ \"a\": 1 /* open")).is_err(),
            "an unterminated block comment is still an error"
        );
    }

    /// A workspace root with `ws/src/App.csproj` inside it, plus a
    /// `sibling/Other.csproj` and a `ws-evil/Evil.csproj` next to the root.
    /// The last one exists to catch a *string* prefix comparison: `ws-evil`
    /// starts with the text `ws` but is not inside it.
    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(dir.path()).unwrap();

        std::fs::create_dir_all(base.join("ws/src")).unwrap();
        std::fs::write(base.join("ws/src/App.csproj"), WITH_ID).unwrap();
        std::fs::create_dir_all(base.join("sibling")).unwrap();
        std::fs::write(base.join("sibling/Other.csproj"), WITH_ID).unwrap();
        std::fs::create_dir_all(base.join("ws-evil")).unwrap();
        std::fs::write(base.join("ws-evil/Evil.csproj"), WITH_ID).unwrap();

        let root = base.join("ws");
        (dir, root)
    }

    #[test]
    fn resolves_a_nested_project() {
        let (_dir, root) = workspace();
        let resolved = resolve_project_path(&root, "src/App.csproj").unwrap();

        assert_eq!(
            resolved,
            dunce::canonicalize(root.join("src/App.csproj")).unwrap()
        );
    }

    #[test]
    fn rejects_a_parent_traversal() {
        let (_dir, root) = workspace();
        let err = resolve_project_path(&root, "../sibling/Other.csproj").unwrap_err();

        assert_eq!(err, "../sibling/Other.csproj is outside the workspace");
    }

    #[test]
    fn rejects_a_traversal_that_dips_through_a_real_subdirectory() {
        // `src` exists, so every component of this path resolves; only the
        // canonical result reveals that it left the workspace.
        let (_dir, root) = workspace();
        let err = resolve_project_path(&root, "src/../../sibling/Other.csproj").unwrap_err();

        assert_eq!(
            err,
            "src/../../sibling/Other.csproj is outside the workspace"
        );
    }

    #[test]
    fn rejects_an_absolute_path_outside_the_root() {
        // `Path::join` with an absolute path discards the root entirely, so
        // the containment check is the only thing standing here.
        let (_dir, root) = workspace();
        let outside = root.parent().unwrap().join("sibling/Other.csproj");
        let err = resolve_project_path(&root, &outside.to_string_lossy()).unwrap_err();

        assert!(err.ends_with("is outside the workspace"), "{err}");
    }

    #[test]
    fn a_sibling_sharing_a_name_prefix_is_outside() {
        let (_dir, root) = workspace();
        let err = resolve_project_path(&root, "../ws-evil/Evil.csproj").unwrap_err();

        assert_eq!(err, "../ws-evil/Evil.csproj is outside the workspace");
    }

    #[test]
    fn a_missing_project_names_the_path_it_looked_for() {
        let (_dir, root) = workspace();
        let err = resolve_project_path(&root, "src/Nope.csproj").unwrap_err();

        // The message names the path as it was assembled — the root plus the
        // relative path verbatim — so the user can see where it looked.
        assert!(
            err.contains(&root.join("src/Nope.csproj").display().to_string()),
            "{err}"
        );
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    #[cfg(windows)]
    fn accepts_either_separator_on_windows() {
        let (_dir, root) = workspace();
        let expected = dunce::canonicalize(root.join("src/App.csproj")).unwrap();

        assert_eq!(
            resolve_project_path(&root, "src\\App.csproj").unwrap(),
            expected
        );
        assert_eq!(
            resolve_project_path(&root, "src/App.csproj").unwrap(),
            expected
        );
        // Mixed separators in a traversal must not slip past either.
        assert!(resolve_project_path(&root, "src\\..\\../sibling/Other.csproj").is_err());
    }

    #[test]
    fn serialises_with_the_keys_the_ui_reads() {
        let secrets = ProjectSecrets {
            secrets_id: Some("id".into()),
            path: Some("p".into()),
            content: None,
        };
        let json = serde_json::to_value(&secrets).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort();

        assert_eq!(keys, ["content", "path", "secretsId"]);
    }
}
