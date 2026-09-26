use std::{collections::BTreeMap, path::Path};

use graphhelm_events::PreparedAppend;
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    DeclaredExecutor, DeclaredTopology, DeclaredTopologyEdge, EdgeType, EventKind,
    ExecutionFormDeclared, ExecutionMode, ExecutionPaused, ExecutionStarted, NewEvent,
    NodeDescriptor, OpaqueId, PersistedActor, PersistedActorType, Sensitivity, WireHash,
};

use super::driver::{Release, drive_to_quiescence};
use super::{
    Failure, PreparedDrive, argument, execution_state, finish, idempotency_key, load_fixtures,
    load_projection, render, replay_failure, repository_failure,
};
use crate::commands::{event_store, owner, publish_loaded};
use crate::output::Outcome;
use graphhelm_simulation::FixtureExecutor;

const COMMAND: &str = "execution.start";
// Leave room for the event envelope below the Event Store's 1 MiB per-event ceiling.
const MAX_FORM_BYTES_WITH_TOPOLOGY: usize = 768 * 1024;

/// Calls `execute` with the system actor and a fresh per-invocation idempotency key, exactly as
/// before Milestone 05a Task 4 — byte-identical CLI behaviour. Unlike `pause`/`resume`/`cancel`,
/// this stays `system_actor()` rather than `owner_actor()`: `start` has never been one of D-019's
/// owner-initiated commands (see `owner_actor`'s own doc comment, which lists `approve`, `pause`,
/// `resume`, `cancel`, `signal` and pointedly not `start`) — it is attributed identically to `graph
/// simulate`. This task's widening does not change that for the CLI; it only lets the Public
/// Runtime API supply a real caller actor instead (see `execute`'s own doc comment).
pub fn run(
    file: &Path,
    events: &Path,
    fixtures: Option<&Path>,
    mode: &str,
    execution: Option<&str>,
    held: bool,
) -> Outcome {
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
    // #192: a warning-only lint pass reaches every exit past this point, not only the success
    // one — a caller whose publish then failed already knew about the lint warnings, and the
    // reply must not look like it withheld something it already computed.
    let warnings = report.warnings;
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal(COMMAND, error).with_warnings(warnings),
    };
    let started = if held {
        execute_held(
            &version,
            events,
            fixtures,
            mode,
            execution,
            super::system_actor(),
            idempotency_key("execution-started"),
        )
    } else {
        execute(
            &version,
            events,
            fixtures,
            mode,
            execution,
            super::system_actor(),
            idempotency_key("execution-started"),
        )
    };
    finish(COMMAND, started, |value| value).with_warnings(warnings)
}

