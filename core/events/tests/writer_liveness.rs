//! Writers must make progress while readers overlap without a break (PR #1317 BLOCK).
//!
//! `read_concurrency.rs` forbids readers waiting on readers; `shared_lock_blocks_writers.rs`
//! forbids a writer getting IN while a reader holds the lock. Neither asked whether a writer
//! ever gets back in. Readers hold the store's locks SHARED, and neither `flock` nor
//! `LockFileEx` gives a waiting exclusive request priority, so readers that overlap without a
//! break used to keep a writer out for good. Measured at `8ed5284d` on Linux: 0 appends in
//! 300 s against a read storm, where the same load finished all its appends in 61 s when readers
//! still took the root lock exclusive. The fix is the writer-preference turnstile in `local.rs`.
//!
//! The readers are OTHER PROCESSES, because that is the deployment: several agents and the
//! Studio reading one store through their own handles. The child is this same test binary,
//! re-run with an environment variable that turns `reader_process_entry` from a no-op into a
//! read loop. Every wait is bounded: a starved writer turns this cell red in about
//! `APPEND_DEADLINE`, it does not hang the suite.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use graphhelm_events::{LocalEventRepository, PreparedAppend};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, ForcedFreshReason, FreshnessClass, IdGenerator,
    NewEvent, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RepositoryScope,
    ReuseDecision, ReuseKeyComponent, ReuseOutcome, ReusePlane, Sensitivity, WireHash, WorkspaceId,
};

const ROOT_VARIABLE: &str = "GRAPHHELM_LIVENESS_READER_ROOT";
const STOP_VARIABLE: &str = "GRAPHHELM_LIVENESS_READER_STOP";
const READY_VARIABLE: &str = "GRAPHHELM_LIVENESS_READER_READY";

const READER_PROCESSES: usize = 3;
const THREADS_PER_READER: usize = 4;
const APPENDS: usize = 10;
/// Generous for a debug build on a loaded machine: with the turnstile each append waits for
/// the reads already in flight and nothing else. Without it the measured answer was "never".
const APPEND_DEADLINE: Duration = Duration::from_secs(90);
/// A reader process that is never told to stop (the parent died) exits on its own.
const READER_SELF_LIMIT: Duration = Duration::from_secs(300);

struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

#[derive(Default)]
struct Ids(AtomicUsize);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!(
            "{prefix}-{}-{}",
            std::process::id(),
            self.0.fetch_add(1, Ordering::SeqCst)
        )
    }
}

fn open(root: &Path) -> LocalEventRepository {
    LocalEventRepository::open(root, Arc::new(SystemClock), Arc::new(Ids::default())).unwrap()
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-live").unwrap(),
        ProjectId::parse("project-live").unwrap(),
        Some(ExecutionId::parse("execution-live").unwrap()),
    )
}

