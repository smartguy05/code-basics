//! Tests for the declaration heuristic.
//! Included by `declarations.rs` under `#[cfg(test)]`.
//!
//! The six cases that pin the *name* rules for annotated bindings, C#
//! properties and non-declaring imports stayed in `git/grouping_tests.rs`
//! where they were written. They are load-bearing there: they document why a
//! hunk card is titled the way it is, and moving them would have quietly
//! changed what a reader of the grouping tests is told. They exercise this
//! module through `grouping`'s re-import, which is exactly the path the
//! Changes tab takes, so nothing is lost by leaving them.

use super::*;

/// The bug this module was lifted with. Splitting the line on `<` chops it at
/// the generic argument list, leaving `public async Task` as the head, whose
/// last identifier is the *return type*.
#[test]
fn a_generic_return_type_does_not_become_the_symbol_name() {
    assert_eq!(
        declaration_name("public async Task<int> DoWork(int a)").as_deref(),
        Some("DoWork")
    );
}

#[test]
fn a_csharp_method_is_named_by_its_parameter_list() {
    assert_eq!(
        declaration_name("public static void Configure(IServiceCollection services)").as_deref(),
        Some("Configure")
    );
}

/// The guard on the parameter-list rule. This line has a `(`, but it opens a
/// call on the right of the `=`; the declared thing is on the left.
#[test]
fn an_initialiser_call_does_not_capture_the_name() {
    assert_eq!(
        declaration_name("const cache: Map<string, number> = new Map();").as_deref(),
        Some("cache")
    );
    assert_eq!(
        declaration_name("static COUNTER: AtomicU64 = AtomicU64::new(0);").as_deref(),
        Some("COUNTER")
    );
}

#[test]
fn a_rust_function_is_named_and_typed() {
    let d =
        declaration("pub fn scan_with(root: &Path, options: ScanOptions) -> Result<Workspace> {")
            .unwrap();

    assert_eq!(d.name, "scan_with");
    assert_eq!(d.kind, SymbolKind::Function);
}

/// The generic parameter list sits between the name and the arguments, so it
/// has to be stepped over rather than treated as the name.
#[test]
fn a_generic_rust_function_is_named_past_its_type_parameters() {
    assert_eq!(
        declaration_name("pub fn collect<T: Clone>(items: &[T]) -> Vec<T> {").as_deref(),
        Some("collect")
    );
}

#[test]
fn a_typescript_exported_function_is_named_and_typed() {
    let d = declaration("export function buildQuote(input: Input): Quote {").unwrap();

    assert_eq!(d.name, "buildQuote");
    assert_eq!(d.kind, SymbolKind::Function);
}

#[test]
fn a_python_def_is_named_and_typed() {
    let d = declaration("def load_report(path: str) -> Report:").unwrap();

    assert_eq!(d.name, "load_report");
    assert_eq!(d.kind, SymbolKind::Function);
}

/// Characterisation, not endorsement. `impl Display for Foo` names the *trait
/// target*, because the last identifier on the line is `Foo` and `for` is not
/// a declaring keyword that the scan skips. It is not corrected here: for the
/// hunk-grouping consumer "Foo" is a defensible location for the block, and no
/// single-line rule can tell `impl Foo` from `impl Trait for Foo` without
/// special-casing a Rust keyword in a language-agnostic scan.
#[test]
fn an_impl_block_is_named_after_the_type_not_the_trait() {
    let d = declaration("impl Display for Foo {").unwrap();

    assert_eq!(d.name, "Foo");
    assert_eq!(d.kind, SymbolKind::Other);
}

#[test]
fn a_call_that_declares_nothing_is_not_a_declaration() {
    assert_eq!(declaration_name("logger.warn(message);"), None);
}

/// The name states a general property, so it asserts one. Every comment
/// syntax used by a language in the index's parsable-extension list belongs
/// here: a `//` line, a `/*` opener, the ` * ` continuation of a doc block, a
/// SQL `--` line and a VB `'` line. Prose is full of declaring keywords, and a
/// scan that reads it produces a fabricated name *with a confident kind* —
/// exactly the wrong-answer failure this module exists to refuse.
#[test]
fn a_comment_is_never_a_declaration() {
    for line in [
        "// pub fn nope(a: u32) {",
        "/* public class Foo - the old implementation",
        " * @param options the public config object",
        " * this function is deprecated",
        "  * Returns a new instance of the class Widget",
        "-- create the public users table",
        "' Public Class Customer",
    ] {
        assert_eq!(
            declaration(line),
            None,
            "line was read as a declaration: {line:?}"
        );
    }
}

