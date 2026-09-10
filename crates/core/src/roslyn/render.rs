//! Turning an LSP result into the words an agent reads.
//!
//! The application answers each tool from its warm `LspHandle` and hands the
//! result here; this module — pure, in the core crate — turns it into a
//! [`super::wire::ToolAnswer`]. The runner and the app-side dispatch decide
//! nothing about the prose, exactly as the browser's [`crate::browser::render`]
//! keeps the wording out of the shim.
//!
//! # The rule that shapes every function here
//!
//! **An answer must never let an absence pass for a fact about the code.** Each
//! result carries a [`crate::lsp::model::Availability`], and the six variants stay
//! six answers:
//!
//! * [`Availability::Ready`] is the only outcome that is *data*. A `Ready` result
//!   with an empty list is a **genuine** answer — no usages, a clean file, not on
//!   a type, not inside a call — and is rendered as such, not as a refusal.
//! * Every non-`Ready` outcome becomes a refusal through
//!   [`super::answer::RoslynRefusal::from_availability`], carrying its own code
//!   and, when the result named a specific reason, that reason.
//! * A `Ready` result may still carry a `message` — a readiness caveat, or a
//!   partially refused type-hierarchy direction — and it is appended, because a
//!   count that needs qualifying and is quoted bare is a wrong claim.

use crate::lsp::model::{
    Availability, DiagnosticRow, DiagnosticSeverity, DiagnosticsResult, OverloadResult,
    TypeHierarchyResult, TypeNode, Usage, UsageResult,
};

use super::answer::RoslynRefusal;
use super::wire::ToolAnswer;

/// The `find_references` answer.
pub fn find_references(result: &UsageResult) -> ToolAnswer {
    if result.outcome != Availability::Ready {
        return unavailable(result.outcome, &result.message, &result.server);
    }

    let total = result.total.unwrap_or(0);
    if total == 0 {
        return ToolAnswer::ok(with_notes(
            "No usages: the language server found no place this symbol is used.".to_string(),
            &result.message,
            &result.server,
        ));
    }

    let mut lines = Vec::with_capacity(result.usages.len() + 2);
    if result.truncated {
        lines.push(format!(
            "{total} usage(s); the first {} are listed and the rest are not.",
            result.usages.len()
        ));
    } else {
        lines.push(format!("{total} usage(s):"));
    }
    for usage in &result.usages {
        lines.push(usage_line(usage));
    }
    ToolAnswer::ok(with_notes(
        lines.join("\n"),
        &result.message,
        &result.server,
    ))
}

fn usage_line(usage: &Usage) -> String {
    let snippet = usage.snippet.trim();
    if snippet.is_empty() {
        format!("  {}:{}", usage.label, usage.line)
    } else {
        format!("  {}:{}  {snippet}", usage.label, usage.line)
    }
}

/// The `get_diagnostics` answer.
pub fn diagnostics(result: &DiagnosticsResult) -> ToolAnswer {
    if result.outcome != Availability::Ready {
        return unavailable(result.outcome, &result.message, &result.server);
    }

    if result.diagnostics.is_empty() {
        return ToolAnswer::ok(with_notes(
            "No diagnostics: the language server reports this file is clean.".to_string(),
            &result.message,
            &result.server,
        ));
    }

    let mut lines = Vec::with_capacity(result.diagnostics.len() + 1);
    lines.push(format!("{} diagnostic(s):", result.diagnostics.len()));
    for row in &result.diagnostics {
        lines.push(diagnostic_line(row));
    }
    ToolAnswer::ok(with_notes(
        lines.join("\n"),
        &result.message,
        &result.server,
    ))
}

fn diagnostic_line(row: &DiagnosticRow) -> String {
    let mut line = format!(
        "  {} {}:{}  {}",
        severity_word(row.severity),
        row.line,
        row.character,
        row.message.trim()
    );
    match (&row.source, &row.code) {
        (Some(source), Some(code)) => line.push_str(&format!("  [{source} {code}]")),
        (Some(source), None) => line.push_str(&format!("  [{source}]")),
        (None, Some(code)) => line.push_str(&format!("  [{code}]")),
        (None, None) => {}
    }
    line
}

fn severity_word(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Information => "info",
        DiagnosticSeverity::Hint => "hint",
    }
}

