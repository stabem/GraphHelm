//! The async executor seam: one attempt of one node, everything it needs and nothing it may
//! guess. Work that ran and failed is a failure-shaped [`WorkOutcome`]; an error from the seam
//! is a *refusal to attempt* — the async mirror of the sync seam's illegal-dispatch posture.

use std::future::Future;
use std::pin::Pin;

use graphhelm_protocols::{NodeOutcome, NodeOutcomeReason};

/// One dispatched unit of node work.
#[derive(Clone, Debug)]
pub struct NodeWork {
    pub execution_id: String,
    pub node_id: String,
    pub attempt: u32,
    pub prompt: crate::prompt::AssembledPrompt,
    pub kind: crate::classify::NodeWorkKind,
    /// Whether an honest completed non-zero tool exit is a verdict. Absence in the graph
    /// defaults to verdict-bearing; retries require an explicit declaration.
    pub tool_failure_semantics: ToolFailureSemantics,
    /// The decided tool call for `Tool`-kind work, built by whoever constructs the work unit
    /// (the driver, from the node's contract). `None` for cognitive work; a `Tool` kind with
    /// no call is unassemblable — the executor must not invent one. (Task 5 extension to the
    /// Task 1 shape, declared: the plan's sketch carried no channel for the call itself.)
    pub tool_call: Option<graphhelm_tool_broker::call::ToolCall>,
    /// The decided gate check for `GateCheck`-kind work (M06 Task 4), from the node's
    /// contract by the same rule as `tool_call`: the executor must not invent one.
    pub gate_check: Option<GateCheckWork>,
    /// The blind-judge specialization (M06 Task 5): present when an Evaluator node's
    /// contract carries a `judge` block. Same cognitive transport; the prompt was
    /// assembled from the judge's OWN diet and the reply parses under the verdict
    /// contract instead of the plain-reply rule.
    pub judge: Option<crate::judge::JudgeWork>,
}

/// A tool node's declaration for an honest, completed non-zero process exit.
///
/// This declaration never changes timeout or host-failure handling: a timeout delivered no
/// verdict, while a host record no longer carries the OS `ErrorKind` needed to prove transience.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureSemantics {
    RetryEligible,
    #[default]
    VerdictBearing,
}

/// One decided gate evaluation: which event-sourced gate definition runs, and the whole
/// delivered surface it scores — carried by the node's contract, deserialized by the
/// driver, never invented downstream.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateCheckWork {
    /// The gate definition this check runs — what the certification precondition looks up
    /// and what the appended `GateVerdict` names.
    pub gate_id: String,
    /// The delivered surface under evaluation.
    pub delivered: graphhelm_quality::Delivered,
    /// The spec-derived content manifest.
    pub manifest: graphhelm_quality::ContentManifest,
    /// The layout grammar's budgets; the crate default when the contract is silent.
    #[serde(default)]
    pub budget: graphhelm_quality::LayoutBudget,
}

/// What real work produced: the outcome for `apply_transition`, plus the free-form material to
/// seal (D-036: it goes to Evidence, never into the event) and the event-safe summary that may
/// travel beside the outcome. The driver seals every [`Sealable`] and threads the resulting
/// references into the SAME `PreparedAppend` as the outcome event.
pub struct WorkOutcome {
    pub outcome: NodeOutcome,
    pub sealables: Vec<Sealable>,
    pub summary: WorkSummary,
    /// The host's reuse-decision summary, propagated by the tool path so the Task 6 writer
    /// can append the `ReuseDecision` ledger entry beside the outcome. Always `None` for
    /// cognitive work.
    pub reuse: Option<crate::ports::ReuseSummary>,
    /// The gate verdict produced by `GateCheck` work (M06 Task 4), appended by the writer
    /// beside the outcome in the same batch — the auditable WHY the graph routed as it
    /// did. Always `None` for cognitive and tool work.
    pub gate_verdict: Option<GateVerdictSummary>,
    /// WHY this outcome happened (M07 F3), in the closed wire vocabulary the
    /// `NodeOutcomeRecorded` kind carries. `None` on success and NEVER on a failure: an
    /// outcome the operator cannot act on is the defect the blind judge named.
    pub reason: Option<NodeOutcomeReason>,
}

