//! Governed memory admission (#220).
//!
//! The first guard here is G9 of the task blueprint: a candidate is refused BECAUSE it carries a
//! secret, and the refusal that results must not become the vehicle that persists it. The refusal
//! is itself an event, so "nothing has been persisted yet" describes the instant BEFORE the
//! refusal, never the refusal.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

use chrono::TimeZone;
use graphhelm_events::{
    ActiveVersion, EventPage, EventRepository, EventRepositoryError, LocalEventRepository,
    PreparedAppend,
};
use graphhelm_governor::{
    CaptureOptIn, CaptureTouch, MemoryAdmissionRefusalRequest, MemoryCandidate, MemoryField,
    MemoryOrigin, MemoryPublicationState, MemoryPublicationTransition,
    MemoryPublicationTransitionRequest, MemoryRecord, MemoryRecordSupersededRequest,
    MemoryRefusalCode, MemorySemanticState, PublicationStep, SupersessionReason,
    admit_memory_candidate, apply_publication_transition, bind_evidence, capture_memory,
    check_dependency_freshness, handoff_into_scope, publication_steps,
    record_memory_admission_refusal, record_memory_publication_transition,
    record_memory_record_superseded, republish, supersede, validate_candidate,
};
use graphhelm_protocols::{
    ActorId, ArtifactId, Clock, DevelopmentScope, EventEnvelope, EventKind, EvidenceId,
    IdGenerator, MemoryAdmissionLocal,
    MemoryAdmissionRefusalCode as PersistedMemoryAdmissionRefusalCode, OpaqueId, PersistedActor,
    PersistedActorType, PersistedMemoryPublicationState, PersistedMemoryPublicationTransition,
    PersistedMemorySemanticState, PersistedSupersessionReason, ProjectId, RepositoryScope,
    WorkspaceId,
};

/// A value that exists in the INPUT by construction, which is what makes an empty search
/// meaningful: the string is known to be present upstream, so its absence downstream is either a
/// clean record or a broken search, and the input distinguishes the two.
const SENTINEL: &str = "ghp_G9SENTINELSECRETdoNotPersistMe0000000";

fn scope() -> DevelopmentScope {
    DevelopmentScope {
        workspace_id: WorkspaceId::parse("workspace-g9").expect("valid workspace id"),
        project_id: ProjectId::parse("project-g9").expect("valid project id"),
        subproject_id: None,
        execution_id: None,
    }
}

/// The production change this catches: a refusal record that quotes the value it refused — the
/// obvious way to make a diagnostic useful, and the way the secret reaches an append-only journal
/// through the one path the design argued was safe.
#[test]
fn refusal_names_the_code_and_location_but_never_the_secret_value() {
    let candidate = MemoryCandidate::draft(scope(), format!("api token: {SENTINEL}"));

    let admitting_into = candidate.scope().clone();
    let refusal = admit_memory_candidate(&candidate, &admitting_into)
        .expect_err("a candidate bearing a secret must be refused");

    // Landmark. A bare absence assertion passes against an empty record, so the record must first
    // be shown to say something: the CODE and the LOCATION are what a refusal is for.
    assert_eq!(refusal.code(), MemoryRefusalCode::SecretDetected);
    assert_eq!(refusal.field(), MemoryField::Content);

    // ... and the value that caused it must not travel with it, in any rendering that a log, an
    // event payload or a panic message would use.
    let rendered = format!("{refusal:?} {refusal}");
    assert!(
        !rendered.contains(SENTINEL),
        "the refusal record carried the offending value: {rendered}"
    );
}

/// G1 of the blueprint, as a PAIR. A bare "disabled produced nothing" assertion passes against a
/// capture path that is broken for every input, so the enabled arm runs first and its failure is
/// reported as HARNESS-BROKE rather than as the property failing: an exit code cannot separate
/// "ran and found nothing" from "never ran", and neither can a two-state pair.
///
/// The production change this catches: the opt-in check placed BELOW the first boundary touch —
/// the natural way to write it, and the way a disabled project still reaches a provider.
#[test]
fn capture_touches_nothing_when_the_project_has_not_opted_in() {
    let mut enabled_touches = Vec::new();
    let captured = capture_memory(
        CaptureOptIn::Enabled,
        &scope(),
        "a note worth keeping",
        &mut enabled_touches,
    );

    assert!(
        captured.is_ok(),
        "HARNESS-BROKE: the enabled arm produced no candidate, so the disabled arm's emptiness \
         would prove nothing about opt-in"
    );
    // Each boundary the acceptance criteria name is shown being touched on the ENABLED arm, one
    // named assertion per boundary. A single "not empty" check lets four of the five zeros on the
    // disabled arm pass for the wrong reason: the boundary was never reachable in this fixture at
    // all. Written out by hand rather than looped over the enum, so that dropping a boundary from
    // the capture path fails HERE instead of quietly shrinking what the disabled arm proves.
    for boundary in [
        CaptureTouch::CandidateConstructed,
        CaptureTouch::ProviderQueried,
        CaptureTouch::LogWritten,
        CaptureTouch::EventAppended,
        CaptureTouch::PersistentBoundaryTouched,
    ] {
        assert!(
            enabled_touches.contains(&boundary),
            "HARNESS-BROKE: the enabled arm never touched {boundary:?}, so the disabled arm's \
             silence about it proves nothing"
        );
    }

    assert!(
        !enabled_touches.is_empty(),
        "HARNESS-BROKE: the enabled arm touched no boundary, so an empty disabled arm is \
         indistinguishable from a capture path that never runs"
    );

    let mut disabled_touches = Vec::new();
    let captured = capture_memory(
        CaptureOptIn::Disabled,
        &scope(),
        "a note worth keeping",
        &mut disabled_touches,
    );

    let refusal = captured.expect_err("a disabled project produced a candidate");
    assert_eq!(refusal.code(), MemoryRefusalCode::OptInAbsent);
    assert_eq!(
        disabled_touches,
        Vec::new(),
        "a disabled project touched a boundary before the opt-in check"
    );
}

#[derive(Default)]
struct TouchCountingRepository(AtomicUsize);

impl TouchCountingRepository {
    fn touched(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }

    fn reject<T>(&self) -> Result<T, EventRepositoryError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(EventRepositoryError::Invalid)
    }
}

impl EventRepository for TouchCountingRepository {
    fn append_atomic(
        &self,
        _request: &PreparedAppend,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        self.reject()
    }

    fn read_stream(
        &self,
        _scope: &RepositoryScope,
        _stream_id: &str,
        _limit: usize,
        _cursor: Option<&str>,
    ) -> Result<EventPage, EventRepositoryError> {
        self.reject()
    }

    fn read_replay_stream(
        &self,
        _scope: &RepositoryScope,
        _stream_id: &str,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        self.reject()
    }

    fn next_sequence(
        &self,
        _scope: &RepositoryScope,
        _stream_id: &str,
    ) -> Result<u64, EventRepositoryError> {
        self.reject()
    }

    fn evidence_exists(
        &self,
        _scope: &RepositoryScope,
        _evidence_id: &EvidenceId,
    ) -> Result<bool, EventRepositoryError> {
        self.reject()
    }

    fn artifact_exists(
        &self,
        _scope: &RepositoryScope,
        _artifact_id: &ArtifactId,
    ) -> Result<bool, EventRepositoryError> {
        self.reject()
    }

    fn active_version(
        &self,
        _scope: &RepositoryScope,
        _stream_id: &str,
    ) -> Result<Option<ActiveVersion>, EventRepositoryError> {
        self.reject()
    }

