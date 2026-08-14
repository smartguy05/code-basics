//! The LSP messages this app actually uses, and nothing else.
//!
//! # Why this is a hand-written subset and not `lsp-types`
//!
//! The specification has hundreds of types and this feature sends nine messages.
//! What it needs from a model is not coverage but *permissiveness in exactly the
//! right places*, and that is a per-field judgement no generated model makes:
//! every decoder here accepts more shapes than the specification requires,
//! because real servers send them, while refusing to invent a value it was not
//! given. A dependency would also decide the framing, the async runtime and the
//! error types for us — see the module doc of [`super`] for why those are ours.
//!
//! # The two rules every decoder in here follows
//!
//! **Unknown fields are ignored.** Not one struct carries
//! `deny_unknown_fields`. The real Roslyn server puts a non-standard `glyph` on
//! every `documentSymbol` node, `_vs_`-prefixed capabilities in its handshake and
//! a `_roslyn_processId` beside them. A decoder written against the specification
//! rather than against the servers that exist would reject all three.
//!
//! **An empty answer and an unreadable answer are different answers.** So
//! [`decode_goto`] and [`decode_document_symbols`] return a `Result`: `null` and
//! `[]` both decode to an empty vector, and a shape matching none of the legal
//! ones is a [`DecodeError`]. Flattening the second case into the first is how a
//! subsystem tells a user "0 usages" about a method that has forty, which is the
//! one failure this whole module exists to prevent.
//!
//! # What is deliberately not here
//!
//! No process, no I/O, no async, no server-specific behaviour: this module is
//! pure so the shapes can be tested against captured payloads headlessly, which
//! is the only way the awkward ones get covered at all. Sending lives in
//! [`super::jsonrpc`], framing in [`super::framing`], and the path⇄URI rules in
//! [`super::uri`] — a URI is an opaque `String` in here on purpose, because
//! identity is decided on paths and never on URI strings.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub use crate::symbols::declarations::SymbolKind;

use super::positions::byte_to_utf16;

/// Every method name the subsystem sends or receives.
///
/// A method name is the one string here that no type can check and that fails
/// *quietly* when wrong: the server answers `-32601` and the feature reports
/// nothing rather than crashing. Spelling each exactly once, in one place, is
/// what turns a typo into a compile error at the call site.
pub mod method {
    // Client → server: lifecycle.
    pub const INITIALIZE: &str = "initialize";
    pub const INITIALIZED: &str = "initialized";
    pub const SHUTDOWN: &str = "shutdown";
    pub const EXIT: &str = "exit";

    // Client → server: document sync. Sending these is not optional — a server
    // answers questions about the buffer it was told about, not the file on disk.
    pub const DID_OPEN: &str = "textDocument/didOpen";
    pub const DID_CHANGE: &str = "textDocument/didChange";
    pub const DID_CLOSE: &str = "textDocument/didClose";

    // Client → server: the questions this feature exists to ask.
    pub const REFERENCES: &str = "textDocument/references";
    pub const DEFINITION: &str = "textDocument/definition";
    pub const IMPLEMENTATION: &str = "textDocument/implementation";
    pub const TYPE_DEFINITION: &str = "textDocument/typeDefinition";
    pub const DOCUMENT_SYMBOL: &str = "textDocument/documentSymbol";
    pub const CANCEL_REQUEST: &str = "$/cancelRequest";

    // Server → client **requests**: each one must be answered or the server
    // blocks forever. Verified against the real Roslyn server, which issues
    // `window/workDoneProgress/create` and `workspace/configuration` during
    // start-up and waits — and the wait is indistinguishable from a slow
    // project load.
    pub const CONFIGURATION: &str = "workspace/configuration";
    pub const REGISTER_CAPABILITY: &str = "client/registerCapability";
    pub const UNREGISTER_CAPABILITY: &str = "client/unregisterCapability";
    pub const SHOW_MESSAGE_REQUEST: &str = "window/showMessageRequest";
    pub const WORK_DONE_PROGRESS_CREATE: &str = "window/workDoneProgress/create";
    /// Answered `{"applied": false}`: this app never lets a server edit files
    /// behind the user's back.
    pub const APPLY_EDIT: &str = "workspace/applyEdit";

    // Server → client **notifications**: must *not* be answered.
    pub const LOG_MESSAGE: &str = "window/logMessage";
    pub const PROGRESS: &str = "$/progress";
}

