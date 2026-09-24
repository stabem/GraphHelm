//! The operator's sleep question, decided ONCE (M07 F1).
//!
//! The blind judge refused the M06 dogfood story because the one-glance surface reported
//! green on a wedged execution. The defect was never the wording: three surfaces each
//! decided "is anything wrong?" on their own (`execution::render`, the monitor page, and
//! `recovery`'s inline triage condition), so a surface could be honest and still disagree
//! with its neighbour. This module is the one home for that predicate; every surface
//! CALLS it and none recomputes, which is what makes the one-truth test meaningful rather
//! than coincidental.
//!
//! Two predicate corrections against the declared contract, contested in Task 1's handoff
//! and pinned by `core/execution/tests/attention.rs`:
//! - [`NodeState::WaitingCapacity`] is NOT a wedge. It is the §12 park-and-wait rule doing
//!   its job (the M05 acceptance clause `exhausted-route-parks`): quota returns and the
//!   node advances with no operator action. Paging for it would be a false alarm on a
//!   designed state — and false alarms are how an attention field dies.
//! - The wedge is decided against the PUBLISHED TOPOLOGY, never against `node_states`
//!   alone: an untouched node is absent from that map, and absence means queued work (the
//!   driver defaults it to `Draft`). Reading silence as "nothing can advance" would have
//!   screamed at every just-started execution and at every window between one node
//!   finishing and the next dispatching — the exact moments an operator looks.
//! - [`NodeState::WaitingInput`] IS attention, but under its own reason. Nothing advances
//!   until the owner answers, so the operator must be told; calling it a wedge would
//!   misname a system that is working correctly and waiting on a human, which is the same
//!   class of dishonesty F1 exists to kill.

use graphhelm_events::ExecutionProjection;
use std::collections::BTreeMap;

use graphhelm_protocols::{NodeOutcome, NodeState, SimulationStatus};
use serde::{Deserialize, Serialize};

/// What the seam cannot see, injected by the surface that can (M08 Task 1).
///
/// No time type crosses this boundary, and that is the point. `core/execution` forbids
/// `chrono` in production dependencies (`the_execution_crate_has_no_impure_dependency`),
/// and the projection folds no per-node instant at all — only `last_event_hash`, a hash.
/// So the SURFACE does the subtraction, where a clock is allowed and where the event tail
/// already lives, and the seam does the judgement. Purity stops depending on discipline and
/// becomes a property of the signature.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttentionInputs {
    /// Seconds since each node's newest event, computed by the surface. Only nodes in
    /// flight are consulted; a node missing here cannot be judged, and says so.
    pub node_silence_seconds: BTreeMap<String, u64>,
    /// Per NODE ceiling, keyed by node id, from the `timeoutSeconds` the operator declared
    /// on that node — never guessed by the seam, which cannot see it.
    ///
    /// Keyed by node and not by node TYPE, which is what this field used to be: the
    /// declaration the linter has always demanded (`GHG101_DEFAULT_TIMEOUT`, on
    /// `/spec/nodes/<id>/timeoutSeconds`) is per node, and a per-type map could only have
    /// been filled by a number somebody invented. A node whose declaration is absent or
    /// unreadable simply has no entry here, and absence stays UNKNOWN all the way to the
    /// verdict — quiet is never inferred from a budget nobody wrote down.
    pub silence_budget_seconds: BTreeMap<String, u64>,
    /// Where the surface was looking when it asked — the head sequence of the read.
    ///
    /// Injected rather than folded, because it is a fact about the READ and not about the
    /// history: the projection does not carry it. `None` means this surface did not report
    /// where it looked, and a remedy built from it says so instead of writing a zero that
    /// would read like sequence zero.
    pub at_sequence: Option<u64>,
}

impl AttentionInputs {
    /// The ONE feed. Every surface asks for its inputs here instead of assembling them (#176).
    ///
    /// The rule (`attention`) has always been shared and could not drift. The FEED could: each
    /// surface built this struct itself, and a surface that filled `silence_budget_seconds` from
    /// some other source would compile without complaint and then disagree with its siblings
    /// while applying the identical rule. The symptom is "same rule, different verdicts", which
    /// nobody hunts for, because the first thing anyone checks is whether the rule is shared --
    /// and it is.
    ///
    /// #176 deferred this while two surfaces did the assembling, on the ground that a constructor
    /// with two documented callers is machinery against a hunch, and named its own trigger: the
    /// third. There were FOUR by `95a7ad9d` -- `execution/amend.rs`, `execution/list.rs`,
    /// `execution/status.rs`, `serve/monitor.rs` -- so the deferral's arithmetic no longer held.
    ///
    /// **What is derived and what is passed is the whole design.** The budgets are DERIVED here,
    /// always, from `effective_budgets`: no caller can supply them, so no caller can supply a
    /// different set. The other two fields are PASSED, because they are facts about the surface
    /// that the projection cannot know -- the subtraction needs a clock, which this crate does not
    /// have, and the vantage point is a property of the read rather than of the history. A
    /// surface that did not fetch by sequence passes `None` and says so, instead of inventing a
    /// zero that would read like sequence zero.
    #[must_use]
    pub fn for_surface(
        projection: &ExecutionProjection,
        node_silence_seconds: BTreeMap<String, u64>,
        at_sequence: Option<u64>,
    ) -> Self {
        Self {
            node_silence_seconds,
            silence_budget_seconds: effective_budgets(projection),
            at_sequence,
        }
    }
}