    fn committed_events_for_idempotency(
        &self,
        _scope: &RepositoryScope,
        _stream_id: &str,
        _idempotency_key: &OpaqueId,
    ) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {
        self.reject()
    }
}

fn refusal_request<'a>(content: &'a str) -> MemoryAdmissionRefusalRequest<'a> {
    MemoryAdmissionRefusalRequest::new(
        RepositoryScope::new(
            WorkspaceId::parse("workspace-g9").unwrap(),
            ProjectId::parse("project-g9").unwrap(),
            None,
        ),
        OpaqueId::parse("memory-admission").unwrap(),
        1,
        OpaqueId::parse("refusal-opt-in-absent").unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("governor-memory").unwrap(),
        ),
        content,
    )
}

#[test]
fn an_absent_opt_in_never_calls_the_repository() {
    let content = format!("please remember {SENTINEL}");
    let mut touches = Vec::new();
    let refusal = capture_memory(CaptureOptIn::Disabled, &scope(), &content, &mut touches)
        .expect_err("a disabled project produced a candidate");
    let repository = TouchCountingRepository::default();

    let recorded = record_memory_admission_refusal(
        CaptureOptIn::Disabled,
        &repository,
        refusal_request(&content),
        &refusal,
    )
    .expect("disabled capture should return before repository work");

    assert!(recorded.is_none());
    assert!(touches.is_empty());
    assert_eq!(repository.touched(), 0);
    assert!(!format!("{refusal:?} {refusal}").contains(SENTINEL));
}

#[test]
fn an_opt_in_absent_refusal_cannot_be_relabelled_as_enabled() {
    let content = "a note worth keeping";
    let mut touches = Vec::new();
    let refusal = capture_memory(CaptureOptIn::Disabled, &scope(), content, &mut touches)
        .expect_err("a disabled project produced a candidate");
    let repository = TouchCountingRepository::default();

    let error = record_memory_admission_refusal(
        CaptureOptIn::Enabled,
        &repository,
        refusal_request(content),
        &refusal,
    )
    .expect_err("an opt-in refusal was appended under an enabled label");

    assert_eq!(error.code(), "GHE004_INVALID_EVENT");
    assert_eq!(repository.touched(), 0);
    assert!(touches.is_empty());
}

/// ADR-032 decision 5: a handoff into a scope without capture opt-in is refused BY NAME, never
/// folded into `OptInAbsent`. The two refusals name different sides of the same check -- one
/// project's own capture is disabled, versus a handoff's RECEIVING scope has not opted in -- and an
/// operator reading the persisted code must be able to tell which one it was without cross-
/// referencing which call site raised it.
#[test]
fn a_handoff_into_an_unopted_scope_is_refused_by_its_own_code() {
    let refusal = handoff_into_scope(CaptureOptIn::Disabled)
        .expect_err("a handoff into an unopted scope produced no refusal");

    assert_eq!(refusal.code(), MemoryRefusalCode::HandoffTargetNotOptedIn);
    assert_eq!(refusal.field(), MemoryField::Scope);
}

/// Arrangement check for the test above: proven first so a function that refuses every input,
/// regardless of opt-in, cannot make the refusal test pass vacuously.
#[test]
fn a_handoff_into_an_opted_in_scope_is_not_refused() {
    assert!(handoff_into_scope(CaptureOptIn::Enabled).is_ok());
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 8, 28, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct Ids(AtomicU64);

impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "graphhelm-memory-refusal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn an_enabled_secret_refusal_persists_only_code_local_and_bytes() {
    let content = format!("please remember {SENTINEL}");
    let mut touches = Vec::new();
    let refusal = capture_memory(CaptureOptIn::Enabled, &scope(), &content, &mut touches)
        .expect_err("secret-bearing capture was accepted");
    assert!(touches.is_empty());

    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let events = record_memory_admission_refusal(
        CaptureOptIn::Enabled,
        &repository,
        refusal_request(&content),
        &refusal,
    )
    .unwrap()
    .expect("enabled refusal did not append");

    assert_eq!(events.len(), 1);
    let EventKind::MemoryAdmissionRefused(payload) = &events[0].kind else {
        panic!("the Governor appended the wrong event kind");
    };
    assert_eq!(
        payload.code,
        PersistedMemoryAdmissionRefusalCode::SecretDetected
    );
    assert_eq!(payload.local, MemoryAdmissionLocal::Content);
    assert_eq!(payload.bytes, content.len() as u64);

    let journal = std::fs::read_to_string(directory.0.join("journal.jsonl")).unwrap();
    assert!(!journal.contains(SENTINEL));
    assert!(!journal.contains("digest"));
    assert!(!format!("{refusal:?} {refusal}").contains(SENTINEL));
}

fn publication_transition_request() -> MemoryPublicationTransitionRequest {
    MemoryPublicationTransitionRequest::new(
        RepositoryScope::new(
            WorkspaceId::parse("workspace-g9").unwrap(),
            ProjectId::parse("project-g9").unwrap(),
            None,
        ),
        OpaqueId::parse("memory-lifecycle").unwrap(),
        1,
        OpaqueId::parse("transition-publish").unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("governor-memory").unwrap(),
        ),
    )
}

/// The producer maps the GOVERNOR record's post-transition state onto the wire, not a value the
/// caller hands it separately -- so this proves the mapping from `record.publication()`, not from
/// an argument that could silently disagree with the record.
#[test]
fn a_successful_transition_persists_the_records_new_publication_state() {
    let mut record = MemoryRecord::new(OpaqueId::parse("record-g9").unwrap());
    apply_publication_transition(&mut record, MemoryPublicationTransition::Propose)
        .expect("unpublished record must accept propose");

    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let events = record_memory_publication_transition(
        &repository,
        publication_transition_request(),
        &record,
        MemoryPublicationTransition::Propose,
    )
    .unwrap();

    assert_eq!(events.len(), 1);
    let EventKind::MemoryPublicationTransitioned(payload) = &events[0].kind else {
        panic!("the Governor appended the wrong event kind");
    };
    assert_eq!(payload.record_id.as_str(), "record-g9");
    assert_eq!(
        payload.transition,
        PersistedMemoryPublicationTransition::Propose
    );
    assert_eq!(
        payload.resulting_state,
        PersistedMemoryPublicationState::Proposed
    );
}

fn supersession_request() -> MemoryRecordSupersededRequest {
    MemoryRecordSupersededRequest::new(
        RepositoryScope::new(
            WorkspaceId::parse("workspace-g9").unwrap(),
            ProjectId::parse("project-g9").unwrap(),
            None,
        ),
        OpaqueId::parse("memory-lifecycle").unwrap(),
        1,
        OpaqueId::parse("supersession-1").unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("governor-memory").unwrap(),
        ),
    )
}

