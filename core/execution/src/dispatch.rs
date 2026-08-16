//! Bounded concurrency as a pure decision.
//!
//! `ready_set` says what may run; this says what runs *now*, given how much is already in flight.
//! Selection is attempt-fair and deterministic: fewer prior attempts dispatch first (the 04f
//! starvation finding — a persistently retrying, alphabetically earlier node must not starve a
//! sibling's first attempt), with lexicographic order only as the tiebreak within an attempt
//! count. Determinism is what replay needs from dispatch — the dispatch DECISION is never an
//! event input, so the policy may evolve, but the same inputs must always yield the same plan.

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
