//! A wall-clock bound on ONE read, checked DURING the walk.
//!
//! `MAX_READ_ALL` bounds how MANY events a read will walk. Nothing bounded how LONG it takes,
//! and the two are not the same claim: a limit that is declared but never enforced is a claim,
//! not a boundary (#750). A caller that trips the linear walk got no signal that it was about
//! to wait, and a wait long enough reads as a hang rather than as a slow answer.
//!
//! Two properties this type exists to hold, both learned from defects already on the record:
//!
//! - **The check happens INSIDE the walk, never before it.** A budget read once before the
//!   block it is supposed to bound bounds the START of that block and nothing else (#743's
//!   shape). `check` is called with the running count as the walk proceeds.
//! - **The decision sits behind a seam that accepts values.** The deadline is computed from an
//!   injected [`Clock`], so a cell can drive expiry by supplying a clock that reports a later
//!   instant - no sleeping, which would be a permanent tax on every gate run.
//!
//! An unbounded budget is the default everywhere: every existing entry point keeps its exact
//! behaviour, and only a caller that asks for a bound gets one.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use graphhelm_protocols::Clock;

/// Events walked between two clock reads.
///
/// The check itself is not free (a clock read per event would be its own linear cost), and it
/// does not need to be exact: what a caller needs is to be told within a bounded overshoot,
/// not at the precise millisecond. At the measured ~0.105 ms/event of a shipped-profile status
/// read, 256 events is roughly 27 ms of overshoot; on the ~1.9 ms/event of an unoptimised
/// build it is roughly half a second. Both are small against any budget worth declaring.
pub const READ_BUDGET_CHECK_INTERVAL: u64 = 256;

/// A deadline for one read, or no deadline at all.
#[derive(Clone)]
pub struct ReadBudget {
    bound: Option<Bound>,
}

#[derive(Clone)]
struct Bound {
    clock: Arc<dyn Clock>,
    deadline: DateTime<Utc>,
    limit_millis: i64,
}

/// What a walk that ran out of budget reports.
///
/// Both numbers are carried rather than a bare error: a caller told only "too slow" cannot tell
/// a store it should stop asking about from a budget that is set too low, and the operator
/// reading the diagnostic needs to know which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadBudgetExceeded {
    /// Events walked when the budget ran out. A floor: the walk is checked every
    /// [`READ_BUDGET_CHECK_INTERVAL`] events, so the true count is within one interval above.
    pub walked: u64,
    /// The budget that was declared, in milliseconds.
    pub limit_millis: i64,
}

impl ReadBudget {
    /// No deadline. The behaviour every caller had before a budget existed.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self { bound: None }
    }

    /// A deadline `limit` after the clock's current instant.
    ///
    /// The clock is the caller's, so the same seam a repository already carries for its
    /// timestamps decides expiry here, and a test drives it without waiting.
    #[must_use]
    pub fn starting_now(clock: Arc<dyn Clock>, limit: chrono::Duration) -> Self {
        let deadline = clock.now() + limit;
        Self {
            bound: Some(Bound {
                clock,
                deadline,
                limit_millis: limit.num_milliseconds(),
            }),
        }
    }

    /// Whether this budget can ever refuse. Callers use it to keep an unbounded walk on the
    /// exact code path it had before, rather than paying a branch that can never fire.
    #[must_use]
    pub const fn is_bounded(&self) -> bool {
        self.bound.is_some()
    }

    /// Called after each unit of work with the event count BEFORE and AFTER that unit. Reads
    /// the clock only when the pair straddles an interval boundary.
    ///
    /// **The two-argument shape is the point.** One caller walks one event at a time (the
    /// fold); the other walks a whole batch line at a time (the journal verification), and a
    /// batch can carry many events. A rule written as `count % INTERVAL == 0` is exact for the
    /// first and can be skipped forever by the second - a batch of 50 events steps
    /// 50, 100, ..., 250, 300 and never lands on 256, so the budget would never be read at all.
    /// Asking whether the SPAN crossed a boundary is correct for both, and keeps the overshoot
    /// bounded by one interval plus one unit in each.
    ///
    /// A span ending at 0 never refuses: a budget that can refuse before the walk has done
    /// anything would turn "this store is too big to read in time" into "this store cannot be
    /// read", a different and untrue claim.
    pub fn check_progress(&self, before: u64, after: u64) -> Result<(), ReadBudgetExceeded> {
        let Some(bound) = &self.bound else {
            return Ok(());
        };
        if after == 0 || before / READ_BUDGET_CHECK_INTERVAL == after / READ_BUDGET_CHECK_INTERVAL {
            return Ok(());
        }
        if bound.clock.now() <= bound.deadline {
            return Ok(());
        }
        Err(ReadBudgetExceeded {
            walked: after,
            limit_millis: bound.limit_millis,
        })
    }
}

impl ReadBudget {
    /// Reads the clock NOW, regardless of progress. For work that follows the fold and is not
    /// counted in events -- a derivation over the active graph after `replay_within` made its
    /// last interval check (#134): a status that finished its fold just inside the deadline and
    /// then spent the rest of it rendering must still say so, or the advertised bound is only a
    /// bound on the fold. `walked` is carried for the same reason `check_progress` carries it: the
    /// operator reading the diagnostic needs the size beside the limit.
    pub fn check_now(&self, walked: u64) -> Result<(), ReadBudgetExceeded> {
        let Some(bound) = &self.bound else {
            return Ok(());
        };
        if bound.clock.now() <= bound.deadline {
            return Ok(());
        }
        Err(ReadBudgetExceeded {
            walked,
            limit_millis: bound.limit_millis,
        })
    }
}

impl std::fmt::Debug for ReadBudget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.bound {
            None => formatter.write_str("ReadBudget::unbounded"),
            Some(bound) => formatter
                .debug_struct("ReadBudget")
                .field("limit_millis", &bound.limit_millis)
                .field("deadline", &bound.deadline)
                .finish(),
        }
    }
}

impl Default for ReadBudget {
    fn default() -> Self {
        Self::unbounded()
    }
}
