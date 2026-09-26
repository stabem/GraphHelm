//! The sweep VERB: what `sweep(as_of)` actually writes, read back off a real journal.
//!
//! These are the R1-R8 cells of #162's design. They are deliberately NOT tests of the overdue
//! ARITHMETIC — `overdue_at` already has its own cells one layer down, and a second copy of that
//! oracle here would diverge in silence rather than loudly, which is the worse of the two failure
//! modes. Every cell in this file calls the real verb against a real repository and then reads the
//! real appended envelopes back. Nothing recomputes an expected value: if a fixture ever needs to
//! work out the answer for itself, the answer is written in the wrong place.
//!
//! WHY THE SETUP IS EXPENSIVE, recorded so the next reader does not "simplify" it away.
//!
//! A customs deadline is `entering event's occurred_at + the node's declared budget`, and the
//! budget is read from the SEALED TOPOLOGY of the published graph version. So a fixture that wants
//! any deadline at all must publish a version whose node carries a customs block, through the same
//! `validate_envelope` production uses — including its execution-id, actor and evidence-bijection
//! checks. That cost is why the #160 cells live in-module against the pure `overdue_at`: those
//! never append, so they never pay it. This lane's verb appends, so the front door is mandatory
//! and the cost is the price of admission rather than a choice.
//!
//! Measured, and it is the reason this file exists: before it, NO test in `core/events/tests`
//! published a graph version, so every integration fixture in this crate ran with
//! `deadline: None` on every stage. The whole deadline half of the fold had no coverage from
//! outside the module.

use std::collections::BTreeSet;
use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, PreparedAppend, SealedEvidence, WrappedKey};
use graphhelm_protocols::{
    ActorId, Clock, ContentSlot, EventEnvelope, EventKind, EvidenceReference, ExecutionId,
    ExecutionMode, ExecutionStarted, GraphVersionPublished, IdGenerator, NewEvent,
    NodeOutcome as Outcome, NodeOutcomeRecorded, NodeState, OpaqueId, PersistedActor,
    PersistedActorType, PersistedCustoms, PersistedGraphVersion, PersistedTimestamp, ProjectId,
    RawSha256, RepositoryScope, Sensitivity, SweepCaller, WorkspaceId,
};
use sha2::{Digest, Sha256};

const STREAM: &str = "stream-sweep-verb";
const NODE: &str = "start";
const EXECUTION: &str = "execution-fixture";

/// The budget the fixture's node declares for the CLAIMED stage, in seconds.
///
/// Named rather than inlined because later cells need to place an `as_of` on either side of the
/// horizon it produces, and a literal repeated at both ends is a second copy of the arithmetic
/// this file exists not to duplicate.
const CLEARANCE_BUDGET_SECONDS: u64 = 3600;

/// The first publication on a stream must be version 1 with no predecessor, and the conformance
/// document is not one -- it is version 2 with a predecessor.
const FIRST_VERSION: u64 = 1;

/// A clock the fixture can move, because a FROZEN one makes this file's central question
/// unaskable.
///
/// Every cell here needs a stage to have LAPSED, and a deadline is the entering event's instant
/// plus a budget — so the instant asked about is always after the instant the events were written.
/// Under a frozen clock that means every sweep asks about the future, and "only the past is
/// askable" could never be true of any cell. The arrangement is written at 12:00 and the clock is
/// then moved to 15:00, so a sweep at 14:00 is a question about the PAST, which is what an honest
/// sweep is.
struct TestClock(AtomicI64);

impl TestClock {
    fn at(hour: u32) -> Self {
        Self(AtomicI64::new(
            Utc.with_ymd_and_hms(2026, 8, 10, hour, 0, 0)
                .unwrap()
                .timestamp(),
        ))
    }

    /// Move to an arbitrary instant, for the cell that needs a year rather than an hour.
    fn advance_to_instant(&self, at: &PersistedTimestamp) {
        self.0.store(at.as_datetime().timestamp(), Ordering::SeqCst);
    }

    fn advance_to(&self, hour: u32) {
        self.0.store(
            Utc.with_ymd_and_hms(2026, 8, 10, hour, 0, 0)
                .unwrap()
                .timestamp(),
            Ordering::SeqCst,
        );
    }
}

impl Clock for TestClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(self.0.load(Ordering::SeqCst), 0)
            .expect("the fixture's instants are representable")
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
        Some(ExecutionId::parse(EXECUTION).unwrap()),
    )
}

fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-test").unwrap(),
    )
}

fn event(key: &str, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

fn instant(text: &str) -> PersistedTimestamp {
    PersistedTimestamp::from_datetime(
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc),
    )
    .unwrap()
}

