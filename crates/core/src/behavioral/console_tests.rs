use super::*;

fn norm() -> ConsoleNormalization {
    ConsoleNormalization::default()
}

#[test]
fn equal_after_masking_reports_no_delta() {
    // Same message, different timestamps — noise, not a behavioral change.
    let base = "2026-01-01T10:00:00 server started";
    let work = "2026-06-06T12:34:56 server started";
    let d = diff_console(base, work, &norm());
    assert!(!d.is_change(), "added={:?} removed={:?}", d.added_lines, d.removed_lines);
    assert!(d.normalized, "masking was applied");
    assert_eq!(d.confidence, Confidence::High);
}

#[test]
fn both_worktree_roots_mask_to_same_token() {
    // The two sides run in different directories; that difference must vanish.
    let base_root = r"C:\repo\.code-basics\behavioral\base\abc123";
    let work_root = r"C:\repo";
    let base = format!(r"loaded {base_root}\bin\app.dll");
    let work = format!(r"loaded {work_root}\bin\app.dll");
    let mut n = norm();
    n.roots = vec![base_root.to_string(), work_root.to_string()];
    let d = diff_console(&base, &work, &n);
    assert!(!d.is_change(), "root paths should mask to the same token: {d:?}");
}

#[test]
fn ansi_is_stripped_before_compare() {
    let base = "\x1b[31mFAILED\x1b[0m one";
    let work = "FAILED one";
    let d = diff_console(base, work, &norm());
    assert!(!d.is_change());
}

#[test]
fn a_real_change_is_reported_at_medium() {
    let base = "result: ok";
    let work = "result: error";
    let d = diff_console(base, work, &norm());
    assert!(d.is_change());
    assert_eq!(d.removed_lines, vec!["result: ok".to_string()]);
    assert_eq!(d.added_lines, vec!["result: error".to_string()]);
    // Console output is weak evidence even when clean — never High.
    assert_eq!(d.confidence, Confidence::Medium);
}

#[test]
fn heavy_masking_drops_confidence_to_low() {
    // Three of four lines are pure timestamp noise (masked); one is a genuine
    // change. Masking touched > 50% of lines, so the surviving delta is Low.
    let base = "2026-01-01T00:00:01 a\n2026-01-01T00:00:02 b\n2026-01-01T00:00:03 c\nvalue=1";
    let work = "2026-02-02T00:00:01 a\n2026-02-02T00:00:02 b\n2026-02-02T00:00:03 c\nvalue=2";
    let d = diff_console(base, work, &norm());
    assert!(d.is_change(), "value line changed");
    assert_eq!(d.confidence, Confidence::Low);
}

#[test]
fn ignore_ordering_caps_confidence_at_low() {
    let base = "line changed\nstable";
    let work = "line different\nstable";
    let mut n = norm();
    n.ignore_ordering = true;
    let d = diff_console(base, work, &n);
    assert!(d.is_change());
    assert_eq!(d.confidence, Confidence::Low);
}

#[test]
fn reordered_identical_lines_are_not_a_change() {
    // Multiset comparison: interleave order is noise.
    let base = "alpha\nbeta\ngamma";
    let work = "gamma\nalpha\nbeta";
    let d = diff_console(base, work, &norm());
    assert!(!d.is_change());
}
