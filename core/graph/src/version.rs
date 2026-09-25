use chrono::{DateTime, Utc};
use graphhelm_protocols::{
    Actor, ExecutionGraph, GraphVersionRecord, GraphVersionRef, SemanticHash,
};

use crate::{GraphError, canonicalize, semantic_hash};

/// An immutable, content-addressed published graph version.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphVersion {
    graph: ExecutionGraph,
    predecessor: Option<GraphVersionRef>,
    semantic: serde_json::Value,
    content_hash: SemanticHash,
    created_by: Actor,
    created_at: DateTime<Utc>,
}

impl GraphVersion {
    /// Publishes a new immutable value after checking predecessor monotonicity.
    pub fn publish(
        graph: ExecutionGraph,
        predecessor: Option<GraphVersionRef>,
        actor: Actor,
        created_at: DateTime<Utc>,
    ) -> Result<Self, GraphError> {
        if graph.metadata.version == 0 {
            return Err(GraphError::InvalidVersion);
        }
        if let Some(previous) = predecessor.as_ref() {
            let expected = previous
                .number
                .checked_add(1)
                .ok_or(GraphError::InvalidPredecessor)?;
            if graph.metadata.version != expected {
                return Err(GraphError::InvalidPredecessor);
            }
        }
        let canonical = canonicalize(&graph)?;
        let content_hash = semantic_hash(&graph)?;
        Ok(Self {
            graph,
            predecessor,
            semantic: canonical.value,
            content_hash,
            created_by: actor,
            created_at,
        })
    }

    #[must_use]
    pub const fn graph(&self) -> &ExecutionGraph {
        &self.graph
    }

    #[must_use]
    pub const fn number(&self) -> u64 {
        self.graph.metadata.version
    }

    #[must_use]
    pub const fn predecessor(&self) -> Option<&GraphVersionRef> {
        self.predecessor.as_ref()
    }

    #[must_use]
    pub const fn semantic(&self) -> &serde_json::Value {
        &self.semantic
    }

    #[must_use]
    pub const fn content_hash(&self) -> &SemanticHash {
        &self.content_hash
    }

    #[must_use]
    pub fn to_record(&self) -> GraphVersionRecord {
        GraphVersionRecord {
            graph: self.graph.clone(),
            predecessor: self.predecessor.clone(),
            semantic: self.semantic.clone(),
            content_hash: self.content_hash.clone(),
            created_by: self.created_by.clone(),
            created_at: self.created_at,
        }
    }

    pub fn from_record(record: GraphVersionRecord) -> Result<Self, GraphError> {
        let canonical = canonicalize(&record.graph)?;
        if canonical.value != record.semantic {
            return Err(GraphError::SemanticMismatch);
        }
        if semantic_hash(&record.graph)? != record.content_hash {
            return Err(GraphError::HashMismatch);
        }
        Self::publish(
            record.graph,
            record.predecessor,
            record.created_by,
            record.created_at,
        )
    }
}
