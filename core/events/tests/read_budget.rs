//! The status read's wall-clock bound (#750).
//!
//! `MAX_READ_ALL` bounds how many events a read walks; nothing bounded how long it took, and a
//! wait long enough reads as a hang rather than as a slow answer. These cells pin the bound
//! itself: that it fires INSIDE each of the two linear walks a status read performs, that it
//! does not fire when it has not expired, and that the interval rule works for a caller that
//! steps one event at a time and for one that steps a whole batch.
//!
//! Every cell drives expiry through an INJECTED clock. None of them sleeps: a cell that waits
//! out a real budget is a permanent tax on every gate run, and it would prove the same thing.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    EventRepositoryError, LocalEventRepository, PreparedAppend, READ_BUDGET_CHECK_INTERVAL,
    ReadBudget, ReplayError, replay, replay_within,
};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator, NewEvent,
    OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope,
    Sensitivity, SignalRecorded, SignalSeverity, SignalSourceKind, WireHash, WorkspaceId,
};

const START: (i32, u32, u32, u32, u32, u32) = (2026, 9, 3, 12, 0, 0);

/// Never moves. A budget built on it can never expire, which is what every cell that must NOT
/// refuse needs - and what makes a refusal in the cells below attributable to the clock rather
/// than to the history.
struct FrozenClock;
impl Clock for FrozenClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(START.0, START.1, START.2, START.3, START.4, START.5)
            .unwrap()
    }
}

/// Reports the start instant for the first `calm` reads and an hour later after that. The first
/// read is the one `ReadBudget::starting_now` spends computing the deadline, so `calm = 1` means
/// "the deadline is set, and every check inside the walk is already past it".
struct LapsingClock {
    reads: AtomicU64,
    calm: u64,
}
impl LapsingClock {
    fn new(calm: u64) -> Arc<Self> {
        Arc::new(Self {
            reads: AtomicU64::new(0),
            calm,
        })
    }
}
impl Clock for LapsingClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        let base = Utc
            .with_ymd_and_hms(START.0, START.1, START.2, START.3, START.4, START.5)
            .unwrap();
        if self.reads.fetch_add(1, Ordering::SeqCst) < self.calm {
            base
        } else {
            base + chrono::Duration::hours(1)
        }
    }
}

#[derive(Default)]
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-budget").unwrap()),
    )
}
fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-test").unwrap(),
    )
}
fn event(key: String, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(&key).unwrap(),
        actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}
fn root() -> NewEvent {
    event(
        "key-root".to_owned(),
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: OpaqueId::parse("execution-budget").unwrap(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
            mode: ExecutionMode::Supervised,
        }),
    )
}
fn signal(index: u64) -> NewEvent {
    event(
        format!("key-{index}"),
        EventKind::SignalRecorded(SignalRecorded {
            execution_id: OpaqueId::parse("execution-budget").unwrap(),
            signal_id: OpaqueId::parse(format!("signal-{index}")).unwrap(),
            source_kind: SignalSourceKind::Node,
            source_id: OpaqueId::parse("implementation").unwrap(),
            kind: "no_progress".to_owned(),
            severity: SignalSeverity::High,
            envelope_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
        }),
    )
}

/// A store holding `count` events, written in 50-event batches. Fifty is both realistic (#744
/// refuses a single batch far below `MAX_BATCH_EVENTS`) and the shape that matters here: a
/// stepping caller whose stride never lands on the interval exactly.
fn seeded(directory: &std::path::Path, count: u64) -> Vec<graphhelm_protocols::EventEnvelope> {
    let writer =
        LocalEventRepository::open(directory, Arc::new(FrozenClock), Arc::new(Ids::default()))
            .unwrap();
    let mut next = 1_u64;
    while next <= count {
        let size = 50.min(count - next + 1);
        let mut events = Vec::new();
        if next == 1 {
            events.push(root());
        }
        events.extend((next + events.len() as u64..next + size).map(signal));
        writer
            .append_atomic(
                &PreparedAppend::new(
                    scope(),
                    OpaqueId::parse("stream-budget").unwrap(),
                    next,
                    events,
                    vec![],
                    vec![],
                )
                .unwrap(),
            )
            .unwrap();
        next += size;
    }
    writer
        .read_replay_stream(&scope(), "stream-budget")
        .unwrap()
}

