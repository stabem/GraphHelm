//! Context reaches the node (#1065): bounded lexical retrieval over the project, a capsule in
//! the prompt, measured accounting.
//!
//! This module is the CHAIN the census in #1065 found missing. Every piece already existed —
//! the bounded search port, the source reader, `context_compiler::fit_within_budget` and
//! `compile_capsule`, the accounting receipt — and nothing called them in sequence for a running
//! node. What lives here is that sequence and nothing else: objective → terms → bounded search →
//! bounded prefix reads → budget fit → capsule bytes → prompt field + receipt numbers.
//!
//! **Bounded work before expensive work, in this order.** The term list is capped before the
//! search runs; the search runs within [`SEARCH_BOUNDS`]; at most [`MAX_CANDIDATES`] candidates are
//! read, each as a prefix of at most [`MAX_BYTES_PER_CANDIDATE`], and the shipped total never
//! crosses [`MAX_TOTAL_CANDIDATE_BYTES`]; then the node's own budget decides what fits. A ceiling
//! REFUSES the item it would cut and COUNTS it — nothing is trimmed to fit. The one declared
//! partial is the prefix read itself, and it is declared in the item text (`bytes 0..n of len`)
//! rather than hidden.
//!
//! **Content-free accounting.** [`NodeContextSummary`] carries paths, counts, an estimator id
//! and a digest — never a byte of what was read. It is what the drive reply publishes and what
//! the sealed provenance beside the model reply records (`context-provenance@1`,
//! [`ContextProvenanceRecord`]), so the two cannot disagree.
//!
//! **Why the numbers live in their own sealed document and not in the accounting receipt's
//! lines.** The receipt (`execution-accounting-receipt` 1.0.0) is not part of the frozen 1.0.0
//! release, and the house holds two rules at once: against `main`, a change the compatibility
//! comparator classes as breaking — a swapped positional `$ref`, a new `prefixItems` position —
//! must move the document to exactly `major + 1`; against the frozen release, a schema absent
//! from it must stay `1.0.0`. An unreleased schema therefore admits only comparator-compatible
//! changes, and no reshaping of the receipt's positional lines is one. So the receipt's six
//! context lines stay `unavailable` with their frozen note, and the SAME `CostField` vocabulary —
//! `measured` by `context_retrieval`, `derived` under `bytes-div-4/v1` — is written into the
//! record sealed beside the reply, under a schema added whole. The receipt's lines move at the
//! next frozen baseline.
//!
//! **Deterministic by construction.** No clock, no randomness: the terms are a pure function of
//! the objective, the channel ranks by (distinct terms matched, path), and the capsule id is
//! derived from the execution, node and attempt. The same tree and the same node compile to the
//! same bytes and the same digest.
//!
//! **The estimator is an estimator.** `bytes-div-4/v1` divides bytes by four and floors. It is the
//! roadmap §9.2 v1 method — "a free compiler byproduct … stated explicitly as a conservative
//! lower bound" — recorded under `derived` provenance with the method id in the note, never under
//! `measured`. A provider's own count of the capsule's tokens does not exist today; this is the
//! number that lets `tokens_saved` be a checkable claim instead of nothing.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use graphhelm_protocols::GraphNode;
use serde::{Deserialize, Serialize};

use crate::executor::ExecutorRefusal;

use crate::context_accounting::CostField;
use crate::context_compiler::{
    BudgetOutcome, ExpansionRequest, base_digest, compile_capsule, fit_within_budget,
};
use crate::ports::{
    BoundedSourceReader, BoundedSourceSearch, ExecutionTreeAccess, ExecutionTreePort, ScanCancel,
    SourceReadError, SourceSearchBounds, SourceSearchError, SourceSearchOrigin, SourceSearchReason,
};

/// The tokenizer's own version, recorded in every summary so a change to the rules below
/// starts a new series rather than silently moving old numbers.
pub const TOKENIZER_ID: &str = "objective-terms/v1";

/// The token estimator's method id (roadmap §9.2): `tokens = bytes / 4`, floored.
pub const ESTIMATOR_ID: &str = "bytes-div-4/v1";

/// The component that observes the retrieval counters. Named on every `measured` receipt line
/// this module produces; the accounting module refuses to be named as an observer itself.
pub const RETRIEVAL_PRODUCER: &str = "context_retrieval";

/// Media type of the sealed, content-free provenance record placed beside a model reply.
pub const CONTEXT_PROVENANCE_MEDIA_TYPE: &str = "application/vnd.graphhelm.context-provenance+json";

/// Terms shorter than this are dropped: they are almost always articles, particles and noise.
pub const MIN_TERM_CHARS: usize = 3;

/// The most terms one objective may contribute. Also the search port's declared `max_terms`, so
/// the two cannot disagree.
pub const MAX_TERMS: usize = 12;

/// The longest term kept. A single run of 65+ alphanumerics is not a word anyone searches by
/// (a digest, a key, a minified blob); dropping it keeps every stored term inside the
/// `context-provenance@1` schema's own `maxLength` and keeps the query-side byte bound honest.
pub const MAX_TERM_CHARS: usize = 64;

/// The largest `context.budgetBytes` a node may declare. A budget is a ceiling on what reaches
/// a model; a ceiling above this is an operator typo, not a decision, and is refused.
pub const MAX_BUDGET_BYTES: usize = 1024 * 1024;

/// English function words that carry no retrieval signal. Fixed, small, and part of the
/// tokenizer's identity: a change here is a change to [`TOKENIZER_ID`].
pub const STOP_WORDS: [&str; 40] = [
    "the", "and", "for", "that", "this", "with", "from", "into", "why", "than", "then", "when",
    "what", "which", "where", "while", "does", "did", "has", "have", "had", "are", "was", "were",
    "will", "would", "should", "could", "can", "not", "but", "you", "your", "its", "our", "any",
    "all", "each", "every", "some",
];

/// Candidates read per node. The channel is asked for exactly this many (`max_results`), and a
/// channel that returns more sees the surplus REFUSED and counted, never silently ignored.
pub const MAX_CANDIDATES: usize = 8;

/// Bytes read from the start of one candidate.
pub const MAX_BYTES_PER_CANDIDATE: u64 = 16 * 1024;

/// Bytes shipped across all candidates before the node's own budget is consulted.
pub const MAX_TOTAL_CANDIDATE_BYTES: u64 = 64 * 1024;

/// The node budget when the node declares none (`context.budgetBytes`).
pub const DEFAULT_BUDGET_BYTES: usize = 32 * 1024;

/// The largest integer `context-provenance@1` admits (`maximum: 9007199254740991` on every
/// counter — JSON's exactly-representable ceiling). A candidate whose declared length would
/// carry `eligibleCandidateBytes` past it is refused before the record is built, so the sealed
/// document never holds a number its own schema rejects.
pub const MAX_RECORDED_INTEGER: u64 = 9_007_199_254_740_991;

