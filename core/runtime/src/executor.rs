//! The async executor seam: one attempt of one node, everything it needs and nothing it may
//! guess. Work that ran and failed is a failure-shaped [`WorkOutcome`]; an error from the seam
//! is a *refusal to attempt* — the async mirror of the sync seam's illegal-dispatch posture.

use std::future::Future;
use std::pin::Pin;

use graphhelm_protocols::NodeOutcome;

/// One dispatched unit of node work.
#[derive(Clone, Debug)]
pub struct NodeWork {
    pub execution_id: String,
    pub node_id: String,
    pub attempt: u32,
    pub prompt: crate::prompt::AssembledPrompt,
    pub kind: crate::classify::NodeWorkKind,
    /// The decided tool call for `Tool`-kind work, built by whoever constructs the work unit
    /// (the driver, from the node's contract). `None` for cognitive work; a `Tool` kind with
    /// no call is unassemblable — the executor must not invent one. (Task 5 extension to the
    /// Task 1 shape, declared: the plan's sketch carried no channel for the call itself.)
    pub tool_call: Option<graphhelm_tool_broker::call::ToolCall>,
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
                }
            }
            // Delegation, not a second mapping: 05b's outcome_for_error is the one authority
            // on what parks, what retries and what is terminal (the sabotage proves it).
            Err(error) => WorkOutcome {
                outcome: graphhelm_gateway::taxonomy::outcome_for_error(error),
                sealables: Vec::new(),
                summary: WorkSummary {
                    input_tokens: None,
                    output_tokens: None,
                    exit_code: None,
                },
                reuse: None,
            },
        }
    }

    fn tool_outcome(&self, result: crate::ports::ToolPortResult) -> WorkOutcome {
        use graphhelm_tool_broker::record::ToolDisposition;
        let outcome = match &result.record.disposition {
            ToolDisposition::Completed { exit_code: 0 } => NodeOutcome::Succeeded,
            ToolDisposition::Completed { .. } | ToolDisposition::TimedOut => {
                NodeOutcome::RetryableFailure
            }
            // A lease refusal will not heal by retrying the same call.
            ToolDisposition::Denied { .. } => NodeOutcome::TerminalFailure,
            ToolDisposition::HostError { .. } => NodeOutcome::RetryableFailure,
        };
        let exit_code = match &result.record.disposition {
            ToolDisposition::Completed { exit_code } => Some(*exit_code),
            _ => None,
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
                    Ok(self.cognitive_outcome(reply))
                }
                crate::classify::NodeWorkKind::Tool => {
                    let Some(call) = work.tool_call.as_ref() else {
                        // A Tool node with no decided call cannot be executed honestly —
                        // the executor must not invent one.
                        return Err(ExecutorRefusal::Unassemblable);
                    };
                    let result = self.tools.invoke(call, &self.lease, &self.actor).await;
                    Ok(self.tool_outcome(result))
                }
            }
        })
    }
}
