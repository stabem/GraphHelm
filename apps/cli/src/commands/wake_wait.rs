//! `graphhelm wake-wait` (05g Task 4): the sidecar half of the doorbell. Creates the
//! platform rendezvous for an OPAQUE id (the same fixed-prefix derivation the serve's ring
//! uses — never a caller-supplied path), blocks for free, and exits by code alone:
//! `0` = rung, `3` = timeout, `2` (GHCLI017) = unusable arguments. **Content never
//! crosses**: whatever a hostile ringer writes, the bytes die here — the woken host learns
//! only THAT it should re-read its log, never WHAT anyone wanted it to think.
//!
//! #88: the timeout answer CONSULTS THE RECEIPT. Before this, the deadline path reported
//! from the lease it read before blocking, so a sleeper whose ring was consumed but whose
//! byte was lost heard "nothing happened" while the store's own receipt said `rung` — two
//! surfaces, one question, opposite answers, and the operator acts on the calm one. The
//! deadline answer now carries what one read of the store found at that instant
//! (`lastConsumed` / `missedRing` / `laterArmingLive`, HTTP-surface vocabulary), without
//! moving the deadline, retrying, re-arming, or changing any exit code — exit 3 already
//! contracts "do your fallback read", and that stays the truth.

use graphhelm_protocols::{Diagnostic, OpaqueId};

use crate::args::WakeWaitArgs;
use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "wake.wait";
const MCP_WAKE_INVALID: &str = crate::error_codes::GHCLI017_WAKE_INVALID;
const TEST_CAUSAL_TRANSCRIPT: &str = "GRAPHHELM_TEST_WAKE_CAUSAL_TRANSCRIPT";
const TEST_CAUSAL_TRANSCRIPT_MAX_BYTES: u64 = 4 * 1024;

/// Timeout is not a failure: the dead-man design MEANS timeouts happen routinely. Exit 3
/// distinguishes "nothing rang" from success (0) and refusal (2) for hook-friendly callers.
const EXIT_TIMEOUT: i32 = 3;

/// Fixed, test-owned causal marks for the real deadline path. The seam is absent unless
/// its private environment variable names a transcript file, and its single child writer
/// refuses each append that would grow the transcript beyond 4 KiB. Failures are
/// deliberately unobservable to the command: a test observer must never change the wake
/// result or leak its local path through CLI output.
fn record_test_causal_mark(mark: &str) {
    let Some(path) = std::env::var_os(TEST_CAUSAL_TRANSCRIPT) else {
        return;
    };
    let Ok(mut transcript) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let Some(next_len) = transcript
        .metadata()
        .ok()
        .and_then(|metadata| metadata.len().checked_add(mark.len().try_into().ok()?))
        .and_then(|length| length.checked_add(1))
    else {
        return;
    };
    if next_len > TEST_CAUSAL_TRANSCRIPT_MAX_BYTES {
        return;
    }
    use std::io::Write as _;
    let _ = writeln!(transcript, "{mark}");
}

fn refuse(message: &str, pointer: &str) -> Outcome {
    Outcome::domain(
        COMMAND,
        vec![Diagnostic::error(
            MCP_WAKE_INVALID,
            message,
            pointer,
            COMMAND,
        )],
    )
}