/// The `get_type_hierarchy` answer.
pub fn type_hierarchy(result: &TypeHierarchyResult) -> ToolAnswer {
    if result.outcome != Availability::Ready {
        return unavailable(result.outcome, &result.message, &result.server);
    }

    let Some(item) = &result.item else {
        // A real answer: the position was not on a type. Not a failure.
        return ToolAnswer::ok(with_notes(
            "The position is not on a type, so there is no inheritance to report.".to_string(),
            &result.message,
            &result.server,
        ));
    };

    let mut lines = vec![format!("Type {}", node_line(item))];
    // An empty list means "there are none" only when no message qualifies it —
    // a server that refused one direction keeps Ready and names it in `message`,
    // which `with_notes` appends, so here an empty list is stated plainly.
    lines.push(direction("Supertypes", &result.supertypes));
    lines.push(direction("Subtypes", &result.subtypes));
    ToolAnswer::ok(with_notes(
        lines.join("\n"),
        &result.message,
        &result.server,
    ))
}

fn direction(heading: &str, nodes: &[TypeNode]) -> String {
    if nodes.is_empty() {
        return format!("{heading}: none.");
    }
    let mut lines = vec![format!("{heading}:")];
    for node in nodes {
        lines.push(format!("  {}", node_line(node)));
    }
    lines.join("\n")
}

fn node_line(node: &TypeNode) -> String {
    let kind = kind_word(node.kind);
    let detail = node
        .detail
        .as_deref()
        .map(|detail| format!(" — {detail}"))
        .unwrap_or_default();
    format!(
        "{} ({kind}){detail}  {}:{}",
        node.name, node.label, node.line
    )
}

fn kind_word(kind: crate::symbols::declarations::SymbolKind) -> String {
    format!("{kind:?}").to_lowercase()
}

/// The `resolve_overloads` answer.
pub fn overloads(result: &OverloadResult) -> ToolAnswer {
    if result.outcome != Availability::Ready {
        return unavailable(result.outcome, &result.message, &result.server);
    }

    if result.signatures.is_empty() {
        return ToolAnswer::ok(with_notes(
            "The position is not inside a call, so there are no overloads to report.".to_string(),
            &result.message,
            &result.server,
        ));
    }

    let mut lines = vec![format!("{} signature(s):", result.signatures.len())];
    for (index, signature) in result.signatures.iter().enumerate() {
        let active = result.active_signature == Some(index as u32);
        let marker = if active { "→ " } else { "  " };
        lines.push(format!("{marker}{}", signature.label));
        if active {
            if let Some(parameter) = result.active_parameter {
                if let Some(info) = signature.parameters.get(parameter as usize) {
                    lines.push(format!("     active parameter: {}", info.label));
                }
            }
        }
    }
    if result.active_signature.is_none() {
        lines.push(
            "The server did not mark an active signature, so none is highlighted above."
                .to_string(),
        );
    }
    ToolAnswer::ok(with_notes(
        lines.join("\n"),
        &result.message,
        &result.server,
    ))
}

/// The refusal for a non-`Ready` outcome.
///
/// The generic sentence comes from [`RoslynRefusal`]; a specific reason the
/// result named (the LSP model's own, curated message) is preferred when present.
fn unavailable(
    outcome: Availability,
    message: &Option<String>,
    server: &Option<String>,
) -> ToolAnswer {
    let refusal = RoslynRefusal::from_availability(outcome)
        .expect("unavailable is only called for a non-Ready outcome");
    // A `Failed` result's `message` is the language server's own raw error text —
    // its stderr tail, file paths, OS error strings — so forwarding it would
    // break this module's "no internal error text" guarantee (and answer.rs's own
    // Failed sentence, which says the server's words stay in the application). The
    // curated readiness messages (starting / loading / unsupported / not
    // configured) are safe to prefer over the generic sentence; the Failed one is
    // not, so it always falls back to the generic sentence.
    let base = match (outcome, message) {
        (Availability::Failed, _) => refusal.sentence(),
        (_, Some(reason)) if !reason.trim().is_empty() => reason.trim().to_string(),
        _ => refusal.sentence(),
    };
    ToolAnswer::refused(refusal.code(), append_server(base, server))
}

/// Append a `Ready` result's qualifying message and the answering server.
fn with_notes(body: String, message: &Option<String>, server: &Option<String>) -> String {
    let mut out = body;
    if let Some(message) = message {
        if !message.trim().is_empty() {
            out.push_str(&format!("\nNote: {}", message.trim()));
        }
    }
    append_server(out, server)
}

fn append_server(body: String, server: &Option<String>) -> String {
    match server {
        Some(server) if !server.trim().is_empty() => format!("{body}\n(server: {})", server.trim()),
        _ => body,
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