/// What a gate evaluation decided, in the wire vocabulary the `GateVerdict` kind carries.
/// A failing verdict ALWAYS has findings (the evaluators emit one per defect; an empty
/// findings list means pass) — the same rule the envelope schema enforces on the wire.
#[derive(Clone, Debug)]
pub struct GateVerdictSummary {
    pub gate_id: String,
    pub passed: bool,
    pub findings: Vec<graphhelm_protocols::GateFinding>,
}

/// One item of free-form material bound for Evidence, named by a deterministic suffix so
/// retries never collide (`exec-{id}-{node}-a{attempt}-{suffix}` — Task 6's derivation).
pub struct Sealable {
    pub local_ref_suffix: &'static str,
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Digest-only, event-safe accounting. No free-form text can live here: the type has only
/// numbers, and the Task 5 test pins that a serialized summary never contains reply content.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSummary {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub exit_code: Option<i32>,
}

/// A refusal to attempt at all — never a work failure (those are failure-shaped
/// [`WorkOutcome`]s, exactly as runtime-design §6.2 resolves it).
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum ExecutorRefusal {
    #[error("this node type is not executable in this milestone")]
    Unsupported,
    #[error("the node's contract cannot be assembled into a prompt")]
    Unassemblable,
    /// M06 Task 4: the gate holds no certification against the CURRENT pathogen suite —
    /// certified or not at all. A refusal, never an outcome: an uncertified gate does not
    /// run, does not verdict, and leaves no gate ledger entry.
    #[error("the gate is not certified against the current pathogen suite")]
    Uncertified,
}

/// The async seam. Boxed-future form rather than `async fn` in the trait: the driver holds
/// executors and ports as `Arc<dyn ...>` (Task 5's `PortExecutor` composes `Arc<dyn
/// ModelPort>`), so object safety is required and the plan's fallback form is the primary one
/// — reported per the plan's own instruction.
pub trait AsyncNodeExecutor: Send + Sync {
    /// One attempt of one node.
    fn execute<'a>(
        &'a self,
        work: &'a NodeWork,
    ) -> Pin<Box<dyn Future<Output = Result<WorkOutcome, ExecutorRefusal>> + Send + 'a>>;

    /// Propagates immediate-stop to whatever the executor holds (Task 8). Default: nothing.
    fn cancel_all(&self) {}
}

/// Default token budget for a model call. The node contract has no per-call budget channel
/// until the manifest work arrives; one documented constant beats an invented field.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// The real executor: assemble → dispatch by kind through the ports → map the outcome
/// honestly. Route selection is the configured route id (one route in 05d; scoring stayed
/// deferred in 05b).
pub struct PortExecutor {
    pub model: std::sync::Arc<dyn crate::ports::ModelPort>,
    pub tools: std::sync::Arc<dyn crate::ports::ToolPort>,
    pub route_id: String,
    pub lease: graphhelm_tool_broker::lease::ToolLease,
    pub actor: String,
}

/// Names a gateway class as a cause. This is NOT a second outcome mapping —
/// `outcome_for_error` remains the one authority on what parks, retries or is terminal
/// (M06's sabotage still proves that). This maps the same error value to its NAME, and the
/// match is exhaustive so a future class must be named rather than silently unexplained.
const fn reason_for_gateway_error(
    error: graphhelm_gateway::taxonomy::GatewayError,
) -> NodeOutcomeReason {
    use graphhelm_gateway::taxonomy::GatewayError as E;
    match error {
        E::AuthRequired => NodeOutcomeReason::AuthRequired,
        E::AuthRevoked => NodeOutcomeReason::AuthRevoked,
        E::QuotaExhausted => NodeOutcomeReason::QuotaExhausted,
        E::RateLimited => NodeOutcomeReason::RateLimited,
        E::ProviderUnavailable => NodeOutcomeReason::ProviderUnavailable,
        E::ModelRemoved => NodeOutcomeReason::ModelRemoved,
        E::ContextTooLarge => NodeOutcomeReason::ContextTooLarge,
        E::MalformedOutput => NodeOutcomeReason::MalformedOutput,
        E::ToolDenied => NodeOutcomeReason::ToolDenied,
        E::RuntimeCrashed => NodeOutcomeReason::RuntimeCrashed,
        E::UnsupportedCapability => NodeOutcomeReason::UnsupportedCapability,
        E::PolicyDenied => NodeOutcomeReason::PolicyDenied,
        E::Cancelled => NodeOutcomeReason::Cancelled,
        E::Timeout => NodeOutcomeReason::Timeout,
    }
}