/// The producer maps the PREDECESSOR's post-supersede semantic state, not a value the caller
/// hands it separately, mirroring `a_successful_transition_persists_the_records_new_publication_state`
/// one axis over.
#[test]
fn a_successful_supersession_persists_the_predecessors_new_semantic_state() {
    let mut predecessor = MemoryRecord::new(OpaqueId::parse("record-g9-pred").unwrap());
    let mut successor = MemoryRecord::new(OpaqueId::parse("record-g9-succ").unwrap());
    supersede(
        &mut predecessor,
        &mut successor,
        SupersessionReason::Contradicted,
    )
    .expect("two distinct, not-yet-superseding records must accept supersede");

    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let events = record_memory_record_superseded(
        &repository,
        supersession_request(),
        &predecessor,
        &successor,
        SupersessionReason::Contradicted,
    )
    .unwrap();

    assert_eq!(events.len(), 1);
    let EventKind::MemoryRecordSuperseded(payload) = &events[0].kind else {
        panic!("the Governor appended the wrong event kind");
    };
    assert_eq!(payload.predecessor_id.as_str(), "record-g9-pred");
    assert_eq!(payload.successor_id.as_str(), "record-g9-succ");
    assert_eq!(payload.reason, PersistedSupersessionReason::Contradicted);
    assert_eq!(
        payload.predecessor_new_semantic_state,
        PersistedMemorySemanticState::Contradicted
    );
}

/// The vocabularies, written out BY HAND from the enum declarations.
///
/// DELIBERATELY NOT GENERATED. This looks exactly like the duplication this repository has been
/// removing -- and it is the opposite: `every()` is generated from the enum, so a guard that reads
/// `every()` on both sides is fed by the list it is checking and is green by construction. These
/// names are the only statement in the suite that can CONTRADICT the generated one, which is the
/// entire reason they exist. A future reader who deletes them to remove redundancy would be
/// applying the house rule correctly and removing the only independent witness. Update them by
/// looking at the enum, never by copying from a failure message.
const SEMANTIC_STATE_NAMES: [&str; 5] = [
    "Candidate",
    "Validated",
    "Contradicted",
    "Deprecated",
    "Expired",
];
const PUBLICATION_STATE_NAMES: [&str; 4] = ["Unpublished", "Proposed", "Published", "Withdrawn"];
const PUBLICATION_TRANSITION_NAMES: [&str; 3] = ["Propose", "Publish", "Withdraw"];
const SUPERSESSION_REASON_NAMES: [&str; 2] = ["Contradicted", "Deprecated"];

/// Compare a hand-written vocabulary against the generated one IN BOTH DIRECTIONS, naming the two
/// counts and the difference on each side. A one-way check passes when the generated list grows,
/// and a message that reports only two numbers invites the next reader to "fix" the hand-written
/// one in whichever direction makes the test green.
fn cross_check<T: std::fmt::Debug>(hand: &[&str], generated: &[T], what: &str) {
    let generated: Vec<String> = generated.iter().map(|v| format!("{v:?}")).collect();
    let missing_from_generated: Vec<&&str> = hand
        .iter()
        .filter(|name| !generated.iter().any(|g| g == *name))
        .collect();
    let missing_from_hand: Vec<&String> = generated
        .iter()
        .filter(|g| !hand.iter().any(|name| name == g))
        .collect();

    assert!(
        missing_from_generated.is_empty() && missing_from_hand.is_empty(),
        "HARNESS-BROKE: the {what} vocabulary disagrees with the hand-read declaration, so the matrix exercised below is not the matrix this test claims to cover.
  hand-written ({}): {hand:?}
  generated ({}): {generated:?}
  declared but NOT generated: {missing_from_generated:?}
  generated but NOT declared: {missing_from_hand:?}",
        hand.len(),
        generated.len()
    );
}

fn record_id(label: &str) -> OpaqueId {
    OpaqueId::parse(label).expect("HARNESS-BROKE: the fixture label is not a legal OpaqueId")
}

/// G3 of the blueprint, ported to the PUBLICATION axis (ADR-032): the matrix is DERIVED from the
/// vocabularies rather than hand-listed, so a tuple nobody thought about is still exercised; the
/// allowed set stays explicit because that part is policy, not enumeration.
///
/// The production change this catches: a refused transition that mutates the predecessor on its way
/// out -- the record moves and the caller is told it did not.
#[test]
fn every_publication_transition_tuple_either_applies_or_leaves_the_predecessor_untouched() {
    cross_check(
        &PUBLICATION_STATE_NAMES,
        MemoryPublicationState::every(),
        "publication state",
    );
    cross_check(
        &PUBLICATION_TRANSITION_NAMES,
        MemoryPublicationTransition::every(),
        "publication transition",
    );

    let mut applied = 0_usize;
    let mut refused = 0_usize;

    for publication in MemoryPublicationState::every() {
        for transition in MemoryPublicationTransition::every() {
            // The fixture carries a SECOND field, and the refusal arm compares the WHOLE
            // predecessor. Asserting `record.publication()` alone leaves every other field of the
            // record unguarded, and with a bare `MemoryRecord::at()` the dependency list was
            // always empty -- so no sabotage of the refusal path could turn this red on that half.
            let mut record = MemoryRecord::at(
                record_id("rec-publication-matrix"),
                MemorySemanticState::Candidate,
                *publication,
            )
            .depending_on("policy", "v1");
            let before = record.clone();

            match apply_publication_transition(&mut record, *transition) {
                Ok(()) => {
                    assert_eq!(
                        record.publication(),
                        transition.target(),
                        "an allowed {publication:?} -> {transition:?} landed in the wrong state"
                    );
                    assert_eq!(
                        record.semantic(),
                        before.semantic(),
                        "a publication transition moved the SEMANTIC axis (ADR-032 decision 3: \
                         these axes are independent)"
                    );
                    applied += 1;
                }
                Err(refusal) => {
                    assert_eq!(
                        record,
                        before,
                        "a refused {publication:?} -> {transition:?} changed its predecessor; \
                         the refusal reported {:?}",
                        refusal.code()
                    );
                    refused += 1;
                }
            }
        }
    }

    // Both arms must be non-empty. A matrix where nothing is ever allowed passes every refusal
    // assertion, and a matrix where nothing is ever refused passes every application assertion.
    assert!(applied > 0, "HARNESS-BROKE: no tuple was ever allowed");
    assert!(refused > 0, "HARNESS-BROKE: no tuple was ever refused");
    assert_eq!(
        applied + refused,
        PUBLICATION_STATE_NAMES.len() * PUBLICATION_TRANSITION_NAMES.len()
    );
}

/// ADR-032's own reason for existing: `superseded` is a RELATIONSHIP, not a state. Superseding
/// moves the PREDECESSOR's semantic axis to the reason's target and records the relationship on
/// the SUCCESSOR -- and nothing else moves, which is the property a flattened one-axis state could
/// never express (the old model could not distinguish "superseded because contradicted" from
/// "superseded because deprecated": one wire spelling for two operator responses).
///
/// The production change this catches: inferring the reason from the relationship existing,
/// instead of requiring the caller to state it -- both reasons are exercised so a hard-coded one
/// cannot pass.
#[test]
fn supersede_moves_the_predecessors_semantic_axis_and_records_the_relationship_on_the_successor() {
    // Both hand-written witnesses checked here: this test is the one place both vocabularies
    // (the axis `supersede` moves, and the closed reason set it accepts) are exercised together.
    cross_check(
        &SEMANTIC_STATE_NAMES,
        MemorySemanticState::every(),
        "semantic state",
    );
    cross_check(
        &SUPERSESSION_REASON_NAMES,
        SupersessionReason::every(),
        "supersession reason",
    );

    for reason in SupersessionReason::every() {
        let mut predecessor = MemoryRecord::at(
            record_id("rec-predecessor"),
            MemorySemanticState::Validated,
            MemoryPublicationState::Published,
        );
        let mut successor = MemoryRecord::new(record_id("rec-successor"));
        let predecessor_publication_before = predecessor.publication();
        let successor_semantic_before = successor.semantic();

        supersede(&mut predecessor, &mut successor, *reason)
            .expect("HARNESS-BROKE: a legal supersession was refused");

        assert_eq!(
            predecessor.semantic(),
            reason.target(),
            "supersede({reason:?}) did not move the predecessor's semantic axis to the reason's \
             own target"
        );
        assert_eq!(
            successor.supersedes(),
            Some(predecessor.id()),
            "the successor does not record the predecessor it supersedes"
        );
        assert_eq!(
            predecessor.publication(),
            predecessor_publication_before,
            "supersede touched the predecessor's PUBLICATION axis; ADR-032 decision 2 makes this \
             a semantic-axis-only operation, withdrawal is a separate act"
        );
        assert_eq!(
            successor.semantic(),
            successor_semantic_before,
            "supersede touched the successor's own semantic axis, which it must not: the \
             successor's belief in its OWN content is unrelated to which predecessor it replaces"
        );
    }
}

