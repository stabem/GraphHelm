//! Bounded concurrency as a pure decision.
//!
//! `ready_set` says what may run; this says what runs *now*, given how much is already in flight.
//! Selection is attempt-fair and deterministic: fewer prior attempts dispatch first (the 04f
//! starvation finding — a persistently retrying, alphabetically earlier node must not starve a
//! sibling's first attempt), with lexicographic order only as the tiebreak within an attempt
//! count. Determinism is what replay needs from dispatch — the dispatch DECISION is never an
//! event input, so the policy may evolve, but the same inputs must always yield the same plan.

// Doc ATTACHMENT is otherwise unmeasured in this repo: #101 inserted a new function between
// `dispatch_plan`'s doc block and `dispatch_plan`, leaving one function undocumented and the other
// carrying a `# Errors` for an error it cannot return (#154). A green gate, a hand-verified review
// and a merge all passed over it, correctly — clippy runs the DEFAULT set, where this lint is
// pedantic and `missing_docs` is allow-by-default.
//
// This catches exactly ONE shape: a public `Result`-returning fn IN THIS MODULE losing its
// `# Errors`, which is the half that actually happened. It does NOT catch the mirror (a stray
// `# Errors` on a non-`Result` fn) and it does NOT generalise beyond this file. The repo-wide
// question — crate-level `missing_docs`, or `clippy::pedantic` — is deliberately NOT decided here.
#![warn(clippy::missing_errors_doc)]

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    /// `max_parallel` of zero means the execution can never progress. Surfacing it beats returning
    /// an empty plan forever, which would look exactly like a healthy idle execution.
    ZeroParallelism,
}

/// Selects which ready nodes to dispatch now.
///
/// `in_flight >= max_parallel` returns an empty plan by contract — the caller may legitimately be
/// saturated, and silence is the intended answer, not an error. The caller also owns converting
/// `GraphBudgets::max_parallel_model_calls` (`Option<u64>`) into `max_parallel`; this function
/// deliberately takes minimal `usize` inputs.
///
/// # Errors
/// `ZeroParallelism` when `max_parallel` is zero.
pub fn dispatch_plan(
    ready: &BTreeSet<String>,
    attempts: &BTreeMap<String, u32>,
    in_flight: usize,
    max_parallel: usize,
) -> Result<Vec<String>, DispatchError> {
    if max_parallel == 0 {
        return Err(DispatchError::ZeroParallelism);
    }
    let capacity = max_parallel.saturating_sub(in_flight);
    let mut ordered: Vec<&String> = ready.iter().collect();
    // Attempt-fair: fewer attempts first; the BTreeSet's lexicographic order is the tiebreak.
    // A node absent from the map has zero attempts — first attempts always lead.
    ordered.sort_by_key(|node| (attempts.get(*node).copied().unwrap_or(0), (*node).clone()));
    Ok(ordered.into_iter().take(capacity).cloned().collect())
}

