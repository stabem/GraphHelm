use std::path::Path;

use graphhelm_graph::lint;
use graphhelm_protocols::{EdgeType, GraphEdge, NodeType};

fn graph() -> graphhelm_protocols::ExecutionGraph {
    graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph
}

fn assert_code(graph: &graphhelm_protocols::ExecutionGraph, code: &str, path: &str) {
    let report = lint(graph, "fixture.yaml");
    assert!(
        report
            .errors
            .iter()
            .any(|item| item.code == code && item.path == path),
        "missing {code} at {path}: {report:?}"
    );
}

#[test]
fn reference_duplicate_and_binding_failures_have_stable_codes_and_paths() {
    let mut invalid = graph();
    invalid.spec.entrypoints[0] = "missing".into();
    invalid.spec.edges[0].from = "absent-source".into();
    invalid.spec.edges[1].to = "absent-target".into();
    invalid.spec.edges[2].id = invalid.spec.edges[0].id.clone();
    invalid.spec.edges[3]
        .bindings
        .insert("artifact".into(), "outputs.ghost.result".into());

    assert_code(&invalid, "GHG001_ENTRYPOINT_UNKNOWN", "/spec/entrypoints/0");
    assert_code(&invalid, "GHG002_EDGE_SOURCE_UNKNOWN", "/spec/edges/0/from");
    assert_code(&invalid, "GHG003_EDGE_TARGET_UNKNOWN", "/spec/edges/1/to");
    assert_code(&invalid, "GHG004_EDGE_ID_DUPLICATE", "/spec/edges/2/id");
    assert_code(
        &invalid,
        "GHG007_BINDING_SOURCE_UNKNOWN",
        "/spec/edges/3/map/artifact",
    );
}

#[test]
fn topology_security_and_deployment_failures_are_not_silent() {
    let mut invalid = graph();
    invalid.spec.completion = serde_json::json!({"terminalNodes": ["ghost"]});
    invalid.spec.edges.push(GraphEdge {
        id: "docs-back-to-plan".into(),
        from: "docs".into(),
        to: "plan".into(),
        edge_type: EdgeType::Control,
        payload_schema: None,
        condition: None,
        on_false: None,
        on_unknown: None,
        bindings: Default::default(),
        priority: None,
    });
    let deploy = invalid.spec.nodes.get_mut("implement").unwrap();
    deploy.node_type = NodeType::Deploy;
    deploy.properties.remove("targetRef");
    deploy
        .properties
        .insert("apiKey".into(), serde_json::json!("literal-secret"));
    deploy
        .properties
        .insert("effects".into(), serde_json::json!({"reversible": true}));

    assert_code(&invalid, "GHG005_NO_TERMINAL_PATH", "/spec/nodes/docs");
    assert_code(
        &invalid,
        "GHG006_UNCONTROLLED_CYCLE",
        "/spec/nodes/docs/loop",
    );
    assert_code(
        &invalid,
        "GHG008_INLINE_SECRET",
        "/spec/nodes/implement/apiKey",
    );
    assert_code(
        &invalid,
        "GHG009_DEPLOY_TARGET_MISSING",
        "/spec/nodes/implement/targetRef",
    );
    assert_code(
        &invalid,
        "GHG010_COMPENSATION_MISSING",
        "/spec/nodes/implement/effects/compensationNode",
    );
}

#[test]
fn budgets_and_hard_policies_are_enforced_deterministically() {
    let mut invalid = graph();
    invalid.spec.budgets.max_nodes = Some(1);
    invalid.spec.budgets.max_depth = Some(1);
    invalid.spec.budgets.max_retries_per_node = Some(1);
    invalid
        .spec
        .nodes
        .get_mut("implement")
        .unwrap()
        .properties
        .insert("retry".into(), serde_json::json!({"maxAttempts": 4}));
    let deploy = invalid.spec.nodes.get_mut("docs").unwrap();
    deploy.node_type = NodeType::Deploy;
    deploy.properties.insert(
        "targetRef".into(),
        serde_json::json!("environment://staging"),
    );
    invalid
        .spec
        .policies
        .push(serde_json::json!({"deny": "deploy"}));

    assert_code(
        &invalid,
        "GHG011_NODE_BUDGET_EXCEEDED",
        "/spec/budgets/maxNodes",
    );
    assert_code(
        &invalid,
        "GHG012_DEPTH_BUDGET_EXCEEDED",
        "/spec/budgets/maxDepth",
    );
    assert_code(
        &invalid,
        "GHG013_RETRY_BUDGET_EXCEEDED",
        "/spec/nodes/implement/retry/maxAttempts",
    );
    assert_code(&invalid, "GHG014_HARD_POLICY_DENIED", "/spec/policies/1");

    let first = lint(&invalid, "fixture.yaml");
    let second = lint(&invalid, "fixture.yaml");
    assert_eq!(first, second);
    assert!(first.errors.windows(2).all(|pair| {
        (&pair[0].path, &pair[0].code, &pair[0].message)
            <= (&pair[1].path, &pair[1].code, &pair[1].message)
    }));
}

#[test]
fn executable_node_without_timeout_gets_stable_warning() {
    let report = lint(&graph(), "fixture.yaml");
    assert!(report.warnings.iter().any(|item| {
        item.code == "GHG101_DEFAULT_TIMEOUT" && item.path == "/spec/nodes/implement/timeoutSeconds"
    }));
}

