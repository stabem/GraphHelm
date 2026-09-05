//! Governed memory admission (#220).
//!
//! Admission runs BEFORE a candidate is constructed or handed to a provider, so the interesting
//! failure is not that admission lets a secret through — it is that the REFUSAL becomes the thing
//! that persists it. A refusal is an event; "nothing has been persisted yet" is true of the instant
//! before it and false of the refusal itself.

use graphhelm_events::{
    EventRepository, EventRepositoryError, MemoryAdmissionRefusalAppend,
    MemoryPublicationTransitionAppend, MemoryRecordSupersededAppend,
    prepare_memory_admission_refusal, prepare_memory_publication_transition,
    prepare_memory_record_superseded,
};
use graphhelm_protocols::{
    DevelopmentScope, EventEnvelope, MemoryAdmissionLocal,
    MemoryAdmissionRefusalCode as PersistedMemoryAdmissionRefusalCode, OpaqueId, PersistedActor,
    PersistedMemoryPublicationState, PersistedMemoryPublicationTransition,
    PersistedMemorySemanticState, PersistedSupersessionReason, RepositoryScope,
};
use std::fmt;

/// One list generates the enum, its `every()` AND its wire spelling, so the three cannot disagree.
///
/// Proximity is not enforcement: written separately, the compiler forces an arm in a `match` but
/// nothing forces an entry in `every()`, and a variant missing from `every()` is invisible to any
/// guard that iterates it. A guard fed by the list it is checking is green by construction. The
/// contract lane measured that exact failure before this task existed; this shape removes the
/// possibility rather than adding a check for it.
macro_rules! closed_vocabulary {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// Every variant, generated from the same list as the enum itself.
            #[must_use]
            pub const fn every() -> &'static [Self] {
                &[$(Self::$variant),+]
            }
        }
    };
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// Every variant, generated from the same list as the enum itself.
            #[must_use]
            pub const fn every() -> &'static [Self] {
                &[$(Self::$variant),+]
            }

            /// This variant's wire spelling, from the same list as the enum itself.
            #[must_use]
            pub const fn wire_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }
        }
    };
}

closed_vocabulary! {
    /// Where in a candidate a refusal was raised.
    MemoryField {
        /// The candidate's free content.
        Content => "content",
        /// The record's lifecycle state.
        State => "state",
        /// The scope the candidate was captured under.
        Scope => "scope",
        /// The set of parties validating the candidate.
        Validators => "validators",
        /// Where the content came from.
        Origin => "origin",
        /// The record's sealed evidence.
        Evidence => "evidence",
        /// A dependency the record was built against.
        Dependency => "dependency",
    }
}

closed_vocabulary! {
    /// Why a candidate was refused. Closed: an unknown reason is refused, never ignored.
    ///
    /// JURISDICTION. This vocabulary belongs to the MEMORY ADMISSION contract, not to the central
    /// `DevelopmentRefusalCode` of the development envelope, under the vocabulary-jurisdiction
    /// ruling recorded in the body of #216: a vocabulary belongs to the contract that carries it.
    /// The three conditions are met here -- these codes never cross the development envelope, they
    /// are paired with a shipped schema and an equality guard in `tests/memory.rs`, and this
    /// paragraph is the declaration the ruling requires. A code needed by a lane that DOES cross
    /// the envelope is allocated centrally instead, never minted here.
    ///
    /// The wire spellings are the AUTHORITATIVE side shared with `policies/memory-admission.yaml`
    /// and `schemas/memory-admission.schema.json`; all three are compared in both directions.
    MemoryRefusalCode {
        /// The project did not opt in to durable memory capture.
        OptInAbsent => "opt_in_absent",
        /// The candidate was captured under a different scope than the one admitting it.
        ScopeMismatch => "scope_mismatch",
        /// The content came back from a memory provider and was offered as fresh capture.
        RecaptureLoop => "recapture_loop",
        /// The content carried something that matches a credential shape.
        SecretDetected => "secret_detected",
        /// The only party validating the candidate is the party that produced it.
        SelfValidated => "self_validated",
        /// The requested state/transition tuple is not allowed.
        TransitionNotAllowed => "transition_not_allowed",
        /// The evidence could not be resealed for this record.
        ResealFailed => "reseal_failed",
        /// A dependency the record was built against has moved.
        DependencyStale => "dependency_stale",
        /// A handoff targeted a scope that has not opted in to durable memory capture.
        HandoffTargetNotOptedIn => "handoff_target_not_opted_in",
    }
}

