//! `OwnerOutputValidator` (#221): validates and renders the final owner-facing response.
//!
//! THE CENTRAL INVARIANT (owner-output blueprint §3): no sequence
//! of valid inputs produces a `Result`-slot phrase that contradicts `result.status`. The phrase is
//! DERIVED from [`TaskOutcome`] by this module's own code — never accepted as caller-supplied
//! text — which is the same structural move #200's `status` tag made for the slot-lock read: the
//! fact travels in the value's TYPE, not in a convention a future edit can bypass.
//!
//! This validator makes a STATED failure unable to render as success. It cannot and does not make
//! `status` itself truthful — a swallowed upstream failure reporting `status: Success` is a valid
//! input this validator renders correctly (C's review on #221, `5392931308`, F1).

/// What actually happened, not what anyone wants to say happened. Closed: four states, no fifth
/// "success-ish" state a compression pass could reach for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskOutcome {
    Success,
    Failure,
    Refused,
    Uncertain,
}

/// A closed vocabulary of every named slot, so a style plan cannot smuggle a slot that doesn't
/// exist. `Summary`/`Result`/`YourAction` are structural; the rest are decision-conditional.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotId {
    Summary,
    Result,
    YourAction,
    OptionA,
    OptionAConsequence,
    OptionB,
    OptionBConsequence,
    Recommendation,
    RecommendationRationale,
    EvidenceLimitation,
    Rollback,
}

/// The typed input to this validator. Cardinality between `owner_action_required` and `decision`
/// is enforced by [`render`], not by this type alone (T3).
#[derive(Clone, Debug)]
pub struct OwnerTaskResult {
    pub summary: String,
    pub status: TaskOutcome,
    pub owner_action_required: bool,
    pub decision: Option<AdvisoryDecisionResult>,
    pub evidence_limitation: Option<String>,
    pub rollback: Option<String>,
    pub risk_flags: RiskFlags,
}

/// Which material-consequence categories this result carries. Any flag set here forces full
/// (never Terse-compressed) rendering of the content that flag protects — design §8.4's "safety
/// expansion". A plain struct of named bools, not a `Vec<RiskFlag>`: the design doc names these
/// six categories closed, and a struct makes "did I check all of them" a compiler question
/// (`..Default::default()` in a literal still requires every named field to exist).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RiskFlags {
    pub security: bool,
    pub destructive: bool,
    pub financial: bool,
    pub production: bool,
    pub privacy: bool,
    pub irreversible: bool,
}

impl RiskFlags {
    /// Whether ANY category is flagged — the trigger for overriding a Terse request.
    #[must_use]
    pub const fn any(&self) -> bool {
        self.security
            || self.destructive
            || self.financial
            || self.production
            || self.privacy
            || self.irreversible
    }
}

/// The stylist's entire vocabulary (design §4.3): allowed layout/style tokens and slot ordering
/// for MOVABLE slots only. No field here can hold a rendered value — slot-value injection is
/// structurally impossible, not policy-forbidden (T8).
#[derive(Clone, Debug)]
pub struct OwnerPresentationPlan {
    pub order: Vec<SlotId>,
    pub style: StyleToken,
}

/// Closed style vocabulary. `Terse` may compress non-safety-critical content; `Standard` never
/// compresses anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleToken {
    Terse,
    Standard,
}

/// A real decision's content. `options` is a fixed-size array, not `Vec<DecisionOption>` —
/// "exactly two options" becomes a type-level guarantee (zero or three options fails to
/// construct) instead of a runtime check a future edit could skip.
#[derive(Clone, Debug)]
pub struct AdvisoryDecisionResult {
    pub decision_statement: String,
    pub options: [DecisionOption; 2],
    pub recommended: RecommendedOption,
    pub rationale: String,
}

impl AdvisoryDecisionResult {
    /// The recommended option's own content — the only place `recommended` is turned into an
    /// index, so there is exactly one line in the whole module where that conversion can go
    /// wrong, and it cannot go wrong: `RecommendedOption` has two variants for a two-element
    /// array, matched exhaustively.
    #[must_use]
    fn recommended_option(&self) -> &DecisionOption {
        match self.recommended {
            RecommendedOption::First => &self.options[0],
            RecommendedOption::Second => &self.options[1],
        }
    }
}