/// A publication of the CONFORMANCE graph version, with customs injected into its node.
///
/// Built from `conformance/schemas/valid/persisted-graph-version.json` rather than hand-assembled,
/// and that is a measurement rather than taste. A hand-built topology whose node declares NO
/// content slots is REJECTED by `validate_persisted_projection` — measured by stripping the slots
/// off the conformance version, rehashing, and watching a version that had just validated stop
/// validating. There is no cheap slotless shortcut: any fixture in this lane that wants a deadline
/// must carry real slots and real sealed evidence.
///
/// The postgres test-support module used to contain exactly such a hand-built slotless
/// publication, `graph_published_event`, with ZERO callers — so nothing had ever run it through a
/// validator, and it did not pass one. Lifting from it is what cost this lane its first red runs:
/// sitting in a conformance test-support file reads as "this shape is known good", and that is a
/// claim nobody had made.
///
/// **It was deleted in #266, and this paragraph is updated in the same commit** rather than left
/// describing a function that no longer exists. The class it belonged to is now guarded instead of
/// narrated: `apps/cli/tests/test_support_has_no_dead_helpers.rs` fails on any `pub fn` in a
/// test-support module that nothing calls. A note like this one is a claim; that sweep is a check.
///
/// ONE thing is patched before anything is hashed — the node's customs block, which is the entire
/// point. Order matters after that: the hashes are recomputed over the patched topology FIRST,
/// because the publication evidence id is derived from the semantic hash, and deriving it from the
/// stale one would mint ids the bijection rejects.
///
/// The execution id is NOT patched; the fixture adopts the conformance document's own
/// (`execution-fixture`) instead, because the scope and the topology must name the same execution
/// and adopting is one fewer thing to keep in step.
///
/// The version number and predecessor ARE overridden, to 1 and none. The conformance document is a
/// SUCCESSOR — number 2, with a predecessor — and the first publication on a stream must be
/// version 1 with no predecessor or `validate_graph_lineage` refuses it. That refusal arrives as
/// the same bare `Invalid` every other rule uses, after every hand-callable validator has already
/// said `Ok`, which is why it took an afternoon to find.
fn version_and_evidence(
    customs: Option<PersistedCustoms>,
) -> (PersistedGraphVersion, Vec<SealedEvidence>) {
    fn push(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        output.extend_from_slice(value);
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the workspace root is two levels above core/events");
    let text = std::fs::read_to_string(
        root.join("conformance/schemas/valid/persisted-graph-version.json"),
    )
    .expect("the conformance fixture must be readable");

    let mut document: serde_json::Value =
        serde_json::from_str(&text).expect("the conformance fixture must parse");
    let node = document["topology"]["nodes"]
        .get_mut(NODE)
        .expect("the conformance topology must contain the fixture's node");
    match &customs {
        Some(value) => node["customs"] = serde_json::to_value(value).unwrap(),
        None => {
            node.as_object_mut().unwrap().remove("customs");
        }
    }

    let patched: PersistedGraphVersion =
        serde_json::from_value(document).expect("the patched version must deserialise");
    let hashes = graphhelm_graph::persisted_hashes(patched.topology(), patched.content_slots())
        .expect("the patched topology must hash");

    let slots = patched
        .content_slots()
        .iter()
        .map(|slot| {
            ContentSlot::new(
                slot.slot_id().clone(),
                slot.owner_kind(),
                slot.owner_id().clone(),
                slot.field_kind(),
                slot.ordinal(),
                graphhelm_graph::derive_publication_evidence_id(
                    &scope(),
                    FIRST_VERSION,
                    hashes.semantic_hash(),
                    slot,
                )
                .unwrap(),
                slot.content_sha256().clone(),
                slot.sensitivity(),
                slot.required_for_execution(),
            )
        })
        .collect::<Vec<_>>();

    // NUMBER 1 AND NO PREDECESSOR, overriding the conformance document, which is a SUCCESSOR:
    // it carries number 2 and a predecessor. Appending it into an empty stream is refused by the
    // lineage rule -- the first publication on a stream must be version 1 with no predecessor.
    // The postgres recipe this fixture was adapted from passes 1/None explicitly for the same
    // reason; that override was the part not carried across, and it cost this lane an afternoon of
    // reading validators that were all innocent.
    let version = PersistedGraphVersion::new(
        FIRST_VERSION,
        None,
        patched.topology().clone(),
        hashes.topology_hash().clone(),
        hashes.semantic_hash().clone(),
        slots,
        patched.created_by().clone(),
        patched.created_at().clone(),
    )
    .expect("the republished version must build");

    let evidence = version
        .content_slots()
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let ciphertext = vec![u8::try_from(index + 1).unwrap(); 16];
            let reference = EvidenceReference::new(
                slot.evidence_id().clone(),
                slot.content_sha256().clone(),
                RawSha256::parse(hex::encode(Sha256::digest(&ciphertext))).unwrap(),
            );
            let scope = scope();
            let mut aad = Vec::new();
            push(&mut aad, b"graphhelm-evidence-aad-v1");
            push(&mut aad, scope.workspace_id().as_str().as_bytes());
            push(&mut aad, scope.project_id().as_str().as_bytes());
            aad.push(1);
            push(&mut aad, scope.execution_id().unwrap().as_str().as_bytes());
            for value in [
                slot.evidence_id().as_str(),
                "1.0.0",
                "application/json",
                match slot.sensitivity() {
                    Sensitivity::Public => "public",
                    Sensitivity::Internal => "internal",
                    Sensitivity::Confidential => "confidential",
                    Sensitivity::Restricted => "restricted",
                },
                "standard",
                slot.content_sha256().as_str(),
            ] {
                push(&mut aad, value.as_bytes());
            }
            let wrapped = WrappedKey::new(
                "key-1",
                slot.evidence_id().as_str(),
                "xchacha20poly1305",
                vec![1; 24],
                vec![2; 48],
                RawSha256::parse(hex::encode(Sha256::digest(&aad))).unwrap(),
            )
            .unwrap();
            SealedEvidence::new(
                reference,
                scope,
                "application/json",
                slot.sensitivity(),
                "standard",
                "xchacha20poly1305",
                vec![3; 24],
                ciphertext,
                wrapped,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();

    (version, evidence)
}

fn outcome_event(key: &str, outcome: Outcome, next_state: NodeState) -> NewEvent {
    event(
        key,
        EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
            node_id: OpaqueId::parse(NODE).unwrap(),
            outcome,
            next_state,
            executor: None,
            reason: None,
        }),
    )
}

fn claim_event(key: &str, completes_wait_seq: u64) -> NewEvent {
    event(
        key,
        EventKind::CompletionClaimed(graphhelm_protocols::CompletionClaimed {
            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
            node: OpaqueId::parse(NODE).unwrap(),
            completes_wait_seq,
            evidence: vec![],
            attestation: graphhelm_protocols::ClaimAttestation {
                asserter: OpaqueId::parse("agent-claimer").unwrap(),
                mode: graphhelm_protocols::ClaimAttestationMode::OperatorAttested,
            },
        }),
    )
}

