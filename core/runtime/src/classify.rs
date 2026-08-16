//! Which `NodeType`s the real executor runs, and the typed refusal for everything else. A
//! refusal is a refusal — never laundered into an outcome the fold would record: the driver
//! simply does not dispatch what the executor refuses (`core/protocols/src/graph.rs:82` is
//! the closed authoring vocabulary this classification covers exhaustively).

/// The two kinds of real work this milestone executes. Cognitive work is a model call and
/// carries no workspace (Tier 0); Tool work goes through the 05c broker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeWorkKind {
    Cognitive,
    Tool,
}

use graphhelm_protocols::NodeType;

use crate::executor::ExecutorRefusal;

/// Classify one node type, exhaustively — no wildcard arm, so a new authoring node type must
/// decide its execution story here or the crate does not compile.
///
/// # Errors
/// [`ExecutorRefusal::Unsupported`] for every type this milestone does not execute.
pub fn work_kind(node_type: &NodeType) -> Result<NodeWorkKind, ExecutorRefusal> {
    match node_type {
        NodeType::Agent | NodeType::Planner | NodeType::Classifier | NodeType::Evaluator => {
            Ok(NodeWorkKind::Cognitive)
        }
        NodeType::Tool => Ok(NodeWorkKind::Tool),
        NodeType::Gate
        | NodeType::Fork
        | NodeType::Join
        | NodeType::HumanDecision
        | NodeType::Timer
        | NodeType::Trigger
        | NodeType::Subgraph
        | NodeType::Materializer
        | NodeType::Deploy
        | NodeType::Rollback
        | NodeType::ArtifactTransform => Err(ExecutorRefusal::Unsupported),
    }
}