/// The refusal half of `supersede`: a record cannot supersede itself.
#[test]
fn a_record_cannot_supersede_itself() {
    let id = record_id("rec-self");
    let mut predecessor = MemoryRecord::new(id.clone());
    let mut successor = MemoryRecord::new(id);

    let refusal = supersede(
        &mut predecessor,
        &mut successor,
        SupersessionReason::Deprecated,
    )
    .expect_err("a record was allowed to supersede itself");
    assert_eq!(refusal.code(), MemoryRefusalCode::TransitionNotAllowed);
}

/// The refusal half of `supersede`: a successor cannot supersede two predecessors -- the
/// relationship is one field, not a list, so a second call must refuse rather than silently
/// overwrite the first predecessor's identity.
#[test]
fn a_successor_cannot_supersede_twice() {
    let mut first_predecessor = MemoryRecord::new(record_id("rec-first-predecessor"));
    let mut second_predecessor = MemoryRecord::new(record_id("rec-second-predecessor"));
    let mut successor = MemoryRecord::new(record_id("rec-successor-twice"));

    supersede(
        &mut first_predecessor,
        &mut successor,
        SupersessionReason::Deprecated,
    )
    .expect("HARNESS-BROKE: the first supersession must succeed for this test to mean anything");

    let refusal = supersede(
        &mut second_predecessor,
        &mut successor,
        SupersessionReason::Contradicted,
    )
    .expect_err("a successor was allowed to supersede a second predecessor");
    assert_eq!(refusal.code(), MemoryRefusalCode::TransitionNotAllowed);
    assert_eq!(
        successor.supersedes(),
        Some(first_predecessor.id()),
        "the refused second call overwrote the first supersession's relationship"
    );
    assert_eq!(
        second_predecessor.semantic(),
        MemorySemanticState::Candidate,
        "the refused second call still moved the second predecessor's semantic axis"
    );
}

/// ADR-032 decision 3, proven directly rather than only as a side effect of one cell of
/// `every_publication_transition_tuple_either_applies_or_leaves_the_predecessor_untouched` above.
/// That matrix test always withdraws a record whose semantic axis is `Candidate` -- the fixture's
/// own default -- so a defect that reset the semantic axis specifically ON WITHDRAWAL, but only
/// when the record started somewhere OTHER than `Candidate`, would pass every cell of that matrix
/// without this test existing. Every non-default semantic value is exercised here for that reason.
///
/// Proven in both directions in the same cell, so neither assertion is vacuous on its own: the
/// publication axis really DOES move (a call that changed nothing at all would make the semantic
/// assertion below true for the wrong reason), and the semantic axis does NOT -- withdrawal closes
/// USE, never belief in the content.
///
/// The production change this catches: `apply_publication_transition` resetting or otherwise
/// touching `record.semantic` on any transition, most plausibly on `Withdraw` specifically (the
/// transition whose name reads, in isolation, like it might mean "this content is no longer
/// good" rather than "this content is no longer shown").
#[test]
fn withdrawing_a_record_moves_only_the_publication_axis_never_the_semantic_one() {
    for semantic in [
        MemorySemanticState::Validated,
        MemorySemanticState::Contradicted,
        MemorySemanticState::Deprecated,
        MemorySemanticState::Expired,
    ] {
        let mut record = MemoryRecord::at(
            record_id("rec-withdraw-axis-independence"),
            semantic,
            MemoryPublicationState::Published,
        );

        apply_publication_transition(&mut record, MemoryPublicationTransition::Withdraw)
            .expect("HARNESS-BROKE: Published -> Withdraw is an allowed publication transition");

        // Arrangement check first: the publication axis really did move.
        assert_eq!(
            record.publication(),
            MemoryPublicationState::Withdrawn,
            "HARNESS-BROKE: withdraw did not move the publication axis at all, so the semantic \
             assertion below would be vacuously true of a call that changed nothing"
        );
        assert_eq!(
            record.semantic(),
            semantic,
            "withdrawing a record starting at {semantic:?} changed its semantic axis -- \
             withdrawal must close USE, never touch belief in the content"
        );
    }
}

/// G4 of the blueprint, and the one place where the choice of instrument IS the guard.
///
/// A canonical digest is blind to object key order BY DESIGN, so "the digest matches" and "the
/// bytes are the same" are different claims. This task's product is an audit trail: two Evidence
/// payloads with different bytes must not bind identically, or the evidence under a published
/// record can be substituted without breaking the binding.
///
/// The production change this catches: a binding that compares canonical form instead of bytes.
#[test]
fn a_binding_rejects_evidence_that_is_byte_different_but_canonically_equal() {
    let sealed = br#"{"alpha":1,"beta":2}"#;
    let reordered = br#"{"beta":2,"alpha":1}"#;

    // Both halves of the trap, asserted rather than assumed. Same meaning ...
    let as_value: serde_json::Value = serde_json::from_slice(sealed).expect("sealed parses");
    let reordered_value: serde_json::Value =
        serde_json::from_slice(reordered).expect("reordered parses");
    assert_eq!(
        as_value, reordered_value,
        "HARNESS-BROKE: the two payloads are not canonically equal, so this test is not \
         exercising the blindness it claims to exercise"
    );
    // ... and different bytes. Without this the rejection below could be trivially true.
    assert_ne!(
        sealed.as_slice(),
        reordered.as_slice(),
        "HARNESS-BROKE: the two payloads are byte-identical"
    );

    let binding = bind_evidence(sealed);

    assert!(
        binding.matches(sealed),
        "HARNESS-BROKE: the binding does not match the bytes it was built from, so a rejection \
         below would prove nothing"
    );
    assert!(
        !binding.matches(reordered),
        "the binding accepted byte-different evidence: a canonical digest cannot tell these apart, \
         and substituting the evidence under a published record would leave the binding intact"
    );
}

fn scope_in(workspace: &str, project: &str) -> DevelopmentScope {
    DevelopmentScope {
        workspace_id: WorkspaceId::parse(workspace).expect("valid workspace id"),
        project_id: ProjectId::parse(project).expect("valid project id"),
        subproject_id: None,
        execution_id: None,
    }
}

