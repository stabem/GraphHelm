use std::path::Path;

use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{EventKind, GraphSpec};

use super::{
    Failure, execution_state, finish, render, replay_failure, repository_failure, resolve_stream,
};
use crate::commands::event_store;
use crate::commands::{owner, publish_loaded};
use crate::output::Outcome;

const COMMAND: &str = "execution.status";

/// `execution status`: replay only, no appends. The output is the operator's triage view — the
/// same `render` every mutating command replies with, including the untriaged-interruption list,
/// plus `headSequence` (Milestone 05a Task 2): the raw stream's last sequence, or 0 for an empty
/// stream. The Public Runtime API's `GET /v1/executions/{id}` answers from the SAME BODY through
/// [`budgeted`], not through this function (see `commands::serve::routes::status`) — and the
/// `execution status` CLI command reaches this same [`budgeted`] entry point too, through
/// [`run`] below, NOT through this bare [`execute`]. So the CLI and the API report structurally
/// identical `data` (as `serde_json::Value`, not raw serialized bytes — key order and whitespace
/// are not part of the guarantee) for the same stream — "one store, one truth" — via the SAME
/// budgeted read on both sides — MODULO TWO AXES, and both are wall-clock, not stream content.
/// LIVENESS: this replays into `render` through `execute_within`, which feeds
/// `node_silence_seconds(&history, Utc::now())` into `attention`. Two reads of a shared,
/// still-running stream taken on opposite sides of a node's declared silence threshold can
/// legitimately return different `attention`/`attentionReasons` from identical stream content.
/// BUDGET EXPIRATION: [`budgeted`] mints its deadline via `ReadBudget::starting_now` at the
/// instant EACH call is made, not one shared clock — for a repository-open WALK crossing
/// `READ_BUDGET_CHECK_INTERVAL` (256 events, see below for what is counted), two
/// independently-started five-second windows can cross that checkpoint at different real elapsed
/// times, so one sequential CLI/API read can exceed its budget while the other still returns
/// data. See `apps/cli/tests/api_http.rs`'s CLI/API parity guard for the full precondition
/// analysis on both axes and why its own fixture is exempt from both (short stream, states outside
/// `has_judgeable_silence`).
/// That fixture compares independently produced histories, not a shared stream: its signal IDs,
/// idempotency keys and actors differ, and timestamp values are normalised before comparison.
/// Its assertion covers the resulting rendered values, not equality of those history inputs.
/// This bare [`execute`] function stays in use elsewhere, where a
/// read must never observe a budget at all (a mutation's own reply after it already committed,
/// and the immediate-pause poll loop — see [`budgeted`]'s doc comment) — it is not what the CLI's
/// `status` command or the API's status route calls, so it is not a source of divergence between
/// them. `ReadBudget::check_progress` consults its clock
/// only when a walk crosses a `READ_BUDGET_CHECK_INTERVAL` (256) event boundary — and on this
/// path there are TWO walks, and they count different things. `check_progress` has two production
/// call sites. (1) `replay_within` (`core/events/src/projection.rs`), reached from
/// `execute_within` below: the fold over the SELECTED history, stepping `index → index+1`, so it
/// crosses at the 256th event of that history. (2) `verify_lines` (`core/events/src/local.rs`),
/// on the repository-open path before `resolve_stream` narrows to any execution (`open_within` →
/// `open_inner` → `load_state` → `verify_lines`): what it walks is chosen by prefix-verification
/// state the caller of `status` cannot see — with a verified prefix, only the suffix past
/// `verified_offset`; without one, the whole journal — stepped by whole batches. So a five-event
/// execution in a repository whose cold open walks 305 events IS refused when the budget lapses
/// (measured on #990) even though its own fold never reaches 256, and a warm open of a
/// 10 000-event journal with twelve new events consults the deadline only if the selected history
/// itself crosses 256. Inside the walks, below every crossing, the deadline is ADVISORY — nothing
/// looks. But the read is NOT done when the walks are: `execute_within` calls
/// `ReadBudget::check_now` (`core/events/src/budget.rs`), which reads the clock on EVERY call with
/// no interval guard, after `render_read` and again after `render_snapshot`. So a read whose
/// render and derivation outlast `STATUS_READ_BUDGET_MILLIS` is refused with `walked` equal to the
/// history's length, whatever that length is — an eight-event stream was refused this way on a
/// loaded host (#993's gate, #990). Only the time spent INSIDE a sub-interval walk goes unreported;
/// a slow render is caught at the end, and the refusal's wording ("this stream is longer than the
/// read can fold") then names the wrong cause, because it is the fold's sentence borrowed by the
/// post-render check.
/// The API's `If-Match` workflow (a later task) reads `headSequence` as the version to race
/// against.
///
/// THIS SENTENCE SAID "calls this exact function" UNTIL #169, and had been stale since #750 split
/// [`budgeted`] out. A reader who believed it concluded the two surfaces could not diverge, which
/// is how a decidable question stayed open for eighteen days: the case for deleting the parity
/// test as decoration rested on this comment rather than on the call site.
///
/// Widened from private to `pub(crate)`: the one cross-module visibility widening this task needs
/// for status, so the server can reach the same "replay, then render" body the CLI does rather
/// than a second implementation of it.
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
) -> Result<serde_json::Value, Failure> {
    // The unbounded render describes work a mutation ALREADY committed (see `budgeted`), so it
    // reads without the unknown-id refusal: the operator-facing read is the one that owes it.
    let read = read_within(events, execution, graphhelm_events::ReadBudget::unbounded())?;
    Ok(render_read(&read, None))
}