/// The shortest trailing hex run a CLIPPED excerpt is refused for. At the cut nothing says how
/// long the run really is: 32 is half a 64-hex key, well above a short git sha (7–20), and a
/// full 40-hex sha that happens to end exactly at the boundary is the false positive this
/// accepts — one candidate and a count, against 63/64 of a key. A `sha256:` prefix does not
/// exempt a run AT the cut (#1086): a whole digest ending exactly at the boundary may be the
/// first 64 characters of a longer run nobody read.
pub const TRAILING_HEX_FRAGMENT_CHARS: usize = 32;

/// The longest source path `context-provenance@1` admits (`maxLength: 4096` on
/// `repositoryRelativePath`, counted in characters as JSON Schema counts). A candidate whose
/// relative path is longer is refused — and counted as dropped — before it is read, rendered
/// into a `source://` citation or recorded, so the sealed document never names a path its own
/// schema rejects. The length is one half of [`recordable_source_path`]; the pattern is the other.
pub const MAX_SOURCE_PATH_CHARS: usize = 4096;

/// Whether `path` is a `repositoryRelativePath` as `context-provenance@1` defines it (#1086 item
/// 11): 1 to [`MAX_SOURCE_PATH_CHARS`] characters, no backslash, and `/`-separated segments that
/// are neither empty nor `.` nor `..` — the schema's pattern, which is segment-aware (`.env`,
/// `..x` and `...` are names). A search channel is trusted for WHICH paths it returns, never for
/// their SHAPE: a path the record could not carry is refused before it is read, cited or recorded.
#[must_use]
pub fn recordable_source_path(path: &str) -> bool {
    !path.is_empty()
        && path.chars().count() <= MAX_SOURCE_PATH_CHARS
        && !path.contains('\\')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// Directory NAMES that are the factory's own working state, never repository evidence, at any
/// depth: the process diary (`.factory`, `.superpowers`), the object store (`.git`) and the
/// credential locations (`.graphhelm`, `keyring`). The workspace channel skips the same names in
/// its walk; [`sensitive_path`] refuses them again before any read, so a channel that forgot the
/// list would still not ship them.
pub const INTERNAL_DIRS: [&str; 5] = [".factory", ".superpowers", ".git", ".graphhelm", "keyring"];

/// The assignment KEYS whose value is a credential: `password = hunter2`, `token: ghp_x`,
/// `client_secret="..."`. Matched case-insensitively at a word boundary (`github_token` counts,
/// `mytoken` does not), followed by optional blanks, `=` or `:`, optional blanks, an optional
/// quote and then a value that looks like a secret. `password:` at the end of a line — a
/// heading, a YAML key with its value on the next line — is not an assignment with a value;
/// `token: String` or `password: Option<String>` is a type annotation, not a credential, and
/// neither is refused (`assignment_value_is_secret` holds the rule).
pub const SECRET_ASSIGNMENT_KEYS: [&str; 10] = [
    "password",
    "passwd",
    "token",
    "secret",
    "api_key",
    "apikey",
    "private_key",
    "client_secret",
    "access_token",
    "auth_token",
];

/// What the workspace channel may spend on one node's search. Declared here, once, as the
/// port's own doc demands ("one declared struct, one oracle").
pub const SEARCH_BOUNDS: SourceSearchBounds = SourceSearchBounds {
    max_entries_visited: 50_000,
    max_files_scanned: 20_000,
    max_bytes_scanned: 256 * 1024 * 1024,
    max_results: MAX_CANDIDATES as u32,
    max_terms: MAX_TERMS,
    max_term_bytes: 1024,
};

/// Derive query terms from a node objective — `objective-terms/v1`.
///
/// Lowercase; split on every non-alphanumeric character; drop terms shorter than
/// [`MIN_TERM_CHARS`] and the [`STOP_WORDS`]; dedupe preserving first occurrence; cap at
/// [`MAX_TERMS`]. The split rule is what makes the objective unable to name a path: `../` and
/// `/` are separators, so no term ever contains a component the reader could follow.
#[must_use]
pub fn objective_terms(objective: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw in objective
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
    {
        let chars = raw.chars().count();
        if !(MIN_TERM_CHARS..=MAX_TERM_CHARS).contains(&chars) || STOP_WORDS.contains(&raw) {
            continue;
        }
        if terms.iter().any(|term| term == raw) {
            continue;
        }
        terms.push(raw.to_owned());
        if terms.len() == MAX_TERMS {
            break;
        }
    }
    terms
}

/// `bytes-div-4/v1`: the v1 estimator, floored.
#[must_use]
pub const fn estimate_tokens(bytes: u64) -> u64 {
    bytes / 4
}

/// The two ports the chain needs plus the ledger the caller reads back, cloned into the drive.
///
/// Absent ports (`None` on the drive call) are today's behaviour: no search, no capsule, every
/// context field of the receipt `unavailable`. Present ports make the search run for every
/// plain cognitive node — the blind judge keeps its own diet and is deliberately excluded.
///
/// **Which tree is read (#1086 item 5).** `search` and `reader` are the PROJECT checkout.
/// `execution_tree`, when the drive has one, is the execution's own Tier 1 tree — the tree its
/// tool nodes patch and commit in. A node compiled while that tree exists reads it instead of
/// the project, so a cognitive node that runs after a tool node sees the tool's work rather than
/// pre-tool excerpts, and the summary's `root` says which tree was read.
#[derive(Clone)]
pub struct ContextPorts {
    pub search: Arc<dyn BoundedSourceSearch>,
    pub reader: Arc<dyn BoundedSourceReader>,
    pub ledger: ContextLedger,
    pub execution_tree: Option<Arc<dyn ExecutionTreePort>>,
}

/// Which tree a node's context was retrieved from (#1086 item 5). Content-free: a name, never a
/// path. Recorded in the summary, the drive reply and the sealed `context-provenance@1` record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRoot {
    /// The project checkout the drive resolved (request, `--project`, working directory). Also
    /// what a record sealed before #1086 read, which is why it is the default when absent.
    #[default]
    Project,
    /// The execution's own Tier 1 tree (`ToolHost`'s execution workspace, provisioned from
    /// `refs/graphhelm/executions/<id>` when a prior drive landed it).
    Execution,
}

/// The per-node summaries a drive produced, in the caller's hands once the drive returns.
///
/// The driver returns a projection and only a projection; the content-free summary of what
/// each node ran with is recorded here as it is compiled, so the caller that owns the drive
/// (the HTTP `start`/`resume` handler) can publish it beside the projection without reading
/// sealed evidence back. Keyed by node id; a retried node overwrites its earlier attempt.
#[derive(Clone, Default)]
pub struct ContextLedger(Arc<Mutex<BTreeMap<String, NodeContextSummary>>>);

