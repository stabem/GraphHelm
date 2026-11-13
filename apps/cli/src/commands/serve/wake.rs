//! The serve-side ring (05g Task 2): after a mutation route's append is DURABLE, sweep the
//! stream's live leases and ring each rendezvous with ONE content-free byte, then record the
//! consumption with the reason the ring actually earned.
//!
//! Two-phase by physics (declared discrepancy against the Task 1 doc comment's "same durable
//! batch"): the ring may only happen after the trigger is durable, and the TRUE consume
//! reason (`rung` vs `stale_rendezvous`) is only known after the ring attempt — so the
//! consumption is its own small follow-up append. A crash between trigger and consumption
//! leaves a live lease and a possibly-delivered byte: a spurious wake, safe by design (the
//! wake is content-free and the sleeper re-reads its own log).
//!
//! Only NON-wake appends ring: the lease's own arming event (and any consumption) is
//! bookkeeping past the cursor, not content — without this rule, arming would self-ring.
//!
//! A wake failure never fails the route that triggered it: the append already happened, and
//! the sleeper's dead-man timer covers every lost byte (accelerator, never correction).

use std::path::Path;
use std::sync::Arc;

use graphhelm_execution::tasks::Verdict;
use graphhelm_protocols::{
    ActorId, EventKind, NewEvent, OpaqueId, PersistedActor, PersistedActorType, Sensitivity,
    WakeConsumeReason, WakeLeaseConsumed,
};

/// Derives the platform rendezvous from the OPAQUE id (the Task 1 security deviation): a
/// fixed local prefix plus the id — a hostile lease can never point the serve at an
/// arbitrary path.
#[cfg(windows)]
fn rendezvous_path(rendezvous_id: &str) -> String {
    format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}")
}

#[cfg(not(windows))]
fn rendezvous_path(rendezvous_id: &str) -> String {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_owned());
    format!("{runtime}/graphhelm/wake-{rendezvous_id}.sock")
}

/// One live lease due to ring, as the sweep's read phase reports it.
#[derive(Clone)]
pub(crate) struct DueLease {
    pub(crate) execution_id: String,
    pub(crate) session_id: String,
    pub(crate) rendezvous_id: String,
    /// WHICH ARMING this capture is for, copied from the projection phase 1 already holds.
    ///
    /// The rendezvous is kept for phase 2 — it is what the ring is addressed to — but it is no
    /// longer a blade in the filter, because a session that re-arms on the same fixed id
    /// produces two leases the rendezvous cannot tell apart. This can.
    pub(crate) armed_at_sequence: u64,
}

/// How long a ring waits for a rendezvous that is not there yet (#1058, #1118).
///
/// A sleeper arms its lease FIRST and creates the rendezvous after (it must: the rendezvous
/// id is read back from the lease). An append in that window used to find no rendezvous and
/// burn the lease `stale_rendezvous` at once, and the arm's own sweep did the same to a lease
/// armed below an ask that had already landed: the sleeper then blocked on a dead lease and
/// re-armed past the ask, which never rang. Retrying the open for a short bound lets a sleeper
/// that is still starting be rung; a sleeper that really died is burned once the bound passes.
/// The sweep is fire-and-forget, so the wait never delays the route that triggered it.
const RENDEZVOUS_GRACE: std::time::Duration = std::time::Duration::from_secs(5);
const RENDEZVOUS_RETRY: std::time::Duration = std::time::Duration::from_millis(50);

/// Rings once the rendezvous can be opened, or answers `StaleRendezvous` when it still cannot
/// be after [`RENDEZVOUS_GRACE`].
async fn ring(rendezvous_id: &str) -> WakeConsumeReason {
    let deadline = tokio::time::Instant::now() + RENDEZVOUS_GRACE;
    loop {
        match ring_once(rendezvous_id).await {
            Some(reason) => return reason,
            None if tokio::time::Instant::now() >= deadline => {
                return WakeConsumeReason::StaleRendezvous;
            }
            None => tokio::time::sleep(RENDEZVOUS_RETRY).await,
        }
    }
}

/// One attempt to write the one content-free byte. `Rung` when it crossed, `StaleRendezvous`
/// when the rendezvous opened but the write failed, `None` when it could not be opened (not
/// created yet, or the sleeper died): the caller decides whether to try again.
#[cfg(windows)]
async fn ring_once(rendezvous_id: &str) -> Option<WakeConsumeReason> {
    use tokio::io::AsyncWriteExt;
    let path = rendezvous_path(rendezvous_id);
    match tokio::net::windows::named_pipe::ClientOptions::new().open(&path) {
        Ok(mut client) => Some(match client.write_all(&[1_u8]).await {
            Ok(()) => WakeConsumeReason::Rung,
            Err(_) => WakeConsumeReason::StaleRendezvous,
        }),
        // Absent or busy: nothing was written, so it may be tried again.
        Err(_) => None,
    }
}

#[cfg(not(windows))]
async fn ring_once(rendezvous_id: &str) -> Option<WakeConsumeReason> {
    use tokio::io::AsyncWriteExt;
    let path = rendezvous_path(rendezvous_id);
    match tokio::net::UnixStream::connect(&path).await {
        Ok(mut stream) => Some(match stream.write_all(&[1_u8]).await {
            Ok(()) => WakeConsumeReason::Rung,
            Err(_) => WakeConsumeReason::StaleRendezvous,
        }),
        Err(_) => None,
    }
}

/// The arming actor of each live lease, by its arming sequence: from the digest of the read the
/// projection came from, so a lease armed in the tail after a warm fold is covered (#2694).
fn arming_actors<'a>(
    leases: impl Iterator<Item = (u64, &'a str, &'a str)>,
    projection: &graphhelm_events::ExecutionProjection,
) -> std::collections::BTreeMap<u64, &'a str> {
    leases
        .filter(|(sequence, session, _)| {
            projection
                .wake_leases
                .get(*session)
                .is_some_and(|live| live.armed_at_sequence == *sequence)
        })
        .map(|(sequence, _, actor)| (sequence, actor))
        .collect()
}

