# plan — confidence-heatmap BACKEND (DONE)

Grammar token `[confidence: low|medium|high]` appended to an `Intent:` line is
the only authored channel (free-text closing message). Parsed at Stop.

1. mod.rs: SelfConfidence enum + optional field on IntentLabel. [done]
2. hook.rs: strip token in parse_declared_labels; abstain on unknown; thread
   through parse_labels_with_source -> ingest_label. INTENT_REQUEST wording. [done]
3. instructions.rs SECTION wording (sync w/ INTENT_REQUEST). [done]
4. grouping.rs: IntentGroup.self_confidence from declared label via (turn,label)
   map; most-cautious merge; never touch attribution confidence. [done]
5. key-pinning + round-trip tests (grouping_tests, intents_tests, hook_tests). [done]
6. types.ts SelfConfidence union + optional field. [done]
7. docs:index regenerated. [done]

Key decisions:
- Separate concept from attribution::Confidence — never overloaded.
- Abstain everywhere: no token/unknown level -> None; ambiguous card -> None;
  inferred/derived/non-intent -> None. Unknown level leaves text intact.
- self_confidence sourced from intents.labels (spans lack it), keyed (turn,label).
