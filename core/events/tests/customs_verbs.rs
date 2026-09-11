//! The customs VERBS (#159, #132): what `claim` and `clear` actually write, read back off a real
//! journal.
//!
//! Every cell calls the real verb against a real repository and then replays the real appended
//! envelopes. Nothing recomputes an expected verdict: the fold decides, and these cells observe.
//!
//! Every refusal cell below asserts its ARRANGEMENT before it demands the refusal — a refusal
//! produced by a fixture that never reached the decision it names would be a companion pass for
//! the headline, and the decision order (first match wins) is the thing under test.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    ClaimError, ClaimOutcome, ClaimRequest, ClearError, ClearanceOutcome, CustomsStage,
    LocalEventRepository, PreparedAppend, claim, claim_evidence_digest, clear, refusal, replay,
};
use graphhelm_protocols::{
    ActorId, ClaimAttestation, ClaimAttestationMode, ClaimEvidence, Clock, DlqReturned, DlqRouted,
    EventEnvelope, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator, NewEvent,
    NodeOutcome as Outcome, NodeOutcomeRecorded, NodeState, OpaqueId, PersistedActor,
    PersistedActorType, ProjectId, RepositoryScope, SafeCode, Sensitivity, WireHash, WorkspaceId,
};

const STREAM: &str = "stream-execution-test";
const CUSTOMS_NODE: &str = "implementation";

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-test").unwrap()),
    )
}

fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-test").unwrap(),
    )
}

fn key(name: &str) -> OpaqueId {
    OpaqueId::parse(name).unwrap()
}

fn event(key: impl Into<String>, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key.into()).unwrap(),
        actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

/// A repository in a fresh temporary directory, opened exactly as `execution_projection.rs`
/// opens its own: fixed clock, counting ids. The directory is returned so it outlives the
/// repository handle; dropping it first would pull the journal out from under the store.
fn fresh_repository() -> (LocalEventRepository, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    (repository, directory)
}

/// Appends `events` in one batch through the real repository, so sequencing, idempotency and
/// the hash chain are produced by the same code that runs in production rather than hand-computed
/// here. The batch starts at sequence 1 because every cell here writes its arrangement first.
fn append_to(repository: &LocalEventRepository, events: Vec<NewEvent>) -> Vec<EventEnvelope> {
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        1,
        events,
        vec![],
        vec![],
    )
    .unwrap();
    repository.append_atomic(&request).unwrap()
}

/// Appends a LATER batch, starting at `at` rather than at 1: the order cells below arrange their
/// world in stages, with a real `claim` call in between.
fn append_at(
    repository: &LocalEventRepository,
    at: u64,
    events: Vec<NewEvent>,
) -> Vec<EventEnvelope> {
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        at,
        events,
        vec![],
        vec![],
    )
    .unwrap();
    repository.append_atomic(&request).unwrap()
}

fn read_all(repository: &LocalEventRepository) -> Vec<EventEnvelope> {
    repository.read_replay_stream(&scope(), STREAM).unwrap()
}

fn outcome_event(key: &str, outcome: Outcome, next_state: NodeState) -> NewEvent {
    event(
        key.to_owned(),
        EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            node_id: OpaqueId::parse(CUSTOMS_NODE).unwrap(),
            outcome,
            next_state,
            reason: None,
        }),
    )
}

fn started_event() -> NewEvent {
    event(
        "execution-started",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Supervised,
        }),
    )
}

/// Routes `implementation` to the dead-letter queue. The fold closes the node's WAIT here and
/// leaves `node_states` and `open_claims` alone — that asymmetry is what the `not_waiting`
/// order cells below stand on.
fn dlq_routed_event(key: &str, episode_sequence: u64) -> NewEvent {
    event(
        key.to_owned(),
        EventKind::DlqRouted(DlqRouted {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            node_id: OpaqueId::parse(CUSTOMS_NODE).unwrap(),
            episode_sequence,
            reason: SafeCode::parse("dead_lettered").unwrap(),
        }),
    )
}

