---
title: Verify Claims / ACs
id: verify-claims
---
Check whether the current diff actually does what it claims. Read-only — report, do not edit.

The run may **prepend a before/after behavioral report** as context — test result deltas, console-output differences, and HTTP responses replayed against `HEAD` versus the working tree. Treat that report as the evidence; treat the diff and its stated intents as the claims.

1. Extract the concrete, checkable claims and acceptance criteria implied by the diff and its intent messages — each phrased as a specific observable outcome ("returns 404 for an unknown id", "no longer allocates per row", "the failing test now passes").
2. Judge each claim against the provided behavioral evidence when it speaks to that claim: does a test flip, a console change, or an HTTP-response diff confirm it, contradict it, or say nothing about it?
3. For any claim the evidence does not cover, say exactly what would confirm it — a specific unit test to add, or a `.http` scenario (method, path, and the response that would prove it).

For each claim report: the claim, its verdict (**confirmed** / **contradicted** / **unverified**), and the evidence you based it on. Never mark a claim confirmed without evidence that speaks to it — an unverified claim is a correct answer; a fabricated confirmation is not. If there are no checkable claims in the diff, say so plainly.