/// Phase 1 of the sweep: what is due? Replay the stream and keep every live lease whose
/// cursor lies before relevant content, respecting each operator note recipient. `None` on any failure — the sweep is
/// fire-and-forget and every failure path is a silent no-op.
///
/// Its own function so the unit tests below build their captures by CALLING it rather than by
/// copying it (#258). A copy cannot notice the original drifting, and that drift is exactly
/// what those tests exist to catch: with `armed_at_sequence` corrupted here, every cell stayed
/// green while the read was a copy, because none of them ran this code.
fn due_leases(
    events: &Path,
    execution: &str,
    sealing: Option<&crate::commands::execution::signal::SignalKeyring>,
) -> Option<Vec<DueLease>> {
    let store = crate::commands::event_store(events).ok()?;
    let streams = store.list_streams().ok()?;
    let stream = streams
        .into_iter()
        .find(|stream| stream.stream_id == execution)?;
    // #2694: the kept fold plus the events appended since (`status::glance`), not a copy and a
    // refold of the whole stream under the store's lock on every sweep.
    let glance = crate::commands::execution::status::glance(
        &store,
        events,
        &stream.scope,
        &stream.stream_id,
        &graphhelm_events::ReadBudget::unbounded(),
    )
    .ok()?;
    let projection = glance.projection.ok()?;
    // Only NON-wake appends ring: bookkeeping past the cursor is not content. A presence
    // declaration is bookkeeping too (#1054): it is appended right after the mutation that
    // armed the lease, by the same session, and counting it as content consumed the lease
    // before the sleeper ever waited (gate 9b277afb, `wake_http`:
    // a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window`).
    let cursor = projection
        .wake_leases
        .values()
        .map(|lease| lease.cursor)
        .min()?;
    let (history, _) = store
        .read_stream_after(&stream.scope, &stream.stream_id, cursor)
        .ok()?;
    let actors = arming_actors(glance.digest.wake_leases(), &projection);
    let mut content_head = 0;
    let mut notes: std::collections::BTreeMap<&str, u64> =
        actors.values().map(|actor| (*actor, 0)).collect();
    // Open only pending notes, with the Runtime's existing keyring. Missing/unreadable
    // evidence cannot be treated as an unaddressed note and broadcast to unrelated actors.
    let mut opener = None;
    for event in history.iter() {
        match &event.kind {
            EventKind::WakeLease(_)
            | EventKind::WakeLeaseConsumed(_)
            | EventKind::AgentPresenceDeclared(_) => {}
            EventKind::SignalRecorded(signal)
                if signal.kind == "operator_note" || signal.kind.starts_with("task.") =>
            {
                let opener = opener.get_or_insert_with(|| super::ports::build_opener(sealing).ok());
                let Some(opener) = opener else {
                    continue;
                };
                for reference in &event.evidence_refs {
                    let Ok(graphhelm_events::EvidenceRead::Available(sealed)) =
                        store.sealed_evidence(&stream.scope, reference.evidence_id())
                    else {
                        continue;
                    };
                    // Attachments are evidence too; only the admitted envelope carries routing.
                    if sealed.reference().content_sha256().as_str()
                        != signal.envelope_sha256.as_str()
                    {
                        continue;
                    }
                    let Ok(plaintext) = tokio::runtime::Handle::current()
                        .block_on(opener.open(stream.scope.clone(), &sealed))
                    else {
                        continue;
                    };
                    let Ok(envelope) = plaintext
                        .expose(|bytes| serde_json::from_slice::<serde_json::Value>(bytes))
                    else {
                        continue;
                    };
                    // #1772: a task record is lane bookkeeping. It wakes only the lane it hands
                    // work to (a review assignment wakes its reviewer); every other task record
                    // rings no one, so a lane parked on wake-wait is not woken by peers' claims,
                    // plans and heartbeats.
                    let to = if signal.kind == "operator_note" {
                        envelope.get("to").cloned()
                    } else if signal.kind == "task.review_assigned" {
                        envelope["description"]
                            .as_str()
                            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                            .and_then(|document| document.get("reviewer").cloned())
                            .filter(serde_json::Value::is_string)
                    } else {
                        break;
                    };
                    match to {
                        None if signal.kind == "operator_note" => {
                            content_head = content_head.max(event.sequence);
                        }
                        None => {}
                        Some(serde_json::Value::String(to)) => {
                            if let Some(head) = notes.get_mut(to.as_str()) {
                                *head = (*head).max(event.sequence);
                            }
                        }
                        _ => continue,
                    }
                    break;
                }
            }
            _ => content_head = content_head.max(event.sequence),
        }
    }
    let due = projection
        .wake_leases
        .iter()
        .filter(|(_, lease)| {
            lease.cursor < content_head
                || actors
                    .get(&lease.armed_at_sequence)
                    .and_then(|actor| notes.get(actor))
                    .is_some_and(|head| lease.cursor < *head)
        })
        .map(|(session, lease)| DueLease {
            execution_id: execution.to_owned(),
            session_id: session.clone(),
            rendezvous_id: lease.rendezvous_id.clone(),
            armed_at_sequence: lease.armed_at_sequence,
        })
        .collect();
    Some(due)
}

/// #2714: the authors of the PRs this Runtime has seen opened, so a BLOCK or a merge can name the
/// lane to wake. Kept in `wake-webhook.authors.json` beside the events directory (small, newest
/// last, at most `AUTHORS_KEPT`); a PR opened before the webhook was turned on has no author here.
type Authors = std::collections::BTreeMap<u64, String>;
const AUTHORS_KEPT: usize = 500;

fn read_authors(path: &Path) -> Authors {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| {
            serde_json::from_str::<std::collections::BTreeMap<String, String>>(&text).ok()
        })
        .map(|raw| {
            raw.into_iter()
                .filter_map(|(pr, lane)| pr.parse::<u64>().ok().map(|pr| (pr, lane)))
                .collect()
        })
        .unwrap_or_default()
}

fn write_authors(path: &Path, authors: &Authors) {
    let mut kept: Vec<(&u64, &String)> = authors.iter().collect();
    kept.sort();
    let start = kept.len().saturating_sub(AUTHORS_KEPT);
    let raw: std::collections::BTreeMap<String, &String> = kept[start..]
        .iter()
        .map(|(pr, lane)| (pr.to_string(), *lane))
        .collect();
    if let Err(error) = std::fs::write(path, serde_json::json!(raw).to_string()) {
        eprintln!("wake webhook: authors not written ({})", error.kind());
    }
}

/// What a task record means for the webhook.
#[derive(Debug, PartialEq, Eq)]
enum Trigger {
    /// A step is ready for a lane.
    Step(graphhelm_wake_webhook::Step),
    /// A step would be ready, but the PR's author is not known here: said in the delivery log.
    NoAuthor(u64),
    /// Nothing to send (another record kind, or the author was only learned).
    Nothing,
}

/// The step a task record makes ready, or what it teaches (`task.pr_opened` records its PR's
/// author). `document` is the record's parsed description, `actor` the record's signer.
/// review_assigned -> its reviewer (review); BLOCK -> the PR's author (fix); APPROVE and
/// APPROVE-WITH-RISK -> the approving reviewer, who merges (merge); merged -> the PR's author, for
/// the lane's next item (next).
fn trigger_for(
    kind: &str,
    document: &serde_json::Value,
    actor: &str,
    authors: &mut Authors,
) -> Trigger {
    use graphhelm_wake_webhook::{Reason, Step};
    let pr = document["pr"].as_u64();
    let issue = document["taskId"]
        .as_str()
        .and_then(|id| id.strip_prefix("issue-"))
        .and_then(|number| number.parse::<u64>().ok());
    let step = |lane: &str, name: &str, skill: &str, head: &str, because: Reason| {
        Trigger::Step(Step {
            lane: lane.to_owned(),
            step: name.to_owned(),
            skill: skill.to_owned(),
            issue: issue.unwrap_or(0),
            pr,
            head: head.to_owned(),
            because,
        })
    };
    match (kind, pr, document["headSha"].as_str()) {
        ("task.pr_opened", Some(pr), _) => {
            if let Some(lane) = document["lane"].as_str() {
                authors.insert(pr, lane.to_owned());
            }
            Trigger::Nothing
        }
        ("task.review_assigned", Some(_), Some(head)) => match document["reviewer"].as_str() {
            Some(reviewer) => step(
                reviewer,
                "review",
                "blind-review",
                head,
                Reason::ReviewAssigned,
            ),
            None => Trigger::Nothing,
        },
        ("task.review_verdict", Some(pr), Some(head)) => {
            match document["verdict"].as_str().and_then(Verdict::from_word) {
                Some(Verdict::Block) => match authors.get(&pr) {
                    Some(author) => step(author, "fix", "implement", head, Reason::BlockVerdict),
                    None => Trigger::NoAuthor(pr),
                },
                Some(Verdict::Approve | Verdict::ApproveWithRisk) => {
                    step(actor, "merge", "merge", head, Reason::Approved)
                }
                _ => Trigger::Nothing,
            }
        }
        ("task.merged", Some(pr), _) => match (authors.get(&pr), document["mergeSha"].as_str()) {
            (Some(author), Some(sha)) => {
                step(author, "next", "graphhelm-resume", sha, Reason::Merged)
            }
            (None, Some(_)) => Trigger::NoAuthor(pr),
            _ => Trigger::Nothing,
        },
        _ => Trigger::Nothing,
    }
}

/// The task records of the tail that are worth opening for the webhook.
fn webhook_kind(kind: &str) -> bool {
    matches!(
        kind,
        "task.pr_opened" | "task.review_assigned" | "task.review_verdict" | "task.merged"
    )
}