/// Returns `implementation` from the dead-letter queue. The fold MINTS a wait keyed by this
/// envelope's own sequence and does NOT touch `node_states`: that is how a node can hold an
/// open wait while being out of `WaitingInput`.
fn dlq_returned_event(key: &str, dlq_episode_sequence: u64) -> NewEvent {
    event(
        key.to_owned(),
        EventKind::DlqReturned(DlqReturned {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            node_id: OpaqueId::parse(CUSTOMS_NODE).unwrap(),
            dlq_episode_sequence,
            wait_within_seconds: None,
        }),
    )
}

/// Drives `implementation` to a parked wait: execution start, dispatch, run, park.
///
/// The parking event is the last entry, so its sequence is the batch's length — predicted here,
/// checked by [`sequence_of`] once the batch is appended.
fn parked_batch() -> Vec<NewEvent> {
    vec![
        started_event(),
        outcome_event("dispatch", Outcome::Started, NodeState::Queued),
        outcome_event("run", Outcome::Started, NodeState::Running),
        outcome_event("park", Outcome::NeedsInput, NodeState::WaitingInput),
    ]
}

/// The sequence of the event at `index`, CHECKED rather than assumed: a batch appended from a
/// fresh repository numbers its events 1..n in order, and this asserts that held before any test
/// depends on it.
fn sequence_of(appended: &[EventEnvelope], index: usize) -> u64 {
    let sequence = appended[index].sequence;
    assert_eq!(
        sequence,
        index as u64 + 1,
        "batch position {index} did not become sequence {}: the fixture's whole identity model \
         rests on this",
        index + 1
    );
    sequence
}

fn attestation() -> ClaimAttestation {
    ClaimAttestation {
        asserter: OpaqueId::parse("agent-claimer").unwrap(),
        mode: ClaimAttestationMode::OperatorAttested,
    }
}

fn evidence(kind: &str) -> ClaimEvidence {
    ClaimEvidence {
        kind: kind.to_owned(),
        content_hash: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        size: 7,
    }
}

fn request<'a>(wait: Option<u64>, kinds: &[&str], required: &'a [String]) -> ClaimRequest<'a> {
    ClaimRequest {
        node: CUSTOMS_NODE,
        completes_wait_seq: wait,
        evidence: kinds.iter().map(|kind| evidence(kind)).collect(),
        attestation: attestation(),
        required_proof_kinds: required,
    }
}

fn refused_payload(envelope: &EventEnvelope) -> &graphhelm_protocols::CompletionRefused {
    match &envelope.kind {
        EventKind::CompletionRefused(payload) => payload,
        other => panic!("expected completion_refused, got {other:?}"),
    }
}

/// The positive control every refusal cell below is measured against.
#[test]
fn a_claim_against_the_open_wait_is_journaled_and_enters_quarantine() {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let wait_seq = sequence_of(&history, history.len() - 1);
    let required: Vec<String> = vec!["test_report".to_owned()];

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-1"),
        request(Some(wait_seq), &["test_report"], &required),
    )
    .unwrap();

    let claim_seq = appended[0].sequence;
    assert_eq!(
        outcome,
        ClaimOutcome::Claimed {
            claim_seq,
            wait_seq
        }
    );
    assert!(matches!(appended[0].kind, EventKind::CompletionClaimed(_)));
    assert_eq!(
        claim_seq,
        history.len() as u64 + 1,
        "appended at the sequence the decision read"
    );
    let projection = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert!(
        projection.open_claims.contains_key(&claim_seq),
        "quarantine holds the claim"
    );
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput),
        "a claim is testimony, not a transition"
    );
}

/// Absent `completes_wait_seq` answers the node's OPEN wait — the CLI's default.
#[test]
fn a_claim_without_a_named_wait_answers_the_open_one() {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let open_wait = sequence_of(&history, history.len() - 1);

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-open"),
        request(None, &[], &[]),
    )
    .unwrap();

    let claim_seq = appended[0].sequence;
    assert_eq!(
        outcome,
        ClaimOutcome::Claimed {
            claim_seq,
            wait_seq: open_wait
        }
    );
    match &appended[0].kind {
        EventKind::CompletionClaimed(payload) => {
            assert_eq!(payload.completes_wait_seq, open_wait);
        }
        other => panic!("expected completion_claimed, got {other:?}"),
    }
}

