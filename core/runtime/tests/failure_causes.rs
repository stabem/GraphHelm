//! M07 Task 2 (F3): a failure that cannot say WHY is a failure the operator cannot act on.
//!
//! The blind judge refused the M06 story partly because `retryable_failure` events carried
//! no cause at all — no error string, no exit code, no evidence. Worse, the one arm that
//! knew the most (the gateway error) sealed the LEAST: it recorded nothing while every
//! other failure path sealed its material. This suite pins both halves — the closed cause
//! vocabulary on the event, and the evidence beside it — plus the rule that makes them
//! honest: no failure outcome may be silent about why.
//!
//! The cause is a CLOSED enum, never free text. `GatewayError` is a field-free `Copy` enum
//! of static classes (`core/gateway/src/taxonomy.rs`), so the class cannot smuggle a
//! provider message, a path or a token into the durable stream; the text belongs to
//! Evidence, sealed under D-036, which is exactly where this task puts it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use graphhelm_gateway::call::{ModelCall, ModelReply, Usage};
use graphhelm_gateway::taxonomy::GatewayError;
use graphhelm_protocols::{NodeOutcome, NodeOutcomeReason};
use graphhelm_runtime::classify::NodeWorkKind;
use graphhelm_runtime::executor::{AsyncNodeExecutor, NodeWork, PortExecutor};
use graphhelm_runtime::ports::{ModelPort, ToolPort, ToolPortResult, ToolStreams};
use graphhelm_runtime::prompt::AssembledPrompt;
use graphhelm_tool_broker::call::{ShellAction, ToolCall};
use graphhelm_tool_broker::effect::IsolationTier;
use graphhelm_tool_broker::lease::{Capability, ToolLease};
use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition, digest_hex};

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(future)
}

struct FakeModelPort {
    result: Result<ModelReply, GatewayError>,
    calls: AtomicUsize,
}

impl ModelPort for FakeModelPort {
    fn call<'a>(
        &'a self,
        _route_id: &'a str,
        _call: &'a ModelCall,
    ) -> Pin<Box<dyn Future<Output = Result<ModelReply, GatewayError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

struct FakeToolPort {
    disposition: ToolDisposition,
}

impl ToolPort for FakeToolPort {
    fn invoke<'a>(
        &'a self,
        _call: &'a ToolCall,
        _lease: &'a ToolLease,
        _actor: &'a str,
    ) -> Pin<Box<dyn Future<Output = ToolPortResult> + Send + 'a>> {
        let record = ToolCallRecord {
            tool: "shell".to_owned(),
            action: "run".to_owned(),
            actor: "agent-runtime".to_owned(),
            program_allowlist: ["git".to_owned()].into_iter().collect(),
            tier: IsolationTier::Tier1,
            disposition: self.disposition.clone(),
            stdout_sha256: digest_hex(b"TOOL-STDOUT-SENTINEL"),
            stdout_bytes: 20,
            stderr_sha256: digest_hex(b""),
            stderr_bytes: 0,
            truncated: false,
            reused: false,
            verified_executable: None,
            contained_session: None,
            commit: None,
            landed_ref: None,
            recovered_workspace: false,
        };
        Box::pin(async move {
            ToolPortResult {
                record,
                streams: ToolStreams {
                    stdout: b"TOOL-STDOUT-SENTINEL".to_vec(),
                    stderr: Vec::new(),
                },
                reuse: None,
            }
        })
    }
}

fn lease() -> ToolLease {
    ToolLease {
        actor: "agent-runtime".to_owned(),
        capabilities: [Capability::ShellExecute].into_iter().collect(),
        programs: ["git".to_owned()].into_iter().collect(),
    }
}

