//! The signal types and [`admit`] — the one gate every candidate component in
//! this phase has to pass through.
//!
//! See the module documentation on [`super`] for the grading rule this file
//! implements and why it lives in one place instead of in each producer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

/// How much a signal is allowed to do.
///
/// Deliberately two values and not a confidence number. A score invites a
/// threshold, a threshold invites tuning, and tuning a threshold is how a tool
/// ends up drawing an arrow because 0.71 happened to beat 0.7 that week. The
/// question this enum answers is categorical: *did the author write this down,
/// or did we work it out?* There is no useful middle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    /// A declared fact, in a manifest, a configuration file or a filename.
    /// May create a component and an edge.
    High,
    /// Something worked out from context. May only enrich a component that a
    /// [`Strength::High`] signal already created.
    Medium,
}

/// What sort of thing a signal is about.
///
/// [`ComponentKind::Unknown`] is not a failure case and is not a placeholder to
/// be filled in later: it is the honest answer when a dependency is declared
/// but its role is not. It still creates a box, because the *dependency* was
/// declared even though its nature was not, and a box the reader can name
/// beats an arrow into nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComponentKind {
    HttpService,
    Database,
    Cache,
    MessageQueue,
    Unknown,
}

impl ComponentKind {
    /// The stable slug used in component ids and in warnings.
    ///
    /// Written out rather than derived from the variant name so that renaming a
    /// variant cannot silently change every stored component id.
    pub fn slug(self) -> &'static str {
        match self {
            ComponentKind::HttpService => "http",
            ComponentKind::Database => "database",
            ComponentKind::Cache => "cache",
            ComponentKind::MessageQueue => "queue",
            ComponentKind::Unknown => "unknown",
        }
    }
}

/// Where a signal was read from, so that a reader can check the tool instead of
/// trusting it.
///
/// Mandatory on every signal, with no constructor that omits it. The excerpt in
/// particular is what makes a diagram auditable: a user who does not believe an
/// arrow can open that file at that line and see the same text this module saw.
/// An assertion nobody can check is not admissible here, however plausible.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Evidence {
    /// Workspace-relative where possible. Producers are expected to pass the
    /// same relative, forward-slashed form the rest of [`super::super::graph`]
    /// uses, because these strings end up in exported diagrams.
    pub path: PathBuf,
    /// 1-based, when the producer knows it. `None` when the evidence is the
    /// existence of the file itself rather than anything inside it.
    pub line: Option<u32>,
    /// The text that was read, or a sanctioned redaction of it (see
    /// [`Evidence::elided_value`]).
    pub excerpt: String,
}

impl Evidence {
    pub fn new(path: impl Into<PathBuf>, line: Option<u32>, excerpt: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            line,
            excerpt: excerpt.into(),
        }
    }

    /// Evidence for a configuration entry whose *value* must not be read.
    ///
    /// The sanctioned way to cite a connection string. A producer that quotes
    /// the line verbatim will have the whole signal refused by [`admit`] — the
    /// screen cannot tell a careful producer from a careless one and does not
    /// try — so this exists to give producers the shape that passes:
    /// `Orders: <value not read>`. The key is the author's own label and is
    /// safe; the value never enters the process's output at all.
    pub fn elided_value(path: impl Into<PathBuf>, line: Option<u32>, key: &str) -> Self {
        Self::new(path, line, format!("{key}: <value not read>"))
    }
}

/// One candidate observation, from one producer, about one project.
///
/// A signal is a *claim with a receipt*, not a component. Producers emit them
/// freely; [`admit`] decides which ones become anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub strength: Strength,
    pub kind: ComponentKind,
    /// What this signal is about — a component label, never a value read out of
    /// configuration. `PostgreSQL`, `Orders`, `billing-api`.
    pub label: String,
    /// The [`crate::model::Project::id`] of the project that emitted it. A
    /// signal always belongs to a project: a component with no consumer is not
    /// a thing this module can observe.
    pub project_id: String,
    /// The enrichment a [`Strength::Medium`] signal carries — a route list, an
    /// alternative name. `None` on a signal that only corroborates.
    pub detail: Option<String>,
    pub evidence: Evidence,
}