pub fn run(args: &WakeWaitArgs) -> Outcome {
    // ONE read of the store, and exactly one thing read from it: the lease belonging to THIS
    // session. The rendezvous and the deadline both come from there, so the two numbers that
    // used to answer "how long before I give up" -- the declared bound and a `--timeout` the
    // caller typed -- collapse into one that cannot disagree with itself.
    //
    // The handle is dropped BEFORE the wait begins, and that is not tidiness. The repository
    // holds an OS-level exclusive lock for the handle's lifetime (the reason `serve` refuses to
    // cache one), so a waiter that held it across an eight-hour sleep would lock every
    // concurrent process out of the store for the whole night -- the window in which the work
    // is supposed to carry on without the operator.
    let lease = match read_own_lease(&args.events, args.execution.as_deref(), &args.session_id) {
        Ok(lease) => lease,
        Err(outcome) => return outcome,
    };

    let now = chrono::Utc::now();
    let remaining = lease.matures_at.signed_duration_since(now);
    if remaining <= chrono::Duration::zero() {
        // B10: the deadline is already behind us. Answering at once matters because this is
        // the case where something has ALREADY gone wrong, and blocking would make it the one
        // case the tool sits quiet through.
        return matured(args, &lease, true);
    }

    match wait(
        &lease.rendezvous_id,
        u64::try_from(remaining.num_seconds()).unwrap_or(0).max(1),
    ) {
        WaitEnd::Rung => Outcome::success(
            COMMAND,
            serde_json::json!({"rung": true, "maturesAt": lease.matures_at.to_rfc3339()}),
        ),
        // The only bound there is now is the DECLARED one, so this exit stopped being
        // ambiguous by construction: it cannot mean "the number I happened to type ran out".
        WaitEnd::TimedOut => matured(args, &lease, false),
        WaitEnd::Unusable(message) => refuse(&message, "/sessionId"),
    }
}

/// The lease this session armed, and nothing else from the store.
struct OwnLease {
    rendezvous_id: String,
    matures_at: chrono::DateTime<chrono::Utc>,
    /// Which arming this wait belongs to — the sequence of the `wake_lease` event that
    /// produced the lease (`WakeLeaseState.armed_at_sequence`, valid because the pre-block
    /// read REPLAYS). Captured before blocking so the deadline answer can tell "a receipt
    /// for THIS arming" from a previous cycle's leftovers: session and rendezvous both
    /// repeat by convention (#74's dead-ends), the arming sequence cannot.
    armed_at_sequence: u64,
}

/// What the one receipt read at the deadline found (#88). Three worlds plus "could not
/// look" — never collapsed into each other, because the operator's next move differs.
enum ReceiptAtDeadline {
    /// The receipt CANNOT BE TRUSTED — reported as its own value ("unreadable"), never as
    /// null, because an untrustworthy answer and a silent receipt are different worlds and
    /// must not flatten. NOT a refusal: the wait already timed out, and that verdict
    /// stands whether or not the diagnostic read works.
    ///
    /// This value spans TWO causes whose remedies differ (C's #88 pass, on record until a
    /// distinct value splits them — tracked in the PR's debt):
    ///   could not look — open/resolve/replay failed: retry; check path, permissions,
    ///     mount. Transient world.
    ///   looked, and the answer is impossible — the history's head predates this arming
    ///     (deleted, truncated, or swapped store): STOP TRUSTING THIS STORE, do not
    ///     retry. Integrity-class finding, #119-adjacent, not an I/O hiccup.
    Unreadable,
    Read {
        /// The consumption that took THIS arming's lease, if one exists: (reason, the
        /// consumption's sequence). A previous cycle's receipt is suppressed to null — one
        /// glance, one answer. Reported even when the burn was mis-aimed (fold parity: the
        /// fold records a receipt for the victim session either way).
        last_consumed: Option<(graphhelm_protocols::WakeConsumeReason, u64)>,
        /// True only for a `rung` burn that was AIMED at this arming — a mis-aimed burn
        /// consumed the lease without its ring ever being meant for it.
        ///
        /// DECISION ON RECORD (C's #88 strike 2, premise corrected by C's follow-up):
        /// `false` spans every world that is not a missed ring, and stays a boolean
        /// anyway — an enum re-answering "which world" beside the markers would be a
        /// second derivation path that can disagree with them, the duplicate-path defect
        /// #74 and #83 removed. The markers are DISJOINT but NOT TOTAL as three rules:
        /// there are FOUR states, and the fourth is identified by the reason strike 1
        /// made readable —
        ///   silence:      lastConsumed null
        ///   missed ring:  missedRing true
        ///   mis-burn:     misBurn present
        ///   burned stale: lastConsumed.reason == "stale_rendezvous" with misBurn absent
        ///     (the sweep aimed at THIS arming and judged its rendezvous dead while the
        ///     waiter lived — reachable today when a ring lands before the sidecar's pipe
        ///     exists; the operator's move is unlike all three others: the wake PATH
        ///     degraded, re-arm and look at why).
        /// A consumer must read the triple; a consumer reading one boolean was always
        /// going to be wrong somewhere, and this doc says exactly where.
        missed_ring: bool,
        /// A live lease for this session armed AFTER this one — someone already re-armed,
        /// so waking this waiter's host into "re-arm" would double-arm.
        later_arming_live: bool,
        /// A consumption took this arming's lease while NAMING a different one: (the burn's
        /// sequence, the arming it captured). Same diagnosis the fold's `wake_mis_burns`
        /// records — derived here from the log because the fold's map keeps only the LAST
        /// entry per session (C's #88 review finding) and this arming's entry may already
        /// be overwritten by the time the deadline looks.
        mis_burn: Option<(u64, u64)>,
    },
}