impl ContextLedger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, node: &str, summary: NodeContextSummary) {
        self.0
            .lock()
            .expect("the context ledger mutex is never poisoned: no panic holds it")
            .insert(node.to_owned(), summary);
    }

    #[must_use]
    pub fn snapshot(&self) -> BTreeMap<String, NodeContextSummary> {
        self.0
            .lock()
            .expect("the context ledger mutex is never poisoned: no panic holds it")
            .clone()
    }
}

/// Why a node ran with less than a full capsule. One value, so a summary cannot claim two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextFallback {
    /// The objective yielded no term; no query was issued.
    NoTerms,
    /// The search ran and returned nothing.
    NoCandidates,
    /// The channel refused: no workspace was wired or it could not be read.
    SearchUnavailable,
    /// The search would have crossed a declared ceiling and refused rather than truncate.
    SearchBoundExceeded,
    /// Candidates were returned and not one could be read.
    NoReadableCandidate,
    /// Candidates were read, and every rendered item exceeded the node's budget (or the total
    /// byte bound). The remedy is a larger budget, not a different tree.
    NothingFitsBudget,
    /// Every candidate that could be read was refused as a credential location or a
    /// secret-shaped excerpt. Nothing about the secret is recorded beyond this name.
    SecretShapedCandidate,
}

/// Content-free record of what one node ran with: paths, counts, estimator, digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeContextSummary {
    pub tokenizer: String,
    pub estimator: String,
    /// Which tree the search and the reads ran over (#1086). Absent in a record sealed before
    /// #1086, which read the project.
    #[serde(default)]
    pub root: ContextRoot,
    /// Where candidate paths came from. Optional on read for records sealed before this field
    /// existed; newly emitted records always include it.
    #[serde(default)]
    pub search_origin: SourceSearchOrigin,
    /// Closed, content-free reason for the search origin. It never carries paths or query text.
    #[serde(default)]
    pub search_reason: SourceSearchReason,
    pub terms: Vec<String>,
    /// Repository-relative paths shipped in the capsule, in rank order.
    pub sources: Vec<String>,
    /// How many of `sources` were shipped as a declared prefix rather than whole.
    pub excerpted_sources: u64,
    pub candidates_returned: u64,
    /// Candidates refused whole by the candidate cap or the total byte bound — counted, never
    /// trimmed.
    pub candidates_dropped: u64,
    /// Candidates the reader refused (escape, not a regular file, not text).
    pub candidates_unreadable: u64,
    /// Candidates refused BEFORE their bytes could enter the capsule because the path is a
    /// credential location (`.env*`, `*.key`, `*.token`, `.graphhelm/`, `keyring/`) or the
    /// excerpt carries a secret shape (`secret_shapes`). Counted, never named: the count is the
    /// only content-free fact about a secret.
    pub candidates_secret_shaped: u64,
    /// Items `fit_within_budget` left out of the node's own budget.
    pub dropped_optional: u64,
    pub budget_bytes: u64,
    pub capsule_bytes: u64,
    /// Total length of every candidate the search returned and the reader could size, before
    /// any cut — the naive full-context baseline the estimator is measured against.
    pub eligible_candidate_bytes: u64,
    pub eligible_candidate_tokens: u64,
    pub compiled_input_tokens: u64,
    pub tokens_saved: u64,
    pub retrieval_pages: u64,
    pub zero_result_queries: u64,
    pub retrieval_fallbacks: u64,
    pub fallback: Option<ContextFallback>,
    /// `base_digest` of the capsule bytes; absent when no capsule was compiled.
    pub digest: Option<String>,
}

impl NodeContextSummary {
    /// The sealed document: this summary plus its six accounting lines in the receipt's own
    /// `CostField` vocabulary.
    #[must_use]
    pub fn provenance_record(&self) -> ContextProvenanceRecord {
        let compiled = self.compiled_input_tokens;
        let eligible = self.eligible_candidate_tokens;
        let saved = self.tokens_saved;
        let measured = |name: &'static str, value: u64| ProvenanceLine {
            name,
            cost: CostField::measured(value, RETRIEVAL_PRODUCER),
        };
        let derived = |name: &'static str, value: u64, basis: String| ProvenanceLine {
            name,
            cost: CostField::derived(value, &basis),
        };
        ContextProvenanceRecord {
            schema_version: CONTEXT_PROVENANCE_SCHEMA_VERSION,
            summary: self.clone(),
            accounting: vec![
                measured("zero_result_queries", self.zero_result_queries),
                measured("retrieval_pages", self.retrieval_pages),
                measured("retrieval_fallbacks", self.retrieval_fallbacks),
                derived(
                    "compiled_input_tokens",
                    compiled,
                    format!(
                        "{ESTIMATOR_ID}: {} capsule bytes / 4, floored; an estimate, not a provider count",
                        self.capsule_bytes
                    ),
                ),
                derived(
                    "eligible_candidate_tokens",
                    eligible,
                    format!(
                        "{ESTIMATOR_ID}: {} bytes across every candidate the search returned before the budget cut / 4, floored",
                        self.eligible_candidate_bytes
                    ),
                ),
                derived(
                    "tokens_saved",
                    saved,
                    format!(
                        "{ESTIMATOR_ID}: eligible {eligible} - shipped {compiled}, floored at 0"
                    ),
                ),
            ],
        }
    }
}

/// Version of the sealed `context-provenance` document (`schemas/context-provenance.schema.json`).
pub const CONTEXT_PROVENANCE_SCHEMA_VERSION: &str = "1.0.0";

/// One accounting line of the provenance record: the receipt's `CostField` shape under a name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProvenanceLine {
    pub name: &'static str,
    #[serde(flatten)]
    pub cost: CostField,
}

/// The sealed, content-free record placed beside a model reply: the summary and six accounting
/// lines — three `measured` counters, three `derived` estimates — in the accounting receipt's
/// own vocabulary. Field order is fixed by construction; no clock, no generated id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextProvenanceRecord {
    pub schema_version: &'static str,
    #[serde(flatten)]
    pub summary: NodeContextSummary,
    pub accounting: Vec<ProvenanceLine>,
}

impl ContextProvenanceRecord {
    /// The bytes that seal. Infallible by construction: every field is a string, an integer, a
    /// list of strings or a closed enum.
    #[must_use]
    pub fn stable_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a content-free record serializes")
    }
}

/// What reaches the prompt and what reaches the record, built together so they agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledContext {
    /// The capsule bytes as text (every part is UTF-8 by construction), or empty when nothing
    /// was shipped. This is the third field of the assembled prompt.
    pub text: String,
    pub summary: NodeContextSummary,
}