impl PortExecutor {
    fn cognitive_outcome(
        &self,
        reply: Result<
            graphhelm_gateway::call::ModelReply,
            graphhelm_gateway::taxonomy::GatewayError,
        >,
    ) -> WorkOutcome {
        match reply {
            Ok(reply) => {
                let sealed = serde_json::to_vec(&reply).expect("a reply serializes");
                let summary = WorkSummary {
                    input_tokens: reply.usage.input_tokens,
                    output_tokens: reply.usage.output_tokens,
                    exit_code: None,
                };
                // An empty reply is a provider defect: retryable, bounded by the attempt
                // machinery, landing in Blocked for an owner when persistent — never a
                // success, and never NeedsInput (which would park the node waiting for input
                // nothing in this milestone can deliver; the plan review's finding).
                let outcome = if reply.text.trim().is_empty() {
                    NodeOutcome::RetryableFailure
                } else {
                    NodeOutcome::Succeeded
                };
                WorkOutcome {
                    outcome,
                    sealables: vec![Sealable {
                        local_ref_suffix: "reply",
                        media_type: "application/json",
                        bytes: sealed,
                    }],
                    summary,
                    reuse: None,
                    gate_verdict: None,
                    reason: (outcome != NodeOutcome::Succeeded)
                        .then_some(NodeOutcomeReason::EmptyReply),
                }
            }
            // Delegation, not a second mapping: 05b's outcome_for_error is the one authority
            // on what parks, what retries and what is terminal (the sabotage proves it).
            //
            // M07 F3: this arm knew the most and recorded the least — no cause on the event
            // and, uniquely among the failure paths, nothing sealed beside it. Both halves
            // land here now. The sealed text is the taxonomy's own static words, so the
            // evidence cannot carry provider prose the class was chosen to keep out.
            Err(error) => WorkOutcome {
                outcome: graphhelm_gateway::taxonomy::outcome_for_error(error),
                sealables: vec![Sealable {
                    local_ref_suffix: "gateway-error",
                    media_type: "text/plain",
                    bytes: error.to_string().into_bytes(),
                }],
                summary: WorkSummary {
                    input_tokens: None,
                    output_tokens: None,
                    exit_code: None,
                },
                reuse: None,
                gate_verdict: None,
                reason: Some(reason_for_gateway_error(error)),
            },
        }
    }