/// G2 of the blueprint: content captured under one scope must not be admitted under another.
///
/// The production change this catches: a scope comparison that reads ONE field. Comparing only the
/// project lets content cross workspaces whenever two tenants happen to name a project the same
/// way -- and identical project names across tenants is the normal case, not the exotic one.
#[test]
fn admission_refuses_content_captured_under_another_scope() {
    let captured_in = scope_in("workspace-a", "shared-name");
    let admitting_into = scope_in("workspace-b", "shared-name");

    // Both halves of the trap. The project matches ...
    assert_eq!(
        captured_in.project_id, admitting_into.project_id,
        "HARNESS-BROKE: the projects differ, so a one-field comparison would refuse for the wrong \
         reason and this test would pass without exercising the bleed"
    );
    // ... and the workspace does not.
    assert_ne!(
        captured_in.workspace_id, admitting_into.workspace_id,
        "HARNESS-BROKE: the workspaces match, so there is no cross-scope bleed to detect"
    );

    let candidate = MemoryCandidate::draft(captured_in, "a note from another tenant");

    // Landmark: the same candidate IS admitted into its own scope, so the refusal below is about
    // the scope and not about the content being rejected for some unrelated reason.
    let own_scope = candidate.scope().clone();
    admit_memory_candidate(&candidate, &own_scope)
        .expect("HARNESS-BROKE: a candidate was refused inside its own scope");

    let refusal = admit_memory_candidate(&candidate, &admitting_into)
        .expect_err("cross-scope content was admitted");
    assert_eq!(refusal.code(), MemoryRefusalCode::ScopeMismatch);
}

/// G5 of the blueprint: a candidate is not validated by the party that produced it.
///
/// The production change this catches: a validation check that counts validators instead of
/// identifying them. "At least one validator signed off" is true when the only signature is the
/// producer's own, and that is the shape self-validation actually takes in the wild -- nobody
/// writes `validate(self)`, they write a list that happens to contain themselves.
#[test]
fn a_candidate_is_not_validated_by_its_own_producer() {
    let candidate =
        MemoryCandidate::draft(scope(), "a claim about the world").produced_by("agent-a");

    // Landmark: an independent validator IS accepted, so the refusal below is about WHO signed and
    // not about validation being broken for everyone.
    validate_candidate(&candidate, &["agent-b"])
        .expect("HARNESS-BROKE: an independent validator was refused");

    let refusal = validate_candidate(&candidate, &["agent-a"])
        .expect_err("the producer validated its own candidate");
    assert_eq!(refusal.code(), MemoryRefusalCode::SelfValidated);

    // The list-that-contains-itself is the real shape: a non-empty roster still fails when the
    // only signature on it is the producer's.
    let refusal = validate_candidate(&candidate, &["agent-a", "agent-a"])
        .expect_err("a roster of the producer alone counted as independent validation");
    assert_eq!(refusal.code(), MemoryRefusalCode::SelfValidated);

    // ... and a roster that also carries an independent name is accepted.
    validate_candidate(&candidate, &["agent-a", "agent-b"])
        .expect("a roster carrying an independent validator must be accepted");
}

/// G6 of the blueprint: provider output does not come back in as a fresh candidate.
///
/// The production change this catches: the CHECK exists and the MARK is never set. A guard that
/// reads an origin nobody stamps is an absence assertion with nothing behind it -- it refuses
/// exactly never, and it looks like a guard in review.
#[test]
fn provider_output_cannot_be_recaptured_as_a_fresh_candidate() {
    let mut touches = Vec::new();
    let captured = capture_memory(
        CaptureOptIn::Enabled,
        &scope(),
        "a note the user wrote",
        &mut touches,
    )
    .expect("HARNESS-BROKE: the enabled arm produced no candidate");

    // Landmark: user-written content is admitted, so the refusal below is about ORIGIN.
    let into = captured.scope().clone();
    admit_memory_candidate(&captured, &into).expect("user-written content must be admitted");

    // The mark must actually be SET by the path that reads from a provider. Asserted before it is
    // relied on: a guard reading a flag nobody stamps refuses exactly never.
    let recalled = MemoryCandidate::from_provider(scope(), "a note the provider returned");
    assert_eq!(
        recalled.origin(),
        MemoryOrigin::Provider,
        "HARNESS-BROKE: the provider path did not stamp the origin, so the refusal below would \
         never fire in production no matter what this test asserts"
    );

    let refusal = admit_memory_candidate(&recalled, &into)
        .expect_err("provider output was recaptured as a fresh candidate");
    assert_eq!(refusal.code(), MemoryRefusalCode::RecaptureLoop);
}

/// G7 of the blueprint: whatever survives a crash must read as the SAFE state.
///
/// Every prefix is checked, not a hand-picked one. A crash test that picks its own crash point
/// tests the point the author thought of; the invariant has to hold at every boundary the write
/// sequence actually has.
///
/// The production change this catches: publishing the record before sealing its evidence. Both
/// orders are correct once complete; they differ only in what a crash leaves behind, and one of
/// them leaves a published record with no evidence under it.
#[test]
fn no_crash_point_leaves_a_record_published_without_its_evidence() {
    let steps = publication_steps();

    // Landmark: the completed sequence contains both, so the invariant below is not vacuously true
    // for a sequence that never publishes anything at all.
    assert!(
        steps.contains(&PublicationStep::EvidenceSealed),
        "HARNESS-BROKE: the sequence never seals evidence"
    );
    assert!(
        steps.contains(&PublicationStep::RecordPublished),
        "HARNESS-BROKE: the sequence never publishes the record"
    );

    for crash_after in 0..=steps.len() {
        let survived = &steps[..crash_after];

        if survived.contains(&PublicationStep::RecordPublished) {
            assert!(
                survived.contains(&PublicationStep::EvidenceSealed),
                "a crash after {crash_after} step(s) left a published record with no evidence \
                 under it; the survivor reads as {survived:?}"
            );
        }
    }
}

/// G8 of the blueprint: publication requires NEWLY sealed evidence.
///
/// The production change this catches: falling back to the predecessor's binding when resealing
/// fails. It keeps publication working, which is the point -- and it publishes a new record over
/// old evidence, so the audit trail says the evidence was sealed for THIS record when it was not.
#[test]
fn a_failed_reseal_does_not_publish_over_the_previous_evidence() {
    let previous = bind_evidence(b"evidence sealed for the predecessor");

    let refusal =
        republish(&previous, Err(())).expect_err("publication proceeded after the reseal failed");
    assert_eq!(refusal.code(), MemoryRefusalCode::ResealFailed);

    // Landmark: a successful reseal DOES publish, and binds the new bytes rather than the old
    // ones, so the refusal above is about the failure and not about republish never working.
    let fresh = republish(&previous, Ok(b"evidence sealed for this record".to_vec()))
        .expect("HARNESS-BROKE: a successful reseal did not publish");
    assert!(fresh.matches(b"evidence sealed for this record"));
    assert!(
        !fresh.matches(b"evidence sealed for the predecessor"),
        "the republished record bound the predecessor's evidence"
    );
}

/// G10 of the blueprint: a record whose dependency has moved is stale, and stale is refused.
///
/// The production change this catches: a freshness check that asks whether the dependency EXISTS
/// rather than whether it is the one the record was built against. Presence is the easy question
/// and it is answered yes by the wrong version too.
#[test]
fn a_record_built_against_a_moved_dependency_is_refused() {
    let record =
        MemoryRecord::new(record_id("rec-dependency-freshness")).depending_on("policy", "v1");

    // Landmark: the unmoved dependency is accepted, so the refusal is about the VERSION.
    check_dependency_freshness(&record, &[("policy", "v1")])
        .expect("HARNESS-BROKE: an unmoved dependency was refused");

    let refusal = check_dependency_freshness(&record, &[("policy", "v2")])
        .expect_err("a record built against a moved dependency was accepted");
    assert_eq!(refusal.code(), MemoryRefusalCode::DependencyStale);
}