/// Widened from private to `pub(crate)` (Milestone 05a Task 4), gaining `actor` and `key` as
/// explicit parameters, exactly as Task 3 did for `signal`/`approve`.
///
/// `key` is used for exactly the one event that identifies "this start command happened":
/// `ExecutionStarted`. `actor` attributes that same event — over the API this is the caller's real
/// owner/agent identity from the request headers, satisfying "every mutation is attributed"; the
/// CLI keeps passing `system_actor()` (see `run`), so CLI output is unchanged.
///
/// `drive_to_quiescence` below is **not** given `actor`: it is called with its own fresh
/// `system_actor()`, deliberately decoupled. Before this task both were the same value (`actor` was
/// always `system_actor()` on the CLI path, so the two calls were indistinguishable), which is
/// exactly why decoupling them here is safe — CLI behaviour does not change. What the decoupling
/// buys is honesty on the API path: `drive_to_quiescence`'s dispatch loop calls a real executor and
/// its event count varies with the graph and fixtures (many `NodeOutcomeRecorded` hops, each with
/// its own fresh key — see `driver.rs`'s own `idempotency_key` calls, untouched by this task) — it
/// is the driver's own bookkeeping, not a caller decision, and per the plan's endpoint contract
/// `System` is reserved for exactly that. A variable-count sequence of events also cannot derive
/// deterministic keys from `key`'s single fixed suffix the way the one `ExecutionStarted` event
/// can, so `run_idempotent_mutation`'s pre-flight Complete/Partial classification only ever looks at
/// `ExecutionStarted`'s derived key — never at the drive loop's. A retry recognized as `Complete`
/// from that one key never re-enters this function (see `serve::mod::run_idempotent_mutation`), so
/// the drive loop's fresh keys are never at risk of a caller-triggered double-apply; they only need
/// to be valid, non-colliding keys for the one genuinely fresh attempt that reaches them.
pub(crate) fn execute(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    mode: &str,
    execution: Option<&str>,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let prepared = execute_prepared(
        version,
        events,
        fixtures,
        mode,
        execution,
        Attribution {
            actor,
            key,
            // This path drives with `FixtureExecutor` below and nothing else.
            executor: Some(DeclaredExecutor::Fixture),
        },
        false,
    )?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let projection = drive_to_quiescence(
        &store,
        &prepared.scope,
        prepared.stream.as_str(),
        &prepared.spec,
        &FixtureExecutor::new(prepared.fixtures.clone()),
        &super::system_actor(),
        &Release {
            nodes: &std::collections::BTreeSet::new(),
            actor: &super::owner_actor(),
        },
    )?;

    Ok(render(
        &projection,
        // Nothing measured here on purpose: this command reports the mutation it just made,
        // not a liveness reading. The seam turns "not measured" into `silenceUnevaluated`
        // rather than into calm, so the omission is stated instead of implied.
        // But the BUDGET is not a measurement. It is declared in the graph this projection
        // already holds, and `default()` asserted there was none -- so `attention` took the
        // `(None, measured)` arm and answered `NoDeclaredBudget` with the remedy "declare a
        // budget for this node", to an operator who had declared one (G's measurement on
        // #1013). `for_surface` derives it from the projection; the empty map is the AGE,
        // which really is unmeasured here, and that lands on the honest `(Some, None)` arm:
        // `NotMeasured`, remedy `Unavailable { SurfaceMeasuredNoAge }`.
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
        // The instants ARE measured here: this command just wrote to the store, so when the
        // log last moved is a fact it can read back. The silence budget above comes from the
        // projection; the per-node age remains unmeasured on this surface.
        &super::Liveness::from_store(&store, &prepared.scope, prepared.stream.as_str()),
        Some(&prepared.spec),
    ))
}

/// #90: a start that publishes and records `execution_started` and stops there.
///
/// THE HOLD IS THE ABSENCE OF THE DRIVE, so this is a separate function rather than an eighth
/// parameter on `execute`. `execute_prepared` is already "everything through the `ExecutionStarted`
/// append" -- the split Milestone 05d made so the async route could drive separately -- and a held
/// start is that half and no second half. A boolean would have said the same thing while making the
/// caller read a flag to find out which of two behaviours it gets; two names say it at the call
/// site. (`clippy::too_many_arguments` asked the question at 8/7; the answer it wanted was not an
/// `allow`.)
///
/// The projection is read BACK from the store rather than carried forward: the reply then describes
/// what a separate reader would see, which is what `execution status` reports and what the cell
/// asserts independently.
pub(crate) fn execute_held(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    mode: &str,
    execution: Option<&str>,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let prepared = execute_prepared(
        version,
        events,
        fixtures,
        mode,
        execution,
        Attribution {
            actor,
            key,
            // NOTHING drives a held start; what will is decided by whoever resumes it - the CLI
            // with fixtures, or `serve`'s runtime-backed route with the gateway - and `resume`
            // re-declares nothing. Declaring `fixture` here would be a claim about the future,
            // and the briefing would repeat it for work the gateway actually did.
            executor: None,
        },
        true,
    )?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (_, _, projection) = load_projection(&store, Some(prepared.execution_id.as_str()))?;
    Ok(render(
        &projection,
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
        &super::Liveness::from_store(&store, &prepared.scope, prepared.stream.as_str()),
        // #134: this door holds no graph, so no dispatch gate is published from it.
        None,
    ))
}

/// The decision half of `execute` (Milestone 05d Task 9's `execute_prepared` split): everything
/// through the `ExecutionStarted` append. Returns the [`PreparedDrive`] handoff the drive half —
/// sync (`execute`, above) or async (`serve::routes::start`) — needs to run
/// `drive_to_quiescence`/`drive_to_quiescence_async` afterward. `execute` is exactly
/// `execute_prepared` plus the same sync drive and render as before this split: CLI behavior is
/// byte-identical.
/// Who performed the act, and the key that makes performing it twice one act.
///
/// These two travel together at every call site and are handed to every event this function
/// appends, so they are one argument rather than two. Bundling them is also what leaves room for
/// `hold` below WITHOUT reaching for `#[allow(clippy::too_many_arguments)]` -- the lint was asking
/// a fair question and an allow would have answered it by silencing it.
pub(crate) struct Attribution {
    pub actor: PersistedActor,
    pub key: OpaqueId,
    /// What the caller is about to drive the nodes with, recorded on the declared form (#1063)
    /// so a later harness reading the store knows whether the outcomes came from fixtures or
    /// from a gateway. It rides with the attribution because it is decided at the same door by
    /// the same caller, and because the alternative was an eighth parameter. `None` for a held
    /// start, which drives nothing.
    pub executor: Option<DeclaredExecutor>,
}