/// THE TRAP GUARD (#159): the arrangement must be constructable BEFORE the refusal is demanded.
///
/// `(WaitingInput, NeedsInput) -> WaitingInput` is the state machine's own arm, so a node
/// re-parks and its first wait is superseded with today's events alone.
#[test]
fn a_claim_naming_a_superseded_wait_is_refused_stale_rendezvous_and_the_node_stays_parked() {
    let (repository, _dir) = fresh_repository();
    let mut batch = parked_batch();
    let first_wait_index = batch.len() - 1;
    // Re-park: the SECOND wait supersedes the first under the same node name.
    batch.push(outcome_event(
        "park-again",
        Outcome::NeedsInput,
        NodeState::WaitingInput,
    ));
    let history = append_to(&repository, batch);
    let first_wait = sequence_of(&history, first_wait_index);
    let projection = replay(&scope(), STREAM, &history).unwrap();
    assert_ne!(
        projection.open_waits[CUSTOMS_NODE].at_sequence, first_wait,
        "PRECONDITION: the first wait is superseded"
    );

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-stale"),
        request(Some(first_wait), &[], &[]),
    )
    .unwrap();

    assert_eq!(
        outcome,
        ClaimOutcome::Refused {
            reason_code: refusal::STALE_RENDEZVOUS,
            wait_seq: first_wait
        }
    );
    let payload = refused_payload(&appended[0]);
    assert_eq!(payload.claimed_wait_seq, first_wait);
    assert_eq!(payload.reason_code.as_str(), "stale_rendezvous");

    let after = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        after.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput)
    );
    assert!(after.open_claims.is_empty());
}

/// Sequence 1 is the `execution_started` event: nothing ever parked there, so it is not a stale
/// wait but a sequence that was never a wait at all.
#[test]
fn a_claim_naming_a_sequence_that_never_parked_is_refused_unknown_wait() {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let projection = replay(&scope(), STREAM, &history).unwrap();
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput),
        "PRECONDITION: the node IS waiting, so `not_waiting` cannot be the reason"
    );
    assert!(
        !projection.customs_scans[CUSTOMS_NODE]
            .iter()
            .any(|scan| scan.at_sequence == 1),
        "PRECONDITION: no scan of this node ever happened at sequence 1"
    );

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-unknown"),
        request(Some(1), &[], &[]),
    )
    .unwrap();

    assert_eq!(
        outcome,
        ClaimOutcome::Refused {
            reason_code: refusal::UNKNOWN_WAIT,
            wait_seq: 1
        }
    );
    let payload = refused_payload(&appended[0]);
    assert_eq!(payload.claimed_wait_seq, 1);
    assert_eq!(payload.reason_code.as_str(), "unknown_wait");
}

/// A node that has only been dispatched (`Queued`) has nothing to complete.
///
/// Two claims, because the arm has two halves. A claim that NAMED no wait resolves to none on a
/// node that has none, and the wire still requires a positive `claimedWaitSeq`: the refusal names
/// its own sequence, which no wait can carry. A claim that DID name one is recorded against what
/// it named, exactly like every other refusal.
#[test]
fn a_claim_on_a_node_that_is_not_waiting_is_refused_not_waiting() {
    let (repository, _dir) = fresh_repository();
    let history = append_to(
        &repository,
        vec![
            started_event(),
            outcome_event("dispatch", Outcome::Started, NodeState::Queued),
        ],
    );
    let projection = replay(&scope(), STREAM, &history).unwrap();
    assert_ne!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput),
        "PRECONDITION: the node is not parked"
    );

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-not-waiting"),
        request(None, &[], &[]),
    )
    .unwrap();

    let own_sequence = appended[0].sequence;
    assert_eq!(
        own_sequence,
        history.len() as u64 + 1,
        "appended at the sequence the decision read"
    );
    assert_eq!(
        outcome,
        ClaimOutcome::Refused {
            reason_code: refusal::NOT_WAITING,
            wait_seq: own_sequence
        },
        "no wait was named and none is open, so the refusal names its own sequence"
    );
    let payload = refused_payload(&appended[0]);
    assert_eq!(payload.reason_code.as_str(), "not_waiting");
    assert_eq!(payload.claimed_wait_seq, own_sequence);
    let after = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        after.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Queued)
    );
    assert!(after.open_claims.is_empty());

    // The other half: a NAMED wait on a node that is not waiting is recorded against the name.
    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-not-waiting-named"),
        request(Some(1), &[], &[]),
    )
    .unwrap();
    assert_eq!(
        outcome,
        ClaimOutcome::Refused {
            reason_code: refusal::NOT_WAITING,
            wait_seq: 1
        }
    );
    assert_eq!(refused_payload(&appended[0]).claimed_wait_seq, 1);
}