/// A repository holding: a published version with `customs`, a node parked, then that wait claimed.
///
/// Returns the temp directory (kept alive), the repository, and the appended envelopes — so a cell
/// can name an episode by the sequence the JOURNAL gave it rather than by one predicted here.
fn fixture(
    customs: Option<PersistedCustoms>,
    tail: impl FnOnce(u64) -> Vec<NewEvent>,
) -> (
    tempfile::TempDir,
    LocalEventRepository,
    Vec<EventEnvelope>,
    Arc<TestClock>,
) {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::at(12));
    let repository =
        LocalEventRepository::open(directory.path(), clock.clone(), Arc::new(Ids::default()))
            .unwrap();

    let (version, evidence) = version_and_evidence(customs);

    // The execution declares the graph it is running, and the store checks that declaration
    // against the version actually published on the stream. A placeholder hash here is accepted
    // by `validate_envelope` and refused by the append — the same bare `Invalid`, from a
    // completely different rule. So the hash is taken FROM the version rather than invented, and
    // the version is built before the execution event that names it.
    let graph_hash = version.topology_hash().clone();
    let graph_version = version.number();
    // The publication is the ONE event here that does not use `actor()`: `validate_envelope`
    // requires the envelope's actor to equal the version's own `createdBy`.
    let published = NewEvent::new(
        OpaqueId::parse("graph-published").unwrap(),
        version.created_by().clone(),
        Sensitivity::Internal,
        EventKind::GraphVersionPublished(Box::new(GraphVersionPublished { version })),
        evidence
            .iter()
            .map(|item| item.reference().clone())
            .collect(),
        vec![],
    );

    let mut batch = vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                graph_version,
                graph_hash,
                mode: ExecutionMode::Supervised,
            }),
        ),
        published,
        outcome_event("dispatch", Outcome::Started, NodeState::Queued),
        outcome_event("run", Outcome::Started, NodeState::Running),
        outcome_event("park", Outcome::NeedsInput, NodeState::WaitingInput),
    ];
    // The park is the last SHARED event, so its sequence is the batch length. Predicted here and
    // CHECKED below once the batch is appended -- the tail names episodes by this number, so an
    // unchecked prediction would make every cell's identity claim rest on arithmetic nobody ran.
    let wait_sequence = batch.len() as u64;
    batch.extend(tail(wait_sequence));

    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        1,
        batch,
        evidence,
        vec![],
    )
    .unwrap();
    let appended = repository.append_atomic(&request).unwrap();

    // CHECKED, not assumed: every cell below names an episode by a position in this batch, and
    // the identity model rests on a fresh stream numbering 1..n in order.
    for (index, envelope) in appended.iter().enumerate() {
        assert_eq!(
            envelope.sequence,
            index as u64 + 1,
            "batch position {index} did not become sequence {}",
            index + 1
        );
    }
    // The arrangement is now in the past. Every cell asks about 14:00, which is after the
    // deadlines and BEFORE this instant, so each is a question about what already happened.
    clock.advance_to(15);

    (directory, repository, appended, clock)
}

/// The shared fixture with a CLAIM on the parked wait: the arrangement R1 and R4 need.
fn claimed_fixture(
    customs: Option<PersistedCustoms>,
) -> (
    tempfile::TempDir,
    LocalEventRepository,
    Vec<EventEnvelope>,
    Arc<TestClock>,
) {
    fixture(customs, |wait_sequence| {
        vec![claim_event("claim", wait_sequence)]
    })
}