/// The record a refusal leaves behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRefusal {
    code: MemoryRefusalCode,
    field: MemoryField,
}

impl MemoryRefusal {
    /// The closed code for this refusal.
    #[must_use]
    pub fn code(&self) -> MemoryRefusalCode {
        self.code
    }

    /// Where the refusal was raised.
    #[must_use]
    pub fn field(&self) -> MemoryField {
        self.field
    }
}

impl fmt::Display for MemoryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "memory candidate refused: {:?} at {:?}",
            self.code, self.field
        )
    }
}

/// A candidate for durable memory, before admission.
#[derive(Clone, Debug)]
pub struct MemoryCandidate {
    scope: DevelopmentScope,
    content: String,
    produced_by: Option<String>,
    origin: MemoryOrigin,
}

/// Where a candidate's content came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryOrigin {
    /// Written by the user or the working session.
    Captured,
    /// Returned by a memory provider. Offering it back as fresh capture launders it.
    Provider,
}

impl MemoryCandidate {
    /// A candidate that has not been admitted.
    #[must_use]
    pub fn draft(scope: DevelopmentScope, content: impl Into<String>) -> Self {
        Self {
            scope,
            content: content.into(),
            produced_by: None,
            origin: MemoryOrigin::Captured,
        }
    }

    /// A candidate built from what a memory provider returned.
    ///
    /// The origin is stamped HERE, at the boundary that knows it, rather than left for a caller to
    /// remember: a guard that reads a mark nobody sets refuses exactly never.
    #[must_use]
    pub fn from_provider(scope: DevelopmentScope, content: impl Into<String>) -> Self {
        Self {
            origin: MemoryOrigin::Provider,
            ..Self::draft(scope, content)
        }
    }

    /// Where this candidate's content came from.
    #[must_use]
    pub const fn origin(&self) -> MemoryOrigin {
        self.origin
    }

    /// The same candidate, attributed to the party that produced it.
    #[must_use]
    pub fn produced_by(mut self, producer: impl Into<String>) -> Self {
        self.produced_by = Some(producer.into());
        self
    }

    /// The scope this candidate was captured under.
    #[must_use]
    pub fn scope(&self) -> &DevelopmentScope {
        &self.scope
    }
}

/// The checks that must complete BEFORE any boundary is touched.
///
/// Shared by admission and capture on purpose. Written twice, the two paths drift: one of them
/// gains a check and the other keeps passing, and every unit test stays green because each half is
/// tested alone. `policies/memory-admission.yaml` marks these `runsBeforeFirstTouch: true`, and a
/// declaration whose wiring is absent reads as coverage.
fn screen(
    scope: &DevelopmentScope,
    admitting_into: &DevelopmentScope,
    origin: MemoryOrigin,
    content: &str,
) -> Result<(), MemoryRefusal> {
    // The WHOLE scope is compared, not a field of it. Comparing the project alone lets content
    // cross workspaces whenever two tenants name a project the same way -- and identical project
    // names across tenants is the normal case, not the exotic one. Comparing the struct means a
    // field added to DevelopmentScope is covered the day it lands, instead of the day someone
    // remembers to add it here.
    if scope != admitting_into {
        return Err(MemoryRefusal {
            code: MemoryRefusalCode::ScopeMismatch,
            field: MemoryField::Scope,
        });
    }

    // Matched exhaustively: an origin added later must be classified here rather than inheriting
    // whichever arm happens to be written first.
    match origin {
        MemoryOrigin::Provider => {
            return Err(MemoryRefusal {
                code: MemoryRefusalCode::RecaptureLoop,
                field: MemoryField::Origin,
            });
        }
        MemoryOrigin::Captured => {}
    }

    if content_is_secret_shaped(content) {
        // The record names the code and the location. It never names the value: the refusal is
        // itself a persisted event, so quoting the secret here would carry it into the journal
        // through the one path this design argued was safe.
        return Err(MemoryRefusal {
            code: MemoryRefusalCode::SecretDetected,
            field: MemoryField::Content,
        });
    }

    Ok(())
}