struct ReceiptSnapshot {
    history: Vec<graphhelm_protocols::EventEnvelope>,
    projection: graphhelm_events::ExecutionProjection,
}

/// The operation boundary for one real receipt-store attempt. The causal mark belongs
/// immediately beside the open/resolve/replay operation, not at its caller: a future retry
/// must cross this boundary again and therefore becomes a second observable attempt.
fn read_receipt_snapshot_attempt(
    events: &std::path::Path,
    execution: Option<&str>,
) -> Option<ReceiptSnapshot> {
    record_test_causal_mark("receipt-read-attempt");
    let store = crate::commands::event_store(events).ok()?;
    let (scope, stream, history) =
        crate::commands::execution::resolve_stream(&store, execution).ok()?;
    let projection = graphhelm_events::replay(&scope, &stream, &history).ok()?;
    Some(ReceiptSnapshot {
        history,
        projection,
    })
}

/// ONE read, at the deadline, and the handle never survives it. Every failure shape is
/// `Unreadable` — the timeout verdict is already made and this read can only enrich it.
///
/// This is a SNAPSHOT, not a verdict: the consume append is eventually-durable by design
/// (serve/wake.rs two-phase), so a receipt absent here may land a moment later. The field
/// `receiptReadAt: "deadline-once"` carries exactly that epistemics, and the fallback read
/// stays the truth (the wake is an accelerator, never a correction). Waiting here for the
/// receipt would import an unbounded tail into a bounded wait — never done.
fn receipt_at_deadline(
    events: &std::path::Path,
    execution: Option<&str>,
    session_id: &str,
    armed_at_sequence: u64,
) -> ReceiptAtDeadline {
    use graphhelm_protocols::EventKind;
    let Some(ReceiptSnapshot {
        history,
        projection,
    }) = read_receipt_snapshot_attempt(events, execution)
    else {
        return ReceiptAtDeadline::Unreadable;
    };
    // A store whose head predates THIS arming is not the store we armed against. The
    // arming was durable at `armed_at_sequence` before the wait began (the pre-block read
    // proved it), and sequences are monotone — so a shorter history means deleted,
    // truncated, or swapped, not "quiet". Discovered live at stage 2 of the red protocol:
    // deleting the events directory does NOT fail the open/read (a missing stream reads
    // as legally EMPTY), so without this check an unreadable store reports as W1 silence
    // — the exact flattening G5 exists to refuse. The check is the sidecar using the one
    // fact it already owns about the store, not a new probe.
    //
    // THIS BRANCH IS SOUND ONLY WHILE STREAMS ARE APPEND-ONLY (verified at review: no
    // compaction, archival, truncation or rotation entry point exists in core/events or
    // the adapters). A feature that legitimately shortens a stream would make this check
    // report healthy stores as untrusted — whoever lands compaction must revisit this
    // line. This sentence is the canary for that change.
    if history.last().map_or(0, |event| event.sequence) < armed_at_sequence {
        return ReceiptAtDeadline::Unreadable;
    }
    // Attribution walks the LOG, not the projection's receipt maps: `wake_last_consumed`
    // and `wake_mis_burns` keep only the LAST entry per session (insert overwrites), so a
    // full burn/re-arm/burn cycle inside this wait would erase THIS arming's entry and
    // collapse a missed ring into silence — the exact flattening #88 exists to prevent
    // (C's review finding). The log forgets nothing: one ordered pass over this session's
    // wake events, tracking the live arming the same way the fold itself does (a burn's
    // victim is whatever lease is live when it lands), finds this arming's burn no matter
    // what happened after it.
    let mut current_live: Option<u64> = None;
    let mut my_receipt: Option<(graphhelm_protocols::WakeConsumeReason, u64)> = None;
    let mut my_mis_burn: Option<(u64, u64)> = None;
    for event in &history {
        match &event.kind {
            EventKind::WakeLease(payload) if payload.session_id.as_str() == session_id => {
                current_live = Some(event.sequence);
            }
            EventKind::WakeLeaseConsumed(payload) if payload.session_id.as_str() == session_id => {
                // A replayed history guarantees a live lease existed (the fold refuses a
                // consume-without-lease as Corrupt); `take` mirrors the fold's remove.
                let victim = current_live.take();
                if victim == Some(armed_at_sequence) {
                    my_receipt = Some((payload.reason, event.sequence));
                    // `captured_arming` distinguishes an honest burn from a mis-aimed one;
                    // absent (pre-field history), victim-order IS the old inference, made
                    // exact by construction.
                    if let Some(captured) = payload.captured_arming
                        && captured != armed_at_sequence
                    {
                        my_mis_burn = Some((event.sequence, captured));
                    }
                }
            }
            _ => {}
        }
    }
    let missed_ring = my_mis_burn.is_none()
        && matches!(
            my_receipt,
            Some((graphhelm_protocols::WakeConsumeReason::Rung, _))
        );
    ReceiptAtDeadline::Read {
        last_consumed: my_receipt,
        missed_ring,
        // Live state is the one question the projection DOES retain losslessly, so it is
        // read from the fold rather than re-derived: split deliberately — walk the log
        // where the fold's map is lossy (per-arming receipt), trust the fold where it is
        // not (the current live lease).
        later_arming_live: projection
            .wake_leases
            .get(session_id)
            .is_some_and(|lease| lease.armed_at_sequence > armed_at_sequence),
        mis_burn: my_mis_burn,
    }
    // The handle goes out of scope here — same discipline as the pre-block read.
}

