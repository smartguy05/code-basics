---
title: Idempotency & Resilience Review
id: idempotency-review
---
Review the current diff for idempotency and resilience defects. Read-only — report, do not edit.

Look, in order of severity:
1. **Duplicate delivery** — a queue, stream, or webhook consumer that is unsafe when the same message is redelivered, assuming exactly-once under an at-least-once transport.
2. **Missing dedupe key** — an operation that should carry a dedupe key, idempotency key, or an outbox/inbox record but processes each call as if it were new.
3. **Partial failure** — a multi-step operation that can leave inconsistent state if it fails midway, with no compensation, transaction, or resumable checkpoint.
4. **Retry safety** — a retried call that is not safe to repeat, or a retry loop with no backoff that can amplify load against a failing dependency.
5. **Non-idempotent side effects** — a path that on replay can double charge, double send, double insert, or otherwise apply an external effect twice.

For each finding: the file and line, one sentence on the weakness, and a concrete redelivery or mid-step failure → the incorrect effect it produces. Skip effects that are already naturally idempotent. If the diff introduces no idempotency or resilience defect, say so plainly rather than inventing findings.
