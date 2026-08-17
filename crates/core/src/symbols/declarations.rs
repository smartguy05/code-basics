//! Reading one line of source and deciding whether it declares something.
//!
//! This heuristic began life inside [`crate::git::grouping`], where it did one
//! job: give a hunk a title when git's own hunk header could not. It lives here
//! now because a second consumer needs it — the symbol index, which walks the
//! whole workspace and wants not just a name but a *kind*, so a palette can put
//! a badge beside each row.
//!
//! It was lifted rather than merely made `pub(crate)` because of the direction
//! of the dependency. `git` may depend on `symbols`; `symbols` must never
//! depend on `git`. A symbol is a property of source text, and nothing about
//! recognising one requires a repository, a diff or a hunk. Had this stayed in
//! `grouping.rs` the index would have had to reach into a git module to ask
//! what a line of C# declares, and the arrow would have pointed the wrong way
//! the moment the index wanted to extend the heuristic with kind inference.
//!
//! What stayed behind in `grouping.rs` is everything about *hunk headers* —
//! `enclosing_symbol`, `symbol_from_header`, `header_can_name_a_symbol`,
//! `symbol_is_new`. A hunk header is a git concept: git chose that line, using
//! its own per-language `funcname` patterns, and the rules for salvaging a
//! title out of one are rules about git's output, not about source code.
//!
//! # This is not a parser
//!
//! It is a word scan over a single line, and it will be wrong sometimes. That
//! is tolerable in both consumers only because both abstain loudly rather than
//! guess: a hunk with no name falls back to being grouped by file, and a symbol
//! with no [`SymbolKind`] shows no badge at all rather than the wrong one.
//! Every rule below is written to return `None` or [`SymbolKind::Other`] in
//! preference to producing something plausible and false.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use specta::Type;

/// What sort of thing a line declares.
///
/// Deliberately coarse. The point is a badge in a palette, not a type system:
/// `Trait` and `Interface` are kept apart because the languages that have them
/// call them different things and users grep for the word they know, while
/// everything a scan cannot place lands in [`SymbolKind::Other`], which renders
/// as no badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SymbolKind {
    Function,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Type,
    Namespace,
    Constant,
    /// A member with accessors, as C#, TypeScript, Java and Kotlin mean it.
    ///
    /// **Unreachable from the text scan below, and that is deliberate.** No
    /// keyword introduces one unambiguously: C# writes `public string Name
    /// { get; set; }`, which is a line the scan reads as a variable, and
    /// `property` is not a keyword in any language here. It exists for
    /// [`crate::lsp::protocol::symbol_kind`], where a real server has already
    /// done the parsing and says `SymbolKind.Property` (7) outright.
    ///
    /// So this enum now has two producers with different confidence, which is
    /// the arrangement the `lsp` module was added under: a heuristic that
    /// abstains, beside a server that knows.
    Property,
    Variable,
    Other,
}

/// A name, and what the line that carried it appeared to be declaring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub name: String,
    pub kind: SymbolKind,
}

/// Header lines that are definitely not a symbol.
pub(crate) const NOT_A_SYMBOL: &[&str] = &[
    "import", "use", "using", "from", "#include", "include", "package",
];

/// Keywords that introduce something worth naming, across the languages this
/// application already knows how to build and test.
///
/// Matching is exact and lowercase, which means **Visual Basic declares
/// nothing as far as this scan is concerned**: `Public Class Customer` does
/// not match `class`, so the line abstains. That is a real gap and it is left
/// open on purpose. Case-insensitive matching would fix VB and break the
/// case-sensitive languages, where a capitalised word is an ordinary
/// identifier and not a keyword at all.
///
/// The cost is easiest to see in the very language a looser match is meant to
/// rescue. A case-insensitive build of this module was made and run, and it
/// does read `Public Class Customer` as `Customer` and `Public Function
/// GetName() As String` as `GetName` — but it also reads `Public Shared
/// ReadOnly Property Total As Integer` as **`Integer`**, naming the property
/// after its type. VB puts the type last, behind `As`, exactly where the
/// last-identifier rule looks, and nothing short of VB-specific rules can tell
/// the two apart. That is a confident wrong answer where there is currently a
/// silence, which is the trade this module exists to refuse. So VB stays quiet
/// until it gets rules of its own rather than a looser version of everyone
/// else's.
///
/// (An earlier version of this note cited two English sentences in this
/// repository that begin with the word `Import` as the danger. They are not:
/// `import` is in [`NOT_A_SYMBOL`], and both lines return `None` in either
/// matching mode. The example above is the one that was actually executed.)
pub(crate) const DECLARING: &[&str] = &[
    "fn",
    "func",
    "function",
    "def",
    "class",
    "struct",
    "enum",
    "interface",
    "trait",
    "impl",
    "type",
    "record",
    "namespace",
    "module",
    "const",
    "let",
    "var",
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "async",
    "export",
    "default",
    "abstract",
    "override",
    "virtual",
    "sealed",
    "partial",
    "readonly",
    "unsafe",
    "extern",
    "pub",
];

