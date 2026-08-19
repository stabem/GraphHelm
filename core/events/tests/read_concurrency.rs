//! Reads must not serialize against each other.
//!
//! Measured before this guard existed: eight concurrent reads of one store took the same wall
//! time as eight sequential ones (18.0ms against 16.2ms). Every store operation -- read and
//! append alike -- passed through one exclusive file lock, so the API that exists to serve
//! several agents at once served them one at a time, and its latency grew linearly with the
//! number of agents.
//!
//! Reads are 89% of the store operations a mutation-heavy HTTP storm produces: even a mutation
//! request reads the sequence, the stream, the active version and the idempotency record, and
//! appends once in the middle. So this is not a niche path.
//!
//! Appends still serialize, and that is the store's correctness model rather than a defect: one
//! writer at a time is what makes the log a log. What this guard forbids is READERS waiting on
//! READERS.

use std::sync::Arc;
use std::time::{Duration, Instant};

use graphhelm_events::{LocalEventRepository, PreparedAppend};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, ForcedFreshReason, FreshnessClass, IdGenerator,
    NewEvent, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RepositoryScope,
    ReuseDecision, ReuseKeyComponent, ReuseOutcome, ReusePlane, Sensitivity, WireHash, WorkspaceId,
};

const READERS: usize = 8;

struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

struct Ids;
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-probe")
    }
}

fn open(root: &std::path::Path) -> LocalEventRepository {
    LocalEventRepository::open(root, Arc::new(SystemClock), Arc::new(Ids)).unwrap()
}

/// One reader's work: what every serve handler does -- open the store, ask it something, drop.
///
/// Each reader gets its OWN handle, because that is what serve does: it deliberately refuses to
/// cache a repository, so concurrency between requests is concurrency between handles.
fn one_read(root: &std::path::Path) {
    let repository = open(root);
    let _ = repository.read_unique_replay_stream().unwrap();
}

/// A store with enough history that READING it is the cost being measured.
///
/// The first version of this guard used an empty store, and it could not have measured what it
/// claimed: opening cost 1.5ms and the read itself 0.37ms, so four fifths of the time was
/// filesystem work the change does not touch, and the ratio never moved. A guard whose
/// workload is dominated by something the change cannot affect will report failure whatever
/// the change does -- the same defect as an assertion one level above its question, wearing a
/// stopwatch.
fn seed(root: &std::path::Path, events: usize) {
    let repository = open(root);
    let scope = RepositoryScope::new(
        WorkspaceId::parse("workspace-read").unwrap(),
        ProjectId::parse("project-read").unwrap(),
        Some(ExecutionId::parse("execution-read").unwrap()),
    );
    let stream = OpaqueId::parse("stream-read").unwrap();
    for index in 0..events {
        let next = repository.next_sequence(&scope, "stream-read").unwrap();
        let request = PreparedAppend::new(
            scope.clone(),
            stream.clone(),
            next,
            vec![NewEvent::new(
                OpaqueId::parse(format!("seed-{index}")).unwrap(),
                PersistedActor::new(
                    PersistedActorType::System,
                    ActorId::parse("system-read").unwrap(),
                ),
                Sensitivity::Internal,
                EventKind::ReuseDecision(ReuseDecision {
                    execution_id: OpaqueId::parse("execution-read").unwrap(),
                    node_id: None,
                    plane: ReusePlane::ToolBroker,
                    decision: ReuseOutcome::ForcedFresh,
                    forced_reason: Some(ForcedFreshReason::DirtyTree),
                    freshness_class: Some(FreshnessClass::SnapshotClosed),
                    key_components: vec![ReuseKeyComponent::ToolVersion],
                    key_digest: WireHash::parse(
                        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                    )
                    .unwrap(),
                    evidence_ref: None,
                    provenance_erased: false,
                }),
                vec![],
                vec![],
            )],
            vec![],
            vec![],
        )
        .unwrap();
        repository.append_atomic(&request).unwrap();
    }
}

#[test]
fn concurrent_reads_do_not_wait_for_each_other() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    seed(root, 60);
    one_read(root); // warm: the first open creates the layout, and that cost is not the subject

    let sequential = {
        let started = Instant::now();
        for _ in 0..READERS {
            one_read(root);
        }
        started.elapsed()
    };

    let concurrent = {
        let started = Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..READERS {
                scope.spawn(|| one_read(root));
            }
        });
        started.elapsed()
    };

    // Deliberately generous. The claim is not "N times faster" -- thread start-up, the
    // filesystem and the machine's own load all take their cut, and a tight ratio here would
    // be a flake wearing the clothes of a guarantee. The claim is that concurrency BUYS
    // SOMETHING, and under the old lock it bought nothing at all: 18.0ms against 16.2ms,
    // measured. Anything under three quarters of sequential is impossible when every reader
    // waits its turn.
    assert!(
        concurrent < sequential.mul_f64(0.75).max(Duration::from_millis(1)),
        "{READERS} concurrent reads took {concurrent:?} against {sequential:?} sequential: \
         readers are still waiting for readers"
    );
}