#[test]
fn a_second_claim_on_a_claimed_wait_is_refused_duplicate_completion() {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let wait_seq = sequence_of(&history, history.len() - 1);

    let (first, _) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-first"),
        request(Some(wait_seq), &[], &[]),
    )
    .unwrap();
    assert!(
        matches!(first, ClaimOutcome::Claimed { .. }),
        "PRECONDITION: the first claim entered quarantine"
    );

    let (second, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-second"),
        request(Some(wait_seq), &[], &[]),
    )
    .unwrap();

    assert_eq!(
        second,
        ClaimOutcome::Refused {
            reason_code: refusal::DUPLICATE_COMPLETION,
            wait_seq
        }
    );
    assert_eq!(
        refused_payload(&appended[0]).reason_code.as_str(),
        "duplicate_completion"
    );
    let after = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        after.open_claims.len(),
        1,
        "the refusal minted no second claim"
    );
}

#[test]
fn a_claim_missing_a_declared_proof_kind_is_refused_evidence_budget_unmet_and_an_extra_kind_is_not_stronger()
 {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let wait_seq = sequence_of(&history, history.len() - 1);
    let required: Vec<String> = vec!["test_report".to_owned(), "diff".to_owned()];

    let (short, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-short"),
        request(Some(wait_seq), &["test_report"], &required),
    )
    .unwrap();
    assert_eq!(
        short,
        ClaimOutcome::Refused {
            reason_code: refusal::EVIDENCE_BUDGET_UNMET,
            wait_seq
        }
    );
    assert_eq!(
        refused_payload(&appended[0]).reason_code.as_str(),
        "evidence_budget_unmet"
    );

    // Every declared kind present plus one nobody asked for: the extra is accepted, never
    // counted as stronger proof, and never a reason to refuse.
    let (full, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-full"),
        request(
            Some(wait_seq),
            &["test_report", "diff", "screenshot"],
            &required,
        ),
    )
    .unwrap();
    assert_eq!(
        full,
        ClaimOutcome::Claimed {
            claim_seq: appended[0].sequence,
            wait_seq
        }
    );
}

/// Every code the verb names is a member of the frozen registry and a legal `SafeCode`. This
/// checks membership only: that the registry holds nothing the verb cannot produce is not a
/// property of the verb, and this cell does not claim it.
#[test]
fn every_refusal_code_the_verb_produces_is_in_the_frozen_vocabulary() {
    for code in [
        refusal::NOT_WAITING,
        refusal::STALE_RENDEZVOUS,
        refusal::UNKNOWN_WAIT,
        refusal::DUPLICATE_COMPLETION,
        refusal::EVIDENCE_BUDGET_UNMET,
    ] {
        assert!(
            graphhelm_protocols::REFUSAL_REASON_CODES.contains(&code),
            "{code} is not in REFUSAL_REASON_CODES"
        );
        assert!(
            SafeCode::parse(code).is_ok(),
            "{code} must be a legal SafeCode, or the verb would panic at the append"
        );
    }
}

#[test]
fn a_clearance_with_the_claimed_bundles_digest_clears_and_releases_the_node() {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let wait_seq = sequence_of(&history, history.len() - 1);
    let bundle = vec![evidence("test_report")];

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-1"),
        request(Some(wait_seq), &["test_report"], &[]),
    )
    .unwrap();
    let claim_seq = appended[0].sequence;
    assert_eq!(
        outcome,
        ClaimOutcome::Claimed {
            claim_seq,
            wait_seq
        }
    );

    let digest = claim_evidence_digest(&bundle);
    let (verdict, appended) = clear(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("clear-1"),
        claim_seq,
        &digest,
    )
    .unwrap();

    assert_eq!(verdict, ClearanceOutcome::Cleared);
    assert!(matches!(appended[0].kind, EventKind::CompletionCleared(_)));
    let after = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        after.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Succeeded),
        "clearance is the release"
    );
    assert!(after.open_claims.is_empty());
    assert!(!after.open_waits.contains_key(CUSTOMS_NODE));
}

