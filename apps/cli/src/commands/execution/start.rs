use std::{collections::BTreeMap, path::Path};

use graphhelm_events::PreparedAppend;
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    EventKind, ExecutionFormDeclared, ExecutionId, ExecutionMode, ExecutionStarted, NewEvent,
    OpaqueId, PersistedActor, ProjectId, RepositoryScope, Sensitivity, WireHash, WorkspaceId,
};

use super::driver::drive_to_quiescence;
use super::{
    Failure, PROJECT, PreparedDrive, WORKSPACE, argument, execution_state, finish, idempotency_key,
    load_fixtures, render, replay_failure, repository_failure,
};
use crate::commands::{event_store, owner, publish_loaded};
use crate::output::Outcome;

const COMMAND: &str = "execution.start";

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
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal(COMMAND, error),
    };
    finish(
        COMMAND,
        execute(
            &version,
            events,
            fixtures,
            mode,
            execution,
            super::system_actor(),
            idempotency_key("execution-started"),
        ),
        |value| value,
    )
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
    let prepared = execute_prepared(version, events, fixtures, mode, execution, actor, key)?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let projection = drive_to_quiescence(
        &store,
        &prepared.scope,
        prepared.stream.as_str(),
        &prepared.spec,
        &prepared.fixtures,
        &super::system_actor(),
    )?;

    Ok(render(
        &projection,
        // Nothing measured here on purpose: this command reports the mutation it just made,
        // not a liveness reading. The seam turns "not measured" into `silenceUnevaluated`
        // rather than into calm, so the omission is stated instead of implied.
        &graphhelm_execution::AttentionInputs::default(),
        // The instants ARE measured here: this command just wrote to the store, so when the
        // log last moved is a fact it can read back. Only the silence BUDGET stays absent.
        &super::Liveness::from_store(&store, &prepared.scope, prepared.stream.as_str()),
    ))
}

/// The decision half of `execute` (Milestone 05d Task 9's `execute_prepared` split): everything
/// through the `ExecutionStarted` append. Returns the [`PreparedDrive`] handoff the drive half —
/// sync (`execute`, above) or async (`serve::routes::start`) — needs to run
/// `drive_to_quiescence`/`drive_to_quiescence_async` afterward. `execute` is exactly
/// `execute_prepared` plus the same sync drive and render as before this split: CLI behavior is
/// byte-identical.
pub(crate) fn execute_prepared(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    mode: &str,
    execution: Option<&str>,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<PreparedDrive, Failure> {
    let mode = parse_mode(mode)?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let fixtures = load_fixtures(fixtures)?;
    let (stream_id, execution_scope_id) = resolve_execution_id(version, execution)?;
    let scope = RepositoryScope::new(
        WorkspaceId::parse(WORKSPACE).expect("constant workspace id is valid"),
        ProjectId::parse(PROJECT).expect("constant project id is valid"),
        Some(execution_scope_id),
    );

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
        node_ids.push(parsed);
    }
    let declared_form = ExecutionFormDeclared {
        execution_id: stream_id.clone(),
        node_ids,
        node_timeout_seconds,
    };
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
                actor,
                Sensitivity::Internal,
                EventKind::ExecutionFormDeclared(declared_form),
                vec![],
                vec![],
            ),
        ],
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
/// `metadata.executionId` — the same convention `graph simulate` uses unconditionally. Returns
/// both the `OpaqueId` (stream identity) and `ExecutionId` (scope identity) parsed from the same
/// string, matching `simulate.rs`'s own double-parse of `version.graph().metadata.execution_id`.
fn resolve_execution_id(
    version: &GraphVersion,
    execution: Option<&str>,
) -> Result<(OpaqueId, ExecutionId), Failure> {
    match execution {
        Some(value) => {
            let stream_id = OpaqueId::parse(value)
                .map_err(|_| argument("--execution is not a valid identifier", "/execution"))?;
            let scope_id = ExecutionId::parse(value)
                .map_err(|_| argument("--execution is not a valid identifier", "/execution"))?;
            Ok((stream_id, scope_id))
        }
        None => {
            let raw = &version.graph().metadata.execution_id;
            let stream_id =
                OpaqueId::parse(raw).expect("validated graph execution id is wire-safe");
            let scope_id =
                ExecutionId::parse(raw).expect("validated graph execution id is wire-safe");
            Ok((stream_id, scope_id))
        }
    }
}
