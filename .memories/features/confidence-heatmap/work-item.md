# Feature: confidence-heatmap (BACKEND)

Capture per-file agent self-reported confidence via the Intent grammar and
surface it on `IntentGroup`.

## Concept
- Agents have NO structured channel to attach metadata to a tool call. The only
  authored channel is the free-text closing message parsed at `Stop` into Intent
  labels.
- So confidence is an OPTIONAL token in the Intent line — voluntary, sparse.
  MUST abstain (no value) when absent.
- DIFFERENT concept from `attribution::Confidence` (machine match-trust). Keep
  separate. Do NOT overload `IntentGroup.confidence`.

## Grammar
`Intent(path): text [confidence: low|medium|high]`
- trailing, case-insensitive `[confidence: ...]` token stripped from the label
  text before `is_usable_label` runs; recorded as `SelfConfidence`.
- unknown level -> None (abstain). No token -> None.
- low = please review closely.

## ACs
1. `SelfConfidence { Low, Medium, High }` enum (serde camelCase, Type) +
   optional `self_confidence` on `IntentLabel` (`#[serde(default,
   skip_serializing_if = "Option::is_none")]`).
2. `parse_declared_labels` strips + records the token.
3. instructions.rs SECTION + hook.rs INTENT_REQUEST tell agent about the token.
4. `IntentGroup.self_confidence` set from declared label (weakest if several).
5. Key-pinning tests updated for `selfConfidence`.
6. types.ts: `SelfConfidence` union + `selfConfidence?` on `IntentGroup`.
