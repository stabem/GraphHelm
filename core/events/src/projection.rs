use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::{
    EventEnvelope, EventHash, EventKind, EvidenceId, ExecutionFormDeclared, ExecutionId,
    ExecutionMode, NodeOutcome, NodeState, OpaqueId, PersistedGraphVersion, PersistedTimestamp,
    PolicyWaiver, ProjectId, RepositoryScope, SimulationStatus, WireHash, WorkspaceId,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

use crate::{
    AsyncEventRepository, EventRepositoryError, ReadStart, ReadStreamRequest, RepositoryFuture,
    StreamHead,
    canonical::{event_hash, serialized_len_bounded},
    limits::{MAX_EVENT_BYTES, MAX_JOURNAL_BYTES, MAX_READ_ALL, MAX_SAFE_INTEGER},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopedEvidenceId {
    scope: RepositoryScope,
    evidence_id: EvidenceId,
}

impl Serialize for ScopedEvidenceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = [
            self.scope.workspace_id().as_str(),
            self.scope.project_id().as_str(),
            self.scope.execution_id().map_or("", |id| id.as_str()),
            self.evidence_id.as_str(),
        ];
        let mut wire = String::from("v1:");
        for field in fields {
            wire.push_str(&field.len().to_string());
            wire.push(':');
            wire.push_str(field);
        }
        serializer.serialize_str(&wire)
    }
}

impl<'de> Deserialize<'de> for ScopedEvidenceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = String::deserialize(deserializer)?;
        let Some(mut remainder) = wire.strip_prefix("v1:") else {
            return Err(D::Error::custom("invalid scoped Evidence key"));
        };
        let mut fields = Vec::with_capacity(4);
        for _ in 0..4 {
            let separator = remainder
                .find(':')
                .ok_or_else(|| D::Error::custom("invalid scoped Evidence key"))?;
            let length_token = &remainder[..separator];
            let length = length_token
                .parse::<usize>()
                .map_err(|_| D::Error::custom("invalid scoped Evidence key"))?;
            if length_token != length.to_string() {
                return Err(D::Error::custom("invalid scoped Evidence key"));
            }
            remainder = &remainder[separator + 1..];
            let field = remainder
                .get(..length)
                .ok_or_else(|| D::Error::custom("invalid scoped Evidence key"))?;
            fields.push(field);
            remainder = &remainder[length..];
        }
        if !remainder.is_empty() {
            return Err(D::Error::custom("invalid scoped Evidence key"));
        }
        let scope = RepositoryScope::new(
            WorkspaceId::parse(fields[0]).map_err(D::Error::custom)?,
            ProjectId::parse(fields[1]).map_err(D::Error::custom)?,
            if fields[2].is_empty() {
                None
            } else {
                Some(ExecutionId::parse(fields[2]).map_err(D::Error::custom)?)
            },
        );
        Ok(Self::new(
            scope,
            EvidenceId::parse(fields[3]).map_err(D::Error::custom)?,
        ))
    }
}

impl PartialOrd for ScopedEvidenceId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScopedEvidenceId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            self.scope.workspace_id(),
            self.scope.project_id(),
            self.scope.execution_id(),
            &self.evidence_id,
        )
            .cmp(&(
                other.scope.workspace_id(),
                other.scope.project_id(),
                other.scope.execution_id(),
                &other.evidence_id,
            ))
    }
}

impl ScopedEvidenceId {
    #[must_use]
    pub fn new(scope: RepositoryScope, evidence_id: EvidenceId) -> Self {
        Self { scope, evidence_id }
    }

    #[must_use]
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }

    #[must_use]
    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }
}

const GENESIS_HASH: &str =
    "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAvailability {
    Available,
    ErasurePending,
    Erased,
    Deleted,
}

/// M11 #160: one stage a node passed through in the customs pipeline.
///
/// The enum is OPEN TO GROWTH by design: lane 1 mints only the stages lane 1 produces, and the
/// DLQ lane adds its own. Declaring variants here that nothing emits would put a legal-but-never-
/// produced value in front of every reader — a confusion this project has already paid for, so
/// consumers must treat this as a growing enum rather than a closed world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomsStage {
    /// The node parked and a wait was minted (this entry's `at_sequence` IS that wait's identity).
    Parked,
    /// Testimony recorded against the open wait. Releases nothing.
    Claimed,
    /// Countersigned — the only stage that releases a dependent.
    Cleared,
    /// Clearance withheld with a reason; the claim is spent, the node stays parked.
    Rejected,
    /// The command layer would not accept the claim; state unchanged, trail kept.
    Refused,
    /// A stage deadline lapsed and a sweep said so.
    Overdue,
}

/// M11 #160: one line of a node's customs history, in log order.
///
/// This is the fold-derived timeline #163 renders. It is append-only and derived: no reader may
/// write it, and a projection rehydrated without replay does not carry it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomsScan {
    /// Envelope sequence of the event that produced this entry.
    pub at_sequence: u64,
    pub stage: CustomsStage,
    /// The claim this entry is about, when it is about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_seq: Option<u64>,
    /// Registry reason — ONLY for stages where this timeline is the fact's sole owner.
    ///
    /// A REFUSED claim belongs here because the CONTAINER SHAPE matches its cardinality, not
    /// merely because it lacks a claim identity (J's sharpening of my first argument, and the
    /// durable version of it). A refusal has no CLAIM identity — no claim was accepted — only a
    /// WAIT identity: the sequence the refused claim NAMED. And many refusals can name one wait:
    /// a bad attempt, a retry, another bad attempt. A map keyed by wait would be last-wins and
    /// would erase the earlier refusals — the overwrite defect this project spent M09 hunting,
    /// where a last-per-X map is asked a per-N question. This field is a `Vec`, ordered, and
    /// many-refusals-per-wait is exactly the shape a Vec holds correctly.
    ///
    /// A REJECTED or CLEARED claim has an outcome record keyed by `claim_seq` (#161) — one
    /// lookup, no choice — and that record owns its reason. This entry carries the pointer
    /// (`claim_seq`) and not a copy.
    ///
    /// The rule behind the split, argued by J against my first shape: two structures recording
    /// one fact is duplicated STATE, which is worse than duplicated code because a later replay
    /// can make the copies disagree. The test that settles who owns a fact is which structure
    /// answers the question WITHOUT re-deriving: "what happened to the claim at sequence N" is
    /// answered by a claim-keyed map in one lookup, and by this node-keyed timeline only after a
    /// node lookup, a filter, and a choice of which entry wins — and that choice is exactly what
    /// diverges. So the outcome lives there, and the timeline points at it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// When THIS stage's patience ran out, for the stages that start a clock (parked, claimed).
    ///
    /// Computed ONCE, here, at fold time — never recomputed by a renderer. The instant is
    /// `occurred_at` of the event that entered the stage plus the duration the node declared, so
    /// every consumer reads the same number instead of three surfaces each doing the arithmetic
    /// their own way. That is the same one-derivation-many-call-sites shape `ready_set` has, and
    /// it is here for the same reason: a deadline recomputed per surface is a deadline that will
    /// eventually disagree with itself.
    ///
    /// `None` means the node declared no budget for this stage — absent stays absent, and no
    /// implicit patience is invented. (It is ALSO None on every entry today, because the spec
    /// block the durations come from is still an open decision: a live `completion:` field
    /// already carries a different meaning, so the budget's home is escalated rather than
    /// guessed. Consumers should render "no deadline declared", not "not yet implemented".)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<PersistedTimestamp>,
}

/// M11 #160: one open wait, with the instant its patience runs out.
///
/// `deadline` is `None` when the node declared no `wait_within_seconds` budget — absent stays
/// absent, and no implicit patience is ever invented for a wait that did not ask for one. A wait
/// with no deadline is visible to the sweep and never overdue, which is a DECLARED gap rather
/// than a silent one: the load-time warning names such nodes at authoring time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWait {
    /// Envelope sequence of the event that parked this wait — the wait's identity AND its
    /// stage-entry point, which are deliberately the same number (§2b': one arithmetic).
    pub at_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<PersistedTimestamp>,
}

/// M11 #160: a claim in quarantine — testimony recorded, clearance owed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClaim {
    pub node: String,
    /// The wait this claim answered, carried so a clearance can find the node's open wait even
    /// after later events.
    pub completes_wait_seq: u64,
    /// Stage-entry sequence for the CLAIMED stage: the claim's own sequence on first entry, and
    /// the redrive's sequence after a re-entry (the fold rebases; the testimony stands, only the
    /// clock restarts).
    pub stage_entered_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<PersistedTimestamp>,
}