/// Compile the context one cognitive node runs with.
///
/// The budget is the node's `context.budgetBytes` when present, else [`DEFAULT_BUDGET_BYTES`].
/// The capsule id is `<execution>/<node>/a<attempt>`, so a retry compiles a distinct capsule
/// identity over the same tree.
///
/// # Errors
/// [`ExecutorRefusal::Unassemblable`] when `context.budgetBytes` is present and is not a
/// positive integer at most [`MAX_BUDGET_BYTES`]. A declared ceiling that cannot be read is a
/// typo, not permission to apply the default: the node is refused, never silently defaulted.
pub fn compile_for_node(
    ports: &ContextPorts,
    execution_id: &str,
    node_id: &str,
    attempt: u32,
    node: &GraphNode,
) -> Result<CompiledContext, ExecutorRefusal> {
    compile_for_node_cancellable(
        ports,
        execution_id,
        node_id,
        attempt,
        node,
        &ScanCancel::new(),
    )
}

/// [`compile_for_node`] with the scan's cancellation token (#1086): the driver sets `cancel` when
/// it abandons the compile, and the execution-tree port stops at its next check and lets go of
/// the tree. The capsule a cancelled compile returns is discarded by the driver, never dispatched.
///
/// # Errors
/// As [`compile_for_node`].
pub fn compile_for_node_cancellable(
    ports: &ContextPorts,
    execution_id: &str,
    node_id: &str,
    attempt: u32,
    node: &GraphNode,
    cancel: &ScanCancel,
) -> Result<CompiledContext, ExecutorRefusal> {
    let budget = declared_budget(node)?;
    let terms = objective_terms(&node.objective);
    let capsule_id = format!("{execution_id}/{node_id}/a{attempt}");
    if let Some(tree) = &ports.execution_tree {
        let mut compiled = None;
        let access = tree.with_tree(cancel, &mut |search, reader| {
            compiled = Some(retrieve_and_compile_in(
                ContextRoot::Execution,
                search,
                reader,
                &terms,
                &capsule_id,
                budget,
            ));
        });
        match (access, compiled) {
            (ExecutionTreeAccess::Absent, _) => {}
            (ExecutionTreeAccess::Read, Some(compiled)) => return Ok(compiled),
            // The execution HAS a tree and it could not be read: the project's bytes are the
            // pre-tool bytes this port exists to stop serving, so the node runs with a counted
            // `search_unavailable` fallback instead of a stale capsule.
            (ExecutionTreeAccess::Read | ExecutionTreeAccess::Unavailable, _) => {
                return Ok(retrieve_and_compile_in(
                    ContextRoot::Execution,
                    &UnreadableTree,
                    &UnreadableTree,
                    &terms,
                    &capsule_id,
                    budget,
                ));
            }
        }
    }
    Ok(retrieve_and_compile(
        ports.search.as_ref(),
        ports.reader.as_ref(),
        &terms,
        &capsule_id,
        budget,
    ))
}

/// The ports of an execution tree that exists and could not be opened: every search refuses.
struct UnreadableTree;

impl BoundedSourceSearch for UnreadableTree {
    fn search(
        &self,
        _terms: &[String],
        _bounds: &SourceSearchBounds,
    ) -> Result<Vec<String>, SourceSearchError> {
        Err(SourceSearchError::Unavailable)
    }
}

impl BoundedSourceReader for UnreadableTree {
    fn read_prefix(&self, _: &str, _: u64) -> Result<crate::ports::SourceExcerpt, SourceReadError> {
        Err(SourceReadError::Unreadable)
    }
}

/// The node's `context.budgetBytes`: absent means the default; present must be an integer in
/// `1..=MAX_BUDGET_BYTES`. `"0"`, `0`, a negative, a float, or a number above the ceiling refuse.
pub fn declared_budget(node: &GraphNode) -> Result<usize, ExecutorRefusal> {
    let Some(declared) = node
        .properties
        .get("context")
        .and_then(|context| context.get("budgetBytes"))
    else {
        return Ok(DEFAULT_BUDGET_BYTES);
    };
    let bytes = declared
        .as_u64()
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(ExecutorRefusal::Unassemblable)?;
    if bytes == 0 || bytes > MAX_BUDGET_BYTES {
        return Err(ExecutorRefusal::Unassemblable);
    }
    Ok(bytes)
}

/// The producer: search within bounds, read prefixes within bounds, fit, compile.
///
/// Never fails. A refused search, an empty result, an unreadable candidate — each is a counted
/// fallback in the summary and the node still runs with whatever was shipped (possibly nothing).
#[must_use]
pub fn retrieve_and_compile(
    search: &dyn BoundedSourceSearch,
    reader: &dyn BoundedSourceReader,
    terms: &[String],
    capsule_id: &str,
    budget: usize,
) -> CompiledContext {
    retrieve_and_compile_in(
        ContextRoot::Project,
        search,
        reader,
        terms,
        capsule_id,
        budget,
    )
}

