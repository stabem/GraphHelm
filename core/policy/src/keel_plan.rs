//! The task planner (#382 phase B, spec `docs/specs/2026-10-07-journey-first-keel-design.md` §6).
//!
//! Deterministic first: each path the task names is classified, in order and stopping at the first
//! match, as an invariant class (`keel.yaml` `invariants`), user-visible (a compiled journey's
//! screen `scopePaths`), code, or docs. The task takes its highest class; reviews, proof, skills
//! and delegation follow from it through `keel.yaml` `plan`. The same paths, promise and policy
//! always give the same record. When the rules are ambiguous (the promise names an invariant its
//! paths do not touch) the planner takes the stricter class and records `fallback_strict`; asking
//! Jev on ambiguity is a later step of the spec and is not done here.
use serde::{Deserialize, Serialize};

use crate::keel::{KeelPolicy, paths_touch};

/// The record's schema id, `graphhelm-task-plan-v1`.
pub const PLAN_SCHEMA: &str = "graphhelm-task-plan-v1";

/// `keel.yaml` `plan` (1.5.0, critic 1.6.0): per-class review counts, delegation, the Jev
/// threshold, and which classes get a design critic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRules {
    pub reviews: ClassMap<u32>,
    pub delegation: ClassMap<[String; 2]>,
    /// Probability, in percent, a Jev answer must reach to be taken (spec: 0.8).
    pub jev_threshold_percent: u32,
    /// #467: whether a class gets a graded design critic before implementation. A policy
    /// without the field gives none to every class.
    #[serde(default)]
    pub critic: ClassMap<CriticMode>,
    /// #467: the critic's pass score and round budget.
    #[serde(default)]
    pub critic_loop: CriticLoop,
}

/// Whether a task's design is graded by a critic before implementation (#467).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticMode {
    #[default]
    None,
    Design,
}

/// The critic's bounds (#467): a design passes at `pass_score` out of 10 or better within
/// `max_rounds`; out of rounds, it goes to a person, never an automatic pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CriticLoop {
    pub pass_score: u32,
    pub max_rounds: u32,
}

impl Default for CriticLoop {
    fn default() -> Self {
        Self {
            pass_score: 8,
            max_rounds: 3,
        }
    }
}

/// The plan's critic (#467): the mode its class gets and, for `design`, the bounds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Critic {
    pub mode: CriticMode,
    pub pass_score: u32,
    pub max_rounds: u32,
}

/// One value per task class.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ClassMap<T> {
    pub docs: T,
    pub code: T,
    pub user_visible: T,
    pub invariant: T,
}

impl<T> ClassMap<T> {
    fn get(&self, class: TaskClass) -> &T {
        match class {
            TaskClass::Docs => &self.docs,
            TaskClass::Code => &self.code,
            TaskClass::UserVisible => &self.user_visible,
            TaskClass::Invariant => &self.invariant,
        }
    }
}

/// A task class, lowest first: the task takes the highest of its paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskClass {
    Docs,
    Code,
    UserVisible,
    Invariant,
}

/// What the planner reads besides the policy.
#[derive(Clone, Debug, Default)]
pub struct PlanInput {
    pub task_id: String,
    pub revision: String,
    pub paths: Vec<String>,
    pub promise: String,
    /// Every compiled journey's screens: contract id and the scope paths of each screen.
    pub journeys: Vec<(String, Vec<String>)>,
}

/// The `keel.plan` record (`graphhelm-task-plan-v1`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPlan {
    pub schema: String,
    pub task_id: String,
    pub revision: String,
    pub paths: Vec<String>,
    pub classes: Vec<TaskClass>,
    pub invariant_classes: Vec<String>,
    pub journeys: Vec<String>,
    pub proof: String,
    pub reviews: u32,
    pub skills: Vec<String>,
    pub tools: Vec<String>,
    pub delegation: Delegation,
    pub path: Vec<String>,
    pub decided_by: String,
    pub jev: Option<serde_json::Value>,
    /// #467. Absent on records made before the critic existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critic: Option<Critic>,
}