/// Pure replay result rebuilt only from the safe journal projection.
///
/// `Option<T>` tolerates an absent key on its own; serde only special-cases `Option`. A collection
/// field needs an explicit `#[serde(default)]` to load empty when it is absent — but only the
/// fields added after a generation format shipped should carry one. Defaulting a field that every
/// stored generation already has would let a truncated state load silently as empty instead of
/// failing, which is the opposite of what a corrupted generation should do.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProjection {
    pub stream_id: Option<String>,
    pub current_graph: Option<PersistedGraphVersion>,
    /// The shape the operator DECLARED, which is a different and weaker claim than
    /// `current_graph`'s "this was sealed and published". A rule that needs the node set or a
    /// node's deadline can be answered from a declaration; a rule that needs sealed evidence
    /// cannot, and must keep asking `current_graph`. Keeping them apart is what stops a
    /// declaration from being laundered into a publication.
    ///
    /// `None` means no declaration was recorded — including every history written before this
    /// event existed. Undeclared, never calm.
    ///
    /// `skip_serializing_if` is load-bearing, not tidiness: `projection.rs` re-serializes a
    /// projection to digest it, so a field that emitted `null` for every history written
    /// before this event existed would change all their digests and break the frozen
    /// demonstrations. Absent stays absent on the wire, exactly as it does in the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_form: Option<ExecutionFormDeclared>,
    /// Amendments in log order, each with the sequence that carried it. Never collapsed into
    /// a single map: see the fold arm.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub form_amendments: Vec<(u64, graphhelm_protocols::ExecutionFormAmended)>,
    pub proposed_drafts: Vec<String>,
    pub rejected_drafts: Vec<String>,
    pub applied_drafts: Vec<String>,
    pub waivers: Vec<PolicyWaiver>,
    pub node_states: BTreeMap<String, NodeState>,
    /// M11 #160: the OPEN wait per node, identified by the envelope sequence of the event that
    /// parked it.
    ///
    /// Sequence-as-identity, for the reason M09 chose it for wake leases: a node re-parks
    /// (`WaitingInput` on `NeedsInput` is the state machine's own arm), so a name identifies the
    /// NODE but never the WAIT. A claim naming a superseded wait is refused instead of silently
    /// answering whichever wait is open now — the trap this map exists to make detectable.
    ///
    /// DERIVED, never stored: valid on a replayed projection only, exactly like
    /// `armed_at_sequence`. A projection rehydrated from a digest without replay does not carry
    /// it, and no reader may treat its absence as "no wait".
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub open_waits: BTreeMap<String, OpenWait>,
    /// M11 #160: claims awaiting clearance, keyed by the CLAIM's own envelope sequence.
    ///
    /// A claim in here is quarantined testimony: recorded, not believed. It releases nothing —
    /// the node stays `WaitingInput` until a clearance arrives, which is the whole thesis of the
    /// customs pipeline and the thing `the_downstream_of_a_claimed_wait_is_not_ready` guards.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub open_claims: BTreeMap<u64, OpenClaim>,
    /// M11 #160: episodes that have already raised an overdue exception, by stage-entry
    /// sequence. One exception per episode, ever; a re-entry is a new episode.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub exception_marked: BTreeSet<u64>,
    /// M11 #160: the per-node customs timeline (#163 renders it; the fold owns it).
    ///
    /// Bounded by the same node guard as every other per-node map, and by a per-node entry cap:
    /// a history is evidence, but an unbounded one is a memory exhaustion vector wearing
    /// evidence's clothes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub customs_scans: BTreeMap<String, Vec<CustomsScan>>,
    /// M11 #161: who may countersign, AS OF THE FOLD'S CURRENT POSITION.
    ///
    /// Read this field only as "membership at the cursor". While the fold walks it is exactly
    /// the registry at the event being folded, which is why validating in place is correct and
    /// free. AFTER the fold it means membership AT HEAD, and answering "could X sign at
    /// sequence N?" from it for any earlier N is the last-state-answering-a-per-sequence-question
    /// defect (M09 #88's named cause, one layer up). The journal answers that question; a replay
    /// to N is how you ask it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub clearance_registry: BTreeMap<String, WireHash>,
    pub simulation_status: Option<SimulationStatus>,
    pub evidence_availability: BTreeMap<ScopedEvidenceId, EvidenceAvailability>,
    pub legal_holds: BTreeSet<ScopedEvidenceId>,
    /// The execution this projection describes, once one has started.
    pub execution_id: Option<String>,
    /// Autonomy in force, per D-022. `None` until an execution starts.
    pub mode: Option<ExecutionMode>,
    /// Attempts observed per node. Derived by folding outcomes, never read from a payload, so
    /// history stays the single source of truth.
    #[serde(default)]
    pub node_attempts: BTreeMap<String, u32>,
    /// The most recent outcome per node, used to detect consecutive identical outcomes.
    #[serde(default)]
    pub last_outcome: BTreeMap<String, NodeOutcome>,
    /// Consecutive semantically identical outcomes per node, per decision 5.7.
    ///
    /// This is the run length *including* the last outcome recorded, which is not the same quantity
    /// as `TransitionRequest::identical_outcomes` — that one describes the outcome about to be
    /// reported. They agree only when the next outcome equals `last_outcome`. Read it through
    /// [`ExecutionProjection::identical_outcomes_for`], or a node whose previous run was of a
    /// *different* outcome will look stalled on its very first failure.
    #[serde(default)]
    pub identical_outcomes: BTreeMap<String, u32>,
    /// Signals recorded for this execution. Counted here, judged by the governor.
    #[serde(default)]
    pub signals_recorded: u32,
    /// Governor mutations accepted, per decision 5.1. Counted here, judged by the governor.
    #[serde(default)]
    pub accepted_mutations: u32,
    /// Certified gates (M06): gate id -> the pathogen-suite digest the certification is
    /// valid against. Task 4's precondition reads this and refuses a gate whose receipt
    /// does not match the CURRENT suite digest — growing the suite voids old immunity.
    #[serde(default)]
    pub gate_certifications: BTreeMap<String, String>,
    /// Live wake leases by session (05g): AT MOST ONE per session — arming again replaces,
    /// never stacks (the anti-fork-bomb rule as a fold invariant). Consumption removes;
    /// consumption without a live lease is a replay integrity refusal.
    #[serde(default)]
    pub wake_leases: BTreeMap<String, WakeLeaseState>,
    /// The LAST consumption per session (M07 F4): why the lease burned and at which
    /// sequence. A burned lease used to vanish without a trace, so `wake_status` could
    /// only say "not live" — indistinguishable from "never armed". The receipt is what
    /// lets the alarm answer its own question: "rang at #N" vs "burned as stale at #N".
    ///
    /// Additive with `serde(default)`: unlike an event payload, a projection is DERIVED,
    /// never hashed per event, so a new field costs an artifact digest re-record and never
    /// a broken hash chain (the M07 Task 2 distinction, stated where both live).
    #[serde(default)]
    pub wake_last_consumed: BTreeMap<String, WakeConsumptionReceipt>,
    /// Consumptions that named an arming other than the one that was live — a sweep burning a
    /// lease it did not mean to burn.
    ///
    /// Keyed by session because the session is who is stranded: the burned lease is gone, so it
    /// has no live lease and no later append can ring it. Last-wins, matching its neighbours.
    ///
    /// AN ENTRY HERE NEVER IMPLIES REPLAY REFUSED. The fold records this and refuses nothing —
    /// burning a live lease is legal, the log stays readable, and that is precisely what makes
    /// this failure silent. Refusal is the fold's answer to a log it CANNOT INTERPRET; this one
    /// is fully interpretable and merely records something bad. A reader finding an entry must
    /// not infer the stream was ever unreadable.
    ///
    /// It lives in the projection rather than only in the events because the attention verdict
    /// is computed from the projection through one predicate. A discrepancy visible only in raw
    /// events is one that surface structurally cannot see — which would reproduce this defect
    /// inside its own detector.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub wake_mis_burns: BTreeMap<String, WakeMisBurn>,
}

/// A consumption that burned an arming other than the one it captured.
///
/// Both armings are kept rather than a bare flag: "something was wrong here" is not actionable,
/// and the PAIR is the diagnosis. Recovering it from raw events is exactly the derivation the
/// single predicate must not perform.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeMisBurn {
    /// Sequence of the consumption that did it.
    pub at_sequence: u64,
    /// The arming the sweep captured.
    pub captured_arming: u64,
    /// The arming that was actually live, and got burned.
    pub live_arming: u64,
}

/// Why and where a lease burned (M07 F4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeConsumptionReceipt {
    pub reason: graphhelm_protocols::WakeConsumeReason,
    /// The sequence of the consumption event itself — the "#N" an operator quotes.
    pub sequence: u64,
}

/// One live lease as the projection holds it (05g): the armed cursor and the opaque
/// rendezvous identity — never a path; the ring side derives the platform rendezvous
/// under its own fixed prefix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeLeaseState {
    pub cursor: u64,
    pub rendezvous_id: String,
    /// Which arming this lease IS: the sequence of the `wake_lease` event that produced it.
    ///
    /// DERIVED from the envelope, never carried in the payload, and that is the whole reason it
    /// works for history written before the field existed — every event has always had a
    /// sequence, so a lease armed a year ago gets one on the next replay like any other.
    ///
    /// It exists because a session's identity is not enough to tell two of its own armings
    /// apart, and neither is the rendezvous: our agents re-arm on FIXED rendezvous ids, so the
    /// old arming and its replacement match on both. The cursor cannot separate them either —
    /// re-arming at the head the surface reported is a documented fixed point, so it can repeat.
    /// The sequence is strictly monotone per stream and is the only thing that cannot.
    ///
    /// THIS DEPENDS ON RE-ARM BEING AN OVERWRITE. The fold replaces the lease on a new arming,
    /// so this moves to the new event's sequence while a capture held from before still carries
    /// the old one — they differ by construction rather than by luck. If re-arm ever becomes a
    /// merge in place, a merged lease keeps the old sequence, stale captures match again, and
    /// the discriminator fails SILENTLY. This sentence is the canary for that change.
    ///
    /// Two guards, named so that breaking either one leads to both: the overwrite premise is
    /// pinned by `arming_twice_replaces_the_lease_and_consumption_burns_it`, and this field's
    /// derivation by `a_leases_armed_at_sequence_is_its_envelopes_and_a_re_arm_moves_it` — both
    /// in `core/events/tests/execution_projection.rs`.
    ///
    /// AND IT IS ONLY VALID ON A REPLAYED PROJECTION. Being derived, it is reconstructed from
    /// the log every time — but a projection REHYDRATED from any stored or serialized form
    /// carries `0` for every lease written before this field existed. Feed that to the sweep's
    /// filter and no capture ever matches, so every consumption is dropped and every sleeper
    /// stops being rung, in silence. Callers must REPLAY, never rehydrate, and the failure mode
    /// for getting that wrong is quiet enough that nothing will tell them.
    #[serde(default)]
    pub armed_at_sequence: u64,
    /// M09 decision B: the instant the sleeper's quiet stops being acceptable, exactly as it
    /// was armed. STORED, never compared here — whether it has passed is a question the
    /// surface asks with an instant it injects, and a fold that answered it would make replay
    /// a function of the wall clock.
    ///
    /// A TIMESTAMP, not the wire string it renders to. The first draft held a `String` on the
    /// reasoning that the projection speaks wire vocabulary, and the reviewer measured what
    /// that costs: the canonical rendering emits 0, 3, 6 or 9 fractional digits depending on
    /// the value, so `...:00.500Z` sorts BEFORE `...:00Z` — `.` is 0x2E and `Z` is 0x5A.
    /// Comparing the stored strings would give the REVERSE of chronological order for the
    /// commonest pair there is: one instant with a fraction and one without. The type carries
    /// the correct `Ord`, so that mistake cannot be made rather than merely being one nobody
    /// ought to make.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matures_at: Option<PersistedTimestamp>,
}