/// A point in a document, as LSP counts.
///
/// Both fields are **0-based**, and `character` is a count of **UTF-16 code
/// units** — not bytes and not `char`s, which disagree on any line with
/// non-ASCII in it. [`super::positions`] holds every conversion to and from what
/// this app uses (1-based lines, byte offsets); nothing else should do the
/// arithmetic, because an off-by-one here is invisible until it puts the cursor
/// somewhere the user did not ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// A half-open span between two [`Position`]s: `end` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// A range in a named document.
///
/// `uri` stays a `String`. It is the server's own spelling — Roslyn emits a
/// plain colon and an upper-case drive, rust-analyzer percent-encodes the colon —
/// and normalising it here would hide the fact that two spellings of one path
/// must never be compared as strings. [`super::uri`] converts to a path, and
/// paths are what get compared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// The richer goto answer a server may send when the client offers `linkSupport`.
///
/// Kept as its own type rather than decoded straight into [`Location`] so the
/// distinction between the two ranges is visible where it matters: see
/// [`LocationLink::aim`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationLink {
    pub target_uri: String,
    /// The whole declaration, braces and doc comment included.
    pub target_range: Range,
    /// The identifier alone. Optional only to survive a server that omits a
    /// field the specification requires.
    pub target_selection_range: Option<Range>,
}

impl LocationLink {
    /// Where to actually put the cursor.
    ///
    /// `targetSelectionRange` wins: `targetRange` spans the entire declaration,
    /// so jumping to its start lands on `public`, on an attribute, or on a brace.
    /// That is the difference between navigating *to* a symbol and navigating
    /// near it. Falling back to the whole range when the selection range is
    /// absent costs precision and keeps the answer.
    fn aim(self) -> Location {
        Location {
            uri: self.target_uri,
            range: self.target_selection_range.unwrap_or(self.target_range),
        }
    }
}

/// A response arrived that matched none of the shapes the protocol allows.
///
/// Deliberately not convertible into an empty result. `what` names the request
/// so the message reaching the UI says which question could not be answered
/// rather than that "something" failed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("the {what} response matched none of the shapes the protocol allows: {detail}")]
    Shape { what: &'static str, detail: String },
}

impl DecodeError {
    fn shape(what: &'static str, value: &Value) -> Self {
        // The value itself is not logged: a `documentSymbol` answer for a large
        // file is megabytes, and the useful part is which key set arrived.
        let detail = match value {
            Value::Object(map) => {
                let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
                keys.sort_unstable();
                format!("an object with keys [{}]", keys.join(", "))
            }
            Value::Array(items) => format!("an array of {} unreadable items", items.len()),
            other => format!("a bare {}", type_name(other)),
        };
        Self::Shape { what, detail }
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Every shape a `definition` / `implementation` / `typeDefinition` /
/// `references` response is allowed to take.
///
/// `untagged` because nothing on the wire discriminates them — serde tries each
/// in order and the first that decodes wins. The order matters exactly once:
/// `[]` is matched by `Locations` before `Links`, which is harmless because both
/// mean "none". `Location` and `LocationLink` cannot be confused, since their
/// field *names* differ (`uri` versus `targetUri`).
#[derive(Deserialize)]
#[serde(untagged)]
enum GotoResponse {
    /// `null` — a real answer meaning "nowhere to go", not a missing one.
    Null,
    Single(Location),
    Locations(Vec<Location>),
    Links(Vec<LocationLink>),
}

/// Read a goto-style response into locations, or say why it could not be read.
///
/// An `Err` is not a dressed-up empty list: an empty result invites the user to
/// conclude the symbol is unused, and only the error tells the UI that the
/// question went unanswered. One unreadable element fails the whole array for
/// the same reason — a short list is a wrong count, silently.
pub fn decode_goto(value: Value) -> Result<Vec<Location>, DecodeError> {
    match serde_json::from_value::<GotoResponse>(value.clone()) {
        Ok(GotoResponse::Null) => Ok(Vec::new()),
        Ok(GotoResponse::Single(one)) => Ok(vec![one]),
        Ok(GotoResponse::Locations(many)) => Ok(many),
        Ok(GotoResponse::Links(links)) => Ok(links.into_iter().map(LocationLink::aim).collect()),
        // The method name is `definition` for all four requests on purpose: the
        // shapes are identical, so naming one of them keeps the message concrete
        // without claiming to know which request this was.
        Err(_) => Err(DecodeError::shape(method::DEFINITION, &value)),
    }
}

/// One declaration in a file, with the chain of declarations enclosing it.
///
/// Flat rather than nested because the consumer's question is flat: "which
/// declarations are in this file, and on what line". The nesting survives as
/// [`Symbol::container`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Exactly what the server sent. Roslyn puts a whole signature here —
    /// `TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool` — and
    /// trimming it is a presentation decision: a picker row wants the overload,
    /// an inline row wants the bare name. Deciding here would destroy what the
    /// other consumer needs.
    pub name: String,
    pub detail: Option<String>,
    pub kind: SymbolKind,
    /// The whole declaration. What decides whether it is on screen.
    pub range: Range,
    /// The identifier alone. **Where a `references` request must be aimed** — at
    /// the name, not at the `class` keyword the declaration range starts on.
    pub selection_range: Range,
    /// Enclosing declaration names, outermost first.
    pub container: Vec<String>,
    /// Set only by the flat shape, which names a document per symbol. `None`
    /// means the document that was asked about, which is the hierarchical
    /// shape's only possible answer.
    pub uri: Option<String>,
}

/// The hierarchical shape: `DocumentSymbol[]`, what the real Roslyn server sends.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentSymbolNode {
    name: String,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    kind: u32,
    range: Range,
    selection_range: Range,
    #[serde(default)]
    children: Vec<DocumentSymbolNode>,
}