/// Pull a plausible symbol name out of one line of source.
///
/// Defined in terms of [`declaration`] so that the name a hunk card shows and
/// the name the index stores can never disagree: there is one implementation,
/// and this is a projection of it.
pub fn declaration_name(line: &str) -> Option<String> {
    declaration(line).map(|d| d.name)
}

/// Pull a plausible declaration — name and kind — out of one line of source.
pub fn declaration(line: &str) -> Option<Declared> {
    let line = line.trim();
    if line.is_empty() || is_comment(line) {
        return None;
    }

    // A constraint or inheritance clause is trailing detail, never the name.
    // Cutting it off the whole line rather than off the head is deliberate: it
    // has to happen before the parameter-list rule as well, or
    // `where TEntity : class, new()` offers the C# keyword `new` as a name.
    let line = without_clause(line);

    // A visibility scope is punctuation the two paren rules below would both
    // misread, so it is removed once here rather than guarded for twice.
    let visible = strip_visibility_scope(line);
    let line = visible.as_ref();

    // Everything before a parameter list, an assignment or a body.
    let head_text = head_of(line);
    let head = head_text.as_str();

    // A colon in the head is a type annotation — `let total: usize`,
    // `static COUNTER: AtomicU64`, `const cache: Map`. The name is on the
    // left of it; the last identifier would be the *type*. Without one, the
    // last identifier is right: `public decimal EstimateCost`.
    let head = match head.find(':') {
        Some(colon) => head[..colon].trim(),
        None => head,
    };

    let words: Vec<&str> = head.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }

    // A declaration is only claimed when a declaring keyword is present:
    // otherwise every assignment and every call would name a "symbol".
    let has_keyword = words.iter().any(|w| DECLARING.contains(w));
    if !has_keyword {
        return None;
    }

    // Import and re-export lines can still carry a declaring keyword —
    // `import type { … }`, `pub use …` — but they declare nothing, and the
    // scan below would name the card "import" or "use".
    if words.iter().any(|w| NOT_A_SYMBOL.contains(w)) {
        return None;
    }

    // A parameter list is the strongest signal a single line offers: whatever
    // sits immediately before `(` is the thing being declared, and everything
    // to its left is modifiers and a return type. It is tried first because it
    // reads the line's own punctuation rather than betting on word order.
    //
    // The guard matters as much as the rule. `const cache: Map<string, number>
    // = new Map();` also contains a `(`, but it belongs to the initialiser on
    // the right of the `=`, and the thing being declared is on the left. So
    // the parameter-list rule only applies when the `(` comes before any `=`;
    // otherwise the word scan over the head still wins.
    let name = name_before_parameter_list(line).or_else(|| {
        words
            .iter()
            .rev()
            .find(|w| !DECLARING.contains(*w) && is_identifier(w))
            .map(|w| (*w).to_string())
    })?;

    Some(Declared {
        kind: kind_of(&words),
        name,
    })
}

/// Whether a line is comment prose rather than source.
///
/// Prose is dense with the words in [`DECLARING`] — "the public config
/// object", "this function is deprecated", "create the public users table" —
/// so a scan that reads a comment does not merely produce a poor name, it
/// produces a fabricated one *and* a confident kind beside it. The badge is
/// the worse half: `SymbolKind::Class` on a sentence about a class is exactly
/// the wrong answer this module promises never to give.
///
/// Only openers are matched, and only at the start of a trimmed line. A
/// comment that trails real code (`public int Count; // the number of rows`)
/// still declares something and must not be dropped, and finding the boundary
/// between code and a trailing comment needs a string-aware scan this is not.
/// Living with a trailing comment costs at worst a slightly wrong name on a
/// line that really is a declaration; treating a whole-line comment as source
/// invents a symbol that does not exist anywhere.
///
/// The five openers cover every language in the index's parsable-extension
/// list: `//` (C-family, Rust, Go, Swift, Kotlin, Scala, PHP), `#` (Python,
/// Ruby), `/*` and its ` * ` continuation (C-family block and doc comments),
/// `--` (SQL) and `'` (VB). None of them begins a declaration in any of those
/// languages, so the check is language-agnostic and needs no file extension.
fn is_comment(line: &str) -> bool {
    ["//", "#", "/*", "*", "--", "'"]
        .iter()
        .any(|opener| line.starts_with(opener))
}