impl ExecutionProjection {
    /// The sequence the newest amendment arrived at, or 0 when none has. Callers name this
    /// as the frontier they computed against, so a stale amendment can be refused.
    #[must_use]
    pub fn amendment_head(&self) -> u64 {
        self.form_amendments
            .last()
            .map_or(0, |(sequence, _)| *sequence)
    }

    /// This projection as it stood AT `sequence`: amendments after it are dropped.
    ///
    /// The whole point of step 2 lives here. An amendment declares a bound going forward, so
    /// a reader positioned before it must still see the unknown that was true then. Without
    /// this, "forward" would be an adjective rather than a behaviour.
    #[must_use]
    pub fn as_of(&self, sequence: u64) -> Self {
        let mut earlier = self.clone();
        earlier.form_amendments.retain(|(at, _)| *at <= sequence);
        earlier
    }

    /// Appends an amendment in memory, for callers that build a projection directly.
    pub fn apply_amendment(&mut self, amendment: graphhelm_protocols::ExecutionFormAmended) {
        let next = self.amendment_head().saturating_add(1);
        self.form_amendments.push((next, amendment));
    }
    /// Consecutive identical outcomes already observed for `node`, for the `outcome` about to be
    /// reported.
    ///
    /// Returns 0 when the last recorded outcome differs, which is what makes this safe to hand to
    /// `TransitionRequest::identical_outcomes`. Reading the raw map instead would block a node on
    /// its first failure whenever some *other* outcome had already run to the bound.
    #[must_use]
    pub fn identical_outcomes_for(&self, node: &str, outcome: NodeOutcome) -> u32 {
        if self.last_outcome.get(node) != Some(&outcome) {
            return 0;
        }
        self.identical_outcomes.get(node).copied().unwrap_or(0)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReplayError {
    #[error("event stream exceeds a deterministic replay limit")]
    LimitExceeded,
    #[error("event stream failed integrity verification")]
    Corrupt,
}

/// Durable, exact source position for one disposable projection generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionWatermark {
    scope: RepositoryScope,
    stream_id: String,
    projection_name: String,
    projection_version: u32,
    generation: u64,
    last_sequence: u64,
    last_event_hash: Option<EventHash>,
}

impl ProjectionWatermark {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scope: RepositoryScope,
        stream_id: String,
        projection_name: String,
        projection_version: u32,
        generation: u64,
        last_sequence: u64,
        last_event_hash: Option<EventHash>,
    ) -> Result<Self, ReplayError> {
        OpaqueId::parse(&stream_id).map_err(|_| ReplayError::Corrupt)?;
        OpaqueId::parse(&projection_name).map_err(|_| ReplayError::Corrupt)?;
        if projection_version > i32::MAX as u32
            || generation > MAX_SAFE_INTEGER
            || last_sequence > MAX_SAFE_INTEGER
        {
            return Err(ReplayError::LimitExceeded);
        }
        if projection_version == 0
            || generation == 0
            || (last_sequence == 0) != last_event_hash.is_none()
        {
            return Err(ReplayError::Corrupt);
        }
        Ok(Self {
            scope,
            stream_id,
            projection_name,
            projection_version,
            generation,
            last_sequence,
            last_event_hash,
        })
    }

    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
    pub fn projection_name(&self) -> &str {
        &self.projection_name
    }
    pub const fn projection_version(&self) -> u32 {
        self.projection_version
    }
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
    pub const fn last_event_hash(&self) -> Option<&EventHash> {
        self.last_event_hash.as_ref()
    }
}

/// In-progress or complete disposable projection generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionGeneration {
    watermark: ProjectionWatermark,
    projection: ExecutionProjection,
    #[serde(default)]
    seen_idempotency_keys: BTreeSet<String>,
    #[serde(default)]
    active_legal_holds: BTreeSet<(ScopedEvidenceId, String)>,
    #[serde(default)]
    aggregate_bytes: u64,
    #[serde(default)]
    aggregate_references: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionRebuildRequest {
    scope: RepositoryScope,
    stream_id: String,
    projection_name: String,
    projection_version: u32,
    generation: u64,
    page_size: u32,
}

/// Durable generation storage. Implementations keep the old active generation until swap succeeds.
pub trait ProjectionRepository: Send + Sync {
    fn load_generation<'a>(
        &'a self,
        request: &'a ProjectionRebuildRequest,
    ) -> RepositoryFuture<'a, Result<Option<ProjectionGeneration>, EventRepositoryError>>;
    fn save_generation<'a>(
        &'a self,
        generation: ProjectionGeneration,
    ) -> RepositoryFuture<'a, Result<(), EventRepositoryError>>;
    fn load_active<'a>(
        &'a self,
        scope: RepositoryScope,
        stream_id: String,
        projection_name: String,
        projection_version: u32,
    ) -> RepositoryFuture<'a, Result<Option<ProjectionGeneration>, EventRepositoryError>>;
    fn swap_active<'a>(
        &'a self,
        generation: ProjectionGeneration,
        expected_source_head: Option<StreamHead>,
    ) -> RepositoryFuture<'a, Result<(), EventRepositoryError>>;
}

pub struct ProjectionRebuilder {
    events: std::sync::Arc<dyn AsyncEventRepository>,
    projections: std::sync::Arc<dyn ProjectionRepository>,
}

// Persisting every caller-sized read page lets an adversarial page size turn adapters that
// authenticate a checkpoint's complete prefix into quadratic work. Keep durable progress bounded
// by a domain-owned interval instead.
const PROJECTION_CHECKPOINT_INTERVAL: u64 = 10_000;

impl ProjectionRebuilder {
    #[must_use]
    pub fn new(
        events: std::sync::Arc<dyn AsyncEventRepository>,
        projections: std::sync::Arc<dyn ProjectionRepository>,
    ) -> Self {
        Self {
            events,
            projections,
        }
    }

    pub fn rebuild<'a>(
        &'a self,
        request: ProjectionRebuildRequest,
    ) -> RepositoryFuture<'a, Result<ProjectionGeneration, EventRepositoryError>> {
        Box::pin(async move {
            let (mut generation, created) = match self.projections.load_generation(&request).await?
            {
                Some(existing)
                    if existing.watermark().scope() == request.scope()
                        && existing.watermark().stream_id() == request.stream_id()
                        && existing.watermark().projection_name() == request.projection_name()
                        && existing.watermark().projection_version()
                            == request.projection_version()
                        && existing.watermark().generation() == request.generation() =>
                {
                    (existing, false)
                }
                // A stored generation exists but does not describe the requested one. That is a
                // non-resumable watermark, not a broken hash chain, and an operator needs to be
                // able to tell those apart.
                Some(_) => return Err(EventRepositoryError::WatermarkMismatch),
                None => (
                    ProjectionGeneration::new(
                        request.scope().clone(),
                        request.stream_id().to_owned(),
                        request.projection_name().to_owned(),
                        request.projection_version(),
                        request.generation(),
                    )
                    .map_err(map_replay_error)?,
                    true,
                ),
            };
            if created {
                self.projections.save_generation(generation.clone()).await?;
            }
            let mut last_saved_sequence = generation.watermark().last_sequence();
            loop {
                let observed = self
                    .events
                    .stream_head(request.scope().clone(), request.stream_id().to_owned())
                    .await?;
                let target_sequence = observed
                    .as_ref()
                    .map_or(0, |head| head.next_sequence.saturating_sub(1));
                if generation.watermark().last_sequence() > target_sequence {
                    return Err(EventRepositoryError::Integrity);
                }
                while generation.watermark().last_sequence() < target_sequence {
                    let start = match generation.watermark().last_event_hash() {
                        None => ReadStart::Beginning,
                        Some(hash) => ReadStart::After {
                            sequence: generation.watermark().last_sequence(),
                            event_hash: hash.clone(),
                        },
                    };
                    let page = self
                        .events
                        .read_stream(ReadStreamRequest::new(
                            request.scope().clone(),
                            request.stream_id().to_owned(),
                            start,
                            request.page_size(),
                        )?)
                        .await?;
                    if page.events.is_empty() {
                        return Err(EventRepositoryError::Integrity);
                    }
                    generation
                        .apply_page(&page.events)
                        .map_err(map_replay_error)?;
                    if generation
                        .watermark()
                        .last_sequence()
                        .saturating_sub(last_saved_sequence)
                        >= PROJECTION_CHECKPOINT_INTERVAL
                    {
                        self.projections.save_generation(generation.clone()).await?;
                        last_saved_sequence = generation.watermark().last_sequence();
                    }
                }
                let current = self
                    .events
                    .stream_head(request.scope().clone(), request.stream_id().to_owned())
                    .await?;
                if current != observed {
                    continue;
                }
                if generation.watermark().last_sequence() != last_saved_sequence {
                    self.projections.save_generation(generation.clone()).await?;
                }
                self.projections
                    .swap_active(generation.clone(), observed)
                    .await?;
                return Ok(generation);
            }
        })
    }
}

