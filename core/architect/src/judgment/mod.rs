//! Typed judgments over a draft (spec §1, D3). Every site here may only (a) append a repairable
//! diagnostic, (b) rank under a fixed policy, or (c) report; nothing here edits a draft, and
//! nothing here removes a diagnostic a deterministic check produced.

pub mod nodes;
pub mod policy;
pub mod ranking;
pub mod red;
pub mod reuse;

use std::collections::BTreeMap;

use graphhelm_gateway::call::Usage;

use crate::judge::JudgeModel;
use crate::library::GraphLibrary;

/// What a caller may add to `synthesize` beyond the three inputs of the first compile (spec
/// D4). `Extras::default()` reproduces `synthesize` byte for byte.
#[derive(Clone, Copy)]
pub struct Extras<'a> {
    /// The judge door, when the caller names one. `None` is today's road: no judgment is asked.
    pub judge: Option<&'a dyn JudgeModel>,
    /// How many drafts to ask for and rank (site 2); `1` is today's road. Bounded to `1..=3`,
    /// refused as `InvalidProfile` at `/drafts` outside that.
    pub drafts: u8,
    /// The caller's graph library (sites 4 and 1, spec D8). Read only when a judge is named and
    /// the library holds at least one template; `None`, or an empty library, is today's road.
    pub library: Option<&'a GraphLibrary>,
}

impl Default for Extras<'_> {
    fn default() -> Self {
        Self {
            judge: None,
            drafts: 1,
            library: None,
        }
    }
}

/// The report field of a run that named a judge AND a non-empty library (spec D8).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReuseReport {
    /// The road taken: `reuse`, `adapt` or `create` (`reuse::Road::label`).
    pub road: String,
    /// The template the road used: filled under `reuse`, seeded under `adapt`; the one whose
    /// fill was unresolved when `create` was fallen to for that reason; else `None`.
    pub template: Option<String>,
    /// The parameter values the judge resolved (every one, under `reuse`).
    pub parameters: BTreeMap<String, String>,
    /// The judge's confidence in the road answer; `0.0` when unanswered.
    pub confidence: f64,
    /// The decision or the fill fell under the acting threshold (or an answer was missing):
    /// today's road was taken and nothing was reused (spec D6).
    pub unresolved: bool,
    /// Summed over the library road's judge calls: the decision, plus the fill under `reuse`.
    /// These calls are not in `JudgmentReport::usage`, and the pure `reuse` road has no
    /// `JudgmentReport` at all (#1126 review finding).
    pub usage: Usage,
}

/// One node's answers, verbatim, as the report shows them.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeJudgment {
    pub node: String,
    /// The probability the node serves the goal. `NaN` when the judge did not answer.
    pub on_goal: f64,
    /// The node type the judge would give this objective, from the catalog; empty when the
    /// judge did not answer.
    pub kind: String,
    /// `NaN` when the judge did not answer.
    pub kind_confidence: f64,
}

/// The report field of a run that named a judge (spec D3 door (c), D4).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgmentReport {
    /// One entry per node of the accepted draft, in node-id order.
    pub nodes: Vec<NodeJudgment>,
    /// Nodes whose answers fell between the thresholds (or were missing): nothing was done,
    /// and that is visible (spec D6).
    pub unresolved: Vec<String>,
    /// Summed over every per-node and ranking judge call of the run, including the rounds that
    /// were repaired. The library road's calls are summed on `ReuseReport::usage` instead.
    pub usage: Usage,
}

/// One ranked candidate, as the report shows it (site 2, spec D7).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The candidate's position: draft 1 is index 0.
    pub index: u8,
    /// The label of the stance that drafted it (`Stance::label`).
    pub stance: String,
    /// The coverage score, a position on `ranking::COVERAGE_LEVELS`; `NaN` when unanswered.
    pub coverage: f64,
    /// The probability the candidate does work the goal did not ask for; `NaN` when unanswered.
    pub waste: f64,
    /// The judge's confidence in the coverage score; `0.0` when unanswered.
    pub confidence: f64,
    /// `coverage - waste`, or `-inf` when either answer was missing or mistyped.
    pub composite: f64,
}

/// The report field of a run that asked for more than one draft (site 2, spec D7).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingReport {
    /// Every candidate, in index order.
    pub candidates: Vec<Candidate>,
    /// The index of the document returned: the highest composite, ties to the lower index; `0`
    /// when `unresolved`.
    pub chosen: u8,
    /// The top candidate's confidence was under the acting threshold (or its answers were
    /// missing): the first draft was kept and nothing was ranked (spec D6).
    pub unresolved: bool,
}