    fn tool_outcome(
        &self,
        result: crate::ports::ToolPortResult,
        failure_semantics: ToolFailureSemantics,
    ) -> WorkOutcome {
        use graphhelm_tool_broker::record::ToolDisposition;
        let outcome = match &result.record.disposition {
            ToolDisposition::Completed { exit_code: 0 } => NodeOutcome::Succeeded,
            ToolDisposition::Completed { .. } => match failure_semantics {
                ToolFailureSemantics::RetryEligible => NodeOutcome::RetryableFailure,
                ToolFailureSemantics::VerdictBearing => NodeOutcome::TerminalFailure,
            },
            // A process killed by its deadline delivered no verdict, regardless of declaration.
            ToolDisposition::TimedOut => NodeOutcome::RetryableFailure,
            // A lease refusal will not heal by retrying the same call.
            ToolDisposition::Denied { .. } => NodeOutcome::TerminalFailure,
            // The durable record carries only the stable code, not Spawn/Prepare's OS ErrorKind.
            // Four codes are permanent by construction, and the two ambiguous classes cannot
            // prove transience after that information loss. Terminal is the conservative rule;
            // retrying requires a future durable cause contract, not a guess at this consumer.
            ToolDisposition::HostError { .. } => NodeOutcome::TerminalFailure,
        };
        let exit_code = match &result.record.disposition {
            ToolDisposition::Completed { exit_code } => Some(*exit_code),
            _ => None,
        };
        // The disposition's own name, so "it failed" becomes "the deadline killed it" or
        // "the lease refused it" without the operator opening Evidence first.
        let reason = match &result.record.disposition {
            ToolDisposition::Completed { exit_code: 0 } => None,
            ToolDisposition::Completed { .. } => Some(NodeOutcomeReason::ToolExitedNonZero),
            ToolDisposition::TimedOut => Some(NodeOutcomeReason::ToolTimedOut),
            ToolDisposition::Denied { .. } => Some(NodeOutcomeReason::ToolDenied),
            ToolDisposition::HostError { .. } => Some(NodeOutcomeReason::ToolHostError),
        };
        let record_json = serde_json::to_vec(&result.record).expect("a record serializes");
        WorkOutcome {
            outcome,
            sealables: vec![
                Sealable {
                    local_ref_suffix: "record",
                    media_type: "application/json",
                    bytes: record_json,
                },
                Sealable {
                    local_ref_suffix: "stdout",
                    media_type: "text/plain",
                    bytes: result.streams.stdout,
                },
                Sealable {
                    local_ref_suffix: "stderr",
                    media_type: "text/plain",
                    bytes: result.streams.stderr,
                },
            ],
            summary: WorkSummary {
                input_tokens: None,
                output_tokens: None,
                exit_code,
            },
            reuse: result.reuse,
            gate_verdict: None,
            reason,
        }
    }
}

impl AsyncNodeExecutor for PortExecutor {
    fn cancel_all(&self) {
        self.model.cancel_all();
        self.tools.cancel_all();
    }

    fn execute<'a>(
        &'a self,
        work: &'a NodeWork,
    ) -> Pin<Box<dyn Future<Output = Result<WorkOutcome, ExecutorRefusal>> + Send + 'a>> {
        Box::pin(async move {
            match work.kind {
                crate::classify::NodeWorkKind::Cognitive => {
                    // One prompt string on the wire: the assembled system block then the
                    // task, in the assembler's fixed order (the gateway's ModelCall carries
                    // a single prompt channel).
                    let prompt = format!("{}\n{}", work.prompt.system, work.prompt.task);
                    let call = graphhelm_gateway::call::ModelCall {
                        prompt,
                        max_tokens: DEFAULT_MAX_TOKENS,
                    };
                    let reply = self.model.call(&self.route_id, &call).await;
                    match work.judge.as_ref() {
                        Some(judge) => Ok(judge_outcome(judge, reply)),
                        None => Ok(self.cognitive_outcome(reply)),
                    }
                }
                crate::classify::NodeWorkKind::Tool => {
                    let Some(call) = work.tool_call.as_ref() else {
                        // A Tool node with no decided call cannot be executed honestly —
                        // the executor must not invent one.
                        return Err(ExecutorRefusal::Unassemblable);
                    };
                    let result = self.tools.invoke(call, &self.lease, &self.actor).await;
                    Ok(self.tool_outcome(result, work.tool_failure_semantics))
                }
                crate::classify::NodeWorkKind::GateCheck => {
                    let Some(gate) = work.gate_check.as_ref() else {
                        return Err(ExecutorRefusal::Unassemblable);
                    };
                    Ok(gate_check_outcome(gate))
                }
            }
        })
    }
}