/// The delegation the class asks for (`core/protocols/src/delegation.rs` names).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delegation {
    pub kind: String,
    pub tier: String,
    pub effort: String,
}

/// Source and script files count as code; anything else outside an invariant or a screen is docs.
fn is_code(path: &str) -> bool {
    const CODE: [&str; 12] = [
        ".rs", ".ts", ".tsx", ".js", ".mjs", ".cjs", ".py", ".ps1", ".sh", ".toml", ".lock", ".css",
    ];
    CODE.iter().any(|ext| path.ends_with(ext))
}

/// Plainly prose (review of #405): only these are docs. Every other path that is no invariant, no
/// screen and no source file (a policy like `keel.yaml`, a schema, a fixture, an unknown file) is
/// not plainly docs, so the plan is ambiguous and takes the stricter class (spec §6).
pub fn is_prose(path: &str) -> bool {
    const PROSE: [&str; 4] = [".md", ".txt", ".rst", ".adoc"];
    PROSE
        .iter()
        .any(|ext| path.to_ascii_lowercase().ends_with(ext))
}

/// Word stems that name an invariant class in a promise; a promise word must START with one
/// (`race` matches `race`/`races`, never `trace`). A promise that uses one while its paths touch
/// no invariant is ambiguous, and the planner takes the stricter answer.
const INVARIANT_WORDS: [&str; 10] = [
    "permission",
    "security",
    "token",
    "credential",
    "persist",
    "journal",
    "concurren",
    "race",
    "delete",
    "destructive",
];

/// Plan one task. Pure: the same input and policy always produce the same record.
#[must_use]
pub fn plan(input: &PlanInput, policy: &KeelPolicy, rules: &PlanRules) -> TaskPlan {
    let mut paths = input.paths.clone();
    paths.sort();
    paths.dedup();
    let mut classes = Vec::new();
    let mut invariant_classes = Vec::new();
    let mut journeys = Vec::new();
    for path in &paths {
        let invariants: Vec<&String> = policy
            .invariants
            .iter()
            .filter(|(_, roots)| roots.iter().any(|root| paths_touch(path, root)))
            .map(|(class, _)| class)
            .collect();
        let touched: Vec<&String> = input
            .journeys
            .iter()
            .filter(|(_, scopes)| scopes.iter().any(|scope| paths_touch(path, scope)))
            .map(|(id, _)| id)
            .collect();
        let class = if !invariants.is_empty() {
            TaskClass::Invariant
        } else if !touched.is_empty() {
            TaskClass::UserVisible
        } else if is_code(path) {
            TaskClass::Code
        } else {
            TaskClass::Docs
        };
        classes.push(class);
        invariant_classes.extend(invariants.into_iter().cloned());
        // A path is user-visible in addition to invariant-bearing when it touches a screen too.
        journeys.extend(touched.into_iter().cloned());
        if class == TaskClass::Invariant
            && input
                .journeys
                .iter()
                .any(|(_, scopes)| scopes.iter().any(|scope| paths_touch(path, scope)))
        {
            classes.push(TaskClass::UserVisible);
        }
    }
    classes.sort();
    classes.dedup();
    invariant_classes.sort();
    invariant_classes.dedup();
    journeys.sort();
    journeys.dedup();

    let promise = input.promise.to_lowercase();
    let words: Vec<&str> = promise
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let names_invariant = INVARIANT_WORDS
        .iter()
        .any(|stem| words.iter().any(|word| word.starts_with(stem)));
    let unclassified = paths.iter().any(|path| {
        !is_prose(path)
            && !is_code(path)
            && !policy
                .invariants
                .values()
                .flatten()
                .any(|root| paths_touch(path, root))
            && !input
                .journeys
                .iter()
                .any(|(_, scopes)| scopes.iter().any(|scope| paths_touch(path, scope)))
    });
    let ambiguous = !classes.contains(&TaskClass::Invariant) && (names_invariant || unclassified);
    let mut task = classes.last().copied().unwrap_or(TaskClass::Docs);
    if ambiguous {
        task = TaskClass::Invariant;
    }
    let base = TaskPlan {
        schema: PLAN_SCHEMA.to_owned(),
        task_id: input.task_id.clone(),
        revision: input.revision.clone(),
        paths,
        classes,
        invariant_classes,
        journeys,
        proof: String::new(),
        reviews: 0,
        skills: Vec::new(),
        tools: Vec::new(),
        delegation: Delegation {
            kind: String::new(),
            tier: String::new(),
            effort: String::new(),
        },
        path: Vec::new(),
        decided_by: String::new(),
        jev: None,
        critic: None,
    };
    decide(
        &base,
        task,
        if ambiguous {
            "fallback_strict"
        } else {
            "rules"
        },
        None,
        rules,
    )
}