/// The flat, deprecated shape: `SymbolInformation[]`. Still what several shipped
/// servers answer, so it is not optional to support.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SymbolInformation {
    name: String,
    #[serde(default)]
    kind: u32,
    location: Location,
    #[serde(default)]
    container_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SymbolsResponse {
    Null,
    /// First: `DocumentSymbolNode` requires `range` *and* `selectionRange`, so a
    /// flat payload cannot accidentally match it.
    Hierarchical(Vec<DocumentSymbolNode>),
    Flat(Vec<SymbolInformation>),
}

/// Read a `documentSymbol` response, whichever of its two shapes arrived.
pub fn decode_document_symbols(value: Value) -> Result<Vec<Symbol>, DecodeError> {
    match serde_json::from_value::<SymbolsResponse>(value.clone()) {
        Ok(SymbolsResponse::Null) => Ok(Vec::new()),
        Ok(SymbolsResponse::Hierarchical(nodes)) => {
            let mut out = Vec::new();
            flatten(&nodes, &mut Vec::new(), &mut out);
            Ok(out)
        }
        Ok(SymbolsResponse::Flat(items)) => Ok(items
            .into_iter()
            .map(|item| Symbol {
                name: item.name,
                detail: None,
                kind: symbol_kind(item.kind),
                range: item.location.range,
                // There is no identifier range in this shape and nothing to
                // derive one from, so the declaration range serves as both.
                // That aims `references` at the start of the declaration — a
                // worse anchor than the identifier, and an honest one. Guessing
                // the name's offset by scanning the line would be a guess about
                // a file this module has not read.
                selection_range: item.location.range,
                // One element, not split on `.`: the server sent one opaque
                // string, and splitting it would invent a nesting depth it
                // never stated.
                container: item.container_name.into_iter().collect(),
                uri: Some(item.location.uri),
            })
            .collect()),
        Err(_) => Err(DecodeError::shape(method::DOCUMENT_SYMBOL, &value)),
    }
}

/// Depth-first, parent before children, so the output runs roughly down the file.
fn flatten(nodes: &[DocumentSymbolNode], chain: &mut Vec<String>, out: &mut Vec<Symbol>) {
    for node in nodes {
        out.push(Symbol {
            name: node.name.clone(),
            detail: node.detail.clone(),
            kind: symbol_kind(node.kind),
            range: node.range,
            selection_range: node.selection_range,
            container: chain.clone(),
            uri: None,
        });
        chain.push(node.name.clone());
        flatten(&node.children, chain, out);
        chain.pop();
    }
}