/// Which of the two options is recommended. A two-case enum, not a `usize` — an out-of-range
/// index cannot be constructed at all, the same technique already used for `options:
/// [DecisionOption; 2]`. The field it replaces was documented "always 0 or 1 by construction",
/// which was never true of a `pub usize` with no validating constructor: `recommended: 2`
/// compiled and panicked inside the renderer whose declared purpose is that a stated failure
/// never renders as success — a panic is worse than that, it is not a rendering at all (L's
/// review on #254, `5395616664`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendedOption {
    First,
    Second,
}

/// One option's content: what it is, and what choosing it costs.
#[derive(Clone, Debug)]
pub struct DecisionOption {
    pub label: String,
    pub consequence: String,
}

/// One rendered slot: for now, only the text a slot carries. Grows into the full
/// `PresentationNode` shape (blueprint §1.3) as later tests demand `ExactValue`, `Option`,
/// `Recommendation` node kinds.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RenderedSlot {
    id: SlotId,
    text: String,
}

/// The closed AST's rendered form: an ordered set of slots, queryable by [`SlotId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerPresentation {
    slots: Vec<RenderedSlot>,
}

impl OwnerPresentation {
    /// The text of the slot named `id`, if this presentation carries one.
    #[must_use]
    pub fn text(&self, id: SlotId) -> Option<&str> {
        self.slots
            .iter()
            .find(|slot| slot.id == id)
            .map(|slot| slot.text.as_str())
    }

    /// How many slots named `id` this presentation carries.
    #[must_use]
    pub fn count(&self, id: SlotId) -> usize {
        self.slots.iter().filter(|slot| slot.id == id).count()
    }