#[test]
fn a_clearance_with_a_foreign_digest_is_journaled_as_rejected_and_spends_the_claim() {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let wait_seq = sequence_of(&history, history.len() - 1);

    let (_, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-1"),
        request(Some(wait_seq), &["test_report"], &[]),
    )
    .unwrap();
    let claim_seq = appended[0].sequence;

    let foreign = WireHash::parse(format!("sha256:{}", "f".repeat(64))).unwrap();
    assert_ne!(
        foreign,
        claim_evidence_digest(&[evidence("test_report")]),
        "PRECONDITION: the presented digest is not the bundle's"
    );
    let (verdict, appended) = clear(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("clear-wrong"),
        claim_seq,
        &foreign,
    )
    .unwrap();

    assert_eq!(
        verdict,
        ClearanceOutcome::Refused {
            reason_code: SafeCode::parse("hash_mismatch").unwrap()
        }
    );
    assert!(matches!(appended[0].kind, EventKind::CompletionCleared(_)));
    let after = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        after.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput),
        "the node stays parked"
    );
    assert!(after.open_claims.is_empty(), "the claim is spent");

    // The wait survives, so a NEW claim is accepted afterwards: a refused countersignature costs
    // the claimant their testimony, not their turn.
    let (again, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-2"),
        request(Some(wait_seq), &["test_report"], &[]),
    )
    .unwrap();
    assert_eq!(
        again,
        ClaimOutcome::Claimed {
            claim_seq: appended[0].sequence,
            wait_seq
        }
    );
}

/// REFUSED WITHOUT APPENDING: the fold reads a clearance with no claim under it as Corrupt, so a
/// verb that journaled one would poison every later replay of this stream.
///
/// Two shapes of "not an open claim": a sequence that was never a claim at all, and a claim that
/// WAS open until a rejected clearance spent it. The second is the one an operator retrying a
/// failed clear actually produces, and it must be refused the same way — a second clearance on a
/// spent claim would be a second record under one claim, which the fold cannot read either.
#[test]
fn a_clearance_naming_a_sequence_that_is_not_an_open_claim_appends_nothing() {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let wait_seq = sequence_of(&history, history.len() - 1);
    let before = read_all(&repository).len();

    let digest = claim_evidence_digest(&[]);
    let result = clear(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("clear-999"),
        999,
        &digest,
    );

    assert!(
        matches!(result, Err(ClearError::NotAnOpenClaim { claim_seq: 999 })),
        "expected NotAnOpenClaim {{ 999 }}, got {result:?}"
    );
    assert_eq!(
        read_all(&repository).len(),
        before,
        "head sequence unchanged: nothing was appended"
    );
    // And the stream still replays: the refusal left no uninterpretable record behind.
    replay(&scope(), STREAM, &read_all(&repository)).unwrap();

    // The other shape: a claim that a foreign digest has already SPENT.
    let (_, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-1"),
        request(Some(wait_seq), &["test_report"], &[]),
    )
    .unwrap();
    let claim_seq = appended[0].sequence;
    let foreign = WireHash::parse(format!("sha256:{}", "f".repeat(64))).unwrap();
    let (verdict, _) = clear(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("clear-wrong"),
        claim_seq,
        &foreign,
    )
    .unwrap();
    assert!(
        matches!(verdict, ClearanceOutcome::Refused { .. }),
        "PRECONDITION: the first clearance was rejected and spent the claim, got {verdict:?}"
    );
    let spent = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert!(
        !spent.open_claims.contains_key(&claim_seq),
        "PRECONDITION: the rejected claim is no longer open"
    );
    let before = read_all(&repository).len();

    let again = clear(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("clear-again"),
        claim_seq,
        &claim_evidence_digest(&[evidence("test_report")]),
    );

    assert!(
        matches!(again, Err(ClearError::NotAnOpenClaim { claim_seq: seq }) if seq == claim_seq),
        "a spent claim is not an open claim: expected NotAnOpenClaim {{ {claim_seq} }}, got {again:?}"
    );
    assert_eq!(
        read_all(&repository).len(),
        before,
        "the right digest arriving after the claim was spent still appends nothing"
    );
}