/// Words that begin a trailing clause: everything from one of these onwards
/// constrains or relates the declared thing rather than naming it.
///
/// All three are spelled the same in every language this scan sees. `where`
/// introduces a generic constraint in both C# and Rust; `extends` and
/// `implements` introduce a base list in TypeScript, JavaScript and Java,
/// where C# would use a colon that [`declaration`] already cuts at.
///
/// This exists because of a regression rather than by design. While the head
/// was truncated at the first `<`, a constraint clause was thrown away as a
/// side effect — it always sits after the type parameter list — and nobody had
/// to name it. Skipping balanced `<…>` regions instead, which is what makes a
/// C# generic property come out right, left the clause in the head, where the
/// colon-cut sliced at the *constraint's* colon and the last-identifier rule
/// returned a type parameter: `Repository<TEntity> where TEntity : class` was
/// named `TEntity`, and `where TEntity : class, new()` was named `new`.
///
/// Matching is on whole words only, and the case that demands it is an
/// ordinary lowercase identifier with a keyword inside it: a substring match
/// was measured turning `var somewhere = 1;` into `some`. C#'s `.Where(` and a
/// type named `WhereClause` are safe either way — the match is lowercase and
/// both carry a capital `W` — so they are not the reason for the rule, only
/// the shapes it is most often mistaken for.
/// The words are matched lowercase for the same reason [`DECLARING`] is: the
/// languages that spell them are case-sensitive, and see the note there for the
/// VB line that a looser match was executed against and got wrong.
const CLAUSE_KEYWORDS: &[&str] = &["where", "extends", "implements"];

/// The line up to the first trailing clause, or the whole line when it has
/// none.
///
/// Clause keywords are only recognised at bracket depth zero, so the
/// `T extends Base` inside a TypeScript type parameter list is left alone: it
/// is part of the generic arguments the head walk already elides, and cutting
/// there would discard the name that follows the `>`.
fn without_clause(line: &str) -> &str {
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '.' || c == '$';

    let mut depth = 0usize;
    let mut word_start: Option<usize> = None;

    for (offset, c) in line.char_indices() {
        let boundary = match c {
            '<' => {
                depth += 1;
                true
            }
            '>' => {
                depth = depth.saturating_sub(1);
                true
            }
            _ if depth > 0 => true,
            _ if is_word_char(c) => {
                word_start.get_or_insert(offset);
                false
            }
            _ => true,
        };

        if boundary {
            if let Some(start) = word_start.take() {
                if CLAUSE_KEYWORDS.contains(&&line[start..offset]) {
                    return &line[..start];
                }
            }
        }
    }

    match word_start {
        Some(start) if CLAUSE_KEYWORDS.contains(&&line[start..]) => &line[..start],
        _ => line,
    }
}