fn map_replay_error(error: ReplayError) -> EventRepositoryError {
    match error {
        ReplayError::LimitExceeded => EventRepositoryError::LimitExceeded,
        ReplayError::Corrupt => EventRepositoryError::Integrity,
    }
}

impl ProjectionRebuildRequest {
    pub fn new(
        scope: RepositoryScope,
        stream_id: String,
        projection_name: String,
        projection_version: u32,
        generation: u64,
        page_size: u32,
    ) -> Result<Self, ReplayError> {
        ProjectionWatermark::new(
            scope.clone(),
            stream_id.clone(),
            projection_name.clone(),
            projection_version,
            generation,
            0,
            None,
        )?;
        if page_size == 0
            || usize::try_from(page_size).map_or(true, |size| size > crate::limits::MAX_READ_PAGE)
        {
            return Err(ReplayError::LimitExceeded);
        }
        Ok(Self {
            scope,
            stream_id,
            projection_name,
            projection_version,
            generation,
            page_size,
        })
    }
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
    pub fn projection_name(&self) -> &str {
        &self.projection_name
    }
    pub const fn projection_version(&self) -> u32 {
        self.projection_version
    }
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    pub const fn page_size(&self) -> u32 {
        self.page_size
    }
}

impl ProjectionGeneration {
    pub fn new(
        scope: RepositoryScope,
        stream_id: String,
        projection_name: String,
        projection_version: u32,
        generation: u64,
    ) -> Result<Self, ReplayError> {
        Ok(Self {
            watermark: ProjectionWatermark::new(
                scope,
                stream_id,
                projection_name,
                projection_version,
                generation,
                0,
                None,
            )?,
            projection: ExecutionProjection::default(),
            seen_idempotency_keys: BTreeSet::new(),
            active_legal_holds: BTreeSet::new(),
            aggregate_bytes: 0,
            aggregate_references: 0,
        })
    }

    pub const fn watermark(&self) -> &ProjectionWatermark {
        &self.watermark
    }
    pub const fn projection(&self) -> &ExecutionProjection {
        &self.projection
    }

    /// Applies one bounded contiguous source page without opening Evidence.
    pub fn apply_page(&mut self, events: &[EventEnvelope]) -> Result<(), ReplayError> {
        if events.len() > crate::limits::MAX_READ_PAGE {
            return Err(ReplayError::LimitExceeded);
        }
        let schemas =
            graphhelm_schema::repository_schema_set().map_err(|_| ReplayError::Corrupt)?;
        for event in events {
            let event_bytes = serialized_len_bounded(event, MAX_EVENT_BYTES)
                .map_err(|_| ReplayError::LimitExceeded)?;
            self.aggregate_bytes = self
                .aggregate_bytes
                .checked_add(u64::try_from(event_bytes).map_err(|_| ReplayError::LimitExceeded)?)
                .ok_or(ReplayError::LimitExceeded)?;
            self.aggregate_references = self
                .aggregate_references
                .checked_add(event.evidence_refs.len())
                .and_then(|value| value.checked_add(event.artifact_refs.len()))
                .ok_or(ReplayError::LimitExceeded)?;
            if self.aggregate_bytes > MAX_JOURNAL_BYTES || self.aggregate_references > MAX_READ_ALL
            {
                return Err(ReplayError::LimitExceeded);
            }
            let value = serde_json::to_value(event).map_err(|_| ReplayError::Corrupt)?;
            let expected_previous = self
                .watermark
                .last_event_hash
                .as_ref()
                .map_or(GENESIS_HASH, EventHash::as_str);
            if !schemas.validate_event(&value).is_empty()
                || crate::integrity::validate_envelope(event).is_err()
                || event.scope != self.watermark.scope
                || event.stream_id.as_str() != self.watermark.stream_id
                || event.kind.is_project_level() == event.scope.execution_id().is_some()
                || event.sequence != self.watermark.last_sequence + 1
                || event.previous_hash.as_str() != expected_previous
                || event_hash(event, expected_previous).map_err(|_| ReplayError::Corrupt)?
                    != event.event_hash.as_str()
                || !self
                    .seen_idempotency_keys
                    .insert(event.idempotency_key.to_string())
            {
                return Err(ReplayError::Corrupt);
            }
            apply_projection_event(&mut self.projection, &mut self.active_legal_holds, event)?;
            self.watermark.last_sequence = event.sequence;
            self.watermark.last_event_hash = Some(event.event_hash.clone());
        }
        Ok(())
    }
}

/// Resource guard on the projection's per-node maps.
///
/// This is **not** one of decision 5.7's bounds. Those are domain limits a real execution can
/// reach, and reaching one blocks for an owner decision. This one exists so a corrupt or hostile
/// history cannot grow the maps without limit before the projection's size check can reject it, and
/// a legitimate execution must never reach it — which is why it sits an order of magnitude above
/// `MAX_READY_SET`. `graphhelm_execution` pins that relationship in a test.
pub const MAX_PROJECTION_NODES: usize = 10_000;

/// Appends one line to a node's customs timeline, bounded (#160).
///
/// The per-node cap is `MAX_PROJECTION_NODES` reused as an entry bound: it is the same order of
/// magnitude the projection already accepts per node-keyed structure, and picking a second,
/// smaller number here would invent a limit nobody derived.
fn record_scan(
    projection: &mut ExecutionProjection,
    node: &str,
    scan: CustomsScan,
) -> Result<(), ReplayError> {
    if projection.customs_scans.len() >= MAX_PROJECTION_NODES
        && !projection.customs_scans.contains_key(node)
    {
        return Err(ReplayError::LimitExceeded);
    }
    let history = projection
        .customs_scans
        .entry(node.to_owned())
        .or_default();
    if history.len() >= MAX_PROJECTION_NODES {
        return Err(ReplayError::LimitExceeded);
    }
    history.push(scan);
    Ok(())
}

