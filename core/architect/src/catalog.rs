//! The capability catalog (spec D3): what the model may build with, derived from what the runtime
//! executes and what the operator allowed. Nothing here is declared by hand — a node type is in
//! the catalog because `classify::work_kind` accepts it, and a program is in it because the
//! operator passed it — so the catalog cannot promise a capability the executor would refuse.
//! The population walked is `NodeType::EVERY_VARIANT`, the protocols crate's own exhaustive
//! list, so a variant added to the enum reaches the catalog decision without a second list here
//! to forget.

use std::collections::BTreeSet;

use graphhelm_protocols::NodeType;

/// The three builtin tool families the broker dispatches (`graphhelm_tool_broker::call::ToolCall`),
/// by their wire tags, sorted.
pub const TOOL_FAMILIES: [&str; 3] = ["repository", "shell", "tests"];

/// What a synthesized graph may be made of.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCatalog {
    /// Wire names of the node types the executor runs, sorted.
    pub node_types: Vec<String>,
    /// The builtin tool families, sorted.
    pub tool_families: Vec<&'static str>,
    /// The operator's program allowlist, sorted and deduplicated. Empty when the operator allowed
    /// nothing: the architect never invents a default program.
    pub programs: Vec<String>,
}

impl CapabilityCatalog {
    /// Derives the catalog from the runtime's classification and the operator's allowlist.
    #[must_use]
    pub fn from_runtime(programs: &[String]) -> Self {
        let mut node_types: Vec<String> = NodeType::EVERY_VARIANT
            .iter()
            .filter(|node_type| graphhelm_runtime::classify::work_kind(node_type).is_ok())
            .map(|node_type| node_type.as_str().to_owned())
            .collect();
        node_types.sort();
        let programs: BTreeSet<String> = programs.iter().cloned().collect();
        Self {
            node_types,
            tool_families: TOOL_FAMILIES.to_vec(),
            programs: programs.into_iter().collect(),
        }
    }

    /// Whether a shell call may name `program`.
    #[must_use]
    pub fn allows_program(&self, program: &str) -> bool {
        self.programs.iter().any(|allowed| allowed == program)
    }

    /// Whether a node may carry `wire_name` as its `type`.
    #[must_use]
    pub fn allows_node_type(&self, wire_name: &str) -> bool {
        self.node_types.iter().any(|allowed| allowed == wire_name)
    }
}