/// [`retrieve_and_compile`] over the tree `root` names — the same producer, with the summary
/// saying which tree `search` and `reader` belong to.
#[must_use]
pub fn retrieve_and_compile_in(
    root: ContextRoot,
    search: &dyn BoundedSourceSearch,
    reader: &dyn BoundedSourceReader,
    terms: &[String],
    capsule_id: &str,
    budget: usize,
) -> CompiledContext {
    let mut summary = NodeContextSummary {
        tokenizer: TOKENIZER_ID.to_owned(),
        estimator: ESTIMATOR_ID.to_owned(),
        root,
        search_origin: SourceSearchOrigin::default(),
        search_reason: SourceSearchReason::default(),
        terms: terms.to_vec(),
        sources: Vec::new(),
        excerpted_sources: 0,
        candidates_returned: 0,
        candidates_dropped: 0,
        candidates_unreadable: 0,
        candidates_secret_shaped: 0,
        dropped_optional: 0,
        budget_bytes: budget as u64,
        capsule_bytes: 0,
        eligible_candidate_bytes: 0,
        eligible_candidate_tokens: 0,
        compiled_input_tokens: 0,
        tokens_saved: 0,
        retrieval_pages: 0,
        zero_result_queries: 0,
        retrieval_fallbacks: 0,
        fallback: None,
        digest: None,
    };

    if terms.is_empty() {
        summary.search_origin = SourceSearchOrigin::Fallback;
        summary.search_reason = SourceSearchReason::NoSearch;
        return fallback(summary, ContextFallback::NoTerms);
    }
    summary.retrieval_pages = 1;
    let search_result = match search.search_with_provenance(terms, &SEARCH_BOUNDS) {
        Ok(result) => result,
        Err(SourceSearchError::Unavailable) => {
            summary.search_origin = SourceSearchOrigin::Fallback;
            summary.search_reason = SourceSearchReason::SearchUnavailable;
            return fallback(summary, ContextFallback::SearchUnavailable);
        }
        Err(SourceSearchError::BoundExceeded) => {
            summary.search_origin = SourceSearchOrigin::Fallback;
            summary.search_reason = SourceSearchReason::SearchBoundExceeded;
            return fallback(summary, ContextFallback::SearchBoundExceeded);
        }
    };
    summary.search_origin = search_result.provenance.origin;
    summary.search_reason = search_result.provenance.reason;
    let candidates = search_result.paths;
    summary.candidates_returned = candidates.len() as u64;
    if candidates.is_empty() {
        summary.zero_result_queries = 1;
        return fallback(summary, ContextFallback::NoCandidates);
    }

    let mut shipped_total: u64 = 0;
    let mut optional: Vec<(String, String, bool)> = Vec::new();
    for (index, path) in candidates.iter().enumerate() {
        if index >= MAX_CANDIDATES {
            summary.candidates_dropped += 1;
            continue;
        }
        // A path `context-provenance@1` cannot carry — too long, or not the schema's
        // `repositoryRelativePath` shape (#1086) — is refused whole, before the read, the
        // citation and the record, and counted as dropped, like every other bound.
        if !recordable_source_path(path) {
            summary.candidates_dropped += 1;
            continue;
        }
        // A credential LOCATION is refused before a byte is read; a credential SHAPE is refused
        // after the read and before the bytes can enter an item. Either way the capsule never
        // carries it, and the record carries only a count. The NAME is checked for a secret
        // shape too: a path lands verbatim in the `source://` citation and in
        // `summary.sources`, so a file named after a token would ship the token by citation.
        if sensitive_path(path) || secret_shaped(path) {
            summary.candidates_secret_shaped += 1;
            continue;
        }
        let excerpt = match reader.read_prefix(path, MAX_BYTES_PER_CANDIDATE) {
            Ok(excerpt) => excerpt,
            Err(SourceReadError::Escape | SourceReadError::Unreadable) => {
                summary.candidates_unreadable += 1;
                continue;
            }
        };
        // The record is sealed under `context-provenance@1`, whose integers stop at
        // `MAX_RECORDED_INTEGER`. A declared length the record cannot carry — or one that
        // would push the running total past what it can carry — is refused whole and counted
        // as dropped by the byte bound (it is over every byte bound this chain has), before a
        // number the schema rejects can be built into the record.
        let Some(eligible) = summary
            .eligible_candidate_bytes
            .checked_add(excerpt.file_len)
            .filter(|total| *total <= MAX_RECORDED_INTEGER)
        else {
            summary.candidates_dropped += 1;
            continue;
        };
        summary.eligible_candidate_bytes = eligible;
        let Some(text) = excerpt_text(&excerpt.bytes) else {
            summary.candidates_unreadable += 1;
            continue;
        };
        let partial = (text.len() as u64) < excerpt.file_len;
        // A clipped prefix is checked twice: for a whole secret shape, and for a shape that
        // BEGINS inside the excerpt and is cut by the boundary — 63 of a 64-hex key's characters
        // pass the whole-shape rule and leak 63/64 of the key.
        if secret_shaped_in(path, text) || (partial && trailing_secret_fragment(text)) {
            summary.candidates_secret_shaped += 1;
            continue;
        }
        // A line that begins like a capsule boundary is quoted HERE, before the item is
        // rendered, so the capsule that is digested, sealed and shown is one text: the wire
        // framing (`executor::wire_prompt`) applies the same idempotent quote and changes
        // nothing. The declared range stays the bytes READ, not the quoted length.
        let quoted = crate::executor::neutralise_capsule_markers(text);
        let item = render_item(path, &quoted, text.len(), excerpt.file_len, partial);
        let item_len = item.len() as u64;
        if shipped_total.saturating_add(item_len) > MAX_TOTAL_CANDIDATE_BYTES {
            summary.candidates_dropped += 1;
            continue;
        }
        shipped_total += item_len;
        optional.push((path.clone(), item, partial));
    }

    let items: Vec<String> = optional.iter().map(|(_, item, _)| item.clone()).collect();
    let fitted_by_sum = match fit_within_budget(&[], &items, budget) {
        BudgetOutcome::Fits {
            included,
            dropped_optional,
        } => {
            summary.dropped_optional = dropped_optional as u64;
            included
        }
        // Unreachable with no required items — the required total is zero — but the arm is
        // written rather than `unwrap`ped so the type cannot panic if `required` ever grows.
        BudgetOutcome::Refused { .. } => Vec::new(),
    };
    // The budget bounds the CAPSULE, not the sum of its items: the compiled form adds the id,
    // the version, the section name, the counts and a length prefix per part. Items that fit by
    // sum are fitted again by their RENDERED bytes, in rank order — the walk `compile_items`
    // does (#1086 item 8): popping from the tail dropped a small late item first and then the
    // large early one that was the real overflow, and shipped nothing the small item fit.
    let (included, bytes, framing_dropped) =
        refit_rendered(capsule_id, &[], &fitted_by_sum, budget);
    summary.dropped_optional += framing_dropped;
    for (path, item, partial) in &optional {
        if included.contains(item) {
            summary.sources.push(path.clone());
            if *partial {
                summary.excerpted_sources += 1;
            }
        }
    }
    summary.eligible_candidate_tokens = estimate_tokens(summary.eligible_candidate_bytes);
    if included.is_empty() {
        // Three different remedies, three different names: raise the budget, look at the
        // tree, or accept that the only matches were credentials.
        let cause = if !optional.is_empty() || summary.candidates_dropped > 0 {
            ContextFallback::NothingFitsBudget
        } else if summary.candidates_secret_shaped > 0 && summary.candidates_unreadable == 0 {
            ContextFallback::SecretShapedCandidate
        } else {
            ContextFallback::NoReadableCandidate
        };
        return fallback(summary, cause);
    }
    summary.capsule_bytes = bytes.len() as u64;
    summary.compiled_input_tokens = estimate_tokens(summary.capsule_bytes);
    summary.tokens_saved = summary
        .eligible_candidate_tokens
        .saturating_sub(summary.compiled_input_tokens);
    summary.digest = Some(base_digest(&bytes));
    let text = String::from_utf8(bytes).expect("the capsule is compiled from UTF-8 strings");
    CompiledContext { text, summary }
}

/// Compile a capsule from caller-supplied items — the producer `development compile-context`
/// runs (#724's producer half): the same `fit_within_budget` + `compile_capsule` + digest the
/// node path uses, without a search in front of it.
///
/// # Errors
/// The allocated refusal code and the budget that would fit when the required items exceed
/// `budget` — by item sum, or by the rendered capsule's bytes once every optional item has been
/// dropped — required context is never dropped to fit.
pub fn compile_items(
    capsule_id: &str,
    required: &[String],
    optional: &[String],
    budget: usize,
) -> Result<
    CompiledItems,
    (
        graphhelm_protocols::DevelopmentRefusalCode,
        ExpansionRequest,
    ),