/// An LSP `SymbolKind` number as one of this app's own kinds.
///
/// The mapping exists so LSP results reuse the palette's badges and the
/// `types.ts` mirror instead of introducing a second kind enum that would have
/// to be kept in step with it. It is lossy on purpose — [`SymbolKind`] is
/// deliberately coarse, because the point is a badge and not a type system.
///
/// LSP numbers kinds 1..=26 and adds to the end, so **anything else is
/// [`SymbolKind::Other`]**, which renders as no badge at all. That is the same
/// abstention [`crate::symbols::declarations`] makes for a line it cannot place:
/// a future kind must show nothing rather than the nearest thing that fits.
pub fn symbol_kind(number: u32) -> SymbolKind {
    match number {
        // 2 Module is Rust's `mod` and TypeScript's `namespace`; both are what
        // this app calls a namespace.
        2 | 3 => SymbolKind::Namespace,
        5 => SymbolKind::Class,
        // Method, Constructor and Function are one badge: the palette's
        // `Function` is "callable", and separating them would need three badges
        // to say something no user is scanning for.
        6 | 9 | 12 => SymbolKind::Function,
        // A property and a field are named storage. `Variable` is the closest
        // thing this enum has, and there is no `Property` to add without
        // changing the wire type and the frontend mirror.
        7 | 8 | 13 => SymbolKind::Variable,
        10 => SymbolKind::Enum,
        // 11 Interface is also what rust-analyzer reports a **trait** as: LSP
        // has no trait kind. So `SymbolKind::Trait` is unreachable from here,
        // and inferring it from the file extension would be a guess made in the
        // wrong module.
        11 => SymbolKind::Interface,
        // An enum member is a named constant, which is exactly what it is.
        14 | 22 => SymbolKind::Constant,
        23 => SymbolKind::Struct,
        26 => SymbolKind::Type,
        // Everything left is deliberate silence rather than an oversight:
        // 1 File and 4 Package are not declarations in a file; 15..=21
        // (String/Number/Boolean/Array/Object/Key/Null) are what a server
        // answers for a `.json` document, and they are values, not declarations;
        // 24 Event and 25 Operator have no badge and no near neighbour worth
        // borrowing.
        _ => SymbolKind::Other,
    }
}

/// How much of a document the server wants to be told about on every edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    /// The server does not want document sync. It is then **refused**: we cannot
    /// keep it in step with the buffer, so nothing it says about the buffer can
    /// be trusted.
    None,
    Full,
    Incremental,
}

/// The handshake fields that gate a feature, and only those.
///
/// Everything else the server advertised is dropped. Roslyn sends thirty-odd
/// capabilities and this feature acts on seven; decoding the rest would be
/// surface area that nothing reads and that has to keep decoding as servers
/// change.
///
/// Each provider is a plain `bool` because the caller's question is binary, but
/// the *decode* is not: a provider is legally `true` **or** an options object,
/// and the real `referencesProvider` is `{"workDoneProgress": true}`. Reading
/// that as `false` would disable "find usages" against the one server this was
/// built for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCapabilities {
    pub references: bool,
    pub definition: bool,
    pub implementation: bool,
    pub type_definition: bool,
    pub document_symbol: bool,
    pub sync: SyncKind,
    /// Exactly what the server said, `None` when it said nothing.
    ///
    /// Absence is **not** a refusal — the real Roslyn server omits the field
    /// entirely, and UTF-16 is the protocol's default and what we asked for.
    /// Keeping the raw string rather than a bool means the refusal message can
    /// name the encoding the server insisted on.
    pub position_encoding: Option<String>,
}