/// WHAT THIS SCREEN DOES NOT REJECT, declared rather than left to be inferred from the cases that
/// are covered.
///
/// #220 names six rejection classes. Three are enforced above -- cross-scope content, provider
/// output offered back as fresh capture, and credential-shaped content within the boundary declared
/// below. The other three are NOT, and each has a different reason:
///
/// - **Raw prompt or chat transcript.** `MemoryOrigin` distinguishes `Captured` from `Provider` and
///   nothing else, so a transcript pasted into `content` is indistinguishable from a note a person
///   wrote. Distinguishing them needs a capture surface that carries the difference, and no such
///   surface exists in this task's scope: everything reaches capture as one opaque `&str`.
///
/// - **Broad tool output.** Same cause. There is no origin for it to be marked with, so there is
///   nothing for a check to read. A check over a mark nobody sets refuses exactly never.
///
/// - **Unknown fields.** Covered for the POLICY documents by `additionalProperties: false` in both
///   shipped schemas, exercised by `fixtures/memory/invalid/admission-unknown-field.json`. NOT
///   covered for candidate content, which is not a structured document here.
///
/// Whoever adds the first typed capture surface inherits the first two.
///
/// (Attached to the screen itself so it is read where the screening happens.)
/// The credential shapes THIS task refuses, and the boundary is deliberate.
///
/// `core/graph/src/persistence.rs` carries the repository's real detector -- roughly two dozen
/// prefixes plus JWT, PEM and secret-URI forms -- and it is private to that crate. Copying its list
/// here would duplicate an ORACLE rather than a mechanism: a duplicated mechanism diverges loudly,
/// a duplicated oracle diverges in silence and quietly changes what "refused" means.
///
/// So this list is narrow ON PURPOSE and the boundary is asserted by a test rather than left to be
/// discovered. Widening it belongs to whoever exports the real detector or amends this task's
/// scope to reach it.
fn content_is_secret_shaped(content: &str) -> bool {
    content.contains("ghp_")
}

/// Admit a candidate, or refuse it.
///
/// # Errors
///
/// Returns [`MemoryRefusal`] when the candidate must not be admitted.
pub fn admit_memory_candidate(
    candidate: &MemoryCandidate,
    admitting_into: &DevelopmentScope,
) -> Result<(), MemoryRefusal> {
    screen(
        &candidate.scope,
        admitting_into,
        candidate.origin,
        &candidate.content,
    )
}

/// Whether a project has opted in to durable memory capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOptIn {
    /// The project opted in.
    Enabled,
    /// The project did not.
    Disabled,
}

/// A boundary that capture touched. Recorded so that "touched nothing" is observable rather than
/// inferred from a missing return value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureTouch {
    /// A candidate was constructed from the content.
    CandidateConstructed,
    /// A memory provider was asked for anything at all.
    ProviderQueried,
    /// A line was written to a log or a tracing span. A log line is a touch: it is the one people
    /// forget, because it does not feel like a boundary until the content is in it.
    LogWritten,
    /// An event was appended to the journal.
    EventAppended,
    /// Anything durable was written.
    PersistentBoundaryTouched,
}

/// Capture content as a memory candidate.
///
/// # Errors
///
/// Returns [`MemoryRefusal`] when the project did not opt in, or when the content fails any check
/// that must run before a boundary is touched.
/// The scope is taken by REFERENCE, matching `admit_memory_candidate`. Taken by value it forced
/// every caller into a clone, because admission needs the same scope immediately afterwards -- the
/// first real consumer hit exactly that and the friction was reported from outside. A signature
/// that makes its own caller clone is charging for the callee's convenience.
pub fn capture_memory(
    opt_in: CaptureOptIn,
    scope: &DevelopmentScope,
    content: &str,
    touches: &mut Vec<CaptureTouch>,
) -> Result<MemoryCandidate, MemoryRefusal> {
    // Everything below this line runs ABOVE the first boundary touch, not beside the return value.
    // Below it, a disabled project still constructs the candidate and only then declines to hand it
    // back -- legal, invisible, and already past the boundary the opt-in exists to hold.
    //
    // Matched exhaustively without a wildcard on purpose: a third opt-in state must not inherit
    // whichever arm happens to be written first.
    match opt_in {
        CaptureOptIn::Disabled => {
            return Err(MemoryRefusal {
                code: MemoryRefusalCode::OptInAbsent,
                field: MemoryField::Origin,
            });
        }
        CaptureOptIn::Enabled => {}
    }

    // Capture is the path a caller actually uses. Screening only inside admission left this path
    // able to touch five boundaries with a credential in hand while every unit guard stayed green:
    // each half proven, the composition never exercised.
    screen(scope, scope, MemoryOrigin::Captured, content)?;

    let candidate = MemoryCandidate::draft(scope.clone(), content);
    touches.push(CaptureTouch::CandidateConstructed);
    touches.push(CaptureTouch::ProviderQueried);
    touches.push(CaptureTouch::LogWritten);
    touches.push(CaptureTouch::EventAppended);
    touches.push(CaptureTouch::PersistentBoundaryTouched);
    Ok(candidate)
}

