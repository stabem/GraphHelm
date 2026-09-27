//! Typed graph proposal intake.
//!
//! Proposal publication is deliberately unavailable until the Runtime has a project and node
//! authorization policy. This command still records the typed draft and its governed rejection,
//! so an agent's proposal is observable without turning a bearer identity into permission to
//! mutate the operational graph.

use std::path::Path;

use graphhelm_protocols::{
    DraftRejected, EventKind, GraphDraft, NewEvent, OpaqueId, PersistedActor, Sensitivity,
};

use super::{
    Failure, append_event, argument, execution_state, load_projection, repository_failure,
};
use crate::commands::event_store;

const REASON_CODE: &str = "proposal_authorization_unavailable";

/// Records one typed proposal as an explicit governed rejection.
pub(crate) fn execute(
    events: &Path,
    execution: &str,
    draft: &[u8],
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, Some(execution))?;
    let draft: GraphDraft = serde_json::from_slice(draft)
        .map_err(|_| argument("the proposal is not a valid typed graph draft", "/draft"))?;
    let draft_id = OpaqueId::parse(&draft.id)
        .map_err(|_| argument("the draft id is not a wire-safe identifier", "/draft/id"))?;
    let execution_id = projection
        .execution_id
        .as_deref()
        .ok_or_else(|| execution_state("no execution has started on this stream", "/execution"))?;
    let execution_id = OpaqueId::parse(execution_id)
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;
    let event = NewEvent::new(
        key,
        actor.clone(),
        Sensitivity::Internal,
        EventKind::DraftRejected(DraftRejected {
            draft_id: draft_id.clone(),
            reason_code: graphhelm_protocols::SafeCode::parse(REASON_CODE)
                .expect("the proposal rejection code is a valid SafeCode"),
            diagnostics: vec![],
            detail_evidence_id: None,
        }),
        vec![],
        vec![],
    );
    append_event(
        &store,
        &scope,
        &OpaqueId::parse(&stream)
            .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?,
        event,
    )?;
    Ok(serde_json::json!({
        "executionId": execution_id,
        "draftId": draft_id,
        "status": "rejected",
        "reasonCode": REASON_CODE,
        "nextAction": "configure project and node authorization before publication",
        "actor": actor,
    }))
}
