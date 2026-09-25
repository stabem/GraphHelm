//! The caller-supplied task profile (spec D4). Minimal by decision: the PRD's task profiler is
//! not built by this slice, so every field here is what a caller states, bounded by `validate`.

use crate::refusal::ArchitectRefusal;

/// The node ceiling a profile carries when the caller states none.
pub const DEFAULT_MAX_NODES: usize = 6;
/// The largest node ceiling a profile may state. Bounded before any prompt is assembled, so a
/// caller cannot ask the model for a graph the linter would spend unbounded work on.
pub const MAX_MAX_NODES: usize = 50;
/// How long an un-claimed customs wait may park, when the caller states nothing: one day.
pub const DEFAULT_WAIT_WITHIN_SECONDS: u64 = 86_400;
/// How long a claim may await clearance, when the caller states nothing: one hour.
pub const DEFAULT_CLEARANCE_WITHIN_SECONDS: u64 = 3_600;
/// The largest goal accepted, in bytes. The goal is embedded in the prompt verbatim; the bound
/// keeps the prompt — and the sha256 the fixture is keyed by — a function of a small input.
pub const MAX_GOAL_BYTES: usize = 4 * 1024;
/// The customs schema's ceiling on a budget, in seconds (`node.schema.json` `maximum`). A budget
/// above it would be stamped onto every node and refused by the schema on every round, so the
/// profile refuses it first and names the field.
const MAX_BUDGET_SECONDS: u64 = 315_576_000;

/// The execution modes a profile may name. The mode reaches the prompt; nothing in this slice
/// branches on it, and a mode outside this list is refused rather than passed through, so the
/// vocabulary stays the one the runtime already speaks.
pub const MODES: [&str; 3] = ["autopilot", "supervised", "manual"];

/// What the caller asks the architect to compile. Deserialized with `deny_unknown_fields` so a
/// misspelled budget is refused rather than silently defaulted — the same rule `NodeCustoms`
/// applies for the same reason.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskProfile {
    pub goal: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_max_nodes")]
    pub max_nodes: usize,
    #[serde(default = "default_wait")]
    pub wait_within_seconds: u64,
    #[serde(default = "default_clearance")]
    pub clearance_within_seconds: u64,
}

fn default_mode() -> String {
    MODES[1].to_owned()
}

fn default_max_nodes() -> usize {
    DEFAULT_MAX_NODES
}

fn default_wait() -> u64 {
    DEFAULT_WAIT_WITHIN_SECONDS
}

fn default_clearance() -> u64 {
    DEFAULT_CLEARANCE_WITHIN_SECONDS
}

impl TaskProfile {
    /// A profile for `goal` with every other field at its default.
    #[must_use]
    pub fn new(goal: &str) -> Self {
        Self {
            goal: goal.to_owned(),
            mode: default_mode(),
            max_nodes: default_max_nodes(),
            wait_within_seconds: default_wait(),
            clearance_within_seconds: default_clearance(),
        }
    }

    /// Refuses a profile the compiler could not honour, naming the field as a JSON pointer.
    ///
    /// # Errors
    /// [`ArchitectRefusal::InvalidProfile`] when the goal is empty or over [`MAX_GOAL_BYTES`],
    /// the node ceiling is outside `1..=MAX_MAX_NODES`, a budget is zero or above the customs
    /// schema's ceiling, or the mode is not one of [`MODES`].
    pub fn validate(&self) -> Result<(), ArchitectRefusal> {
        let refuse = |pointer: &str, message: String| ArchitectRefusal::InvalidProfile {
            pointer: pointer.to_owned(),
            message,
        };
        if self.goal.trim().is_empty() {
            return Err(refuse("/goal", "the goal is empty".to_owned()));
        }
        if self.goal.len() > MAX_GOAL_BYTES {
            return Err(refuse(
                "/goal",
                format!(
                    "the goal is {} bytes; at most {MAX_GOAL_BYTES} are accepted",
                    self.goal.len()
                ),
            ));
        }
        if !MODES.contains(&self.mode.as_str()) {
            return Err(refuse(
                "/mode",
                format!("mode must be one of {}", MODES.join(", ")),
            ));
        }
        if self.max_nodes == 0 || self.max_nodes > MAX_MAX_NODES {
            return Err(refuse(
                "/maxNodes",
                format!("maxNodes must be within 1..={MAX_MAX_NODES}"),
            ));
        }
        for (pointer, seconds) in [
            ("/waitWithinSeconds", self.wait_within_seconds),
            ("/clearanceWithinSeconds", self.clearance_within_seconds),
        ] {
            if seconds == 0 || seconds > MAX_BUDGET_SECONDS {
                return Err(refuse(
                    pointer,
                    format!("a customs budget must be within 1..={MAX_BUDGET_SECONDS} seconds"),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_profile_carries_the_documented_defaults_and_validates() {
        let profile = TaskProfile::new("check that the repository builds");
        assert_eq!(profile.mode, "supervised");
        assert_eq!(profile.max_nodes, DEFAULT_MAX_NODES);
        assert_eq!(profile.wait_within_seconds, DEFAULT_WAIT_WITHIN_SECONDS);
        assert_eq!(
            profile.clearance_within_seconds,
            DEFAULT_CLEARANCE_WITHIN_SECONDS
        );
        assert_eq!(profile.validate(), Ok(()));
    }

    #[test]
    fn every_bound_is_refused_at_its_own_pointer() {
        let base = TaskProfile::new("goal");
        let cases: Vec<(TaskProfile, &str)> = vec![
            (
                TaskProfile {
                    goal: "   ".to_owned(),
                    ..base.clone()
                },
                "/goal",
            ),
            (
                TaskProfile {
                    goal: "g".repeat(MAX_GOAL_BYTES + 1),
                    ..base.clone()
                },
                "/goal",
            ),
            (
                TaskProfile {
                    mode: "yolo".to_owned(),
                    ..base.clone()
                },
                "/mode",
            ),
            (
                TaskProfile {
                    max_nodes: 0,
                    ..base.clone()
                },
                "/maxNodes",
            ),
            (
                TaskProfile {
                    max_nodes: MAX_MAX_NODES + 1,
                    ..base.clone()
                },
                "/maxNodes",
            ),
            (
                TaskProfile {
                    wait_within_seconds: 0,
                    ..base.clone()
                },
                "/waitWithinSeconds",
            ),
            (
                TaskProfile {
                    clearance_within_seconds: MAX_BUDGET_SECONDS + 1,
                    ..base.clone()
                },
                "/clearanceWithinSeconds",
            ),
        ];
        for (profile, expected) in cases {
            match profile.validate() {
                Err(ArchitectRefusal::InvalidProfile { pointer, .. }) => {
                    assert_eq!(pointer, expected);
                }
                other => panic!("{expected}: {other:?}"),
            }
        }
        assert_eq!(
            TaskProfile {
                goal: "g".repeat(MAX_GOAL_BYTES),
                max_nodes: MAX_MAX_NODES,
                ..base
            }
            .validate(),
            Ok(()),
            "the bounds are inclusive"
        );
    }

    #[test]
    fn json_defaults_fill_and_unknown_fields_are_refused() {
        let profile: TaskProfile = serde_json::from_str(r#"{"goal":"g"}"#).unwrap();
        assert_eq!(profile, TaskProfile::new("g"));
        let typo = serde_json::from_str::<TaskProfile>(r#"{"goal":"g","maxNode":3}"#);
        assert!(
            typo.is_err(),
            "a misspelled field must not default silently"
        );
    }
}