/// The line with any visibility scope — the `(crate)` of `pub(crate)` — cut
/// out of it.
///
/// This closed a silent hole rather than a wrong answer, which is the failure
/// mode this module is least able to notice. Both paren rules read the *first*
/// `(` on the line: [`head_of`] stops there, and
/// [`name_before_parameter_list`] takes the identifier in front of it. For a
/// `pub(crate) fn helper(a: u32)` that first paren is the one inside the
/// visibility modifier, so the head collapsed to the bare word `pub` and the
/// parameter-list rule read `pub` straight back off the same paren and refused
/// it as a declaring keyword. The line abstained, and abstention is the one
/// outcome nothing complains about: every `pub(crate)` and `pub(super)` item in
/// this repository was simply missing from the index, including functions this
/// project had written days earlier.
///
/// It is written as a punctuation rule — skip a balanced region, the same move
/// [`head_of`] already makes for `<…>` — rather than as a list of Rust
/// visibility keywords, because that is the style the rest of the module is in
/// and because the keyword list would have to grow with `pub(in path)` and
/// whatever comes next. The scope contents are never inspected, so
/// `pub(in crate::thing)` costs nothing extra.
///
/// The trigger is deliberately narrow: a `(` touching the end of the whole word
/// `pub`, with no space between them. C# has no shape like this — `internal`
/// and `protected internal` are bare words — so no other language is touched.
/// The word test earns its place on a name that merely *ends* in those letters
/// and sits hard against its parameter list: with the test removed,
/// `pub fn epub(a: A) -> B {` was measured returning `B`, naming the return
/// type, because the excision ate the parameter list the name is read from.
/// (An ordinary call like `dedup(items)` never reaches this rule at all — the
/// literal `pub(` does not occur in it — so it is no evidence either way.)
/// An unbalanced `(` abandons the rule and returns the line as it stands,
/// because dropping the whole tail of a line is a far larger edit than the one
/// this is authorised to make.
fn strip_visibility_scope(line: &str) -> Cow<'_, str> {
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '$';

    let mut out: Option<String> = None;
    let mut copied_to = 0usize;
    let mut search = 0usize;

    while let Some(found) = line[search..].find("pub(") {
        let at = search + found;
        let open = at + "pub".len();

        if line[..at].chars().next_back().is_some_and(is_word_char) {
            search = open;
            continue;
        }

        let Some(close) = closing_paren(line, open) else {
            break;
        };

        let buffer = out.get_or_insert_with(String::new);
        buffer.push_str(&line[copied_to..open]);
        // A space, not nothing: `pub(crate)fn helper()` compiles, and splicing
        // the ends together would leave `pubfn`, which matches no declaring
        // keyword and takes the whole line silent. The usual spelling gains a
        // second space, which `split_whitespace` never notices.
        buffer.push(' ');
        copied_to = close + 1;
        search = close + 1;
    }

    match out {
        Some(mut buffer) => {
            buffer.push_str(&line[copied_to..]);
            Cow::Owned(buffer)
        }
        None => Cow::Borrowed(line),
    }
}