/// [`execute`] under the wall-clock budget an OPERATOR-FACING read declares (#750).
///
/// **Why this is a second entry point rather than a change to `execute`.** One function renders
/// three different things: the answer to "what is this run doing", the reply a mutation returns
/// AFTER it has already committed (`serve::reply_with_status_value`), and each poll of the
/// immediate pause's wait loop (`serve::routes::pause`). The budget belongs to the first and to
/// neither of the others. Bounding the second would let a slow read turn a mutation that DID
/// commit into a refusal - the exact ambiguity the evidence contract exists to remove. Bounding
/// the third would make a large store's immediate pause report `unknown` forever while the
/// pause itself worked. So the read an operator is waiting on declares a budget, and the
/// renders that merely describe work already done do not.
pub(crate) fn budgeted(
    events: &Path,
    execution: Option<&str>,
    graph: Option<&GraphVersion>,
) -> Result<serde_json::Value, Failure> {
    execute_within(
        events,
        execution,
        crate::commands::status_read_budget(),
        graph,
    )
}

/// One operator-facing read: the folded projection, the raw history it was folded from, and the
/// attention inputs MEASURED from that history. `status` renders it; `briefing` (#1063) briefs
/// from it. One measurement, two renders, so the two surfaces can never disagree about what is
/// pending.
pub(crate) struct Read {
    pub(crate) projection: graphhelm_events::ExecutionProjection,
    pub(crate) history: Vec<graphhelm_protocols::EventEnvelope>,
    pub(crate) inputs: graphhelm_execution::AttentionInputs,
    /// `at_sequence(&history)`: `None` for an empty history, never `Some(0)`.
    pub(crate) at_sequence: Option<u64>,
}

/// The shared body, with the budget supplied rather than read from the wall clock.
///
/// The seam exists so the bound can be PROVEN. `budgeted` reads the system clock, and a cell
/// that had to wait out a real five-second budget would be five seconds of every gate run, for
/// a fact a supplied clock states exactly.
pub(crate) fn execute_within(
    events: &Path,
    execution: Option<&str>,
    budget: graphhelm_events::ReadBudget,
    graph: Option<&GraphVersion>,
) -> Result<serde_json::Value, Failure> {
    let read = read_known_within(events, execution, budget.clone())?;
    // #134 TAKEOVER PLACEMENT: this resolution lived in `read_within` on the branch, but main
    // split that function and `graph` is a parameter of THIS one. Moved rather than duplicated:
    // `read_within` stays graph-free and every caller that holds a graph passes through here.
    // #134: the dispatch view needs the graph, which `status` does not hold -- the CLI's own
    // `execution start` appends no publication event, so `current_graph` is empty on these
    // streams (the same discrepancy `resume` records). `--file` supplies it, and it must be the
    // graph this execution started from: the hash the `execution_started` payload recorded is
    // compared, exactly as `resume` compares before it redispatches. A different graph would
    // produce a gate for edges this execution never had, so it is refused rather than rendered.
    let spec = match graph {
        None => None,
        Some(version) => {
            // THE ACTIVE GRAPH FIRST, exactly as `resume` reads it: a stream that published a newer
            // `GraphVersionPublished` after its start has `current_graph` set, and that version is
            // the one the edges belong to; the start hash is the fallback for the CLI's own streams,
            // which publish nothing (Codex on #1014).
            let recorded = read
                .projection
                .current_graph
                .as_ref()
                .map(|published| published.semantic_hash().clone())
                .or_else(|| {
                    read.history
                        .iter()
                        .rev()
                        .find_map(|event| match &event.kind {
                            EventKind::ExecutionStarted(payload) => {
                                Some(payload.graph_hash.clone())
                            }
                            _ => None,
                        })
                });
            // The same derivation `resume` uses for its file-trust seam: the supplied graph's
            // content hash, parsed to the wire form the start payload recorded.
            let supplied = graphhelm_protocols::WireHash::parse(version.content_hash().as_str())
                .map_err(|_| {
                    execution_state("the supplied graph's hash is not wire-safe", "/file")
                })?;
            match recorded {
                Some(hash) if hash == supplied => Some(&version.graph().spec),
                Some(_) => {
                    return Err(execution_state(
                        "--file names a graph other than this execution's active one (semantic hash differs)",
                        "/file",
                    ));
                }
                None => {
                    return Err(execution_state(
                        "this stream records no execution start, so --file cannot be checked against it",
                        "/file",
                    ));
                }
            }
        }
    };
    let value = render_read(&read, spec);
    // THE SAME BUDGET, READ AGAIN after the render (Codex on #1014). `replay_within` checks the
    // clock as the fold crosses event intervals; the dispatch derivation that follows walks the
    // active graph and is counted in no events, so a fold that ended just inside the deadline
    // could return a success minutes after it. The refusal is the fold's own, so the operator
    // sees one code for "this read ran out of time" whichever half spent it.
    budget
        .check_now(read.history.len() as u64)
        .map_err(|exceeded| {
            replay_failure(&graphhelm_events::ReplayError::BudgetExceeded {
                walked: exceeded.walked,
                limit_millis: exceeded.limit_millis,
            })
        })?;
    Ok(value)
}