/// A list that cannot be empty, enforced by the constructor rather than by a test.
///
/// The reviewer's attack on my first design, accepted: an assertion ("reasons is empty only
/// when the verdict is can_sleep") can be forgotten in the next refactor, while a type that
/// cannot hold the illegal state fails to COMPILE. No dependency is added for this -- the
/// shape is four lines, and paying a crate for it would be the expensive half of a cheap
/// idea.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    /// The only way in. `None` for an empty input, so the caller must handle the case where
    /// there is nothing to say -- which is exactly the case that used to become a silent
    /// empty list beside a non-calm verdict.
    #[must_use]
    pub fn new(items: Vec<T>) -> Option<Self> {
        if items.is_empty() {
            None
        } else {
            Some(Self(items))
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

/// Why a node in flight could not be judged — and therefore what the operator can DO.
///
/// The blind judge read a bare list of node ids and called it jargon with no severity and no
/// remedy. He was right, and the fix is not to make the unknown disappear: it is to stop the
/// unknown being mute. Each variant here is a DIFFERENT ignorance with a DIFFERENT cure, and
/// collapsing them into one word is how "I could not tell" became unactionable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnevaluatedReason {
    /// Nobody declared a bound for this node. The operator's remedy is one line of authoring
    /// (`timeoutSeconds`), which the graph linter has always warned about — the answer just
    /// never repeated it at the moment it mattered.
    NoDeclaredBudget,
    /// A bound exists but the surface supplied no age for this node. The remedy is not the
    /// operator's: it is a surface that judged without measuring, and telling someone to
    /// declare a timeout they already declared would be worse than saying nothing.
    NotMeasured,
    /// Nothing in the history records the node set this run was supposed to cover, so
    /// "everything is done" is not a statement anyone here can make. Not an alarm and not an
    /// all-clear: the question is unanswerable, which is its own answer.
    TopologyUnrecorded,
}

/// The bound in force for each node, folding declaration and amendments IN ORDER.
///
/// The effective budget for a node is the LAST amendment carrying it, or the declaration if
/// none does. Order is the whole contract: an amendment declares a bound from its own
/// sequence forward, so replaying an earlier projection (`ExecutionProjection::as_of`) yields
/// the earlier answer — the past stays honestly unjudged rather than being repainted calm by
/// a decision made after it.
///
/// A node absent from every declaration has NO entry here, and absence keeps meaning
/// "nobody said", never zero.
#[must_use]
pub fn effective_budgets(projection: &ExecutionProjection) -> BTreeMap<String, u64> {
    let mut budgets: BTreeMap<String, u64> = projection
        .declared_form
        .as_ref()
        .map(|form| {
            form.node_timeout_seconds
                .iter()
                .map(|(node, seconds)| (node.as_str().to_owned(), *seconds))
                .collect()
        })
        .unwrap_or_default();
    // In log order, so a later amendment wins by arriving later rather than by any rule
    // somebody has to remember.
    for (_, amendment) in &projection.form_amendments {
        for (node, seconds) in &amendment.node_timeout_seconds {
            budgets.insert(node.as_str().to_owned(), *seconds);
        }
    }
    budgets
}

/// What the OPERATOR can do about an unknown, carried as data inside the answer.
///
/// The judge's fifth finding: the reasons give a CAUSE and no ACTION. A surface that explains
/// a problem it cannot offer to fix strands the reader, and three surfaces each inventing
/// their own action would be the two-budgets defect wearing a button. So the remedy is
/// decided ONCE here, and the monitor, the MCP server and the CLI are renderers of it -- none
/// can offer an action the seam did not sanction, and none can stay silent about one it did.
///
/// **The value the operator must DECIDE has no field.** Not an `Option` the seam might fill:
/// there is nothing to fill. A suggested default would turn "absent means unknown" into
/// "absent means 300 seconds" through the back door -- the defect this milestone deleted from
/// the monitor on day one, returning dressed as convenience. Everything here is either a fact
/// already in the projection or the IDENTITY of the declaration that is missing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "remedy")]
pub enum Remedy {
    /// Declare a silence bound for this node. The operator supplies the number; the seam
    /// supplies only what it already saw.
    DeclareNodeBudget {
        node: String,
        observed_silence_seconds: u64,
        /// `None` when the surface did not say where it was looking. Step 2 must REFUSE a
        /// remedy it cannot place in the history, and an absent sequence is exactly that.
        computed_at_sequence: Option<u64>,
    },
    /// There is nothing to offer, said out loud. "No remedy" and "field forgotten" must not
    /// be the same bytes.
    Unavailable { because: RemedyUnavailable },
}

/// Why an unknown has no remedy. One variant today; the enum exists so a second reason
/// cannot arrive as a bare `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemedyUnavailable {
    /// The amendment declares BOUNDS, not SHAPE. An operator can attach a timeout to a node
    /// going forward, but nothing lets them say, after the fact, which nodes a run was meant
    /// to cover — so an execution whose node set was never recorded still has no remedy.
    ///
    /// This variant replaces `ShapeCannotBeRecordedAfterTheFact`, whose sentence stopped
    /// being true the moment `ExecutionFormAmended` existed. It did not survive by accident:
    /// a guard bound the claim to the absence of that event and failed the build the instant
    /// the event arrived, with the repair written into its own failure message. The claim was
    /// replaced by the commit that falsified it, which is the only honest way for a sentence
    /// like this to die.
    AmendmentDeclaresBoundsNotShape,
    /// A bound exists and the surface reported no age for the node. Nothing the operator can
    /// declare fixes a reading that was never taken; the fault is on the surface asking.
    SurfaceMeasuredNoAge,
}

