//! `graphhelm wake-wait` (05g Task 4): the sidecar half of the doorbell. Creates the
//! platform rendezvous for an OPAQUE id (the same fixed-prefix derivation the serve's ring
//! uses — never a caller-supplied path), blocks for free, and exits by code alone:
//! `0` = rung, `3` = timeout, `2` (GHCLI017) = unusable arguments. **Content never
//! crosses**: whatever a hostile ringer writes, the bytes die here — the woken host learns
//! only THAT it should re-read its log, never WHAT anyone wanted it to think.

use graphhelm_protocols::{Diagnostic, OpaqueId};

use crate::args::WakeWaitArgs;
use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "wake.wait";
const MCP_WAKE_INVALID: &str = "GHCLI017_WAKE_INVALID";

/// Timeout is not a failure: the dead-man design MEANS timeouts happen routinely. Exit 3
/// distinguishes "nothing rang" from success (0) and refusal (2) for hook-friendly callers.
const EXIT_TIMEOUT: i32 = 3;

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
        return matured(&lease, true);
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
        WaitEnd::TimedOut => matured(&lease, false),
        WaitEnd::Unusable(message) => refuse(&message, "/sessionId"),
    }
}

/// The lease this session armed, and nothing else from the store.
struct OwnLease {
    rendezvous_id: String,
    matures_at: chrono::DateTime<chrono::Utc>,
}

/// The end of a wait that ended by the clock rather than by a ring.
///
/// `alreadyPast` is not decoration. It separates "the deadline you declared had ALREADY gone
/// by when you asked" from "it went by while you waited", and those are different situations
/// for the operator: the first means something went wrong before anyone looked. It is also
/// what makes the immediate answer testable — an assertion on elapsed time cannot tell a
/// zero-second wait from a one-second one, because process start-up costs more than either.
fn matured(lease: &OwnLease, already_past: bool) -> Outcome {
    Outcome {
        output: CommandOutput {
            ok: true,
            command: COMMAND,
            data: Some(serde_json::json!({
                "rung": false,
                "timedOut": true,
                "matured": true,
                "alreadyPast": already_past,
                "maturesAt": lease.matures_at.to_rfc3339(),
            })),
            diagnostics: vec![],
        },
        exit_code: EXIT_TIMEOUT,
    }
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
                "{session_id} has no live lease on this execution: a waiter waits on its OWN                  lease, never on whichever one it finds"
            ),
            "/sessionId",
        ));
    };
    let Some(matures_at) = lease.matures_at else {
        return Err(refuse(
            &format!(
                "{session_id} armed no bound, so nothing here promises to end the wait: arm                  again with maturesInSeconds"
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
                let mut sink = [0_u8; 64];
                let rung = matches!(stream.read(&mut sink), Ok(read) if read > 0);
                let _ = std::fs::remove_file(&path);
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