/// The COMPOSITION, which no guard above covers.
///
/// G1 proves capture holds the opt-in. G9 proves admission refuses a secret. G2 proves admission
/// refuses a foreign scope. Every half is green, and nothing drives a secret THROUGH capture -- so
/// the one path a caller actually uses can touch five boundaries with a credential in hand while
/// every unit test stays green.
///
/// The policy file already says so: scope, origin and secret are all `runsBeforeFirstTouch: true`.
/// The declaration was correct and the wiring was absent, which is the failure that reads as
/// coverage.
#[test]
fn capture_refuses_secret_bearing_content_before_touching_any_boundary() {
    let mut touches = Vec::new();

    let outcome = capture_memory(
        CaptureOptIn::Enabled,
        &scope(),
        &format!("please remember my token {SENTINEL}"),
        &mut touches,
    );

    assert!(
        outcome.is_err(),
        "capture accepted content carrying a credential"
    );
    assert_eq!(
        outcome.unwrap_err().code(),
        MemoryRefusalCode::SecretDetected
    );
    assert_eq!(
        touches,
        Vec::new(),
        "capture touched a boundary before refusing credential-bearing content; a disabled project \
         is held above the first touch and a secret-bearing one is not"
    );
}

/// The declared boundary of this task's secret detector, asserted rather than left to be found.
///
/// `core/graph/src/persistence.rs` holds the repository's real detector -- two dozen prefixes plus
/// JWT, PEM and secret-URI forms -- and it is private to that crate. Copying its list here would
/// duplicate an ORACLE rather than a mechanism, and a duplicated oracle diverges in SILENCE: the
/// two lists drift and "refused" quietly comes to mean two different things.
///
/// So the narrow list is deliberate, and this test is what makes it deliberate rather than
/// forgotten. It fails if someone widens the detector without widening the declaration, and it
/// fails if someone narrows it.
#[test]
fn the_secret_detector_covers_the_shape_it_declares_and_no_other() {
    let into = scope();

    let caught = MemoryCandidate::draft(scope(), format!("token {SENTINEL}"));
    assert_eq!(
        admit_memory_candidate(&caught, &into)
            .expect_err("the declared shape must be refused")
            .code(),
        MemoryRefusalCode::SecretDetected
    );

    // NOT covered, and the test says so out loud. This is a real credential shape the repository's
    // own detector refuses; this task's does not reach it.
    let missed = MemoryCandidate::draft(scope(), "token sk-abcdefghijklmnopqrst");
    assert!(
        admit_memory_candidate(&missed, &into).is_ok(),
        "the detector grew past its declared boundary; widen the declaration and the doc comment \
         in the same change, or consume the repository detector instead of duplicating its list"
    );
}

/// The wire vocabulary and the shipped policy file are two declarations of one closed set.
///
/// The enum is compiled and the YAML is shipped; nothing makes them agree. A code added to one and
/// not the other produces a refusal the policy never declared, or a policy entry no code can
/// produce -- and both read as coverage from whichever side you happen to be reading.
#[test]
fn every_refusal_code_is_declared_in_the_shipped_policy_and_the_reverse() {
    let policy = std::fs::read_to_string(
        "../../extensions/builtin/graphhelm-development-contracts/policies/memory-admission.yaml",
    )
    .expect("HARNESS-BROKE: the shipped policy file is not readable from the crate root");

    let mut declared: Vec<String> = Vec::new();
    let mut inside = false;
    for line in policy.lines() {
        if line.starts_with("refusalCodes:") {
            inside = true;
            continue;
        }
        if inside {
            match line.strip_prefix("  - ") {
                Some(code) => declared.push(code.trim().to_owned()),
                None if line.trim().is_empty() || line.trim_start().starts_with('#') => {}
                None => break,
            }
        }
    }

    assert!(
        !declared.is_empty(),
        "HARNESS-BROKE: no refusal codes were parsed out of the policy, so the comparison below \
         would pass against an empty set"
    );

    let compiled: Vec<String> = MemoryRefusalCode::every()
        .iter()
        .map(|code| code.wire_name().to_owned())
        .collect();

    let missing_from_policy: Vec<&String> =
        compiled.iter().filter(|c| !declared.contains(c)).collect();
    let missing_from_enum: Vec<&String> =
        declared.iter().filter(|c| !compiled.contains(c)).collect();

    assert!(
        missing_from_policy.is_empty() && missing_from_enum.is_empty(),
        "the compiled refusal vocabulary and the shipped policy disagree.
  compiled ({}): {compiled:?}
  declared ({}): {declared:?}
  compiled but NOT in the policy: {missing_from_policy:?}
  in the policy but NOT compiled: {missing_from_enum:?}",
        compiled.len(),
        declared.len()
    );
}

const PACKAGE: &str = "../../extensions/builtin/graphhelm-development-contracts";

/// The refusal vocabulary written out BY HAND, for the same reason as `SEMANTIC_STATE_NAMES`: it
/// is the only witness that can contradict the generated one. Deliberately not derived. See the
/// note on `SEMANTIC_STATE_NAMES` before deleting it as redundant.
const REFUSAL_CODE_NAMES: [&str; 9] = [
    "opt_in_absent",
    "scope_mismatch",
    "recapture_loop",
    "secret_detected",
    "self_validated",
    "transition_not_allowed",
    "reseal_failed",
    "dependency_stale",
    "handoff_target_not_opted_in",
];

fn package_json(relative: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(format!("{PACKAGE}/{relative}"))
        .unwrap_or_else(|error| panic!("HARNESS-BROKE: {relative} is not readable: {error}"));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("HARNESS-BROKE: {relative} is not JSON: {error}"))
}

/// The shipped SCHEMA is the third declaration of the refusal vocabulary, and until now nothing
/// read it. The cross-check above compares the enum against the POLICY; the schema sat beside them
/// unguarded, and the three fixtures were inert -- including the one whose filename asserts a
/// property (`admission-refusal-code-outside-the-closed-set`) that nothing validated.
///
/// Nothing is wrong today: the three lists agree. It was UNGUARDED, and the artifacts that look
/// like the guard were not one.
#[test]
fn the_schema_the_enum_and_the_hand_written_list_are_one_vocabulary() {
    let schema = package_json("schemas/memory-admission.schema.json");

    let from_schema: Vec<String> = schema["$defs"]["refusalCode"]["enum"]
        .as_array()
        .expect("HARNESS-BROKE: the schema has no $defs/refusalCode/enum to read")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("HARNESS-BROKE: a refusal code in the schema is not a string")
                .to_owned()
        })
        .collect();

    assert!(
        !from_schema.is_empty(),
        "HARNESS-BROKE: the schema declared no refusal codes, so every comparison below would pass \
         against an empty set"
    );

    let compiled: Vec<String> = MemoryRefusalCode::every()
        .iter()
        .map(|code| code.wire_name().to_owned())
        .collect();
    let hand: Vec<String> = REFUSAL_CODE_NAMES.iter().map(|n| (*n).to_owned()).collect();

    for (left_name, left, right_name, right) in [
        ("schema", &from_schema, "compiled", &compiled),
        ("hand-written", &hand, "schema", &from_schema),
    ] {
        let only_left: Vec<&String> = left.iter().filter(|v| !right.contains(v)).collect();
        let only_right: Vec<&String> = right.iter().filter(|v| !left.contains(v)).collect();
        assert!(
            only_left.is_empty() && only_right.is_empty(),
            "the {left_name} and {right_name} refusal vocabularies disagree.
  {left_name} ({}): {left:?}
  {right_name} ({}): {right:?}
  in {left_name} but NOT in {right_name}: {only_left:?}
  in {right_name} but NOT in {left_name}: {only_right:?}",
            left.len(),
            right.len()
        );
    }
}