fn apply_projection_event(
    projection: &mut ExecutionProjection,
    active_legal_holds: &mut BTreeSet<(ScopedEvidenceId, String)>,
    event: &EventEnvelope,
) -> Result<(), ReplayError> {
    projection
        .stream_id
        .get_or_insert_with(|| event.stream_id.to_string());
    match &event.kind {
        EventKind::ExecutionFormDeclared(payload) => {
            // A second declaration on one stream would give the shape two owners, which is the
            // trap this event was designed around: a deadline the execution can legally change
            // must not be frozen twice under one name.
            if projection.declared_form.is_some() {
                return Err(ReplayError::Corrupt);
            }
            projection.declared_form = Some(payload.clone());
        }
        EventKind::ExecutionFormAmended(payload) => {
            // APPENDED, never replacing. Amendments accumulate in the order the log gives
            // them, and the effective bound for a node is the LAST one at or before the
            // sequence being replayed -- so a replay positioned earlier answers with what was
            // known THEN. Storing them as a list rather than folding them into one map is
            // what makes that possible: a folded map cannot be un-folded to an earlier
            // moment, and the past would silently inherit a decision made after it.
            projection
                .form_amendments
                .push((event.sequence, payload.clone()));
        }
        EventKind::GraphVersionPublished(payload) => {
            graphhelm_graph::validate_persisted_projection(&payload.version)
                .map_err(|_| ReplayError::Corrupt)?;
            if event.actor != *payload.version.created_by()
                || event.scope.execution_id() != Some(payload.version.topology().execution_id())
                || graphhelm_graph::validate_publication_evidence_ids(
                    &event.scope,
                    &payload.version,
                )
                .is_err()
                || graphhelm_graph::validate_evidence_bijection(
                    payload.version.content_slots(),
                    &event.evidence_refs,
                )
                .is_err()
            {
                return Err(ReplayError::Corrupt);
            }
            match &projection.current_graph {
                Some(active)
                    if active.number().checked_add(1) != Some(payload.version.number())
                        || payload.version.predecessor().is_none_or(|prior| {
                            prior.number() != active.number()
                                || prior.semantic_hash() != active.semantic_hash()
                        }) =>
                {
                    return Err(ReplayError::Corrupt);
                }
                None if payload.version.number() != 1
                    || payload.version.predecessor().is_some() =>
                {
                    return Err(ReplayError::Corrupt);
                }
                _ => {}
            }
            projection.current_graph = Some(payload.version.clone());
            for slot in payload.version.content_slots() {
                projection
                    .evidence_availability
                    .entry(scoped_evidence_key(&event.scope, slot.evidence_id()))
                    .or_insert(EvidenceAvailability::Available);
            }
        }
        EventKind::DraftProposed(payload) => {
            projection
                .proposed_drafts
                .push(payload.draft_id.to_string());
        }
        EventKind::DraftRejected(payload) => {
            projection
                .rejected_drafts
                .push(payload.draft_id.to_string());
        }
        EventKind::DraftApplied(payload) => {
            projection.applied_drafts.push(payload.draft_id.to_string());
        }
        EventKind::PolicyWaiverCreated(payload) => projection.waivers.push(payload.waiver.clone()),
        EventKind::SimulationStarted(_) => {
            projection.simulation_status = Some(SimulationStatus::Running);
        }
        EventKind::NodeStateChanged(payload) => {
            projection
                .node_states
                .insert(payload.node_id.to_string(), payload.next_state);
        }
        EventKind::ExecutionStarted(payload) => {
            if projection.execution_id.is_some() {
                return Err(ReplayError::Corrupt);
            }
            projection.execution_id = Some(payload.execution_id.to_string());
            projection.mode = Some(payload.mode);
        }
        EventKind::ExecutionModeChanged(payload) => {
            // An unrooted stream must not acquire an autonomy mode. With no execution started both
            // sides are None, so the previous-mode comparison alone would let it through.
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str())
                || projection.mode != payload.previous_mode
            {
                return Err(ReplayError::Corrupt);
            }
            projection.mode = Some(payload.mode);
        }
        EventKind::NodeOutcomeRecorded(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            let node = payload.node_id.to_string();
            if projection.node_attempts.len() >= MAX_PROJECTION_NODES
                && !projection.node_attempts.contains_key(&node)
            {
                return Err(ReplayError::LimitExceeded);
            }
            // An attempt is one entry into Running, not every report about a node. Gating on
            // `Started` alone would double-count, because apply_transition needs two of them to
            // dispatch: Ready -> Queued, then Queued -> Running. Keying on the resulting state is
            // unambiguous under either journalling convention 04c settles on.
            let attempts = projection.node_attempts.entry(node.clone()).or_insert(0);
            if payload.outcome == NodeOutcome::Started && payload.next_state == NodeState::Running {
                *attempts = attempts.checked_add(1).ok_or(ReplayError::LimitExceeded)?;
            }

            // `Started` is dispatch bookkeeping. It counts as an attempt when it enters `Running`,
            // but it neither extends nor breaks a run of identical work outcomes — otherwise the
            // two-hop dispatch pattern makes MAX_IDENTICAL_OUTCOMES structurally unreachable,
            // which the 04e review proved holds for every state-machine-conforming driver.
            if payload.outcome != NodeOutcome::Started {
                let run = match projection
                    .last_outcome
                    .insert(node.clone(), payload.outcome)
                {
                    Some(previous) if previous == payload.outcome => projection
                        .identical_outcomes
                        .get(&node)
                        .copied()
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or(ReplayError::LimitExceeded)?,
                    _ => 1,
                };
                projection.identical_outcomes.insert(node.clone(), run);
            }
            // M11 #160: parking MINTS a wait whose identity is this envelope's sequence, and
            // re-parking SUPERSEDES the previous one under the same node name. Leaving the old
            // entry would let a claim answer a wait that is no longer open — the stale-rendezvous
            // trap, which the refusal path exists to catch precisely because this map makes it
            // detectable. Any other next_state closes the wait: the node is no longer waiting.
            if payload.next_state == NodeState::WaitingInput {
                if projection.open_waits.len() >= MAX_PROJECTION_NODES
                    && !projection.open_waits.contains_key(&node)
                {
                    return Err(ReplayError::LimitExceeded);
                }
                projection.open_waits.insert(
                    node.clone(),
                    OpenWait {
                        at_sequence: event.sequence,
                        deadline: None,
                    },
                );
                record_scan(
                    projection,
                    &node,
                    CustomsScan {
                        at_sequence: event.sequence,
                        stage: CustomsStage::Parked,
                        claim_seq: None,
                        reason_code: None,
                        deadline: None,
                    },
                )?;
            } else {
                projection.open_waits.remove(&node);
            }
            projection.node_states.insert(node, payload.next_state);
        }
        EventKind::CompletionClaimed(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            let node = payload.node.to_string();
            // A claim naming a wait that is not the node's OPEN wait is not an uninterpretable
            // journal — it is a mistake the command layer should have refused, and if it reached
            // the log the honest fold answer is to record it as spent testimony against nothing
            // rather than to poison every later replay. Corrupt is reserved for logs that cannot
            // be READ; this one reads fine and says something false. (M09's refusal rule.)
            let answers_open_wait = projection
                .open_waits
                .get(&node)
                .is_some_and(|wait| wait.at_sequence == payload.completes_wait_seq);
            if projection.open_claims.len() >= MAX_PROJECTION_NODES {
                return Err(ReplayError::LimitExceeded);
            }
            if answers_open_wait {
                projection.open_claims.insert(
                    event.sequence,
                    OpenClaim {
                        node: node.clone(),
                        completes_wait_seq: payload.completes_wait_seq,
                        stage_entered_at: event.sequence,
                        deadline: None,
                    },
                );
                record_scan(
                    projection,
                    &node,
                    CustomsScan {
                        at_sequence: event.sequence,
                        stage: CustomsStage::Claimed,
                        claim_seq: Some(event.sequence),
                        reason_code: None,
                        deadline: None,
                    },
                )?;
            }
            // NOTE, load-bearing: nothing here touches `node_states`. A claim does NOT release a
            // dependent; only a clearance does. This absence IS the feature.
        }
        EventKind::CompletionCleared(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            // A clearance naming a sequence that is not a claim IS uninterpretable: the
            // countersignature has no testimony under it, and no later reader can decide what was
            // cleared. That is the Corrupt case, and it is the only one in this family.
            let Some(claim) = projection.open_claims.remove(&payload.claim_seq) else {
                return Err(ReplayError::Corrupt);
            };
            // THE release, and the only one: the fold performs the transition itself so
            // `ready_set(spec, states)` keeps its signature and BOTH drivers inherit readiness
            // with zero edits — one derivation, two call sites, neither of them changed.
            projection.open_waits.remove(&claim.node);
            record_scan(
                projection,
                &claim.node,
                CustomsScan {
                    at_sequence: event.sequence,
                    stage: CustomsStage::Cleared,
                    claim_seq: Some(payload.claim_seq),
                    reason_code: None,
                    deadline: None,
                },
            )?;
            projection
                .node_states
                .insert(claim.node, NodeState::Succeeded);
        }
        EventKind::CompletionRejected(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            // Same interpretability rule as clearance: a rejection with no claim under it cannot
            // be read. The claim is spent; the node stays parked, waiting for testimony that
            // clears.
            let Some(claim) = projection.open_claims.remove(&payload.claim_seq) else {
                return Err(ReplayError::Corrupt);
            };
            record_scan(
                projection,
                &claim.node,
                CustomsScan {
                    at_sequence: event.sequence,
                    stage: CustomsStage::Rejected,
                    claim_seq: Some(payload.claim_seq),
                    // The reason belongs to the claim's outcome record (#161), reachable through
                    // claim_seq. Copying it here would be the second writer of one fact.
                    reason_code: None,
                    deadline: None,
                },
            )?;
        }
        EventKind::CompletionRefused(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            // A recorded refusal changes NO state — that is its point. The node stays parked and
            // the trail stays legible: the journal shows the attempt, its target and its reason.
            record_scan(
                projection,
                payload.node.as_str(),
                CustomsScan {
                    at_sequence: event.sequence,
                    stage: CustomsStage::Refused,
                    claim_seq: None,
                    reason_code: Some(payload.reason_code.clone()),
                    deadline: None,
                },
            )?;
        }
        EventKind::OverdueException(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            if projection.exception_marked.len() >= MAX_PROJECTION_NODES {
                return Err(ReplayError::LimitExceeded);
            }
            // Idempotency grain is the EPISODE (the stage-entry sequence), so a later sweep at any
            // as_of raises nothing more for it. A re-entry mints a new episode and is eligible
            // again — deliberate, and the reason an advisory exception nobody acts on does not
            // re-fire within its episode.
            projection.exception_marked.insert(payload.episode_seq);
            record_scan(
                projection,
                payload.node.as_str(),
                CustomsScan {
                    at_sequence: event.sequence,
                    stage: CustomsStage::Overdue,
                    claim_seq: payload.claim_seq,
                    reason_code: None,
                    deadline: None,
                },
            )?;
        }
        EventKind::SweepPerformed(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            // The sweep's own record. `as_of` is journal data, never a clock read: that is what
            // makes a replayed sweep reproduce the identical exception set.
        }
        EventKind::ClearanceIdentityRegistered(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            if projection.clearance_registry.len() >= MAX_PROJECTION_NODES
                && !projection
                    .clearance_registry
                    .contains_key(payload.identity.as_str())
            {
                return Err(ReplayError::LimitExceeded);
            }
            // Re-registering an identity REPLACES its fingerprint: a rotation is a register with
            // new key material, and the last one before a clearance is the one that clearance is
            // judged against. Same overwrite discipline as a re-arm replacing a lease.
            projection.clearance_registry.insert(
                payload.identity.as_str().to_owned(),
                payload.key_fingerprint.clone(),
            );
        }
        EventKind::ClearanceIdentityRevoked(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            // Revoking an identity that is not registered is a faithfully recorded mistake, not
            // an uninterpretable log: it reads fine and says something useless. Corrupt stays
            // reserved for logs that cannot be READ (the M09 refusal rule).
            projection
                .clearance_registry
                .remove(payload.identity.as_str());
        }
        EventKind::ExecutionCompleted(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            projection.simulation_status = Some(payload.status.clone());
        }
        EventKind::SignalRecorded(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            projection.signals_recorded = projection
                .signals_recorded
                .checked_add(1)
                .ok_or(ReplayError::LimitExceeded)?;
        }
        EventKind::GhostNodeProposed(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            let node = payload.node_id.to_string();
            // A ghost is born, not transitioned into. A node that already has any state cannot
            // be proposed again; that history cannot have happened.
            if projection.node_states.contains_key(&node) {
                return Err(ReplayError::Corrupt);
            }
            if projection.node_states.len() >= MAX_PROJECTION_NODES {
                return Err(ReplayError::LimitExceeded);
            }
            projection.node_states.insert(node, NodeState::Ghost);
        }
        EventKind::MutationAccepted(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str())
                || projection.mode != Some(payload.mode)
            {
                return Err(ReplayError::Corrupt);
            }
            projection.accepted_mutations = projection
                .accepted_mutations
                .checked_add(1)
                .ok_or(ReplayError::LimitExceeded)?;
        }
        EventKind::ExecutionPaused(payload) => {
            // `ExecutionStarted` sets `execution_id` and `mode` but leaves `simulation_status`
            // `None` — nothing sets it until `simulation_started`, `simulation_completed` or
            // `execution_completed` folds. A fresh, not-yet-simulating execution is therefore
            // `None`, and pausing it is coherent history exactly like pausing one already
            // `Running`.
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str())
                || !matches!(
                    projection.simulation_status,
                    None | Some(SimulationStatus::Running)
                )
            {
                return Err(ReplayError::Corrupt);
            }
            projection.simulation_status = Some(SimulationStatus::Paused);
        }
        EventKind::ExecutionResumed(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str())
                || projection.simulation_status != Some(SimulationStatus::Paused)
            {
                return Err(ReplayError::Corrupt);
            }
            projection.simulation_status = Some(SimulationStatus::Running);
        }
        EventKind::SimulationCompleted(payload) => {
            projection.simulation_status = Some(payload.status.clone());
        }
        EventKind::EvidenceErasureRequested(payload) => {
            projection.evidence_availability.insert(
                scoped_evidence_key(&payload.evidence_scope, &payload.evidence_id),
                EvidenceAvailability::ErasurePending,
            );
        }
        EventKind::EvidenceErasureCompleted(payload) => {
            projection.evidence_availability.insert(
                scoped_evidence_key(&payload.evidence_scope, &payload.evidence_id),
                EvidenceAvailability::Erased,
            );
        }
        EventKind::EvidenceCiphertextDeleted(payload) => {
            projection.evidence_availability.insert(
                scoped_evidence_key(&payload.evidence_scope, &payload.evidence_id),
                EvidenceAvailability::Deleted,
            );
        }
        EventKind::EvidenceLegalHoldChanged(payload) => update_legal_hold_projection(
            projection,
            active_legal_holds,
            scoped_evidence_key(&payload.evidence_scope, &payload.evidence_id),
            payload.hold_id.as_str(),
            payload.state,
        ),
        // Ledger, not state (05c Task 9b): a reuse decision changes no node state and no
        // counter — the savings accounting arrives with its producer (the 05d executor), and
        // this arm is explicit rather than a wildcard so the closed set keeps forcing a
        // deliberate decision per kind.
        EventKind::ReuseDecision(_) => {}
        // 05g: the wake doorbell's ledger half. The fold is PURE — no ring, no pipe, no
        // clock lives here; a replay rebuilds lease state and never touches a rendezvous
        // (the ringer is the serve layer's post-append hook, outside this crate).
        EventKind::WakeLease(payload) => {
            projection.wake_leases.insert(
                payload.session_id.as_str().to_owned(),
                WakeLeaseState {
                    cursor: payload.cursor,
                    rendezvous_id: payload.rendezvous_id.as_str().to_owned(),
                    // The envelope's own sequence — see the field's own note for why it is
                    // derived here rather than carried in the payload.
                    armed_at_sequence: event.sequence,
                    // The horizon, computed HERE from the event's own recorded instant plus
                    // the duration the sleeper declared. Pure arithmetic over the log: no
                    // clock is read, so replay stays byte-identical, and the reader is handed
                    // an instant rather than a sum it has to work out against a clock of its
                    // own.
                    matures_at: payload.matures_in_seconds.and_then(|seconds| {
                        let base = *event.occurred_at.as_datetime();
                        let seconds = i64::try_from(seconds).ok()?;
                        let horizon =
                            base.checked_add_signed(chrono::Duration::seconds(seconds))?;
                        PersistedTimestamp::from_datetime(horizon).ok()
                    }),
                },
            );
        }
        // M06: the verdict is ledger, not state — the WHY beside the outcome the driver
        // records separately; an explicit no-op arm, never a wildcard (the ReuseDecision
        // precedent).
        EventKind::GateVerdict(_) => {}
        EventKind::GateCertified(payload) => {
            projection.gate_certifications.insert(
                payload.gate_id.as_str().to_owned(),
                payload.suite_digest.as_str().to_owned(),
            );
        }
        EventKind::WakeLeaseConsumed(payload) => {
            let Some(burned) = projection.wake_leases.remove(payload.session_id.as_str()) else {
                // History cannot burn a lease that was never armed.
                return Err(ReplayError::Corrupt);
            };
            // Did it burn the arming it named? RECORDED, NEVER REFUSED.
            //
            // A consumption naming a different arming than the one it burned is a wrong action
            // faithfully recorded — the log is consistent and replay reconstructs it exactly.
            // That is a different thing from the refusal above, which fires on a log that
            // CANNOT be interpreted at all, and conflating them would make history unreadable
            // because it recorded something bad. The wake subsystem's own rule points the same
            // way: a wake failure never fails the route that triggered it.
            //
            // Absent means a consumption written before the field existed, and absence stays
            // absence — no comparison, no invention.
            if let Some(captured) = payload.captured_arming
                && captured != burned.armed_at_sequence
            {
                projection.wake_mis_burns.insert(
                    payload.session_id.as_str().to_owned(),
                    WakeMisBurn {
                        at_sequence: event.sequence,
                        captured_arming: captured,
                        live_arming: burned.armed_at_sequence,
                    },
                );
            }
            // F4: the burn leaves a receipt. Recorded from the CONSUMPTION event (its own
            // reason and its own sequence), never reconstructed from the lease that was
            // just removed — the lease knows when it was armed, not why or when it died.
            projection.wake_last_consumed.insert(
                payload.session_id.as_str().to_owned(),
                WakeConsumptionReceipt {
                    reason: payload.reason,
                    sequence: event.sequence,
                },
            );
        }
        EventKind::GraphImported(_)
        | EventKind::GraphValidationFailed(_)
        | EventKind::PolicyObligationEvaluated(_)
        | EventKind::IntegrityCheckpointCreated(_) => {}
    }
    Ok(())
}