/// A sentence's full stop is not part of an identifier, and no identifier in
/// any language the index parses ends in one. A trailing dot is therefore
/// positive evidence that the line was prose rather than source, which is why
/// it is rejected outright rather than trimmed off and used.
#[test]
fn an_identifier_never_ends_in_a_full_stop() {
    assert!(is_identifier("CodeBasics.Inspector"));
    assert!(!is_identifier("assumed."));
    assert!(!is_identifier("APIs."));
}

/// The C# shape that made this module's own bug worse than it looked. A
/// property or a field has no parameter list, so the rule that rescues
/// `Task<int> DoWork(…)` never fires, and the head scan named every one of
/// them after its *type*: `List`, `Dictionary`, `Task`. On a .NET solution
/// that is hundreds of palette rows all called `List` while the members that
/// actually exist are absent under their own names.
#[test]
fn a_generic_type_is_never_the_name_of_the_property_it_annotates() {
    for (line, expected) in [
        ("public List<Order> Orders { get; set; }", "Orders"),
        (
            "private readonly Dictionary<string, int> _map = new();",
            "_map",
        ),
        (
            "private readonly IReadOnlyList<Symbol> _symbols;",
            "_symbols",
        ),
        ("public Task<int> Pending { get; }", "Pending"),
        (
            "protected ObservableCollection<Row> Rows { get; } = new()",
            "Rows",
        ),
        ("public IReadOnlyList<NodeDto> Nodes => _nodes;", "Nodes"),
        ("public List<NodeDto> Nodes { get; set; } = [];", "Nodes"),
        ("private readonly List<NodeDto> _nodes = [];", "_nodes"),
    ] {
        assert_eq!(
            declaration_name(line).as_deref(),
            Some(expected),
            "wrong name for {line:?}"
        );
    }
}

/// Skipping balanced `<…>` regions must not mistake a comparison or a shift
/// for a generic argument list. These lines survive only because they carry no
/// declaring keyword and abstain before the head is ever read — this test
/// exists to keep that accident honest, since the head walk itself would
/// happily swallow the right-hand side of `a < b`.
#[test]
fn a_comparison_or_a_shift_is_not_mistaken_for_a_generic() {
    assert_eq!(declaration_name("if (a < b) {"), None);
    assert_eq!(declaration_name("x <<= 2;"), None);
    assert_eq!(declaration_name("while i < len:"), None);
}

/// A generic type in a declaration that *does* keep a declaring keyword still
/// has to name the binding, not the type argument.
#[test]
fn a_generic_binding_is_named_past_its_type_arguments() {
    assert_eq!(
        declaration_name("pub struct Index<T> {").as_deref(),
        Some("Index")
    );
    assert_eq!(
        declaration_name("public class Repository<TEntity> : IRepository<TEntity>").as_deref(),
        Some("Repository")
    );
}

/// The regression that skipping balanced `<…>` regions introduced. Truncating
/// at the first `<` used to throw the whole constraint clause away by accident;
/// keeping the balanced region means `where TEntity : class` survives into the
/// head, the colon-cut then slices at the *constraint's* colon, and the
/// last-identifier rule lands on the type parameter. Every line here was named
/// correctly before that change and wrongly after it: `Repository` became
/// `TEntity`, `Cache` became `TKey`, and `where TEntity : class, new()` became
/// `new` — a C# keyword offered as a symbol name.
#[test]
fn a_where_constraint_clause_never_supplies_the_name() {
    for (line, expected) in [
        (
            "public class Repository<TEntity> where TEntity : class",
            "Repository",
        ),
        (
            "public class Repository<TEntity> where TEntity : class, new()",
            "Repository",
        ),
        (
            "public sealed class Cache<TKey, TValue> where TKey : notnull",
            "Cache",
        ),
        ("public interface IStore<T> where T : class", "IStore"),
        ("public record Envelope<T> where T : struct", "Envelope"),
        ("public struct Pair<A, B> where A : notnull", "Pair"),
        ("pub struct Wrapper<T> where T: Ord {", "Wrapper"),
    ] {
        assert_eq!(
            declaration_name(line).as_deref(),
            Some(expected),
            "wrong name for {line:?}"
        );
    }
}