/// The end of a wait that ended by the clock rather than by a ring.
///
/// `alreadyPast` is not decoration. It separates "the deadline you declared had ALREADY gone
/// by when you asked" from "it went by while you waited", and those are different situations
/// for the operator: the first means something went wrong before anyone looked. It is also
/// what makes the immediate answer testable — an assertion on elapsed time cannot tell a
/// zero-second wait from a one-second one, because process start-up costs more than either.
fn matured(args: &WakeWaitArgs, lease: &OwnLease, already_past: bool) -> Outcome {
    record_test_causal_mark("deadline-transition");
    // #88: the deadline answer consults the receipt — ONE read, right now, both for the
    // waited-out path and the already-past one (B10 pays one extra open over reusing its
    // pre-block read; one seam that both paths share beats one saved open on the path
    // where something already went wrong). The deadline itself never moves for this.
    let receipt = receipt_at_deadline(
        &args.events,
        args.execution.as_deref(),
        &args.session_id,
        lease.armed_at_sequence,
    );
    let mut data = serde_json::json!({
        "rung": false,
        "timedOut": true,
        "matured": true,
        "alreadyPast": already_past,
        "maturesAt": lease.matures_at.to_rfc3339(),
        // The epistemics of everything below: read once, at the deadline. A consume append
        // may still be in flight (two-phase by design) — this is a snapshot, not a verdict,
        // and the fallback read remains the truth.
        "receiptReadAt": "deadline-once",
    });
    let object = data.as_object_mut().expect("data is an object");
    match receipt {
        ReceiptAtDeadline::Unreadable => {
            // The store could not be read; the timeout verdict stands (never exit 2 for a
            // failed enrichment). "unreadable" is its own value, never conflated with null.
            object.insert("lastConsumed".to_owned(), serde_json::json!("unreadable"));
            object.insert("missedRing".to_owned(), serde_json::json!(false));
            object.insert("laterArmingLive".to_owned(), serde_json::json!(false));
        }
        ReceiptAtDeadline::Read {
            last_consumed,
            missed_ring,
            later_arming_live,
            mis_burn,
        } => {
            object.insert(
                "lastConsumed".to_owned(),
                match last_consumed {
                    // Same vocabulary as the HTTP wake-lease surface (M07 F4): one fact,
                    // one name, two transports.
                    Some((reason, sequence)) => serde_json::json!({
                        "reason": reason,
                        "atSequence": sequence,
                    }),
                    None => serde_json::Value::Null,
                },
            );
            // "Deadline passed BUT the receipt says rung at #N — you were woken and may
            // have missed it": re-read from your cursor, THEN decide about re-arming.
            object.insert("missedRing".to_owned(), serde_json::json!(missed_ring));
            // Someone already re-armed this session: waking this host into "re-arm"
            // would double-arm. Different world, different move, different field.
            object.insert(
                "laterArmingLive".to_owned(),
                serde_json::json!(later_arming_live),
            );
            if let Some((at_sequence, captured_arming)) = mis_burn {
                // The fold's own diagnosis (wake_mis_burns): this arming was burned by a
                // consumption that NAMED a different one. Surfaced verbatim, not re-derived.
                object.insert(
                    "misBurn".to_owned(),
                    serde_json::json!({
                        "atSequence": at_sequence,
                        "capturedArming": captured_arming,
                    }),
                );
            }
        }
    }
    let outcome = Outcome {
        output: CommandOutput {
            ok: true,
            command: COMMAND,
            data: Some(data),
            diagnostics: vec![],
        },
        exit_code: EXIT_TIMEOUT,
    };
    record_test_causal_mark("outcome-publication");
    outcome
}

