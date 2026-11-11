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

type Pair = (&'static str, Option<&'static str>);

const PLAN: Pair = ("plan", Some("task-plan"));
const CRITIC: Pair = ("critic", None);
const IMPLEMENT: Pair = ("implement", Some("implement"));
const REVIEW: Pair = ("review", Some("blind-review"));
const MERGE: Pair = ("merge", Some("merge"));
const DONE: Pair = ("done", None);

/// Every `(step, skill)` pair [`next_step`] can emit, so a test can check each skill exists and
/// agrees with the guide's State table.
pub const NEXT_TABLE: &[Pair] = &[PLAN, CRITIC, IMPLEMENT, REVIEW, MERGE, DONE];

fn answer((step, skill): Pair, actor: Option<NextActor>, because: String) -> NextStep {
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
    if task.step != TaskStep::Merged
        && let Some(who) = &task.released_by
    {
        return answer(DONE, None, format!("released by {who}"));
    }
    match task.step {
        TaskStep::Merged => answer(DONE, None, "merged".to_owned()),
        TaskStep::Plan => answer(PLAN, author(task), "claimed, no plan recorded".to_owned()),
        TaskStep::Critic => answer(CRITIC, None, "design plan waits for its critic".to_owned()),
        TaskStep::Implement => answer(
            IMPLEMENT,
            author(task),
            "planned, no PR open on a head".to_owned(),
        ),
        TaskStep::Review => match &task.blocked_by {
            Some(blocker) => answer(
                IMPLEMENT,
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
                answer(REVIEW, reviewer, because.to_owned())
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
            answer(MERGE, actor, because.to_owned())
        }
    }
}
