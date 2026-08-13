//! Bounded concurrency as a pure decision.
//!
//! `ready_set` says what may run; this says what runs *now*, given how much is already in flight.
//! Selection order is the `BTreeSet`'s, so the same inputs always dispatch the identical prefix —
//! a replay must not dispatch a different subset than the original run.

use std::collections::BTreeSet;

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
    in_flight: usize,
    max_parallel: usize,
) -> Result<Vec<String>, DispatchError> {
    if max_parallel == 0 {
        return Err(DispatchError::ZeroParallelism);
    }
    let capacity = max_parallel.saturating_sub(in_flight);
    Ok(ready.iter().take(capacity).cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ready(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// At most `limit - in_flight` nodes are dispatched, in deterministic BTreeSet order, so a
    /// replay dispatches the identical prefix.
    #[test]
    fn dispatch_respects_the_parallel_limit_deterministically() {
        let plan = dispatch_plan(&ready(&["c", "a", "b"]), 1, 3).unwrap();
        assert_eq!(plan, ["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn a_saturated_execution_dispatches_nothing() {
        assert!(dispatch_plan(&ready(&["a"]), 3, 3).unwrap().is_empty());
        assert!(dispatch_plan(&ready(&["a"]), 5, 3).unwrap().is_empty());
    }

    /// A zero limit would mean an execution that can never progress: that is a graph authoring
    /// error surfaced loudly, not an empty plan returned silently forever.
    #[test]
    fn a_zero_limit_is_an_error_not_a_silent_stall() {
        assert_eq!(
            dispatch_plan(&ready(&["a"]), 0, 0).unwrap_err(),
            DispatchError::ZeroParallelism
        );
    }

    #[test]
    fn an_empty_ready_set_is_an_empty_plan() {
        assert!(dispatch_plan(&BTreeSet::new(), 0, 3).unwrap().is_empty());
    }
}
