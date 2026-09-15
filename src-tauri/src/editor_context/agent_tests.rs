use super::*;

use cb_core::editor_context::tools::EditorToolCall;
use cb_core::model::{EditorContext, EditorOpenFile};

/// A context with one active file open, enough to tell a data answer apart from a
/// refusal.
fn a_context() -> EditorContext {
    EditorContext {
        active_file: Some("src/main.rs".to_string()),
        cursor: None,
        viewport: None,
        selection: None,
        open_files: vec![EditorOpenFile {
            path: "src/main.rs".to_string(),
            active: true,
            dirty: false,
            pinned: false,
        }],
        recent_files: Vec::new(),
    }
}

/// The feature being off is its own refusal with its own code — never confused for
/// "nothing pushed yet", because the fix differs (switch it on vs. open a file).
/// It is refused **even when a context is present**, because that context may be
/// one pushed before the feature was switched off and must not be served.
#[test]
fn the_feature_off_refuses_as_disabled_even_with_a_context_present() {
    let result = answer(false, Some(a_context()), EditorToolCall::GetActiveFile);
    assert!(!result.ok);
    assert_eq!(result.code.as_deref(), Some("featureDisabled"));
}

/// The feature on with nothing pushed is `noContext`, distinct from `disabled`.
#[test]
fn the_feature_on_with_no_context_refuses_as_no_context() {
    let result = answer(true, None, EditorToolCall::GetActiveFile);
    assert!(!result.ok);
    assert_eq!(result.code.as_deref(), Some("noContext"));
}

/// `disabled` and `noContext` are never the same answer.
#[test]
fn disabled_and_no_context_are_distinct_codes() {
    let disabled = answer(false, None, EditorToolCall::GetOpenFiles);
    let no_context = answer(true, None, EditorToolCall::GetOpenFiles);
    assert_ne!(disabled.code, no_context.code);
}

/// With the feature on and a context present, the call reaches the renderer and a
/// data answer comes back.
#[test]
fn an_enabled_call_with_a_context_renders_data() {
    let result = answer(true, Some(a_context()), EditorToolCall::GetActiveFile);
    assert!(result.ok);
    assert!(result.text.contains("src/main.rs"), "{}", result.text);
}

/// Each tool routes to its own renderer: `get_open_files` on the same context does
/// not answer as `get_active_file`.
#[test]
fn each_tool_routes_to_its_own_renderer() {
    let context = a_context();
    let open = answer(true, Some(context.clone()), EditorToolCall::GetOpenFiles);
    assert!(open.ok);
    assert!(open.text.contains("open file"), "{}", open.text);

    // A genuine empty (no recent files) is a data answer, not a refusal — the
    // render layer's rule, exercised through the dispatch.
    let recent = answer(true, Some(context), EditorToolCall::GetRecentFiles);
    assert!(recent.ok);
    assert!(recent.code.is_none());
}