> {
    match fit_within_budget(required, optional, budget) {
        BudgetOutcome::Fits {
            included,
            dropped_optional,
        } => {
            // The budget bounds the CAPSULE, not the sum of its items — the same rule the node
            // path applies in `retrieve_and_compile`: the compiled form adds the id, the
            // version, the section name, the counts and a length prefix per part, so items that
            // fit by sum can overflow by framing. The optional items that fit by sum are then
            // fitted by their RENDERED bytes, in order and one at a time, the way the sum fit
            // walks them: an item that overflows the rendered form is dropped — and counted —
            // and the walk goes on, so a small item after a large one still ships (popping from
            // the tail would drop the small item first and then the large one, and refuse a
            // capsule the small item fits). Required items are never dropped, so a capsule
            // whose required items alone render over the budget is a refusal naming the budget
            // that would fit the rendered form. With NOTHING required there is nothing to
            // refuse for: the framing-only capsule is what the argument-free call (`{}` over
            // HTTP, the bare MCP tool) has compiled since before any budget existed, and it
            // keeps compiling.
            let (_, bytes, framing_dropped) =
                refit_rendered(capsule_id, required, &included[required.len()..], budget);
            let dropped_optional = dropped_optional as u64 + framing_dropped;
            if bytes.len() > budget && !required.is_empty() {
                return Err((
                    graphhelm_protocols::DevelopmentRefusalCode::ContextBudgetInsufficient,
                    ExpansionRequest {
                        required_budget: bytes.len(),
                    },
                ));
            }
            let capsule_bytes = bytes.len() as u64;
            Ok(CompiledItems {
                digest: base_digest(&bytes),
                capsule_bytes,
                compiled_input_tokens: estimate_tokens(capsule_bytes),
                dropped_optional,
            })
        }
        BudgetOutcome::Refused { code, expansion } => Err((code, expansion)),
    }
}

/// The rendered-bytes fit both producers share: `required` always kept, then each item of
/// `fitted_by_sum` in order, dropped (and counted) when the capsule rendered with it would cross
/// `budget`, so a later item that fits still ships after an earlier one that does not. When the
/// required items alone render over the budget nothing optional is tried. Returns the kept items,
/// the bytes of the last capsule that fit (or of the required-only capsule), and the drop count.
fn refit_rendered(
    capsule_id: &str,
    required: &[String],
    fitted_by_sum: &[String],
    budget: usize,
) -> (Vec<String>, Vec<u8>, u64) {
    let render = |items: &[String]| {
        compile_capsule(capsule_id, 1, &[("evidence".to_owned(), items.to_vec())])
    };
    let mut kept: Vec<String> = required.to_vec();
    let mut bytes = render(&kept);
    let mut dropped = 0u64;
    if bytes.len() > budget {
        return (kept, bytes, fitted_by_sum.len() as u64);
    }
    for item in fitted_by_sum {
        kept.push(item.clone());
        let candidate = render(&kept);
        if candidate.len() > budget {
            kept.pop();
            dropped += 1;
        } else {
            bytes = candidate;
        }
    }
    (kept, bytes, dropped)
}

/// What [`compile_items`] reports: identity and size, never the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledItems {
    pub digest: String,
    pub capsule_bytes: u64,
    pub compiled_input_tokens: u64,
    pub dropped_optional: u64,
}

fn fallback(mut summary: NodeContextSummary, cause: ContextFallback) -> CompiledContext {
    summary.retrieval_fallbacks = 1;
    summary.fallback = Some(cause);
    summary.eligible_candidate_tokens = estimate_tokens(summary.eligible_candidate_bytes);
    summary.tokens_saved = summary.eligible_candidate_tokens;
    CompiledContext {
        text: String::new(),
        summary,
    }
}

/// The longest valid UTF-8 prefix of the bytes read, or `None` for a file that is not text at
/// its start (a binary with a text suffix, or bytes cut inside a character with nothing before).
fn excerpt_text(bytes: &[u8]) -> Option<&str> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let valid = error.valid_up_to();
            if valid == 0 && !bytes.is_empty() {
                return None;
            }
            std::str::from_utf8(&bytes[..valid]).expect("valid_up_to bounds a valid prefix")
        }
    };
    Some(text)
}

/// Whether a repository-relative path is a credential location or the factory's own state: any
/// segment beginning with `.env`, a final segment ending in `.key` or `.token`, or any segment
/// — at any depth, not only the first — named in [`INTERNAL_DIRS`] (`.factory`, `.superpowers`,
/// `.git`, `.graphhelm`, `keyring`). Case-folded, because the tree may be.
#[must_use]
pub fn sensitive_path(path: &str) -> bool {
    let folded = path.to_lowercase();
    let segments: Vec<&str> = folded.split('/').filter(|s| !s.is_empty()).collect();
    let Some(last) = segments.last() else {
        return false;
    };
    segments
        .iter()
        .any(|segment| INTERNAL_DIRS.contains(segment))
        || segments.iter().any(|segment| segment.starts_with(".env"))
        || last.ends_with(".key")
        || last.ends_with(".token")
}

/// The conservative secret shapes an excerpt is refused for, by name, so the rule is one list.
///
/// A hex run of 64 characters or more that is not a `sha256:`/`sha256-` digest (digests are how
/// this repository names evidence, so they are not secrets — a longer run is not a digest and is
/// refused whole, `sha256:` in front of it or not); an `sk-`, `ghp_` or `AKIA`-prefixed
/// token; a PEM private-key block; a credential assignment — a key [`credential_key`] names
/// (`password=`, `AWS_SECRET_ACCESS_KEY=`, `"api_key":`, `client_secret = "..."`), blanks, a
/// closing key quote and an opening value quote normalised — with a value that looks like a
/// secret (`assignment_value_is_secret` holds the rule, including the configuration-file form
/// and a YAML block scalar whose value is the indented block below the key); an
/// `authorization:` header or a `Bearer` credential of 16+ characters carrying a digit.
/// False positives cost one candidate and a count; a false negative costs a credential.
pub const SECRET_SHAPES: [&str; 10] = [
    "hex64-not-a-digest",
    "sk-token",
    "ghp_token",
    "AKIA-key",
    "pem-private-key",
    "password=",
    "token=",
    "secret=",
    "authorization-bearer",
    "yaml-block-credential",
];

/// File suffixes whose `key: value` lines are CONFIGURATION rather than declarations (#1086).
/// Behind `:` a short bare value is a type annotation in source code (`token: String`) and a
/// value in these files (`client_secret: xyz`), so the suffix decides which rule applies.
pub const CONFIG_SUFFIXES: [&str; 9] = [
    ".yml",
    ".yaml",
    ".toml",
    ".ini",
    ".env",
    ".properties",
    ".json",
    ".conf",
    ".cfg",
];