/// Longer than one check interval, so a walk over it must reach a boundary - and long enough
/// that the FIRST boundary-crossing 50-event batch (the span 250..300) is not the last batch
/// either. Both matter: a history of exactly 300 still refuses, but at its own end, so it
/// cannot tell "refused inside the walk" from "refused after paying for all of it".
const HISTORY: u64 = READ_BUDGET_CHECK_INTERVAL + 144;

#[test]
fn the_fold_refuses_when_the_budget_lapses_and_says_how_far_it_got() {
    let directory = tempfile::tempdir().unwrap();
    let history = seeded(directory.path(), HISTORY);
    let budget = ReadBudget::starting_now(LapsingClock::new(1), chrono::Duration::seconds(5));

    let refused = replay_within(&scope(), "stream-budget", &history, &budget).unwrap_err();

    match refused {
        ReplayError::BudgetExceeded {
            walked,
            limit_millis,
        } => {
            assert_eq!(
                limit_millis, 5_000,
                "the refusal names the budget it was given"
            );
            // Inside the walk, not before it and not after it: the fold refused having walked
            // a whole number of intervals, and fewer events than the history holds. A refusal
            // at 0 would be "this stream cannot be read"; one at the end would be a bound that
            // let the whole cost be paid first.
            assert_eq!(walked, READ_BUDGET_CHECK_INTERVAL);
            assert!(walked < history.len() as u64);
        }
        other => panic!("expected a budget refusal, got {other:?}"),
    }
    assert_eq!(refused.code(), "GHE013_READ_BUDGET_EXCEEDED");
}

/// The control the cell above needs: the SAME history and the same walk, refusing nothing when
/// the clock has not moved. Without it, a fold that refused every stream would pass.
#[test]
fn the_fold_answers_the_same_history_when_the_budget_has_not_lapsed() {
    let directory = tempfile::tempdir().unwrap();
    let history = seeded(directory.path(), HISTORY);

    let frozen = ReadBudget::starting_now(Arc::new(FrozenClock), chrono::Duration::seconds(5));
    let bounded = replay_within(&scope(), "stream-budget", &history, &frozen).unwrap();
    let unbounded = replay_within(
        &scope(),
        "stream-budget",
        &history,
        &ReadBudget::unbounded(),
    )
    .unwrap();
    let default_entry_point = replay(&scope(), "stream-budget", &history).unwrap();

    assert_eq!(u64::from(bounded.signals_recorded), HISTORY - 1);
    assert_eq!(u64::from(unbounded.signals_recorded), HISTORY - 1);
    // The unbounded entry point every other caller uses is unchanged by this work.
    assert_eq!(u64::from(default_entry_point.signals_recorded), HISTORY - 1);
}

/// The OTHER half. A bound that covers only the fold covers 42% of a status read's measured
/// cost while reading as a boundary (#750): the journal verification `open` performs is the
/// rest. This cell fails if that half is left unbounded.
#[test]
fn the_journal_verification_refuses_when_the_budget_lapses() {
    let directory = tempfile::tempdir().unwrap();
    let _ = seeded(directory.path(), HISTORY);

    let budget = ReadBudget::starting_now(LapsingClock::new(1), chrono::Duration::seconds(5));
    let opened = LocalEventRepository::open_within(
        directory.path(),
        Arc::new(FrozenClock),
        Arc::new(Ids::default()),
        budget,
    );
    // `LocalEventRepository` carries no `Debug`, so the Ok arm is named rather than unwrapped.
    let Err(refused) = opened else {
        panic!("a lapsed budget must refuse the open, not answer it");
    };

    match refused {
        EventRepositoryError::ReadBudgetExceeded {
            walked,
            limit_millis,
        } => {
            assert_eq!(limit_millis, 5_000);
            // The stride here is a 50-event batch, not one event: the refusal lands on the
            // first batch whose span crosses the interval, which is the first multiple of 50
            // at or above it.
            assert!(walked >= READ_BUDGET_CHECK_INTERVAL, "walked {walked}");
            assert!(walked < HISTORY, "walked {walked}");
        }
        other => panic!("expected a budget refusal, got {other:?}"),
    }
    assert_eq!(refused.code(), "GHE013_READ_BUDGET_EXCEEDED");
}