/// R1 — a claimed episode whose clearance never came, past its instant, is swept.
///
/// The claim ENTERS the claimed stage, so its clock is the clearance budget starting at the
/// claim's own `occurred_at`. Nothing clears it; the sweep is asked about an instant beyond that
/// horizon; exactly one exception must exist, and it must name the CLAIM's episode.
///
/// NAMED SABOTAGE: exempt `Claimed` from the overdue computation — the state most likely to be
/// waved through as "already being handled by someone".
/// FALLS AT: the exception-count assertion, which reads zero. Not at the stage check and not at
/// the sweep's own bookkeeping, both of which survive the sabotage untouched.
#[test]
fn a_claim_past_its_clearance_instant_raises_exactly_one_exception_naming_its_episode() {
    let (_directory, repository, appended, _clock) = claimed_fixture(Some(PersistedCustoms::new(
        600,
        CLEARANCE_BUDGET_SECONDS,
        None,
    )));
    let claim_sequence = appended.last().unwrap().sequence;

    // The clock stands at 12:00:00Z, so the clearance horizon is 13:00:00Z. Asked an hour past it.
    let written = graphhelm_events::sweep(
        &repository,
        &scope(),
        STREAM,
        &instant("2026-08-10T14:00:00Z"),
        &actor(),
        SweepCaller::Operator,
        None,
    )
    .expect("a sweep over a readable journal must not fail");

    let exceptions: Vec<&graphhelm_protocols::OverdueException> = written
        .iter()
        .filter_map(|envelope| match &envelope.kind {
            EventKind::OverdueException(payload) => Some(payload),
            _ => None,
        })
        .collect();

    assert_eq!(
        exceptions.len(),
        1,
        "one lapsed claimed episode must raise exactly one exception; the sweep wrote {:?}",
        written
            .iter()
            .map(|envelope| envelope.kind.wire_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        exceptions[0].episode_sequence, claim_sequence,
        "the exception must name the CLAIM's episode, not the wait it answered"
    );
}

/// R4 — the same sweep asked twice at the same instant raises the exception once.
///
/// Both calls must RECORD: a sweep that ran and found nothing is not the same fact as a sweep that
/// never ran, and collapsing the two would make an idle tick indistinguishable from a dead one.
/// What must not repeat is the EXCEPTION. One per episode, ever — the alternative grain re-fires on
/// every tick and trains operators to ignore the channel, which is the cry-wolf failure this
/// project has already paid for once.
///
/// The prevention has to live in the FOLD, not in the append keys. Keys derived from `as_of` would
/// only cover a repeat at the identical instant; the sweep that matters runs every tick, at a NEW
/// instant each time, against the same lapsed episode. A key-based guard is green on this cell and
/// useless in production.
///
/// NAMED SABOTAGE: make the second sweep re-emit (stop recording the episode as excepted).
/// FALLS AT: the emptiness assertion on the SECOND run's exceptions. Not on the first run's, and
/// not on the sweep-record count, both of which survive the sabotage unchanged.
#[test]
fn a_second_sweep_at_the_same_instant_records_again_but_raises_no_second_exception() {
    let (_directory, repository, appended, _clock) = claimed_fixture(Some(PersistedCustoms::new(
        600,
        CLEARANCE_BUDGET_SECONDS,
        None,
    )));
    let claim_sequence = appended.last().unwrap().sequence;
    let as_of = instant("2026-08-10T14:00:00Z");

    let first = sweep_at(&repository, &as_of);
    let second = sweep_at(&repository, &as_of);

    assert_eq!(
        exceptions_in(&first).len(),
        1,
        "the first sweep must raise the lapsed episode"
    );
    assert_eq!(
        exceptions_in(&first)[0].episode_sequence,
        claim_sequence,
        "and it must be the claim's episode"
    );

    assert!(
        exceptions_in(&second).is_empty(),
        "the second sweep must raise nothing: one exception per episode, ever. It wrote {:?}",
        second
            .iter()
            .map(|envelope| envelope.kind.wire_name())
            .collect::<Vec<_>>()
    );

    // Both RAN, and both said so. Asserted on the journal rather than on the return values,
    // because the return value is what the verb claims and the journal is what happened.
    let history = repository.read_replay_stream(&scope(), STREAM).unwrap();
    let records = history
        .iter()
        .filter(|envelope| matches!(envelope.kind, EventKind::SweepPerformed(_)))
        .count();
    assert_eq!(
        records, 2,
        "two sweeps ran, so the journal must carry two sweep records"
    );
}

/// The verb under this file's fixed actor and caller, so a cell states the QUESTION and not the
/// plumbing around it.
fn sweep_at(repository: &LocalEventRepository, as_of: &PersistedTimestamp) -> Vec<EventEnvelope> {
    graphhelm_events::sweep(
        repository,
        &scope(),
        STREAM,
        as_of,
        &actor(),
        SweepCaller::Operator,
        None,
    )
    .expect("a sweep over a readable journal must not fail")
}

/// The exceptions inside an appended batch, in the order the batch carries them.
fn exceptions_in(batch: &[EventEnvelope]) -> Vec<&graphhelm_protocols::OverdueException> {
    batch
        .iter()
        .filter_map(|envelope| match &envelope.kind {
            EventKind::OverdueException(payload) => Some(payload),
            _ => None,
        })
        .collect()
}

fn dlq_routed_event(key: &str, episode_sequence: u64) -> NewEvent {
    event(
        key,
        EventKind::DlqRouted(graphhelm_protocols::DlqRouted {
            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
            node_id: OpaqueId::parse(NODE).unwrap(),
            episode_sequence,
            reason: graphhelm_protocols::SafeCode::parse("stalled").unwrap(),
        }),
    )
}

fn dlq_returned_event(key: &str, dlq_episode_sequence: u64, wait_within: u64) -> NewEvent {
    event(
        key,
        EventKind::DlqReturned(graphhelm_protocols::DlqReturned {
            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
            node_id: OpaqueId::parse(NODE).unwrap(),
            dlq_episode_sequence,
            wait_within_seconds: Some(wait_within),
        }),
    )
}

/// R2 — the wait a DLQ return REOPENS is a new episode, and it is that one the sweep names.
///
/// The return does not resume the old wait; it opens a new one, anchored to its own instant and
/// budgeted by its own `waitWithinSeconds`. So the exception must carry the RETURN's sequence.
///
/// NAMED SABOTAGE: carry the original wait's identity through the return — the plausible
/// implementation, "the wait is the same wait".
/// FALLS AT: the episode-sequence assertion. The exception exists under the sabotage too, so a
/// cell that only counted exceptions would go green on it. That is exactly why this one asserts
/// the sequence and R1 asserts the count: two cells, two properties, and neither covers the other.
#[test]
fn a_dlq_return_reopens_the_wait_as_a_new_episode_and_the_sweep_names_that_one() {
    // The node's own budget is 600s and the RETURN declares 1800s, deliberately different: if the
    // two agreed, a sabotage that carried the old wait through would produce the same deadline and
    // the only surviving difference would be the sequence. Keeping them apart means the fixture
    // does not depend on the assertion being the sharpest possible one.
    let (_directory, repository, appended, _clock) = fixture(
        Some(PersistedCustoms::new(600, CLEARANCE_BUDGET_SECONDS, None)),
        |wait_sequence| {
            vec![
                dlq_routed_event("routed", wait_sequence),
                dlq_returned_event("returned", wait_sequence, 1800),
            ]
        },
    );

    let returned_sequence = appended.last().unwrap().sequence;
    let original_wait_sequence = returned_sequence - 2;

    let written = sweep_at(&repository, &instant("2026-08-10T14:00:00Z"));
    let raised = exceptions_in(&written);

    assert_eq!(
        raised.len(),
        1,
        "the reopened wait lapsed, so exactly one exception is due; the sweep wrote {:?}",
        written
            .iter()
            .map(|envelope| envelope.kind.wire_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        raised[0].episode_sequence, returned_sequence,
        "the exception must name the REOPENED episode (the return's own sequence), not the \
         original wait at {original_wait_sequence}"
    );
    assert_eq!(
        raised[0].stage,
        graphhelm_protocols::CustomsStage::Parked,
        "a reopened wait is parked, not claimed"
    );
}

fn dlq_redrive_event(key: &str, dlq_episode_sequence: u64) -> NewEvent {
    event(
        key,
        EventKind::DlqRedrive(graphhelm_protocols::DlqRedrive {
            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
            node_id: OpaqueId::parse(NODE).unwrap(),
            dlq_episode_sequence,
        }),
    )
}

/// R3 — a redrive starts a NEW episode, so the same node can lapse twice and be raised twice.
///
/// This is the grain decision made observable. "One exception per episode, ever" (R4) must not
/// harden into "one exception per node, ever": a node that was dead-lettered, put back into play,
/// and left to rot again has failed a SECOND time, and an operator who was told once and never
/// again would be told nothing about the second failure.
///
/// The redrive mints no new claim — the testimony stands. What restarts is the CLOCK, and the new
/// stage entry is the redrive's own envelope, which is what makes the second episode a different
/// episode.
///
/// NAMED SABOTAGE: key the "already raised" marker by node id instead of episode sequence.
/// FALLS AT: the count of the second sweep's exceptions, which reads zero where one is due. The
/// first sweep is unaffected, which is what separates this cell from R4 — R4 proves the marker
/// STOPS a repeat, this one proves it does not stop too much. Neither covers the other, and an
/// implementation with a node-keyed marker passes R4 and fails only here.
#[test]
fn a_redrive_starts_a_new_episode_that_can_lapse_and_be_raised_again() {
    let (_directory, repository, appended, clock) = claimed_fixture(Some(PersistedCustoms::new(
        600,
        CLEARANCE_BUDGET_SECONDS,
        None,
    )));
    let first_claim = appended.last().unwrap().sequence;
    let as_of = instant("2026-08-10T14:00:00Z");

    let first = sweep_at(&repository, &as_of);
    assert_eq!(
        exceptions_in(&first)
            .iter()
            .map(|payload| payload.episode_sequence)
            .collect::<Vec<_>>(),
        vec![first_claim],
        "the first lapse must raise the claim's episode"
    );

    // Dead-letter the claimed node and put it straight back. Appended AFTER the sweep on purpose:
    // the redrive has to be able to re-arm an episode the journal already excepted, which is the
    // whole question this cell asks.
    let routed = append_after(&repository, vec![dlq_routed_event("routed", first_claim)]);
    let redriven = append_after(
        &repository,
        vec![dlq_redrive_event(
            "redriven",
            routed.last().unwrap().sequence,
        )],
    );
    let redrive_sequence = redriven.last().unwrap().sequence;

    // The redrive was written at the fixture's current instant, so its clearance horizon is an
    // hour further on. Time has to move before that stage can be LAPSED -- and the second sweep
    // then asks about a moment that is again in the past, exactly as the first one did.
    clock.advance_to(18);
    let second = sweep_at(&repository, &instant("2026-08-10T17:00:00Z"));
    let raised = exceptions_in(&second);

    assert_eq!(
        raised.len(),
        1,
        "the redriven episode lapsed too, so it is due its own exception; the sweep wrote {:?}",
        second
            .iter()
            .map(|envelope| envelope.kind.wire_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        raised[0].episode_sequence, redrive_sequence,
        "and the second exception must name the REDRIVE's episode, not the first claim at \
         {first_claim} -- a marker keyed by node would have swallowed this one"
    );

    // And the grain still holds in the other direction: asked again, neither episode repeats.
    let third = sweep_at(&repository, &instant("2026-08-10T17:00:00Z"));
    assert!(
        exceptions_in(&third).is_empty(),
        "both episodes are spent, so a third sweep raises nothing; it wrote {:?}",
        third
            .iter()
            .map(|envelope| envelope.kind.wire_name())
            .collect::<Vec<_>>()
    );
}

/// Appends more events onto the fixture's stream, at whatever sequence it currently stands.
///
/// Cells that need a step to land AFTER a sweep cannot put it in the opening batch, and reading
/// the sequence rather than counting it keeps the fixture honest when a cell changes shape.
fn append_after(repository: &LocalEventRepository, events: Vec<NewEvent>) -> Vec<EventEnvelope> {
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        repository.next_sequence(&scope(), STREAM).unwrap(),
        events,
        vec![],
        vec![],
    )
    .unwrap();
    repository.append_atomic(&request).unwrap()
}

/// R6 — a wait that was NEVER claimed is swept exactly like a claimed one.
///
/// NAMED SABOTAGE: skip the wait arm of the overdue reckoning.
/// FALLS AT: the exception-count assertion, which reads zero.
///
/// MEASURED, AND IT CORRECTS THE REASON THIS CELL WAS PLANNED FOR. The design said a claims-only
/// implementation would pass every OTHER cell, so this one was the only thing standing between the
/// lane and the cheapest false green. That is not true: under the sabotage R2 falls as well,
/// because the episode a DLQ return reopens is a WAIT. The two are not independent on this arm.
///
/// The cell keeps its place on the narrower claim it can actually support: it is the only one that
/// covers the wait arm WITHOUT a dead-letter round-trip, so if the DLQ path is ever reshaped this
/// is what still holds the plain un-answered wait. Repeating the wide version now that the narrow
/// one has been measured would be restating a prediction the evidence already trimmed.
#[test]
fn a_wait_that_was_never_claimed_is_swept_like_any_other_lapsed_stage() {
    let (_directory, repository, appended, _clock) = fixture(
        Some(PersistedCustoms::new(600, CLEARANCE_BUDGET_SECONDS, None)),
        |_wait_sequence| vec![],
    );
    let wait_sequence = appended.last().unwrap().sequence;

    let written = sweep_at(&repository, &instant("2026-08-10T14:00:00Z"));
    let raised = exceptions_in(&written);

    assert_eq!(
        raised.len(),
        1,
        "an un-answered wait past its budget is overdue; the sweep wrote {:?}",
        written
            .iter()
            .map(|envelope| envelope.kind.wire_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        raised[0].episode_sequence, wait_sequence,
        "and the episode is the wait's own entry"
    );
    assert_eq!(
        raised[0].stage,
        graphhelm_protocols::CustomsStage::Parked,
        "a wait nobody claimed lapses PARKED"
    );
}

/// R7 — a node that declared no budget is never overdue, however long it sits.
///
/// Absence is absence. The trap this seals is a default of zero: a node with no customs block
/// would have its horizon at the instant it entered the stage, so every stage of every
/// pre-customs replay becomes instantly overdue and the journal fills with exceptions against work
/// nobody ever bounded — arriving as what looks like a flood of real findings.
///
/// NAMED SABOTAGE: treat a missing customs block as a zero budget.
/// FALLS AT: the emptiness assertion, which finds an exception where none is due.
#[test]
fn a_node_with_no_declared_budget_is_never_overdue() {
    let (_directory, repository, _appended, clock) = fixture(None, |wait_sequence| {
        vec![claim_event("claim", wait_sequence)]
    });

    // A YEAR after the stage was entered, not an hour: if a zero default ever creeps in, no choice
    // of instant hides it, and a reader can see the cell is not resting on a narrow window.
    //
    // The world has to have REACHED that year for the question to be askable at all -- R8's rule
    // is that only the past can be swept, and this cell was written before that rule existed. It
    // asked about 2027 from a clock standing in 2026 and was refused, which is R8 catching R7:
    // the refusal is not selective about which cell asks.
    let asked = instant("2027-08-10T12:00:00Z");
    clock.advance_to_instant(&instant("2027-08-10T13:00:00Z"));
    let written = sweep_at(&repository, &asked);

    assert!(
        exceptions_in(&written).is_empty(),
        "nobody bounded this stage, so nothing is overdue; the sweep wrote {:?}",
        written
            .iter()
            .map(|envelope| envelope.kind.wire_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        written.len(),
        1,
        "and the sweep still RECORDED: a reckoning that found nothing is not a reckoning that \
         never happened"
    );
}

/// R5 — the instant a sweep was asked about survives in the journal, and replay reads it back.
///
/// `as_of` is a QUESTION, not a timestamp of the run: the instant the sweep was asked to evaluate
/// at, which need not be the instant it ran. This cell is what makes that decision load-bearing
/// instead of decorative — the fixture asks about an instant TWO HOURS after the clock the events
/// were written with, so a value derived from any clock cannot coincide with the right answer.
///
/// NAMED SABOTAGE: journal a DERIVED instant instead of the argument the caller carried in. The
/// lapsed deadline is the nearest one to hand; a clock is not reachable from the verb at all,
/// which is itself part of the decision this cell defends.
/// FALLS AT: the assertion that the journal carries the ASKED instant. Measured: left 13:00,
/// right 14:00, and every other cell stays green.
///
/// The design predicted it would fall at the run-instant inequality instead. It does not, and the
/// prediction is corrected here rather than quietly dropped: a derived value can differ from the
/// run instant and still be wrong, so the inequality survives the sabotage while the equality does
/// not. The inequality is kept anyway — it is the half that would catch a value taken from a
/// clock, which is the sabotage the design had in mind and the one the verb's shape now forbids.
#[test]
fn the_asked_instant_is_journalled_and_is_not_the_instant_the_sweep_ran() {
    let (_directory, repository, _appended, _clock) = claimed_fixture(Some(PersistedCustoms::new(
        600,
        CLEARANCE_BUDGET_SECONDS,
        None,
    )));
    let asked = instant("2026-08-10T14:00:00Z");

    let written = sweep_at(&repository, &asked);
    let record = written
        .iter()
        .find_map(|envelope| match &envelope.kind {
            EventKind::SweepPerformed(payload) => Some((payload, envelope)),
            _ => None,
        })
        .expect("the sweep must record that it ran");

    assert_eq!(
        record.0.as_of, asked,
        "the journal must carry the instant that was ASKED about"
    );
    assert_ne!(
        record.0.as_of, record.1.occurred_at,
        "and it must not be the instant the sweep RAN -- these differ by two hours in this \
         fixture precisely so that a value taken from a clock cannot pass by coincidence"
    );

    // Replay reads the same journal and reaches the same verdict about what is spent. Asserted
    // through the verb rather than by inspecting the projection: the property is that a REPLAYED
    // journal does not re-raise, which is what a restarted process would do.
    let after_replay = sweep_at(&repository, &asked);
    assert!(
        exceptions_in(&after_replay).is_empty(),
        "the exception is spent in the journal, so re-reading it raises nothing"
    );
}

/// R8 — a sweep may not be asked about the future, and refusing must consume nothing.
///
/// A sweep does not predict. A future-dated answer would be indistinguishable from a real one in
/// the journal while permanently spending the episodes it touched — the exception would be marked,
/// and the honest sweep that came later would find nothing left to raise.
///
/// TWO SABOTAGES, because the design named one failure and the cell was built to catch a second.
///
/// (a) REMOVE THE REFUSAL. Falls at the refusal assertion, immediately.
///
/// (b) REFUSE, BUT ONLY AFTER APPENDING — the silent one. An implementation that returns an error
///     while having already written the batch looks correct from the call site: the caller sees
///     `Err`, and the episode is spent. Measured: it falls at the append-count assertion, left 8
///     right 6.
///
/// The design predicted (b) would fall on the LAST assertion — the honest sweep finding nothing
/// left to raise. It falls two assertions earlier, on the journal length, because that assertion
/// was added to this cell and the design did not have it. Both would catch it; the earlier one
/// fires first and says something narrower and more useful ("it wrote two events") than the later
/// one ("something was consumed"). The prediction is corrected rather than dropped.
///
/// The last assertion still earns its place: it is the only one that would catch a refusal that
/// marks the episode WITHOUT appending anything — no journal growth to count, and nothing but the
/// next honest sweep to notice.
#[test]
fn a_sweep_asked_about_the_future_is_refused_and_spends_nothing() {
    let (_directory, repository, appended, _clock) = claimed_fixture(Some(PersistedCustoms::new(
        600,
        CLEARANCE_BUDGET_SECONDS,
        None,
    )));
    let claim_sequence = appended.last().unwrap().sequence;

    // The fixture's clock stands at 15:00. This asks about 16:00.
    let refused = graphhelm_events::sweep(
        &repository,
        &scope(),
        STREAM,
        &instant("2026-08-10T16:00:00Z"),
        &actor(),
        SweepCaller::Operator,
        None,
    );
    assert!(
        refused.is_err(),
        "a sweep does not predict, so an instant after the appending one must be refused"
    );

    let history = repository.read_replay_stream(&scope(), STREAM).unwrap();
    assert_eq!(
        history.len(),
        appended.len(),
        "a refused sweep appends nothing at all"
    );

    // THE ASSERTION THAT MATTERS: the episode is still there to be raised.
    let honest = sweep_at(&repository, &instant("2026-08-10T14:00:00Z"));
    assert_eq!(
        exceptions_in(&honest)
            .iter()
            .map(|payload| payload.episode_sequence)
            .collect::<Vec<_>>(),
        vec![claim_sequence],
        "the refused call must have spent nothing, so the honest sweep still raises the episode"
    );
}

/// R5b — replay reproduces the IDENTICAL exception set, not merely a quiet one.
///
/// R5 proves the journal does not RE-RAISE after replay. That is idempotence, and it is a weaker
/// claim than the criterion asks for: a fold that silently DROPPED an episode also passes it,
/// because a dropped episode raises nothing either. Nothing re-fires in both worlds, and only one
/// of them is correct.
///
/// The separating question is not "does anything fire again" but "does the rebuilt state name the
/// same episodes the journal recorded". So this cell folds from scratch and compares the SET:
/// every episode the journal carries an `overdue_exception` for must appear in the rebuilt
/// `exception_marked`, and no other.
///
/// TWO episodes, not one, and that is the whole design of the fixture: with a single episode, a
/// fold that dropped it would empty the set and R4 would fall too, so the cells would not be
/// separable at all.
///
/// NAMED SABOTAGE: mark only the first episode the fold sees.
/// FALLS AT: the set comparison — the rebuilt set is missing the redriven episode while the
/// journal names it.
///
/// MEASURED, INCLUDING WHERE IT OVERLAPS. Under that sabotage:
///
///     R5 (idempotence)  GREEN   <- the whole reason this cell exists
///     R4                GREEN
///     R1, R2, R6, R7, R8 GREEN
///     R3                RED     <- honest overlap, stated rather than left out
///     this cell         RED
///
/// R5 staying green is the finding: a fold that DROPS an episode re-raises nothing, so idempotence
/// is satisfied while the rebuilt state is wrong. That is the gap L named — idempotent is not
/// identical — and it is now a red rather than an argument.
///
/// R3 falls as well, because a dropped mark lets the redriven episode fire again and R3 asks for
/// silence on a third sweep. The two are not independent on this sabotage, and saying so is worth
/// more than a claim of exclusivity that the matrix would contradict.
#[test]
fn a_rebuilt_projection_names_the_same_episodes_the_journal_recorded() {
    let (_directory, repository, appended, clock) = claimed_fixture(Some(PersistedCustoms::new(
        600,
        CLEARANCE_BUDGET_SECONDS,
        None,
    )));
    let first_claim = appended.last().unwrap().sequence;

    sweep_at(&repository, &instant("2026-08-10T14:00:00Z"));

    let routed = append_after(&repository, vec![dlq_routed_event("routed", first_claim)]);
    let redriven = append_after(
        &repository,
        vec![dlq_redrive_event(
            "redriven",
            routed.last().unwrap().sequence,
        )],
    );
    let redrive_sequence = redriven.last().unwrap().sequence;

    clock.advance_to(18);
    sweep_at(&repository, &instant("2026-08-10T17:00:00Z"));

    // What the JOURNAL says happened, read back off the log rather than remembered from the
    // return values above.
    let history = repository.read_replay_stream(&scope(), STREAM).unwrap();
    let journalled: BTreeSet<u64> = history
        .iter()
        .filter_map(|envelope| match &envelope.kind {
            EventKind::OverdueException(payload) => Some(payload.episode_sequence),
            _ => None,
        })
        .collect();

    assert_eq!(
        journalled,
        BTreeSet::from([first_claim, redrive_sequence]),
        "the fixture must have produced two distinct episodes, or this cell proves nothing"
    );

    // What a FOLD FROM SCRATCH derives from those same bytes.
    let rebuilt = graphhelm_events::replay(&scope(), STREAM, &history).unwrap();

    assert_eq!(
        rebuilt.exception_marked, journalled,
        "a projection rebuilt from the journal must name exactly the episodes the journal \
         recorded -- no fewer (an episode silently dropped by the fold re-arms and will be raised \
         again by some later sweep) and no more"
    );
}

// ---------------------------------------------------------------------------------------------
// #1184 review BLOCK: a wait created on the ORDINARY START PATH has a deadline.
//
// `stage_deadline` read customs only from `current_graph`, which nothing but a sealed
// `GraphVersionPublished` fills. Every cell above publishes one, so every cell above measured the
// half of the world that works. The `start` path does not publish: it appends
// `execution_form_declared`, and a test in the fold asserts that a declaration must NOT fill the
// sealed graph. So a node parked by a real start carried `deadline: None`, `overdue_at` skipped it
// forever, and `waitWithinSeconds` -- REQUIRED by its own schema -- bounded nothing.
//
// The budgets now travel with the declaration. These two cells are the pair the review asked for:
// one that the fix turns from red to green, and one that pins which source wins when both exist.
// ---------------------------------------------------------------------------------------------

/// The wait budget the declaration-path fixture declares, in seconds.
///
/// Thirty rather than an hour so the cell can ask on BOTH sides of the horizon within the same
/// minute, and so the expected instant below is readable as arithmetic a person can check.
const DECLARED_WAIT_SECONDS: u64 = 30;

/// A parked node on the START path: an execution declared, never published.
///
/// The publication is the one event this deliberately omits, and the omission IS the subject. The
/// `graph_hash` is a literal rather than a version's own, exactly as `execution_projection.rs`
/// writes it, because there is no version here to take one from -- which is the point.
fn declared_fixture(
    budgets: Option<graphhelm_protocols::CustomsBudgets>,
    publish: bool,
) -> (tempfile::TempDir, LocalEventRepository, Vec<EventEnvelope>) {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::at(12));
    let repository =
        LocalEventRepository::open(directory.path(), clock.clone(), Arc::new(Ids::default()))
            .unwrap();

    let mut node_customs_budgets = std::collections::BTreeMap::new();
    if let Some(budgets) = budgets {
        node_customs_budgets.insert(OpaqueId::parse(NODE).unwrap(), budgets);
    }

    // Only the precedence cell publishes. When it does, the published node declares an HOUR while
    // the declaration declares thirty seconds, so the two sources cannot produce the same answer
    // and whichever one the fold read is legible from the deadline alone.
    let (version, evidence) = if publish {
        let (version, evidence) = version_and_evidence(Some(published_customs()));
        (Some(version), evidence)
    } else {
        (None, Vec::new())
    };
    let (graph_version, graph_hash) = match &version {
        Some(version) => (version.number(), version.topology_hash().clone()),
        None => (
            FIRST_VERSION,
            graphhelm_protocols::WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
        ),
    };

    let mut batch = vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                graph_version,
                graph_hash,
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "form-declared",
            EventKind::ExecutionFormDeclared(graphhelm_protocols::ExecutionFormDeclared {
                execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                node_ids: vec![OpaqueId::parse(NODE).unwrap()],
                node_descriptors: std::collections::BTreeMap::new(),
                topology: None,
                node_timeout_seconds: std::collections::BTreeMap::new(),
                name: None,
                objective: None,
                executor: None,
                node_customs_budgets,
            }),
        ),
    ];
    if let Some(version) = version {
        batch.push(NewEvent::new(
            OpaqueId::parse("graph-published").unwrap(),
            version.created_by().clone(),
            Sensitivity::Internal,
            EventKind::GraphVersionPublished(Box::new(GraphVersionPublished { version })),
            evidence
                .iter()
                .map(|item| item.reference().clone())
                .collect(),
            vec![],
        ));
    }
    batch.extend([
        outcome_event("dispatch", Outcome::Started, NodeState::Queued),
        outcome_event("run", Outcome::Started, NodeState::Running),
        outcome_event("park", Outcome::NeedsInput, NodeState::WaitingInput),
    ]);

    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        1,
        batch,
        evidence,
        vec![],
    )
    .unwrap();
    let appended = repository.append_atomic(&request).unwrap();
    // The park is the last event, and every assertion below is about the instant IT carries. A
    // fixture whose park landed somewhere else would make the expected horizon arithmetic about a
    // different event, so the instant is checked rather than assumed.
    assert_eq!(
        appended.last().unwrap().occurred_at,
        instant("2026-08-10T12:00:00Z"),
        "the park must carry the fixture's own instant"
    );
    clock.advance_to(15);
    (directory, repository, appended)
}