/// Refuse a handoff into a project scope that has not opted in to durable memory capture.
///
/// ADR-032 decision 5: named separately from [`MemoryRefusalCode::OptInAbsent`] on purpose.
/// `OptInAbsent` refuses a project's OWN capture; this refuses a handoff's RECEIVING scope. Folding
/// the two into one code would make a persisted refusal ambiguous about which side of the handoff an
/// operator must act on -- the capturing project, or the one the handoff targets.
///
/// # Errors
///
/// Returns [`MemoryRefusal`] when `target_opt_in` is [`CaptureOptIn::Disabled`].
pub fn handoff_into_scope(target_opt_in: CaptureOptIn) -> Result<(), MemoryRefusal> {
    match target_opt_in {
        CaptureOptIn::Disabled => Err(MemoryRefusal {
            code: MemoryRefusalCode::HandoffTargetNotOptedIn,
            field: MemoryField::Scope,
        }),
        CaptureOptIn::Enabled => Ok(()),
    }
}

/// Bounded context needed to append one admission refusal.
///
/// Rejected content is borrowed only long enough to count its bytes. It is never copied into the
/// append request, diagnostics, or the event projection.
pub struct MemoryAdmissionRefusalRequest<'a> {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    idempotency_key: OpaqueId,
    actor: PersistedActor,
    rejected_content: &'a str,
}

impl<'a> MemoryAdmissionRefusalRequest<'a> {
    #[must_use]
    pub fn new(
        scope: RepositoryScope,
        stream_id: OpaqueId,
        expected_next_sequence: u64,
        idempotency_key: OpaqueId,
        actor: PersistedActor,
        rejected_content: &'a str,
    ) -> Self {
        Self {
            scope,
            stream_id,
            expected_next_sequence,
            idempotency_key,
            actor,
            rejected_content,
        }
    }
}

/// Records a refusal only for projects that explicitly enabled durable memory capture.
///
/// The disabled arm returns before invoking any repository method. The enabled arm delegates the
/// single event to the repository's atomic, idempotent append boundary.
pub fn record_memory_admission_refusal(
    opt_in: CaptureOptIn,
    repository: &dyn EventRepository,
    request: MemoryAdmissionRefusalRequest<'_>,
    refusal: &MemoryRefusal,
) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {
    match opt_in {
        CaptureOptIn::Disabled => return Ok(None),
        CaptureOptIn::Enabled => {}
    }
    let (code, local) = match (refusal.code, refusal.field) {
        (MemoryRefusalCode::ScopeMismatch, MemoryField::Scope) => (
            PersistedMemoryAdmissionRefusalCode::ScopeMismatch,
            MemoryAdmissionLocal::Scope,
        ),
        (MemoryRefusalCode::RecaptureLoop, MemoryField::Origin) => (
            PersistedMemoryAdmissionRefusalCode::RecaptureLoop,
            MemoryAdmissionLocal::Origin,
        ),
        (MemoryRefusalCode::SecretDetected, MemoryField::Content) => (
            PersistedMemoryAdmissionRefusalCode::SecretDetected,
            MemoryAdmissionLocal::Content,
        ),
        (MemoryRefusalCode::SelfValidated, MemoryField::Validators) => (
            PersistedMemoryAdmissionRefusalCode::SelfValidated,
            MemoryAdmissionLocal::Validators,
        ),
        _ => return Err(EventRepositoryError::Invalid),
    };

    let bytes = u64::try_from(request.rejected_content.len())
        .map_err(|_| EventRepositoryError::LimitExceeded)?;
    let prepared = prepare_memory_admission_refusal(MemoryAdmissionRefusalAppend::new(
        request.scope,
        request.stream_id,
        request.expected_next_sequence,
        request.idempotency_key,
        request.actor,
        code,
        local,
        bytes,
    ))?;

    repository.append_atomic(&prepared).map(Some)
}