impl ReplayError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::LimitExceeded => "GHE006_LIMIT_EXCEEDED",
            Self::Corrupt => "GHE005_INTEGRITY_FAILURE",
        }
    }
}

/// Rebuilds current state without decrypting Evidence or interpreting authoring records.
pub fn replay(
    expected_scope: &graphhelm_protocols::RepositoryScope,
    expected_stream_id: &str,
    events: &[EventEnvelope],
) -> Result<ExecutionProjection, ReplayError> {
    graphhelm_protocols::OpaqueId::parse(expected_stream_id).map_err(|_| ReplayError::Corrupt)?;
    if events.len() > MAX_READ_ALL {
        return Err(ReplayError::LimitExceeded);
    }
    let schemas = graphhelm_schema::repository_schema_set().map_err(|_| ReplayError::Corrupt)?;
    let mut projection = ExecutionProjection::default();
    let mut seen = BTreeSet::new();
    let mut previous_hash = GENESIS_HASH;
    let mut scope = None;
    let mut aggregate_bytes = 0_u64;
    let mut aggregate_references = 0_usize;
    let mut active_legal_holds = BTreeSet::new();
    for (index, event) in events.iter().enumerate() {
        let event_bytes = serialized_len_bounded(event, MAX_EVENT_BYTES)
            .map_err(|_| ReplayError::LimitExceeded)?;
        aggregate_bytes = aggregate_bytes
            .checked_add(u64::try_from(event_bytes).map_err(|_| ReplayError::LimitExceeded)?)
            .ok_or(ReplayError::LimitExceeded)?;
        aggregate_references = aggregate_references
            .checked_add(event.evidence_refs.len())
            .and_then(|value| value.checked_add(event.artifact_refs.len()))
            .ok_or(ReplayError::LimitExceeded)?;
        if aggregate_bytes > MAX_JOURNAL_BYTES || aggregate_references > MAX_READ_ALL {
            return Err(ReplayError::LimitExceeded);
        }
        let value = serde_json::to_value(event).map_err(|_| ReplayError::Corrupt)?;
        if !schemas.validate_event(&value).is_empty()
            || crate::integrity::validate_envelope(event).is_err()
            || &event.scope != expected_scope
            || event.stream_id.as_str() != expected_stream_id
            || scope
                .as_ref()
                .is_some_and(|expected| expected != &event.scope)
            || event.kind.is_project_level() == event.scope.execution_id().is_some()
            || event_hash(event, previous_hash).map_err(|_| ReplayError::Corrupt)?
                != event.event_hash.as_str()
        {
            return Err(ReplayError::Corrupt);
        }
        scope.get_or_insert_with(|| event.scope.clone());
        if event.sequence != index as u64 + 1
            || event.previous_hash.as_str() != previous_hash
            || !seen.insert(event.idempotency_key.clone())
        {
            return Err(ReplayError::Corrupt);
        }
        previous_hash = event.event_hash.as_str();
        if let Some(stream) = &projection.stream_id {
            if stream != event.stream_id.as_str() {
                return Err(ReplayError::Corrupt);
            }
        } else {
            projection.stream_id = Some(event.stream_id.to_string());
        }
        // One fold, not two. A second copy of this logic is what would let a rebuilt generation
        // and a direct replay disagree, which is the property this milestone exists to prove.
        apply_projection_event(&mut projection, &mut active_legal_holds, event)?;
    }
    Ok(projection)
}

fn update_legal_hold_projection(
    projection: &mut ExecutionProjection,
    active: &mut BTreeSet<(ScopedEvidenceId, String)>,
    evidence: ScopedEvidenceId,
    hold_id: &str,
    state: graphhelm_protocols::LegalHoldState,
) {
    let hold = (evidence.clone(), hold_id.to_owned());
    match state {
        graphhelm_protocols::LegalHoldState::Placed => {
            active.insert(hold);
            projection.legal_holds.insert(evidence);
        }
        graphhelm_protocols::LegalHoldState::Released => {
            active.remove(&hold);
            if !active.iter().any(|(key, _)| key == &evidence) {
                projection.legal_holds.remove(&evidence);
            }
        }
    }
}

fn scoped_evidence_key(
    scope: &graphhelm_protocols::RepositoryScope,
    evidence_id: &EvidenceId,
) -> ScopedEvidenceId {
    ScopedEvidenceId::new(scope.clone(), evidence_id.clone())
}

#[cfg(test)]
mod execution_fields_tests {
    use super::*;

    /// A realistic pre-04b generation: every field that already existed is present, and only the
    /// fields this milestone added are absent.
    fn older_generation_fixture() -> serde_json::Value {
        serde_json::json!({
            "streamId": "stream-1",
            "currentGraph": null,
            "proposedDrafts": [],
            "rejectedDrafts": [],
            "appliedDrafts": [],
            "waivers": [],
            "nodeStates": {"start": "succeeded"},
            "simulationStatus": "completed",
            "evidenceAvailability": {},
            "legalHolds": []
        })
    }

