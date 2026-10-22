//! #1797 (Studio v2 R6): the file-collision read model. A path is a collision when two or more
//! distinct open task slices name it in their `task.planned` paths (their Keel card's scope).
//! The input is only what the lanes recorded; the Runtime never asks GitHub for a PR's files.

use std::collections::BTreeMap;

use graphhelm_execution::tasks::{TaskState, TaskStep};
use serde::Serialize;

use super::tasks::TasksView;

/// The most colliding paths one reply lists; the rest is reported by `truncated`.
pub const MAX_COLLISIONS: usize = 200;

/// `GET /v1/executions/{id}/collisions`: every path two or more open tasks claim, by path.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCollisions {
    pub execution_id: String,
    pub head_sequence: u64,
    pub collisions: Vec<Collision>,
    /// More colliding paths exist than [`MAX_COLLISIONS`]; the first ones by path are listed.
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collision {
    pub path: String,
    /// The open tasks naming `path`, by slice key.
    pub holders: Vec<Holder>,
}

/// One open task slice that names the path: who holds it and where its work is.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Holder {
    pub key: String,
    pub task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub step: &'static str,
}

/// Open: a live slice (not closed) that is neither merged nor released.
fn open(task: &TaskState) -> bool {
    task.step != TaskStep::Merged && task.released_by.is_none() && !task.orphaned
}

impl TasksView {
    /// #1797: the file collisions among this view's open tasks (see [`FileCollisions`]).
    #[must_use]
    pub fn file_collisions(&self) -> serde_json::Value {
        let mut by_path: BTreeMap<&str, BTreeMap<&str, Holder>> = BTreeMap::new();
        for task in self.tasks.iter().filter(|task| open(task)) {
            for path in &task.paths {
                by_path.entry(path).or_default().insert(
                    &task.key,
                    Holder {
                        key: task.key.clone(),
                        task_id: task.task_id.clone(),
                        issue: task.issue,
                        pr: task.pr,
                        lane: task.lane.clone(),
                        branch: task.branch.clone(),
                        step: task.step.as_str(),
                    },
                );
            }
        }
        let mut colliding = by_path
            .into_iter()
            .filter(|(_, holders)| holders.len() > 1)
            .map(|(path, holders)| Collision {
                path: path.to_owned(),
                holders: holders.into_values().collect(),
            });
        let collisions: Vec<Collision> = colliding.by_ref().take(MAX_COLLISIONS).collect();
        let truncated = colliding.next().is_some();
        serde_json::to_value(FileCollisions {
            execution_id: self.execution_id.clone(),
            head_sequence: self.head_sequence,
            collisions,
            truncated,
        })
        .expect("a collision list of plain fields")
    }
}