closed_vocabulary! {
    /// Whether a memory record's content is still believed. Independent of publication (#220,
    /// ADR-032): a record can be `Contradicted` and still `Published` for a beat, or `Deprecated`
    /// while still `Unpublished`. This is not a new vocabulary invented here -- it matches
    /// `memory_record.status` in `docs/architecture/DATA_AND_PROTOCOLS.md` §16, already normative;
    /// this is the first implementation that consumes it instead of a parallel enum drifting from
    /// it. Replaces the one-axis `MemoryState` (#362), whose `Superseded` value conflated three
    /// distinct repair reasons into one name -- see [`SupersessionReason`] and [`supersede`].
    MemorySemanticState {
        /// Captured, not yet checked.
        Candidate => "candidate",
        /// Checked and believed.
        Validated => "validated",
        /// Contradicted by new evidence. A caller's stated reason, never inferred.
        Contradicted => "contradicted",
        /// Deprecated by policy, without disputing the content. A caller's stated reason, never
        /// inferred.
        Deprecated => "deprecated",
        /// Past its validity window.
        Expired => "expired",
    }
}

closed_vocabulary! {
    /// Whether a memory record is visible in the default view. Independent of semantic validity
    /// (#220, ADR-032): withdrawal moves this axis alone, never the semantic axis, and never
    /// deletes or rewrites anything durable -- opt-in governs CAPTURE, withdrawal governs the VIEW.
    MemoryPublicationState {
        /// Captured, not yet requested for publication.
        Unpublished => "unpublished",
        /// Requested, pending publication.
        Proposed => "proposed",
        /// Published and in the default view.
        Published => "published",
        /// Withdrawn from the default view. Still auditable: withdrawal closes USE, never the
        /// record.
        Withdrawn => "withdrawn",
    }
}

closed_vocabulary! {
    /// The transitions a memory record's PUBLICATION axis can be asked to make. Never touches the
    /// semantic axis -- see [`supersede`] for the operation that moves a PREDECESSOR's semantic
    /// axis, which is a relationship between two records rather than a transition on one.
    ///
    /// The wire spellings are the AUTHORITATIVE side shared with
    /// `policies/memory-transition.yaml` and `schemas/memory-transition.schema.json`.
    MemoryPublicationTransition {
        /// Unpublished becomes proposed.
        Propose => "propose",
        /// Proposed becomes published.
        Publish => "publish",
        /// Published leaves the default view.
        Withdraw => "withdraw",
    }
}

impl MemoryPublicationTransition {
    /// The publication state this transition lands in when it is allowed.
    #[must_use]
    pub const fn target(self) -> MemoryPublicationState {
        match self {
            Self::Propose => MemoryPublicationState::Proposed,
            Self::Publish => MemoryPublicationState::Published,
            Self::Withdraw => MemoryPublicationState::Withdrawn,
        }
    }
}

closed_vocabulary! {
    /// Why a predecessor is being superseded. Closed to exactly the two values ADR-032 names:
    /// there is no generic "superseded" reason, because the reason is exactly the information a
    /// caller must supply and [`supersede`] must never infer from the relationship existing.
    SupersessionReason {
        /// New evidence disagrees with the predecessor's content.
        Contradicted => "contradicted",
        /// Policy retires the predecessor without disputing its content.
        Deprecated => "deprecated",
    }
}

impl SupersessionReason {
    /// The predecessor's semantic state once this reason is recorded.
    #[must_use]
    pub const fn target(self) -> MemorySemanticState {
        match self {
            Self::Contradicted => MemorySemanticState::Contradicted,
            Self::Deprecated => MemorySemanticState::Deprecated,
        }
    }
}

/// A memory record, at some point in its life.
///
/// The two axes are independent fields, never flattened into one enum (ADR-032). `supersedes` is
/// the RELATIONSHIP that replaces the old `Superseded` STATE: this record's own `semantic` says
/// whether ITS content is still believed, while `supersedes` says whether it displaces an earlier
/// record -- a different question, decided by [`supersede`], never by this type inferring one from
/// the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRecord {
    id: OpaqueId,
    semantic: MemorySemanticState,
    publication: MemoryPublicationState,
    supersedes: Option<OpaqueId>,
    dependencies: Vec<(String, String)>,
}

impl MemoryRecord {
    /// A fresh record with the given identity: `Candidate` and `Unpublished`, superseding nothing.
    #[must_use]
    pub fn new(id: OpaqueId) -> Self {
        Self {
            id,
            semantic: MemorySemanticState::Candidate,
            publication: MemoryPublicationState::Unpublished,
            supersedes: None,
            dependencies: Vec::new(),
        }
    }