/// A generic *method* with a constraint was already safe, because the
/// parameter-list rule fires before the head is consulted. It is pinned here
/// so that cutting the constraint clause off the line cannot break the rule
/// that made these work — the cut has to leave the parameter list intact.
#[test]
fn a_constrained_generic_method_is_still_named_by_its_parameter_list() {
    assert_eq!(
        declaration_name("public void Add<T>(T item) where T : class").as_deref(),
        Some("Add")
    );
    assert_eq!(
        declaration_name("public static TOut Map<TIn, TOut>(TIn v) where TOut : new()").as_deref(),
        Some("Map")
    );
    assert_eq!(
        declaration_name("pub fn sort_by<T>(items: &mut [T]) where T: Ord {").as_deref(),
        Some("sort_by")
    );
}

/// The cut is a whole-word match on a lowercase keyword, so an identifier that
/// merely *contains* the word is untouched. `.Where(` is the case that matters:
/// LINQ puts it on a large fraction of the C# lines in any real solution, and a
/// substring match would silently truncate every one of them.
#[test]
fn a_word_merely_containing_a_clause_keyword_is_not_a_clause() {
    assert_eq!(
        declaration_name("public class WhereClause").as_deref(),
        Some("WhereClause")
    );
    assert_eq!(
        declaration_name("public IEnumerable<Order> Recent => _all.Where(o => o.IsRecent);")
            .as_deref(),
        Some("Recent")
    );
    assert_eq!(
        declaration_name("public List<Order> Filtered { get; } = _all.Where(o => o.Ok).ToList();")
            .as_deref(),
        Some("Filtered")
    );
    assert_eq!(
        declaration_name("let somewhere = 1;").as_deref(),
        Some("somewhere")
    );
    assert_eq!(
        declaration_name("public string Extendsion { get; set; }").as_deref(),
        Some("Extendsion")
    );
}

/// The same shape in the other two spellings. A base list in TypeScript or
/// Java is written with words rather than C#'s colon, so nothing cut it off
/// and the last identifier was the *base* type — and once generic arguments
/// stopped being truncated, `extends Bar<T>` began winning outright.
#[test]
fn an_extends_or_implements_clause_never_supplies_the_name() {
    for (line, expected) in [
        ("export class Grid<T> extends Base<T> {", "Grid"),
        ("export class Grid extends Base {", "Grid"),
        (
            "public class Repo<T> extends Base<T> implements IRepo<T> {",
            "Repo",
        ),
        ("export interface Props extends BaseProps {", "Props"),
    ] {
        assert_eq!(
            declaration_name(line).as_deref(),
            Some(expected),
            "wrong name for {line:?}"
        );
    }
}

/// Characterisation of the one line in this set whose answer is *not* what it
/// was before the `<…>`-skipping change. `impl<T> Thing for Wrapper<T> where
/// T: Ord {` used to abstain, purely as a side effect of the head being
/// truncated at the `<` immediately after `impl`; it now names the type the
/// block is implemented on.
///
/// That is left as it is because the alternative is incoherent. The suite
/// already pins `impl Display for Foo {` as naming `Foo`, and the same line
/// without a constraint clause — `impl<T> Thing for Wrapper<T> {` — has always
/// named `Wrapper`. Restoring the abstention would make two Rust lines that
/// declare the same thing behave differently on nothing but the presence of a
/// `where`, which is a rule no reader could predict. Asserted here so that the
/// change is a recorded decision rather than an unnoticed drift.
#[test]
fn an_impl_block_with_a_constraint_clause_names_the_type_like_any_other_impl() {
    assert_eq!(
        declaration_name("impl<T> Thing for Wrapper<T> where T: Ord {").as_deref(),
        Some("Wrapper")
    );
    assert_eq!(
        declaration_name("impl<T> Thing for Wrapper<T> {").as_deref(),
        Some("Wrapper")
    );
}

// --- one test per kind ------------------------------------------------------

#[test]
fn a_class_is_kind_class() {
    assert_eq!(
        declaration("public sealed class QuoteCalculator")
            .unwrap()
            .kind,
        SymbolKind::Class
    );
}

#[test]
fn a_struct_is_kind_struct() {
    assert_eq!(
        declaration("pub struct Workspace {").unwrap().kind,
        SymbolKind::Struct
    );
}

#[test]
fn an_enum_is_kind_enum() {
    assert_eq!(
        declaration("pub enum SymbolKind {").unwrap().kind,
        SymbolKind::Enum
    );
}

