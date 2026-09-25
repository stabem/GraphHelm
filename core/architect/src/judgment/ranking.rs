//! Site 2: rank N valid drafts (spec D7). State = the goal and, per candidate, its nodes
//! `{id, type, objective}`. Questions per candidate: `coverage:<i>` (Score over
//! [`COVERAGE_LEVELS`]) and `waste:<i>` (Noul: does it do work the goal did not ask for).
//! Composite = `coverage - waste`; the highest wins; a tie goes to the lower index; a top
//! candidate whose confidence is under `ACT_THRESHOLD`, or whose answers were missing or
//! mistyped, keeps candidate 0 (today's road) and the report says `unresolved`. Nothing here
//! edits a draft: the caller picks one whole document by index.

use std::collections::BTreeMap;

use graphhelm_gateway::judgment::{
    Answer, JEV_LATEST, JudgeReply, JudgeRequest, NoulCriteria, Question,
};
use graphhelm_protocols::ExecutionGraph;

use super::policy::acts;
use super::{Candidate, RankingReport};
use crate::profile::TaskProfile;

/// The ordered levels of `coverage:<i>`; the score is a position on them, `0.0..=2.0`.
pub const COVERAGE_LEVELS: [&str; 3] = [
    "Misses at least one thing the goal explicitly asks for",
    "Covers the goal but leaves a stated outcome unchecked",
    "Covers every stated outcome and checks it",
];

/// The request over every candidate: two questions per index, over the goal and each
/// candidate's nodes. Candidates are numbered by their position in `candidates`.
#[must_use]
pub fn request(profile: &TaskProfile, candidates: &[ExecutionGraph]) -> JudgeRequest {
    let listed: Vec<serde_json::Value> = candidates
        .iter()
        .enumerate()
        .map(|(index, graph)| {
            let nodes: Vec<serde_json::Value> = graph
                .spec
                .nodes
                .iter()
                .map(|(id, node)| {
                    serde_json::json!({
                        "id": id,
                        "type": node.node_type.as_str(),
                        "objective": node.objective,
                    })
                })
                .collect();
            serde_json::json!({ "index": index, "nodes": nodes })
        })
        .collect();
    let mut questions = BTreeMap::new();
    for index in 0..candidates.len() {
        questions.insert(
            format!("coverage:{index}"),
            Question::Score {
                instructions: format!(
                    "How completely do the nodes of the candidate with index {index} (see \
                     `candidates`) cover the `goal`? Judge the objectives against what the goal \
                     explicitly asks for and whether each stated outcome is checked."
                ),
                criteria: COVERAGE_LEVELS
                    .iter()
                    .map(|level| (*level).to_owned())
                    .collect(),
            },
        );
        questions.insert(
            format!("waste:{index}"),
            Question::Noul {
                instructions: format!(
                    "Does the candidate with index {index} (see `candidates`) do work the `goal` \
                     did not ask for?"
                ),
                criteria: Some(NoulCriteria {
                    r#true: "At least one node's objective is outside the goal, or duplicates \
                             another node's"
                        .to_owned(),
                    r#false: "Every node's objective is work the goal needs".to_owned(),
                }),
            },
        );
    }
    JudgeRequest {
        state: serde_json::json!({ "goal": profile.goal, "candidates": listed }),
        model: JEV_LATEST.to_owned(),
        questions,
    }
}

/// Reads the reply for `count` candidates into a report. A missing or mistyped answer makes
/// that candidate's `coverage` / `waste` NaN and its composite `-inf` (never a verdict in its
/// favour); on the wire every one of those is JSON `null`, because `serde_json` writes a
/// non-finite `f64` as `null` (#1125 review finding). `stance` is left empty for the caller,
/// which knows which stance produced which index.
#[must_use]
pub fn read(reply: &JudgeReply, count: u8) -> RankingReport {
    let mut candidates = Vec::with_capacity(usize::from(count));
    for index in 0..count {
        let (coverage, confidence) = match reply.answers.get(&format!("coverage:{index}")) {
            Some(Answer::Score {
                score, confidence, ..
            }) => (*score, *confidence),
            _ => (f64::NAN, 0.0),
        };
        let waste = match reply.answers.get(&format!("waste:{index}")) {
            Some(Answer::Noul { noul }) => *noul,
            _ => f64::NAN,
        };
        let composite = if coverage.is_nan() || waste.is_nan() {
            f64::NEG_INFINITY
        } else {
            coverage - waste
        };
        candidates.push(Candidate {
            index,
            stance: String::new(),
            coverage,
            waste,
            confidence,
            composite,
        });
    }
    // Strictly-greater replaces, so an equal composite keeps the earlier (lower) index.
    let best = candidates
        .iter()
        .fold(None::<&Candidate>, |best, candidate| match best {
            Some(held) if held.composite >= candidate.composite => Some(held),
            _ => Some(candidate),
        });
    let (chosen, unresolved) = match best {
        Some(candidate) if acts(candidate.confidence) && candidate.composite.is_finite() => {
            (candidate.index, false)
        }
        _ => (0, true),
    };
    RankingReport {
        candidates,
        chosen,
        unresolved,
    }
}