/// WHAT could not be judged, and WHY — as one type, because the two are not independent.
///
/// The reviewer proposed this shape and my first implementation was WEAKER than his: I split
/// it into a `scope` enum beside a shared `reason` field, which lets
/// `{ scope: Execution, reason: NoDeclaredBudget }` exist — an execution that failed to
/// declare a per-node timeout, which is not a thing. Representable nonsense IS the defect;
/// that sentence is the whole day's lesson and I shipped a type that broke it while quoting
/// it. Each variant now carries only the reasons that can apply to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "scope")]
pub enum Unevaluated {
    /// One node in flight whose silence could not be judged.
    Node {
        node: String,
        reason: NodeUnevaluated,
        remedy: Remedy,
    },
    /// The whole run, because nothing recorded what it was supposed to cover. There is no
    /// node here and no id is invented for one.
    Execution {
        reason: ExecutionUnevaluated,
        remedy: Remedy,
    },
}

impl Unevaluated {
    /// Every unknown answers this, including the ones whose answer is "nothing to offer".
    #[must_use]
    pub const fn remedy(&self) -> &Remedy {
        match self {
            Self::Node { remedy, .. } | Self::Execution { remedy, .. } => remedy,
        }
    }
}

/// Why ONE NODE's silence could not be judged. Both are curable; they differ in by whom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeUnevaluated {
    /// Nobody declared a bound for this node. The operator's remedy is one line of authoring
    /// (`timeoutSeconds`), which the graph linter has always warned about — the answer just
    /// never repeated it at the moment it mattered.
    NoDeclaredBudget,
    /// A bound exists but the surface supplied no age. The remedy is not the operator's:
    /// telling them to declare a timeout they already declared is worse than saying nothing.
    NotMeasured,
}

/// Why the RUN as a whole could not be judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionUnevaluated {
    /// Nothing in the history records the node set this run was meant to cover, so
    /// "everything is done" is not a statement anyone here can make.
    NoRecordedNodeSet,
}