/// The three shipped fixtures, exercised. A fixture nobody loads is a filename making a claim.
///
/// Each negative must be refused for ITS OWN reason: "some diagnostic appeared" is satisfied by a
/// schema that rejects everything, and a subset check passes vacuously over an empty set.
#[test]
fn the_shipped_fixtures_are_accepted_and_refused_for_their_own_reasons() {
    let admission = package_json("schemas/memory-admission.schema.json");
    let transition = package_json("schemas/memory-transition.schema.json");

    let accepted = graphhelm_schema::validate_inline_value(
        &admission,
        &package_json("fixtures/memory/valid/admission-minimal.json"),
        "fixture",
    )
    .expect("the admission schema compiles");
    assert!(
        accepted.is_empty(),
        "the positive fixture must validate against its own schema: {accepted:?}"
    );

    for (schema, fixture, expected_path) in [
        (
            &admission,
            "fixtures/memory/invalid/admission-refusal-code-outside-the-closed-set.json",
            "/refusalCodes/1",
        ),
        (
            &transition,
            "fixtures/memory/invalid/transition-tuple-missing-its-target.json",
            "/allowedPublicationTransitions/0",
        ),
        // Unknown fields: `additionalProperties: false` is declared in the schema and, until this
        // case, nothing exercised it. A declared constraint with no fixture is the same shape as a
        // declared check with no wiring.
        (
            &admission,
            "fixtures/memory/invalid/admission-unknown-field.json",
            "/orderedChecks/0",
        ),
    ] {
        let refused =
            graphhelm_schema::validate_inline_value(schema, &package_json(fixture), "fixture")
                .expect("the schema compiles");

        assert!(
            !refused.is_empty(),
            "the negative fixture {fixture} was accepted by its schema"
        );
        assert!(
            refused.iter().any(|d| d.path.starts_with(expected_path)),
            "the negative fixture {fixture} was refused, but not at {expected_path} -- so it is not \
             failing for the reason its name claims: {:?}",
            refused.iter().map(|d| d.path.clone()).collect::<Vec<_>>()
        );
    }
}

/// ADR-032: the semantic-state vocabulary written out BY HAND, for the same reason as
/// `REFUSAL_CODE_NAMES`: it is the only witness that can contradict the generated one. Wire-
/// identical to `memory_record.status` in `docs/architecture/DATA_AND_PROTOCOLS.md` §16.
const SEMANTIC_STATE_WIRE_NAMES: [&str; 5] = [
    "candidate",
    "validated",
    "contradicted",
    "deprecated",
    "expired",
];

/// ADR-032: the publication-state vocabulary written out BY HAND, same reason.
const PUBLICATION_STATE_WIRE_NAMES: [&str; 4] =
    ["unpublished", "proposed", "published", "withdrawn"];

/// ADR-032: the publication-transition vocabulary written out BY HAND, same reason.
const PUBLICATION_TRANSITION_WIRE_NAMES: [&str; 3] = ["propose", "publish", "withdraw"];

/// ADR-032: the supersession-reason vocabulary written out BY HAND, same reason.
const SUPERSESSION_REASON_WIRE_NAMES: [&str; 2] = ["contradicted", "deprecated"];

/// A minimal YAML list reader: everything under `header` (e.g. `"states:"`) indented as
/// `  - item`, stopping at the first non-list, non-blank, non-comment line. Matches the shape
/// `every_refusal_code_is_declared_in_the_shipped_policy_and_the_reverse` reads inline above;
/// factored out here because this file reads TWO lists (`states:`, `transitions:`) out of the
/// SAME document rather than one out of its own.
fn parse_yaml_list(text: &str, header: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with(header) {
            inside = true;
            continue;
        }
        if inside {
            match line.strip_prefix("  - ") {
                Some(item) => items.push(item.trim().to_owned()),
                None if line.trim().is_empty() || line.trim_start().starts_with('#') => {}
                None => break,
            }
        }
    }
    items
}

/// ADR-032: the four lifecycle vocabularies (both axes, publication transitions, and supersession
/// reasons) now carry wire spellings via the same `closed_vocabulary!` shape #362 gave
/// `MemoryState`/`MemoryTransition`, which this ADR retires.
///
/// Binds all four against the shipped policy, in both directions, the same shape
/// `every_refusal_code_is_declared_in_the_shipped_policy_and_the_reverse` uses for
/// `MemoryRefusalCode` above.
#[test]
fn every_lifecycle_vocabulary_is_declared_in_the_shipped_policy_and_the_reverse() {
    let policy = std::fs::read_to_string(
        "../../extensions/builtin/graphhelm-development-contracts/policies/memory-transition.yaml",
    )
    .expect("HARNESS-BROKE: the shipped policy file is not readable from the crate root");

    let semantic_declared = parse_yaml_list(&policy, "semanticStates:");
    let publication_declared = parse_yaml_list(&policy, "publicationStates:");
    let transitions_declared = parse_yaml_list(&policy, "publicationTransitions:");
    let reasons_declared = parse_yaml_list(&policy, "supersessionReasons:");

    assert!(
        !semantic_declared.is_empty()
            && !publication_declared.is_empty()
            && !transitions_declared.is_empty()
            && !reasons_declared.is_empty(),
        "HARNESS-BROKE: at least one lifecycle vocabulary parsed empty out of the policy, so the \
         comparison below would pass against an empty set"
    );

    let semantic_compiled: Vec<String> = MemorySemanticState::every()
        .iter()
        .map(|state| state.wire_name().to_owned())
        .collect();
    let publication_compiled: Vec<String> = MemoryPublicationState::every()
        .iter()
        .map(|state| state.wire_name().to_owned())
        .collect();
    let transitions_compiled: Vec<String> = MemoryPublicationTransition::every()
        .iter()
        .map(|transition| transition.wire_name().to_owned())
        .collect();
    let reasons_compiled: Vec<String> = SupersessionReason::every()
        .iter()
        .map(|reason| reason.wire_name().to_owned())
        .collect();

    for (what, compiled, declared) in [
        ("semantic state", &semantic_compiled, &semantic_declared),
        (
            "publication state",
            &publication_compiled,
            &publication_declared,
        ),
        (
            "publication transition",
            &transitions_compiled,
            &transitions_declared,
        ),
        ("supersession reason", &reasons_compiled, &reasons_declared),
    ] {
        let missing_from_policy: Vec<&String> =
            compiled.iter().filter(|c| !declared.contains(c)).collect();
        let missing_from_enum: Vec<&String> =
            declared.iter().filter(|c| !compiled.contains(c)).collect();

        assert!(
            missing_from_policy.is_empty() && missing_from_enum.is_empty(),
            "the compiled {what} vocabulary and the shipped policy disagree.
  compiled ({}): {compiled:?}
  declared ({}): {declared:?}
  compiled but NOT in the policy: {missing_from_policy:?}
  in the policy but NOT compiled: {missing_from_enum:?}",
            compiled.len(),
            declared.len()
        );
    }
}

