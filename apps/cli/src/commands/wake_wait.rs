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
    // The id is OPAQUE and wire-safe or nothing: the rendezvous derives from it under the
    // fixed local prefix, so a hostile id cannot become a hostile path.
    if OpaqueId::parse(args.rendezvous_id.clone()).is_err() {
        return refuse(
            "--rendezvous-id must be a wire-safe opaque id",
            "/rendezvousId",
        );
    }
    if args.timeout == 0 {
        return refuse("--timeout must be at least 1 second", "/timeout");
    }

    match wait(&args.rendezvous_id, args.timeout) {
        WaitEnd::Rung => Outcome::success(COMMAND, serde_json::json!({"rung": true})),
        WaitEnd::TimedOut => Outcome {
            output: CommandOutput {
                ok: true,
                command: COMMAND,
                data: Some(serde_json::json!({"rung": false, "timedOut": true})),
                diagnostics: vec![],
            },
            exit_code: EXIT_TIMEOUT,
        },
        WaitEnd::Unusable(message) => refuse(&message, "/rendezvousId"),
    }
}

enum WaitEnd {
    Rung,
    TimedOut,
    Unusable(String),
}

#[cfg(windows)]
fn wait(rendezvous_id: &str, timeout_seconds: u64) -> WaitEnd {
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
fn wait(rendezvous_id: &str, timeout_seconds: u64) -> WaitEnd {
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
