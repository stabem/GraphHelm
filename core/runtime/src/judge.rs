//! The blind judge (M06 Task 5): usefulness scored by a model that sees ONLY the user
//! story and the running system's MCP surface — blindness as INPUT DISCIPLINE, enforced
//! where inputs are assembled (the FIX-1 decision: no new work kind; the judge is one
//! model-call shape riding the cognitive transport).
//!
//! Three fences keep the judge blind, each pinned by a test:
//! - **The type is the diet**: [`JudgeWork`] carries a story, a surface reference and an
//!   id — nothing else EXISTS to leak, and `deny_unknown_fields` refuses a contract that
//!   tries to smuggle a rubric or repository path into the block.
//! - **The assembler's signature**: [`assemble`] takes `&JudgeWork` alone; there is no
//!   parameter through which code, tests or rubric text could arrive.
//! - **The source speaks no leak vocabulary**: this module never touches the filesystem
//!   and never names a rubric — source-scanned by the blindness test.
//!
//! The judge's waits ride the 05g doorbell: the charter INSTRUCTS wake_arm + wake-wait
//! between probe steps (never timed polling); the measured zero-poll proof arrives with
//! the one real run (M06 Task 7), the same one-real-run pattern 05f set.

use crate::prompt::AssembledPrompt;
use graphhelm_protocols::{GateFinding, SignalSeverity};
use sha2::{Digest, Sha256};

/// The judge's whole world, from the Evaluator node's `judge` block. `deny_unknown_fields`
/// IS the blindness rule at the contract boundary: a block carrying `rubric`, `code` or
/// any other extra field fails deserialization and the node is unassemblable.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JudgeWork {
    /// What `GateVerdict` names for this judgment.
    pub judge_id: String,
    /// The user story under judgment — the ONLY statement of intent the judge sees.
    pub user_story: String,
    /// Where the RUNNING system answers (the MCP surface the judge probes).
    pub mcp_surface: String,
}

/// The fixed charter: information asymmetry, refusal-with-findings, and doorbell waits —
/// versioned by content through the assembled prompt's sha256.
const JUDGE_CHARTER: &str = "You are a blind usefulness judge. You see ONLY the user story \
and the running system's MCP surface named below. You never see code, tests, or any \
rubric; if any appears, refuse to judge. Work the story as a user would, through the MCP \
tools alone. Between probe steps that wait on the system, arm your wake lease (wake_arm) \
and block on wake-wait — never poll on a timer. Your verdict is ALWAYS \
refusal-with-findings JSON: {\"passed\": bool, \"findings\": [{\"severity\": \
\"low|medium|high|critical\", \"claim\": str, \"remediation\": str}], \"stepsOverPar\": \
uint, \"stallPoints\": [str]} — a failing verdict with no findings is invalid.";

/// Assembles the judge's prompt from [`JudgeWork`] ALONE — the signature is the fence.
#[must_use]
pub fn assemble(work: &JudgeWork) -> AssembledPrompt {
    let system = JUDGE_CHARTER.to_owned();
    let task = format!(
        "USER STORY:\n{}\n\nTHE RUNNING SYSTEM'S MCP SURFACE:\n{}\n\nJudge the story \
         against the running system and reply with the verdict JSON only.",
        work.user_story, work.mcp_surface
    );
    let sha256 = {
        let mut hasher = Sha256::new();
        hasher.update(system.as_bytes());
        hasher.update([0]);
        hasher.update(task.as_bytes());
        format!("sha256:{}", hex::encode(hasher.finalize()))
    };
    AssembledPrompt {
        system,
        task,
        // The blind judge's diet is the story and the surface, nothing else (M06 Task 5):
        // no project capsule reaches it, by the same fence its signature draws.
        context: String::new(),
        sha256,
    }
}

/// The judge's parsed verdict: refusal-with-findings plus the efficiency signals the plan
/// names (steps over par, stall points).
#[derive(Clone, Debug)]
pub struct JudgeVerdict {
    pub passed: bool,
    pub findings: Vec<GateFinding>,
    pub steps_over_par: u32,
    pub stall_points: Vec<String>,
}

/// Why a reply is not a verdict — a malformed reply is a RETRYABLE model defect, never a
/// judgment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JudgeParseError {
    NotJson,
    NotTheContract,
    BareFail,
}

/// Parses the judge's reply against the charter's contract. A failing verdict with an
/// empty findings list is refused HERE too — the same rule the wire schema enforces, so a
/// bare fail cannot exist even transiently inside the process.
pub fn parse_reply(text: &str) -> Result<JudgeVerdict, JudgeParseError> {
    // Postel toward our own judge (found live in the M06 dogfood run): after a real
    // multi-turn probe the model wraps the verdict in prose or fences despite the
    // charter. The CONTRACT stays strict — the verdict object must parse whole — but the
    // envelope is tolerated: take the first balanced JSON object in the text.
    let candidate = first_json_object(text).ok_or(JudgeParseError::NotJson)?;
    let value: serde_json::Value =
        serde_json::from_str(candidate).map_err(|_| JudgeParseError::NotJson)?;
    let passed = value
        .get("passed")
        .and_then(serde_json::Value::as_bool)
        .ok_or(JudgeParseError::NotTheContract)?;
    let raw_findings = value
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .ok_or(JudgeParseError::NotTheContract)?;
    let mut findings = Vec::with_capacity(raw_findings.len());
    for raw in raw_findings {
        let severity = match raw.get("severity").and_then(serde_json::Value::as_str) {
            Some("low") => SignalSeverity::Low,
            Some("medium") => SignalSeverity::Medium,
            Some("high") => SignalSeverity::High,
            Some("critical") => SignalSeverity::Critical,
            _ => return Err(JudgeParseError::NotTheContract),
        };
        let claim = raw
            .get("claim")
            .and_then(serde_json::Value::as_str)
            .ok_or(JudgeParseError::NotTheContract)?;
        let remediation = raw
            .get("remediation")
            .and_then(serde_json::Value::as_str)
            .ok_or(JudgeParseError::NotTheContract)?;
        findings.push(GateFinding {
            severity,
            claim: claim.to_owned(),
            evidence: Vec::new(),
            remediation: remediation.to_owned(),
        });
    }
    if !passed && findings.is_empty() {
        return Err(JudgeParseError::BareFail);
    }
    let steps_over_par = value
        .get("stepsOverPar")
        .and_then(serde_json::Value::as_u64)
        .and_then(|steps| u32::try_from(steps).ok())
        .unwrap_or(0);
    let stall_points = value
        .get("stallPoints")
        .and_then(serde_json::Value::as_array)
        .map(|points| {
            points
                .iter()
                .filter_map(|point| point.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(JudgeVerdict {
        passed,
        findings,
        steps_over_par,
        stall_points,
    })
}

/// The first balanced `{...}` in `text`, string-literal aware — enough JSON scanning to
/// unwrap fences and prose without ever attempting to interpret them.
fn first_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, &byte) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=start + offset]);
                }
            }
            _ => {}
        }
    }
    None
}