/// ADR-032: the shipped SCHEMA is a third declaration of the four lifecycle vocabularies, and the
/// reason a co-drift (renaming a spelling in the policy AND in the enum together) is caught: the
/// schema is a spelling nobody touches for a policy or Rust-side rename, so it is the independent
/// witness the two-way bind above cannot provide alone.
#[test]
fn the_schema_the_enum_and_the_hand_written_list_are_one_lifecycle_vocabulary() {
    let schema = package_json("schemas/memory-transition.schema.json");

    let read_schema_enum = |defs_key: &str, what: &str| -> Vec<String> {
        schema["$defs"][defs_key]["enum"]
            .as_array()
            .unwrap_or_else(|| {
                panic!("HARNESS-BROKE: the schema has no $defs/{defs_key}/enum to read")
            })
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| {
                        panic!("HARNESS-BROKE: a {what} in the schema is not a string")
                    })
                    .to_owned()
            })
            .collect()
    };

    let from_schema_semantic = read_schema_enum("semanticState", "semantic state");
    let from_schema_publication = read_schema_enum("publicationState", "publication state");
    let from_schema_transitions =
        read_schema_enum("publicationTransition", "publication transition");
    let from_schema_reasons = read_schema_enum("supersessionReason", "supersession reason");

    assert!(
        !from_schema_semantic.is_empty()
            && !from_schema_publication.is_empty()
            && !from_schema_transitions.is_empty()
            && !from_schema_reasons.is_empty(),
        "HARNESS-BROKE: the schema declared at least one empty lifecycle vocabulary, so every \
         comparison below would pass against an empty set"
    );

    let compiled_semantic: Vec<String> = MemorySemanticState::every()
        .iter()
        .map(|state| state.wire_name().to_owned())
        .collect();
    let compiled_publication: Vec<String> = MemoryPublicationState::every()
        .iter()
        .map(|state| state.wire_name().to_owned())
        .collect();
    let compiled_transitions: Vec<String> = MemoryPublicationTransition::every()
        .iter()
        .map(|transition| transition.wire_name().to_owned())
        .collect();
    let compiled_reasons: Vec<String> = SupersessionReason::every()
        .iter()
        .map(|reason| reason.wire_name().to_owned())
        .collect();

    let hand_semantic: Vec<String> = SEMANTIC_STATE_WIRE_NAMES
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
    let hand_publication: Vec<String> = PUBLICATION_STATE_WIRE_NAMES
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
    let hand_transitions: Vec<String> = PUBLICATION_TRANSITION_WIRE_NAMES
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
    let hand_reasons: Vec<String> = SUPERSESSION_REASON_WIRE_NAMES
        .iter()
        .map(|n| (*n).to_owned())
        .collect();

    for (left_name, left, right_name, right) in [
        (
            "schema",
            &from_schema_semantic,
            "compiled",
            &compiled_semantic,
        ),
        (
            "hand-written",
            &hand_semantic,
            "schema",
            &from_schema_semantic,
        ),
        (
            "schema",
            &from_schema_publication,
            "compiled",
            &compiled_publication,
        ),
        (
            "hand-written",
            &hand_publication,
            "schema",
            &from_schema_publication,
        ),
        (
            "schema",
            &from_schema_transitions,
            "compiled",
            &compiled_transitions,
        ),
        (
            "hand-written",
            &hand_transitions,
            "schema",
            &from_schema_transitions,
        ),
        (
            "schema",
            &from_schema_reasons,
            "compiled",
            &compiled_reasons,
        ),
        (
            "hand-written",
            &hand_reasons,
            "schema",
            &from_schema_reasons,
        ),
    ] {
        let only_left: Vec<&String> = left.iter().filter(|v| !right.contains(v)).collect();
        let only_right: Vec<&String> = right.iter().filter(|v| !left.contains(v)).collect();
        assert!(
            only_left.is_empty() && only_right.is_empty(),
            "the {left_name} and {right_name} lifecycle vocabularies disagree.
  {left_name} ({}): {left:?}
  {right_name} ({}): {right:?}
  in {left_name} but NOT in {right_name}: {only_left:?}
  in {right_name} but NOT in {left_name}: {only_right:?}",
            left.len(),
            right.len()
        );
    }
}

/// The harness doc's BEHAVIOURAL claim about this crate's screen, measured instead of cited (#691).
///
/// `docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md:578` says the admission screen here is
/// `content.contains("ghp_")` -- ONE prefix and NO tail requirement -- and `:599` records the
/// divergence that follows: a bare `ghp_` is refused by this crate and NOT by `core/graph`, which
/// needs a tail of at least 16.
///
/// **What the citation guard cannot see.** `every_cited_line_in_the_harness_doc_still_holds_its_symbol`
/// proves the function NAME still sits at the promised coordinate. It says nothing about whether
/// `ghp_` still matches or whether a tail became required, so a rewrite that kept the name and
/// adopted the graph crate's rule would leave every citation green while the documented divergence
/// silently disappeared. Citation rot and behaviour rot are different properties.
///
/// **The axis the existing suite never varied.** Measured across both owning crates before this
/// cell was written: every `ghp_` literal in a test carried a tail of 26 to 37 characters. All of
/// them satisfy BOTH detectors, so not one distinguishes them. Tail length is the entire content of
/// the documented divergence, and it had no coverage on either side.
///
/// The direction is the reason this is a guard and not a curiosity: this crate refuses MORE, so the
/// divergence is fail-safe. A change that made it refuse LESS would be a hole, and would show up
/// here as the bare prefix being admitted.
#[test]
fn a_bare_prefix_with_no_tail_is_refused_here_as_the_harness_doc_claims() {
    // ARRANGEMENT FIRST, and it is not decoration: without it "the candidate was refused" is true
    // of a screen that refuses everything, and the cell would pass while saying nothing about
    // `ghp_`.
    let innocuous =
        MemoryCandidate::draft(scope(), "an ordinary note with no token in it".to_owned());
    let admitting_into = innocuous.scope().clone();
    admit_memory_candidate(&innocuous, &admitting_into).expect(
        "HARNESS-BROKE: a candidate carrying no secret prefix must be admitted, or the refusal below is unattributable",
    );

    // THE PREFIX SITS AT THE END, so the tail is zero bytes and that is visible in the fixture.
    //
    // An earlier version read `"the prefix is ghp_ and nothing follows"` under a comment claiming
    // "four characters and nothing after them". Twenty characters followed. The cell still passed
    // for the right reason -- this screen is `contains`, so what follows never mattered HERE -- but
    // the comment's other half, that `core/graph` would not refuse the same string, was true only
    // because the next byte was a SPACE and that crate's tail scan stops at the first byte outside
    // `[A-Za-z0-9_-]`. A load-bearing fact about another crate, resting on an invisible byte, in a
    // comment that by this PR's own design rule no cell here may test. Changing the fixture to
    // `ghp_x` would have made it false with nothing failing anywhere. Found in review by
    // GraphHelm ISSUES.
    //
    // What `core/graph` does with a tail-less prefix is ASSERTED, not claimed, by
    // `a_tail_shorter_than_the_documented_minimum_is_not_refused_here` in that crate.
    let bare = MemoryCandidate::draft(scope(), "a token prefix with no tail: ghp_".to_owned());
    let admitting_into = bare.scope().clone();
    let refusal = admit_memory_candidate(&bare, &admitting_into).expect_err(
        "a bare `ghp_` with no tail must be refused HERE (docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md:599)",
    );

    assert_eq!(refusal.code(), MemoryRefusalCode::SecretDetected);
    assert_eq!(refusal.field(), MemoryField::Content);
}
