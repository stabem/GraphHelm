//! The fixture bridge: an [`AsyncNodeExecutor`] over the effect-free `FixtureExecutor`, so the
//! API's async driver can run a fixture story (the 05a parity contract) without a gateway. A
//! test-support type by design — the sync CLI path never touches it, and no production wiring
//! constructs one outside the serve fixture path.

use std::future::Future;
use std::pin::Pin;

use graphhelm_execution::NodeExecutor as _;
use graphhelm_simulation::FixtureExecutor;

use crate::executor::{AsyncNodeExecutor, ExecutorRefusal, NodeWork, WorkOutcome, WorkSummary};

/// A thin async wrapper over the fixture table — same table, same absent-fixture semantics,
/// an immediately-ready future. Delegating to the REAL `FixtureExecutor` (rather than reading
/// the table again) is the point: the async path cannot drift from the sync path's fixture
/// meaning, because there is only one meaning.
#[doc(hidden)]
pub struct FixtureAsyncExecutor {
    fixtures: FixtureExecutor,
}

impl FixtureAsyncExecutor {
    #[must_use]
    pub fn new(fixtures: FixtureExecutor) -> Self {
        Self { fixtures }
    }
}

impl AsyncNodeExecutor for FixtureAsyncExecutor {
    fn execute<'a>(
        &'a self,
        work: &'a NodeWork,
    ) -> Pin<Box<dyn Future<Output = Result<WorkOutcome, ExecutorRefusal>> + Send + 'a>> {
        let outcome = self
            .fixtures
            .execute(&work.node_id, work.attempt)
            .map_err(|_| ExecutorRefusal::Unsupported);
        Box::pin(async move {
            let outcome = outcome?;
            Ok(WorkOutcome {
                outcome,
                // A fixture produces no free-form material and spends no tokens: nothing to
                // seal, nothing to count.
                sealables: Vec::new(),
                summary: WorkSummary {
                    input_tokens: None,
                    output_tokens: None,
                    exit_code: None,
                },
                reuse: None,
                gate_verdict: None,
                // A fixture failure is scripted, not observed. Naming it as such keeps a
                // simulated red from being triaged as a provider defect (M07 F3).
                reason: (outcome != graphhelm_protocols::NodeOutcome::Succeeded)
                    .then_some(graphhelm_protocols::NodeOutcomeReason::FixtureScripted),
                executor_kind: Some(graphhelm_protocols::AttemptExecutorKind::Fixture),
                model_route_id: None,
            })
        })
    }
}
