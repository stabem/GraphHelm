//! Pure execution contracts for the Graph Engine.
//!
//! This crate is total and side-effect free: no clock, no randomness, no filesystem, no network and
//! no adapter dependency. Every rule is a function over values so that replaying the same inputs
//! reproduces the same decision exactly.

mod bounds;
mod signal;
mod transition;

pub use bounds::{
    MAX_ACCEPTED_MUTATIONS, MAX_IDENTICAL_OUTCOMES, MAX_NODE_ATTEMPTS, MAX_READY_SET,
    MAX_SIGNALS_PER_EXECUTION,
};
pub use graphhelm_protocols::NodeOutcome;
pub use signal::{
    SignalError, SignalKind, SignalSeverity, SignalSource, SignalSourceKind, TypedSignal,
};
pub use transition::{ExecutionError, NodeExecutor, TransitionRequest, apply_transition};