/// The answer to "can I go back to sleep?", which has THREE seats and not two.
///
/// A boolean could only say yes or no, so "I could not tell" had nowhere to sit and ended up
/// in a side field while the headline kept answering "no, nothing needs you" -- which reads
/// as an all-clear. The blind judge caught exactly that: `attentionRequired:false` published
/// in the same payload as `silenceUnevaluated:['judge']`, on a node that had been running for
/// minutes without an append. A false alarm is noise; FALSE CALM is a lie, because the
/// operator who reads "you can sleep" never reaches the third field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Verdict {
    /// Something named is waiting on a human, and the evidence travels INSIDE the answer.
    ///
    /// It carries `unevaluated` too, and that is not decoration: a named reason OUTRANKS an
    /// unknown for the headline, but it must never ERASE it. The first draft of this enum
    /// dropped the list whenever a reason won, and the wire guard caught it immediately —
    /// ranking two answers is not the same as forgetting one of them.
    NeedsYou {
        reasons: NonEmpty<AttentionReason>,
        unevaluated: Vec<Unevaluated>,
    },
    /// Work is in flight whose silence could not be judged. Not an alarm -- nothing is
    /// claimed broken -- but never an all-clear either: the check that would justify one did
    /// not run, and what went unmeasured is carried here rather than beside here.
    Unknown { unevaluated: NonEmpty<Unevaluated> },
    /// Calm, but PURCHASED: a node is quiet under its current bound and would have been
    /// reported silent under a bound declared EARLIER for the same node.
    ///
    /// The reviewer found this by spinning the roulette -- 30s produced an alarm, then
    /// 100000s produced calm on a node silent for fifteen minutes. The arithmetic is right
    /// and the system must not refuse an operator's declaration. The defect was that
    /// `CanSleep` carried nothing, so "calm nobody contested" and "calm that replaced an
    /// alarm" left as the same bytes. The log held both amendments, but a surface that needs
    /// an auditor to be honest is not honest.
    ///
    /// It carries the MAGNITUDE and not just the node, because cause without magnitude is
    /// jargon -- the judge's fifth finding, answered before he can raise it again.
    CalmedByAmendment { nodes: NonEmpty<PurchasedCalm> },
    /// Nothing is waiting, everything in flight was actually CHECKED, and nobody had to raise
    /// a ceiling to get here. The ONLY variant that carries nothing, because it is the only
    /// one with nothing to say.
    CanSleep,
}

/// One node whose calm was bought by raising its ceiling, with both ends of the trade.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchasedCalm {
    pub node: String,
    /// How long the node has actually been quiet.
    pub silence_seconds: u64,
    /// The STRICTEST bound ever declared for this node, which this silence had already
    /// exceeded. Strictest rather than immediately-previous, and that is the reviewer's
    /// second question answered in code: a chain of 30 -> 100000 -> 100001 shows nothing
    /// against its predecessor while having plainly bought its calm, so the comparison is
    /// against the tightest promise ever made about this node.
    pub superseded_budget_seconds: u64,
    /// The bound in force now.
    pub budget_seconds: u64,
}

/// Nodes whose quiet today would have been an ALARM under a bound declared earlier.
///
/// The earlier bounds come from the PROJECTION -- the declaration and every amendment -- so
/// this is computed from the log rather than from anything a surface chooses to pass. The
/// current bound is the one the surface injected, because that is the number the verdict was
/// actually decided with.
fn purchased_calm(
    projection: &ExecutionProjection,
    inputs: &AttentionInputs,
) -> Vec<PurchasedCalm> {
    projection
        .node_states
        .iter()
        // The SAME question the reason arm asks. Two predicates for one question is the
        // defect this milestone has already paid for twice.
        .filter(|(node, state)| has_judgeable_silence(**state, attempts_of(projection, node)))
        .filter_map(|(node, _)| {
            let silence = *inputs.node_silence_seconds.get(node)?;
            let budget = *inputs.silence_budget_seconds.get(node)?;
            // Quiet NOW...
            if silence > budget {
                return None;
            }
            // ...but past the tightest promise ever made about this node.
            let strictest = strictest_declared_bound(projection, node)?;
            (silence > strictest).then(|| PurchasedCalm {
                node: node.clone(),
                silence_seconds: silence,
                superseded_budget_seconds: strictest,
                budget_seconds: budget,
            })
        })
        .collect()
}

/// The tightest bound ever declared for one node, across the declaration and every amendment.
fn strictest_declared_bound(projection: &ExecutionProjection, node: &str) -> Option<u64> {
    let declared = projection
        .declared_form
        .as_ref()
        .and_then(|form| {
            form.node_timeout_seconds
                .iter()
                .find(|(id, _)| id.as_str() == node)
                .map(|(_, seconds)| *seconds)
        })
        .into_iter();
    let amended = projection
        .form_amendments
        .iter()
        .filter_map(|(_, amendment)| {
            amendment
                .node_timeout_seconds
                .iter()
                .find(|(id, _)| id.as_str() == node)
                .map(|(_, seconds)| *seconds)
        });
    declared.chain(amended).min()
}