    /// A record with the given identity and axis pair, for fixtures that need to start somewhere
    /// other than the fresh default.
    #[must_use]
    pub fn at(
        id: OpaqueId,
        semantic: MemorySemanticState,
        publication: MemoryPublicationState,
    ) -> Self {
        Self {
            id,
            semantic,
            publication,
            supersedes: None,
            dependencies: Vec::new(),
        }
    }

    /// The same record, built against a named dependency at a named version.
    #[must_use]
    pub fn depending_on(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.dependencies.push((name.into(), version.into()));
        self
    }

    /// This record's identity.
    #[must_use]
    pub const fn id(&self) -> &OpaqueId {
        &self.id
    }

    /// Whether this record's content is still believed.
    #[must_use]
    pub const fn semantic(&self) -> MemorySemanticState {
        self.semantic
    }

    /// Whether this record is visible in the default view.
    #[must_use]
    pub const fn publication(&self) -> MemoryPublicationState {
        self.publication
    }

    /// The predecessor this record supersedes, if any.
    #[must_use]
    pub const fn supersedes(&self) -> Option<&OpaqueId> {
        self.supersedes.as_ref()
    }
}

/// Move a record's PUBLICATION axis, or refuse it. Never touches the semantic axis.
///
/// # Errors
///
/// Returns [`MemoryRefusal`] when the tuple is not allowed. The record is left untouched.
pub fn apply_publication_transition(
    record: &mut MemoryRecord,
    transition: MemoryPublicationTransition,
) -> Result<(), MemoryRefusal> {
    // The tuple is judged BEFORE the record moves. Judged after, the check reads the value it just
    // wrote -- and a refusal then returns an error over a record that has already changed, which is
    // the worst of both: the caller is told nothing happened and the successor is already there.
    //
    // The allowed set is written out because it is POLICY. The matrix that exercises it is derived
    // from `every()`, so a tuple nobody thought about is still visited; only the verdict is listed.
    let allowed = matches!(
        (record.publication, transition),
        (
            MemoryPublicationState::Unpublished,
            MemoryPublicationTransition::Propose
        ) | (
            MemoryPublicationState::Proposed,
            MemoryPublicationTransition::Publish
        ) | (
            MemoryPublicationState::Published,
            MemoryPublicationTransition::Withdraw
        )
    );

    if !allowed {
        return Err(MemoryRefusal {
            code: MemoryRefusalCode::TransitionNotAllowed,
            field: MemoryField::State,
        });
    }

    record.publication = transition.target();
    Ok(())
}

const fn persisted_publication_transition(
    transition: MemoryPublicationTransition,
) -> PersistedMemoryPublicationTransition {
    match transition {
        MemoryPublicationTransition::Propose => PersistedMemoryPublicationTransition::Propose,
        MemoryPublicationTransition::Publish => PersistedMemoryPublicationTransition::Publish,
        MemoryPublicationTransition::Withdraw => PersistedMemoryPublicationTransition::Withdraw,
    }
}

const fn persisted_publication_state(
    state: MemoryPublicationState,
) -> PersistedMemoryPublicationState {
    match state {
        MemoryPublicationState::Unpublished => PersistedMemoryPublicationState::Unpublished,
        MemoryPublicationState::Proposed => PersistedMemoryPublicationState::Proposed,
        MemoryPublicationState::Published => PersistedMemoryPublicationState::Published,
        MemoryPublicationState::Withdrawn => PersistedMemoryPublicationState::Withdrawn,
    }
}

/// Bounded context needed to append one publication-transition event.
pub struct MemoryPublicationTransitionRequest {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    idempotency_key: OpaqueId,
    actor: PersistedActor,
}

impl MemoryPublicationTransitionRequest {
    #[must_use]
    pub fn new(
        scope: RepositoryScope,
        stream_id: OpaqueId,
        expected_next_sequence: u64,
        idempotency_key: OpaqueId,
        actor: PersistedActor,
    ) -> Self {
        Self {
            scope,
            stream_id,
            expected_next_sequence,
            idempotency_key,
            actor,
        }
    }
}