/// The budgets a PUBLISHED node declares in the precedence cell: an hour, so it cannot be confused
/// with the declaration's thirty seconds.
fn published_customs() -> PersistedCustoms {
    PersistedCustoms::new(3600, CLEARANCE_BUDGET_SECONDS, None)
}

fn declared_budgets(wait: u64) -> graphhelm_protocols::CustomsBudgets {
    graphhelm_protocols::CustomsBudgets {
        wait_within_seconds: wait,
        clearance_within_seconds: CLEARANCE_BUDGET_SECONDS,
        dlq_within_seconds: None,
    }
}

fn projection_of(repository: &LocalEventRepository) -> graphhelm_events::ExecutionProjection {
    let history = repository.read_replay_stream(&scope(), STREAM).unwrap();
    graphhelm_events::replay(&scope(), STREAM, &history).unwrap()
}

/// THE CELL THE REVIEW ASKED FOR. Red before the fix, green after: no published graph, a declared
/// `waitWithinSeconds` of 30, a park at 12:00:00Z, a deadline at exactly 12:00:30Z, and one
/// overdue episode at 12:00:31Z.
#[test]
fn a_wait_declared_at_start_without_a_published_graph_has_a_deadline_and_can_lapse() {
    let (_directory, repository, appended) =
        declared_fixture(Some(declared_budgets(DECLARED_WAIT_SECONDS)), false);
    let projection = projection_of(&repository);

    assert!(
        projection.current_graph.is_none(),
        "ARRANGEMENT: this cell is about the path that publishes NO graph version"
    );
    assert_eq!(
        projection.node_states.get(NODE),
        Some(&NodeState::WaitingInput),
        "ARRANGEMENT: the node must actually be parked"
    );

    let wait = projection
        .open_waits
        .get(NODE)
        .expect("the park opens a wait");
    assert_eq!(
        wait.at_sequence,
        appended.last().unwrap().sequence,
        "the wait is identified by the parking event's own sequence"
    );
    // EXACTLY, not "is some": a deadline at the wrong instant lapses at the wrong time, and
    // `is_some` would pass for a horizon of zero -- the trap `stage_deadline`'s own doc names.
    assert_eq!(
        wait.deadline.as_ref(),
        Some(&instant("2026-08-10T12:00:30Z")),
        "12:00:00Z plus the declared 30 seconds, and nothing else"
    );

    // ONE SECOND BEFORE: not overdue. Without this the cell would pass on a deadline that had
    // already lapsed at the instant of entry, which is the failure mode a horizon of zero
    // produces and the one a single late reading cannot distinguish.
    assert!(
        graphhelm_events::overdue_at(&projection, &instant("2026-08-10T12:00:29Z")).is_empty(),
        "the wait must not be overdue before its own horizon"
    );

    let overdue = graphhelm_events::overdue_at(&projection, &instant("2026-08-10T12:00:31Z"));
    assert_eq!(overdue.len(), 1, "exactly one episode lapsed: {overdue:?}");
    assert_eq!(overdue[0].node, NODE);
    assert_eq!(overdue[0].episode_seq, wait.at_sequence);
    assert_eq!(overdue[0].deadline, instant("2026-08-10T12:00:30Z"));
    assert_eq!(overdue[0].claim_seq, None);
}