#[test]
fn a_verb_on_a_stream_with_no_execution_is_not_started() {
    let (repository, _dir) = fresh_repository();
    assert!(
        read_all(&repository).is_empty(),
        "PRECONDITION: the stream is empty"
    );

    let claimed = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-none"),
        request(None, &[], &[]),
    );
    assert!(
        matches!(claimed, Err(ClaimError::NotStarted)),
        "expected ClaimError::NotStarted, got {claimed:?}"
    );

    let cleared = clear(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("clear-none"),
        1,
        &claim_evidence_digest(&[]),
    );
    assert!(
        matches!(cleared, Err(ClearError::NotStarted)),
        "expected ClearError::NotStarted, got {cleared:?}"
    );
    assert!(
        read_all(&repository).is_empty(),
        "neither verb appended to a stream that has no execution"
    );
}

/// ADJACENT-PAIR ORDER, first pair: `duplicate_completion` is decided BEFORE
/// `evidence_budget_unmet`. A second claim on an already-claimed node is refused as a duplicate
/// even when its evidence is also short of the budget — the arrangement satisfies both arms,
/// and the code the journal carries is the one the table reaches first.
#[test]
fn a_short_claim_on_an_already_claimed_node_is_refused_duplicate_completion_not_evidence_budget_unmet()
 {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let wait_seq = sequence_of(&history, history.len() - 1);
    let required: Vec<String> = vec!["test_report".to_owned()];

    let (first, _) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-first"),
        request(Some(wait_seq), &["test_report"], &required),
    )
    .unwrap();
    assert!(
        matches!(first, ClaimOutcome::Claimed { .. }),
        "PRECONDITION: an open claim names the node"
    );
    let short = request(Some(wait_seq), &[], &required);
    assert!(
        short.evidence.is_empty() && !required.is_empty(),
        "PRECONDITION: the second claim presents less than the declared budget, so the \
         evidence arm would fire if it were reached"
    );

    let (second, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-second-short"),
        short,
    )
    .unwrap();

    assert_eq!(
        second,
        ClaimOutcome::Refused {
            reason_code: refusal::DUPLICATE_COMPLETION,
            wait_seq
        }
    );
    assert_eq!(
        refused_payload(&appended[0]).reason_code.as_str(),
        "duplicate_completion"
    );
}

/// ADJACENT-PAIR ORDER, second pair: `stale_rendezvous` is decided BEFORE
/// `duplicate_completion`. A claim that names a superseded wait on a node that ALSO has an open
/// claim is refused for the wait it named, not for the claim it would have duplicated — the wait
/// resolution runs first, and a stale name never reaches the duplicate check.
#[test]
fn a_stale_wait_named_on_a_node_with_an_open_claim_is_refused_stale_rendezvous_not_duplicate_completion()
 {
    let (repository, _dir) = fresh_repository();
    let mut batch = parked_batch();
    let first_wait_index = batch.len() - 1;
    batch.push(outcome_event(
        "park-again",
        Outcome::NeedsInput,
        NodeState::WaitingInput,
    ));
    let history = append_to(&repository, batch);
    let first_wait = sequence_of(&history, first_wait_index);
    let open_wait = sequence_of(&history, history.len() - 1);

    let (opened, _) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-open"),
        request(Some(open_wait), &[], &[]),
    )
    .unwrap();
    assert!(
        matches!(opened, ClaimOutcome::Claimed { .. }),
        "PRECONDITION: an open claim names the node, so the duplicate arm would fire if reached"
    );
    let projection = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_ne!(
        projection.open_waits[CUSTOMS_NODE].at_sequence, first_wait,
        "PRECONDITION: the first wait is superseded"
    );
    assert!(
        projection.customs_scans[CUSTOMS_NODE]
            .iter()
            .any(|scan| scan.at_sequence == first_wait),
        "PRECONDITION: the first wait is a scan of this node, so it is stale rather than unknown"
    );

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-stale-on-claimed"),
        request(Some(first_wait), &[], &[]),
    )
    .unwrap();

    assert_eq!(
        outcome,
        ClaimOutcome::Refused {
            reason_code: refusal::STALE_RENDEZVOUS,
            wait_seq: first_wait
        }
    );
    assert_eq!(
        refused_payload(&appended[0]).reason_code.as_str(),
        "stale_rendezvous"
    );
    let after = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        after.open_claims.len(),
        1,
        "the refusal minted no second claim"
    );
}