/// Persist a memory record's publication-axis move.
///
/// The caller must call this only AFTER [`apply_publication_transition`] already succeeded on
/// `record` -- this function persists whatever `(record, transition)` it is given; it does not
/// re-derive or re-validate the transition matrix, the same division of labor
/// `record_memory_admission_refusal` keeps between deciding a refusal and persisting one.
///
/// # Errors
///
/// Returns [`EventRepositoryError`] when the underlying append fails.
pub fn record_memory_publication_transition(
    repository: &dyn EventRepository,
    request: MemoryPublicationTransitionRequest,
    record: &MemoryRecord,
    transition: MemoryPublicationTransition,
) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
    let prepared = prepare_memory_publication_transition(MemoryPublicationTransitionAppend::new(
        request.scope,
        request.stream_id,
        request.expected_next_sequence,
        request.idempotency_key,
        request.actor,
        record.id().clone(),
        persisted_publication_transition(transition),
        persisted_publication_state(record.publication()),
    ))?;

    repository.append_atomic(&prepared)
}

/// Supersede `predecessor` with `successor`, for the stated reason.
///
/// This is the relationship ADR-032 requires in place of the old `Superseded` STATE: it moves the
/// PREDECESSOR's semantic axis to the reason's target and records the relationship on the
/// SUCCESSOR. Neither record's publication axis is touched -- superseding is a semantic-axis fact,
/// and a caller that also wants the predecessor out of the default view withdraws it separately
/// through [`apply_publication_transition`].
///
/// # Errors
///
/// Returns [`MemoryRefusal`] when `predecessor` and `successor` are the same record, or when
/// `successor` already supersedes another record. Neither record is changed on refusal.
pub fn supersede(
    predecessor: &mut MemoryRecord,
    successor: &mut MemoryRecord,
    reason: SupersessionReason,
) -> Result<(), MemoryRefusal> {
    if predecessor.id == successor.id {
        return Err(MemoryRefusal {
            code: MemoryRefusalCode::TransitionNotAllowed,
            field: MemoryField::State,
        });
    }
    if successor.supersedes.is_some() {
        return Err(MemoryRefusal {
            code: MemoryRefusalCode::TransitionNotAllowed,
            field: MemoryField::State,
        });
    }

    predecessor.semantic = reason.target();
    successor.supersedes = Some(predecessor.id.clone());
    Ok(())
}

const fn persisted_supersession_reason(reason: SupersessionReason) -> PersistedSupersessionReason {
    match reason {
        SupersessionReason::Contradicted => PersistedSupersessionReason::Contradicted,
        SupersessionReason::Deprecated => PersistedSupersessionReason::Deprecated,
    }
}

const fn persisted_semantic_state(state: MemorySemanticState) -> PersistedMemorySemanticState {
    match state {
        MemorySemanticState::Candidate => PersistedMemorySemanticState::Candidate,
        MemorySemanticState::Validated => PersistedMemorySemanticState::Validated,
        MemorySemanticState::Contradicted => PersistedMemorySemanticState::Contradicted,
        MemorySemanticState::Deprecated => PersistedMemorySemanticState::Deprecated,
        MemorySemanticState::Expired => PersistedMemorySemanticState::Expired,
    }
}

/// Bounded context needed to append one memory-record-supersession event.
pub struct MemoryRecordSupersededRequest {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    idempotency_key: OpaqueId,
    actor: PersistedActor,
}

impl MemoryRecordSupersededRequest {
    #[must_use]
    pub fn new(
        scope: RepositoryScope,
        stream_id: OpaqueId,
        expected_next_sequence: u64,
        idempotency_key: OpaqueId,
        actor: PersistedActor,
    ) -> Self {
        Self {
            scope,
            stream_id,
            expected_next_sequence,
            idempotency_key,
            actor,
        }
    }
}

/// Persist a memory-record supersession.
///
/// The caller must call this only AFTER [`supersede`] already succeeded on `predecessor` and
/// `successor` -- this function persists whatever `(predecessor, successor, reason)` it is given;
/// it does not re-derive or re-validate the two-record invariants `supersede` already checked,
/// the same division of labor [`record_memory_publication_transition`] keeps.
///
/// # Errors
///
/// Returns [`EventRepositoryError`] when the underlying append fails.
pub fn record_memory_record_superseded(
    repository: &dyn EventRepository,
    request: MemoryRecordSupersededRequest,
    predecessor: &MemoryRecord,
    successor: &MemoryRecord,
    reason: SupersessionReason,
) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
    let prepared = prepare_memory_record_superseded(MemoryRecordSupersededAppend::new(
        request.scope,
        request.stream_id,
        request.expected_next_sequence,
        request.idempotency_key,
        request.actor,
        predecessor.id().clone(),
        successor.id().clone(),
        persisted_supersession_reason(reason),
        persisted_semantic_state(predecessor.semantic()),
    ))?;

    repository.append_atomic(&prepared)
}