/// One append the way `serve` makes it: a fresh handle, ask the next sequence, append.
fn append_one(root: &Path, label: &str) {
    let repository = open(root);
    let next = repository.next_sequence(&scope(), "stream-live").unwrap();
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-live").unwrap(),
        next,
        vec![NewEvent::new(
            OpaqueId::parse(label).unwrap(),
            PersistedActor::new(
                PersistedActorType::System,
                ActorId::parse("system-live").unwrap(),
            ),
            Sensitivity::Internal,
            EventKind::ReuseDecision(ReuseDecision {
                execution_id: OpaqueId::parse("execution-live").unwrap(),
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

/// One reader's work, as every serve read handler does it: open, read, drop.
fn one_read(root: &Path) {
    let repository = open(root);
    let _ = repository.read_unique_replay_stream().unwrap();
}

/// The child side. A no-op in a normal run; a read loop when the parent sets the variables.
#[test]
fn reader_process_entry() {
    let (Some(root), Some(stop), Some(ready)) = (
        std::env::var_os(ROOT_VARIABLE),
        std::env::var_os(STOP_VARIABLE),
        std::env::var_os(READY_VARIABLE),
    ) else {
        return;
    };
    let root = PathBuf::from(root);
    let stop = PathBuf::from(stop);
    let started = Instant::now();
    let reads = AtomicUsize::new(0);
    std::thread::scope(|threads| {
        for _ in 0..THREADS_PER_READER {
            threads.spawn(|| {
                while !stop.exists() && started.elapsed() < READER_SELF_LIMIT {
                    one_read(&root);
                    reads.fetch_add(1, Ordering::SeqCst);
                }
            });
        }
        // Ready only once reads are demonstrably happening, so the parent's writer starts
        // against a storm that is already running rather than against its start-up.
        while reads.load(Ordering::SeqCst) < THREADS_PER_READER && !stop.exists() {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::write(&ready, b"ready").unwrap();
    });
    println!("liveness-reads={}", reads.load(Ordering::SeqCst));
}

fn spawn_reader(root: &Path, stop: &Path, ready: &Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "reader_process_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROOT_VARIABLE, root)
        .env(STOP_VARIABLE, stop)
        .env(READY_VARIABLE, ready)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Waits for a child with a bound and kills it past the bound, returning its stdout.
fn finish(mut child: Child, bound: Duration) -> (bool, String) {
    let started = Instant::now();
    let exited_cleanly = loop {
        match child.try_wait().unwrap() {
            Some(status) => break status.success(),
            None if started.elapsed() > bound => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut pipe, &mut stdout);
    }
    (exited_cleanly, stdout)
}

#[test]
fn writers_make_progress_while_readers_overlap_without_a_break() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("store");
    std::fs::create_dir(&root).unwrap();
    // Enough history that a read is real work, so reads genuinely overlap.
    for index in 0..60 {
        append_one(&root, &format!("seed-{index}"));
    }
    let stop = directory.path().join("stop");
    let readies = (0..READER_PROCESSES)
        .map(|index| directory.path().join(format!("ready-{index}")))
        .collect::<Vec<_>>();
    let children = readies
        .iter()
        .map(|ready| spawn_reader(&root, &stop, ready))
        .collect::<Vec<_>>();

    let ready_by = Instant::now() + Duration::from_secs(120);
    while !readies.iter().all(|ready| ready.exists()) && Instant::now() < ready_by {
        std::thread::sleep(Duration::from_millis(20));
    }
    let storm_running = readies.iter().all(|ready| ready.exists());

    // The writer runs on its own thread so the deadline below is the parent's, not the lock's:
    // a starved `append_atomic` never returns, and the cell must still report.
    let appended = Arc::new(AtomicUsize::new(0));
    let writer_done = Arc::new(AtomicBool::new(false));
    let writer = {
        let root = root.clone();
        let appended = Arc::clone(&appended);
        let writer_done = Arc::clone(&writer_done);
        std::thread::spawn(move || {
            for index in 0..APPENDS {
                append_one(&root, &format!("live-{index}"));
                appended.fetch_add(1, Ordering::SeqCst);
            }
            writer_done.store(true, Ordering::SeqCst);
        })
    };
    let writing_started = Instant::now();
    while storm_running
        && !writer_done.load(Ordering::SeqCst)
        && writing_started.elapsed() < APPEND_DEADLINE
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let appended_under_storm = appended.load(Ordering::SeqCst);
    let writing_took = writing_started.elapsed();

    // Stop the storm BEFORE joining the writer: a starved writer is released by the storm
    // ending, so the join is bounded either way.
    std::fs::write(&stop, b"stop").unwrap();
    let outcomes = children
        .into_iter()
        .map(|child| finish(child, Duration::from_secs(120)))
        .collect::<Vec<_>>();
    writer.join().unwrap();

    assert!(
        storm_running,
        "ARRANGEMENT: the reader processes never reported a read, so no storm ran: {outcomes:?}"
    );
    let reads = outcomes
        .iter()
        .map(|(clean, stdout)| {
            assert!(
                *clean,
                "a reader process failed or had to be killed: {stdout}"
            );
            stdout
                .lines()
                // libtest prints `test <name> ... ` before the child's own output, on one line.
                .find_map(|line| line.split_once("liveness-reads=").map(|(_, count)| count))
                .and_then(|count| count.split_whitespace().next()?.parse::<usize>().ok())
                .unwrap_or_else(|| panic!("a reader process printed no count: {stdout}"))
        })
        .collect::<Vec<_>>();
    assert!(
        appended_under_storm == APPENDS,
        "only {appended_under_storm} of {APPENDS} appends completed in {writing_took:?} while \
         {READER_PROCESSES} reader processes x {THREADS_PER_READER} threads read without a \
         break (reads per process: {reads:?}): writers are starved by overlapping readers"
    );
    // The storm really overlapped the writes: every reader process kept reading throughout.
    assert!(
        reads.iter().all(|&count| count > THREADS_PER_READER),
        "ARRANGEMENT: a reader process barely read ({reads:?}), so the appends were not \
         measured against overlapping readers"
    );
    // And every append landed exactly once.
    let final_count = open(&root).read_unique_replay_stream().unwrap().1.len();
    assert_eq!(
        final_count,
        60 + APPENDS,
        "the store holds {final_count} records after {APPENDS} appends on a 60-event seed"
    );
}
