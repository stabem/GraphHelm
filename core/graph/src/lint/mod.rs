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
            // Not dispatched by this milestone, and warned anyway because their WORK is the kind
            // that waits: a human, a deployment, a rollback, a transform of something someone else
            // produces. `work_kind` refuses them today, so they cannot park yet -- but warning about
            // a node that cannot park costs an author one line, and silence about one that can costs
            // a parked execution nobody is watching. `HumanDecision` is the clearest case: a node
            // whose entire purpose is to wait for a person is the one most able to wait forever.
            NodeType::HumanDecision
            | NodeType::Deploy
            | NodeType::Rollback
            | NodeType::ArtifactTransform => true,
            // DISPATCHED, and still cannot park -- the one member here whose exclusion is a measured
            // capability rather than a judgement (#549). `work_kind` returns `GateCheck`
            // (`core/runtime/src/classify.rs:34`) and the driver runs it, so "never dispatched" was
            // FALSE of this variant and shipped in #547 as part of a sentence covering eight. What is
            // true: `gate_check_outcome` (`core/runtime/src/executor.rs:470-508`) produces exactly two
            // outcomes -- `Succeeded` when the findings are empty, `TerminalFailure` when they are not
            // -- and `WaitingInput` is only reachable via `NeedsInput`, which is not in that set. A
            // gate decides; it does not wait for anyone. Per-gate dispatch (#668) added a REFUSAL to
            // that function for a gate this build does not register, which is not an outcome: the
            // node is never dispatched, so the outcome set is unchanged.
            //
            // NOT GUARDED, and said so rather than implied: nothing fails if that outcome set gains a
            // third member. This reason is a citation across a crate boundary `core/graph` does not
            // depend on, exactly like the population question above it.
            NodeType::Gate => false,
            // Control flow, and the reason is their SHAPE rather than the current dispatch table:
            // a fork, a join, a subgraph boundary or a materialisation is a structural step that
            // completes or fails, with nobody to wait for. `Timer` is the one that reads like a
            // counter-example and is not: it waits on its OWN clock, which is the mechanism customs
            // budgets exist to bound elsewhere, not an input another party supplies.
            //
            // These sit apart from the four above on a stated principle, because the two groups are
            // both "refused by `work_kind` today" and a reader is owed the difference: the four are
            // work whose NATURE is waiting, these are steps whose nature is completing. If one of
            // them ever gains a driver that can report `NeedsInput`, it belongs above, and this
            // comment is where that argument starts.
            //
            // `Trigger` IS NOT COVERED BY THE SENTENCE ABOVE, and saying so is the point (#549, found
            // by D). Its nature is UNESTABLISHED in this repository: measured at `origin/main`, the
            // variant carries no doc comment on the enum, no graph or fixture authors `type: trigger`
            // (0 files, against 7 for `type: agent` as the control that the sweep sees things), and
            // its only three appearances are membership lists -- this arm, `work_kind`'s Unsupported
            // arm, and `driver_contract`'s. Three lists, zero definitions.
            //
            // It sits here by CAPABILITY DEFAULT -- `work_kind` refuses it, so it cannot be Running
            // and cannot park today -- and NOT by a classified reason, because there is nothing to
            // classify it from. Its name is the one here that most suggests waiting, which is exactly
            // why the default must be written rather than absorbed. Landing a justification that
            // silently covered a member it could not describe would repeat, inside the fix, the
            // defect the fix exists for.
            NodeType::Fork
            | NodeType::Join
            | NodeType::Timer
            | NodeType::Trigger
            | NodeType::Subgraph
            | NodeType::Materializer => false,
            // Terminal, and the strongest exclusion here: a dead-lettered node is where work STOPS.
            // `work_kind` refuses it PERMANENTLY rather than pending a driver (#288), so a graveyard
            // that could go overdue would be a queue.
            NodeType::DeadLetter => false,
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