fn executor(model: Result<ModelReply, GatewayError>, disposition: ToolDisposition) -> PortExecutor {
    PortExecutor {
        model: Arc::new(FakeModelPort {
            result: model,
            calls: AtomicUsize::new(0),
        }),
        tools: Arc::new(FakeToolPort { disposition }),
        route_id: "claude_subscription".to_owned(),
        lease: lease(),
        actor: "agent-runtime".to_owned(),
        gates: Arc::new(NoGates),
    }
}

fn reply(text: &str) -> ModelReply {
    ModelReply {
        text: text.to_owned(),
        usage: Usage {
            input_tokens: Some(12),
            output_tokens: Some(5),
        },
    }
}

fn prompt() -> AssembledPrompt {
    AssembledPrompt {
        system: "system".to_owned(),
        task: "task".to_owned(),
        context: String::new(),
        sha256: "sha256:0".to_owned(),
    }
}

fn cognitive_work() -> NodeWork {
    NodeWork {
        execution_id: "exec-1".to_owned(),
        node_id: "implement".to_owned(),
        attempt: 1,
        prompt: prompt(),
        kind: NodeWorkKind::Cognitive,
        tool_failure_semantics: Default::default(),
        tool_call: None,
        gate_check: None,
        judge: None,
        context: None,
    }
}

fn tool_work() -> NodeWork {
    NodeWork {
        execution_id: "exec-1".to_owned(),
        node_id: "tests".to_owned(),
        attempt: 1,
        prompt: prompt(),
        kind: NodeWorkKind::Tool,
        tool_failure_semantics: Default::default(),
        tool_call: Some(ToolCall::Shell(ShellAction {
            program: "git".to_owned(),
            arguments: vec!["status".to_owned()],
        })),
        gate_check: None,
        judge: None,
        context: None,
    }
}

/// Every gateway class the taxonomy knows names itself on the outcome. The outcome mapping
/// stays 05b's alone (`outcome_for_error`); this is the CAUSE, a separate, exhaustive
/// naming of the same error value — no wildcard, so a new class cannot default to silence.
#[test]
fn every_gateway_class_names_itself_as_the_cause() {
    for (error, expected) in [
        (
            GatewayError::QuotaExhausted,
            NodeOutcomeReason::QuotaExhausted,
        ),
        (GatewayError::RateLimited, NodeOutcomeReason::RateLimited),
        (GatewayError::Timeout, NodeOutcomeReason::Timeout),
        (GatewayError::PolicyDenied, NodeOutcomeReason::PolicyDenied),
        (GatewayError::Cancelled, NodeOutcomeReason::Cancelled),
        (
            GatewayError::ProviderUnavailable,
            NodeOutcomeReason::ProviderUnavailable,
        ),
        (
            GatewayError::RuntimeCrashed,
            NodeOutcomeReason::RuntimeCrashed,
        ),
        (
            GatewayError::MalformedOutput,
            NodeOutcomeReason::MalformedOutput,
        ),
        (GatewayError::AuthRequired, NodeOutcomeReason::AuthRequired),
        (GatewayError::AuthRevoked, NodeOutcomeReason::AuthRevoked),
        (GatewayError::ModelRemoved, NodeOutcomeReason::ModelRemoved),
        (
            GatewayError::ContextTooLarge,
            NodeOutcomeReason::ContextTooLarge,
        ),
        (
            GatewayError::UnsupportedCapability,
            NodeOutcomeReason::UnsupportedCapability,
        ),
        (GatewayError::ToolDenied, NodeOutcomeReason::ToolDenied),
    ] {
        let executor = executor(Err(error), ToolDisposition::Completed { exit_code: 0 });
        let outcome = block_on(executor.execute(&cognitive_work())).unwrap();
        assert_eq!(outcome.reason, Some(expected), "{error:?} must name itself");
    }
}

