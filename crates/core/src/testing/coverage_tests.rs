//! Tests for the coverage-report parsers.

use super::*;

#[test]
fn cobertura_reads_line_hits_per_file() {
    let xml = r#"<?xml version="1.0"?>
<coverage>
  <packages>
    <package name="Api">
      <classes>
        <class name="Api.Foo" filename="src/Api/Foo.cs">
          <lines>
            <line number="10" hits="3"/>
            <line number="11" hits="0"/>
            <line number="12" hits="1"/>
          </lines>
        </class>
      </classes>
    </package>
  </packages>
</coverage>"#;

    let files = parse_cobertura(xml).unwrap();
    assert_eq!(files.len(), 1);
    let foo = &files[0];
    assert_eq!(foo.path, "src/Api/Foo.cs");
    assert_eq!(foo.lines.get(&10), Some(&3));
    assert_eq!(foo.lines.get(&11), Some(&0));
    assert_eq!(foo.lines.get(&12), Some(&1));
    // A line the tool never emitted is absent, not zero.
    assert_eq!(foo.lines.get(&13), None);
}

#[test]
fn cobertura_merges_two_classes_for_the_same_file() {
    // Partial classes and multi-targeting both produce two <class> entries for
    // one filename; their line maps must merge, a hit anywhere winning.
    let xml = r#"<coverage><packages><package><classes>
      <class filename="src/Foo.cs"><lines>
        <line number="1" hits="0"/>
        <line number="2" hits="1"/>
      </lines></class>
      <class filename="src/Foo.cs"><lines>
        <line number="1" hits="2"/>
        <line number="3" hits="0"/>
      </lines></class>
    </classes></package></packages></coverage>"#;

    let files = parse_cobertura(xml).unwrap();
    assert_eq!(files.len(), 1, "the two classes merge into one file");
    let foo = &files[0];
    // Line 1 was hit in the second class, so the merged count is > 0.
    assert!(foo.lines[&1] > 0);
    assert_eq!(foo.lines.get(&2), Some(&1));
    assert_eq!(foo.lines.get(&3), Some(&0));
}

#[test]
fn load_report_reads_lcov_from_the_file_the_spec_names() {
    use crate::model::{CoverageFormat, CoverageSpec};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lcov.info");
    std::fs::write(&path, "SF:src/a.ts\nDA:1,2\nend_of_record\n").unwrap();

    let files = load_report(&CoverageSpec {
        path,
        format: CoverageFormat::Lcov,
    })
    .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].lines.get(&1), Some(&2));
}

#[test]
fn newest_cobertura_picks_the_most_recently_written_report() {
    let dir = tempfile::tempdir().unwrap();
    let older = dir.path().join("guid-old");
    let newer = dir.path().join("guid-new");
    std::fs::create_dir_all(&older).unwrap();
    std::fs::create_dir_all(&newer).unwrap();

    let old_file = older.join("coverage.cobertura.xml");
    let new_file = newer.join("coverage.cobertura.xml");
    std::fs::write(
        &old_file,
        r#"<coverage><packages><package><classes>
          <class filename="src/Old.cs"><lines><line number="1" hits="0"/></lines></class>
        </classes></package></packages></coverage>"#,
    )
    .unwrap();
    std::fs::write(
        &new_file,
        r#"<coverage><packages><package><classes>
          <class filename="src/New.cs"><lines><line number="1" hits="1"/></lines></class>
        </classes></package></packages></coverage>"#,
    )
    .unwrap();
    // Pin mtimes explicitly so the assertion does not race the filesystem's
    // clock resolution: the old file two minutes behind the new one.
    // (set_modified needs a write handle on Windows.)
    let now = std::time::SystemTime::now();
    let touch = |path: &std::path::Path, time: std::time::SystemTime| {
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    };
    touch(&old_file, now - std::time::Duration::from_secs(120));
    touch(&new_file, now);

    let picked = newest_cobertura(dir.path()).expect("a report exists");
    assert_eq!(picked, new_file);
}

#[test]
fn lcov_reads_line_hits_per_file() {
    let text = "TN:\n\
        SF:src/util/math.ts\n\
        DA:1,5\n\
        DA:2,0\n\
        DA:7,1\n\
        LF:3\n\
        LH:2\n\
        end_of_record\n\
        SF:src/util/other.ts\n\
        DA:4,0\n\
        end_of_record\n";

    let files = parse_lcov(text).unwrap();
    assert_eq!(files.len(), 2);

    let math = files.iter().find(|f| f.path == "src/util/math.ts").unwrap();
    assert_eq!(math.lines.get(&1), Some(&5));
    assert_eq!(math.lines.get(&2), Some(&0));
    assert_eq!(math.lines.get(&7), Some(&1));
    assert_eq!(math.lines.get(&3), None);

    let other = files
        .iter()
        .find(|f| f.path == "src/util/other.ts")
        .unwrap();
    assert_eq!(other.lines.get(&4), Some(&0));
}
