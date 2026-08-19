---
title: Security Review
id: security-review
---
Review the current diff for security defects. Read-only — report, do not edit.

Look, in order of severity:
1. **Injection** — SQL/command/template/log input concatenated unescaped into an interpreter.
2. **Authz/authn gaps** — a route, handler, or operation that skips an ownership or permission check the surrounding code applies elsewhere.
3. **Secret handling** — a credential, token, or key logged, echoed, committed, or sent to a client.
4. **Unsafe deserialization** — untrusted bytes fed to a deserializer that can construct arbitrary types or run code.
5. **SSRF / path traversal** — a user-controlled value reaching a URL fetch or a filesystem path without allow-listing or normalization.
6. **Missing validation** — input crossing a trust boundary that is used before its shape, range, or origin is checked.

For each finding: the file and line, one sentence on the weakness, and a concrete attacker-supplied input → the unsafe effect it produces. Skip theoretical worries with no reachable path. If the diff introduces no security defect, say so plainly rather than inventing findings.
