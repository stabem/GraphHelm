//! Pure execution contracts for the Graph Engine.
//!
//! This crate is total and side-effect free: no clock, no randomness, no filesystem, no network and
//! no adapter dependency. Every rule is a function over values so that replaying the same inputs
//! reproduces the same decision exactly.
//!
//! One bounded exception: [`journeys::GitHistory`] runs `git` read-only (fixed arguments, no
//! pathspec) to answer whether a screen capture is still fresh. The fold itself stays pure and
//! reads history only through the [`journeys::ScopeHistory`] trait.

pub mod attention;
mod bounds;
mod briefing;
mod customs;
mod dispatch;
pub mod journeys;
mod progress;
mod ready;
mod recovery;
mod signal;
mod transition;

pub use attention::{
    Attention, AttentionInputs, AttentionReason, ExecutionUnevaluated, NodeUnevaluated, NonEmpty,
    PurchasedCalm, Remedy, RemedyUnavailable, Unevaluated, Verdict, attention, effective_budgets,
};
pub use bounds::{
    MAX_ACCEPTED_MUTATIONS, MAX_IDENTICAL_OUTCOMES, MAX_NODE_ATTEMPTS, MAX_READY_SET,
    MAX_SIGNALS_PER_EXECUTION,
};
pub use briefing::{
    AnswerRemedy, Briefing, Decision, DecisionKind, NextStep, WorkItem, briefing_view,
};
pub use customs::{
    CUSTOMS_DECLARATION_INVALID_CODE, CustomsView, NodeCustomsView, OpenClaimView,
    completion_is_gated, completion_is_gated_in, customs_view, unreadable_customs_nodes,
};
pub use dispatch::{DispatchError, dispatch_plan, parallel_limit};
pub use graphhelm_protocols::{NodeOutcome, SignalSeverity, SignalSourceKind};
pub use journeys::{
    ArrowState, ArrowView, CaptureRecord, CaptureView, ChangedSince, ContractInput, Freshness,
    GitHistory, JourneyView, JourneysView, ScopeHistory, ScreenInput, ScreenView, StepInput,
    StepAction, StepView, TransitionRecord, UnknownCause, Viewport, fold_journeys, valid_journey_id,
    valid_revision,
};
pub use progress::{Progress, classify_progress};
pub use ready::{
    DispatchUnavailable, DispatchView, ScheduleError, dispatch_candidates, dispatch_view,
    edges_satisfied, ready_set,
};
pub use recovery::{ResumeError, recovery_plan, resume_preconditions};
pub use signal::{SignalError, SignalKind, SignalSource, TypedSignal};
pub use transition::{
    ExecutionError, NodeExecutor, TransitionRequest, apply_transition, is_terminal,
};