/// Whether the operator may go back to sleep, decided ONCE over the projection.
///
/// One field, not three. `reasons` and `silence_unevaluated` used to sit BESIDE the verdict,
/// and that geometry bit this project twice in one day: first a boolean that could not hold
/// "I don't know", then an `unknown` published next to an EMPTY reason list, so the field the
/// operator actually reads was blank exactly when they needed it most. Both times the data
/// that justified the answer lived next to the answer instead of inside it.
///
/// Now the evidence travels in the variant, so the flat views below are DERIVED and cannot
/// disagree with the verdict -- the same rule that made `required` a derivation in M07,
/// applied one level down.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub verdict: Verdict,
}

impl Attention {
    /// What is waiting on a human. Empty ONLY for `CanSleep` and `Unknown` -- and for
    /// `Unknown` the surface must publish [`Attention::silence_unevaluated`] instead of a
    /// bare nothing, which is the finding that produced this refactor.
    #[must_use]
    pub fn reasons(&self) -> &[AttentionReason] {
        match &self.verdict {
            Verdict::NeedsYou { reasons, .. } => reasons.as_slice(),
            Verdict::Unknown { .. } | Verdict::CalmedByAmendment { .. } | Verdict::CanSleep => &[],
        }
    }

    /// What could not be judged, each carrying the reason that names its cure.
    #[must_use]
    pub fn silence_unevaluated(&self) -> &[Unevaluated] {
        match &self.verdict {
            Verdict::Unknown { unevaluated } => unevaluated.as_slice(),
            Verdict::NeedsYou { unevaluated, .. } => unevaluated,
            Verdict::CalmedByAmendment { .. } | Verdict::CanSleep => &[],
        }
    }
}

/// Why the operator is needed. Ordered by variant, then by node id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionReason {
    /// `Blocked` + `last_outcome == Interrupted`: the 04f triage rule, now with one home.
    UntriagedInterruption { node: String },
    /// `Blocked` for any other cause (retries exhausted, a gate refusal).
    BlockedNode { node: String },
    /// The terminal `Failed` state.
    FailedNode { node: String },
    /// Parked until the OWNER answers: no dispatch can advance it, so the operator is the
    /// only way forward. Distinct from a wedge by construction (contested correction).
    WaitingInputNode { node: String },
    /// In flight and saying nothing for longer than its own declared deadline. A call
    /// cannot legitimately outlive its own timeout, so this is silence, not patience —
    /// and a node with no declared bound is never judged here (see
    /// [`Attention::silence_unevaluated`]).
    SilentNode { node: String },
    /// Status says `running` while NOTHING can advance — no node `Running`, `Queued`,
    /// `Ready`, or parked in a state that resumes itself. The exact shape the judge saw
    /// reported green.
    WedgedQuiescence,
    /// A consumption named an arming that was not the live one, so a lease was taken by a
    /// writer that is not this server.
    ///
    /// NAMED FOR THE REMEDY, not for the symptom. The fold calls the record a *mis-burn*
    /// (`wake_mis_burns`), and a reason spelled that way beside the wake surfaces would be
    /// read as "your wake is broken right now" — which it is not. Post-#74 the serve path
    /// filters a stale capture before it is ever appended, so this arm fires only for a
    /// consumption written by something else: a direct append, an older binary, or a future
    /// bug. The operator's action is *check who is writing to this store*, and the variant
    /// says so (design note #119).
    ///
    /// Carries the SESSION because that is the actionable identifier, and one reason per
    /// session because the record is `BTreeMap<session, WakeMisBurn>` with overwriting
    /// inserts: existence survives that shape, count and history do not.
    ForeignWakeConsumption { session: String },
}

/// A node state that still moves without the operator: either a dispatcher can pick it up
/// now, or the condition it waits on clears on its own.
///
/// DELIBERATELY STATE-ONLY, and after #80 that is a narrower claim than it used to be. The
/// driver's gate (`graphhelm_execution::dispatch_candidates`) can leave a `Queued` node
/// undispatchable, so "a dispatcher can pick it up now" is true of the STATE and not always of
/// the node. Making this edge-aware was considered and deferred, because the case where the
/// difference goes unheard is unreachable today: this predicate only decides an outcome through
/// `WedgedQuiescence`, which is already suppressed by `reasons.is_empty()` below, and every
/// reachable predecessor that gates a dependent raises a reason of its own — `Failed` speaks,
/// `Blocked` speaks, `WaitingInput` speaks, `cancel` sweeps every non-terminal node in the same
/// pass, and `Invalidated` has no production emitter at all. See #95 for the
/// full clause design and the trigger that revives it.
const fn advances_without_the_operator(state: NodeState) -> bool {
    matches!(
        state,
        // #92: `Ready` is NOT here. The name of this predicate is the argument: in this system no
        // dispatcher runs on its own -- `drive_to_quiescence` is reached from `start` and `resume`
        // and from nowhere else -- so a node left `Ready` when the report is written does not
        // advance without the operator; it waits for one to run `resume`. Counting it as advancing
        // is what let `approve` report `can_sleep` over a node nothing would ever pick up.
        //
        // `Queued` and `Running` stay: attention is computed AFTER the drive, so a node still
        // `Running` there is genuinely in flight, and `Queued` with attempts is already judged as
        // silence by `has_judgeable_silence`.
        NodeState::Queued | NodeState::Running
            // Resumes itself when quota returns (§12 park-and-wait).
            | NodeState::WaitingCapacity
            // Pre-dispatch states the driver approves on its own next pass.
            | NodeState::Draft
            | NodeState::Linting
    )
}