/// Deterministic gate evaluation: no model port, no IO — `core/quality` scores the
/// delivered surface and ANY finding refuses. A failing verdict maps to
/// `TerminalFailure`: the evaluation is deterministic, so a retry of the same deliverable
/// can never change the answer — routing on it is the graph's decision, not the
/// machinery's. The full findings are sealed as evidence; the event-safe summary carries
/// only counts.
/// The judge's outcome mapping (M06 Task 5): a well-formed verdict seals its findings and
/// maps `passed:false` to `TerminalFailure` — the DELIVERABLE failed judgment, and
/// re-asking the same judge about the same deliverable is routing's decision, not the
/// machinery's. A malformed reply is `RetryableFailure`: the MODEL flaked, the deliverable
/// was never judged. Gateway errors ride 05b's one authority unchanged.
fn judge_outcome(
    judge: &crate::judge::JudgeWork,
    reply: Result<graphhelm_gateway::call::ModelReply, graphhelm_gateway::taxonomy::GatewayError>,
) -> WorkOutcome {
    let reply = match reply {
        Ok(reply) => reply,
        Err(error) => {
            return WorkOutcome {
                outcome: graphhelm_gateway::taxonomy::outcome_for_error(error),
                sealables: vec![Sealable {
                    local_ref_suffix: "gateway-error",
                    media_type: "text/plain",
                    bytes: error.to_string().into_bytes(),
                }],
                summary: WorkSummary {
                    input_tokens: None,
                    output_tokens: None,
                    exit_code: None,
                },
                reuse: None,
                gate_verdict: None,
                reason: Some(reason_for_gateway_error(error)),
            };
        }
    };
    let summary = WorkSummary {
        input_tokens: reply.usage.input_tokens,
        output_tokens: reply.usage.output_tokens,
        exit_code: None,
    };
    match crate::judge::parse_reply(&reply.text) {
        Ok(verdict) => {
            let sealed = serde_json::json!({
                "findings": verdict
                    .findings
                    .iter()
                    .map(|finding| serde_json::json!({
                        "severity": finding.severity,
                        "claim": finding.claim,
                        "remediation": finding.remediation,
                    }))
                    .collect::<Vec<_>>(),
                "stepsOverPar": verdict.steps_over_par,
                "stallPoints": verdict.stall_points,
            });
            let passed = verdict.passed;
            WorkOutcome {
                outcome: if passed {
                    NodeOutcome::Succeeded
                } else {
                    NodeOutcome::TerminalFailure
                },
                sealables: vec![Sealable {
                    local_ref_suffix: "judgment",
                    media_type: "application/json",
                    bytes: serde_json::to_vec(&sealed).expect("a judgment serializes"),
                }],
                summary,
                reuse: None,
                gate_verdict: Some(GateVerdictSummary {
                    gate_id: judge.judge_id.clone(),
                    passed: verdict.passed,
                    findings: verdict.findings,
                }),
                reason: (!passed).then_some(NodeOutcomeReason::JudgeRefused),
            }
        }
        // The model flaked, the deliverable was never judged: retryable, bounded by the
        // attempt machinery like every provider defect.
        Err(_) => WorkOutcome {
            outcome: NodeOutcome::RetryableFailure,
            sealables: vec![Sealable {
                local_ref_suffix: "judgment-malformed",
                media_type: "text/plain",
                bytes: reply.text.into_bytes(),
            }],
            summary,
            reuse: None,
            gate_verdict: None,
            reason: Some(NodeOutcomeReason::MalformedJudgment),
        },
    }
}

fn gate_check_outcome(gate: &crate::executor::GateCheckWork) -> WorkOutcome {
    let findings =
        graphhelm_quality::evaluate_geometry(&gate.delivered, &gate.manifest, &gate.budget);
    let passed = findings.is_empty();
    let sealed = serde_json::to_vec(&findings).expect("findings serialize");
    WorkOutcome {
        outcome: if passed {
            NodeOutcome::Succeeded
        } else {
            NodeOutcome::TerminalFailure
        },
        sealables: vec![Sealable {
            local_ref_suffix: "verdict",
            media_type: "application/json",
            bytes: sealed,
        }],
        summary: WorkSummary {
            input_tokens: None,
            output_tokens: None,
            exit_code: None,
        },
        reuse: None,
        gate_verdict: Some(GateVerdictSummary {
            gate_id: gate.gate_id.clone(),
            passed,
            findings,
        }),
        reason: (!passed).then_some(NodeOutcomeReason::GateRefused),
    }
}
