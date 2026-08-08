use graphhelm_graph::{GraphVersion, lint};
use graphhelm_policy::evaluate_transition;
use graphhelm_protocols::{Diagnostic, DraftOperation, ExecutionGraph, GraphDraft, PolicyReport};

/// Pure draft analysis result before durable publication.
#[derive(Clone, Debug)]
pub struct DraftAnalysis {
    pub candidate: Option<ExecutionGraph>,
    pub diagnostics: Vec<Diagnostic>,
    pub policy_report: Option<PolicyReport>,
}

/// Applies typed operations to an isolated clone and evaluates it without persistence.
#[must_use]
pub fn analyze_draft(base: &GraphVersion, draft: &GraphDraft) -> DraftAnalysis {
    let mut candidate = base.graph().clone();
    let mut diagnostics = Vec::new();
    if let Err(message) = apply_operations(&mut candidate, &draft.operations) {
        diagnostics.push(Diagnostic::error(
            "GHD003_OPERATION_INVALID",
            message,
            "/operations",
            draft.id.clone(),
        ));
        return DraftAnalysis {
            candidate: None,
            diagnostics,
            policy_report: None,
        };
    }
    candidate.metadata.version = base.number() + 1;
    candidate.metadata.based_on = Some(base.graph().metadata.id.clone());
    let report = lint(&candidate, &draft.id);
    diagnostics.extend(report.errors);
    let policy_report = evaluate_transition(base, &candidate, draft.manual_override.as_ref());
    DraftAnalysis {
        candidate: Some(candidate),
        diagnostics,
        policy_report: Some(policy_report),
    }
}

pub(super) fn apply_operations(
    graph: &mut ExecutionGraph,
    operations: &[DraftOperation],
) -> Result<(), String> {
    for operation in operations {
        match operation {
            DraftOperation::AddNode { id, node } => {
                if graph.spec.nodes.contains_key(id) {
                    return Err(format!("node {id} already exists"));
                }
                graph.spec.nodes.insert(id.clone(), node.clone());
            }
            DraftOperation::RemoveNode { id } => {
                if graph.spec.nodes.remove(id).is_none() {
                    return Err(format!("node {id} does not exist"));
                }
            }
            DraftOperation::PatchNode { id, patch } => {
                let node = graph
                    .spec
                    .nodes
                    .get(id)
                    .ok_or_else(|| format!("node {id} does not exist"))?;
                let patch = patch
                    .as_object()
                    .ok_or_else(|| "node patch must be an object".to_string())?;
                let mut value = serde_json::to_value(node).map_err(|error| error.to_string())?;
                let object = value
                    .as_object_mut()
                    .ok_or_else(|| "node cannot be patched".to_string())?;
                for (key, value) in patch {
                    object.insert(key.clone(), value.clone());
                }
                let patched = serde_json::from_value(value)
                    .map_err(|error| format!("invalid node patch: {error}"))?;
                graph.spec.nodes.insert(id.clone(), patched);
            }
            DraftOperation::AddEdge { edge } => {
                if graph.spec.edges.iter().any(|item| item.id == edge.id) {
                    return Err(format!("edge {} already exists", edge.id));
                }
                graph.spec.edges.push(edge.clone());
            }
            DraftOperation::RemoveEdge { id } => {
                let before = graph.spec.edges.len();
                graph.spec.edges.retain(|edge| edge.id != *id);
                if graph.spec.edges.len() == before {
                    return Err(format!("edge {id} does not exist"));
                }
            }
        }
    }
    Ok(())
}