impl Signal {
    pub fn high(
        kind: ComponentKind,
        label: impl Into<String>,
        project_id: impl Into<String>,
        evidence: Evidence,
    ) -> Self {
        Self {
            strength: Strength::High,
            kind,
            label: label.into(),
            project_id: project_id.into(),
            detail: None,
            evidence,
        }
    }

    pub fn medium(
        kind: ComponentKind,
        label: impl Into<String>,
        project_id: impl Into<String>,
        evidence: Evidence,
    ) -> Self {
        Self {
            strength: Strength::Medium,
            kind,
            label: label.into(),
            project_id: project_id.into(),
            detail: None,
            evidence,
        }
    }

    /// Attach the enrichment text a MEDIUM signal contributes.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

// ---------------------------------------------------------------------------
// What survives
// ---------------------------------------------------------------------------

/// One box that HIGH signals earned.
///
/// # The identity rule, and why it is `(kind, label)` rather than
/// `(project, kind, label)`
///
/// Two projects each declaring a dependency on `Npgsql` are two *usages*. They
/// collapse to **one** component here, keyed on kind and case-folded label,
/// with two entries in [`Component::usages`].
///
/// Neither answer is free of claims, which is why the choice is argued rather
/// than assumed. One box per usage would put two boxes labelled `PostgreSQL`
/// side by side, which asserts that two distinct databases exist — nothing in
/// either manifest says that. One shared box risks being read as "these two
/// services share an instance", which nothing says either.
///
/// The tie is broken by what the component actually *is* here. A node in this
/// module is a **technology**, not a deployed instance: `PostgreSQL` means "the
/// PostgreSQL protocol is spoken in this workspace", and an edge into it means
/// "this project declares that it speaks it". Under that reading the shared box
/// is exactly true, and this module never asserts instance identity at all —
/// no manifest states it, so nothing here can. The per-usage evidence is kept
/// and rendered precisely so a reader can see the box was earned twice, by two
/// different projects citing two different files, rather than inferring a
/// shared instance from a single outline.
///
/// Case folding on the key, exact text on the display label: `Npgsql` and
/// `npgsql` from two producers are the same technology, and two boxes differing
/// only in capitalisation would be the duplicate-box failure with extra steps.
/// The displayed spelling is the lexicographically smallest one observed, which
/// is arbitrary but — unlike "whichever arrived first" — identical on every run
/// no matter what order the producers ran in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// `component:<kind>:<slugified label>`. Prefixed so it cannot collide with
    /// the ids [`super::super::graph`] mints for projects, solutions or
    /// externals.
    pub id: String,
    pub kind: ComponentKind,
    pub label: String,
    /// Every HIGH signal that earned this box, sorted. One usage is one edge.
    pub usages: Vec<Usage>,
    /// Every MEDIUM signal that attached to it, sorted. Enrichment only —
    /// removing all of these would change what the box *says*, never whether
    /// it exists.
    pub details: Vec<Detail>,
}

/// One project's declared use of a component, with the receipt for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Usage {
    pub project_id: String,
    pub evidence: Evidence,
}

/// One MEDIUM signal's contribution to a component that already existed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Detail {
    pub project_id: String,
    pub text: String,
    pub evidence: Evidence,
}

/// One edge: a project declares it uses a component.
///
/// Derived from [`Component::usages`] rather than stored, so the two can never
/// disagree about which arrows a box earned.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AdmittedEdge {
    pub project_id: String,
    pub component_id: String,
}