    /// The execution fields must round-trip, because a projection generation is persisted as JSON
    /// and reloaded; a field that serializes but does not deserialize would silently reset on
    /// every rebuild.
    #[test]
    fn execution_fields_survive_a_generation_round_trip() {
        let mut projection = ExecutionProjection {
            execution_id: Some("execution-1".to_owned()),
            mode: Some(graphhelm_protocols::ExecutionMode::Supervised),
            signals_recorded: 5,
            accepted_mutations: 2,
            ..ExecutionProjection::default()
        };
        projection.node_attempts.insert("start".to_owned(), 3);
        projection.last_outcome.insert(
            "start".to_owned(),
            graphhelm_protocols::NodeOutcome::RetryableFailure,
        );
        projection.identical_outcomes.insert("start".to_owned(), 2);

        let encoded = serde_json::to_string(&projection).unwrap();
        let decoded: ExecutionProjection = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, projection);
    }

    /// An older generation predates these fields. It must load with them empty rather than fail,
    /// because generations are disposable and a rebuild will refill them.
    ///
    /// The fixture is a realistic pre-04b generation: every field that already existed is present,
    /// and only the new ones are absent. An empty `{}` would be a different and weaker test — it
    /// would pass only by defaulting fields that every stored generation actually has, and that
    /// would let a truncated state load silently instead of failing.
    #[test]
    fn a_generation_written_before_these_fields_still_loads() {
        let older = older_generation_fixture();
        let decoded: ExecutionProjection = serde_json::from_value(older).unwrap();
        assert_eq!(
            decoded.node_states.get("start"),
            Some(&NodeState::Succeeded)
        );
        assert_eq!(decoded.execution_id, None);
        assert_eq!(decoded.mode, None);
        assert!(decoded.node_attempts.is_empty());
    }

    /// A generation missing a field it should have is corrupt, not old. It must fail rather than
    /// load empty, or a truncated state would look like a legitimately empty one.
    ///
    /// Removing each required key in turn, rather than asserting on a bare `{}`. `{}` proves only
    /// that the *first* required field is required; every other one could silently acquire a
    /// default and this test would stay green.
    #[test]
    fn a_generation_missing_a_pre_existing_field_fails() {
        let complete = older_generation_fixture();
        for required in [
            "proposedDrafts",
            "rejectedDrafts",
            "appliedDrafts",
            "waivers",
            "nodeStates",
            "evidenceAvailability",
            "legalHolds",
        ] {
            let mut truncated = complete.clone();
            truncated.as_object_mut().unwrap().remove(required).unwrap();
            assert!(
                serde_json::from_value::<ExecutionProjection>(truncated).is_err(),
                "a generation missing {required} must fail rather than load empty"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};
    use graphhelm_protocols::{
        ActorId, EventHash, GraphVersionPublished, NodeType, OpaqueId, Optionality, PersistedActor,
        PersistedActorType, PersistedBudgets, PersistedControl, PersistedGraphVersion,
        PersistedGraphVersionRef, PersistedNode, PersistedTimestamp, PersistedTopology, SafeValue,
        WireHash,
    };

    use super::*;

    #[test]
    fn scoped_evidence_keys_keep_identical_ids_in_distinct_executions_separate() {
        let first_scope = RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
        );
        let second_scope = RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-2").unwrap()),
        );
        let evidence_id = EvidenceId::parse("evidence-shared").unwrap();
        let first = ScopedEvidenceId::new(first_scope.clone(), evidence_id.clone());
        let second = ScopedEvidenceId::new(second_scope, evidence_id.clone());
        let values = BTreeMap::from([(first.clone(), "erased"), (second, "available")]);

        assert_eq!(values.len(), 2);
        assert_eq!(values.get(&first), Some(&"erased"));
        assert_eq!(first.scope(), &first_scope);
        assert_eq!(first.evidence_id(), &evidence_id);
        let json = serde_json::to_string(&values).unwrap();
        assert_eq!(
            serde_json::from_str::<BTreeMap<ScopedEvidenceId, &str>>(&json).unwrap(),
            values
        );
        let canonical_key = serde_json::to_string(&first).unwrap();
        assert!(
            serde_json::from_str::<ScopedEvidenceId>(&canonical_key.replace("v1:11:", "v1:011:"))
                .is_err()
        );
        let mut trailing = canonical_key.clone();
        trailing.insert(trailing.len() - 1, 'x');
        assert!(serde_json::from_str::<ScopedEvidenceId>(&trailing).is_err());
        let mut truncated = canonical_key;
        truncated.remove(truncated.len() - 2);
        assert!(serde_json::from_str::<ScopedEvidenceId>(&truncated).is_err());
    }

    #[test]
    fn releasing_one_of_two_holds_keeps_evidence_held() {
        let scope = RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
        );
        let evidence = ScopedEvidenceId::new(scope, EvidenceId::parse("evidence-shared").unwrap());
        let mut projection = ExecutionProjection::default();
        let mut active = BTreeSet::new();
        update_legal_hold_projection(
            &mut projection,
            &mut active,
            evidence.clone(),
            "hold-1",
            graphhelm_protocols::LegalHoldState::Placed,
        );
        update_legal_hold_projection(
            &mut projection,
            &mut active,
            evidence.clone(),
            "hold-2",
            graphhelm_protocols::LegalHoldState::Placed,
        );
        update_legal_hold_projection(
            &mut projection,
            &mut active,
            evidence.clone(),
            "hold-1",
            graphhelm_protocols::LegalHoldState::Released,
        );
        assert!(projection.legal_holds.contains(&evidence));
        update_legal_hold_projection(
            &mut projection,
            &mut active,
            evidence.clone(),
            "hold-2",
            graphhelm_protocols::LegalHoldState::Released,
        );
        assert!(!projection.legal_holds.contains(&evidence));
    }

    fn publication_event(
        version: PersistedGraphVersion,
        scope_execution: &str,
        references: Vec<graphhelm_protocols::EvidenceReference>,
    ) -> EventEnvelope {
        let actor = version.created_by().clone();
        let mut event = EventEnvelope::new(
            OpaqueId::parse("event-publication").unwrap(),
            graphhelm_protocols::RepositoryScope::new(
                graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
                graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
                Some(graphhelm_protocols::ExecutionId::parse(scope_execution).unwrap()),
            ),
            OpaqueId::parse("stream-1").unwrap(),
            1,
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap())
                .unwrap(),
            graphhelm_protocols::NewEvent::new(
                OpaqueId::parse("request-publication").unwrap(),
                actor,
                graphhelm_protocols::Sensitivity::Internal,
                EventKind::GraphVersionPublished(Box::new(GraphVersionPublished { version })),
                references,
                vec![],
            ),
            EventHash::parse(GENESIS_HASH).unwrap(),
            EventHash::parse(GENESIS_HASH).unwrap(),
        );
        event.event_hash = EventHash::parse(event_hash(&event, GENESIS_HASH).unwrap()).unwrap();
        event
    }

    fn invalid_projection() -> PersistedGraphVersion {
        let completion = PersistedControl::new(
            SafeValue::parse("all_terminal").unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
        let node = PersistedNode::new(NodeType::Tool, Optionality::Required, vec![], vec![], None, None)
            .unwrap();
        let topology = PersistedTopology::new(
            OpaqueId::parse("graph-1").unwrap(),
            graphhelm_protocols::ExecutionId::parse("execution-1").unwrap(),
            BTreeMap::new(),
            vec![OpaqueId::parse("missing-entrypoint").unwrap()],
            BTreeMap::from([(OpaqueId::parse("node-1").unwrap(), node)]),
            vec![],
            PersistedBudgets::default(),
            vec![],
            completion,
        )
        .unwrap();
        PersistedGraphVersion::new(
            1,
            None,
            topology,
            WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
            vec![],
            PersistedActor::new(
                PersistedActorType::System,
                ActorId::parse("system-test").unwrap(),
            ),
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap())
                .unwrap(),
        )
        .unwrap()
    }

    fn declaration_event(sequence: u64, id: &str) -> EventEnvelope {
        let mut envelope = EventEnvelope::new(
            OpaqueId::parse(id).unwrap(),
            graphhelm_protocols::RepositoryScope::new(
                graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
                graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
                Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
            ),
            OpaqueId::parse("stream-1").unwrap(),
            sequence,
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap())
                .unwrap(),
            graphhelm_protocols::NewEvent::new(
                OpaqueId::parse(id).unwrap(),
                PersistedActor::new(
                    PersistedActorType::System,
                    ActorId::parse("system-test").unwrap(),
                ),
                graphhelm_protocols::Sensitivity::Internal,
                EventKind::ExecutionFormDeclared(ExecutionFormDeclared {
                    execution_id: OpaqueId::parse("execution-1").unwrap(),
                    node_ids: vec![
                        OpaqueId::parse("plan").unwrap(),
                        OpaqueId::parse("tests").unwrap(),
                    ],
                    node_timeout_seconds: std::collections::BTreeMap::from([(
                        OpaqueId::parse("tests").unwrap(),
                        1800,
                    )]),
                }),
                vec![],
                vec![],
            ),
            EventHash::parse(GENESIS_HASH).unwrap(),
            EventHash::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
        );
        // The chain has to actually close, or `replay` refuses the event for a reason that has
        // nothing to do with what this test is asking about — and a refusal that arrives for
        // the wrong reason is a test measuring its own fixture.
        let previous = envelope.previous_hash.as_str().to_owned();
        let computed = crate::canonical::event_hash(&envelope, &previous).unwrap();
        envelope.event_hash = EventHash::parse(computed).unwrap();
        envelope
    }

    /// The declared shape reaches the projection, in its own home.
    ///
    /// It deliberately does NOT touch `current_graph`. That field means "this was sealed and
    /// published"; this one means "this was declared". Keeping them apart is what stops a
    /// declaration from being read as a publication by a rule that needs sealed evidence.
    #[test]
    fn a_declared_shape_reaches_the_projection_without_touching_the_published_graph() {
        let event = declaration_event(1, "event-1");
        let scope = event.scope.clone();
        let projection = replay(&scope, "stream-1", &[event]).expect("a declaration replays");
        let declared = projection
            .declared_form
            .expect("the declared shape must reach the projection");
        assert_eq!(declared.node_ids.len(), 2);
        // A node that declared no deadline has NO entry — absence stays absence.
        assert!(
            !declared
                .node_timeout_seconds
                .contains_key(&OpaqueId::parse("plan").unwrap())
        );
        assert_eq!(
            declared
                .node_timeout_seconds
                .get(&OpaqueId::parse("tests").unwrap()),
            Some(&1800)
        );
        assert!(
            projection.current_graph.is_none(),
            "a declaration is not a publication and must not fill the sealed graph"
        );
    }

    /// Two declarations on one stream give the shape two owners, and the fold refuses it.
    #[test]
    fn a_second_declaration_on_one_stream_is_corrupt() {
        let first = declaration_event(1, "event-1");
        let scope = first.scope.clone();
        assert_eq!(
            replay(
                &scope,
                "stream-1",
                &[first, declaration_event(2, "event-2")]
            ),
            Err(ReplayError::Corrupt)
        );
    }

    #[test]
    fn replay_rejects_semantically_invalid_persisted_graph() {
        let event = EventEnvelope::new(
            OpaqueId::parse("event-1").unwrap(),
            graphhelm_protocols::RepositoryScope::new(
                graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
                graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
                Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
            ),
            OpaqueId::parse("stream-1").unwrap(),
            1,
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap())
                .unwrap(),
            graphhelm_protocols::NewEvent::new(
                OpaqueId::parse("request-1").unwrap(),
                PersistedActor::new(
                    PersistedActorType::System,
                    ActorId::parse("system-test").unwrap(),
                ),
                graphhelm_protocols::Sensitivity::Internal,
                EventKind::GraphVersionPublished(Box::new(GraphVersionPublished {
                    version: invalid_projection(),
                })),
                vec![],
                vec![],
            ),
            EventHash::parse(GENESIS_HASH).unwrap(),
            EventHash::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
        );

        let scope = event.scope.clone();
        assert_eq!(
            replay(&scope, "stream-1", &[event]),
            Err(ReplayError::Corrupt)
        );
    }

    #[test]
    fn replay_rejects_a_valid_projection_with_divergent_predecessor_identity() {
        let active: PersistedGraphVersion = serde_json::from_str(include_str!(
            "../../../conformance/schemas/valid/persisted-graph-version.json"
        ))
        .unwrap();
        let divergent = PersistedGraphVersion::new(
            active.number() + 1,
            Some(
                PersistedGraphVersionRef::new(
                    active.number(),
                    WireHash::parse(format!("sha256:{}", "f".repeat(64))).unwrap(),
                )
                .unwrap(),
            ),
            active.topology().clone(),
            active.topology_hash().clone(),
            active.semantic_hash().clone(),
            active.content_slots().to_vec(),
            active.created_by().clone(),
            active.created_at().clone(),
        )
        .unwrap();
        assert!(graphhelm_graph::validate_persisted_projection(&divergent).is_ok());
        let make_event = |version, sequence, previous: &str, hash: &str| {
            EventEnvelope::new(
                OpaqueId::parse(format!("event-{sequence}")).unwrap(),
                graphhelm_protocols::RepositoryScope::new(
                    graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
                    graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
                    Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
                ),
                OpaqueId::parse("stream-1").unwrap(),
                sequence,
                PersistedTimestamp::from_datetime(
                    Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap(),
                )
                .unwrap(),
                graphhelm_protocols::NewEvent::new(
                    OpaqueId::parse(format!("request-{sequence}")).unwrap(),
                    PersistedActor::new(
                        PersistedActorType::System,
                        ActorId::parse("system-test").unwrap(),
                    ),
                    graphhelm_protocols::Sensitivity::Internal,
                    EventKind::GraphVersionPublished(Box::new(GraphVersionPublished { version })),
                    vec![],
                    vec![],
                ),
                EventHash::parse(previous).unwrap(),
                EventHash::parse(hash).unwrap(),
            )
        };
        let first_hash = format!("sha256:{}", "c".repeat(64));
        let events = vec![
            make_event(active, 1, GENESIS_HASH, &first_hash),
            make_event(
                divergent,
                2,
                &first_hash,
                &format!("sha256:{}", "d".repeat(64)),
            ),
        ];

        let scope = events[0].scope.clone();
        assert_eq!(
            replay(&scope, "stream-1", &events),
            Err(ReplayError::Corrupt)
        );
    }

    #[test]
    fn replay_rejects_more_than_the_public_event_bound_before_event_work() {
        let event = EventEnvelope::new(
            OpaqueId::parse("event-1").unwrap(),
            graphhelm_protocols::RepositoryScope::new(
                graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
                graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
                Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
            ),
            OpaqueId::parse("stream-1").unwrap(),
            1,
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap())
                .unwrap(),
            graphhelm_protocols::NewEvent::new(
                OpaqueId::parse("request-1").unwrap(),
                PersistedActor::new(
                    PersistedActorType::System,
                    ActorId::parse("system-test").unwrap(),
                ),
                graphhelm_protocols::Sensitivity::Internal,
                EventKind::GraphImported(graphhelm_protocols::GraphImported {
                    source_sha256: graphhelm_protocols::RawSha256::parse("a".repeat(64)).unwrap(),
                    source_kind: graphhelm_protocols::GraphSourceKind::GraphDocument,
                }),
                vec![],
                vec![],
            ),
            EventHash::parse(GENESIS_HASH).unwrap(),
            EventHash::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
        );
        let events = vec![event; 100_001];

        let scope = events[0].scope.clone();
        assert_eq!(
            replay(&scope, "stream-1", &events),
            Err(ReplayError::LimitExceeded)
        );
    }

    #[test]
    fn replay_binds_publication_execution_and_exact_ordered_evidence_bijection() {
        let original: PersistedGraphVersion = serde_json::from_str(include_str!(
            "../../../conformance/schemas/valid/persisted-graph-version.json"
        ))
        .unwrap();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-fixture").unwrap()),
        );
        let slots = original
            .content_slots()
            .iter()
            .map(|slot| {
                graphhelm_protocols::ContentSlot::new(
                    slot.slot_id().clone(),
                    slot.owner_kind(),
                    slot.owner_id().clone(),
                    slot.field_kind(),
                    slot.ordinal(),
                    graphhelm_graph::derive_publication_evidence_id(
                        &scope,
                        1,
                        original.semantic_hash(),
                        slot,
                    )
                    .unwrap(),
                    slot.content_sha256().clone(),
                    slot.sensitivity(),
                    slot.required_for_execution(),
                )
            })
            .collect();
        let version = PersistedGraphVersion::new(
            1,
            None,
            original.topology().clone(),
            original.topology_hash().clone(),
            original.semantic_hash().clone(),
            slots,
            original.created_by().clone(),
            original.created_at().clone(),
        )
        .unwrap();
        let references = version
            .content_slots()
            .iter()
            .map(|slot| {
                graphhelm_protocols::EvidenceReference::new(
                    slot.evidence_id().clone(),
                    slot.content_sha256().clone(),
                    graphhelm_protocols::RawSha256::parse("f".repeat(64)).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let valid = publication_event(version.clone(), "execution-fixture", references.clone());
        let expected_scope = valid.scope.clone();
        assert!(replay(&expected_scope, "stream-1", &[valid]).is_ok());

        let arbitrary_slots = version
            .content_slots()
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                graphhelm_protocols::ContentSlot::new(
                    slot.slot_id().clone(),
                    slot.owner_kind(),
                    slot.owner_id().clone(),
                    slot.field_kind(),
                    slot.ordinal(),
                    graphhelm_protocols::EvidenceId::parse(format!("arbitrary-evidence-{index}"))
                        .unwrap(),
                    slot.content_sha256().clone(),
                    slot.sensitivity(),
                    slot.required_for_execution(),
                )
            })
            .collect::<Vec<_>>();
        let arbitrary = PersistedGraphVersion::new(
            version.number(),
            version.predecessor().cloned(),
            version.topology().clone(),
            version.topology_hash().clone(),
            version.semantic_hash().clone(),
            arbitrary_slots,
            version.created_by().clone(),
            version.created_at().clone(),
        )
        .unwrap();
        let arbitrary_refs = arbitrary
            .content_slots()
            .iter()
            .map(|slot| {
                graphhelm_protocols::EvidenceReference::new(
                    slot.evidence_id().clone(),
                    slot.content_sha256().clone(),
                    graphhelm_protocols::RawSha256::parse("f".repeat(64)).unwrap(),
                )
            })
            .collect();
        let internally_consistent =
            publication_event(arbitrary, "execution-fixture", arbitrary_refs);
        assert_eq!(
            replay(&expected_scope, "stream-1", &[internally_consistent]),
            Err(ReplayError::Corrupt)
        );

        let wrong_execution =
            publication_event(version.clone(), "execution-other", references.clone());
        let wrong_execution_scope = wrong_execution.scope.clone();
        assert_eq!(
            replay(&wrong_execution_scope, "stream-1", &[wrong_execution]),
            Err(ReplayError::Corrupt)
        );

        let mut wrong_order = references;
        wrong_order.swap(0, 1);
        let wrong_order = publication_event(version, "execution-fixture", wrong_order);
        assert_eq!(
            replay(&expected_scope, "stream-1", &[wrong_order]),
            Err(ReplayError::Corrupt)
        );
    }
}