fn render_read(read: &Read, spec: Option<&GraphSpec>) -> serde_json::Value {
    let mut value = render(
        &read.projection,
        &read.inputs,
        &super::Liveness::measured(&read.history),
        spec,
    );
    // The WIRE field keeps its existing shape on purpose, zero and all: `headSequence` is a
    // different contract from `at_sequence`, read by clients that already treat 0 as "nothing
    // yet", and widening it to null is a wire change that needs its own justification. The
    // flattening is left here DECLARED rather than silently carried into the seam.
    value["headSequence"] = serde_json::json!(read.at_sequence.unwrap_or(0));
    // A live lease means armed and not consumed, just as wake status reports. Its actor
    // comes from the arming envelope, not its opaque session id or a display alias.
    let listeners: std::collections::BTreeSet<_> = read
        .history
        .iter()
        .filter_map(|event| {
            let graphhelm_protocols::EventKind::WakeLease(lease) = &event.kind else {
                return None;
            };
            read.projection
                .wake_leases
                .get(lease.session_id.as_str())
                .filter(|live| live.armed_at_sequence == event.sequence)
                .map(|_| event.actor.id().as_str())
        })
        .collect();
    value["wakeListeners"] = serde_json::json!(listeners);
    value
}

/// [`read_within`] for a read an operator is WAITING on: an explicit id whose stream holds no
/// event refuses with `GHCLI028_EXECUTION_NOT_FOUND` (#1083 F1) instead of folding nothing into a
/// calm verdict. Shared by `status` and `briefing`, so the CLI, `GET /v1/executions/{id}`, its
/// `/briefing` and the MCP tools (which call those routes) refuse one unknown id identically.
pub(crate) fn read_known_within(
    events: &Path,
    execution: Option<&str>,
    budget: graphhelm_events::ReadBudget,
) -> Result<Read, Failure> {
    let read = read_within(events, execution, budget)?;
    if execution.is_some() && read.history.is_empty() {
        return Err(super::not_found());
    }
    Ok(read)
}

/// The read half of [`execute_within`], shared with `briefing::execute_within`.
pub(crate) fn read_within(
    events: &Path,
    execution: Option<&str>,
    budget: graphhelm_events::ReadBudget,
) -> Result<Read, Failure> {
    // ONE budget for the whole read. Both halves are linear in the history - the journal
    // verification `open` performs, and then the fold - so both are checked against this same
    // deadline. Bounding only the fold would cover 42% of the measured cost while reading as a
    // boundary; two budgets started separately would let one read spend the budget twice.
    let store = crate::commands::budgeted_event_store(events, budget.clone())
        .map_err(|error| repository_failure(&error))?;
    // Not `load_projection`: that helper only returns the folded projection, and `headSequence`
    // needs the raw history's last sequence too. Inlining `load_projection`'s own two-line body
    // here (resolve, then replay) avoids widening `load_projection` itself — it is also called by
    // `cancel.rs` and `pause.rs`, outside this task's file set, and changing its return shape would
    // have forced edits there.
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let projection = fold_within(events, &scope, &stream, &history, &budget)
        .map_err(|error| replay_failure(&error))?;
    // DO NOT INLINE THIS BACK to `Some(history.last().map_or(0, …))`. The guard for it lives in
    // `mod.rs`'s tests and calls the helper directly, so it CANNOT SEE THIS LINE: inlining the old
    // spelling here leaves every test green. What protects this call site is this comment, and
    // nothing else.
    let at_sequence = super::at_sequence(&history);
    // The read surface is the one that CAN measure: it is holding the history. It supplies
    // the subtraction; the budget stays empty until a surface can see the manifest that
    // declares it, and an unbudgeted node comes back as unevaluated rather than as calm.
    // The budgets are NOT passed here: `for_surface` derives them, so this surface cannot hold a
    // private reading of them (#176). What it does pass is what only it knows.
    let inputs = graphhelm_execution::AttentionInputs::for_surface(
        &projection,
        super::node_silence_seconds(&history, chrono::Utc::now()),
        // Where this read was looking, so a remedy can be placed in the history later. `None`
        // when there was nothing to look at: an empty history has no vantage point, and
        // `Some(0)` would claim one at a sequence streams never issue.
        at_sequence,
    );
    Ok(Read {
        projection,
        history,
        inputs,
        at_sequence,
    })
}

type FoldKey = (std::path::PathBuf, String);

/// How many streams serve keeps folded at once. The owner's Runtime stays up for days and reads
/// dozens of runs; each entry is one run's projection, so the count is the memory bound (#378).
const FOLD_CACHE_STREAMS: usize = 32;
/// A stream nobody has read for this long is dropped; its next read folds it again.
const FOLD_CACHE_IDLE: std::time::Duration = std::time::Duration::from_secs(30 * 60);