/// Why a candidate was refused.
///
/// An enum rather than a free string because the reasons are the rules: a new
/// reason means a new rule, which should be a deliberate edit here and not a
/// new sentence appearing in a producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiscardReason {
    /// The signal's text contained a connection-string value. Screened first,
    /// and the offending text is never echoed anywhere — including into the
    /// warning that reports it.
    SecretValue,
    /// A MEDIUM signal naming a component no HIGH signal created.
    MediumWithoutHigh,
    /// The excerpt was a `using`/`import` line or a comment: not a declaration
    /// about what the program does.
    NotADeclaration,
    /// A HIGH signal citing a file that is not a manifest or configuration
    /// file. HIGH means "the author wrote this down"; a line in a source file
    /// is not that, and quietly demoting it to MEDIUM would be this module
    /// guessing on the producer's behalf.
    UnverifiableEvidence,
    /// An empty label, an empty project id, or no evidence path.
    Incomplete,
    /// The label was shaped like a value rather than a name — it carried `=`,
    /// `;`, a URL scheme, a `host:port`, or was longer than any component name
    /// plausibly is.
    LabelLooksLikeAValue,
    /// The *enrichment text* was shaped like a value rather than prose, by the
    /// same test [`DiscardReason::LabelLooksLikeAValue`] applies to the label.
    ///
    /// A separate reason and not a reuse of that one, because the two differ in
    /// what may still be said about them: a signal refused here had a perfectly
    /// good label, which its own screen already cleared, so the warning can
    /// still name what was refused. The hazard is identical, though —
    /// [`super::super::components`] prints a [`Detail::text`] verbatim when the
    /// project that contributed it did not earn the component, so a detail is
    /// exactly as exported as a label is.
    DetailLooksLikeAValue,
}

impl DiscardReason {
    fn slug(self) -> &'static str {
        match self {
            DiscardReason::SecretValue => "secret-value",
            DiscardReason::MediumWithoutHigh => "medium-without-high",
            DiscardReason::NotADeclaration => "not-a-declaration",
            DiscardReason::UnverifiableEvidence => "unverifiable-evidence",
            DiscardReason::Incomplete => "incomplete",
            DiscardReason::LabelLooksLikeAValue => "label-looks-like-a-value",
            DiscardReason::DetailLooksLikeAValue => "detail-looks-like-a-value",
        }
    }
}

/// One refused candidate, kept so that refusal is visible rather than silent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Discarded {
    pub project_id: String,
    pub kind: ComponentKind,
    pub strength: Strength,
    /// `None` when echoing it would leak.
    ///
    /// That is both reasons whose subject is the label's own text:
    /// [`DiscardReason::SecretValue`], where the label may *be* the secret, and
    /// [`DiscardReason::LabelLooksLikeAValue`], which exists precisely because
    /// the label is a value — a url with credentials in it, or a `host:port`.
    /// Quoting it back in the warning would put the refused text into
    /// [`super::super::graph::ArchGraph::warnings`] and from there into an
    /// exported diagram, which is the leak the refusal was for.
    ///
    /// Every other reason keeps it, because a warning naming nothing is nearly
    /// as useless as no warning — including
    /// [`DiscardReason::DetailLooksLikeAValue`], whose label passed its own
    /// screen and is safe to name.
    pub label: Option<String>,
    pub path: PathBuf,
    pub line: Option<u32>,
    pub reason: DiscardReason,
}

/// What [`admit`] produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Admitted {
    /// Sorted by [`Component::id`].
    pub components: Vec<Component>,
    /// Everything seen and refused, sorted. Its length is the count the module
    /// documentation promises: nothing is dropped without landing here.
    pub discarded: Vec<Discarded>,
}

impl Admitted {
    /// One edge per usage, deduplicated and sorted.
    pub fn edges(&self) -> Vec<AdmittedEdge> {
        let mut edges: Vec<AdmittedEdge> = self
            .components
            .iter()
            .flat_map(|component| {
                component.usages.iter().map(move |usage| AdmittedEdge {
                    project_id: usage.project_id.clone(),
                    component_id: component.id.clone(),
                })
            })
            .collect();
        edges.sort();
        edges.dedup();
        edges
    }