/// Whether an assignment KEY names a credential (#1086), case-folded, with `-` and `.` read as
/// `_`: one of [`SECRET_ASSIGNMENT_KEYS`]; any `_`-segment beginning with `secret`
/// (`AWS_SECRET_ACCESS_KEY`, `client_secrets`); a last segment `token`, `password`, `passwd` or
/// `authorization` (`SLACK_BOT_TOKEN`, `DB_PASSWORD`); or `api_key`, `access_key`,
/// `private_key` anywhere, joined or not (`stripe_api_key`, `accesskey`). `tokenizer`,
/// `token_count`, `password_policy` and `mytoken` are not credential keys.
#[must_use]
pub fn credential_key(identifier: &str) -> bool {
    let folded = identifier.to_lowercase().replace(['-', '.'], "_");
    let segments: Vec<&str> = folded.split('_').filter(|s| !s.is_empty()).collect();
    let Some(last) = segments.last() else {
        return false;
    };
    SECRET_ASSIGNMENT_KEYS.contains(&folded.as_str())
        || segments.iter().any(|segment| segment.starts_with("secret"))
        || matches!(*last, "token" | "password" | "passwd" | "authorization")
        || ["api_key", "access_key", "private_key"]
            .iter()
            .any(|key| folded.contains(key))
        || segments
            .iter()
            .any(|segment| matches!(*segment, "apikey" | "accesskey" | "privatekey"))
}

/// Whether `path` names a configuration file by its suffix ([`CONFIG_SUFFIXES`]), case-folded.
#[must_use]
pub fn configuration_path(path: &str) -> bool {
    let folded = path.to_lowercase();
    CONFIG_SUFFIXES
        .iter()
        .any(|suffix| folded.ends_with(suffix))
}

/// Whether `text` carries any of [`SECRET_SHAPES`], judged as SOURCE: a short bare value behind
/// `:` is a type annotation here. [`secret_shaped_in`] is the same rule with the file's suffix
/// consulted.
#[must_use]
pub fn secret_shaped(text: &str) -> bool {
    secret_shaped_as(text, false)
}

/// [`secret_shaped`] for the excerpt of `path` (#1086): when the suffix is a configuration format
/// ([`CONFIG_SUFFIXES`]) a bare value behind `:` is refused unless it is a literal (`null`,
/// `true`, `false`, `none`, `~`) or opens a nested mapping or sequence, whose members are judged
/// on their own lines.
#[must_use]
pub fn secret_shaped_in(path: &str, text: &str) -> bool {
    secret_shaped_as(text, configuration_path(path))
}

fn secret_shaped_as(text: &str, configuration: bool) -> bool {
    if text.contains("-----BEGIN ") && text.contains("PRIVATE KEY-----") {
        return true;
    }
    let lower = text.to_lowercase();
    if secret_assignments(&lower)
        .into_iter()
        .any(|head| assignment_value_is_secret(&lower, head, configuration))
    {
        return true;
    }
    if bearer_credential(&lower) {
        return true;
    }
    // Token-wise: split on the same separators the tokenizer uses, keeping `-`, `_` inside a
    // token so a prefix survives as one word.
    let mut hex_run = 0usize;
    let mut hex_start = 0usize;
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if byte.is_ascii_hexdigit() {
            if hex_run == 0 {
                hex_start = index;
            }
            hex_run += 1;
        } else {
            if hex_run >= 64 && !digest_prefixed(text, hex_start, hex_run) {
                return true;
            }
            hex_run = 0;
        }
    }
    if hex_run >= 64 && !digest_prefixed(text, hex_start, hex_run) {
        return true;
    }
    for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        if (token.starts_with("sk-") && token.len() >= 20)
            || (token.starts_with("ghp_") && token.len() >= 24)
            || (token.starts_with("AKIA")
                && token.len() == 20
                && token[4..]
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
        {
            return true;
        }
    }
    false
}

/// Whether a CLIPPED excerpt ends in the head of a secret shape the boundary cut: a trailing
/// hex run of [`TRAILING_HEX_FRAGMENT_CHARS`] or more, `sha256:` in front of it or not (#1086:
/// a run that touches the cut has no known end, so a digest-shaped head is not a digest yet); a
/// final token carrying one of the recognised prefixes (`sk-`, `ghp_`, `AKIA`) whatever its
/// length, since the cut can fall one character after the prefix; a `-----BEGIN ` block with no
/// `-----END ` in the excerpt; a credential assignment head (`password =`, `token: "`) whose
/// value lies past the cut, including a YAML block-scalar head (`password: |`) whose indented
/// block does not begin before the cut.
///
/// Applied only when the bytes read are fewer than the file's length: a whole file is judged
/// by [`secret_shaped`] alone, because nothing was cut from it.
#[must_use]
pub fn trailing_secret_fragment(text: &str) -> bool {
    if text.contains("-----BEGIN ") && !text.contains("-----END ") {
        return true;
    }
    let lower = text.to_lowercase();
    if secret_assignments(&lower).into_iter().any(|head| {
        head.value_start == lower.len()
            || block_scalar_body(&lower, head).is_some_and(|body| body.trim().is_empty())
    }) {
        return true;
    }
    let bytes = text.as_bytes();
    let hex_run = bytes
        .iter()
        .rev()
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    if hex_run >= TRAILING_HEX_FRAGMENT_CHARS {
        return true;
    }
    let last = text
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .next()
        .unwrap_or("");
    last.starts_with("sk-") || last.starts_with("ghp_") || last.starts_with("AKIA")
}

/// One credential assignment head: where its key begins, where its value would begin, which
/// separator introduced it, whether an opening value quote was consumed, and whether the key is
/// an `authorization` header.
#[derive(Clone, Copy, Debug)]
struct AssignmentHead {
    key_start: usize,
    value_start: usize,
    colon: bool,
    quoted: bool,
    authorization: bool,
}

/// Every credential assignment head in `lower` (already case-folded), found from the SEPARATOR
/// back to the key (#1086): at every `=` or `:`, optional blanks, an optional CLOSING key quote
/// (`{"password":"x"}`, item 6), then an identifier of letters, digits, `_`, `-` and `.` that
/// [`credential_key`] names; forward, optional blanks and an optional opening value quote. A
/// doubled or compound separator (`token::Kind`, `password == x`, `a != b`, `x := y`,
/// `Token => y`) is a path, a comparison or an operator, not an assignment, and yields nothing.
fn secret_assignments(lower: &str) -> Vec<AssignmentHead> {
    let bytes = lower.as_bytes();
    let identifier = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.');
    let mut heads = Vec::new();
    for (at, &separator) in bytes.iter().enumerate() {
        if separator != b'=' && separator != b':' {
            continue;
        }
        if at > 0 && matches!(bytes[at - 1], b'=' | b':' | b'!' | b'<' | b'>') {
            continue;
        }
        if matches!(bytes.get(at + 1), Some(b'=' | b':'))
            || (separator == b'=' && bytes.get(at + 1) == Some(&b'>'))
        {
            continue;
        }
        let mut key_end = at;
        while key_end > 0 && matches!(bytes[key_end - 1], b' ' | b'\t') {
            key_end -= 1;
        }
        if key_end > 0 && matches!(bytes[key_end - 1], b'"' | b'\'') {
            key_end -= 1;
        }
        let mut key_start = key_end;
        while key_start > 0 && identifier(bytes[key_start - 1]) {
            key_start -= 1;
        }
        let key = &lower[key_start..key_end];
        if key.is_empty() || !credential_key(key) {
            continue;
        }
        let mut cursor = at + 1;
        while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        let quoted = cursor < bytes.len() && matches!(bytes[cursor], b'"' | b'\'');
        if quoted {
            cursor += 1;
        }
        heads.push(AssignmentHead {
            key_start,
            value_start: cursor,
            colon: separator == b':',
            quoted,
            authorization: key.rsplit(['_', '-', '.']).next() == Some("authorization"),
        });
    }
    heads
}

