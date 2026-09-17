//! Typed judgments over a draft (spec §1, D3). Every site here may only (a) append a repairable
//! diagnostic, (b) rank under a fixed policy, or (c) report; nothing here edits a draft, and
//! nothing here removes a diagnostic a deterministic check produced.

pub mod nodes;
pub mod policy;

use graphhelm_gateway::call::Usage;

use crate::judge::JudgeModel;

/// What a caller may add to `synthesize` beyond the three inputs of the first compile (spec
/// D4). `Extras::default()` reproduces `synthesize` byte for byte.
#[derive(Clone, Copy)]
pub struct Extras<'a> {
    /// The judge door, when the caller names one. `None` is today's road: no judgment is asked.
    pub judge: Option<&'a dyn JudgeModel>,
    /// How many drafts to ask for and rank (site 2); `1` is today's road. Bounded to `1..=3`,
    /// refused as `InvalidProfile` at `/drafts` outside that.
    pub drafts: u8,
}

impl Default for Extras<'_> {
    fn default() -> Self {
        Self {
            judge: None,
            drafts: 1,
        }
    }
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
    /// Summed over every judge call of the run, including the rounds that were repaired.
    pub usage: Usage,
}