/// Converts `GraphBudgets::max_parallel_model_calls` into the `max_parallel` [`dispatch_plan`]
/// wants.
///
/// SEPARATE FROM `dispatch_plan` ON PURPOSE. That function's own doc says the caller owns this
/// conversion and that it "deliberately takes minimal `usize` inputs" — a recorded decision, and
/// #101 does not overturn it. What #101 removes is the DUPLICATION: both drivers carried this
/// exact `match`, byte for byte, so the policy that an absent budget means SERIAL was written
/// twice and could drift once.
///
/// `None => 1` is that policy, not a default: a graph that declares no parallelism budget runs one
/// node at a time. Stated here so the next reader finds a reason rather than a literal.
///
/// A LIMIT, NOT A MECHANISM — and the two drivers do not agree on the mechanism. This answers
/// "how many may be in flight", never "does anything actually run at the same time".
/// `core/runtime/src/driver.rs` spawns the plan into a `JoinSet` and runs nodes concurrently;
/// `apps/cli/src/commands/execution/driver.rs` walks the plan in a `for` loop and blocks on each
/// executor call, so the same limit only widens how many nodes one SEQUENTIAL pass may cover.
/// Written here because #101's dedup removed the signal that used to carry it: two identical
/// copies accidentally marked "two drivers, check both", and one shared function reads as
/// unification. The policy is unified. The parallelism is not.
#[must_use]
pub fn parallel_limit(budgets: &graphhelm_protocols::GraphBudgets) -> usize {
    match budgets.max_parallel_model_calls {
        None => 1_usize,
        Some(value) => usize::try_from(value).unwrap_or(usize::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn ready(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// At most `limit - in_flight` nodes are dispatched, in deterministic BTreeSet order, so a
    /// replay dispatches the identical prefix.
    #[test]
    fn dispatch_respects_the_parallel_limit_deterministically() {
        let plan = dispatch_plan(&ready(&["c", "a", "b"]), &BTreeMap::new(), 1, 3).unwrap();
        assert_eq!(plan, ["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn a_saturated_execution_dispatches_nothing() {
        assert!(
            dispatch_plan(&ready(&["a"]), &BTreeMap::new(), 3, 3)
                .unwrap()
                .is_empty()
        );
        assert!(
            dispatch_plan(&ready(&["a"]), &BTreeMap::new(), 5, 3)
                .unwrap()
                .is_empty()
        );
    }

    /// The 04f finding closed: with max_parallel 1, a persistently retrying, alphabetically
    /// earlier node must not starve a sibling's FIRST attempt. Fewer attempts dispatch first;
    /// lexicographic order is only the tiebreak within an attempt count.
    #[test]
    fn a_retrying_node_cannot_starve_a_first_attempt() {
        let mut attempts = std::collections::BTreeMap::new();
        attempts.insert("aaa".to_owned(), 3_u32);
        let plan = dispatch_plan(&ready(&["aaa", "zzz"]), &attempts, 0, 1).unwrap();
        assert_eq!(
            plan,
            ["zzz".to_owned()],
            "the untried node dispatches first"
        );
    }

    /// Determinism is what replay needs from dispatch: the dispatch DECISION is never an event
    /// input — replay folds recorded outcomes — so changing the policy is legal; being
    /// nondeterministic is not. Same inputs, same plan, always.
    #[test]
    fn fairness_is_deterministic_and_replay_indifferent() {
        proptest::proptest!(|(names in proptest::collection::btree_set("[a-z]{1,6}", 0..12),
                              counts in proptest::collection::vec(0_u32..5, 0..12),
                              max_parallel in 1_usize..4)| {
            let attempts: std::collections::BTreeMap<String, u32> = names
                .iter()
                .cloned()
                .zip(counts.iter().copied())
                .collect();
            let first = dispatch_plan(&names, &attempts, 0, max_parallel).unwrap();
            let second = dispatch_plan(&names, &attempts, 0, max_parallel).unwrap();
            proptest::prop_assert_eq!(&first, &second);
            // And the plan is sorted by (attempts, name): no later element may have strictly
            // fewer attempts than an earlier one.
            for pair in first.windows(2) {
                let a = attempts.get(&pair[0]).copied().unwrap_or(0);
                let b = attempts.get(&pair[1]).copied().unwrap_or(0);
                proptest::prop_assert!(a < b || (a == b && pair[0] <= pair[1]));
            }
        });
    }

    /// A zero limit would mean an execution that can never progress: that is a graph authoring
    /// error surfaced loudly, not an empty plan returned silently forever.
    #[test]
    fn a_zero_limit_is_an_error_not_a_silent_stall() {
        assert_eq!(
            dispatch_plan(&ready(&["a"]), &BTreeMap::new(), 0, 0).unwrap_err(),
            DispatchError::ZeroParallelism
        );
    }

    #[test]
    fn an_empty_ready_set_is_an_empty_plan() {
        assert!(
            dispatch_plan(&BTreeSet::new(), &BTreeMap::new(), 0, 3)
                .unwrap()
                .is_empty()
        );
    }
}