/// NON-ADJACENT ORDER, first pair: `not_waiting` is decided BEFORE `stale_rendezvous`.
///
/// The shape is reachable with nothing but the state machine's own arms: a node parks (minting a
/// wait AND recording a `parked` scan at that sequence), then leaves `WaitingInput`, which closes
/// the wait and leaves the scan history standing. Naming the superseded park now satisfies BOTH
/// arms — the node is not waiting, and the named sequence is a park of this node — and the journal
/// must carry the one the table reaches first.
#[test]
fn a_stale_wait_named_on_a_node_that_left_waiting_input_is_refused_not_waiting_not_stale_rendezvous()
 {
    let (repository, _dir) = fresh_repository();
    let mut batch = parked_batch();
    let park_index = batch.len() - 1;
    batch.push(outcome_event(
        "resume",
        Outcome::Started,
        NodeState::Running,
    ));
    let history = append_to(&repository, batch);
    let park_seq = sequence_of(&history, park_index);

    let projection = replay(&scope(), STREAM, &history).unwrap();
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Running),
        "PRECONDITION: the node left WaitingInput, so the not_waiting arm is armed"
    );
    assert!(
        projection.customs_scans[CUSTOMS_NODE]
            .iter()
            .any(|scan| scan.stage == CustomsStage::Parked && scan.at_sequence == park_seq),
        "PRECONDITION: the named sequence IS a park of this node, so the stale_rendezvous arm \
         would fire if it were reached — an unknown_wait here would prove nothing about order"
    );
    assert!(
        !projection.open_waits.contains_key(CUSTOMS_NODE),
        "PRECONDITION: the park is superseded — no open wait can resolve the name"
    );

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-parked-then-left"),
        request(Some(park_seq), &[], &[]),
    )
    .unwrap();

    assert_eq!(
        outcome,
        ClaimOutcome::Refused {
            reason_code: refusal::NOT_WAITING,
            wait_seq: park_seq
        }
    );
    assert_eq!(
        refused_payload(&appended[0]).reason_code.as_str(),
        "not_waiting"
    );
}

/// NON-ADJACENT ORDER, second pair: `not_waiting` is decided BEFORE `duplicate_completion`.
///
/// Reachability is the whole cell, and it rests on two absences in the fold. A
/// `node_outcome_recorded` that moves the node out of `WaitingInput` closes the wait and does NOT
/// remove the node's open claim; a `dlq_returned` MINTS a wait keyed by its own sequence and does
/// NOT touch `node_states`. Together they put the node outside `WaitingInput` while it holds both
/// an open wait the claim can name and an open claim that would duplicate.
#[test]
fn a_duplicate_claim_on_a_node_that_left_waiting_input_is_refused_not_waiting_not_duplicate_completion()
 {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let first_wait = sequence_of(&history, history.len() - 1);

    let (first, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-first"),
        request(Some(first_wait), &[], &[]),
    )
    .unwrap();
    assert!(
        matches!(first, ClaimOutcome::Claimed { .. }),
        "PRECONDITION: an open claim names the node"
    );
    let claim_seq = appended[0].sequence;

    // Out of WaitingInput (the wait closes, the claim does not), then dead-lettered and returned:
    // the return mints a NEW wait without putting the node back into WaitingInput.
    let tail = append_at(
        &repository,
        claim_seq + 1,
        vec![
            outcome_event("resume", Outcome::Started, NodeState::Running),
            dlq_routed_event("dlq-out", claim_seq),
            dlq_returned_event("dlq-back", claim_seq + 2),
        ],
    );
    let returned_wait = tail[tail.len() - 1].sequence;

    let projection = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Running),
        "PRECONDITION: the node is outside WaitingInput, so the not_waiting arm is armed"
    );
    assert_eq!(
        projection
            .open_waits
            .get(CUSTOMS_NODE)
            .map(|wait| wait.at_sequence),
        Some(returned_wait),
        "PRECONDITION: the return minted an open wait, so the name below RESOLVES and the \
         duplicate arm is the next one the table would reach"
    );
    assert!(
        projection
            .open_claims
            .values()
            .any(|open| open.node == CUSTOMS_NODE),
        "PRECONDITION: the node still holds an open claim, so the duplicate arm would fire"
    );

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-duplicate-not-waiting"),
        request(Some(returned_wait), &[], &[]),
    )
    .unwrap();

    assert_eq!(
        outcome,
        ClaimOutcome::Refused {
            reason_code: refusal::NOT_WAITING,
            wait_seq: returned_wait
        }
    );
    assert_eq!(
        refused_payload(&appended[0]).reason_code.as_str(),
        "not_waiting"
    );
    let after = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        after.open_claims.len(),
        1,
        "the refusal minted no second claim"
    );
}