    /// The refusals rendered for [`super::super::graph::ArchGraph::warnings`].
    ///
    /// Prose rather than the structured form because that field is what the
    /// user reads. The wording always names the file, so the refusal can be
    /// checked; it names the label only when [`Discarded::label`] kept one.
    ///
    /// The project is named by its raw [`crate::model::Project::id`], which is
    /// almost never what a reader wants — see [`Self::warnings_named`], which is
    /// what the assembly step calls.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings_named(|project_id| project_id.to_string())
    }

    /// The same refusals, with the project named by whatever `name` returns.
    ///
    /// Every refusal opens with the project it came from, and this phase holds
    /// only a [`crate::model::Project::id`] to open it with: a [`Signal`] carries
    /// an id because a producer has no reason to carry a whole project. That id
    /// is drawn on nothing, labels nothing and — since
    /// [`crate::workspace::project_id`] flattens the separators out of it — is
    /// not even a path the reader can open, so a warning list mixing it with the
    /// producers' display names reads in two vocabularies.
    ///
    /// Translating here rather than in the caller keeps the refusal's wording in
    /// one place. The caller supplies the lookup because it is the only layer
    /// that has one: [`super::super::components::component_graph`] holds the
    /// [`crate::workspace::Workspace`], and the gate deliberately does not.
    pub fn warnings_named(&self, name: impl Fn(&str) -> String) -> Vec<String> {
        let mut warnings: Vec<String> = self
            .discarded
            .iter()
            .map(|discarded| render_warning(discarded, &name(&discarded.project_id)))
            .collect();
        warnings.sort();
        warnings.dedup();
        warnings
    }
}

