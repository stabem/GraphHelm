mod binding;
mod budget;
mod cycle;
mod deployment;
mod reachability;
mod security;

use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::{Diagnostic, ExecutionGraph, NodeType};

/// Deterministically ordered semantic errors and warnings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LintReport {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}

/// Runs pure semantic checks over a schema-valid graph.
#[must_use]
pub fn lint(graph: &ExecutionGraph, source: &str) -> LintReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let node_ids: BTreeSet<_> = graph.spec.nodes.keys().cloned().collect();

    for (index, entrypoint) in graph.spec.entrypoints.iter().enumerate() {
        if !node_ids.contains(entrypoint) {
            errors.push(error(
                "GHG001_ENTRYPOINT_UNKNOWN",
                "entrypoint does not name a graph node",
                format!("/spec/entrypoints/{index}"),
                source,
            ));
        }
    }

    let mut edge_ids = BTreeMap::new();
    for (index, edge) in graph.spec.edges.iter().enumerate() {
        if !node_ids.contains(&edge.from) {
            errors.push(error(
                "GHG002_EDGE_SOURCE_UNKNOWN",
                "edge source does not name a graph node",
                format!("/spec/edges/{index}/from"),
                source,
            ));
        }
        if !node_ids.contains(&edge.to) {
            errors.push(error(
                "GHG003_EDGE_TARGET_UNKNOWN",
                "edge target does not name a graph node",
                format!("/spec/edges/{index}/to"),
                source,
            ));
        }
        if edge_ids.insert(edge.id.as_str(), index).is_some() {
            errors.push(error(
                "GHG004_EDGE_ID_DUPLICATE",
                "edge ID is duplicated",
                format!("/spec/edges/{index}/id"),
                source,
            ));
        }
    }

    errors.extend(reachability::check(graph, source));
    errors.extend(cycle::check(graph, source));
    errors.extend(binding::check(graph, source));
    let secret_errors = security::check(graph, source);
    errors.extend(secret_errors.iter().cloned());
    errors.extend(deployment::check(graph, source));
    errors.extend(budget::check(graph, source));
    errors.extend(check_hard_policies(graph, &secret_errors, source));

    for (id, node) in &graph.spec.nodes {
        if matches!(
            node.node_type,
            NodeType::Agent
                | NodeType::Tool
                | NodeType::Deploy
                | NodeType::Rollback
                | NodeType::ArtifactTransform
        ) && !node.properties.contains_key("timeoutSeconds")
        {
            warnings.push(Diagnostic::warning(
                "GHG101_DEFAULT_TIMEOUT",
                "executable node relies on the runtime default timeout",
                format!("/spec/nodes/{}/timeoutSeconds", escape(id)),
                source,
            ));
        }
    }

    /// Can this node type reach `WaitingInput`, and therefore park?
    ///
    /// EXHAUSTIVE ON PURPOSE, and the wildcard is the defect this function was extracted to remove
    /// (#545). The rule was a `matches!` over six hand-typed names, and it omitted `Planner`,
    /// `Classifier` and `Evaluator` -- three types `classify::work_kind` dispatches through the SAME
    /// match arm as `Agent`. A `Planner` that parked with no budgets was named by nothing, at
    /// authoring time or ever, which is the exact silence GHG102 exists to end. Written as an
    /// exhaustive match so the next variant added to `NodeType` breaks this build instead of
    /// inheriting "not parkable" in silence.
    ///
    /// **TWO AUTHORITIES, AND THIS IS ONLY ONE OF THEM.** Whether a node parks is decided by the state
    /// machine -- `(Running, NeedsInput) => WaitingInput`, which is not gated by node type at all --
    /// so the real population is "what gets dispatched and run", and that lives in
    /// `graphhelm_runtime::classify::work_kind`. `core/graph` does not depend on `core/runtime` and
    /// should not start here, so the two lists are kept in agreement by DECLARATION rather than by
    /// derivation. Nothing in one crate fails when the other moves. If that agreement is ever worth
    /// a guard, it needs a witness that sees both crates and does not derive from either -- a test
    /// mirroring this match against itself would prove only that the file equals the file.
    fn can_park_for_input(node_type: &NodeType) -> bool {
        match node_type {
            // Dispatched as work and therefore able to be Running: these park by the state machine.
            // The four cognitive kinds travel together in `work_kind` and must travel together here.
            NodeType::Agent
            | NodeType::Planner
            | NodeType::Classifier
            | NodeType::Evaluator
            | NodeType::Tool => true,
            // Kept deliberately, and the reason is forward-looking rather than current: the executor
            // refuses these today, so they cannot be Running and cannot park yet. Warning about a node
            // that cannot park costs an author one line; staying silent about one that can costs a
            // parked execution nobody is watching. `HumanDecision` is the clearest case -- a node whose
            // entire purpose is to wait for a person is the one most able to wait forever.
            NodeType::HumanDecision
            | NodeType::Deploy
            | NodeType::Rollback
            | NodeType::ArtifactTransform => true,
            // Structural or terminal: never dispatched as work, so never Running, so never parked.
            // `DeadLetter` is where work STOPS -- a graveyard that could go overdue would be a queue.
            NodeType::Gate
            | NodeType::Fork
            | NodeType::Join
            | NodeType::Timer
            | NodeType::Trigger
            | NodeType::Subgraph
            | NodeType::Materializer
            | NodeType::DeadLetter => false,
        }
    }

    // M11 #160 (G2 part 1): a node that can PARK FOR INPUT and declares no customs budgets can
    // wait forever, and nothing in the system will ever say so. The sweep raises an overdue
    // exception from a stage deadline, a stage deadline comes from a declared budget, and an
    // absent budget is honestly absent rather than defaulted — which closes the "instantly
    // overdue" trap at the cost of leaving genuinely unbounded stages silent. This warning is
    // where that cost gets paid back: the silence becomes visible at authoring time instead of
    // at 3am on a parked execution nobody is watching.
    //
    // WARNING and not an error, deliberately: every graph checked in today predates customs, and
    // making this an error would refuse graphs that are working. It follows `GHG101` exactly —
    // same shape, same reasoning, one milestone later, for the same class of defect (a bound
    // nobody declared).
    //
    // The node set is the set that can reach `WaitingInput`, which is the set that can be
    // dispatched and run. `HumanDecision` is IN and is the clearest case: a node whose entire
    // purpose is to wait for a person is the one most able to wait forever.
    for (id, node) in &graph.spec.nodes {
        let can_park = can_park_for_input(&node.node_type);
        let declares_customs = node
            .properties
            .get("completion")
            .and_then(|completion| completion.get("customs"))
            .is_some();
        if can_park && !declares_customs {
            warnings.push(Diagnostic::warning(
                "GHG102_UNBOUNDED_CUSTOMS",
                "node can park for input but declares no customs budgets, so no stage of it can ever go overdue",
                format!("/spec/nodes/{}/completion/customs", escape(id)),
                source,
            ));
        }
    }

    sort_diagnostics(&mut errors);
    sort_diagnostics(&mut warnings);
    LintReport { errors, warnings }
}

fn check_hard_policies(
    graph: &ExecutionGraph,
    secret_errors: &[Diagnostic],
    source: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (index, policy) in graph.spec.policies.iter().enumerate() {
        let deny = policy
            .as_str()
            .and_then(|value| value.strip_prefix("deny:"))
            .or_else(|| policy.get("deny").and_then(serde_json::Value::as_str));
        let denied = match deny {
            Some("deploy" | "production.deploy") => graph
                .spec
                .nodes
                .values()
                .any(|node| node.node_type == NodeType::Deploy),
            Some("secret.inline") => !secret_errors.is_empty(),
            _ => false,
        };
        if denied {
            diagnostics.push(error(
                "GHG014_HARD_POLICY_DENIED",
                "graph conflicts with an inline hard deny policy",
                format!("/spec/policies/{index}"),
                source,
            ));
        }
    }
    diagnostics
}

pub(super) fn error(
    code: &str,
    message: impl Into<String>,
    path: impl Into<String>,
    source: &str,
) -> Diagnostic {
    Diagnostic::error(code, message, path, source)
}

pub(super) fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        (&left.path, &left.code, &left.message).cmp(&(&right.path, &right.code, &right.message))
    });
}
