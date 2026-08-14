//! Talking to real language servers, so "find usages" can be right.
//!
//! # Why this exists next to `symbols/`
//!
//! [`crate::symbols`] answers "what does this workspace declare?" with a text
//! heuristic — one line in, a name and a kind out. That is the right tool for a
//! search palette, where a near miss costs a keystroke, and it is the wrong tool
//! for *usages*. A text scan cannot tell `Order.Total` from `Invoice.Total`, and
//! a count of use sites is a much stronger claim than a palette row: the user
//! reads "3 usages" and concludes it is safe to change the method. A confidently
//! wrong number is worse than no number, which is this repository's governing
//! rule and is sharper here than almost anywhere else in it.
//!
//! So the answers come from the compiler front ends that already exist —
//! `Microsoft.CodeAnalysis.LanguageServer` for C#, `rust-analyzer`,
//! `typescript-language-server`, a Python server — over LSP. Nothing is bundled;
//! servers are found on disk or on `PATH`, and a missing one is **reported**, not
//! worked around. `symbols/` stays exactly as it is: the two answer different
//! questions and neither is a fallback for the other.
//!
//! # Layering
//!
//! The lower four modules are pure — no process, no I/O, no async — because that
//! is where the errors that cannot be seen live, and they are the ones worth
//! testing exhaustively:
//!
//! * [`framing`] — `Content-Length` framing over bytes. Fails closed: a stream
//!   that cannot be resynchronised is not guessed at.
//! * [`jsonrpc`] — the envelope. Tells a server *request* (must be answered, or
//!   the server hangs) from a *notification* (must not be).
//! * [`uri`] — paths ⇄ `file:` URIs, per-server spelling, and the rule that
//!   identity is decided on paths and never on URI strings.
//! * [`positions`] — UTF-16 code units ⇄ bytes, 0-based ⇄ 1-based lines, and
//!   snippet trimming. Everything clamps; nothing panics.
//!
//! Three more sit on top of those, still pure — no process and no I/O, because
//! deciding *what to send* and *which server to send it to* is exactly the part
//! that must be provable without a server installed:
//!
//! * [`protocol`] — the nine messages this feature uses, hand-written rather
//!   than generated so each field can be permissive where real servers need it
//!   to be. Abstains by returning a `Result`: `null` and `[]` are an empty
//!   answer, and a shape matching none of the legal ones is an **error**, never
//!   an empty list — that flattening is precisely how a subsystem reports "0
//!   usages" about a method with forty.
//! * [`registry`] — which server serves a file, where it is, and what to say
//!   when it is not there. The environment is an injected [`registry::Probe`],
//!   so every rule is decided headlessly. Abstains four ways rather than one:
//!   an unknown extension gets **no** server rather than a default one, and
//!   disabled, misconfigured and absent stay three different things to tell
//!   somebody — with a configured `program` that does not resolve failing
//!   outright rather than quietly starting a different server.
//! * [`settings`] — the `lsp` block of `.code-basics/config.json`. Every field
//!   is optional and every absence means "the built-in default", never
//!   "nothing"; unknown keys load rather than fail, because this file is
//!   committed and shared with a team on other builds.
//!
//! Three more are pure as well, and are the ones a wrong answer would travel
//! through — what the server was told, what a payload means, and what the
//! frontend is handed:
//!
//! * [`documents`] — what each server has been told about each open document, as
//!   a set of *owed notifications*; it decides and never sends. Its rules are all
//!   ordering rules (open before change, never open twice, versions never go
//!   backwards, catch a late server up), which are provable in microseconds here
//!   and close to unprovable against a real server. It abstains by yielding **no
//!   actions** for a change to a document nobody opened, rather than fabricating
//!   an open: an invented open sends text under a version nobody agreed on and
//!   hides the caller's bug behind an answer that happens to look right.
//! * [`results`] — LSP payloads in, the types the frontend reads out. Pure, with
//!   file text injected through [`results::TextProvider`], because the mistakes it
//!   can make are the invisible kind. It abstains per *row* rather than per
//!   answer: a location that does not resolve under the root keeps its row with
//!   no path (dropping it would make the count wrong, joining it onto the root
//!   would open a different file), a line that cannot be read yields no snippet
//!   rather than an empty one, a highlight the trim cut into is **dropped** rather
//!   than shifted, a declaration whose kind cannot be placed earns no inline row,
//!   and the cap shortens the list while never touching the count.
//! * [`model`] — the ten types that cross IPC, mirrored by hand in
//!   `src/ipc/types.ts`. It abstains structurally: [`model::Availability`] has six
//!   variants so that "not configured", "starting", "loading", "died" and "does
//!   not support this" cannot collapse into each other or into a zero, `total` is
//!   an `Option` so `Some(0)` and `None` stay different answers, and there is **no
//!   `skip_serializing_if` anywhere in it** so that "no answer" cannot arrive as a
//!   missing key. Its one asymmetry — 1-based lines, 0-based UTF-16 columns — is
//!   restated on every field it touches.
//!
//! Only the top three touch a process, and they are the ones whose mistakes are
//! invisible rather than wrong — a leaked server tree, a waiter nobody wakes:
//!
//! * [`transport`] — one live process, and the bytes going in and out of it.
//!   Frames, ids, timeouts and death; no LSP semantics at all. It abstains by
//!   never letting a failure become an answer **and never letting one failure
//!   wear another's clothes**: a timeout, a death, a cancellation, a server
//!   error and an unreadable payload stay five things, so an answer that could
//!   not be read fails its own waiter by id rather than being dropped into a
//!   timeout, and a stdout we can no longer read publishes a death rather than
//!   going quiet. Its other invariant is not about honesty but about hygiene:
//!   **no path may leave a server process behind**, including the write failure
//!   whose death does not mean the process has ended.
//! * [`client`] — one server, from handshake to shutdown, as questions and
//!   answers. It owns meaning: what the capabilities permit, whether the server
//!   is primed yet, and what each document has been told. It abstains three
//!   ways — an unadvertised capability is `Unsupported` and never `[]`, a
//!   server still loading is `StillLoading` and never `[]`, and a server that
//!   *died* while loading is left `Loading` rather than promoted at the
//!   readiness ceiling, because "ready, with a caveat" said about a corpse is
//!   the one caveat a UI would render as a working server. The readiness signal
//!   itself is read from the transport's **sticky record** rather than from the
//!   notification stream, which is a `broadcast` a slow subscriber may lag past —
//!   and the message that says a solution is loaded is sent once. The ceiling is
//!   a bound on our patience and not a verdict: the caveat is withdrawn if the
//!   signal arrives after it.
//! * [`session`] — every server for one workspace, behind one actor, and the only
//!   module a Tauri command talks to. It owns lifetime and lateness: which server
//!   is asked, when it is started (lazily, and a start is **never** awaited), what
//!   to say while it is not up, and what a workspace swap means. It abstains by
//!   turning each of those states into its own sentence — an extension nobody
//!   claims names the extension, a resolution failure carries where we looked, a
//!   start in flight is `Starting` and not `[]`, a server still priming is
//!   `Loading`, a dead one is `Failed` with its own last words, and a server
//!   promoted at the readiness ceiling still answers but says the count may be
//!   low — in the answer's `message` *and* in its status row's `caveat`, which is
//!   a field of its own because `detail` already means the program path and a
//!   promotion nobody can see is the promotion being silent. Two rules here are
//!   specific to owning several servers at once: a
//!   partially refused goto answer keeps `Ready` and names the refused group in
//!   its message (one outcome, three lists), and a wholly refused one takes the
//!   *most severe* reason rather than the first.
//!
//! # The abstain rule, in this module's terms
//!
//! A timeout, a dead server, a server still loading its projects, and a server
//! that genuinely found nothing are **four different answers** and must never
//! collapse into "0 usages". Anything that cannot become a real count has to
//! reach the user as the reason it could not.

pub mod client;
pub mod documents;
pub mod framing;
pub mod jsonrpc;
pub mod model;
pub mod positions;
pub mod protocol;
pub mod registry;
pub mod results;
pub mod session;
pub mod settings;
pub mod transport;
pub mod uri;