/// NON-ADJACENT ORDER, third pair: `not_waiting` is decided BEFORE `evidence_budget_unmet`.
///
/// The same `dlq_returned` reachability as the cell above, without the first claim: the node holds
/// an open wait outside `WaitingInput`, and the claim presents nothing against a declared budget.
/// The evidence arm is the LAST in the table, so a cell whose name failed to resolve would prove
/// nothing — the wait is asserted resolvable before the refusal is demanded.
#[test]
fn a_short_claim_on_a_node_that_left_waiting_input_is_refused_not_waiting_not_evidence_budget_unmet()
 {
    let (repository, _dir) = fresh_repository();
    let history = append_to(&repository, parked_batch());
    let park_seq = sequence_of(&history, history.len() - 1);

    let tail = append_at(
        &repository,
        park_seq + 1,
        vec![
            outcome_event("resume", Outcome::Started, NodeState::Running),
            dlq_routed_event("dlq-out", park_seq),
            dlq_returned_event("dlq-back", park_seq + 2),
        ],
    );
    let returned_wait = tail[tail.len() - 1].sequence;
    let required: Vec<String> = vec!["test_report".to_owned()];

    let projection = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Running),
        "PRECONDITION: the node is outside WaitingInput, so the not_waiting arm is armed"
    );
    assert_eq!(
        projection
            .open_waits
            .get(CUSTOMS_NODE)
            .map(|wait| wait.at_sequence),
        Some(returned_wait),
        "PRECONDITION: the return minted an open wait, so the name below RESOLVES"
    );
    assert!(
        projection.open_claims.is_empty(),
        "PRECONDITION: no open claim, so duplicate_completion cannot be the reason and the \
         evidence arm is the next one the table would reach"
    );

    let short = request(Some(returned_wait), &[], &required);
    assert!(
        short.evidence.is_empty() && !required.is_empty(),
        "PRECONDITION: nothing is presented against a declared budget, so the evidence arm \
         would fire if it were reached"
    );

    let (outcome, appended) = claim(
        &repository,
        &scope(),
        STREAM,
        &actor(),
        &key("claim-short-not-waiting"),
        short,
    )
    .unwrap();

    assert_eq!(
        outcome,
        ClaimOutcome::Refused {
            reason_code: refusal::NOT_WAITING,
            wait_seq: returned_wait
        }
    );
    assert_eq!(
        refused_payload(&appended[0]).reason_code.as_str(),
        "not_waiting"
    );
}

/// A SOURCE INVARIANT no runtime cell can observe: a second store read between the replay and the
/// write would let a decision land on a stream that moved in between — and the journal would look
/// exactly like one that did not, so no test on the journal can catch it.
///
/// What this cell CHECKS is narrower than that invariant: it forbids ONE spelling, the
/// `next_sequence(` call that is today's only way to ask the store for a head. Some future
/// spelling of "read the head again" would pass here, and this cell does not claim otherwise.
///
/// The positive control: the same text must contain the `append_atomic(` it is supposed to call,
/// so an empty or mis-addressed read cannot pass as "no forbidden call".
#[test]
fn the_verbs_append_at_the_sequence_they_read_and_never_ask_the_store_again() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("customs.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    assert!(
        source.contains("append_atomic("),
        "positive control: the verb module must call append_atomic( — an empty read cannot pass"
    );
    assert!(
        !source.contains("next_sequence("),
        "the customs verb module must not spell next_sequence( — the one call that asks the \
         store for a head a second time. This forbids that spelling; it is not a proof that no \
         second read exists under another name"
    );
}