/// THE CONTROL for the cell above. Same fixture, same park, same instant -- only the declaration
/// is absent. Without it, "the declaration produced the deadline" is not established: a fold that
/// invented a budget from nowhere would satisfy the cell above just as well.
#[test]
fn a_start_path_node_that_declared_no_customs_still_has_no_deadline() {
    let (_directory, repository, _appended) = declared_fixture(None, false);
    let projection = projection_of(&repository);

    let wait = projection
        .open_waits
        .get(NODE)
        .expect("the park opens a wait either way");
    assert_eq!(
        wait.deadline, None,
        "an undeclared budget stays absent and never becomes a horizon of zero"
    );
    assert!(
        graphhelm_events::overdue_at(&projection, &instant("2027-01-01T00:00:00Z")).is_empty(),
        "nobody bounded this stage, so no instant makes it overdue"
    );
}

/// PRECEDENCE, pinned in the one arrangement where the two sources disagree: a published graph
/// declaring an hour and a declaration declaring thirty seconds, on the same node, in the same
/// stream. The published graph is the sealed form and wins; the declaration is what it grew from.
///
/// The two budgets differ deliberately, so the deadline alone says which source the fold read. A
/// cell whose two sources agreed would pass whichever one it consulted.
#[test]
fn a_published_graph_decides_the_deadline_even_when_a_declaration_is_also_present() {
    let (_directory, repository, _appended) =
        declared_fixture(Some(declared_budgets(DECLARED_WAIT_SECONDS)), true);
    let projection = projection_of(&repository);

    assert!(
        projection.current_graph.is_some(),
        "ARRANGEMENT: this cell needs the published half present"
    );
    assert!(
        projection
            .declared_form
            .as_ref()
            .is_some_and(|form| !form.node_customs_budgets.is_empty()),
        "ARRANGEMENT: this cell needs the declared half present too"
    );

    let wait = projection
        .open_waits
        .get(NODE)
        .expect("the park opens a wait");
    assert_eq!(
        wait.deadline.as_ref(),
        Some(&instant("2026-08-10T13:00:00Z")),
        "the published hour, not the declared thirty seconds"
    );
}