struct Fold {
    generation: graphhelm_events::ProjectionGeneration,
    used: std::time::Instant,
}

#[derive(Default)]
struct FoldCacheInner {
    folds: std::sync::Mutex<std::collections::BTreeMap<FoldKey, Fold>>,
    /// Streams a background fold is already finishing, so a burst of refused reads starts one.
    warming: std::sync::Mutex<std::collections::BTreeSet<FoldKey>>,
}

/// #371: serve keeps one projection generation per stream, so a read folds only the events
/// appended since the last read. Before this, every status, briefing and reply-suggestions read
/// refolded a ~9k-event stream from sequence 1 inside a 5 s budget, and about one read in five ran
/// out. Off for one-shot commands, which fold once and exit. A value rather than only a global so
/// a test owns its own instance instead of switching the cache on for every test in the process.
#[derive(Clone, Default)]
struct FoldCache(std::sync::Arc<FoldCacheInner>);

static SHARED_FOLDS: std::sync::OnceLock<FoldCache> = std::sync::OnceLock::new();

pub(crate) fn enable_shared_fold_cache() {
    let _ = SHARED_FOLDS.set(FoldCache::default());
}

/// Clears a stream's warming mark when the warmer ends, however it ends: a panic unwinds through
/// this drop too, so a stream is never left marked with no warmer running (#378).
struct WarmingClaim {
    cache: FoldCache,
    key: FoldKey,
}

impl Drop for WarmingClaim {
    fn drop(&mut self) {
        if let Ok(mut warming) = self.cache.0.warming.lock() {
            warming.remove(&self.key);
        }
    }
}

impl FoldCache {
    fn get(&self, key: &FoldKey) -> Option<graphhelm_events::ProjectionGeneration> {
        let mut folds = self.0.folds.lock().ok()?;
        let fold = folds.get_mut(key)?;
        fold.used = std::time::Instant::now();
        Some(fold.generation.clone())
    }

    fn store(&self, key: &FoldKey, generation: graphhelm_events::ProjectionGeneration) {
        self.store_at(key, generation, std::time::Instant::now());
    }

    /// Keeps the furthest generation (a slower concurrent read must not rewind a faster one),
    /// then drops idle streams and, past the cap, the least recently used.
    fn store_at(
        &self,
        key: &FoldKey,
        generation: graphhelm_events::ProjectionGeneration,
        now: std::time::Instant,
    ) {
        let Ok(mut folds) = self.0.folds.lock() else {
            return;
        };
        match folds.get_mut(key) {
            Some(current)
                if current.generation.watermark().last_sequence()
                    > generation.watermark().last_sequence() =>
            {
                current.used = now;
            }
            _ => {
                folds.insert(
                    key.clone(),
                    Fold {
                        generation,
                        used: now,
                    },
                );
            }
        }
        folds.retain(|_, fold| now.saturating_duration_since(fold.used) <= FOLD_CACHE_IDLE);
        while folds.len() > FOLD_CACHE_STREAMS {
            let Some(oldest) = folds
                .iter()
                .min_by_key(|(_, fold)| fold.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            folds.remove(&oldest);
        }
    }

    /// Marks `key` as warming, or `None` when a warmer already runs for it.
    fn claim_warming(&self, key: &FoldKey) -> Option<WarmingClaim> {
        let mut warming = self.0.warming.lock().ok()?;
        warming.insert(key.clone()).then(|| WarmingClaim {
            cache: self.clone(),
            key: key.clone(),
        })
    }

    /// Finishes `generation` over `suffix` (only the events it has not folded) off the request.
    fn warm(
        &self,
        key: FoldKey,
        mut generation: graphhelm_events::ProjectionGeneration,
        suffix: Vec<graphhelm_protocols::EventEnvelope>,
    ) {
        let Some(claim) = self.claim_warming(&key) else {
            return;
        };
        let cache = self.clone();
        std::thread::spawn(move || {
            let _claim = claim;
            let folded = suffix
                .chunks(graphhelm_events::MAX_READ_PAGE)
                .try_for_each(|page| generation.apply_page(page));
            if folded.is_ok() {
                cache.store(&key, generation);
            }
        });
    }
}

/// [`graphhelm_events::replay_within`], resumed from serve's cached generation when one exists.
fn fold_within(
    events: &Path,
    scope: &graphhelm_protocols::RepositoryScope,
    stream: &str,
    history: &[graphhelm_protocols::EventEnvelope],
    budget: &graphhelm_events::ReadBudget,
) -> Result<graphhelm_events::ExecutionProjection, graphhelm_events::ReplayError> {
    match SHARED_FOLDS.get() {
        Some(cache) => fold_cached(cache, events, scope, stream, history, budget),
        None => graphhelm_events::replay_within(scope, stream, history, budget),
    }
}

/// The cached generation is reused only when the history still holds the event it stopped at, by
/// sequence AND hash; anything else (a different journal, a rewrite) folds from genesis. The fold
/// is `ProjectionGeneration::apply_page`, which runs the same per-event checks and the same
/// `apply_projection_event` as `replay_within` (`a_discarded_generation_rebuilds_to_identical_state`
/// holds them equal), so a corrupt event still refuses and nothing corrupt is cached. When the
/// budget runs out, the pages already folded are kept, and a background thread finishes the fold
/// over only the events left: the next read answers instead of starting over. That is the
/// cold-start path too.
fn fold_cached(
    cache: &FoldCache,
    events: &Path,
    scope: &graphhelm_protocols::RepositoryScope,
    stream: &str,
    history: &[graphhelm_protocols::EventEnvelope],
    budget: &graphhelm_events::ReadBudget,
) -> Result<graphhelm_events::ExecutionProjection, graphhelm_events::ReplayError> {
    if history.len() > graphhelm_events::MAX_READ_ALL {
        return Err(graphhelm_events::ReplayError::LimitExceeded);
    }
    let key: FoldKey = (
        events.to_path_buf(),
        format!(
            "{}\u{0}{stream}",
            serde_json::to_string(scope).map_err(|_| graphhelm_events::ReplayError::Corrupt)?
        ),
    );
    let mut generation = match cache
        .get(&key)
        .filter(|generation| resumable(generation, history))
    {
        Some(generation) => generation,
        None => graphhelm_events::ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "serve-status".to_owned(),
            1,
            1,
        )?,
    };
    let start = usize::try_from(generation.watermark().last_sequence())
        .map_err(|_| graphhelm_events::ReplayError::Corrupt)?;
    let mut walked = 0_usize;
    let remaining = history.get(start..).unwrap_or_default();
    for page in remaining.chunks(graphhelm_events::MAX_READ_PAGE) {
        generation.apply_page(page)?;
        let before = walked;
        walked += page.len();
        // Checked between pages, never after the last: a finished fold is not refused for the
        // time it already spent (the caller's post-render `check_now` still bounds the read).
        if walked == remaining.len() {
            break;
        }
        if let Err(exceeded) = budget.check_progress(before as u64, walked as u64) {
            cache.store(&key, generation.clone());
            cache.warm(key, generation, remaining[walked..].to_vec());
            return Err(graphhelm_events::ReplayError::BudgetExceeded {
                walked: exceeded.walked,
                limit_millis: exceeded.limit_millis,
            });
        }
    }
    let projection = generation.projection().clone();
    cache.store(&key, generation);
    Ok(projection)
}

