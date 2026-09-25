pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
pub const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
/// A COUNT CEILING ONLY. It is not the number of events a batch can carry (#861).
///
/// The operative bound is **count and size together**, and it is enforced on the write path by
/// parsing the canonical line back and putting it to the reader's own `validate_batch_attempted`
/// (`local.rs`, in `append_locked`, just after the `MAX_BATCH_BYTES` check). The schema is validated on
/// every read, and that validation is preceded by a deterministic work governor whose budget is a
/// function of the document's total value count and text size. So a batch far below the number
/// declared here is routinely refused, with `GHE006_LIMIT_EXCEEDED`.
///
/// **How far below, measured rather than estimated** -- one `ExecutionStarted` plus N
/// `SignalRecorded`, which is the shape `serve/wake.rs` produces when several timers fire in one
/// sweep, bisected with both controls answering:
///
/// ```text
/// largest that round-trips   142 events
/// first refused              143 events   GHE006_LIMIT_EXCEEDED
/// this constant               10,000      about 70x above it, FOR THAT SHAPE
/// ```
///
/// **That number is an illustration and must never become a second constant.** #861 was filed
/// quoting 147, measured against an earlier `main`; the bisection above answers 143 on today's.
/// Nothing about batches changed in between -- the governor's budget moves with the document, so
/// the crossing point moves with work that has nothing to do with this file. A smaller constant
/// here would be just as false as 10,000 and would rot faster, which is why the fix for #744
/// deliberately asks the reader instead of restating the bound.
///
/// **So what is this number for?** It is the coarse refusal that costs nothing: an absurd count is
/// rejected before any work is done on it. It is a guard against a caller that has lost its mind,
/// not a capacity a caller can plan against. A caller sizing a batch should size it small and
/// handle `GHE006_LIMIT_EXCEEDED`, because that refusal is the only true answer available before
/// the batch exists.
///
/// It is not renamed, and that is a decision rather than an omission: the name is public API used
/// across the workspace, and a rename would put churn in every consumer to carry information that
/// belongs in this paragraph. The promise it makes is narrowed here instead.
///
/// The relationship this describes is pinned by `core/events/tests/batch_validation_bound.rs`,
/// which asserts that the store never accepts a batch it cannot read back -- deliberately without
/// naming the crossing point, for the reason above.
pub const MAX_BATCH_EVENTS: usize = 10_000;
pub const MAX_EVIDENCE_ITEMS: usize = 10_000;
pub const MAX_EVIDENCE_BATCH_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ARTIFACTS: usize = 64;
pub const MAX_READ_PAGE: usize = 1_000;
pub const MAX_READ_ALL: usize = 100_000;
pub const MAX_CURSOR_BYTES: usize = 4 * 1024;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Wall-clock budget for ONE status read, in milliseconds (#750).
///
/// `MAX_READ_ALL` above bounds how MANY events a read walks. It never bounded how LONG that
/// takes, and the two are not the same claim: a status read on a long history simply took as
/// long as it took, with no signal to the caller that it was about to wait. Long enough reads
/// as a hang, not as a slow answer.
///
/// **This number is a decision, not a measurement, and the measurement it rests on is this**
/// (#750, on one Windows machine, ~815-byte events, every run's reply checked to carry the
/// whole history):
///
/// - the shipped profile costs ~0.105 ms/event end to end, linear, split roughly 57% in the
///   journal verification `open` performs and 42% in the fold;
/// - an unoptimised build costs ~1.3-2.1 ms/event, which is where the "186 s" figure this
///   issue was raised with comes from;
/// - `MAX_READ_ALL` is not reachable for events of that size: `MAX_JOURNAL_BYTES` binds first,
///   at ~82,300 events (measured: append refused at 82,301, journal at 67,107,150 bytes), so
///   the largest store this format can present answers in ~8.6 s.
///
/// So 5 seconds refuses roughly the top half of the REACHABLE range (above ~47,000 events in
/// the shipped profile) and answers everything below it. That is the trade being made: a
/// caller with a very long history is told, in a typed refusal naming what it walked, that
/// this read cannot answer within its budget - instead of waiting seconds with no signal. The
/// raw log stays readable through the paged route (`GET /v1/executions/{id}/events?after=&limit=`),
/// which is how such a caller still makes progress.
///
/// **What would move this number:** nobody has measured how many events a real owner-journey
/// execution produces. When someone does, this constant should be revisited against it rather
/// than against the format's ceiling.
pub const STATUS_READ_BUDGET_MILLIS: i64 = 5_000;