/// #2714 slices 3-4: the steps made ready since the webhook cursor, each with the sequence of the
/// record that made it ready, and the last sequence read. The cursor (`wake-webhook.cursor` beside
/// the events directory) is the last sequence HANDLED; the first run takes the current head and
/// handles nothing, so history is never replayed to the lanes. Reads the tail only. `None` when the
/// feature is off or anything is unreadable: a webhook never fails or slows a route.
fn webhook_steps(
    events: &Path,
    execution: &str,
    sealing: Option<&crate::commands::execution::signal::SignalKeyring>,
    dispatcher: &graphhelm_wake_webhook::Dispatcher,
) -> Option<(Vec<(u64, graphhelm_wake_webhook::Step)>, u64)> {
    let dir = events.parent()?;
    let cursor_path = dir.join("wake-webhook.cursor");
    let store = crate::commands::event_store(events).ok()?;
    let stream = store
        .list_streams()
        .ok()?
        .into_iter()
        .find(|stream| stream.stream_id == execution)?;
    let cursor = std::fs::read_to_string(&cursor_path)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok());
    let (tail, head) = store
        .read_stream_after(
            &stream.scope,
            &stream.stream_id,
            cursor.unwrap_or(u64::MAX - 1),
        )
        .ok()?;
    let Some(cursor) = cursor else {
        // First run: start at the head, hand nothing to any lane.
        if let Some(head) = head {
            write_cursor(&cursor_path, head.next_sequence.saturating_sub(1));
        }
        return None;
    };
    let authors_path = dir.join("wake-webhook.authors.json");
    let mut authors = read_authors(&authors_path);
    let learned = authors.len();
    let mut steps = Vec::new();
    let mut opener = None;
    let mut last = cursor;
    for event in &tail {
        last = last.max(event.sequence);
        let EventKind::SignalRecorded(signal) = &event.kind else {
            continue;
        };
        if !webhook_kind(&signal.kind) {
            continue;
        }
        let opener = opener.get_or_insert_with(|| super::ports::build_opener(sealing).ok());
        let Some(opener) = opener else {
            continue;
        };
        for reference in &event.evidence_refs {
            let Ok(graphhelm_events::EvidenceRead::Available(sealed)) =
                store.sealed_evidence(&stream.scope, reference.evidence_id())
            else {
                continue;
            };
            if sealed.reference().content_sha256().as_str() != signal.envelope_sha256.as_str() {
                continue;
            }
            let Ok(plaintext) = tokio::runtime::Handle::current()
                .block_on(opener.open(stream.scope.clone(), &sealed))
            else {
                continue;
            };
            let Ok(envelope) =
                plaintext.expose(|bytes| serde_json::from_slice::<serde_json::Value>(bytes))
            else {
                continue;
            };
            let Some(document) = envelope["description"]
                .as_str()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
            else {
                continue;
            };
            let actor = envelope["source"]["id"].as_str().unwrap_or_default();
            match trigger_for(&signal.kind, &document, actor, &mut authors) {
                Trigger::Step(step) => steps.push((event.sequence, step)),
                Trigger::NoAuthor(pr) => {
                    dispatcher.log_skip(&format!("pr-{pr}:{}", signal.kind), "skipped-no-author");
                }
                Trigger::Nothing => {}
            }
            break;
        }
    }
    if authors.len() != learned {
        write_authors(&authors_path, &authors);
    }
    Some((steps, last))
}

/// The webhook cursor, written best-effort: a cursor that cannot be written is said (the error
/// kind only) and the next pass handles the same records again, which the delivery log's
/// once-per-id rule absorbs.
fn write_cursor(path: &Path, sequence: u64) {
    if let Err(error) = std::fs::write(path, sequence.to_string()) {
        eprintln!("wake webhook: cursor not written ({})", error.kind());
    }
}

/// One process-wide pass at a time: two sweeps must not both take the same cursor.
static WEBHOOK_PASS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Phase W: delivers the steps made ready since the cursor to their lane webhooks, in order. The
/// cursor moves past a step only when it is FINAL: accepted, a duplicate, rejected (a 401 or 404 is
/// logged and skipped), the lane off, or given up after too many failed passes. A step whose
/// delivery failed (a 503, a timeout, a refused connection) holds the cursor just before its own
/// record, so the next pass tries it again after its backoff, and the steps behind it wait.
/// Off (no config file under the project's `.graphhelm/`) costs one failed file read.
fn webhook_pass(
    events: &Path,
    execution: &str,
    sealing: Option<&crate::commands::execution::signal::SignalKeyring>,
) {
    let Some(dir) = events.parent() else {
        return;
    };
    let Some(dispatcher) = graphhelm_wake_webhook::Dispatcher::load(dir) else {
        return;
    };
    let Ok(_pass) = WEBHOOK_PASS.try_lock() else {
        return;
    };
    let Some((steps, last)) = webhook_steps(events, execution, sealing, &dispatcher) else {
        return;
    };
    let cursor = settle(&steps, last, |step| dispatcher.notify(step));
    write_cursor(&dir.join("wake-webhook.cursor"), cursor);
}

/// Where the cursor goes after delivering `steps` in order (`last` is the last sequence read): past
/// everything when every step is final, else just before the first step whose delivery failed; no
/// step behind a failed one is sent.
fn settle(
    steps: &[(u64, graphhelm_wake_webhook::Step)],
    last: u64,
    mut notify: impl FnMut(&graphhelm_wake_webhook::Step) -> Option<graphhelm_wake_webhook::Outcome>,
) -> u64 {
    for (sequence, step) in steps {
        if matches!(
            notify(step),
            Some(graphhelm_wake_webhook::Outcome::Failed(_))
        ) {
            return sequence.saturating_sub(1);
        }
    }
    last
}

