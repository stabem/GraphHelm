//! Deterministic policy evaluation.

mod code_contract;
mod evaluator;

pub use code_contract::{
    CodeRuleSpec, Dominance, ResolutionRefusal, ResolvedCodeContract, RuleRecord, dominance,
    resolve_code_contract,
};
pub use evaluator::evaluate_transition;