/// The byte offset of the `)` matching the `(` at `open`, or `None` when the
/// line has no matching one.
///
/// An unbalanced `(` is reported by falling out of the loop, not by the
/// `checked_sub` below: this is only ever called with a `(` at `open`, so the
/// depth is at least one before any `)` is seen and the subtraction cannot
/// underflow. It is defensive, and kept only so the arithmetic cannot become
/// the thing that panics if a future caller passes an arbitrary offset.
fn closing_paren(line: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, c) in line[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// The part of a line that can carry the declared name, with generic argument
/// lists removed.
///
/// The head stops at the first `(`, `=`, `{` or `;` outside any `<…>`: past
/// those lie a parameter list, an initialiser, a body or the next statement,
/// and none of them holds the name. What is new here is the `<…>` skipping.
/// Truncating at the first `<` — the previous rule — threw away everything
/// after the generic arguments, and for a C# property or field that is the
/// name itself: `public List<Order> Orders { get; set; }` became the head
/// `public List` and was confidently named after its own type. Skipping the
/// balanced region instead leaves `public List Orders`, and the ordinary
/// last-identifier rule is then correct **provided nothing else trails the
/// generic arguments**. That proviso is not a detail: a `where` constraint or
/// an `extends` base list trails them constantly, and the old truncation had
/// been discarding those by accident. [`without_clause`] now removes them
/// explicitly, before this walk ever runs.
///
/// The parameter-list rule could not cover this. It fires only on a line
/// containing `(`, and a property or a field has none — which is why the
/// original `Task<int> DoWork(…)` fix left the far commoner shape broken.
///
/// `<` is also comparison and the shift operators, and this walk does not try
/// to tell them apart: inside a supposed generic region it drops characters,
/// so `a < b` would swallow `b`. That is survivable only because such lines
/// carry no declaring keyword. The order is worth being precise about, since
/// the rejection does not come first: the head is built here, damage and all,
/// and only then split into words that [`declaration`] tests for a keyword. It
/// is the mangled head itself that fails that test, so `a < b`, `if (a < b) {`,
/// `while (i < items.Count)` and `total = a < b ? a : b;` all reach `None` —
/// a property the test suite asserts rather than assumes. A `>`
/// with no opener (Rust's `->`, a comparison) is simply not counted down past
/// zero, so an unbalanced line degrades to dropping its tail rather than
/// mangling its front.
fn head_of(line: &str) -> String {
    let mut head = String::with_capacity(line.len());
    let mut depth = 0usize;

    for c in line.chars() {
        match c {
            '<' => {
                depth += 1;
                // The elided arguments still separated two words, and the head
                // is read word by word.
                if depth == 1 {
                    head.push(' ');
                }
            }
            '>' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '(' | '=' | '{' | ';' => break,
            _ => head.push(c),
        }
    }

    head.trim().to_string()
}

/// The identifier immediately preceding the first `(`, when that `(` opens a
/// parameter list rather than a call on the right of an assignment.
fn name_before_parameter_list(line: &str) -> Option<String> {
    let paren = line.find('(')?;
    if line.find('=').is_some_and(|eq| eq < paren) {
        return None;
    }

    let mut before = line[..paren].trim_end();

    // `pub fn thing<T>(x: T)` — the generic parameter list sits between the
    // name and the arguments, so it is stepped over before reading a name.
    // Scanning back with a depth counter rather than searching for the first
    // `<` keeps nested generics (`Map<String, Vec<T>>`) intact.
    if before.ends_with('>') {
        let mut depth = 0usize;
        let mut cut = None;
        for (offset, c) in before.char_indices().rev() {
            match c {
                '>' => depth += 1,
                // `checked_sub` rather than `-`: an unbalanced `>` (a stray
                // arrow, a shell redirect in a string) must abandon the rule,
                // not panic.
                '<' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        cut = Some(offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        before = &before[..cut?];
        before = before.trim_end();
    }

    let name: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    // A declaring keyword sitting directly against the `(` is a control-flow
    // statement or a cast, not a name.
    (is_identifier(&name) && !DECLARING.contains(&name.as_str())).then_some(name)
}

/// What the first *decisive* declaring keyword says the line is.
///
/// Most of [`DECLARING`] is modifiers — `public`, `static`, `async`, `pub`,
/// `export` — which say that something is being declared without saying what,
/// so they are stepped over rather than treated as an answer. When nothing
/// decisive appears the kind is [`SymbolKind::Other`] and the palette shows no
/// badge, which is the honest outcome: a C# method signature carries no
/// keyword naming it a method, and inventing `Function` from the shape of the
/// line would be the sort of confident guess this crate refuses to make.
///
/// Two keywords are deliberately *not* decisive:
///
/// * `record` — C# has both `record class` and `record struct`, and a
///   first-keyword rule would confidently pick the wrong one half the time.
/// * `static` and `readonly` — a Rust `static` really is a constant, but a C#
///   `static` is a modifier on a method or a field, and one line cannot tell
///   the two apart.
fn kind_of(words: &[&str]) -> SymbolKind {
    words
        .iter()
        .find_map(|w| match *w {
            "fn" | "func" | "function" | "def" => Some(SymbolKind::Function),
            "class" => Some(SymbolKind::Class),
            "struct" => Some(SymbolKind::Struct),
            "enum" => Some(SymbolKind::Enum),
            "interface" => Some(SymbolKind::Interface),
            "trait" => Some(SymbolKind::Trait),
            "type" => Some(SymbolKind::Type),
            "namespace" | "module" => Some(SymbolKind::Namespace),
            "const" => Some(SymbolKind::Constant),
            "let" | "var" => Some(SymbolKind::Variable),
            _ => None,
        })
        .unwrap_or(SymbolKind::Other)
}

/// Whether a word could be a name in one of these languages.
///
/// The interior `.` is deliberate and load-bearing: a C# namespace really is
/// `CodeBasics.Inspector`, and splitting it would name the symbol after its
/// last segment. A *trailing* dot is the opposite — no identifier in any
/// language here ends in one, so it is not a malformed name to be tidied up
/// but positive evidence that the word came from the end of an English
/// sentence. Rejecting it rather than trimming it keeps the fabricated name
/// out: `let it be assumed.` is not offered as the symbol `assumed`.
///
/// It does not follow that the line goes unnamed. The rule disqualifies one
/// word, not the line, and the scan simply walks on to an earlier one — that
/// same sentence comes back as `be`, a [`SymbolKind::Variable`], because `let`
/// is a declaring keyword and `be` is a well-formed identifier. Only
/// [`is_comment`] can silence a whole line of prose, and prose that is not
/// marked as a comment is beyond what a word scan can defend against.
pub(crate) fn is_identifier(word: &str) -> bool {
    !word.is_empty()
        && !word.ends_with('.')
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '$')
        && word.chars().next().is_some_and(|c| !c.is_numeric())
}

#[cfg(test)]
#[path = "declarations_tests.rs"]
mod declarations_tests;