/// Whether a node's quiet is SILENCE — something that can be judged against a bound — or
/// merely the absence of a turn.
///
/// This was `state == Running` and nothing else, by a filter nobody could trace to a
/// decision. The blind judge measured the cost: `flaky_check` failed, was requeued, and sat
/// in `Queued` for over two minutes reading as healthy — and declaring a budget for it did
/// not help, because the node never entered the question at all.
///
/// The property is NOT "did it fail". It is **has anyone got to this node yet**. Keying on a
/// retryable failure would name one cause of that and miss the others: `Invalidated` returns
/// a COMPLETED node to the queue with no failure anywhere in its history, and a
/// failure-keyed rule would leave exactly that node mute forever. `attempts` is
/// `node_attempts`, which the fold increments only on an entry into `Running`, so the
/// question is asked of the record that already answers it and needs no maintenance when a
/// new outcome is added.
///
/// Every state is classified here and the excluded ones say WHY — a bare list is how the
/// original `== Running` became untraceable. There is no catch-all arm, so a future state
/// cannot be silently unclassified: the COMPILER refuses it. Verified rather than asserted —
/// deleting the `Paused` arm gives `error[E0004]: non-exhaustive patterns`. What the compiler
/// does NOT check is whether a written reason is true; that stays prose, and prose has no
/// guard.
pub(crate) const fn has_judgeable_silence(state: NodeState, attempts: u32) -> bool {
    match state {
        // In flight by definition: quiet here is either work or a hang, and telling those
        // apart is the whole question.
        NodeState::Running => true,
        // Dispatched at least once and back in the queue — by retry, by invalidation, or by
        // anything later that returns a node the driver already reached. Nothing is moving
        // it, and the operator declared a bound expecting otherwise.
        NodeState::Queued => attempts > 0,
        // EXCLUDED, each for its own reason:
        //
        // Never dispatched: quiet is the absence of a turn, not silence. Judging these would
        // name every node of a freshly started graph, and a confident false alarm is how a
        // rule dies — this milestone watched one accuse a clean branch by name.
        NodeState::Draft | NodeState::Ghost | NodeState::Linting | NodeState::Ready => false,
        // Waiting on a named party, so the quiet has a known owner and an existing reason of
        // its own (`WaitingInputNode`); silence would say the same thing twice.
        NodeState::WaitingInput => false,
        // Parked by a declared park-and-wait rule (M05) with its own resumption condition.
        // Excluded in THIS milestone, not forever: quota that never returns is silence, and
        // saying so here is what stops this exclusion from becoming the next untraceable
        // filter.
        NodeState::WaitingCapacity => false,
        // The operator stopped it. Silence after an explicit pause is the thing asked for.
        NodeState::Paused => false,
        // Already named by a reason of its own (`BlockedNode`, `UntriagedInterruption`,
        // `FailedNode`), so silence would be noise on top of an answer.
        NodeState::Blocked | NodeState::Failed => false,
        // Terminated. A finished node has no silence to judge, and listing it is the noise
        // that kills an honest field.
        NodeState::Succeeded
        | NodeState::Waived
        | NodeState::Skipped
        | NodeState::Cancelled
        | NodeState::Invalidated => false,
    }
}

fn attempts_of(projection: &ExecutionProjection, node: &str) -> u32 {
    projection.node_attempts.get(node).copied().unwrap_or(0)
}