/// What a published record binds to.
///
/// The binding holds BYTES. A canonical digest is blind to object key order by design, so it
/// answers "is the meaning the same?" and never "are these the same bytes?" -- and the second is
/// the question an audit trail asks. Bound canonically, two Evidence payloads that differ in bytes
/// bind identically, and the evidence under a published record can be swapped with the binding
/// left intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceBinding {
    bytes: Vec<u8>,
}

impl EvidenceBinding {
    /// Whether these bytes are the evidence this binding was built from.
    #[must_use]
    pub fn matches(&self, bytes: &[u8]) -> bool {
        self.bytes == bytes
    }
}

/// Bind a published record to its sealed evidence.
#[must_use]
pub fn bind_evidence(bytes: &[u8]) -> EvidenceBinding {
    EvidenceBinding {
        bytes: bytes.to_vec(),
    }
}

/// Validate a candidate against a roster of validators.
///
/// # Errors
///
/// Returns [`MemoryRefusal`] when no party on the roster is independent of the producer.
pub fn validate_candidate(
    candidate: &MemoryCandidate,
    validators: &[&str],
) -> Result<(), MemoryRefusal> {
    // Validators are IDENTIFIED, not counted. "At least one validator signed off" is true when the
    // only signature is the producer's own, and that is the shape self-validation takes in
    // practice: nobody writes validate(self), they write a roster that happens to contain
    // themselves. An empty roster refuses for the same reason -- it carries no independent party.
    let producer = candidate.produced_by.as_deref();
    let independent = validators
        .iter()
        .any(|validator| Some(*validator) != producer);

    if !independent {
        return Err(MemoryRefusal {
            code: MemoryRefusalCode::SelfValidated,
            field: MemoryField::Validators,
        });
    }

    Ok(())
}

/// A durable step in publishing a memory record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationStep {
    /// The record's evidence is sealed and durable.
    EvidenceSealed,
    /// The record is published and visible by default.
    RecordPublished,
}

/// The order the durable steps of publication are written in.
#[must_use]
pub fn publication_steps() -> Vec<PublicationStep> {
    // Evidence first. Both orders are correct once the sequence completes; they differ only in
    // what a crash leaves behind. Sealed-then-published leaves evidence nobody points at, which is
    // garbage to collect. Published-then-sealed leaves a published record with nothing under it,
    // which is an audit trail asserting something it cannot support -- and the reader cannot tell
    // it apart from a record whose evidence was deleted.
    //
    // The rule is not "write in this order", it is: order the writes so that every intermediate a
    // crash can leave behind reads as the TRUTH.
    vec![
        PublicationStep::EvidenceSealed,
        PublicationStep::RecordPublished,
    ]
}

/// Republish a record over freshly sealed evidence.
///
/// # Errors
///
/// Returns [`MemoryRefusal`] when the evidence could not be resealed for this record.
pub fn republish(
    previous: &EvidenceBinding,
    resealed: Result<Vec<u8>, ()>,
) -> Result<EvidenceBinding, MemoryRefusal> {
    // No fallback to the predecessor's evidence. Falling back keeps publication working, which is
    // exactly why it gets written -- and it publishes a NEW record over OLD evidence, so the audit
    // trail claims the evidence was sealed for this record when it was sealed for another one.
    // A refusal is visible; a silent rebind is not.
    let Ok(bytes) = resealed else {
        return Err(MemoryRefusal {
            code: MemoryRefusalCode::ResealFailed,
            field: MemoryField::Evidence,
        });
    };
    let _ = previous;
    Ok(bind_evidence(&bytes))
}

/// Check that a record's dependencies are the ones it was built against.
///
/// # Errors
///
/// Returns [`MemoryRefusal`] when a dependency has moved.
pub fn check_dependency_freshness(
    record: &MemoryRecord,
    available: &[(&str, &str)],
) -> Result<(), MemoryRefusal> {
    // The question is not whether the dependency EXISTS -- presence is the easy question, and the
    // wrong version answers it yes. The question is whether it is the one this record was built
    // against, so the version is part of the comparison.
    for (name, version) in &record.dependencies {
        if !available
            .iter()
            .any(|(have, have_version)| have == name && have_version == version)
        {
            return Err(MemoryRefusal {
                code: MemoryRefusalCode::DependencyStale,
                field: MemoryField::Dependency,
            });
        }
    }
    Ok(())
}