/// The class the paths alone give: the highest class of any path (never lowered by a judge).
#[must_use]
pub fn rules_class(plan: &TaskPlan) -> TaskClass {
    plan.classes.last().copied().unwrap_or(TaskClass::Docs)
}

/// Re-derive a plan's proof, reviews, skills, tools, delegation and route for `task`, keeping its
/// paths, classes and journeys. Used by the rules and, on ambiguity, by a Jev answer (#382 B2).
#[must_use]
pub fn decide(
    plan: &TaskPlan,
    task: TaskClass,
    decided_by: &str,
    jev: Option<serde_json::Value>,
    rules: &PlanRules,
) -> TaskPlan {
    let journeys = &plan.journeys;
    let user_visible = plan.classes.contains(&TaskClass::UserVisible);
    let proof = match (task, user_visible) {
        (TaskClass::Docs, _) => "none",
        (TaskClass::Invariant, true) => "both",
        (TaskClass::Invariant | TaskClass::Code, _) => "tests",
        (TaskClass::UserVisible, _) => "journey",
    };
    let mut skills = Vec::new();
    if task != TaskClass::Docs {
        skills.push("keel".to_owned());
    }
    if matches!(proof, "journey" | "both") {
        skills.push("journey-contract".to_owned());
    }
    if matches!(proof, "tests" | "both") {
        skills.push("test-audit".to_owned());
    }
    let tools = if matches!(proof, "journey" | "both") {
        vec!["observer:journey_driver".to_owned()]
    } else {
        Vec::new()
    };
    let reviews = *rules.reviews.get(task);
    let [tier, effort] = rules.delegation.get(task).clone();
    let critic = Critic {
        mode: *rules.critic.get(task),
        pass_score: rules.critic_loop.pass_score,
        max_rounds: rules.critic_loop.max_rounds,
    };
    let mut route = vec!["card".to_owned()];
    if critic.mode == CriticMode::Design {
        route.push("design".to_owned());
        route.push(format!(
            "critic: blind, pass at {}/10 within {} rounds",
            critic.pass_score, critic.max_rounds
        ));
    }
    route.push("change".to_owned());
    if matches!(proof, "journey" | "both") {
        route.extend(
            journeys
                .iter()
                .map(|id| format!("graphhelm journey replay {id}")),
        );
    }
    if matches!(proof, "tests" | "both") {
        route.push("a test that names the defect and fails on the parent".to_owned());
    }
    route.push(format!("review x{reviews}"));
    route.push("merge".to_owned());
    TaskPlan {
        proof: proof.to_owned(),
        reviews,
        skills,
        tools,
        delegation: Delegation {
            kind: "implementer".to_owned(),
            tier,
            effort,
        },
        path: route,
        decided_by: decided_by.to_owned(),
        jev,
        critic: Some(critic),
        ..plan.clone()
    }
}
