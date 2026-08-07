//! .NET user secrets: per-project secrets stored *outside* the repository.
//!
//! Mirrors what `dotnet user-secrets` and Rider's "Manage .NET User Secrets"
//! do: a project names its store with a `<UserSecretsId>` property, and the
//! secrets themselves live in a `secrets.json` under the user profile —
//! `%APPDATA%\Microsoft\UserSecrets\<id>\` on Windows,
//! `~/.microsoft/usersecrets/<id>/` elsewhere — where the .NET configuration
//! system picks them up at runtime. Nothing secret ever touches the workspace.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
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

/// Where a secrets id's `secrets.json` lives on this machine.
///
/// The id comes out of an XML property and becomes a path segment, so it is
/// validated rather than trusted.
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

    let path = secrets_path(&id)?;
    let content = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
    };

    Ok(ProjectSecrets {
        secrets_id: Some(id),
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

/// Reduce the JSON dialect .NET's configuration loader accepts — `//` and
/// `/* */` comments plus trailing commas — to strict JSON, for validation.
/// Comments become spaces rather than disappearing, so error positions still
/// roughly line up with the original text.
fn strip_jsonc(text: &str) -> String {
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

/// Save a project's secrets, adding a `<UserSecretsId>` to the project first
/// when it has none.
pub fn write(project_path: &Path, content: &str) -> Result<ProjectSecrets> {
    // The .NET configuration loader requires a JSON *object* but tolerates
    // comments and trailing commas; validate against that same dialect —
    // catching real syntax errors here beats a failure at application
    // startup, and rejecting the comments Rider writes would be worse.
    let parsed: serde_json::Value =
        serde_json::from_str(&strip_jsonc(content)).context("secrets are not valid JSON")?;
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

    #[test]
    fn comment_markers_inside_strings_are_left_alone() {
        let jsonc = r#"{ "url": "https://example.com/a", "note": "a // b /* c */" }"#;
        let value: serde_json::Value = serde_json::from_str(&strip_jsonc(jsonc)).unwrap();

        assert_eq!(value["url"], "https://example.com/a");
        assert_eq!(value["note"], "a // b /* c */");
    }

    #[test]
    fn stripping_does_not_make_broken_json_valid() {
        assert!(serde_json::from_str::<serde_json::Value>(&strip_jsonc("{ not json")).is_err());
        assert!(
            serde_json::from_str::<serde_json::Value>(&strip_jsonc("{ \"a\": 1 /* open")).is_err(),
            "an unterminated block comment is still an error"
        );
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