fn read_own_lease(
    events: &std::path::Path,
    execution: Option<&str>,
    session_id: &str,
) -> Result<OwnLease, Outcome> {
    let lease = {
        let store = crate::commands::event_store(events)
            .map_err(|error| refuse(&error.to_string(), "/events"))?;
        let (scope, stream, history) =
            crate::commands::execution::resolve_stream(&store, execution)
                .map_err(|failure| refuse(&failure.message, "/execution"))?;
        let projection = graphhelm_events::replay(&scope, &stream, &history)
            .map_err(|error| refuse(&error.to_string(), "/execution"))?;
        projection.wake_leases.get(session_id).cloned()
        // The handle goes out of scope HERE, before anything blocks.
    };

    let Some(lease) = lease else {
        return Err(refuse(
            &format!(
                "{session_id} has no live lease on this execution: a waiter waits on its OWN lease, never on whichever one it finds"
            ),
            "/sessionId",
        ));
    };
    let armed_at_sequence = lease.armed_at_sequence;
    let Some(matures_at) = lease.matures_at else {
        return Err(refuse(
            &format!(
                "{session_id} armed no bound, so nothing here promises to end the wait: arm again with maturesInSeconds"
            ),
            "/sessionId",
        ));
    };
    // The rendezvous now comes from the STORE, where `arm` parsed it as an opaque id before
    // writing. Checked again anyway: this is the value a platform rendezvous name is derived
    // from, and provenance is an argument while a parse is a guarantee.
    if OpaqueId::parse(lease.rendezvous_id.clone()).is_err() {
        return Err(refuse(
            "the lease carries a rendezvous id that is not wire-safe",
            "/rendezvousId",
        ));
    }
    Ok(OwnLease {
        rendezvous_id: lease.rendezvous_id,
        matures_at: *matures_at.as_datetime(),
        armed_at_sequence,
    })
}

pub(crate) enum WaitEnd {
    Rung,
    TimedOut,
    Unusable(String),
}