/// The sweep: read the stream's projection, ring every lease whose cursor lies before the
/// latest relevant append, and record each consumption. Called fire-and-forget after a
/// mutation route's own append succeeded; every failure path is a silent no-op.
pub(super) async fn sweep(
    events: Arc<Path>,
    execution: String,
    sealing: Option<Arc<crate::commands::execution::signal::SignalKeyring>>,
) {
    // Phase W (#2714): new review assignments go to their lane's webhook, off this sweep's path.
    {
        let events_hook = events.clone();
        let execution_hook = execution.clone();
        let sealing_hook = sealing.clone();
        drop(tokio::task::spawn_blocking(move || {
            crate::commands::execution::timing::request("background.wake_webhook", || {
                webhook_pass(&events_hook, &execution_hook, sealing_hook.as_deref());
            });
        }));
    }
    // Phase 1 (blocking): what is due?
    let events_read = events.clone();
    let execution_read = execution.clone();
    let due = tokio::task::spawn_blocking(move || {
        crate::commands::execution::timing::request("background.wake_due", || {
            due_leases(&events_read, &execution_read, sealing.as_deref())
        })
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();

    if due.is_empty() {
        return;
    }

    // Phase 2: ring each rendezvous; the reason is what actually happened.
    // Concurrently: one sleeper still starting (or dead) must not hold up the others' rings.
    let mut rings = tokio::task::JoinSet::new();
    for lease in due {
        rings.spawn(async move {
            let reason = ring(&lease.rendezvous_id).await;
            (lease, reason)
        });
    }
    let mut consumptions = Vec::with_capacity(rings.len());
    while let Some(rung) = rings.join_next().await {
        if let Ok(consumption) = rung {
            consumptions.push(consumption);
        }
    }

    // Phase 3 (blocking): record every consumption in one follow-up append — through the
    // guarded recorder (hotfix #55): the consumption is validated against a FRESH replay
    // under the SAME open store handle that appends. The handle's exclusive lock spans
    // read and write, so the read-then-append window two racing sweeps used to slip
    // through (the double-consume that poisoned the factory pair store) no longer exists.
    let _ = tokio::task::spawn_blocking(move || {
        crate::commands::execution::timing::request("background.wake_record", || {
            test_only_phase3_delay();
            record_consumptions(&events, &execution, &consumptions);
        });
    })
    .await;
}

/// Test-only seam (#72): stretches the gap between the ring (phase 2) and the consume
/// append becoming durable (phase 3) by the number of milliseconds named in
/// `GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS`. The receipt is eventually-visible BY DESIGN;
/// this seam makes "eventually" a chosen number so a guard can prove it waits for the
/// CONDITION instead of winning a schedule. Production cost when the variable is absent:
/// one getenv, no delay. Setting it in production would only slow receipts down — it can
/// never reorder the phases or drop a consumption.
fn test_only_phase3_delay() {
    if let Some(delay) = std::env::var("GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
}

/// Records consumptions for leases that were STILL LIVE when this decision was taken,
/// silently dropping the rest (hotfix #55; the archived corrupted pair store is the incident
/// this guard exists for). Returns how many consumptions were recorded.
///
/// This used to claim that one store handle's exclusive lock made the re-validation and the
/// append atomic. It never did: the lock is taken and released per operation, and an open
/// handle holds none in between — at the parent of the read-concurrency change as much as
/// after it. Nothing was made worse by widening reads, and reverting that would restore
/// nothing here; what was missing was a guard, and the sentence describing one was standing
/// in for it.
pub(crate) fn record_consumptions(
    events: &Path,
    execution: &str,
    consumptions: &[(DueLease, WakeConsumeReason)],
) -> usize {
    record_consumptions_inner(events, execution, consumptions, &|| {})
}

/// The recorder proper, with a seam between DECIDING a lease is still live and PINNING the
/// sequence the consumption will append at.
///
/// `after_validation` runs once, exactly there, and production passes a no-op closure — so
/// the path a test exercises IS the production path, not a `cfg(test)` copy of it. A test
/// against a duplicated path proves something about the duplicate.
///
/// The seam is placed there because that is where the race lives: no lock is held at that
/// point (`with_lock` takes and releases per call, and an open handle holds none between
/// operations), so a rival writer can land there. Making the window executable is what turns
/// an argued interleaving into an observed one.
fn record_consumptions_inner(
    events: &Path,
    execution: &str,
    consumptions: &[(DueLease, WakeConsumeReason)],
    after_validation: &dyn Fn(),
) -> usize {
    let Ok(store) = crate::commands::event_store(events) else {
        return 0;
    };
    let Ok(streams) = store.list_streams() else {
        return 0;
    };
    let Some(stream) = streams
        .into_iter()
        .find(|stream| stream.stream_id == execution)
    else {
        return 0;
    };
    // The guard, in TWO blades that cover opposite sides of the same instant.
    //
    // Blade one, below: only a consumption whose lease is still live, AND IS STILL THE SAME
    // ARMING, may be recorded. That covers a rival who burned the lease BEFORE this read —
    // ours drops silently (its ring was at worst a spurious content-free byte the sleeper's
    // own re-read absorbs) — and it covers the case that has nothing to do with rivals: the
    // sleeper we just rang woke up and re-armed, and this capture is for the lease it replaced.
    //
    // The rendezvous is NOT compared and its absence is not a weakening — it is IMPLIED. A
    // lease is a pure function of its arming event, so the same session at the same arming
    // sequence is the same event, which has one rendezvous. Comparing it as well could never
    // decide anything, and keeping it would be worse than useless: with the rendezvous still
    // in the filter, a fold that wrote a constant into `armed_at_sequence` would leave the
    // rendezvous-swap guard GREEN, hiding the discriminator's own failure in the one test
    // built to see it. A redundant blade blinds the sabotage of the blade that matters.
    //
    // Blade two: the sequence this batch will append at is PINNED FROM THIS READ, not asked
    // for separately afterwards. That covers a rival who lands AFTER it: the pin goes stale
    // and the store's own sequence check refuses us.
    //
    // Both blades are needed and neither is redundant. Asking the store for the next
    // sequence in a second call was the defect: this read said the lease was live, that call
    // returned a number taken after a rival had already burned it, and the number was
    // genuinely current — so nothing downstream had anything to object to, and a second
    // consumption landed on a lease that no longer existed. The fold then refused every
    // later replay of the stream.
    // #2694: the kept fold and the events appended since, read once (`status::glance`); the
    // digest's head is the stream's last event of that same read.
    let Ok(glance) = crate::commands::execution::status::glance(
        &store,
        events,
        &stream.scope,
        &stream.stream_id,
        &graphhelm_events::ReadBudget::unbounded(),
    ) else {
        return 0;
    };
    // Pinned here, from the read this decision is made against. Equal to what `next_sequence`
    // would answer, because sequences are contiguous per stream.
    let next = glance
        .digest
        .head
        .as_ref()
        .map_or(1, |(sequence, _)| sequence + 1);
    let Ok(projection) = glance.projection else {
        return 0;
    };
    let still_live: Vec<&(DueLease, WakeConsumeReason)> = consumptions
        .iter()
        .filter(|(lease, _)| {
            projection
                .wake_leases
                .get(&lease.session_id)
                .is_some_and(|live| live.armed_at_sequence == lease.armed_at_sequence)
        })
        .collect();
    if still_live.is_empty() {
        return 0;
    }
    // The seam: the decision above is made, the append below has not happened yet.
    after_validation();
    let Ok(actor_id) = ActorId::parse("system-wake") else {
        return 0;
    };
    let actor = PersistedActor::new(PersistedActorType::System, actor_id);
    let mut batch = Vec::with_capacity(still_live.len());
    for (index, (lease, reason)) in still_live.iter().enumerate() {
        let Ok(idempotency) =
            OpaqueId::parse(format!("wake-consume-{next}-{index}-{}", lease.session_id))
        else {
            return 0;
        };
        let (Ok(execution_id), Ok(session_id)) = (
            OpaqueId::parse(lease.execution_id.clone()),
            OpaqueId::parse(lease.session_id.clone()),
        ) else {
            return 0;
        };
        batch.push(NewEvent::new(
            idempotency,
            actor.clone(),
            Sensitivity::Internal,
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id,
                session_id,
                reason: *reason,
                // The arming THIS CAPTURE named, not whatever is live now. The filter above has
                // already established they agree; recording the captured one is what lets a
                // later replay notice if they ever do not.
                captured_arming: Some(lease.armed_at_sequence),
            }),
            vec![],
            vec![],
        ));
    }
    let recorded = batch.len();
    let Ok(request) = graphhelm_events::PreparedAppend::new(
        stream.scope.clone(),
        OpaqueId::parse(stream.stream_id.to_owned()).expect("stream ids are wire-safe"),
        next,
        batch,
        vec![],
        vec![],
    ) else {
        return 0;
    };
    if store.append_atomic(&request).is_err() {
        return 0;
    }
    recorded
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::{
        EventKind, NewEvent, NodeState, NodeStateChanged, OpaqueId, Sensitivity, WakeLease,
    };

    /// The sweep's OWN read — `due_leases`, the function phase 1 of `sweep` runs — narrowed to
    /// the one session a fixture cares about.
    ///
    /// Two generations of this helper came before it, each found wanting the expensive way.
    /// Hand-written captures proved things about the fixture rather than about the production
    /// data flow: with `armed_at_sequence` typed in as a literal, a sabotage that corrupts the
    /// FOLD moves the live lease's value while the capture's stays put, so the two disagree and
    /// a stale capture is dropped — the guard passes, for a reason production would never
    /// reproduce. The replacement replayed the stream and copied the lease's fields "the way
    /// the sweep does" — a copy of the read, which cannot notice the read drifting: with
    /// `armed_at_sequence` corrupted INSIDE the sweep, every cell stayed green, because none of
    /// them ran the sweep's code (#258). It also skipped the sweep's due-predicate outright,
    /// handing back leases the sweep itself would never have captured.
    ///
    /// Now the fixture is judged by the production read, filter included. That is why every
    /// fixture appends content after arming — see `append_content` — and why this panics rather
    /// than returning an `Option`: a fixture whose lease the sweep would not capture is not a
    /// fixture for the sweep's recorder.
    fn captured_by_the_sweep(events: &std::path::Path, stream: &str, session: &str) -> DueLease {
        due_leases(events, stream, None)
            .expect("the sweep's read phase must find the fixture's stream")
            .into_iter()
            .find(|lease| lease.session_id == session)
            .expect(
                "the fixture's lease must be DUE under the sweep's own predicate: armed, with \
                 content appended past its cursor",
            )
    }

    /// One CONTENT event — a node moving state, the kind of append a sleeper is waiting on.
    fn content_event() -> EventKind {
        EventKind::NodeStateChanged(NodeStateChanged {
            simulation_id: OpaqueId::parse("simulation-1").unwrap(),
            node_id: OpaqueId::parse("node-1").unwrap(),
            previous_state: None,
            next_state: NodeState::Running,
        })
    }

    /// Appends one content event, because a lease armed on an otherwise-empty stream is live
    /// but NOT due: the sweep hands back only leases whose cursor lies before the latest
    /// non-wake append. Arm, content, sweep is the interleaving #55 describes, not an extra
    /// step — and it is the reason the sequence literals in these fixtures run 1, 2, 3 rather
    /// than 1, 2.
    fn append_content(
        events: &std::path::Path,
        scope: &graphhelm_protocols::RepositoryScope,
        stream: &str,
        key: &str,
        next: u64,
    ) {
        let store = crate::commands::event_store(events).unwrap();
        let request = graphhelm_events::PreparedAppend::new(
            scope.clone(),
            OpaqueId::parse(stream).unwrap(),
            next,
            vec![NewEvent::new(
                OpaqueId::parse(key).unwrap(),
                PersistedActor::new(
                    PersistedActorType::System,
                    ActorId::parse("system-test").unwrap(),
                ),
                Sensitivity::Internal,
                content_event(),
                vec![],
                vec![],
            )],
            vec![],
            vec![],
        )
        .unwrap();
        store.append_atomic(&request).unwrap();
    }

    /// #2694: the store's append reports its phases only while this thread times, and every
    /// step of the append is named: with timing off nothing is recorded (no clock is read).
    #[test]
    fn the_append_reports_its_phases_only_while_timing_is_on() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = crate::commands::execution::addressable_scope("exec-phases")
            .unwrap_or_else(|_| panic!("exec-phases is a valid execution id"));
        graphhelm_events::stop_lock_timing();
        let _stale = graphhelm_events::take_store_phases();
        append_content(events, &scope, "exec-phases", "phase-1", 1);
        append_content(events, &scope, "exec-phases", "phase-2", 2);
        assert!(
            graphhelm_events::take_store_phases().is_empty(),
            "with timing off no phase is recorded"
        );
        graphhelm_events::start_lock_timing();
        append_content(events, &scope, "exec-phases", "phase-3", 3);
        graphhelm_events::stop_lock_timing();
        let phases = graphhelm_events::take_store_phases();
        for name in [
            "append.load_state",
            "append.sync_loaded",
            "append.validate",
            "append.build",
            "append.evidence",
            "append.write",
            "append.fsync",
            "append.dirsync",
            "append.marker",
        ] {
            assert_eq!(
                phases.get(name).map(|(count, _)| *count),
                Some(1),
                "{name} is reported once per append"
            );
        }
    }

    /// The #55 interleaving, DETERMINISTIC: a sweep captured its due list, then a rival
    /// consumed the lease first (simulated by a direct consume append), then our sweep
    /// reaches the recorder. Unguarded, the second consumption lands and the fold refuses
    /// every later replay — the exact live failure that poisoned the factory pair store.
    /// Guarded, the recorder re-validates under its own append lock and records NOTHING.
    #[test]
    fn a_rival_consume_between_read_and_record_appends_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let store = crate::commands::event_store(events).unwrap();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-race").unwrap()),
        );
        let stream = OpaqueId::parse("exec-race").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        let append = |kind: EventKind, key: &str, next: u64| {
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                next,
                vec![NewEvent::new(
                    OpaqueId::parse(key).unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    kind,
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        };

        // The armed lease this sweep read.
        append(
            EventKind::WakeLease(WakeLease {
                execution_id: OpaqueId::parse("exec-race").unwrap(),
                session_id: OpaqueId::parse("session-r").unwrap(),
                cursor: 0,
                rendezvous_id: OpaqueId::parse("rdv-r").unwrap(),
                matures_in_seconds: None,
            }),
            "arm-r",
            1,
        );
        // Content past the cursor: what makes the lease due, and what the sweep is reacting to.
        append(content_event(), "content-r", 2);
        let captured = vec![(
            captured_by_the_sweep(events, "exec-race", "session-r"),
            WakeConsumeReason::StaleRendezvous,
        )];

        // The rival got there first.
        append(
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id: OpaqueId::parse("exec-race").unwrap(),
                session_id: OpaqueId::parse("session-r").unwrap(),
                reason: WakeConsumeReason::Rung,
                // A rival written the way history already contains them: no captured arming.
                captured_arming: None,
            }),
            "rival-consume",
            3,
        );
        drop(store); // release the handle so the recorder can take its own lock

        // Our sweep now reaches the recorder with its STALE capture.
        let recorded = record_consumptions(events, "exec-race", &captured);
        assert_eq!(
            recorded, 0,
            "a consumption whose lease a rival already burned must be dropped"
        );

        // The oracle the live incident failed: the stream still replays.
        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-race").unwrap();
        let replayed = graphhelm_events::replay(&scope, "exec-race", &history);
        assert!(
            replayed.is_ok(),
            "the stream must remain replayable: {replayed:?}"
        );
    }

    /// The sub-window #55's fix left conflict-UNDETECTED, made deterministic.
    ///
    /// The existing red above covers a rival that got there BEFORE our validation read: the
    /// still-live filter drops us. The CAS covers a rival that lands AFTER our sequence pin:
    /// our expected sequence is stale and the append is refused. Between those two sits a
    /// third window, and nothing guards it — our validation passed against a replay that
    /// predates the rival, and our pin is then taken AFTER it, so the sequence we ask for is
    /// genuinely current and the CAS has nothing to refuse. Both consumptions land, and the
    /// fold then refuses every later replay: #55's own corruption, reached through the door
    /// its fix left open.
    ///
    /// This also settles a claim both surviving commit messages make — that the handle's
    /// exclusive lock spans the validation and the write. It does not; the lock is taken and
    /// released per call, which is why a rival can be injected at the seam at all. If this
    /// test passes before the fix, that reading is wrong and the fix has no premise.
    #[test]
    fn a_rival_consume_between_validation_and_the_sequence_pin_appends_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-pin").unwrap()),
        );
        let stream = OpaqueId::parse("exec-pin").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );

        // The armed lease — live, and still live when our sweep validates it.
        {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                1,
                vec![NewEvent::new(
                    OpaqueId::parse("arm-p").unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-pin").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        cursor: 0,
                        rendezvous_id: OpaqueId::parse("rdv-p").unwrap(),
                        matures_in_seconds: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        }
        append_content(events, &scope, "exec-pin", "content-p", 2);

        let captured = vec![(
            captured_by_the_sweep(events, "exec-pin", "session-p"),
            WakeConsumeReason::StaleRendezvous,
        )];

        // The rival: another sweep that won the race, burning the same lease in the window
        // between our validation and our pin. It is a LEGITIMATE consumption at the instant
        // it lands — the lease is live and its rendezvous matches — so the test corrupts
        // nothing by hand. Whatever damage appears is the product's own.
        //
        // It opens its OWN handle while the recorder's is still alive, which is safe because
        // a live handle holds no lock between operations: `core/events/tests/read_concurrency`
        // has eight threads doing exactly this at once. The sibling guard above drops its
        // store before calling the recorder, and that drop reads as a requirement — it is not
        // one. Checked rather than inherited, because an unwritten coupling nobody could see
        // is what this whole area cost us.
        let rival = || {
            let store = crate::commands::event_store(events).unwrap();
            let next = store.next_sequence(&scope, "exec-pin").unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                next,
                vec![NewEvent::new(
                    OpaqueId::parse("rival-pin").unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                        execution_id: OpaqueId::parse("exec-pin").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        reason: WakeConsumeReason::Rung,
                        captured_arming: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        };

        let recorded = record_consumptions_inner(events, "exec-pin", &captured, &rival);
        assert_eq!(
            recorded, 0,
            "a lease a rival burned after our validation and before our pin must not be \
             consumed a second time"
        );

        // The oracle the live incident failed: one arming, at most one consumption, and the
        // stream still replays.
        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-pin").unwrap();
        let replayed = graphhelm_events::replay(&scope, "exec-pin", &history);
        assert!(
            replayed.is_ok(),
            "the stream must remain replayable: {replayed:?}"
        );

        // And the RIVAL's burn is the last word on this session.
        //
        // Without this, the guard rests on `recorded == 0` — which this recorder can return
        // from a dozen places, all but one of them for reasons that have nothing to do with
        // the property. That is fine for a red, which fails today for the stated reason, but
        // the moment the fix lands this becomes a REGRESSION guard, and there it would stay
        // green while the recorder was broken anywhere else at all. The rival's own append is
        // something a recorder that did nothing cannot produce, so asking for it separates
        // "our consumption was correctly refused" from "nothing got that far".
        let receipt = replayed
            .expect("replayable")
            .wake_last_consumed
            .get("session-p")
            .expect("the rival's consumption is on record")
            .sequence;
        assert_eq!(
            receipt, 3,
            "the rival's burn at #3 must be the last consumption on this session — if ours \
             had also landed it would be #4, and if the recorder had bailed early for an \
             unrelated reason there would be no receipt here at all"
        );
    }

    /// The rendezvous half of the filter, which nothing pinned until now.
    ///
    /// A session re-arms: the fold REPLACES its lease, so `session-p` is live again under a
    /// new rendezvous while a sweep still holds a capture naming the old one. Matching on the
    /// session alone would burn the replacement — a lease whose sleeper is waiting, consumed
    /// on the strength of a ring that went to a rendezvous nobody is listening to any more.
    ///
    /// The reason this guard is worth its lines: the fold cannot catch the mistake. A
    /// `wake_lease_consumed` event carries execution, session and reason — no rendezvous — so
    /// a replay sees a consumption of a live lease and accepts it. The recorder's own
    /// comparison is the ONLY thing standing here, and an only-defense with no test is one
    /// refactor away from silently not existing. Sabotage: compare sessions and drop the
    /// rendezvous check; this must fall, and nothing else does.
    ///
    /// The second assertion is the one that measures. `recorded == 0` alone would also hold
    /// if the recorder had refused for some unrelated reason, so it asks the finer question:
    /// is the REPLACEMENT still live afterwards.
    #[test]
    fn a_stale_capture_never_burns_the_lease_that_replaced_it() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-swap").unwrap()),
        );
        let stream = OpaqueId::parse("exec-swap").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );

        {
            let store = crate::commands::event_store(events).unwrap();
            let arm = |key: &str, rendezvous: &str, next: u64| {
                let request = graphhelm_events::PreparedAppend::new(
                    scope.clone(),
                    stream.clone(),
                    next,
                    vec![NewEvent::new(
                        OpaqueId::parse(key).unwrap(),
                        actor.clone(),
                        Sensitivity::Internal,
                        EventKind::WakeLease(WakeLease {
                            execution_id: OpaqueId::parse("exec-swap").unwrap(),
                            session_id: OpaqueId::parse("session-p").unwrap(),
                            cursor: 0,
                            rendezvous_id: OpaqueId::parse(rendezvous).unwrap(),
                            matures_in_seconds: None,
                        }),
                        vec![],
                        vec![],
                    )],
                    vec![],
                    vec![],
                )
                .unwrap();
                store.append_atomic(&request).unwrap();
            };
            // The lease a sweep captured.
            arm("arm-old", "rdv-old", 1);
        }
        append_content(events, &scope, "exec-swap", "content-swap", 2);

        // The sweep's capture, taken by phase 1 itself — naming the rendezvous it rang and the
        // arming it saw, both from the same read.
        let stale = vec![(
            captured_by_the_sweep(events, "exec-swap", "session-p"),
            WakeConsumeReason::Rung,
        )];

        // THEN the re-arm that replaced it.
        {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                3,
                vec![NewEvent::new(
                    OpaqueId::parse("arm-new").unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-swap").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        cursor: 0,
                        rendezvous_id: OpaqueId::parse("rdv-new").unwrap(),
                        matures_in_seconds: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        }

        let recorded = record_consumptions(events, "exec-swap", &stale);
        assert_eq!(
            recorded, 0,
            "a capture naming a rendezvous the session has since replaced must record nothing"
        );

        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-swap").unwrap();
        let projection = graphhelm_events::replay(&scope, "exec-swap", &history).unwrap();
        let live = projection
            .wake_leases
            .get("session-p")
            .expect("the re-armed lease is still live — the stale capture must not burn it");
        assert_eq!(
            live.rendezvous_id, "rdv-new",
            "and it is the REPLACEMENT that survived, under its own rendezvous"
        );
    }

    /// A capture from BEFORE the sleeper woke must not burn the lease it armed AFTER it.
    ///
    /// The sequence, and it is the factory's own: a lease is armed on rendezvous X; a sweep
    /// captures it and rings it; the sleeper wakes, works, and re-arms — on the SAME rendezvous,
    /// because our agents use fixed rendezvous ids; then the first sweep's delayed phase 3
    /// finally runs. Session matches. Rendezvous matches. Nothing else is compared, so the stale
    /// consumption burns a lease whose sleeper is asleep on it at that moment.
    ///
    /// Not hypothetical: the archived pair store has session `agente-a` arming `factory-a-1` at
    /// sequences 26, 30, 33 and 39 — four arms, one rendezvous. Precondition present there;
    /// incident not observed, because those arm/consume pairs happen to be ordered.
    ///
    /// Nothing downstream catches it. Consuming a LIVE lease is legal, so the fold accepts it
    /// and every replay succeeds — none of the noise the #55 family makes. The sleeper is simply
    /// never rung again: it blocks to its horizon and `wake-wait` exits 3, which reads as "my
    /// deadline passed and nothing happened", while the store's receipt for that session says
    /// `rung`. Two surfaces, one question, and the operator acts on the calm one.
    ///
    /// ASSERTION ORDER IS DELIBERATE. The legality check comes first because it holds in BOTH
    /// states and is what separates this defect from window 3: that one corrupts the stream and
    /// is loud, this one leaves the log perfectly legal and is silent. Asserted after the count,
    /// it would be masked by the count's own failure on every red run, and the clause that
    /// identifies WHICH defect we have would never be observed.
    ///
    /// That ordering is safe only because nothing in the moved region writes: the replay is a
    /// pure fold over a read, the count is captured before it, and the store open that travels
    /// with them republishes active markers only from `GraphVersionPublished` events — of which
    /// this fixture appends none. IF THIS FIXTURE EVER GAINS A PUBLISHED GRAPH VERSION, that
    /// open becomes a write and the ordering has to be re-checked before it can be trusted.
    #[test]
    fn a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-rearm").unwrap()),
        );
        let stream = OpaqueId::parse("exec-rearm").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );

        let arm = |key: &str, cursor: u64, next: u64| {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                next,
                vec![NewEvent::new(
                    OpaqueId::parse(key).unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-rearm").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        cursor,
                        // THE SAME rendezvous both times: the fixed-id convention.
                        rendezvous_id: OpaqueId::parse("rdv-fixed").unwrap(),
                        matures_in_seconds: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        };

        // The lease that exists when the sweep looks, and the content that makes it due.
        arm("arm-before", 0, 1);
        append_content(events, &scope, "exec-rearm", "content-rearm", 2);

        // THE SWEEP CAPTURES HERE — before the sleeper wakes. Taken by phase 1 itself, so a
        // corrupted fold moves this and the live lease TOGETHER just as it would in production.
        // Its ring already crossed, which is why the reason is Rung.
        let stale = vec![(
            captured_by_the_sweep(events, "exec-rearm", "session-p"),
            WakeConsumeReason::Rung,
        )];

        // Now the sleeper wakes and re-arms — same session, same rendezvous, cursor at the
        // content it just read. Both arms land BEFORE the recorder runs: this is the
        // OUT-of-window slice, the one the sequence pin cannot reach. An in-window variant
        // would pass because of that fix and prove nothing about this defect.
        arm("arm-after", 2, 3);

        let recorded = record_consumptions(events, "exec-rearm", &stale);

        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-rearm").unwrap();
        let replayed = graphhelm_events::replay(&scope, "exec-rearm", &history);
        assert!(
            replayed.is_ok(),
            "this defect is the SILENT one: burning a live lease is legal, so the log stays \
             replayable. A corrupt stream here would mean the fixture is reproducing window 3 \
             instead, and the shape needs re-deriving rather than relabelling: {replayed:?}"
        );

        assert_eq!(
            recorded, 0,
            "a consumption for the lease that existed before the wake must not be recorded \
             against the lease armed after it"
        );

        // The assertion that measures. `recorded == 0` alone would also hold if the recorder
        // had refused for an unrelated reason; the property is that the sleeper's CURRENT lease
        // survives, because that lease is what a future append will ring.
        assert!(
            replayed
                .expect("replayable")
                .wake_leases
                .contains_key("session-p"),
            "the lease the sleeper is asleep on must still be live — burned here, no later \
             append can ever ring it and the sleeper waits out its full horizon believing \
             nothing happened"
        );
    }

    /// The recorder still records when the lease IS live — the guard filters rivals'
    /// leftovers, never legitimate consumptions.
    #[test]
    fn a_live_lease_consumption_still_records() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-live").unwrap()),
        );
        {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                OpaqueId::parse("exec-live").unwrap(),
                1,
                vec![NewEvent::new(
                    OpaqueId::parse("arm-l").unwrap(),
                    PersistedActor::new(
                        PersistedActorType::System,
                        ActorId::parse("system-test").unwrap(),
                    ),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-live").unwrap(),
                        session_id: OpaqueId::parse("session-l").unwrap(),
                        cursor: 0,
                        rendezvous_id: OpaqueId::parse("rdv-l").unwrap(),
                        matures_in_seconds: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        }
        append_content(events, &scope, "exec-live", "content-l", 2);
        let recorded = record_consumptions(
            events,
            "exec-live",
            &[(
                captured_by_the_sweep(events, "exec-live", "session-l"),
                WakeConsumeReason::Rung,
            )],
        );
        assert_eq!(recorded, 1, "a live lease's consumption records normally");

        // And what it recorded is the RIGHT consumption, not merely one consumption.
        //
        // A count is the only thing this guard used to check, and measurement showed that
        // matters more here than anywhere else in the family: under a sabotage that stopped
        // the recorder dead, this was the ONLY guard in the whole #55 set that noticed. So
        // the chain's entire ability to see the recorder rests on this assertion — and a
        // count can only see a recorder that is DEAD, never one that is WRONG. A recorder
        // that burned this lease with the wrong reason satisfied it completely.
        //
        // Absent-by-key rather than an empty map: a later fixture with a second session would
        // make "empty" false while this lease's burn was still perfectly correct, and the
        // guard would then fail for something that is not the property.
        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-live").unwrap();
        let projection = graphhelm_events::replay(&scope, "exec-live", &history).unwrap();
        assert!(
            !projection.wake_leases.contains_key("session-l"),
            "the lease it consumed must be gone from the live set"
        );
        assert_eq!(
            projection
                .wake_last_consumed
                .get("session-l")
                .expect("the consumption is on record")
                .reason,
            WakeConsumeReason::Rung,
            "and the receipt must carry the reason the ring actually earned — a burn recorded \
             as a stale rendezvous would be the store asserting the sleeper died when it was \
             rung, which no count can tell apart"
        );
    }

    /// #1560: after every write, serve ran `fresh_stream_snapshot_on` and this sweep, and each
    /// folded the whole stream from sequence 1. On the owner's 32k-event `gh-team` that was
    /// 2.0-3.5 s of CPU per fold (#1576's measurement) behind every task record. They now resume
    /// serve's shared fold. The cache must hold this stream at the head each reader saw, and what
    /// each one folded must equal a full `replay` of the fresh history: a cached fold that
    /// answered for an older head would hand a wake or consumption decision a stale stream.
    /// #2694 (Kakashi's and Itachi's parity conditions): the head and the doorbell's content head
    /// `status` answers equal what a whole-history read says. `HistoryDigest` is kept only in
    /// serve's in-process fold cache (it is not serialised), so there is no old data to read 0.
    fn status_heads_equal_the_whole_history(
        events: &std::path::Path,
        scope: &graphhelm_protocols::RepositoryScope,
        stream: &str,
        session: &str,
        when: &str,
    ) {
        let whole = crate::commands::event_store(events)
            .unwrap()
            .read_replay_stream(scope, stream)
            .unwrap();
        let head = whole.last().map_or(0, |event| event.sequence);
        let content_head = whole
            .iter()
            .filter(|event| crate::commands::execution::wake::is_content(event))
            .map(|event| event.sequence)
            .max()
            .unwrap_or(0);
        let status = crate::commands::execution::wake::status(events, Some(stream), session)
            .unwrap_or_else(|_| panic!("status reads the stream {when}"));
        assert_eq!(status["head"], head, "head {when}");
        assert_eq!(status["contentHead"], content_head, "content head {when}");
    }

    #[test]
    fn post_write_readers_resume_the_shared_fold_at_the_fresh_head() {
        crate::commands::execution::status::enable_shared_fold_cache();
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        // The scope serve itself resolves an execution id to, so the write's reply reads this stream.
        let scope = crate::commands::execution::addressable_scope("exec-fold")
            .unwrap_or_else(|_| panic!("exec-fold is a valid execution id"));
        let held = || {
            crate::commands::execution::status::shared_fold_last_sequence(
                events,
                &scope,
                "exec-fold",
            )
        };
        let full = || {
            let store = crate::commands::event_store(events).unwrap();
            let history = store.read_replay_stream(&scope, "exec-fold").unwrap();
            serde_json::to_value(graphhelm_events::replay(&scope, "exec-fold", &history).unwrap())
                .unwrap()
        };
        let cached = || {
            let store = crate::commands::event_store(events).unwrap();
            let history = store.read_replay_stream(&scope, "exec-fold").unwrap();
            serde_json::to_value(
                crate::commands::execution::status::replay_cached(
                    events,
                    &scope,
                    "exec-fold",
                    &history,
                )
                .unwrap(),
            )
            .unwrap()
        };
        let lease = |next: u64| {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                OpaqueId::parse("exec-fold").unwrap(),
                next,
                vec![NewEvent::new(
                    OpaqueId::parse(format!("arm-{next}")).unwrap(),
                    PersistedActor::new(
                        PersistedActorType::System,
                        ActorId::parse("system-test").unwrap(),
                    ),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-fold").unwrap(),
                        session_id: OpaqueId::parse(format!("session-{next}")).unwrap(),
                        cursor: 0,
                        rendezvous_id: OpaqueId::parse(format!("rdv-{next}")).unwrap(),
                        matures_in_seconds: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        };
        lease(1);
        append_content(events, &scope, "exec-fold", "content-2", 2);
        lease(3);
        assert_eq!(held(), None, "nothing has folded this stream yet");

        due_leases(events, "exec-fold", None).expect("the sweep reads the stream");
        assert_eq!(
            held(),
            Some(3),
            "the sweep folded into serve's shared cache"
        );
        assert_eq!(
            cached(),
            full(),
            "what the sweep left equals a full rebuild"
        );
        // Cold (the first read of this stream folded it whole), with content at #2.
        status_heads_equal_the_whole_history(events, &scope, "exec-fold", "session-1", "cold");

        lease(4);
        let store = crate::commands::event_store(events).unwrap();
        let (head, _) = super::super::fresh_stream_snapshot_on(&store, events, "exec-fold", true);
        assert_eq!(head, Some(4));
        assert_eq!(
            held(),
            Some(4),
            "the write's reply resumed the cache to the new head"
        );
        assert_eq!(
            cached(),
            full(),
            "the resumed fold equals a full rebuild at the new head"
        );

        lease(5);
        due_leases(events, "exec-fold", None).expect("the sweep reads the stream");
        assert_eq!(
            held(),
            Some(5),
            "the sweep after the next write saw that write"
        );
        assert_eq!(cached(), full());

        // #2694: with the fold kept, a sweep after one more append reads the TAIL, never the
        // whole stream again: not for phase 1, not for the recorder.
        lease(6);
        crate::commands::execution::status::WHOLE_READS.with(|count| count.set(0));
        let due = due_leases(events, "exec-fold", None).expect("the sweep reads the stream");
        let first = due.into_iter().next().expect("a lease is due");
        let recorded =
            record_consumptions(events, "exec-fold", &[(first, WakeConsumeReason::Rung)]);
        assert_eq!(recorded, 1, "the consumption still records");
        assert_eq!(
            crate::commands::execution::status::WHOLE_READS.with(std::cell::Cell::get),
            0,
            "neither sweep phase may read the whole stream while the fold is kept"
        );
        // Itachi's condition on #2715: a lease armed in the tail AFTER the warm sweep is still
        // due, and its arming actor is the one that armed it.
        lease(8);
        let due = due_leases(events, "exec-fold", None).expect("the sweep reads the stream");
        assert!(
            due.iter().any(|lease| lease.session_id == "session-8"),
            "a lease armed in the tail after the warm sweep is due"
        );
        let store = crate::commands::event_store(events).unwrap();
        let glance = crate::commands::execution::status::glance(
            &store,
            events,
            &scope,
            "exec-fold",
            &graphhelm_events::ReadBudget::unbounded(),
        )
        .unwrap();
        let projection = glance.projection.unwrap();
        let actors = arming_actors(glance.digest.wake_leases(), &projection);
        assert_eq!(
            actors.get(&8),
            Some(&"system-test"),
            "the tail-armed lease carries its own arming actor"
        );

        // #2694: the lease arm and the lease status read the tail too. (This fixture has no
        // started execution, so `arm` stops after its load, which is the part counted here;
        // its read-back uses the same `glance` read.)
        crate::commands::execution::status::WHOLE_READS.with(|count| count.set(0));
        let status =
            crate::commands::execution::wake::status(events, Some("exec-fold"), "session-6")
                .unwrap_or_else(|_| panic!("status reads the stream"));
        assert_eq!(
            status["head"], 8,
            "status answers the stream's head from the tail read"
        );
        // After a tail resume, on a stream that also holds a wake_lease_consumed.
        let consumed = crate::commands::event_store(events)
            .unwrap()
            .read_replay_stream(&scope, "exec-fold")
            .unwrap()
            .iter()
            .any(|event| matches!(event.kind, EventKind::WakeLeaseConsumed(_)));
        assert!(consumed, "the fixture holds a wake_lease_consumed");
        status_heads_equal_the_whole_history(
            events,
            &scope,
            "exec-fold",
            "session-6",
            "after a tail resume",
        );
        let arming = crate::commands::execution::wake::Arming {
            session_id: "session-x",
            rendezvous_id: "rdv-x",
            cursor: None,
            matures_in_seconds: None,
        };
        let refused = crate::commands::execution::wake::arm(
            events,
            Some("exec-fold"),
            &arming,
            PersistedActor::new(
                PersistedActorType::System,
                ActorId::parse("system-test").unwrap(),
            ),
            OpaqueId::parse("arm-key").unwrap(),
        );
        assert!(refused.is_err(), "no execution started on this fixture");
        assert_eq!(
            crate::commands::execution::status::WHOLE_READS.with(std::cell::Cell::get),
            0,
            "arm and status must not read the whole stream while the fold is kept"
        );
    }

    // ---- #2714 slice 4: which record wakes which lane, and when the cursor may move.

    fn doc(fields: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::json!({
            "taskId": "issue-2714", "pr": 2790,
            "headSha": "0123456789abcdef0123456789abcdef01234567",
        });
        base.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        base
    }

    fn step_of(trigger: Trigger) -> (String, String, String, String) {
        match trigger {
            Trigger::Step(step) => (step.lane, step.step, step.skill, step.head),
            other => panic!("expected a step, got {other:?}"),
        }
    }

    #[test]
    fn each_record_wakes_the_lane_that_has_the_next_step() {
        let mut authors = Authors::new();
        // Learning the author wakes no one.
        assert_eq!(
            trigger_for(
                "task.pr_opened",
                &doc(serde_json::json!({"lane": "openbot-sasuke"})),
                "openbot-sasuke",
                &mut authors
            ),
            Trigger::Nothing
        );
        assert_eq!(
            authors.get(&2790).map(String::as_str),
            Some("openbot-sasuke")
        );
        let head = "0123456789abcdef0123456789abcdef01234567";
        let (lane, name, skill, at) = step_of(trigger_for(
            "task.review_assigned",
            &doc(serde_json::json!({"reviewer": "openbot-neji"})),
            "openbot-tsunade",
            &mut authors,
        ));
        assert_eq!(
            (lane.as_str(), name.as_str(), skill.as_str(), at.as_str()),
            ("openbot-neji", "review", "blind-review", head)
        );
        // BLOCK goes to the AUTHOR, not to the reviewer who wrote it.
        let (lane, name, skill, _) = step_of(trigger_for(
            "task.review_verdict",
            &doc(serde_json::json!({"verdict": "BLOCK"})),
            "openbot-neji",
            &mut authors,
        ));
        assert_eq!(
            (lane.as_str(), name.as_str(), skill.as_str()),
            ("openbot-sasuke", "fix", "implement")
        );
        // Both approvals go to the approving reviewer, who merges.
        for verdict in ["APPROVE", "APPROVE-WITH-RISK"] {
            let (lane, name, skill, _) = step_of(trigger_for(
                "task.review_verdict",
                &doc(serde_json::json!({"verdict": verdict})),
                "openbot-neji",
                &mut authors,
            ));
            assert_eq!(
                (lane.as_str(), name.as_str(), skill.as_str()),
                ("openbot-neji", "merge", "merge"),
                "{verdict}"
            );
        }
        // A merge goes to the author for the lane's next item, keyed by the merge commit.
        let sha = "89abcdef89abcdef89abcdef89abcdef89abcdef";
        let (lane, name, _, at) = step_of(trigger_for(
            "task.merged",
            &doc(serde_json::json!({"mergeSha": sha})),
            "openbot-neji",
            &mut authors,
        ));
        assert_eq!(
            (lane.as_str(), name.as_str(), at.as_str()),
            ("openbot-sasuke", "next", sha)
        );
        // Other records wake no one.
        assert_eq!(
            trigger_for(
                "task.claimed",
                &doc(serde_json::json!({})),
                "x",
                &mut authors
            ),
            Trigger::Nothing
        );
    }

    #[test]
    fn a_block_or_a_merge_for_a_pr_with_no_known_author_is_said_not_sent() {
        let mut authors = Authors::new();
        assert_eq!(
            trigger_for(
                "task.review_verdict",
                &doc(serde_json::json!({"verdict": "BLOCK"})),
                "openbot-neji",
                &mut authors
            ),
            Trigger::NoAuthor(2790)
        );
        assert_eq!(
            trigger_for(
                "task.merged",
                &doc(serde_json::json!({"mergeSha": "89abcdef89abcdef89abcdef89abcdef89abcdef"})),
                "openbot-neji",
                &mut authors
            ),
            Trigger::NoAuthor(2790)
        );
    }

    #[test]
    fn the_authors_survive_a_restart_and_stay_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wake-webhook.authors.json");
        let mut authors = Authors::new();
        for pr in 1..=(AUTHORS_KEPT as u64 + 20) {
            authors.insert(pr, format!("lane-{pr}"));
        }
        write_authors(&path, &authors);
        let back = read_authors(&path);
        assert_eq!(back.len(), AUTHORS_KEPT);
        assert!(!back.contains_key(&1) && back.contains_key(&(AUTHORS_KEPT as u64 + 20)));
    }

    fn a_step(lane: &str) -> graphhelm_wake_webhook::Step {
        graphhelm_wake_webhook::Step {
            lane: lane.to_owned(),
            step: "review".to_owned(),
            skill: "blind-review".to_owned(),
            issue: 1,
            pr: Some(2),
            head: "abcdef12".to_owned(),
            because: graphhelm_wake_webhook::Reason::ReviewAssigned,
        }
    }

    #[test]
    fn the_cursor_moves_only_past_steps_that_are_final() {
        use graphhelm_wake_webhook::Outcome;
        let steps = vec![(10, a_step("a")), (20, a_step("b")), (30, a_step("c"))];
        for (outcome, label) in [
            (Some(Outcome::Accepted), "accepted"),
            (Some(Outcome::Duplicate), "duplicate"),
            (Some(Outcome::Rejected(401)), "401"),
            (Some(Outcome::Rejected(404)), "404"),
            (Some(Outcome::GaveUp), "given up"),
            (None, "lane off or already delivered"),
        ] {
            let mut sent = 0;
            let cursor = settle(&steps, 40, |_| {
                sent += 1;
                outcome.clone()
            });
            assert_eq!(
                (cursor, sent),
                (40, 3),
                "{label}: final, the cursor passes all"
            );
        }
    }

    #[test]
    fn a_failed_delivery_holds_the_cursor_and_the_steps_behind_it_wait() {
        use graphhelm_wake_webhook::Outcome;
        let steps = vec![(10, a_step("a")), (20, a_step("b")), (30, a_step("c"))];
        let mut sent = Vec::new();
        let cursor = settle(&steps, 40, |step| {
            sent.push(step.lane.clone());
            (step.lane == "b").then(|| Outcome::Failed("503 after retries".to_owned()))
        });
        assert_eq!(
            cursor, 19,
            "just before the failed step's own record, so it is read again"
        );
        assert_eq!(sent, ["a", "b"], "nothing behind the failed step is sent");
    }
}
