//! Bounds for execution progress.
//!
//! Every value here is a count of attempts or events, never a wall-clock duration. Replay must
//! reproduce the identical decision, and a timing-dependent bound cannot.

/// Attempts of one node before the execution blocks for an owner decision.
pub const MAX_NODE_ATTEMPTS: u32 = 8;

/// Consecutive semantically identical outcomes treated as no progress.
pub const MAX_IDENTICAL_OUTCOMES: u32 = 3;

/// Governor mutations one execution may accept before blocking for an owner decision.
pub const MAX_ACCEPTED_MUTATIONS: u32 = 64;

/// Nodes that may be ready at once. Exceeding this blocks rather than truncating.
pub const MAX_READY_SET: usize = 1024;

/// Signals one execution may record. Exceeding this blocks rather than dropping evidence.
pub const MAX_SIGNALS_PER_EXECUTION: u32 = 10_000;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bound is a count. A duration would make the same history replay to a different
    /// decision on a slower machine, which breaks the replay guarantee this milestone rests on.
    #[test]
    fn bounds_are_counts_and_are_ordered_sensibly() {
        assert_eq!(MAX_NODE_ATTEMPTS, 8);
        assert_eq!(MAX_IDENTICAL_OUTCOMES, 3);
        assert_eq!(MAX_ACCEPTED_MUTATIONS, 64);
        assert_eq!(MAX_READY_SET, 1024);
        assert_eq!(MAX_SIGNALS_PER_EXECUTION, 10_000);
        const { assert!(MAX_IDENTICAL_OUTCOMES < MAX_NODE_ATTEMPTS) };
    }
}
