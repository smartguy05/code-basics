//! Tests for solution parsing. Included by `solution.rs` under `#[cfg(test)]`.

use std::path::{Path, PathBuf};

use super::solution::*;

/// A cut-down but structurally faithful `.sln`: two projects, one of them
/// inside a nested solution folder.
const SLN: &str = r#"
Microsoft Visual Studio Solution File, Format Version 12.00
# Visual Studio Version 17
Project("{2150E333-8FDC-42A3-9474-1AB1AEA671C7}") = "src", "src", "{11111111-1111-1111-1111-111111111111}"
EndProject
Project("{2150E333-8FDC-42A3-9474-1AB1AEA671C7}") = "core", "core", "{22222222-2222-2222-2222-222222222222}"
EndProject
Project("{9A19103F-16F7-4668-BE54-9A1E7A4F7556}") = "App", "src\App\App.csproj", "{33333333-3333-3333-3333-333333333333}"
EndProject
Project("{9A19103F-16F7-4668-BE54-9A1E7A4F7556}") = "App.Tests", "tests\App.Tests\App.Tests.csproj", "{44444444-4444-4444-4444-444444444444}"
EndProject
Global
	GlobalSection(NestedProjects) = preSolution
		{22222222-2222-2222-2222-222222222222} = {11111111-1111-1111-1111-111111111111}
		{33333333-3333-3333-3333-333333333333} = {22222222-2222-2222-2222-222222222222}
	EndGlobalSection
EndGlobal
"#;

const SLNX: &str = r#"<Solution>
  <Folder Name="/src/">
    <Folder Name="/src/core/">
      <Project Path="src/App/App.csproj" />
    </Folder>
  </Folder>
  <Project Path="tests/App.Tests/App.Tests.csproj" />
</Solution>"#;

#[test]
fn reads_projects_from_a_classic_sln() {
    let projects = parse("Repo", SLN, Path::new(""), false);

    let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["App", "App.Tests"], "solution folders are not projects");
}

#[test]
fn sln_project_paths_use_forward_slashes() {
    // The scan produces `src/App/App.csproj`; a solution writes it with
    // backslashes, and the two have to compare equal.
    let projects = parse("Repo", SLN, Path::new(""), false);
    let app = projects.iter().find(|p| p.name == "App").unwrap();

    assert_eq!(app.path, PathBuf::from("src/App/App.csproj"));
}

#[test]
fn sln_nested_solution_folders_become_a_path() {
    let projects = parse("Repo", SLN, Path::new(""), false);

    let app = projects.iter().find(|p| p.name == "App").unwrap();
    assert_eq!(app.folder.as_deref(), Some("src/core"));

    let tests = projects.iter().find(|p| p.name == "App.Tests").unwrap();
    assert_eq!(tests.folder, None, "a project at the solution root has no folder");
}

#[test]
fn a_solution_below_the_workspace_root_resolves_against_its_own_directory() {
    let projects = parse("Repo", SLN, Path::new("dotnet"), false);
    let app = projects.iter().find(|p| p.name == "App").unwrap();

    assert_eq!(app.path, PathBuf::from("dotnet/src/App/App.csproj"));
}

#[test]
fn parent_segments_in_a_solution_path_are_resolved() {
    let sln = r#"Project("{9A19103F-16F7-4668-BE54-9A1E7A4F7556}") = "Shared", "..\shared\Shared.csproj", "{55555555-5555-5555-5555-555555555555}"
EndProject"#;
    let projects = parse("Repo", sln, Path::new("dotnet"), false);

    assert_eq!(projects[0].path, PathBuf::from("shared/Shared.csproj"));
}

#[test]
fn reads_projects_from_an_slnx() {
    let projects = parse("Repo", SLNX, Path::new(""), true);

    let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["App", "App.Tests"]);
}

#[test]
fn slnx_folders_are_reported_without_their_surrounding_slashes() {
    let projects = parse("Repo", SLNX, Path::new(""), true);

    let app = projects.iter().find(|p| p.name == "App").unwrap();
    assert_eq!(app.folder.as_deref(), Some("src/core"));

    let tests = projects.iter().find(|p| p.name == "App.Tests").unwrap();
    assert_eq!(tests.folder, None);
}

#[test]
fn an_empty_folder_element_does_not_capture_later_projects() {
    // `<Folder Name="/empty/" />` closes immediately; treating it as an open
    // scope would file every following project under it.
    let slnx = r#"<Solution>
      <Folder Name="/empty/" />
      <Project Path="src/App/App.csproj" />
    </Solution>"#;
    let projects = parse("Repo", slnx, Path::new(""), true);

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].folder, None);
}

#[test]
fn both_formats_are_recognised_by_extension() {
    assert!(is_solution_file(Path::new("Repo.sln")));
    assert!(is_solution_file(Path::new("Repo.slnx")));
    assert!(!is_solution_file(Path::new("Repo.csproj")));
    assert!(!is_solution_file(Path::new("Repo.slnf")), "solution filters are not solutions");
}

#[test]
fn malformed_input_yields_no_projects_rather_than_failing() {
    assert!(parse("Repo", "not a solution at all", Path::new(""), false).is_empty());
    assert!(parse("Repo", "<Solution", Path::new(""), true).is_empty());
    assert!(parse("Repo", "", Path::new(""), false).is_empty());
}