#[cfg(windows)]
pub(crate) fn wait(rendezvous_id: &str, timeout_seconds: u64) -> WaitEnd {
    use tokio::io::AsyncReadExt;
    let name = format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}");
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return WaitEnd::Unusable("the wait runtime could not start".to_owned());
    };
    runtime.block_on(async move {
        // The sleeper CREATES, first-instance (a squatter on the name is refused by the
        // API itself — the Task 0 spike's proven shape), owner-only by the default ACL.
        let mut server = match tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .max_instances(1)
            .create(&name)
        {
            Ok(server) => server,
            Err(_) => {
                return WaitEnd::Unusable(
                    "the rendezvous could not be created (already armed elsewhere?)".to_owned(),
                );
            }
        };
        let deadline = std::time::Duration::from_secs(timeout_seconds);
        let waited = tokio::time::timeout(deadline, async {
            if server.connect().await.is_err() {
                return false;
            }
            // Read and DISCARD: content never crosses the sidecar. One byte is the
            // protocol; a hostile ringer's extra bytes die in this buffer.
            let mut sink = [0_u8; 64];
            matches!(server.read(&mut sink).await, Ok(read) if read > 0)
        })
        .await;
        match waited {
            Ok(true) => WaitEnd::Rung,
            Ok(false) => WaitEnd::TimedOut,
            Err(_) => WaitEnd::TimedOut,
        }
    })
}

/// How long a read may take, given the caller's deadline and the current instant -- or `None`
/// when the deadline has already passed.
///
/// NOT `cfg`-gated, and separate from its only call site, deliberately. The socket it serves exists
/// only on Unix, so a cell for the decision could not run on the machines this fleet uses if the
/// arithmetic lived inline. Extracting it does not make the wiring tested -- that is what the
/// `#[cfg(unix)]` cell below is for -- but it does make the part that is easy to get wrong reachable.
///
/// `None` MEANS REFUSE, AND NEVER "no timeout". `set_read_timeout(Some(Duration::ZERO))` answers
/// `Err(InvalidInput)` -- measured on 1.97.1, and pinned by a cell below rather than quoted -- so a
/// caller that passed a zero budget down through `let _ = ...` would leave the socket with NO
/// timeout at all. The deadline expiring would then widen the read to unbounded: this function's
/// own defect, arriving through its own fix, and reported as a timeout that never fires.
// COMPILED ON WINDOWS ONLY UNDER `test`. Its call site is the `cfg(not(windows))` arm, so a
// Windows release build has no caller and `-D warnings` makes an unused function an error -- but
// the cells below DO use it there, and they are the reason it was extracted. `any(not(windows),
// test)` says exactly that, where `allow(dead_code)` would say something weaker and permanent.
#[cfg(any(not(windows), test))]
fn read_budget(
    deadline: std::time::Instant,
    now: std::time::Instant,
) -> Option<std::time::Duration> {
    let remaining = deadline.saturating_duration_since(now);
    if remaining.is_zero() {
        None
    } else {
        Some(remaining)
    }
}