/// Decides the sleep question over a projection. `required` is `!reasons.is_empty()` —
/// derived, never declared, so the field and its justification cannot drift apart.
///
/// Reasons are deterministic: variant order first, node id within a variant (the
/// projection's maps are already ordered).
#[must_use]
pub fn attention(projection: &ExecutionProjection, inputs: &AttentionInputs) -> Attention {
    let mut untriaged = Vec::new();
    let mut silent = Vec::new();
    let mut unevaluated = Vec::new();
    let mut blocked = Vec::new();
    let mut failed = Vec::new();
    let mut waiting_input = Vec::new();

    for (node, state) in &projection.node_states {
        match state {
            NodeState::Blocked => {
                if projection.last_outcome.get(node) == Some(&NodeOutcome::Interrupted) {
                    untriaged.push(AttentionReason::UntriagedInterruption { node: node.clone() });
                } else {
                    blocked.push(AttentionReason::BlockedNode { node: node.clone() });
                }
            }
            NodeState::Failed => failed.push(AttentionReason::FailedNode { node: node.clone() }),
            NodeState::WaitingInput => {
                waiting_input.push(AttentionReason::WaitingInputNode { node: node.clone() });
            }
            // Silence only means something for work IN FLIGHT: a terminated node has no
            // silence to judge, and listing it would be the noise that kills an honest
            // field. The node's TYPE comes from the published topology — with no graph
            // there is no type, therefore no applicable budget, therefore the node is
            // unevaluated rather than assumed fine (the same posture the wedge keeps when
            // nothing is published).
            state if has_judgeable_silence(*state, attempts_of(projection, node)) => {
                let budget = inputs.silence_budget_seconds.get(node);
                match (budget, inputs.node_silence_seconds.get(node)) {
                    (Some(budget), Some(age)) if age > budget => {
                        silent.push(AttentionReason::SilentNode { node: node.clone() });
                    }
                    (Some(_), Some(_)) => {}
                    // Either half missing is an UNKNOWN, never a clean bill of health — and
                    // WHICH half is missing is the operator's remedy, so it is named.
                    (None, measured) => unevaluated.push(Unevaluated::Node {
                        node: node.clone(),
                        reason: NodeUnevaluated::NoDeclaredBudget,
                        remedy: Remedy::DeclareNodeBudget {
                            node: node.clone(),
                            // Only what the seam already saw. Zero when nothing was measured
                            // is not a guess about the node: it is the age it can report.
                            observed_silence_seconds: measured.copied().unwrap_or(0),
                            computed_at_sequence: inputs.at_sequence,
                        },
                    }),
                    (Some(_), None) => unevaluated.push(Unevaluated::Node {
                        node: node.clone(),
                        reason: NodeUnevaluated::NotMeasured,
                        remedy: Remedy::Unavailable {
                            because: RemedyUnavailable::SurfaceMeasuredNoAge,
                        },
                    }),
                }
            }
            _ => {}
        }
    }

    let mut reasons = untriaged;
    reasons.append(&mut blocked);
    reasons.append(&mut failed);
    reasons.append(&mut waiting_input);
    reasons.append(&mut silent);

    // CAPTURED HERE, BEFORE ANYTHING ELSE IS PUSHED, and the wedge below is decided against THIS
    // rather than against `reasons` as a whole (#119, second pass by ISSUES 3).
    //
    // The five reasons above all answer the SAME question the wedge answers -- why is nothing
    // advancing? -- so suppressing the wedge when one of them fired is coherent: the wedge is the
    // last-resort explanation, and a run that already has one does not need it.
    //
    // `ForeignWakeConsumption` answers a DIFFERENT question. Its own doc says the operator's action
    // is to check who is writing to this store; provenance of writes is orthogonal to whether this
    // run is stuck. A store can be both wedged AND written by a foreign writer, and testing
    // `reasons.is_empty()` after the mis-burn push would tell the operator only the second.
    //
    // The consequence was permanent, not transient: `wake_mis_burns` is only ever inserted into --
    // one write site, `projection.rs:1926`, and no `remove`, `clear`, `retain` or `drain` anywhere
    // in the workspace -- so the first mis-burn a store ever recorded would have switched off its
    // wedge detector for the life of that store.
    let nothing_explains_non_advancement = reasons.is_empty();

    // #119: the fold records a consumption that burned an arming other than the one it captured,
    // and until now nothing read it. EXISTENCE, not history: `wake_mis_burns` is keyed by session
    // and its inserts OVERWRITE, so the map can answer "has this session ever mis-burned" and
    // never "how many times". One reason per session is therefore the finest grain the record can
    // back, and the pair inside the entry is deliberately not spent here — an operator who needs
    // the sequences reads the journal, which loses nothing.
    //
    // Read from the PROJECTION rather than re-derived from events, and that is the point: the
    // fold's own note says the record lives in the projection precisely because the verdict is
    // computed from the projection through one predicate, and a discrepancy visible only in raw
    // events is one this surface structurally cannot see.
    //
    // BTreeMap iterates in key order, so the reasons are ordered by session id without sorting.
    for session in projection.wake_mis_burns.keys() {
        reasons.push(AttentionReason::ForeignWakeConsumption {
            session: session.clone(),
        });
    }

    // The wedge: the aggregate claims it is running while nothing left can move it. A node
    // already named above is a reason of its own; the wedge is the case where the story
    // looks alive and is not.
    //
    // `node_states` holds ONLY nodes that already changed state, so an untouched node is
    // ABSENT — and absence means "still to be dispatched" (the driver reads it exactly that
    // way: `.get(node).copied().unwrap_or(NodeState::Draft)`). Deciding the wedge from that
    // map alone would infer a verdict from silence, the same defect F2 kills in this very
    // delivery. The published topology is the completeness check: a topology node with no
    // recorded state is queued work. With no graph published there is no basis to claim a
    // wedge at all, so none is claimed — under-answering beats a false alarm, because a
    // field that cries wolf on a healthy execution stops being read.
    // A REAL execution never emits `simulation_started`, so `simulation_status` stays
    // `None` for the whole run and only becomes `Some(..)` when `execution_completed`
    // folds. Demanding `Some(Running)` here made the wedge rule dead code in production:
    // it could only fire for simulation-driven stories, never for the live run an operator
    // actually watches. The blind judge's re-judgement (M07 Task 6, the closing rule)
    // caught this after both agents shipped it — a null status on a STARTED execution
    // means running, not "no opinion".
    let claims_running = match projection.simulation_status {
        // The simulation path says so outright.
        Some(SimulationStatus::Running) => true,
        // The execution path never says it: a started execution with no recorded status is
        // running by definition, because only `execution_completed` writes one.
        None => projection.execution_id.is_some(),
        // Every other status is a finished or held story, never a wedge.
        Some(_) => false,
    };
    let anything_advances = projection
        .node_states
        .values()
        .any(|state| advances_without_the_operator(*state));
    // The node set the run was supposed to cover, from the STRONGEST record available.
    //
    // Precedence is declared, not incidental (reviewer's S1): a SEALED, published graph
    // outranks a merely DECLARED form, because it is the stronger evidence about the same
    // fact. The declared form is the fallback, and it exists at all because this rule was
    // structurally DEAD until now -- `current_graph` is `None` on every path anyone uses, so
    // a wedged execution could never be reported as wedged. One missing record, two mute
    // rules; the other one was the silence budget.
    let complete_node_set: Option<Vec<String>> = projection
        .current_graph
        .as_ref()
        .map(|graph| {
            graph
                .topology()
                .nodes()
                .keys()
                .map(|node| node.as_str().to_owned())
                .collect()
        })
        .or_else(|| {
            projection.declared_form.as_ref().map(|form| {
                form.node_ids
                    .iter()
                    .map(|node| node.as_str().to_owned())
                    .collect()
            })
        });
    let untouched_topology_work = complete_node_set.as_ref().is_some_and(|nodes| {
        nodes
            .iter()
            .any(|node| !projection.node_states.contains_key(node.as_str()))
    });
    let graph_defines_completeness = complete_node_set.is_some();
    if claims_running && !anything_advances && nothing_explains_non_advancement {
        if graph_defines_completeness {
            if !untouched_topology_work {
                reasons.push(AttentionReason::WedgedQuiescence);
            }
        } else {
            // S4: nothing recorded the node set this run was supposed to cover, so nobody
            // here can say it finished. This used to fall through to CanSleep -- the
            // milestone's own defect one last time, refusing to fabricate an alarm and
            // quietly asserting calm instead.
            unevaluated.push(Unevaluated::Execution {
                reason: ExecutionUnevaluated::NoRecordedNodeSet,
                remedy: Remedy::Unavailable {
                    because: RemedyUnavailable::AmendmentDeclaresBoundsNotShape,
                },
            });
        }
    }

    // Order matters and is the whole rule: a named reason outranks an unknown, and an
    // unknown outranks calm. Calm is the only answer that requires having actually looked --
    // and the ONLY one that may carry nothing, which the types now enforce rather than hope.
    let verdict = match NonEmpty::new(reasons) {
        Some(reasons) => Verdict::NeedsYou {
            reasons,
            unevaluated,
        },
        None => match NonEmpty::new(unevaluated) {
            Some(unevaluated) => Verdict::Unknown { unevaluated },
            // Ranked between Unknown and CanSleep: it IS calm, and it is a calm the operator
            // bought. Saying so is not refusing their declaration.
            None => NonEmpty::new(purchased_calm(projection, inputs))
                .map_or(Verdict::CanSleep, |nodes| Verdict::CalmedByAmendment {
                    nodes,
                }),
        },
    };

    Attention { verdict }
}