/// Whether `generation` stopped at an event `history` still holds, unchanged.
fn resumable(
    generation: &graphhelm_events::ProjectionGeneration,
    history: &[graphhelm_protocols::EventEnvelope],
) -> bool {
    let watermark = generation.watermark();
    let Ok(at) = usize::try_from(watermark.last_sequence()) else {
        return false;
    };
    match at.checked_sub(1) {
        None => true,
        Some(index) => history
            .get(index)
            .is_some_and(|event| Some(&event.event_hash) == watermark.last_event_hash()),
    }
}

pub fn run(
    events: &Path,
    execution: Option<&str>,
    html: Option<&Path>,
    file: Option<&Path>,
) -> Outcome {
    // #134: `--file` is loaded, linted and published the way `resume` does it, so the version
    // whose hash is compared below is the same object `resume` would redispatch from.
    //
    // THE CONTRACT OF THE STATUS BUDGET, stated once (Codex on #1014, third thread of one family):
    // the five-second budget (#750) bounds the READ OF THE STORE -- journal verification, the fold,
    // and everything derived from the projection, the dispatch view included. It starts when
    // `budgeted` reads the clock and it ends at the `check_now` after `render`. It EXCLUDES this
    // block -- `--file` preparation: load, lint, publish -- and excludes it on purpose. The budget
    // exists to bound a walk over a history that grows with the execution, the one input the
    // operator cannot shrink; a graph file is an input the operator named and can see, its cost is
    // the schema crate's (`graphhelm_schema::load_graph`) and does not grow with the stream, and
    // `start` and `resume` prepare the same file unbudgeted. A bound on preparation, if one is
    // ever wanted, is a separate budget with its own name and its own deadline -- not this one
    // stretched to cover a segment it was never a promise about. Every segment of this command is
    // therefore inside the budget or named here as outside it; there is no third kind.
    // #192, here too: a warning-only lint pass reaches every exit past this point, as it does in
    // `start` and `resume` -- a status that computed the warnings and then dropped them would
    // hide diagnostics the operator asked for by naming the file (Codex on #1014).
    let mut warnings = Vec::new();
    let version = match file {
        None => None,
        Some(file) => {
            let loaded = match graphhelm_schema::load_graph(file) {
                Ok(loaded) => loaded,
                Err(diagnostics) => return Outcome::domain(COMMAND, diagnostics),
            };
            let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
            if !report.errors.is_empty() {
                let mut diagnostics = report.errors;
                diagnostics.extend(report.warnings);
                return Outcome::domain(COMMAND, diagnostics);
            }
            warnings = report.warnings;
            match publish_loaded(&loaded, owner("owner-local")) {
                Ok(version) => Some(version),
                Err(error) => return Outcome::internal(COMMAND, error).with_warnings(warnings),
            }
        }
    };
    // The operator is waiting on this one, so it is the call that declares the budget (#750).
    //
    // ONE budget, not one per read (Codex on #1014). `--html` replays the store a SECOND time in
    // `write_snapshot`, and a budget built inside that call would start its own five seconds --
    // so the command could spend the declared budget twice and still report success. Built here
    // and handed to both, the five seconds bound the command rather than each half of it.
    let budget = crate::commands::status_read_budget();
    let value = execute_within(events, execution, budget.clone(), version.as_ref());
    // `--html` AFTER the status derived, never before (Codex on #1014): the snapshot used to be
    // written first, so a refused `--file` -- missing, lint-invalid, or a graph other than the
    // active one -- left an apparently successful artefact behind a command that exited 2. A
    // write failure is still the command's failure, not a silent skip.
    if let (Some(html), Ok(_)) = (html, &value)
        && let Err(failure) = write_snapshot(events, execution, html, budget)
    {
        return finish::<serde_json::Value>(COMMAND, Err(failure), |value| value)
            .with_warnings(warnings);
    }
    finish(COMMAND, value, |value| value).with_warnings(warnings)
}

