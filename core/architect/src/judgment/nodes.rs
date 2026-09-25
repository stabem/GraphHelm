//! Site 3: per-node judgments. State = the goal and every node's `{id, type, objective}`;
//! questions = per node, `on_goal:<id>` (Noul) and `kind:<id>` (Choice over the catalog's node
//! types). A "no" on `on_goal` is `GHA005_NODE_OFF_GOAL`; a confident `kind` that differs from
//! the draft's type is `GHA006_NODE_KIND_MISMATCH`. Both are REPAIRABLE: they go back to the
//! draft model with the round's other diagnostics. Nothing else is read.

use std::collections::BTreeMap;

use graphhelm_gateway::judgment::{
    Answer, JEV_LATEST, JudgeReply, JudgeRequest, NoulCriteria, Question,
};
use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use super::NodeJudgment;
use super::policy::{acts, noul_is_no, noul_is_yes};
use crate::catalog::CapabilityCatalog;
use crate::profile::TaskProfile;
use crate::synthesize::{DRAFT_SOURCE, escape};

/// Judged diagnostic: the judge read the node's objective as not serving the goal. Repairable.
pub const NODE_OFF_GOAL_CODE: &str = "GHA005_NODE_OFF_GOAL";
/// Judged diagnostic: the judge, confidently, would type the node's objective differently from
/// the draft. Repairable.
pub const NODE_KIND_MISMATCH_CODE: &str = "GHA006_NODE_KIND_MISMATCH";

/// The request for one accepted draft: two questions per node, over the goal and the nodes.
#[must_use]
pub fn request(
    profile: &TaskProfile,
    catalog: &CapabilityCatalog,
    graph: &ExecutionGraph,
) -> JudgeRequest {
    let nodes: Vec<serde_json::Value> = graph
        .spec
        .nodes
        .iter()
        .map(|(id, node)| {
            serde_json::json!({
                "id": id,
                "type": node.node_type.as_str(),
                "objective": node.objective,
            })
        })
        .collect();
    let kinds: BTreeMap<String, Option<String>> = catalog
        .node_types
        .iter()
        .map(|kind| (kind.clone(), None))
        .collect();
    let mut questions = BTreeMap::new();
    for id in graph.spec.nodes.keys() {
        questions.insert(
            format!("on_goal:{id}"),
            Question::Noul {
                instructions: format!(
                    "Does the node with id `{id}` (see `nodes`) do work that the `goal` needs? \
                     Judge the node's `objective` against the `goal`."
                ),
                criteria: Some(NoulCriteria {
                    r#true: "The goal cannot be met without this node's objective".to_owned(),
                    r#false: "The objective is unrelated to the goal, or duplicates another node"
                        .to_owned(),
                }),
            },
        );
        questions.insert(
            format!("kind:{id}"),
            Question::Choice {
                instructions: format!(
                    "Which node type from the options best executes the `objective` of the node \
                     with id `{id}`? `agent` reasons and writes; tool types run one program, \
                     repository read, or test call; pick by what the objective actually does."
                ),
                criteria: kinds.clone(),
            },
        );
    }
    JudgeRequest {
        state: serde_json::json!({ "goal": profile.goal, "nodes": nodes }),
        model: JEV_LATEST.to_owned(),
        questions,
    }
}

/// Reads the reply into diagnostics (repairable), one judgment per node, and the unresolved
/// ids. A missing or mistyped answer is treated as unresolved, never as a verdict; so is an
/// `on_goal` outside `[0.0, 1.0]` (or NaN), and a `kind` that is not one of `catalog.node_types`
/// (#1120 review findings): an answer the question did not offer is not a mismatch.
#[must_use]
pub fn read(
    reply: &JudgeReply,
    graph: &ExecutionGraph,
    catalog: &CapabilityCatalog,
) -> (Vec<Diagnostic>, Vec<NodeJudgment>, Vec<String>) {
    let mut diagnostics = Vec::new();
    let mut judgments = Vec::new();
    let mut unresolved = Vec::new();
    for (id, node) in &graph.spec.nodes {
        let on_goal = match reply.answers.get(&format!("on_goal:{id}")) {
            Some(Answer::Noul { noul }) => *noul,
            _ => f64::NAN,
        };
        let (kind, kind_confidence) = match reply.answers.get(&format!("kind:{id}")) {
            Some(Answer::Choice {
                choice, confidence, ..
            }) => (choice.clone(), *confidence),
            _ => (String::new(), f64::NAN),
        };
        let mut resolved = true;
        // A `noul` is a probability; a value outside `[0.0, 1.0]` (NaN included, which fails
        // both comparisons) is unresolved, never read as a verdict (#1120 review finding).
        let on_goal_in_range = (0.0..=1.0).contains(&on_goal);
        if !on_goal_in_range || (!noul_is_no(on_goal) && !noul_is_yes(on_goal)) {
            resolved = false;
        } else if noul_is_no(on_goal) {
            diagnostics.push(Diagnostic::error(
                NODE_OFF_GOAL_CODE,
                format!(
                    "node {id} does not serve the goal (judged {on_goal:.2}); remove it or give \
                     it an objective the goal needs"
                ),
                format!("/spec/nodes/{}/objective", escape(id)),
                DRAFT_SOURCE,
            ));
        }
        // A `choice` outside the catalog the question offered is unresolved, never a mismatch
        // (#1120 review finding): a mismatch is a catalog type that differs from the draft's.
        let kind_in_catalog = catalog.node_types.contains(&kind);
        if kind_confidence.is_nan() || !acts(kind_confidence) || !kind_in_catalog {
            resolved = false;
        } else if kind != node.node_type.as_str() {
            diagnostics.push(Diagnostic::error(
                NODE_KIND_MISMATCH_CODE,
                format!(
                    "node {id} is typed {} but its objective reads as {kind} (confidence \
                     {kind_confidence:.2})",
                    node.node_type.as_str()
                ),
                format!("/spec/nodes/{}/type", escape(id)),
                DRAFT_SOURCE,
            ));
        }
        if !resolved {
            unresolved.push(id.clone());
        }
        judgments.push(NodeJudgment {
            node: id.clone(),
            on_goal,
            kind,
            kind_confidence,
        });
    }
    (diagnostics, judgments, unresolved)
}
