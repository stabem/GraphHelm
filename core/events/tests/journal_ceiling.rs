//! The journal ceiling, `MAX_JOURNAL_BYTES`, is checked before every append and again when a store
//! is opened (#1714). The live store stopped accepting writes at 64 MiB; these cells pin that a
//! store grows past 64 MiB, reopens and reads back. That the new ceiling still refuses is pinned
//! beside the gate, in `local.rs` (`journal_room_*`), because staging a real 256 MiB journal costs
//! minutes and a padded one is refused as tampering before the gate is reached.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, MAX_JOURNAL_BYTES, PreparedAppend};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator, NewEvent,
    OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope,
    Sensitivity, SignalRecorded, SignalSeverity, SignalSourceKind, WireHash, WorkspaceId,
};

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap()
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
        Some(ExecutionId::parse("execution-test").unwrap()),
    )
}

fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-test").unwrap(),
    )
}

fn event(key: impl Into<String>, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key.into()).unwrap(),
        actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

/// One `ExecutionStarted` followed by `signals` `SignalRecorded` events, keyed by `prefix` so each
/// batch is a new append rather than an idempotent replay of an earlier one.
fn keyed_batch_of(prefix: &str, signals: usize) -> Vec<NewEvent> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    let mut events = vec![event(
        format!("{prefix}-execution-started"),
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: execution_id.clone(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Supervised,
        }),
    )];
    for index in 0..signals {
        events.push(event(
            format!("{prefix}-signal-{index}"),
            EventKind::SignalRecorded(SignalRecorded {
                to: None,
                reply_to: None,
                scoped_agent_authenticated: None,
                execution_id: execution_id.clone(),
                signal_id: OpaqueId::parse(format!("signal-{index}")).unwrap(),
                source_kind: SignalSourceKind::Node,
                source_id: OpaqueId::parse("node-a").unwrap(),
                kind: "k".repeat(SIGNAL_KIND_BYTES),
                severity: SignalSeverity::Medium,
                envelope_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
            }),
        ));
    }
    events
}

const OLD_CEILING: u64 = 64 * 1024 * 1024;
/// Few, large events: the open-time verification costs per event, so 64 MiB of 32 KiB signals is
/// about 2,000 events to verify instead of the ~80,000 small ones the live store holds. The batch
/// stays far below the validator's per-batch budget (see `batch_validation_bound.rs`).
const SIGNALS_PER_BATCH: usize = 8;
const SIGNAL_KIND_BYTES: usize = 32 * 1024;

fn open(directory: &std::path::Path) -> LocalEventRepository {
    LocalEventRepository::open(directory, Arc::new(FixedClock), Arc::new(Ids::default())).unwrap()
}

fn append_batch(repository: &LocalEventRepository, index: usize) {
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse(format!("stream-{index}")).unwrap(),
        1,
        keyed_batch_of(&format!("b{index}"), SIGNALS_PER_BATCH),
        vec![],
        vec![],
    )
    .unwrap();
    repository.append_atomic(&request).unwrap();
}

// Ignored by default: writing and re-verifying 64 MiB takes ~7 minutes in the test profile.
// Run it with `cargo +1.97.1 test --locked --release -p graphhelm-events --test journal_ceiling
// -- --ignored`. The default suite pins the gate itself in `local.rs` (`journal_room_*`).
#[test]
#[ignore = "writes 64 MiB; run with --release -- --ignored"]
fn a_journal_grows_past_64_mib_and_reopens() {
    let directory = tempfile::tempdir().unwrap();
    let journal = directory.path().join("journal.jsonl");
    let mut batches = 0;
    {
        let repository = open(directory.path());
        while std::fs::metadata(&journal).map_or(0, |m| m.len()) <= OLD_CEILING {
            append_batch(&repository, batches);
            batches += 1;
        }
        // One more append with the journal already past the old ceiling: the case #1714 refused.
        append_batch(&repository, batches);
        batches += 1;
    }
    let length = std::fs::metadata(&journal).unwrap().len();
    assert!(
        length > OLD_CEILING,
        "the journal never crossed 64 MiB: {length}"
    );
    assert!(length <= MAX_JOURNAL_BYTES);

    // The open-time checks follow the same ceiling: a store the append path accepted must reopen.
    let reader = open(directory.path());
    let last = format!("stream-{}", batches - 1);
    assert_eq!(
        reader.read_replay_stream(&scope(), &last).unwrap().len(),
        SIGNALS_PER_BATCH + 1
    );
}
