//! The assembled prompt's SHAPE. Task 1 declares it because the seam's `NodeWork` carries it;
//! Task 4 owns the assembly logic (determinism, completeness, refusals) and its tests. Note
//! the Task-1 discrepancy report: the plan's file list omitted this module while its own
//! `NodeWork` sketch references it — a stub-free Task 1 cannot compile.

use graphhelm_protocols::{GraphNode, NodeType};
use sha2::Digest as _;

use crate::executor::ExecutorRefusal;

/// A deterministically assembled prompt: fixed field order, digest computed at assembly so
/// Evidence and record agree on identity (Task 4 pins both properties).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssembledPrompt {
    pub system: String,
    pub task: String,
    /// The context capsule the node ran with (#1065) — the compiled capsule bytes as text, or
    /// empty when no capsule was shipped. A third length-prefixed field of the digest, so the
    /// record and the sealed evidence agree on what the model was shown. The digest of a prompt
    /// with an empty capsule is NOT the pre-#1065 digest of the same system and task: the field
    /// is always present, and its zero length is part of the identity.
    pub context: String,
    pub sha256: String,
}

/// The system-prompt fields, in the assembler's fixed order. Named lookups against the
/// `agent.ephemeral` contract — never iteration over `GraphNode.properties` — so field order
/// is this table's, not a map's.
const SYSTEM_FIELDS: [(&str, &str); 4] = [
    ("Purpose", "purpose"),
    ("Instructions", "instructions"),
    ("Input schema", "inputSchema"),
    ("Output schema", "outputSchema"),
];

/// Assemble the prompt a cognitive node's contract defines, or refuse. Only node types that
/// carry an `agent.ephemeral` contract assemble (the wire schema attaches it to cognitive
/// nodes); a Tool node has no prompt, and a missing objective or contract block is a refusal
/// — execution must not invent, never an empty prompt (the FixtureExecutor precedent).
pub fn assemble(node: &GraphNode) -> Result<AssembledPrompt, ExecutorRefusal> {
    assemble_with_context(node, "")
}

/// [`assemble`] with a context capsule as the third field. The capsule enters the digest
/// exactly like the other two fields — length-prefixed, after the task — so two prompts that
/// differ only in what they were shown differ in identity.
pub fn assemble_with_context(
    node: &GraphNode,
    context: &str,
) -> Result<AssembledPrompt, ExecutorRefusal> {
    let cognitive = matches!(
        node.node_type,
        NodeType::Agent | NodeType::Planner | NodeType::Classifier | NodeType::Evaluator
    );
    if !cognitive || node.objective.trim().is_empty() {
        return Err(ExecutorRefusal::Unassemblable);
    }
    let ephemeral = node
        .properties
        .get("agent")
        .and_then(|agent| agent.get("ephemeral"))
        .ok_or(ExecutorRefusal::Unassemblable)?;

    let mut system = String::new();
    for (label, key) in SYSTEM_FIELDS {
        if let Some(value) = ephemeral.get(key).and_then(|value| value.as_str()) {
            system.push_str(label);
            system.push_str(": ");
            system.push_str(value);
            system.push('\n');
        }
    }
    let task = node.objective.trim().to_owned();
    let context = context.to_owned();
    let sha256 = digest_fields(&system, &task, &context);

    Ok(AssembledPrompt {
        system,
        task,
        context,
        sha256,
    })
}

/// Length-prefixed concatenation of the three fields: unambiguous under any split, so the
/// digest is a function of the content alone.
fn digest_fields(system: &str, task: &str, context: &str) -> String {
    let mut identity = Vec::new();
    for field in [system, task, context] {
        identity.extend_from_slice(&(field.len() as u64).to_be_bytes());
        identity.extend_from_slice(field.as_bytes());
    }
    hex::encode(sha2::Sha256::digest(&identity))
}

/// The placeholder a `Tool`-kind work unit carries: tool work has no prompt — the executor's
/// tool path never reads it — but `NodeWork` requires the field, so the digest is honestly
/// computed over the empty content rather than left blank.
#[must_use]
pub fn tool_placeholder() -> AssembledPrompt {
    AssembledPrompt {
        system: String::new(),
        task: String::new(),
        context: String::new(),
        sha256: digest_fields("", "", ""),
    }
}