/// `--html`: the monitor page as a frozen incident snapshot — the SAME `render_snapshot`
/// the serve layer's live page is built from (05f Task 5), written before the envelope so
/// a write failure is the command's failure, not a silent skip.
fn write_snapshot(
    events: &Path,
    execution: Option<&str>,
    html: &Path,
    budget: graphhelm_events::ReadBudget,
) -> Result<(), Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    // #1083 (Codex on PR #1091): the unknown-id refusal must come BEFORE the write. `run` calls
    // this after the status read, so without this check a refused `status --execution <typo>
    // --html <path>` folded nothing, rendered it, and overwrote whatever snapshot already stood
    // at `<path>` - a refused command with a destructive side effect. Same rule, same refusal as
    // `read_known_within`.
    if execution.is_some() && history.is_empty() {
        return Err(super::not_found());
    }
    let projection = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;
    let page = crate::commands::serve::monitor::render_snapshot(
        &projection,
        &history,
        chrono::Utc::now(),
        events,
    );
    // THE SAME BUDGET AS THE STATUS READ, spent down by this second replay (Codex on #1014).
    // Placed after the render and before the write, mirroring the post-render check in
    // `execute_within`: the whole of the second operator-facing read is inside the bound, and a
    // command that overran cannot report success -- or leave a snapshot behind implying it did.
    budget.check_now(history.len() as u64).map_err(|exceeded| {
        replay_failure(&graphhelm_events::ReplayError::BudgetExceeded {
            walked: exceeded.walked,
            limit_millis: exceeded.limit_millis,
        })
    })?;
    std::fs::write(html, page)
        .map_err(|_| super::argument("--html does not name a writable file path", "/html"))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    use chrono::{TimeZone, Utc};
    use graphhelm_events::{LocalEventRepository, PreparedAppend, ReadBudget};
    use graphhelm_protocols::{
        ActorId, Clock, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator,
        NewEvent, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256,
        RepositoryScope, Sensitivity, SignalRecorded, SignalSeverity, SignalSourceKind, WireHash,
        WorkspaceId,
    };

    struct FrozenClock;
    impl Clock for FrozenClock {
        fn now(&self) -> chrono::DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap()
        }
    }
    /// Start instant once - the read `starting_now` spends on the deadline - then an hour on.
    struct LapsingClock(AtomicU64);
    impl Clock for LapsingClock {
        fn now(&self) -> chrono::DateTime<Utc> {
            let base = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                base
            } else {
                base + chrono::Duration::hours(1)
            }
        }
    }
    #[derive(Default)]
    struct Ids(AtomicU64);
    impl IdGenerator for Ids {
        fn next_id(&self, prefix: &'static str) -> String {
            format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
        }
    }

    fn seed(directory: &std::path::Path, count: u64) {
        let scope = RepositoryScope::new(
            WorkspaceId::parse(super::super::WORKSPACE).unwrap(),
            ProjectId::parse(super::super::PROJECT).unwrap(),
            Some(ExecutionId::parse("execution-budget").unwrap()),
        );
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        let store =
            LocalEventRepository::open(directory, Arc::new(FrozenClock), Arc::new(Ids::default()))
                .unwrap();
        let mut next = 1_u64;
        while next <= count {
            let size = 50.min(count - next + 1);
            let mut events = Vec::new();
            if next == 1 {
                events.push(NewEvent::new(
                    OpaqueId::parse("key-root").unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::ExecutionStarted(ExecutionStarted {
                        execution_id: OpaqueId::parse("execution-budget").unwrap(),
                        graph_version: 1,
                        graph_hash: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
                        mode: ExecutionMode::Supervised,
                    }),
                    vec![],
                    vec![],
                ));
            }
            for index in next + events.len() as u64..next + size {
                events.push(NewEvent::new(
                    OpaqueId::parse(format!("key-{index}")).unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::SignalRecorded(SignalRecorded {
                        to: None,
                        reply_to: None,
                        scoped_agent_authenticated: None,
                        execution_id: OpaqueId::parse("execution-budget").unwrap(),
                        signal_id: OpaqueId::parse(format!("signal-{index}")).unwrap(),
                        source_kind: SignalSourceKind::Node,
                        source_id: OpaqueId::parse("implementation").unwrap(),
                        kind: "no_progress".to_owned(),
                        severity: SignalSeverity::High,
                        envelope_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
                    }),
                    vec![],
                    vec![],
                ));
            }
            store
                .append_atomic(
                    &PreparedAppend::new(
                        scope.clone(),
                        OpaqueId::parse("execution-budget").unwrap(),
                        next,
                        events,
                        vec![],
                        vec![],
                    )
                    .unwrap(),
                )
                .unwrap();
            next += size;
        }
    }

    /// The WIRING, which the two `core/events` cells cannot see: `status::execute` builds one
    /// budget and hands it to BOTH halves of its read.
    ///
    /// The pointer is what makes this cell discriminating, and it was added because without it
    /// the cell was not: a first version asserted only the code, and it stayed green when the
    /// status read was put back on an UNBUDGETED store - because the fold, still holding the
    /// same lapsed budget, refused with the same code one step later. Two halves, one code,
    /// and an assertion that could not tell which had spoken. `/repository` is the open's
    /// pointer and `/events` is the fold's, so this line names the half.
    #[test]
    fn a_status_read_whose_budget_has_lapsed_refuses_in_the_open_it_budgeted() {
        let directory = tempfile::tempdir().unwrap();
        seed(directory.path(), 400);

        let lapsed = ReadBudget::starting_now(
            Arc::new(LapsingClock(AtomicU64::new(0))),
            chrono::Duration::seconds(5),
        );
        let failure =
            super::execute_within(directory.path(), Some("execution-budget"), lapsed, None)
                .expect_err("a lapsed budget must refuse the read");

        assert_eq!(failure.code, "GHE013_READ_BUDGET_EXCEEDED");
        assert_eq!(
            failure.pointer, "/repository",
            "the budget reached the journal verification, not only the fold: {}",
            failure.message
        );
        assert!(
            failure.message.contains("5000 ms budget"),
            "the refusal names the budget it was given: {}",
            failure.message
        );
    }

    /// The control. The same store, the same entry point, a budget that has NOT lapsed: the
    /// read answers, and it answers the whole history. Without this, a status read that refused
    /// every store would pass the cell above.
    /// #134: the budget is read AFTER the render too. A stream this short never crosses an
    /// interval, so the fold reads the clock zero times and would return success under a clock
    /// that lapsed the instant after the deadline was set; the post-render check is what refuses.
    #[test]
    fn a_lapsed_budget_is_refused_after_the_render_even_when_the_fold_never_read_the_clock() {
        let directory = tempfile::tempdir().unwrap();
        seed(directory.path(), 3);
        let lapsed = ReadBudget::starting_now(
            Arc::new(LapsingClock(AtomicU64::new(0))),
            chrono::Duration::seconds(5),
        );
        let failure =
            super::execute_within(directory.path(), Some("execution-budget"), lapsed, None)
                .expect_err("a lapsed budget must refuse the read even after a short fold");
        assert_eq!(failure.code, "GHE013_READ_BUDGET_EXCEEDED");
    }

    #[test]
    fn a_status_read_within_its_budget_answers_the_whole_history() {
        let directory = tempfile::tempdir().unwrap();
        seed(directory.path(), 400);

        let ample = ReadBudget::starting_now(Arc::new(FrozenClock), chrono::Duration::seconds(5));
        // `Failure` carries no `Debug`, so the error arm names itself rather than being unwrapped.
        let Ok(value) =
            super::execute_within(directory.path(), Some("execution-budget"), ample, None)
        else {
            panic!("a budget that has not lapsed refuses nothing");
        };

        assert_eq!(value["headSequence"], serde_json::json!(400));
        assert_eq!(value["signalsRecorded"], serde_json::json!(399));
    }

    /// #1014: `--html` replays the store a SECOND time, in `write_snapshot`, AFTER the status
    /// read's final deadline check has already passed. That read used to carry no budget at all,
    /// so `execution status --html` against a large journal could return success -- and leave a
    /// snapshot behind implying it -- long after the declared five-second budget had gone.
    ///
    /// The refusal arm alone would not distinguish "refused because the budget lapsed" from
    /// "refused for any reason at all", so the ample-budget arm is the half that gives it meaning:
    /// the same call, the same stream, the same path, and the file MUST appear.
    #[test]
    fn the_html_snapshot_spends_the_status_budget_and_leaves_no_file_when_it_lapses() {
        let directory = tempfile::tempdir().unwrap();
        seed(directory.path(), 3);
        let html = directory.path().join("snapshot.html");

        let lapsed = ReadBudget::starting_now(
            Arc::new(LapsingClock(AtomicU64::new(0))),
            chrono::Duration::seconds(5),
        );
        let failure =
            super::write_snapshot(directory.path(), Some("execution-budget"), &html, lapsed)
                .expect_err("a lapsed budget must refuse the second read the snapshot performs");
        assert_eq!(failure.code, "GHE013_READ_BUDGET_EXCEEDED");
        assert!(
            !html.exists(),
            "a refused snapshot must leave no file behind: the write is gated on the check"
        );

        let ample = ReadBudget::starting_now(Arc::new(FrozenClock), chrono::Duration::seconds(5));
        // `Failure` carries no `Debug`, so the error arm names itself rather than being unwrapped.
        let Ok(()) =
            super::write_snapshot(directory.path(), Some("execution-budget"), &html, ample)
        else {
            panic!("a budget that has not lapsed refuses no snapshot");
        };
        assert!(
            html.exists(),
            "control: without this the refusal arm proves nothing about the budget"
        );
    }

    /// #371: on ml-saas every read refolded a ~9k-event stream from sequence 1, so a read whose
    /// budget ran out left nothing behind and the next one started over. Each read here gets a
    /// budget that lapses at its first check; with serve's fold cache the folded pages survive the
    /// refusal, so a later read answers, and answers exactly what a full replay answers. Without
    /// the cache every read refuses. Cost: 2,500 events in a tempdir, a few seconds in debug.
    #[test]
    fn a_lapsed_read_keeps_its_folded_pages_so_a_later_read_answers() {
        // A private cache: the process-global one stays off for every other test (#378).
        let cache = super::FoldCache::default();
        let directory = tempfile::tempdir().unwrap();
        seed(directory.path(), 2_500);
        let store = crate::commands::event_store(directory.path()).unwrap();
        let Ok((scope, stream, history)) =
            super::super::resolve_stream(&store, Some("execution-budget"))
        else {
            panic!("the seeded stream resolves");
        };
        let direct = graphhelm_events::replay(&scope, &stream, &history).unwrap();

        let mut answered = None;
        for _ in 0..3 {
            let lapsed = ReadBudget::starting_now(
                Arc::new(LapsingClock(AtomicU64::new(0))),
                chrono::Duration::seconds(5),
            );
            match super::fold_cached(&cache, directory.path(), &scope, &stream, &history, &lapsed) {
                Ok(projection) => {
                    answered = Some(projection);
                    break;
                }
                Err(graphhelm_events::ReplayError::BudgetExceeded { .. }) => {}
                Err(other) => panic!("only the budget may refuse: {other}"),
            }
        }
        let projection =
            answered.expect("three lapsed reads of three pages must not all start over");
        assert_eq!(
            serde_json::to_value(&projection).unwrap(),
            serde_json::to_value(&direct).unwrap(),
            "the resumed fold must equal a full replay"
        );
    }

    fn fold_key(name: &str) -> super::FoldKey {
        (std::path::PathBuf::from(name), String::new())
    }

    fn empty_generation() -> graphhelm_events::ProjectionGeneration {
        let scope = RepositoryScope::new(
            WorkspaceId::parse(super::super::WORKSPACE).unwrap(),
            ProjectId::parse(super::super::PROJECT).unwrap(),
            Some(ExecutionId::parse("execution-budget").unwrap()),
        );
        graphhelm_events::ProjectionGeneration::new(
            scope,
            "execution-budget".to_owned(),
            "serve-status".to_owned(),
            1,
            1,
        )
        .unwrap()
    }

    /// #378: the owner's Runtime stays up for days and reads dozens of runs, and #376 kept every
    /// stream it ever folded. The cache holds at most its cap, drops the least recently used
    /// first, and drops every stream idle past the limit. Cost: in-memory, no I/O.
    #[test]
    fn the_fold_cache_keeps_at_most_its_cap_and_drops_idle_streams() {
        let cache = super::FoldCache::default();
        let t0 = std::time::Instant::now();
        for index in 0..super::FOLD_CACHE_STREAMS + 5 {
            cache.store_at(
                &fold_key(&format!("events-{index}")),
                empty_generation(),
                t0 + std::time::Duration::from_secs(index as u64),
            );
        }
        {
            let folds = cache.0.folds.lock().unwrap();
            assert_eq!(folds.len(), super::FOLD_CACHE_STREAMS);
            assert!(
                !folds.contains_key(&fold_key("events-0")),
                "the least recently used stream goes first"
            );
        }
        cache.store_at(
            &fold_key("late"),
            empty_generation(),
            t0 + super::FOLD_CACHE_IDLE + std::time::Duration::from_secs(3_600),
        );
        assert_eq!(
            cache.0.folds.lock().unwrap().len(),
            1,
            "every stream idle past the limit is dropped"
        );
    }

    /// #378: #376 cleared a stream's warming mark only when the warmer returned, so a warmer that
    /// panicked left the stream marked and no later read could warm it again. Cost: one thread.
    #[test]
    fn a_warmer_that_panics_releases_its_stream() {
        let cache = super::FoldCache::default();
        let key = fold_key("events");
        let claim = cache.claim_warming(&key).expect("the first claim succeeds");
        assert!(
            cache.claim_warming(&key).is_none(),
            "a running warmer holds its stream"
        );
        let panicked = std::thread::spawn(move || {
            let _claim = claim;
            panic!("the warmer failed");
        })
        .join();
        assert!(panicked.is_err());
        assert!(
            cache.claim_warming(&key).is_some(),
            "a panicked warmer must not leave its stream marked"
        );
    }
}