#[cfg(not(windows))]
pub(crate) fn wait(rendezvous_id: &str, timeout_seconds: u64) -> WaitEnd {
    use std::io::Read;
    let runtime = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) => dir,
        Err(_) => "/tmp".to_owned(),
    };
    let directory = std::path::Path::new(&runtime).join("graphhelm");
    if std::fs::create_dir_all(&directory).is_err() {
        return WaitEnd::Unusable("the rendezvous directory could not be created".to_owned());
    }
    let path = directory.join(format!("wake-{rendezvous_id}.sock"));
    let _ = std::fs::remove_file(&path);
    let listener = match std::os::unix::net::UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(_) => return WaitEnd::Unusable("the rendezvous could not be created".to_owned()),
    };
    let _ = listener.set_nonblocking(false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_seconds);
    listener
        .set_nonblocking(true)
        .expect("nonblocking accept is available");
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = std::fs::remove_file(&path);
                // THE DEADLINE HAS TO REACH THE READ, and until #798 it did not. `accept` returns a
                // socket that does NOT inherit the listener's `O_NONBLOCK` -- measured on
                // Linux 6.6.87.2, AF_UNIX, via a raw `accept()` -- so this stream is BLOCKING even
                // though `set_nonblocking(true)` was called on the listener above. With no read
                // timeout set, a peer that connects and never writes held this call forever,
                // whatever `timeout_seconds` said.
                //
                // The Windows arm of this same function has always been right: one
                // `tokio::time::timeout` around both the connect and the read. This is that shape,
                // spelled for a blocking socket.
                let Some(budget) = read_budget(deadline, std::time::Instant::now()) else {
                    return WaitEnd::TimedOut;
                };
                // NOT `let _ =`. Arming is what bounds the read, so an arming failure must not fall
                // through into the unbounded read it exists to prevent -- that would be the defect
                // again, reached through the error path of its own fix.
                if stream.set_read_timeout(Some(budget)).is_err() {
                    return WaitEnd::Unusable(
                        "the rendezvous read could not be bounded".to_owned(),
                    );
                }
                let mut sink = [0_u8; 64];
                let rung = matches!(stream.read(&mut sink), Ok(read) if read > 0);
                return if rung {
                    WaitEnd::Rung
                } else {
                    WaitEnd::TimedOut
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    let _ = std::fs::remove_file(&path);
                    return WaitEnd::TimedOut;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => {
                let _ = std::fs::remove_file(&path);
                return WaitEnd::Unusable("the rendezvous accept failed".to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::read_budget;
    use std::time::{Duration, Instant};

    /// #798'S OWN RED, and it runs ONLY on Unix -- the platform where the defect is real and the
    /// one this fleet does not run on. On Windows it is not compiled, so a green suite here says
    /// NOTHING about it: its colour is unknown until someone runs it on Linux, and this comment
    /// exists so nobody reads the suite's green as covering it.
    ///
    /// BOUNDED BY ITS OWN CHANNEL, not by the harness. The defect is a call that never returns and
    /// `cargo test` has no per-test timeout, so against an unfixed tree this cell would HANG the
    /// suite rather than redden it -- and a hang has no colour. `recv_timeout` turns "never
    /// returned" into a failed assertion that names what happened.
    ///
    /// Twenty seconds against a one-second deadline: wide enough that a loaded host cannot make it
    /// flake, narrow enough that a genuinely unbounded read cannot pass. The gap is deliberate --
    /// this cell is about the difference between "late" and "never", not about precision.
    #[cfg(unix)]
    #[test]
    fn a_connector_that_never_writes_cannot_hold_the_wait_past_its_deadline() {
        use std::io::Read;
        use std::os::unix::net::UnixStream;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after the epoch")
            .as_nanos();
        let id = format!("798-{}-{unique}", std::process::id());
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_owned());
        let path = std::path::Path::new(&runtime)
            .join("graphhelm")
            .join(format!("wake-{id}.sock"));

        let (sender, receiver) = std::sync::mpsc::channel();
        let waiter_id = id.clone();
        std::thread::spawn(move || {
            let _ = sender.send(super::wait(&waiter_id, 1));
        });

        // The rendezvous is created BY `wait`, so wait for it to appear rather than racing it. This
        // bound is a HARNESS guard: if the socket never shows up the arrangement failed and nothing
        // below is about the deadline.
        let arranging = Instant::now();
        while !path.exists() {
            assert!(
                arranging.elapsed() < Duration::from_secs(10),
                "HARNESS-BROKE: the rendezvous never appeared at {path:?}, so this cell never \
                 reached the behaviour it is about"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        // CONNECT AND SAY NOTHING. That is the whole arrangement: before #798 the accept succeeded,
        // the loop was left, and the read blocked forever on a socket that never speaks. The stream
        // is held to the end of the cell on purpose -- dropping it would close the peer and let the
        // read return EOF, which is the one thing that would make an unfixed tree pass.
        let mut held = UnixStream::connect(&path).expect("the rendezvous accepts a connection");

        match receiver.recv_timeout(Duration::from_secs(20)) {
            Ok(end) => assert!(
                matches!(end, super::WaitEnd::TimedOut),
                "a connector that never wrote must end as TimedOut, and never as a ring"
            ),
            Err(_) => panic!(
                "wait did not return 20s after its 1s deadline: the read that follows accept is \
                 observing no budget at all, which is #798"
            ),
        }

        // THE WITNESS THAT THE READ WAS REACHED AT ALL, and without it this cell is vacuous
        // (found by a peer reviewing #801). `TimedOut` is returned by TWO paths: the read after a
        // successful accept -- the subject -- and the accept loop's own deadline check, which was
        // ALREADY bounded before #798. `UnixStream::connect` returning Ok proves only that the
        // kernel queued the connection in the backlog; it does not prove `accept` ever returned.
        // So a green was consistent with the wrong path, which is the defect #740's phase
        // assertion exists to remove, one function over.
        //
        // The rendezvous file is NOT a witness: `remove_file` runs on every exit from `wait`,
        // including the accept-loop timeout, so "the socket is gone" cannot tell the two apart.
        // Nor is elapsed time -- both paths end at the deadline.
        //
        // What DOES separate them is what the client sees, measured on Linux 6.6.87.2 (AF_UNIX):
        //
        //     accepted, then dropped       recv -> 0 bytes, a clean EOF
        //     never accepted, listener closed with the connection still queued
        //                                  recv -> ECONNRESET
        //
        // A clean EOF therefore proves this connection left the backlog, which only the accept
        // that leads to the read can do.
        let mut after = [0_u8; 8];
        match held.read(&mut after) {
            Ok(0) => {}
            Ok(n) => panic!("the rendezvous sent {n} bytes back; it never writes to a ringer"),
            Err(error) => panic!(
                "the connection was never accepted ({error}), so `wait` returned from its accept \
                 loop and this cell measured the path that was already bounded before #798, not \
                 the read"
            ),
        }

        drop(held);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_deadline_still_ahead_yields_the_time_that_is_left() {
        let now = Instant::now();
        let budget = read_budget(now + Duration::from_secs(30), now);
        assert_eq!(budget, Some(Duration::from_secs(30)));
    }

    /// THE CELL THIS FUNCTION EXISTS FOR. A deadline that has arrived must REFUSE, not hand back a
    /// zero budget: `set_read_timeout(Some(Duration::ZERO))` is an error, so a zero passed onward
    /// leaves the socket unbounded and the read never returns. The failure direction of a mistake
    /// here is the original defect, not a short wait.
    #[test]
    fn a_deadline_that_has_arrived_refuses_instead_of_yielding_zero() {
        let now = Instant::now();
        assert_eq!(read_budget(now, now), None);
    }

    /// `saturating_duration_since`, not subtraction: an already-passed deadline is ordinary here
    /// (the accept loop can be scheduled out), and `Instant - Instant` would panic on underflow.
    #[test]
    fn a_deadline_already_passed_refuses_rather_than_underflowing() {
        let now = Instant::now();
        assert_eq!(read_budget(now, now + Duration::from_secs(5)), None);
    }

    /// The boundary is between zero and anything, not at some comfortable minimum: one nanosecond
    /// of budget is still a bounded read, and rounding it up to a floor would spend budget the
    /// caller did not grant.
    #[test]
    fn one_nanosecond_of_budget_is_still_a_bound() {
        let now = Instant::now();
        assert_eq!(
            read_budget(now + Duration::from_nanos(1), now),
            Some(Duration::from_nanos(1))
        );
    }

    /// PINNED AGAINST THE REAL API, because the refusal above rests entirely on this being an
    /// error. If a future std accepted a zero duration as "no timeout", the guard would still be
    /// correct but its stated REASON would be wrong -- and if it accepted zero as "return
    /// immediately", the guard would be unnecessary. Either way the next reader should find out
    /// here rather than from a hang.
    ///
    /// A real socket, not a constructed value: this is a claim about the platform, and the only
    /// instrument that can answer it is the platform.
    #[test]
    fn the_platform_refuses_a_zero_read_timeout() {
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let stream =
            TcpStream::connect(listener.local_addr().expect("its address")).expect("a connection");
        assert!(
            stream.set_read_timeout(Some(Duration::ZERO)).is_err(),
            "a zero read timeout is accepted by this platform, so read_budget's refusal needs a \
             different justification than the one written at its definition"
        );
        assert!(
            stream
                .set_read_timeout(Some(Duration::from_millis(1)))
                .is_ok(),
            "a positive read timeout must be settable, or the fix cannot arm at all"
        );
    }
}