fn render_warning(discarded: &Discarded, project: &str) -> String {
    let path = match discarded.line {
        Some(line) => format!("{}:{line}", display_path(&discarded.path)),
        None => display_path(&discarded.path),
    };
    let subject = match &discarded.label {
        Some(label) => format!("'{label}'"),
        None => "a candidate".to_string(),
    };
    let kind = discarded.kind.slug();
    let why = match discarded.reason {
        DiscardReason::SecretValue => {
            "its text contained a connection-string value, which must never reach a diagram; \
             only the configuration key may be used as a label"
        }
        DiscardReason::MediumWithoutHigh => {
            "it is a supporting signal and nothing declared this component in a manifest, so \
             there was nothing for it to enrich"
        }
        DiscardReason::NotADeclaration => {
            "its evidence is an import line or a comment, which says a name resolves and not \
             that the program uses it"
        }
        DiscardReason::UnverifiableEvidence => {
            "it claimed to be a declared fact but cited a file that is not a manifest or a \
             configuration file"
        }
        DiscardReason::Incomplete => "it arrived without a label, a project or a file to cite",
        DiscardReason::LabelLooksLikeAValue => {
            "its label is shaped like a configuration value rather than a component name"
        }
        DiscardReason::DetailLooksLikeAValue => {
            "the supporting text it carried is shaped like a configuration value rather than \
             prose, and that text would be quoted verbatim if the note were reported"
        }
    };
    format!(
        "{project}: {subject} ({kind}) read from {path} was not drawn because {why} [{}]",
        discarded.reason.slug()
    )
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// Apply the grading rule to a batch of signals.
///
/// The one function in this phase that decides what exists. It is deliberately
/// pure and takes the whole batch at once rather than accepting signals one at
/// a time: MEDIUM signals may only attach to components HIGH signals created,
/// and a streaming interface would make that answer depend on arrival order —
/// a MEDIUM arriving before its HIGH would be refused, the same MEDIUM arriving
/// after would be kept. Since three independent producers feed this, that is
/// not a hypothetical. Taking the batch means every HIGH is known before any
/// MEDIUM is judged, and the result is a function of the *set* of signals.
///
/// Ordering inside the result is total and derived only from content, so the
/// output is byte-identical whichever order the producers ran in.
pub fn admit(signals: Vec<Signal>) -> Admitted {
    let mut discarded: Vec<Discarded> = Vec::new();
    let mut high: Vec<Signal> = Vec::new();
    let mut medium: Vec<Signal> = Vec::new();

    for signal in signals {
        match screen(&signal) {
            Some(reason) => discarded.push(discard(&signal, reason)),
            None => match signal.strength {
                Strength::High => high.push(signal),
                Strength::Medium => medium.push(signal),
            },
        }
    }

    // Components first, from HIGH only. Keyed on the identity rule argued on
    // [`Component`]; the display label is the smallest spelling seen so the
    // answer does not depend on which producer ran first.
    let mut by_key: BTreeMap<(ComponentKind, String), Component> = BTreeMap::new();
    for signal in &high {
        let key = (signal.kind, fold(&signal.label));
        let entry = by_key.entry(key.clone()).or_insert_with(|| Component {
            id: format!("component:{}:{}", signal.kind.slug(), slugify(&key.1)),
            kind: signal.kind,
            label: signal.label.clone(),
            usages: Vec::new(),
            details: Vec::new(),
        });
        if signal.label < entry.label {
            entry.label = signal.label.clone();
        }
        entry.usages.push(Usage {
            project_id: signal.project_id.clone(),
            evidence: signal.evidence.clone(),
        });
        // A HIGH signal carrying a detail contributes it too. It earned the
        // box; refusing its own annotation would be a rule with no purpose.
        if let Some(text) = &signal.detail {
            entry.details.push(Detail {
                project_id: signal.project_id.clone(),
                text: text.clone(),
                evidence: signal.evidence.clone(),
            });
        }
    }

    for signal in &medium {
        let key = (signal.kind, fold(&signal.label));
        let Some(component) = by_key.get_mut(&key) else {
            discarded.push(discard(signal, DiscardReason::MediumWithoutHigh));
            continue;
        };
        component.details.push(Detail {
            project_id: signal.project_id.clone(),
            text: signal
                .detail
                .clone()
                .unwrap_or_else(|| signal.label.clone()),
            evidence: signal.evidence.clone(),
        });
    }

    let mut components: Vec<Component> = by_key.into_values().collect();
    for component in &mut components {
        component.usages.sort();
        component.usages.dedup();
        component.details.sort();
        component.details.dedup();
    }
    components.sort_by(|a, b| a.id.cmp(&b.id));

    // Sorted but **not** deduplicated, unlike the components. `discarded` is a
    // count of refusals, and two candidates that reduce to the same record —
    // two signals from one line of one file, refused for one reason — are still
    // two things this module looked at and declined. Collapsing them would make
    // "counted, not silent" quietly under-report. The duplicate prose that
    // would follow is removed in `warnings`, where repetition is only noise.
    discarded.sort();

    Admitted {
        components,
        discarded,
    }
}

fn discard(signal: &Signal, reason: DiscardReason) -> Discarded {
    Discarded {
        project_id: signal.project_id.clone(),
        kind: signal.kind,
        strength: signal.strength,
        // The two reasons that cannot name their subject: in both, the label
        // is exactly the thing that must not be repeated. See
        // [`Discarded::label`].
        label: (!matches!(
            reason,
            DiscardReason::SecretValue | DiscardReason::LabelLooksLikeAValue
        ))
        .then(|| signal.label.clone()),
        path: signal.evidence.path.clone(),
        line: signal.evidence.line,
        reason,
    }
}

/// Every reason a signal is refused outright, in the order they are checked.
///
/// The secret screen runs first and over every text field, because the cost of
/// the checks disagreeing is a leaked credential; a signal that would have been
/// refused for a duller reason anyway loses nothing by being refused for this
/// one instead.
fn screen(signal: &Signal) -> Option<DiscardReason> {
    let texts = [
        signal.label.as_str(),
        signal.detail.as_deref().unwrap_or(""),
        signal.evidence.excerpt.as_str(),
    ];
    if texts.iter().any(|text| contains_secret_assignment(text)) {
        return Some(DiscardReason::SecretValue);
    }

    if signal.label.trim().is_empty()
        || signal.project_id.trim().is_empty()
        || signal.evidence.path.as_os_str().is_empty()
    {
        return Some(DiscardReason::Incomplete);
    }

    if looks_like_a_value(&signal.label) {
        return Some(DiscardReason::LabelLooksLikeAValue);
    }

    // The same shape test over the enrichment text, because the enrichment
    // text is published too: `components::cross_project_notes` prints it
    // verbatim. Screening only the label left the whole hazard resting on each
    // producer choosing not to interpolate a value into its own prose, which
    // is the per-producer discipline this gate exists to stop relying on.
    //
    // Not applied to `Evidence::excerpt`: an excerpt is a quotation of a line
    // by construction, so a value shape is what it is *supposed* to have, and
    // no excerpt reaches the graph. `Evidence::elided_value` is how a producer
    // cites a line whose value must not be read.
    if signal.detail.as_deref().is_some_and(looks_like_a_value) {
        return Some(DiscardReason::DetailLooksLikeAValue);
    }

    if is_import_or_comment(&signal.evidence.excerpt) {
        return Some(DiscardReason::NotADeclaration);
    }

    if signal.strength == Strength::High && !is_declaration_file(&signal.evidence.path) {
        return Some(DiscardReason::UnverifiableEvidence);
    }

    None
}

/// Whether a piece of text contains a `key=value` pair from a connection
/// string.
///
/// Matching the **key**, not the value, is the whole trick. Trying to recognise
/// a credential by its shape is hopeless — `hunter2` looks like a word and
/// `Orders` looks like a password — but the keys are a small, stable,
/// documented vocabulary that Microsoft, Npgsql, MySQL and the Azure SDKs all
/// spell the same way. Anything carrying one of them is refused whole rather
/// than redacted: a redactor that is 99% right still ships the 1% into a
/// diagram somebody exports, and there is a sanctioned way to cite these lines
/// without their values ([`Evidence::elided_value`]).
///
/// Deliberately blunt about false positives. A refused signal costs an arrow
/// and produces a warning naming the file; a missed one costs a password. The
/// list is not exhaustive and is not claimed to be — it is the reason producers
/// are told never to read these values in the first place, with this as the
/// backstop rather than the plan.
fn contains_secret_assignment(text: &str) -> bool {
    const KEYS: &[&str] = &[
        "password",
        "pwd",
        "user id",
        "userid",
        "uid",
        "accountkey",
        "sharedaccesskey",
        "shared access key",
        "accesskey",
        "secret",
        "apikey",
        "api key",
        "token",
        "integrated security",
        "trusted_connection",
        "initial catalog",
        "data source",
        "datasource",
        "server",
        "host",
        "hostname",
        "port",
        "database",
        "endpoint",
        "sslmode",
        "encrypt",
        "connectiontimeout",
        "connection timeout",
        "connect timeout",
        "account",
        "user",
    ];

    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for key in KEYS {
        let mut from = 0;
        while let Some(found) = lower[from..].find(key) {
            let start = from + found;
            let end = start + key.len();
            from = start + 1;

            // The key has to stand on its own, so `passwordless` and the
            // `port` inside `report` do not trip it.
            if start > 0 && is_key_char(bytes[start - 1]) {
                continue;
            }
            // …and it has to be an assignment: only `key = value` puts a value
            // to the right of it. `Host` on its own in prose is a word.
            let mut cursor = end;
            while cursor < bytes.len() && bytes[cursor] == b' ' {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'=' {
                return true;
            }
        }
    }
    false
}

fn is_key_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether a piece of published text is shaped like a value rather than a name.
///
/// A second, independent guard against the same failure as
/// [`contains_secret_assignment`], catching the shapes that carry no key at
/// all: a bare host (`db.internal:5432`), a URL, a whole connection string
/// pasted in as a label. A component name is a short human word; none of these
/// are, and none of them is something a producer has any business emitting.
///
/// Visible to the whole of [`super::super`] rather than private so that every
/// publisher can reach the *same* test for the text it puts into warnings, which
/// the gate never sees. Two hand-written guards against one hazard is one guard
/// plus a copy that drifts, and it did drift: [`super::dotnet`]'s copy checked
/// `=`, `;` and `://` but not `host:port`, so a connection-string key spelled
/// `redis-prod.internal:6380` passed the producer's guard and landed in an
/// exported diagram while the same text as a label was refused here.
///
/// Two callers outside this module reach it for text the gate never grades:
/// [`super::super::components::Projects::display_name`], because a project's
/// *name* comes from a `package.json` and can be a credentialed url, and
/// [`super::super::graph::quotable_path`], which layers two path-specific
/// refusals on top rather than restating this one.
pub(in crate::architecture) fn looks_like_a_value(text: &str) -> bool {
    if text.contains('=') || text.contains(';') || text.contains("://") {
        return true;
    }
    // A `:` followed by digits is a port. `Microsoft::Something` is not.
    if let Some(rest) = text.split_once(':').map(|(_, rest)| rest) {
        if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    text.chars().count() > 120
}

/// Whether an excerpt is an import line or a comment.
///
/// Both are refused for the same reason: neither is a statement about what the
/// program *does*. `using Npgsql;` says the compiler can resolve a namespace,
/// which is true of every transitively referenced assembly whether or not a
/// single line of it ever runs; and a commented-out registration says the
/// opposite of what it appears to say while looking identical to a live one.
///
/// Checked on the excerpt rather than by asking producers not to emit these,
/// because this is the prohibition most likely to be re-derived independently
/// by whoever adds the next producer.
fn is_import_or_comment(excerpt: &str) -> bool {
    let trimmed = excerpt.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("using ")
        || lower.starts_with("import ")
        || lower.starts_with("from ")
        || lower.starts_with("global using ")
        || trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with('#')
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("--")
}

/// Whether a path names a file whose contents are a declaration by the author.
///
/// The allowlist is what separates HIGH from everything else in practice. A
/// `.csproj`, a `package.json`, an `appsettings.json`, a `Dockerfile` — these
/// exist to state facts about how the software is assembled and deployed, and a
/// line in one means what it says. A `.cs` file states what some code compiles
/// to, which is a different question, so a HIGH signal citing one is refused
/// rather than silently demoted: the producer claimed something it cannot
/// support, and quietly downgrading it would hide the mistake instead of
/// reporting it.
///
/// Extensions, not full names, wherever an ecosystem uses a family of them
/// (`appsettings.Development.json`, `Directory.Build.props`). Deliberately does
/// **not** include `.yml`/`.yaml`: this crate has no YAML parser, so nothing
/// can honestly claim to have read one.
fn is_declaration_file(path: &Path) -> bool {
    const NAMES: &[&str] = &["dockerfile", "makefile", "procfile"];
    const EXTENSIONS: &[&str] = &[
        "csproj", "fsproj", "vbproj", "props", "targets", "sln", "slnx", "json", "toml", "config",
        "nuspec", "sqlproj", "esproj",
    ];

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if NAMES.contains(&name.as_str()) {
        return true;
    }
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.as_str()))
}

/// The identity key for a label: trimmed, case-folded, inner whitespace
/// collapsed. See [`Component`] for the argument.
fn fold(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// A folded label reduced to something safe to put in an id.
///
/// Ids end up in stored graphs and in Mermaid source, so they are restricted to
/// characters neither can misread. Runs of everything else collapse to a single
/// `-`, which can make two different labels share an id; that is acceptable
/// precisely because the id is not the identity — [`fold`] is, and it ran first.
fn slugify(folded: &str) -> String {
    let mut out = String::with_capacity(folded.len());
    let mut pending_separator = false;
    for ch in folded.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' {
            if pending_separator && !out.is_empty() {
                out.push('-');
            }
            pending_separator = false;
            out.push(ch);
        } else {
            pending_separator = true;
        }
    }
    out
}

/// Forward slashes on every platform, for the same reason
/// [`super::super::graph`] does it: these strings are read by a person and
/// stored in files that move between machines.
fn display_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