pub(crate) fn execute_prepared(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    mode: &str,
    execution: Option<&str>,
    attribution: Attribution,
    hold: bool,
) -> Result<PreparedDrive, Failure> {
    let Attribution {
        actor,
        key,
        executor,
    } = attribution;
    let mode = parse_mode(mode)?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let fixtures = load_fixtures(fixtures)?;
    let stream_id = resolve_execution_id(version, execution)?;
    // THE SAME FUNCTION EVERY OTHER VERB ADDRESSES THROUGH (#775). This built the scope inline --
    // the same two constants and the same execution component, written a second time -- so the
    // path that CREATES a stream and the paths that ADDRESS one agreed only by coincidence. #560
    // was the same disagreement one surface over, and its fix added `addressable_scope` to be the
    // single home for this rule; `start` was not moved onto it. One question about identity,
    // answered in two places, is how the surfaces drift apart.
    let scope = super::addressable_scope(stream_id.as_str())?;

    let history = store
        .read_replay_stream(&scope, stream_id.as_str())
        .map_err(|error| repository_failure(&error))?;
    let existing = graphhelm_events::replay(&scope, stream_id.as_str(), &history)
        .map_err(|error| replay_failure(&error))?;
    // The fold's own `ExecutionStarted` arm would call a second one corrupt; the CLI refuses it
    // here so the diagnostic names the real reason instead of a bare integrity failure.
    if existing.execution_id.is_some() {
        return Err(execution_state(
            "an execution has already started on this stream",
            "/execution",
        ));
    }

    let graph_hash = WireHash::parse(version.content_hash().as_str()).map_err(|_| {
        execution_state(
            "the graph hash could not be represented on the wire",
            "/execution",
        )
    })?;

    // The declared shape, read through the single definition in `protocols` that the governor
    // also uses to persist the node. Two readings of one rule is how the first divergence
    // becomes invisible.
    let mut node_ids = Vec::new();
    let mut node_timeout_seconds = BTreeMap::new();
    let mut node_customs_budgets = BTreeMap::new();
    let mut node_descriptors = BTreeMap::new();
    for (id, node) in &version.graph().spec.nodes {
        let parsed = OpaqueId::parse(id).map_err(|_| {
            execution_state(
                "a node id in this graph cannot be represented on the wire",
                "/execution",
            )
        })?;
        // An entry exists ONLY for a node that declared a deadline: an absent key means the
        // operator declared nothing, and must never be read as a budget of zero.
        if let Some(seconds) = graphhelm_protocols::declared_timeout_seconds(node) {
            node_timeout_seconds.insert(parsed.clone(), seconds);
        }
        // #1184 review: the customs budgets travel WITH the declaration, because nothing on this
        // path publishes a graph version for `stage_deadline` to read them from. Same rule as the
        // timeout above — only a node that declared them gets a key.
        //
        // AN UNREADABLE BLOCK RECORDS NOTHING, and does not refuse the start. It cannot produce a
        // wait either: such a graph is refused by the drive's own preflight
        // (`GHG017_CUSTOMS_DECLARATION_INVALID`) before any node runs, so no node ever parks and
        // there is no stage for a missing budget to fail to bound. Refusing here instead would
        // move that refusal to a second place with a different code, and the two could drift.
        if let Ok(Some(customs)) = node.customs() {
            node_customs_budgets.insert(parsed.clone(), customs.budgets);
        }
        node_descriptors.insert(
            parsed.clone(),
            NodeDescriptor {
                name: declared_text(&node.name).unwrap_or_else(|| id.clone()),
                role: node.node_type.clone(),
            },
        );
        node_ids.push(parsed);
    }
    // #1063: what the run is FOR, recorded where the shape is. `name` is the document's own
    // name (the goal, for a synthesized graph); `objective` is the first entrypoint's objective,
    // which is where the Studio's draft keeps the operator's words verbatim. Both bounded and
    // truncated, never refused - see `bound_declared_text`.
    let graph = version.graph();
    let objective = graph
        .spec
        .entrypoints
        .first()
        .and_then(|entry| graph.spec.nodes.get(entry))
        .and_then(|node| declared_text(&node.objective));
    let topology = DeclaredTopology {
        graph_hash: graph_hash.clone(),
        entrypoints: graph.spec.entrypoints.clone(),
        edges: graph
            .spec
            .edges
            .iter()
            .map(|edge| DeclaredTopologyEdge {
                id: edge.id.clone(),
                from: edge.from.clone(),
                to: edge.to.clone(),
                edge_type: match &edge.edge_type {
                    EdgeType::Control => "control",
                    EdgeType::Data => "data",
                    EdgeType::Evidence => "evidence",
                    EdgeType::Event => "event",
                    EdgeType::Failure => "failure",
                    EdgeType::Compensation => "compensation",
                    EdgeType::HumanApproval => "human_approval",
                }
                .to_owned(),
            })
            .collect(),
    };
    let mut declared_form = ExecutionFormDeclared {
        execution_id: stream_id.clone(),
        node_ids,
        node_descriptors,
        topology: Some(topology),
        node_timeout_seconds,
        node_customs_budgets,
        name: declared_text(&graph.metadata.name),
        objective,
        executor,
    };
    // Keep the journal's one-event ceiling reachable for large accepted graphs. A missing
    // snapshot leaves Studio honest (manual hash-checked topology remains available), while
    // refusing start here would make a display convenience block execution.
    if serde_json::to_vec(&declared_form)
        .is_ok_and(|bytes| bytes.len() > MAX_FORM_BYTES_WITH_TOPOLOGY)
    {
        declared_form.topology = None;
    }
    let hold_key = OpaqueId::parse(format!("{}-held", key.as_str())).map_err(|_| {
        execution_state(
            "the hold key could not be represented on the wire",
            "/execution",
        )
    })?;
    let declaration_key = OpaqueId::parse(format!("{}-form", key.as_str())).map_err(|_| {
        execution_state(
            "the declaration key could not be represented on the wire",
            "/execution",
        )
    })?;

    let next_sequence = store
        .next_sequence(&scope, stream_id.as_str())
        .map_err(|error| repository_failure(&error))?;
    let request = PreparedAppend::new(
        scope.clone(),
        stream_id.clone(),
        next_sequence,
        vec![
            NewEvent::new(
                key.clone(),
                actor.clone(),
                Sensitivity::Internal,
                EventKind::ExecutionStarted(ExecutionStarted {
                    execution_id: stream_id.clone(),
                    graph_version: version.number(),
                    graph_hash,
                    mode,
                }),
                vec![],
                vec![],
            ),
            // Appended WITH the start, in the same atomic request, so no reader can observe an
            // execution that began without the shape it declared. Recorded unsealed on purpose:
            // a seal protects the evidence behind content slots, and a declared shape carries
            // no evidence to protect. Fusing those two ideas is what made every rule needing
            // the shape demand a credential it has no use for.
            // Appended WITH the start, in the same atomic request, so no reader can observe an
            // execution that began without the shape it declared. Recorded unsealed on purpose:
            // a seal protects the evidence behind content slots, and a declared shape carries
            // no evidence to protect. Fusing those two ideas is what made every rule needing
            // the shape demand a credential it has no use for.
            NewEvent::new(
                declaration_key,
                actor.clone(),
                Sensitivity::Internal,
                EventKind::ExecutionFormDeclared(declared_form),
                vec![],
                vec![],
            ),
        ]
        .into_iter()
        // #90: THE HOLD RIDES IN THE SAME ATOMIC REQUEST, for the reason written two comments up
        // about the declared shape -- the argument is identical and so is the failure it prevents.
        //
        // Appending it separately left a window (Codex P1 on this PR, and it is right): a process
        // that dies between the two appends leaves `ExecutionStarted` + `ExecutionFormDeclared`
        // with no `ExecutionPaused`. That state is worse than either endpoint, because BOTH exits
        // are shut -- `start --held` refuses with "an execution has already started on this
        // stream", and the `resume` this flag documents refuses with `not_paused`. The operator
        // must first diagnose a partial write and then repair it with a `pause` nobody told them
        // to run. One request has no window: the reader sees a held execution or no execution.
        .chain(hold.then(|| {
            // #90: THE HOLD IS AN EXPLICIT DECISION, so it is attributed to whoever made it.
            //
            // `owner_actor`'s own doc (`execution/mod.rs`) draws the line: "The driver's automatic
            // hops stay under the system actor; every explicit decision is recorded as the owner's",
            // and its list of owner-initiated commands names `pause`. This event is an
            // `ExecutionPaused` -- the same kind `pause` emits, and `pause.rs` writes it as
            // `owner_actor()`. Nobody reached it by a driver hop: an operator typed `--held`.
            //
            // The defence at the top of this file -- that `start` keeps `system_actor()` because it
            // is not on that list -- is about `start`'s OWN events. It does not reach an
            // `ExecutionPaused` that this flag newly emits (A, blocking on this PR).
            //
            // THE CONDITION IS NOT A SPECIAL CASE, IT IS THE RULE: a constant is the fallback for an
            // ABSENT identity, never the substitute for a PRESENT one. `actor` is `system_actor()`
            // on the CLI path -- a placeholder, no caller in it -- but over the API it is the
            // caller's real owner/agent identity from the request headers (see this function's doc).
            // Overwriting that with the `owner-cli` constant would DESTROY an attribution in an
            // append-only log, which is worse than the gap it was meant to close. So: substitute
            // only where there is nothing to lose.
            let hold_actor = if actor.actor_type() == PersistedActorType::System {
                super::owner_actor()
            } else {
                actor
            };
            NewEvent::new(
                hold_key,
                hold_actor,
                Sensitivity::Internal,
                EventKind::ExecutionPaused(ExecutionPaused {
                    execution_id: stream_id.clone(),
                }),
                vec![],
                vec![],
            )
        }))
        .collect(),
        vec![],
        vec![],
    )
    .map_err(|error| repository_failure(&error))?;
    store
        .append_atomic(&request)
        .map_err(|error| repository_failure(&error))?;

    Ok(PreparedDrive {
        scope,
        stream: stream_id.clone(),
        execution_id: stream_id,
        spec: version.graph().spec.clone(),
        fixtures,
        // `start` holds nothing, so it has nothing to release.
        release: std::collections::BTreeSet::new(),
    })
}