/// The asymmetry the judge's finding names: the arm that knew the most sealed the least.
#[test]
fn a_gateway_failure_seals_its_evidence_like_every_other_failure_path() {
    let executor = executor(
        Err(GatewayError::ProviderUnavailable),
        ToolDisposition::Completed { exit_code: 0 },
    );
    let outcome = block_on(executor.execute(&cognitive_work())).unwrap();
    assert!(
        !outcome.sealables.is_empty(),
        "a gateway failure must leave evidence behind, not just a class"
    );
    let sealed = String::from_utf8(outcome.sealables[0].bytes.clone()).expect("utf-8");
    assert!(
        sealed.contains("provider is unavailable"),
        "the sealed material carries the taxonomy's own words: {sealed}"
    );
}

/// An empty reply and a malformed judgment are distinct causes: the first is a provider
/// that said nothing, the second a provider that said something unusable. Collapsing them
/// would tell the operator to look in the wrong place.
#[test]
fn an_empty_reply_names_itself_and_is_not_confused_with_a_malformed_one() {
    let executor = executor(Ok(reply("")), ToolDisposition::Completed { exit_code: 0 });
    let outcome = block_on(executor.execute(&cognitive_work())).unwrap();
    assert_eq!(outcome.outcome, NodeOutcome::RetryableFailure);
    assert_eq!(outcome.reason, Some(NodeOutcomeReason::EmptyReply));
}

/// Each tool disposition names its own cause — the judge asked for the exit code's meaning
/// to survive, not just the retry.
#[test]
fn every_failing_tool_disposition_names_its_own_cause() {
    for (disposition, expected) in [
        (
            ToolDisposition::Completed { exit_code: 2 },
            NodeOutcomeReason::ToolExitedNonZero,
        ),
        (ToolDisposition::TimedOut, NodeOutcomeReason::ToolTimedOut),
        (
            ToolDisposition::HostError {
                code: "spawn-failed".to_owned(),
            },
            NodeOutcomeReason::ToolHostError,
        ),
        (
            ToolDisposition::Denied {
                rule: "shell-not-leased".to_owned(),
            },
            NodeOutcomeReason::ToolDenied,
        ),
    ] {
        let executor = executor(Ok(reply("unused")), disposition.clone());
        let outcome = block_on(executor.execute(&tool_work())).unwrap();
        assert_eq!(outcome.reason, Some(expected), "{disposition:?}");
    }
}

/// The guard that makes the rest binding: success is silent, failure never is. This is the
/// test the sabotage must break.
#[test]
fn no_failure_outcome_is_ever_silent_about_why() {
    let succeeded = executor(
        Ok(reply("done")),
        ToolDisposition::Completed { exit_code: 0 },
    );
    let outcome = block_on(succeeded.execute(&cognitive_work())).unwrap();
    assert_eq!(outcome.outcome, NodeOutcome::Succeeded);
    assert_eq!(
        outcome.reason, None,
        "a success has nothing to explain — a cause here would be noise"
    );

    for failing in [
        Err(GatewayError::Timeout),
        Err(GatewayError::QuotaExhausted),
        Err(GatewayError::PolicyDenied),
        Err(GatewayError::Cancelled),
        Ok(reply("")),
    ] {
        let executor = executor(failing, ToolDisposition::Completed { exit_code: 0 });
        let outcome = block_on(executor.execute(&cognitive_work())).unwrap();
        if outcome.outcome == NodeOutcome::Succeeded {
            continue;
        }
        assert!(
            outcome.reason.is_some(),
            "{:?} landed without a cause — the operator is back to guessing",
            outcome.outcome
        );
    }
}

/// No gate runs in this file, and that is a property worth stating rather than a gap: a
/// registry that answers `None` to every id refuses every gate node, which is what a driver
/// exercising cognitive and tool work should do if a gate node ever appears here by accident.
struct NoGates;

impl graphhelm_runtime::ports::GateRegistryPort for NoGates {
    fn suite_digest(&self, _gate_id: &str) -> Option<String> {
        None
    }

    fn evaluate(
        &self,
        _gate_id: &str,
        _evidence: &serde_json::Value,
    ) -> Option<graphhelm_runtime::ports::GateEvaluation> {
        None
    }
}
