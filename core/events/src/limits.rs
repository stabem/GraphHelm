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