impl ServerCapabilities {
    /// Pull the gating fields out of an `initialize` result.
    ///
    /// A missing `capabilities` object is an `Err`, not a server that provides
    /// nothing: the field is required, so its absence means the handshake was
    /// not understood at all, and those two facts deserve different messages.
    pub fn from_initialize_result(value: &Value) -> Result<Self, DecodeError> {
        let capabilities = value
            .get("capabilities")
            .filter(|c| c.is_object())
            .ok_or_else(|| DecodeError::shape(method::INITIALIZE, value))?;

        Ok(Self {
            references: provides(capabilities.get("referencesProvider")),
            definition: provides(capabilities.get("definitionProvider")),
            implementation: provides(capabilities.get("implementationProvider")),
            type_definition: provides(capabilities.get("typeDefinitionProvider")),
            document_symbol: provides(capabilities.get("documentSymbolProvider")),
            sync: sync_kind(capabilities.get("textDocumentSync")),
            position_encoding: capabilities
                .get("positionEncoding")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    /// Whether columns computed as UTF-16 code units are what this server means.
    ///
    /// Silence counts as yes: UTF-16 is the protocol default. Anything else is a
    /// no, and the caller refuses the server rather than proceeding — every
    /// column would be wrong on every non-ASCII line, and wrong invisibly.
    pub fn encoding_is_utf16(&self) -> bool {
        match self.position_encoding.as_deref() {
            None | Some("utf-16") => true,
            Some(_) => false,
        }
    }
}

/// Does the server provide this, given a field that is legally `bool` or options?
///
/// Anything else — a string, a number, a null, an absent key — is `false`. Not
/// because absence and nonsense are the same thing, but because both leave us
/// without a claim, and claiming a feature on a value we could not read means
/// sending requests the server will reject.
fn provides(field: Option<&Value>) -> bool {
    match field {
        Some(Value::Bool(yes)) => *yes,
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

/// Resolve `textDocumentSync`, which is legally a number or an object.
fn sync_kind(field: Option<&Value>) -> SyncKind {
    let number = match field {
        Some(Value::Number(n)) => n.as_u64(),
        // Per the specification `change` defaults to none when the object omits
        // it. Reading the default as `Full` would let us send notifications the
        // server is entitled to ignore, and the desynchronisation is silent.
        Some(Value::Object(options)) => options.get("change").and_then(Value::as_u64),
        _ => None,
    };
    match number {
        Some(1) => SyncKind::Full,
        Some(2) => SyncKind::Incremental,
        // Includes 0, an absent field and a kind added after this was written:
        // all of them leave us unable to promise the server a synchronised
        // buffer, which is the thing `None` means to the caller.
        _ => SyncKind::None,
    }
}

/// The `initialize` params, as JSON.
///
/// A builder returning [`Value`] rather than a struct: the capabilities object is
/// a nested literal that no Rust type would make clearer, and the one property
/// worth testing is the exact JSON that goes out. Pinning that JSON whole is
/// also the only way to catch the dangerous change here, which is declaring an
/// *extra* capability — a server would then send registrations and requests we
/// do not handle, and no assertion over the fields we meant to declare would
/// notice.
///
/// `process_id` is how a server decides to exit when its client dies, so the key
/// is always present; `null` is the spelling for "do not watch anyone".
pub fn initialize_params(process_id: Option<u32>, root_uri: &str, root_name: &str) -> Value {
    serde_json::json!({
        "processId": process_id,
        "clientInfo": { "name": "code-basics", "version": env!("CARGO_PKG_VERSION") },
        "rootUri": root_uri,
        "workspaceFolders": [{ "uri": root_uri, "name": root_name }],
        "capabilities": {
            // Only `utf-16`, and a server answering anything else is refused
            // rather than negotiated with — see `ServerCapabilities::
            // encoding_is_utf16`. It is free on the frontend, where CodeMirror
            // offsets already are UTF-16 code units.
            "general": { "positionEncodings": ["utf-16"] },
            "textDocument": {
                // Every `false` is a promise kept: this client never sends
                // willSave/didSave and does not handle dynamic registration of
                // sync, so advertising any of them would invite messages that
                // would go unanswered.
                "synchronization": {
                    "dynamicRegistration": false,
                    "willSave": false,
                    "willSaveWaitUntil": false,
                    "didSave": false
                },
                // `linkSupport` on all three gotos: a `LocationLink` carries the
                // identifier range separately, which is a better cursor target
                // than the whole declaration. The server may still answer with
                // plain locations — the real one did — which is why
                // `decode_goto` accepts both.
                "definition": { "linkSupport": true },
                "implementation": { "linkSupport": true },
                "typeDefinition": { "linkSupport": true },
                "references": { "dynamicRegistration": false },
                // The hierarchical shape is the one that carries
                // `selectionRange`, which is the anchor a `references` request
                // needs. The flat shape is still decoded, for servers that
                // ignore this.
                "documentSymbol": { "hierarchicalDocumentSymbolSupport": true }
            },
            "workspace": {
                // Roslyn *blocks* on `workspace/configuration` during start-up,
                // so this is not a nicety.
                "configuration": true,
                "workspaceFolders": true,
                // Declared and false: this app never lets a server edit files
                // behind the user's back, and saying so up front is better than
                // refusing each request after the fact.
                "applyEdit": false
            },
            "window": { "workDoneProgress": true }
        }
    })
}

/// Which document a request or notification is about.
#[derive(Debug, Clone, Serialize)]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

/// Which document, and which of our versions of it.
///
/// The version is the client's counter and the server rejects a change that does
/// not advance it, which is what makes a dropped notification detectable.
#[derive(Debug, Clone, Serialize)]
pub struct VersionedTextDocumentIdentifier {
    pub uri: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentItem {
    pub uri: String,
    pub language_id: String,
    pub version: i32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenTextDocumentParams {
    pub text_document: TextDocumentItem,
}

impl DidOpenTextDocumentParams {
    pub fn new(uri: &str, language_id: &str, version: i32, text: &str) -> Self {
        Self {
            text_document: TextDocumentItem {
                uri: uri.to_string(),
                language_id: language_id.to_string(),
                version,
                text: text.to_string(),
            },
        }
    }
}

/// One edit, described as a range and its replacement.
#[derive(Debug, Clone, Serialize)]
pub struct TextDocumentContentChangeEvent {
    pub range: Range,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeTextDocumentParams {
    pub text_document: VersionedTextDocumentIdentifier,
    pub content_changes: Vec<TextDocumentContentChangeEvent>,
}

impl DidChangeTextDocumentParams {
    /// Replace the whole document with one change event spanning all of it.
    ///
    /// Two constraints meet here. Roslyn advertises Incremental sync, so a
    /// `TextDocumentSyncKind.Full` notification — the one with no range — is not
    /// permitted. And mapping CodeMirror `ChangeSet` deltas across IPC would
    /// make every notification depend on every earlier one, so a single dropped
    /// or reordered message desynchronises the server permanently and
    /// *invisibly*: it would keep answering, from a buffer that no longer
    /// resembles the file.
    ///
    /// A whole-document range is legal under Incremental sync and makes each
    /// notification self-describing, so the worst a lost one costs is staleness
    /// until the next edit. `notes.md` records this as the chosen trade, to be
    /// revisited only if it measures badly.
    pub fn whole_document(uri: &str, version: i32, text: &str) -> Self {
        Self {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.to_string(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: document_range(text),
                text: text.to_string(),
            }],
        }
    }
}

/// The range covering an entire document, in the protocol's own units.
///
/// Two details are load-bearing. A trailing newline creates a final **empty
/// line**, and ending the range on the line before it would leave that newline
/// outside the replacement, so the server would gain a line per edit. And the
/// end column is counted in UTF-16 code units via
/// [`super::positions::byte_to_utf16`] — an emoji is one `char`, four bytes and
/// *two* units, so either of the other two measures would fall short of the end
/// of the document and leave the server holding a tail we thought we replaced.
fn document_range(text: &str) -> Range {
    // Counted as line *breaks* rather than by splitting and counting the pieces:
    // the number of breaks is the 0-based index of the last line directly, and
    // "" (no breaks) is line 0, which is the correct empty document. `rsplit`
    // rather than `split(..).last()` so neither pass walks the text twice.
    let breaks = text.bytes().filter(|byte| *byte == b'\n').count();
    let last = text.rsplit('\n').next().unwrap_or("");
    // The `\r` of a `\r\n` needs no handling and gets none: it can only appear
    // *before* a `\n`, so it is never in this last segment. The one text that
    // would be affected by trimming a trailing `\r` here is a document whose
    // final line ends in a lone one, and there the `\r` is content — trimming it
    // would leave the "whole document" range one character short, so the server
    // would keep that `\r` and append the replacement after it, gaining one per
    // edit. A server that instead reads a lone `\r` as a terminator clamps the
    // end to the line it does see, which is no worse than the short range would
    // have been.
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: breaks as u32,
            character: byte_to_utf16(last, last.len()),
        },
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DidCloseTextDocumentParams {
    pub text_document: TextDocumentIdentifier,
}

impl DidCloseTextDocumentParams {
    pub fn new(uri: &str) -> Self {
        Self {
            text_document: TextDocumentIdentifier {
                uri: uri.to_string(),
            },
        }
    }
}

/// The params of all three goto requests.
///
/// One struct for `definition`, `implementation` and `typeDefinition` because
/// they differ only in the method name — hence one type and three constants in
/// [`method`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentPositionParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

impl TextDocumentPositionParams {
    pub fn new(uri: &str, position: Position) -> Self {
        Self {
            text_document: TextDocumentIdentifier {
                uri: uri.to_string(),
            },
            position,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceContext {
    pub include_declaration: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub context: ReferenceContext,
}

impl ReferenceParams {
    /// `include_declaration` is the caller's choice and not defaulted here: with
    /// it, a list of one means "declared, never used"; without it, a list of one
    /// means "used once". The two read identically in the UI, so the decision
    /// belongs where the UI's wording is.
    pub fn new(uri: &str, position: Position, include_declaration: bool) -> Self {
        Self {
            text_document: TextDocumentIdentifier {
                uri: uri.to_string(),
            },
            position,
            context: ReferenceContext {
                include_declaration,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbolParams {
    pub text_document: TextDocumentIdentifier,
}

impl DocumentSymbolParams {
    pub fn new(uri: &str) -> Self {
        Self {
            text_document: TextDocumentIdentifier {
                uri: uri.to_string(),
            },
        }
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod protocol_tests;
