//! Deterministic policy evaluation.

pub mod adoption;
mod code_contract;
pub mod delegation;
mod evaluator;
pub mod keel;
pub mod keel_prove;

pub use code_contract::{
    CodeRuleSpec, Dominance, ResolutionRefusal, ResolvedCodeContract, RuleRecord, dominance,
    resolve_code_contract,
};
pub use evaluator::{evaluate_transition, validate_manual_override_limits};
