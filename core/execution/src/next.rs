//! #2829: what a task does next, read off its folded state. The entry skill `graphhelm-work` asks
//! the `tasks` reply for this answer, loads the skill it names and does that step.
//!
//! One pure function over a [`TaskState`]: no I/O, no clock, no network and no model. The answer
//! uses the Runtime's own step names (`plan`, `critic`, `implement`, `review`, `merge`) plus `done`.
//! It never says `claim`, `cleanup` or `diagnose`: no task record says when a cleanup finished or
//! that no unblocker resolves a block, so those cannot be derived from records. `because` is a
//! fixed phrase plus at most one name, never a path, a URL or text copied from a record.

use crate::tasks::{TaskState, TaskStep};
use serde::Serialize;

/// Who acts on the next step, and in what role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NextActor {
    pub id: String,
    /// `author`, `reviewer` or `approver`.
    pub role: &'static str,
}

/// The `next` field of a task in the `tasks` reply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NextStep {
    /// One of the steps in [`NEXT_TABLE`].
    pub step: &'static str,
    /// The skill to load for the step, or `None` when no skill serves it (a `critic` wait, `done`).
    pub skill: Option<&'static str>,
    /// Who acts; `None` when nobody can be named from the records.
    pub actor: Option<NextActor>,
    pub because: String,
}

/// Every `(step, skill)` pair [`next_step`] can emit, so a test can check each skill exists and
/// agrees with the guide's State table.
pub const NEXT_TABLE: &[(&str, Option<&str>)] = &[
    ("plan", Some("task-plan")),
    ("critic", None),
    ("implement", Some("implement")),
    ("review", Some("blind-review")),
    ("merge", Some("merge")),
    ("done", None),
];

fn pair(step: &'static str) -> (&'static str, Option<&'static str>) {
    *NEXT_TABLE
        .iter()
        .find(|(name, _)| *name == step)
        .expect("every step next_step emits is in NEXT_TABLE")
}

fn answer(step: &'static str, actor: Option<NextActor>, because: String) -> NextStep {
    let (step, skill) = pair(step);
    NextStep {
        step,
        skill,
        actor,
        because,
    }
}

fn author(task: &TaskState) -> Option<NextActor> {
    task.lane.as_ref().map(|lane| NextActor {
        id: lane.clone(),
        role: "author",
    })
}

/// What `task` does next; the first rule that matches wins.
#[must_use]
pub fn next_step(task: &TaskState) -> NextStep {
    if task.step == TaskStep::Merged {
        return answer("done", None, "merged".to_owned());
    }
    if let Some(who) = &task.released_by {
        return answer("done", None, format!("released by {who}"));
    }
    match task.step {
        TaskStep::Merged => unreachable!("handled above"),
        TaskStep::Plan => answer("plan", author(task), "claimed, no plan recorded".to_owned()),
        TaskStep::Critic => answer(
            "critic",
            None,
            "design plan waits for its critic".to_owned(),
        ),
        TaskStep::Implement => answer(
            "implement",
            author(task),
            "planned, no PR open on a head".to_owned(),
        ),
        TaskStep::Review => match &task.blocked_by {
            Some(blocker) => answer(
                "implement",
                author(task),
                format!("BLOCK on head by {}", blocker.reviewer),
            ),
            None => {
                let reviewer = task.current_reviewer.as_ref().map(|id| NextActor {
                    id: id.clone(),
                    role: "reviewer",
                });
                let because = if reviewer.is_some() {
                    "PR open on head, no verdict"
                } else {
                    "PR open on head, no reviewer assigned"
                };
                answer("review", reviewer, because.to_owned())
            }
        },
        TaskStep::Merge => {
            // The fold keeps the assignment on the head after an approval; a risk approval names
            // its own reviewer.
            let (approver, because) = match &task.approved_with_risk {
                Some(risk) => (Some(risk.reviewer.clone()), "approved with risk on head"),
                None => (task.current_reviewer.clone(), "approved on head"),
            };
            let actor = approver.map(|id| NextActor {
                id,
                role: "approver",
            });
            answer("merge", actor, because.to_owned())
        }
    }
}