fn parse_mode(mode: &str) -> Result<ExecutionMode, Failure> {
    match mode {
        "autopilot" => Ok(ExecutionMode::Autopilot),
        "supervised" => Ok(ExecutionMode::Supervised),
        "manual" => Ok(ExecutionMode::Manual),
        _ => Err(argument(
            "--mode must be one of autopilot, supervised or manual",
            "/mode",
        )),
    }
}

/// The execution identity to drive: `--execution` when given, otherwise the graph's own
/// A graph's own prose, projected onto the unsealed declaration ONLY when the durable-content
/// scan admits it (#1063). The scan is the store's own (`validate_durable_content`, the rule
/// every append is held to), applied at the door so a name or objective that LOOKS like a
/// secret - a `secret://` reference, a token-shaped run - is left off the declaration rather
/// than turning the whole start into `GHE009_EXTERNALIZATION_FAILED`. Before this field
/// existed that graph started; it still does, and the briefing says "absent" for it, which is
/// the honest reading of text the log must not carry.
fn declared_text(text: &str) -> Option<String> {
    let bounded = graphhelm_protocols::bound_declared_text(text)?;
    graphhelm_graph::validate_durable_content(&serde_json::Value::String(bounded.clone()), &[])
        .ok()
        .map(|()| bounded)
}

/// `metadata.executionId` — the same convention `graph simulate` uses unconditionally. Returns
/// both the `OpaqueId` (stream identity) and `ExecutionId` (scope identity) parsed from the same
/// string, matching `simulate.rs`'s own double-parse of `version.graph().metadata.execution_id`.
fn resolve_execution_id(
    version: &GraphVersion,
    execution: Option<&str>,
) -> Result<OpaqueId, Failure> {
    match execution {
        Some(value) => {
            let stream_id = OpaqueId::parse(value)
                .map_err(|_| argument("--execution is not a valid identifier", "/execution"))?;
            Ok(stream_id)
        }
        None => {
            let raw = &version.graph().metadata.execution_id;
            let stream_id =
                OpaqueId::parse(raw).expect("validated graph execution id is wire-safe");
            Ok(stream_id)
        }
    }
}
