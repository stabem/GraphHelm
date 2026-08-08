use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{GraphEdge, GraphNode, ManualOverride, SemanticHash};

/// A transactional graph mutation request based on an exact predecessor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDraft {
    pub id: String,
    pub expected_version: u64,
    pub expected_hash: SemanticHash,
    pub operations: Vec<DraftOperation>,
    #[serde(default)]
    pub manual_override: Option<ManualOverride>,
}

/// The deliberately small, typed graph mutation language.
#[derive(Clone, Debug, PartialEq)]
pub enum DraftOperation {
    AddNode {
        id: String,
        node: GraphNode,
    },
    RemoveNode {
        id: String,
    },
    PatchNode {
        id: String,
        patch: serde_json::Value,
    },
    AddEdge {
        edge: GraphEdge,
    },
    RemoveEdge {
        id: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum WireOperation {
    AddNode {
        path: String,
        value: GraphNode,
    },
    RemoveNode {
        path: String,
    },
    PatchNode {
        path: String,
        value: serde_json::Value,
    },
    AddEdge {
        value: GraphEdge,
    },
    RemoveEdge {
        path: String,
    },
}

impl Serialize for DraftOperation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match self {
            Self::AddNode { id, node } => WireOperation::AddNode {
                path: pointer("nodes", id),
                value: node.clone(),
            },
            Self::RemoveNode { id } => WireOperation::RemoveNode {
                path: pointer("nodes", id),
            },
            Self::PatchNode { id, patch } => WireOperation::PatchNode {
                path: patch
                    .as_object()
                    .and_then(|object| (object.len() == 1).then(|| object.iter().next().unwrap()))
                    .map_or_else(
                        || pointer("nodes", id),
                        |(field, _)| format!("{}/{}", pointer("nodes", id), escape(field)),
                    ),
                value: patch
                    .as_object()
                    .and_then(|object| (object.len() == 1).then(|| object.values().next().unwrap()))
                    .cloned()
                    .unwrap_or_else(|| patch.clone()),
            },
            Self::AddEdge { edge } => WireOperation::AddEdge {
                value: edge.clone(),
            },
            Self::RemoveEdge { id } => WireOperation::RemoveEdge {
                path: pointer("edges", id),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DraftOperation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WireOperation::deserialize(deserializer)?;
        match wire {
            WireOperation::AddNode { path, value } => Ok(Self::AddNode {
                id: parse_pointer(&path, "nodes").map_err(serde::de::Error::custom)?,
                node: value,
            }),
            WireOperation::RemoveNode { path } => Ok(Self::RemoveNode {
                id: parse_pointer(&path, "nodes").map_err(serde::de::Error::custom)?,
            }),
            WireOperation::PatchNode { path, value } => {
                let prefix = "/spec/nodes/";
                let tail = path
                    .strip_prefix(prefix)
                    .ok_or_else(|| serde::de::Error::custom("invalid node patch path"))?;
                let (encoded_id, field) = tail
                    .split_once('/')
                    .map_or((tail, None), |(id, field)| (id, Some(field)));
                if encoded_id.is_empty()
                    || field.is_some_and(|field| field.is_empty() || field.contains('/'))
                {
                    return Err(serde::de::Error::custom("node patch path is too deep"));
                }
                let id = unescape(encoded_id);
                let patch = field.map_or(value.clone(), |field| {
                    let mut object = serde_json::Map::new();
                    object.insert(unescape(field), value);
                    serde_json::Value::Object(object)
                });
                Ok(Self::PatchNode { id, patch })
            }
            WireOperation::AddEdge { value } => Ok(Self::AddEdge { edge: value }),
            WireOperation::RemoveEdge { path } => Ok(Self::RemoveEdge {
                id: parse_pointer(&path, "edges").map_err(serde::de::Error::custom)?,
            }),
        }
    }
}

fn pointer(collection: &str, id: &str) -> String {
    format!("/spec/{collection}/{}", escape(id))
}

fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn unescape(value: &str) -> String {
    value.replace("~1", "/").replace("~0", "~")
}

fn parse_pointer(path: &str, collection: &str) -> Result<String, &'static str> {
    let prefix = format!("/spec/{collection}/");
    let encoded = path
        .strip_prefix(&prefix)
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or("operation path must address exactly one graph member")?;
    Ok(unescape(encoded))
}