/// The control for the half above, and the one that proves the DEFAULT path did not change:
/// the same store opens when the budget has not lapsed, and through plain `open` always.
#[test]
fn the_journal_verification_opens_the_same_store_when_the_budget_holds() {
    let directory = tempfile::tempdir().unwrap();
    let _ = seeded(directory.path(), HISTORY);

    let frozen = ReadBudget::starting_now(Arc::new(FrozenClock), chrono::Duration::seconds(5));
    LocalEventRepository::open_within(
        directory.path(),
        Arc::new(FrozenClock),
        Arc::new(Ids::default()),
        frozen,
    )
    .expect("a budget that has not lapsed refuses nothing");
    LocalEventRepository::open_within(
        directory.path(),
        Arc::new(FrozenClock),
        Arc::new(Ids::default()),
        ReadBudget::unbounded(),
    )
    .expect("an unbounded budget refuses nothing");
    LocalEventRepository::open(
        directory.path(),
        Arc::new(FrozenClock),
        Arc::new(Ids::default()),
    )
    .expect("the entry point every other caller uses is unchanged");
}

/// The rule the two halves share, and the defect it was written against: a caller stepping 50
/// events at a time never lands on 256 exactly. Asked as "did this span cross a boundary" the
/// answer is right for both strides; asked as "is the count a multiple" the batch caller is
/// never checked at all, and the budget silently covers nothing.
#[test]
fn the_interval_fires_for_a_batch_stride_that_never_lands_on_it() {
    let budget = ReadBudget::starting_now(LapsingClock::new(1), chrono::Duration::seconds(5));
    let stride = 50_u64;
    let mut walked = 0_u64;
    let mut refusal = None;
    while walked < READ_BUDGET_CHECK_INTERVAL * 2 {
        let before = walked;
        walked += stride;
        assert_ne!(
            walked % READ_BUDGET_CHECK_INTERVAL,
            0,
            "the stride must never land on the interval, or this cell proves nothing"
        );
        if let Err(exceeded) = budget.check_progress(before, walked) {
            refusal = Some(exceeded);
            break;
        }
    }
    let exceeded = refusal.expect("a stride that steps over the boundary must still be checked");
    assert_eq!(
        exceeded.walked, 300,
        "the first span to cross 256 ends at 300"
    );
}

/// A budget refuses a walk that ran out of time; it never refuses one that has not begun.
/// "This store is too large to read within the budget" and "this store cannot be read" are
/// different claims, and only the first one is true.
#[test]
fn a_walk_that_has_not_started_is_never_refused() {
    let budget = ReadBudget::starting_now(LapsingClock::new(1), chrono::Duration::seconds(5));
    budget
        .check_progress(0, 0)
        .expect("nothing walked is nothing to refuse");
    assert!(
        budget
            .check_progress(0, READ_BUDGET_CHECK_INTERVAL)
            .is_err()
    );
}

/// An unbounded budget never reads its clock at all. Not a micro-optimisation: it is what
/// keeps every existing caller on the exact path it had before this work, and a clock read per
/// event would be its own linear cost added to the one being measured.
#[test]
fn an_unbounded_budget_never_reads_the_clock() {
    struct CountingClock(Arc<Mutex<u64>>);
    impl Clock for CountingClock {
        fn now(&self) -> chrono::DateTime<Utc> {
            *self.0.lock().unwrap() += 1;
            Utc.with_ymd_and_hms(START.0, START.1, START.2, START.3, START.4, START.5)
                .unwrap()
        }
    }
    let reads = Arc::new(Mutex::new(0_u64));
    let _counting = CountingClock(reads.clone());
    let unbounded = ReadBudget::unbounded();
    for step in 0..READ_BUDGET_CHECK_INTERVAL * 3 {
        unbounded.check_progress(step, step + 1).unwrap();
    }
    assert_eq!(*reads.lock().unwrap(), 0);
    assert!(!unbounded.is_bounded());
}