/// Whether the value behind an assignment head is a credential rather than a type or a
/// literal. Nothing, whitespace or a closing quote (`password=""`) is no value. A QUOTED value
/// is a secret under either separator. A YAML block scalar (`password: |`, `>-`, `|2`) is a
/// secret when an indented block follows (#1086 item 7). An `authorization:` value, after an
/// optional scheme (`Bearer`, `Basic`, `Token`, `Digest`, `Negotiate`), is a secret when it
/// carries a credential of 8+ characters. Behind `=` a bare value stays refused
/// (`password = hunter2`), except the literals `None`/`null`/`true`/`false`, which assign
/// nothing. Behind `:` in a CONFIGURATION file (#1086 item 4) a bare value is refused except a
/// literal (the four, and `~`) or an opening `{`/`[`. Behind `:` elsewhere — a Rust or
/// TypeScript type annotation (`token: String`, `password: Option<String>`) — a bare value is a
/// secret only when it looks like one: a recognised prefix (`sk-`, `ghp_`, `AKIA`, `eyJ`,
/// `-----BEGIN`), a bare token of 16+ characters carrying a digit, or a hex run of 32+.
fn assignment_value_is_secret(lower: &str, head: AssignmentHead, configuration: bool) -> bool {
    let rest = &lower[head.value_start..];
    let Some(first) = rest.chars().next() else {
        return false;
    };
    if first.is_whitespace() || first == '"' || first == '\'' {
        return false;
    }
    if head.quoted {
        return true;
    }
    if let Some(body) = block_scalar_body(lower, head) {
        return block_is_indented_under(lower, head, body);
    }
    if head.authorization && authorization_credential(rest) {
        return true;
    }
    let token = rest
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '/')))
        .next()
        .unwrap_or("");
    let literal = matches!(token, "none" | "null" | "true" | "false");
    if !head.colon {
        return !literal;
    }
    if configuration {
        return !(literal || matches!(first, '~' | '{' | '['));
    }
    let hex_run = token.bytes().take_while(u8::is_ascii_hexdigit).count();
    ["sk-", "ghp_", "akia", "eyj"]
        .iter()
        .any(|prefix| token.starts_with(prefix))
        || rest.starts_with("-----begin")
        || (token.len() >= 16 && token.bytes().any(|b| b.is_ascii_digit()))
        || hex_run >= 32
}

/// The text after a YAML block-scalar header at `head` — `|` or `>`, optional chomping and
/// indentation indicators (`+`, `-`, digits), optional blanks and comment, then the end of the
/// line — or `None` when the value behind the colon is not such a header (`| grep x` is not).
fn block_scalar_body(lower: &str, head: AssignmentHead) -> Option<&str> {
    if !head.colon || head.quoted {
        return None;
    }
    let rest = &lower[head.value_start..];
    if !rest.starts_with(['|', '>']) {
        return None;
    }
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let header = &rest[1..line_end];
    let indicators = header.split('#').next().unwrap_or("");
    let indicators = indicators.trim_end_matches([' ', '\t', '\r']);
    if !indicators
        .chars()
        .all(|c| matches!(c, '+' | '-') || c.is_ascii_digit())
    {
        return None;
    }
    Some(rest.get(line_end + 1..).unwrap_or(""))
}

/// Whether the first non-blank line of a block scalar's `body` is indented deeper than its key —
/// the YAML rule for where a block's content lives. An empty block, or a next line at the key's
/// own depth (`password: |` then `next: 1`), carries no value.
fn block_is_indented_under(lower: &str, head: AssignmentHead, body: &str) -> bool {
    let line_start = lower[..head.key_start].rfind('\n').map_or(0, |at| at + 1);
    let key_column = head.key_start - line_start;
    body.lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.len() - line.trim_start_matches([' ', '\t']).len() > key_column)
}

/// The characters a header credential is made of (RFC 6750's `b64token` and Basic's base64).
fn credential_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'+' | b'/' | b'=' | b'-')
}

/// Whether an `authorization:` value carries a credential: an optional scheme word, then 8+
/// credential characters. A placeholder (`Bearer {token}`, `<token>`, `$TOKEN`) carries none.
fn authorization_credential(rest: &str) -> bool {
    let line = rest.split('\n').next().unwrap_or("").trim_start();
    let credential = match line.split_once([' ', '\t']) {
        Some(("bearer" | "basic" | "token" | "digest" | "negotiate", after)) => after.trim_start(),
        _ => line,
    };
    credential
        .bytes()
        .take_while(|b| credential_char(*b))
        .count()
        >= 8
}

/// Whether `lower` carries `Bearer <credential>` anywhere (#1086 item 3): the scheme at a word
/// boundary, blanks, and a credential of 16+ characters carrying a digit — a token, not the word
/// "token" in prose.
fn bearer_credential(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(at) = lower[from..].find("bearer") {
        let start = from + at;
        from = start + "bearer".len();
        if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        let after = &lower[from..];
        let credential = after.trim_start_matches([' ', '\t']);
        if credential.len() == after.len() {
            continue;
        }
        let token = &credential[..credential
            .bytes()
            .take_while(|b| credential_char(*b))
            .count()];
        if token.len() >= 16 && token.bytes().any(|b| b.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Whether the hex run of `hex_run` characters at `hex_start` is a `sha256:`/`sha256-` digest:
/// the prefix, and a run of AT MOST 64 hex characters. A `sha256:` followed by 128 hex
/// characters is not a digest: the prefix does not exempt whatever run follows it, so a secret
/// written after one is refused like any other run over 64. Consulted for runs INSIDE an
/// excerpt only; a run that touches the excerpt cut is never exempt
/// ([`trailing_secret_fragment`], #1086 item 2).
fn digest_prefixed(text: &str, hex_start: usize, hex_run: usize) -> bool {
    if hex_run > 64 {
        return false;
    }
    let head = &text[..hex_start];
    head.ends_with("sha256:") || head.ends_with("sha256-")
}

/// One evidence item: a `source://` citation, the declared range, the text. `read_len` is the
/// number of bytes read from the file — the range declared — which the quoted `text` may
/// exceed by its marker quotes.
fn render_item(path: &str, text: &str, read_len: usize, file_len: u64, partial: bool) -> String {
    if partial {
        format!("source://{path} [bytes 0..{read_len} of {file_len}]\n{text}")
    } else {
        format!("source://{path} [{file_len} bytes]\n{text}")
    }
}
