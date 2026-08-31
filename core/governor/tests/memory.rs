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
    MemoryOrigin, MemoryRecord, MemoryRefusalCode, MemoryState, MemoryTransition, PublicationStep,
    admit_memory_candidate, apply_transition, bind_evidence, capture_memory,
    check_dependency_freshness, publication_steps, record_memory_admission_refusal, republish,
    validate_candidate,
};
use graphhelm_protocols::{
    ActorId, ArtifactId, Clock, DevelopmentScope, EventEnvelope, EventKind, EvidenceId,
    IdGenerator, MemoryAdmissionLocal,
    MemoryAdmissionRefusalCode as PersistedMemoryAdmissionRefusalCode, OpaqueId, PersistedActor,
    PersistedActorType, ProjectId, RepositoryScope, WorkspaceId,
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

/// The vocabularies, written out BY HAND from the enum declarations.
///
/// DELIBERATELY NOT GENERATED. This looks exactly like the duplication this repository has been
/// removing -- and it is the opposite: `every()` is generated from the enum, so a guard that reads
/// `every()` on both sides is fed by the list it is checking and is green by construction. These
/// names are the only statement in the suite that can CONTRADICT the generated one, which is the
/// entire reason they exist. A future reader who deletes them to remove redundancy would be
/// applying the house rule correctly and removing the only independent witness. Update them by
/// looking at the enum, never by copying from a failure message.
const STATE_NAMES: [&str; 4] = ["Provisional", "Published", "Superseded", "Withdrawn"];
const TRANSITION_NAMES: [&str; 3] = ["Publish", "Supersede", "Withdraw"];

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

/// G3 of the blueprint. The matrix is DERIVED from the vocabularies rather than hand-listed, so a
/// tuple nobody thought about is still exercised; the allowed set stays explicit because that part
/// is policy, not enumeration.
///
/// The production change this catches: a refused transition that mutates the predecessor on its way
/// out -- the record moves and the caller is told it did not.
#[test]
fn every_state_transition_tuple_either_applies_or_leaves_the_predecessor_untouched() {
    cross_check(&STATE_NAMES, MemoryState::every(), "state");
    cross_check(&TRANSITION_NAMES, MemoryTransition::every(), "transition");

    let mut applied = 0_usize;
    let mut refused = 0_usize;

    for state in MemoryState::every() {
        for transition in MemoryTransition::every() {
            // The fixture carries a SECOND field, and the refusal arm compares the WHOLE
            // predecessor. Asserting `record.state()` alone leaves every other field of the record
            // unguarded, and with `MemoryRecord::at()` the dependency list was always empty -- so
            // no sabotage of the refusal path could turn this red on that half.
            let mut record = MemoryRecord::at(*state).depending_on("policy", "v1");
            let before = record.clone();

            match apply_transition(&mut record, *transition) {
                Ok(()) => {
                    assert_eq!(
                        record.state(),
                        transition.target(),
                        "an allowed {state:?} -> {transition:?} landed in the wrong state"
                    );
                    applied += 1;
                }
                Err(refusal) => {
                    assert_eq!(
                        record,
                        before,
                        "a refused {state:?} -> {transition:?} changed its predecessor; the \
                         refusal reported {:?}",
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
        STATE_NAMES.len() * TRANSITION_NAMES.len()
    );
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
    let record = MemoryRecord::at(MemoryState::Published).depending_on("policy", "v1");

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

/// The refusal vocabulary written out BY HAND, for the same reason as `STATE_NAMES`: it is the only
/// witness that can contradict the generated one. Deliberately not derived. See the note on
/// `STATE_NAMES` before deleting it as redundant.
const REFUSAL_CODE_NAMES: [&str; 8] = [
    "opt_in_absent",
    "scope_mismatch",
    "recapture_loop",
    "secret_detected",
    "self_validated",
    "transition_not_allowed",
    "reseal_failed",
    "dependency_stale",
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
            "/allowed/0",
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

/// #362: the state vocabulary written out BY HAND, for the same reason as `REFUSAL_CODE_NAMES`:
/// it is the only witness that can contradict the generated one. Deliberately not derived.
const STATE_WIRE_NAMES: [&str; 4] = ["provisional", "published", "superseded", "withdrawn"];

/// #362: the transition vocabulary written out BY HAND, for the same reason as
/// `STATE_WIRE_NAMES`.
const TRANSITION_WIRE_NAMES: [&str; 3] = ["publish", "supersede", "withdraw"];

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

/// #362: `MemoryState` and `MemoryTransition` now carry wire spellings, closing the co-drift gap
/// named in `apps/cli/tests/development_cli.rs`'s former `wire_state`/`wire_transition`
/// functions -- removed by that change, since the guard that made them a stopgap now exists
/// here instead.
///
/// Binds BOTH vocabularies against the shipped policy, in both directions, the same shape
/// `every_refusal_code_is_declared_in_the_shipped_policy_and_the_reverse` uses for
/// `MemoryRefusalCode` above.
#[test]
fn every_memory_state_and_transition_is_declared_in_the_shipped_policy_and_the_reverse() {
    let policy = std::fs::read_to_string(
        "../../extensions/builtin/graphhelm-development-contracts/policies/memory-transition.yaml",
    )
    .expect("HARNESS-BROKE: the shipped policy file is not readable from the crate root");

    let states_declared = parse_yaml_list(&policy, "states:");
    let transitions_declared = parse_yaml_list(&policy, "transitions:");

    assert!(
        !states_declared.is_empty() && !transitions_declared.is_empty(),
        "HARNESS-BROKE: no states or transitions were parsed out of the policy, so the \
         comparison below would pass against an empty set"
    );

    let states_compiled: Vec<String> = MemoryState::every()
        .iter()
        .map(|state| state.wire_name().to_owned())
        .collect();
    let transitions_compiled: Vec<String> = MemoryTransition::every()
        .iter()
        .map(|transition| transition.wire_name().to_owned())
        .collect();

    for (what, compiled, declared) in [
        ("state", &states_compiled, &states_declared),
        ("transition", &transitions_compiled, &transitions_declared),
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

/// #362: the shipped SCHEMA is a third declaration of the state and transition vocabularies, and
/// the reason a co-drift (renaming a spelling in the policy AND in the enum together) is caught:
/// the schema is a spelling nobody touches for a policy or Rust-side rename, so it is the
/// independent witness the two-way bind above cannot provide alone.
#[test]
fn the_schema_the_enum_and_the_hand_written_list_are_one_state_and_transition_vocabulary() {
    let schema = package_json("schemas/memory-transition.schema.json");

    let from_schema_states: Vec<String> = schema["$defs"]["state"]["enum"]
        .as_array()
        .expect("HARNESS-BROKE: the schema has no $defs/state/enum to read")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("HARNESS-BROKE: a state in the schema is not a string")
                .to_owned()
        })
        .collect();
    let from_schema_transitions: Vec<String> = schema["$defs"]["transition"]["enum"]
        .as_array()
        .expect("HARNESS-BROKE: the schema has no $defs/transition/enum to read")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("HARNESS-BROKE: a transition in the schema is not a string")
                .to_owned()
        })
        .collect();

    assert!(
        !from_schema_states.is_empty() && !from_schema_transitions.is_empty(),
        "HARNESS-BROKE: the schema declared no states or transitions, so every comparison below \
         would pass against an empty set"
    );

    let compiled_states: Vec<String> = MemoryState::every()
        .iter()
        .map(|state| state.wire_name().to_owned())
        .collect();
    let compiled_transitions: Vec<String> = MemoryTransition::every()
        .iter()
        .map(|transition| transition.wire_name().to_owned())
        .collect();
    let hand_states: Vec<String> = STATE_WIRE_NAMES.iter().map(|n| (*n).to_owned()).collect();
    let hand_transitions: Vec<String> = TRANSITION_WIRE_NAMES
        .iter()
        .map(|n| (*n).to_owned())
        .collect();

    for (left_name, left, right_name, right) in [
        ("schema", &from_schema_states, "compiled", &compiled_states),
        ("hand-written", &hand_states, "schema", &from_schema_states),
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
    ] {
        let only_left: Vec<&String> = left.iter().filter(|v| !right.contains(v)).collect();
        let only_right: Vec<&String> = right.iter().filter(|v| !left.contains(v)).collect();
        assert!(
            only_left.is_empty() && only_right.is_empty(),
            "the {left_name} and {right_name} state/transition vocabularies disagree.
  {left_name} ({}): {left:?}
  {right_name} ({}): {right:?}
  in {left_name} but NOT in {right_name}: {only_left:?}
  in {right_name} but NOT in {left_name}: {only_right:?}",
            left.len(),
            right.len()
        );
    }
}
