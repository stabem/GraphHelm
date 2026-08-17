//! Which `NodeType`s the real executor runs, and the typed refusal for everything else. A
//! refusal is a refusal — never laundered into an outcome the fold would record: the driver
//! simply does not dispatch what the executor refuses (`core/protocols/src/graph.rs:82` is
//! the closed authoring vocabulary this classification covers exhaustively).

/// The three kinds of real work the runtime executes. Cognitive work is a model call and
/// carries no workspace (Tier 0); Tool work goes through the 05c broker; GateCheck work
/// (M06) is a deterministic evaluation with NO model port at all — a genuinely different
/// transport, which is why it earns a kind while the blind judge does not (the judge is a
/// model call whose blindness is an input discipline, enforced where inputs are
/// assembled — the M06 FIX-1 decision).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeWorkKind {
    Cognitive,
    Tool,
    GateCheck,
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
        NodeType::Gate => Ok(NodeWorkKind::GateCheck),
        NodeType::Fork
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