#[test]
fn an_interface_is_kind_interface() {
    assert_eq!(
        declaration("public interface IQuoteService").unwrap().kind,
        SymbolKind::Interface
    );
}

#[test]
fn a_trait_is_kind_trait() {
    assert_eq!(
        declaration("pub trait Adapter {").unwrap().kind,
        SymbolKind::Trait
    );
}

#[test]
fn a_type_alias_is_kind_type() {
    let d = declaration("export type Quote = { total: number };").unwrap();

    assert_eq!(d.name, "Quote");
    assert_eq!(d.kind, SymbolKind::Type);
}

#[test]
fn a_namespace_is_kind_namespace() {
    let d = declaration("namespace CodeBasics.Inspector {").unwrap();

    assert_eq!(d.name, "CodeBasics.Inspector");
    assert_eq!(d.kind, SymbolKind::Namespace);
}

#[test]
fn a_const_is_kind_constant() {
    assert_eq!(
        declaration("const MAX_DEPTH: usize = 10;").unwrap().kind,
        SymbolKind::Constant
    );
}

#[test]
fn a_let_is_kind_variable() {
    assert_eq!(
        declaration("let total: usize = 0;").unwrap().kind,
        SymbolKind::Variable
    );
}

/// A C# method signature carries no keyword that says "method" — only
/// modifiers. Rather than infer `Function` from the shape of the line, the
/// scan abstains, and the palette shows no badge.
#[test]
fn a_line_with_only_modifiers_abstains_from_a_kind() {
    let d = declaration("public async Task<int> DoWork(int a)").unwrap();

    assert_eq!(d.name, "DoWork");
    assert_eq!(d.kind, SymbolKind::Other);
}

/// `record` is not decisive on its own — C# has both `record class` and
/// `record struct` — so it is skipped and the keyword that follows decides.
#[test]
fn a_record_defers_to_the_keyword_that_follows_it() {
    let d = declaration("public record struct Point(int X, int Y);").unwrap();

    assert_eq!(d.name, "Point");
    assert_eq!(d.kind, SymbolKind::Struct);
}

/// A silent hole rather than a wrong answer, and the worse of the two for a
/// Rust reader: every `pub(crate)` and `pub(super)` item in this repository was
/// missing from the index. The head walk stopped at the first `(`, which for
/// these lines is the one inside the visibility modifier, so the head was the
/// bare word `pub` and the parameter-list rule read `pub` straight back off the
/// same paren and refused it as a keyword.
#[test]
fn a_visibility_modifier_does_not_hide_the_declaration() {
    assert_eq!(
        declaration_name("pub(crate) fn helper(a: u32) {}").as_deref(),
        Some("helper")
    );
    assert_eq!(
        declaration_name("pub(super) fn inner() {}").as_deref(),
        Some("inner")
    );
    assert_eq!(
        declaration_name("pub(in crate::thing) fn scoped() {}").as_deref(),
        Some("scoped")
    );
    assert_eq!(
        declaration_name("pub(crate) struct Thing;").as_deref(),
        Some("Thing")
    );
    assert_eq!(
        declaration_name("pub(crate) const LIMIT: usize = 10;").as_deref(),
        Some("LIMIT")
    );
    assert_eq!(
        declaration_name("pub(crate) static NAMES: &[&str] = &[];").as_deref(),
        Some("NAMES")
    );
}

/// `pub(crate)fn` compiles, and rustfmt normalises it away, which is exactly
/// why it is worth pinning: nothing in this repository is written that way, so
/// the whole-repository sweep that found the modifier hole could not have found
/// this one. Excising the scope splices the two words together into `pubfn`,
/// which matches no declaring keyword, and the line goes silent.
#[test]
fn a_visibility_scope_written_hard_against_the_keyword_is_still_read() {
    assert_eq!(
        declaration_name("pub(crate)fn helper(a: u32) {}").as_deref(),
        Some("helper")
    );
    assert_eq!(
        declaration_name("pub(crate)struct Thing;").as_deref(),
        Some("Thing")
    );
}

/// The reason the scope search tests for a whole word. `epub` ends in the
/// letters and sits hard against its parameter list, so a bare substring
/// search would excise `(a: A)` and leave the return type as the last
/// identifier standing — a confidently wrong name rather than an abstention.
#[test]
fn a_name_merely_ending_in_the_visibility_keyword_keeps_its_parameter_list() {
    assert_eq!(
        declaration_name("pub fn epub(a: A) -> B {").as_deref(),
        Some("epub")
    );
}