/// M11 #160 (G2 part 1): a node that can park for input and declares no customs budgets is
/// flagged at AUTHORING time, because nothing downstream will ever flag it.
///
/// The pair is what makes this a guard rather than a description. A rule that only fires would
/// pass just as happily if it fired on every node in the graph, so the second half names a node
/// that DOES declare budgets and requires the warning to be absent for it. Without that, "warns
/// about unbounded nodes" and "warns about all nodes" are the same green.
#[test]
fn a_node_that_can_park_without_customs_budgets_gets_a_stable_warning() {
    let mut declared = graph();
    declared
        .spec
        .nodes
        .get_mut("implement")
        .unwrap()
        .properties
        .insert(
            "completion".to_owned(),
            serde_json::json!({
                "customs": {
                    "budgets": {
                        "waitWithinSeconds": 3600,
                        "clearanceWithinSeconds": 900,
                    }
                }
            }),
        );

    let undeclared = lint(&graph(), "fixture.yaml");
    assert!(
        undeclared.warnings.iter().any(|item| {
            item.code == "GHG102_UNBOUNDED_CUSTOMS"
                && item.path == "/spec/nodes/implement/completion/customs"
        }),
        "a parkable node with no customs budgets must be named at authoring time: {:?}",
        undeclared.warnings
    );

    let bounded = lint(&declared, "fixture.yaml");
    assert!(
        !bounded.warnings.iter().any(|item| {
            item.code == "GHG102_UNBOUNDED_CUSTOMS"
                && item.path == "/spec/nodes/implement/completion/customs"
        }),
        "and the SAME node with budgets declared must stop being named, or the rule is not \
         reading the declaration: {:?}",
        bounded.warnings
    );
}

#[test]
fn controlled_cycle_is_condensed_when_enforcing_depth_budget() {
    let mut invalid = graph();
    let mut archive = invalid.spec.nodes["docs"].clone();
    archive.name = "Archive".into();
    invalid.spec.nodes.insert("archive".into(), archive);
    invalid
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert("loop".into(), serde_json::json!({"maxIterations": 2}));
    invalid.spec.edges.push(GraphEdge {
        id: "docs-back-to-plan".into(),
        from: "docs".into(),
        to: "plan".into(),
        edge_type: EdgeType::Control,
        payload_schema: None,
        condition: None,
        on_false: None,
        on_unknown: None,
        bindings: Default::default(),
        priority: None,
    });
    invalid.spec.edges.push(GraphEdge {
        id: "docs-to-archive".into(),
        from: "docs".into(),
        to: "archive".into(),
        edge_type: EdgeType::Control,
        payload_schema: None,
        condition: None,
        on_false: None,
        on_unknown: None,
        bindings: Default::default(),
        priority: None,
    });
    invalid.spec.budgets.max_depth = Some(2);

    assert_code(
        &invalid,
        "GHG012_DEPTH_BUDGET_EXCEEDED",
        "/spec/budgets/maxDepth",
    );
}

#[test]
fn inline_secret_in_forward_compatible_metadata_is_rejected() {
    let mut invalid = graph();
    invalid
        .metadata
        .properties
        .insert("token".into(), serde_json::json!("plaintext-secret"));
    assert_code(&invalid, "GHG008_INLINE_SECRET", "/metadata/token");
}

/// #545: the population of GHG102 is a hand-typed list, and it omits three node types that park
/// exactly as `Agent` does.
///
/// `classify::work_kind` dispatches `Agent | Planner | Classifier | Evaluator` through ONE match
/// arm, as Cognitive work. `WaitingInput` is not gated by node type at all -- the state machine
/// says `(Running, NeedsInput) => WaitingInput` -- so every type that arm dispatches can park.
/// The lint names `Agent` and not its three siblings, and the existing pair above stays green
/// through the whole defect because it exercises ONE node.
///
/// This is the population half of a guard whose two hand-chosen parameters are its POPULATION and
/// its FORM. The form was measured; this is the half that was not.
#[test]
fn every_cognitive_sibling_of_agent_is_warned_when_it_declares_no_budgets() {
    // Each of these is dispatched by the same arm as `Agent`, so each can reach `WaitingInput`.
    for node_type in [
        NodeType::Agent,
        NodeType::Planner,
        NodeType::Classifier,
        NodeType::Evaluator,
    ] {
        let mut graph = graph();
        let node = graph.spec.nodes.get_mut("implement").unwrap();
        node.node_type = node_type.clone();
        node.properties.remove("completion");

        let warnings = lint(&graph, "fixture.yaml");
        assert!(
            warnings.warnings.iter().any(|item| {
                item.code == "GHG102_UNBOUNDED_CUSTOMS"
                    && item.path == "/spec/nodes/implement/completion/customs"
            }),
            "a {node_type:?} node with no customs budgets parks forever and is named by nothing: \
             it is dispatched through the same match arm as Agent, and WaitingInput is reached by \
             a state transition rather than by node type, so the authoring warning is the only \
             thing between this graph and permanent quarantine. Warnings seen: {:?}",
            warnings.warnings
        );
    }
}