    /// OWNER-FACING-BYTES-EMITTER (T10's grep marker, `core/runtime/tests/owner_output.rs`):
    /// the exclusive producer of owner-facing bytes. Joins slot text in order — the only place
    /// [`RenderedSlot::text`] becomes bytes a human reads.
    #[must_use]
    pub fn to_owner_bytes(&self) -> Vec<u8> {
        self.slots
            .iter()
            .map(|slot| slot.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes()
    }
}

/// Validates `result` and builds the closed [`OwnerPresentation`] AST. The ONLY place any slot's
/// text is assigned — mirrors `Read-SlotLockSnapshot` owning the only writes into a snapshot.
///
/// `plan` is optional stylist input (design §4.3) — `None` means the safe built-in plan (T7
/// gives this the "malformed plan is discarded" behavior; this signature already reflects that a
/// plan is advisory, never required).
///
/// # Errors
/// Not yet reachable from any sealed test; T3 gives this its first real refusal path.
pub fn render(
    result: &OwnerTaskResult,
    plan: Option<&OwnerPresentationPlan>,
) -> Result<OwnerPresentation, OwnerOutputError> {
    if result.owner_action_required != result.decision.is_some() {
        // A `[DecisionOption; 2]` fixes "two options" at the type level, but nothing stops
        // owner_action_required and decision.is_some() disagreeing - that mismatch is exactly
        // the "conditional-field... cardinality mismatch" the design's own §8.2 names.
        return Err(OwnerOutputError::SchemaInvalid);
    }

    // Defense in depth (design's own threat assessment: "the Runtime... scans before output"),
    // independent of whatever redacted upstream. Decided by the orchestrator: a hit refuses the
    // WHOLE response, never a partial render with the offending slot swapped out (T9, blueprint
    // §9 OQ1) - a silent partial lies about its own completeness the same way a wrong `present`
    // lied on #200.
    if contains_secret_shape(result) {
        return Err(OwnerOutputError::SchemaInvalid);
    }

    // A malformed plan is discarded WHOLESALE (design §2 step 5 / T7): its style request goes
    // with it, never applied selectively. Rendering still succeeds via the built-in default -
    // discarding the plan is not the same failure as a cardinality-mismatched RESULT above,
    // which refuses the whole call.
    let plan = plan.filter(|p| is_valid_plan(p));

    let mut slots = vec![
        RenderedSlot {
            id: SlotId::Summary,
            text: result.summary.clone(),
        },
        RenderedSlot {
            id: SlotId::Result,
            text: result_phrase(result.status).to_owned(),
        },
    ];

    if !result.owner_action_required {
        slots.push(RenderedSlot {
            id: SlotId::YourAction,
            text: "Nothing now".to_owned(),
        });
    }

    if let Some(decision) = &result.decision {
        let option_slots = [
            (SlotId::OptionA, SlotId::OptionAConsequence),
            (SlotId::OptionB, SlotId::OptionBConsequence),
        ];
        for (option, (label_slot, consequence_slot)) in decision.options.iter().zip(option_slots) {
            slots.push(RenderedSlot {
                id: label_slot,
                text: option.label.clone(),
            });
            slots.push(RenderedSlot {
                id: consequence_slot,
                text: option.consequence.clone(),
            });
        }
        slots.push(RenderedSlot {
            id: SlotId::Recommendation,
            text: decision.recommended_option().label.clone(),
        });
        slots.push(RenderedSlot {
            id: SlotId::RecommendationRationale,
            text: decision.rationale.clone(),
        });
    }

    // Safety expansion (design §8.4): a Terse request is honoured for these slots ONLY when
    // nothing is risk-flagged. Any flagged category forces this content through uncompressed —
    // the override is narrow (these slots, this call), never a wholesale plan discard, which is
    // what distinguishes it from the malformed-plan path (T7). Shared by evidence limitation and
    // rollback: both are named alongside material consequences in the issue's own acceptance
    // criteria as content that "cannot be compressed away or rewritten as success".
    let requested_terse = plan.is_some_and(|p| p.style == StyleToken::Terse);
    let survives_compression = !requested_terse || result.risk_flags.any();

    if survives_compression && let Some(evidence_limitation) = &result.evidence_limitation {
        slots.push(RenderedSlot {
            id: SlotId::EvidenceLimitation,
            text: evidence_limitation.clone(),
        });
    }

    if survives_compression && let Some(rollback) = &result.rollback {
        slots.push(RenderedSlot {
            id: SlotId::Rollback,
            text: rollback.clone(),
        });
    }

    Ok(OwnerPresentation { slots })
}

/// Defense-in-depth secret scan over every text field this result could put in front of an
/// owner — heuristic, not a complete detector (that's upstream redaction's job, per `result.
/// redaction` in the design's typed input). Known-shaped prefixes only; a token this narrow
/// misses more than it catches, which is the declared gap, not a hidden one.
fn contains_secret_shape(result: &OwnerTaskResult) -> bool {
    const SECRET_PREFIXES: [&str; 4] = ["AKIA", "sk-", "ghp_", "xox"];

    let has_secret_shape = |text: &str| SECRET_PREFIXES.iter().any(|prefix| text.contains(prefix));

    if has_secret_shape(&result.summary) {
        return true;
    }
    if let Some(evidence_limitation) = &result.evidence_limitation
        && has_secret_shape(evidence_limitation)
    {
        return true;
    }
    if let Some(rollback) = &result.rollback
        && has_secret_shape(rollback)
    {
        return true;
    }
    if let Some(decision) = &result.decision {
        if has_secret_shape(&decision.decision_statement) || has_secret_shape(&decision.rationale) {
            return true;
        }
        if decision
            .options
            .iter()
            .any(|option| has_secret_shape(&option.label) || has_secret_shape(&option.consequence))
        {
            return true;
        }
    }
    false
}

/// A plan is valid iff every slot it names in `order` is one of the MOVABLE slots — today, only
/// `YourAction` (design §1.2 / blueprint §1.2: structural and decision-conditional slots have a
/// fixed position). A plan naming any other slot is malformed and gets discarded whole (T7).
fn is_valid_plan(plan: &OwnerPresentationPlan) -> bool {
    const MOVABLE: [SlotId; 1] = [SlotId::YourAction];
    plan.order.iter().all(|slot| MOVABLE.contains(slot))
}

/// The `Result` slot's phrase, derived from [`TaskOutcome`] ALONE — no caller, decision, or
/// style plan can reach this function's return value. This is the central invariant (§3): a
/// stated `Failure`/`Refused`/`Uncertain` can never render as success, because the phrase is a
/// match on a closed enum, not text `render` accepts from anywhere.
#[must_use]
const fn result_phrase(status: TaskOutcome) -> &'static str {
    match status {
        TaskOutcome::Success => "Done.",
        TaskOutcome::Failure => "Failed.",
        TaskOutcome::Refused => "Refused.",
        TaskOutcome::Uncertain => "Could not determine the result.",
    }
}

/// Refusal codes this task owns. Not yet allocated in the shared
/// `graphhelm_protocols::DevelopmentRefusalCode` vocabulary — see
/// design note #221 and issue #217 comment `5394028501`. Placeholder
/// local type until that's resolved; T3 is the first test to construct one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerOutputError {
    SchemaInvalid,
}
