pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
pub const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
/// The declared ceiling on events in one physical batch.
///
/// **It is not the only bound a batch can cross (#744).** The physical-batch schema is validated
/// on every read, and that validation is preceded by a deterministic work governor whose budget
/// is a function of the document's total value count and text size -- so the largest batch that
/// actually round-trips depends on event SIZE as well as count, and for small events it is far
/// below this number. Rather than restate that as a second constant here, which would drift from
/// the validator it is describing, the append path puts the batch it is about to write to the
/// reader's own validator and refuses what the reader would refuse. This constant therefore
/// remains the coarse ceiling it always was, and is no longer the only thing standing between a
/// caller and a journal line nothing can read.
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
