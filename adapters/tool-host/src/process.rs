//! The scrubbed process primitive: argv-only spawning inside a workspace, with an allowlist
//! environment, deadline kill, and output caps. Every Tier 1 execution in this crate funnels
//! through [`run_in_workspace`]; there is no second spawn path to keep honest.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

pub struct ProcessLimits {
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

#[derive(Debug)]
pub struct CapturedProcess {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Whether the STDOUT cap was reached, on its own.
    ///
    /// #177: this and `stderr_truncated` were computed inside `run_in_workspace` and then thrown
    /// away — the struct carried only their OR, so a stage whose stderr was cut and whose stdout
    /// was whole read identically to the reverse. Nothing new is measured here; the boundary
    /// simply stopped discarding what the reader already knew. `ProcessResult` in
    /// `adapters/postgres-event-store/src/backup.rs` has carried the unfused pair all along, so
    /// this is the repository agreeing with itself rather than a new convention.
    pub stdout_truncated: bool,
    /// Whether the STDERR cap was reached, on its own. See `stdout_truncated`.
    pub stderr_truncated: bool,
    /// The OR of the two above, kept so existing readers do not change meaning under them.
    ///
    /// DERIVED, not a third fact: it stays exactly as informative as it always was, which is the
    /// point — anything that needs to know WHICH stream was cut must read the per-stream fields,
    /// and anything that only asks "was anything cut" keeps working unchanged.
    pub truncated: bool,
    pub timed_out: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("the program failed to spawn")]
    Spawn { source: std::io::Error },
    #[error("a declared environment name is refused: {name}")]
    ExtraEnvDenied { name: String },
    #[error("the workspace could not be prepared")]
    Prepare { source: std::io::Error },
    /// A workspace-configuration rule was violated. `rule` is a fixed rule name, never a path
    /// or caller content.
    #[error("workspace configuration refused: {rule}")]
    Config { rule: &'static str },
    /// A resolved path crossed a link or junction — containment refuses it.
    #[error("the path escapes the workspace through a link")]
    Escape,
    /// The routing layer refused a plan whose tier cannot carry its effect — defense in
    /// depth: `authorize` can never produce the shape, and the host refuses it anyway.
    #[error("the plan's tier cannot carry its effect")]
    TierViolation,
    /// The program is not an identity the broker can verify (#540): a relative name would be
    /// resolved by the OS's search order, which is how one name runs two binaries.
    #[error("the executable is not pinned: {rule}")]
    ExecutableNotPinned { rule: &'static str },
    /// The bytes at the pinned path do not hash to the expected value (#540). Carries both so
    /// the operator can decide between re-pinning and investigating; never falls back to
    /// running.
    #[error("the executable does not match its pin")]
    ExecutableMismatch { expected: String, actual: String },
    /// The index-snapshot copy no longer hashes to its recorded generation (#539). Carries both
    /// values — re-pin or investigate, never read anyway.
    #[error("the index snapshot does not match its pin")]
    SnapshotMismatch { expected: String, actual: String },
}

/// The fixed inheritance allowlist. Everything else the parent holds — passphrases, tokens,
/// profile paths — is structurally absent from the child. `HOME`/`USERPROFILE`/`TEMP`/`TMP`
/// are not inherited but REDIRECTED into the workspace so git and tools that insist on a home
/// write inside the sandbox and read no host config (`GIT_CONFIG_NOSYSTEM` closes the /etc
/// side). Note the contrast with the 05b gateway's allowlist, which deliberately keeps the
/// host `HOME`/`APPDATA` because official CLIs own their own auth: a tool workspace has no
/// auth of its own to keep, so Tier 1 is stricter by design.
const INHERITED: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "COMSPEC",
    "WINDIR",
];

/// The names the host redirects into the workspace rather than inheriting.
///
/// `CBM_CACHE_DIR` is #538 (D-042's last clause): a broker-run index provider's cache is confined
/// to the sandbox exactly the way HOME and TEMP are -- set by the host to a workspace path, denied
/// in `extra_env`, and structurally absent from inheritance via `env_clear`. Confinement by the
/// same mechanism as its siblings, so one sweep of this list answers "what does Tier 1 redirect".
const REDIRECTED: &[&str] = &["HOME", "USERPROFILE", "TEMP", "TMP", "CBM_CACHE_DIR"];

/// The fixed git posture: no system config, no prompts, no optional locks, and a synthetic
/// commit identity — `env_clear` plus an empty redirected HOME leaves git with no
/// `user.name`/`user.email` anywhere, and `git commit` would refuse with "Please tell me who
/// you are"; a fixed identity in the environment is the config-free, deterministic answer
/// (review finding 1 on this plan).
const FIXED_GIT: &[(&str, &str)] = &[
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_AUTHOR_NAME", "GraphHelm Tool Broker"),
    ("GIT_AUTHOR_EMAIL", "tools@graphhelm.invalid"),
    ("GIT_COMMITTER_NAME", "GraphHelm Tool Broker"),
    ("GIT_COMMITTER_EMAIL", "tools@graphhelm.invalid"),
];

/// Whether `name` may appear in `extra_env`. Case-insensitive: Windows environment names are
/// case-insensitive, so `Path` would shadow `PATH` just as surely. Refused classes: the
/// host's own passphrases (`GRAPHHELM_*`), and every name the host itself defines — an extra
/// `PATH` would swap program resolution out from under the lease's allowlist, an extra `HOME`
/// would undo the redirect (review finding 6).
fn extra_env_name_allowed(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if upper.starts_with("GRAPHHELM_") {
        return false;
    }
    if INHERITED.contains(&upper.as_str()) || REDIRECTED.contains(&upper.as_str()) {
        return false;
    }
    !FIXED_GIT.iter().any(|(fixed, _)| *fixed == upper)
}

/// Spawn a [`crate::verified::VerifiedExecutable`] in the workspace (#540): the program the
/// child runs IS the absolute path the verification hashed. Everything else — scrubbed
/// environment, deadline, caps — is [`run_in_workspace`], unchanged: one spawn funnel, one
/// verified doorway into it.
///
/// # Errors
/// Exactly [`run_in_workspace`]'s.
pub fn run_verified_in_workspace(
    root: &Path,
    verified: &crate::verified::VerifiedExecutable,
    arguments: &[String],
    extra_env: &BTreeMap<String, String>,
    path_prepend: &[PathBuf],
    stdin_bytes: Option<&[u8]>,
    limits: &ProcessLimits,
) -> Result<CapturedProcess, HostError> {
    run_in_workspace(
        root,
        &verified.path().display().to_string(),
        arguments,
        extra_env,
        path_prepend,
        stdin_bytes,
        limits,
    )
}

/// Spawns `program` with `arguments` (argv only — never a shell) in `root`, with the scrubbed
/// environment, and captures its output under the limits.
///
/// Output beyond `max_output_bytes` per stream is discarded but the pipe is always drained —
/// a full pipe with no reader deadlocks the child, so the readers never stop reading, they
/// only stop keeping. On deadline the child is killed and reaped; the readers then see EOF.
/// (A grandchild holding the pipe open past the kill would delay that EOF — no builtin tool
/// spawns one and Task 8's proof exercises the real tools; the 05b runtime adapter's
/// detach-on-timeout pattern is the documented escalation if one ever appears.)
///
/// # Errors
/// [`HostError::ExtraEnvDenied`] before anything runs; [`HostError::Prepare`]/
/// [`HostError::Spawn`] if the workspace dirs or the process cannot be created.
pub fn run_in_workspace(
    root: &Path,
    program: &str,
    arguments: &[String],
    extra_env: &BTreeMap<String, String>,
    path_prepend: &[PathBuf],
    stdin_bytes: Option<&[u8]>,
    limits: &ProcessLimits,
) -> Result<CapturedProcess, HostError> {
    for name in extra_env.keys() {
        if !extra_env_name_allowed(name) {
            return Err(HostError::ExtraEnvDenied { name: name.clone() });
        }
    }

    let home = root.join(".home");
    let tmp = root.join(".tmp");
    let cbm_cache = root.join(".cbm-cache");
    std::fs::create_dir_all(&home).map_err(|source| HostError::Prepare { source })?;
    std::fs::create_dir_all(&tmp).map_err(|source| HostError::Prepare { source })?;
    std::fs::create_dir_all(&cbm_cache).map_err(|source| HostError::Prepare { source })?;

    // Child PATH = path_prepend (host configuration, never caller input) ahead of the
    // parent's PATH, joined the OS way.
    let parent_path = std::env::var_os("PATH").unwrap_or_default();
    let composed_path = std::env::join_paths(
        path_prepend
            .iter()
            .map(PathBuf::as_path)
            .map(Path::to_path_buf)
            .chain(std::env::split_paths(&parent_path)),
    )
    // A join failure is a preparation failure, not a refused name (Task 5 review nit).
    .map_err(|_| HostError::Prepare {
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "unjoinable PATH entry"),
    })?;

    let mut command = std::process::Command::new(program);
    command
        .args(arguments)
        .current_dir(root)
        .env_clear()
        .stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in INHERITED {
        if *name == "PATH" {
            command.env("PATH", &composed_path);
        } else if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("HOME", &home).env("USERPROFILE", &home);
    command.env("TEMP", &tmp).env("TMP", &tmp);
    command.env("CBM_CACHE_DIR", &cbm_cache);
    for (name, value) in FIXED_GIT {
        command.env(name, value);
    }
    command.envs(extra_env);

    let mut child = command
        .spawn()
        .map_err(|source| HostError::Spawn { source })?;

    // Stdin is the one per-tool exception to null (a patch travels here). The write runs on
    // its own thread so an input larger than the OS pipe buffer can never wedge this thread
    // past the deadline; a kill mid-write surfaces as BrokenPipe, which is exactly the
    // uninteresting outcome — ignore it and let the disposition speak.
    let stdin_writer = stdin_bytes.map(|bytes| {
        let mut pipe = child.stdin.take().expect("stdin was piped");
        let owned = bytes.to_vec();
        std::thread::spawn(move || {
            use std::io::Write as _;
            let _ = pipe.write_all(&owned);
        })
    });

    let cap = limits.max_output_bytes;
    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");
    let reader = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            // #177 tail half: keep the HEAD *and* the TAIL, eliding the middle.
            //
            // Keeping only the head is biased against the reason anyone opens the log. In a red
            // suite the failing assertion and the `test result: FAILED` line are at the END, so a
            // head-only capture reliably discards the one part a triager needs. Both ends carry
            // real failures, each with a live instance from this repository's own work: a
            // toolchain fault (`invalid metadata for crate core`) prints as the build STARTS, and
            // the assertion that failed prints LAST. Tail-only would just move the blind spot.
            //
            // Memory stays bounded by `cap`: the head stops at `head_cap`, and the tail is a ring
            // holding at most `tail_cap`. The pipe is still drained to EOF either way -- dropping
            // bytes must never mean leaving them in the pipe, which is what would deadlock the
            // child.
            let head_cap = cap / 2;
            let tail_cap = cap - head_cap;
            let mut head = Vec::new();
            let mut tail: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
            let mut elided: u64 = 0;
            let mut chunk = [0_u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let mut rest = &chunk[..count];
                        let room = head_cap.saturating_sub(head.len());
                        if room > 0 {
                            let take = rest.len().min(room);
                            head.extend_from_slice(&rest[..take]);
                            rest = &rest[take..];
                        }
                        if rest.is_empty() {
                            continue;
                        }
                        // NOT `truncated = true` here (D's finding 1 on #582). Bytes past the head
                        // are not lost -- they go to the tail. Setting the flag here made every
                        // stream over `head_cap` claim data loss even when every byte survived,
                        // which under the production cap is every stream over 4 MiB. The flag is
                        // derived from `elided` below, where loss is actually known.
                        tail.extend(rest.iter().copied());
                        while tail.len() > tail_cap {
                            tail.pop_front();
                            elided = elided.saturating_add(1);
                        }
                    }
                }
            }
            let kept = if elided == 0 {
                // Everything after the head still fit: the original bytes, unmarked.
                head.extend(tail.iter().copied());
                head
            } else {
                // The marker is paid for out of the TAIL so the capture still fits `cap`.
                //
                // D's finding 2 on #582: the previous version formatted the marker TWICE and
                // budgeted with the first length. Popping a byte raises `elided`, which can carry
                // it across a power of ten and make the second marker one byte longer -- measured
                // at `cap = 7_388_638`, one byte over. The same defect in a worse shape: when the
                // cap is smaller than the marker itself, the loop exited on `!tail.is_empty()` and
                // appended the whole marker anyway -- 52 bytes under a 40-byte cap.
                //
                // One cause: the budgeted length and the emitted length came from different values
                // of `elided`. So the loop now re-formats each round and exits only when the marker
                // it will actually emit fits beside the tail it will actually keep.
                let mut marker = elision_marker(elided);
                while head.len() + marker.len() + tail.len() > cap && !tail.is_empty() {
                    tail.pop_front();
                    elided = elided.saturating_add(1);
                    marker = elision_marker(elided);
                }
                if head.len() + marker.len() + tail.len() > cap {
                    // The tail is empty and the marker alone still does not fit: the cap is smaller
                    // than the sentence. Shorten the marker rather than exceed the budget -- the
                    // capture's size is a promise to the caller, the marker's completeness is not.
                    marker.truncate(cap.saturating_sub(head.len()));
                }
                head.extend_from_slice(&marker);
                head.extend(tail.iter().copied());
                debug_assert!(head.len() <= cap, "the capture must never exceed its cap");
                head
            };
            let truncated = elided > 0;
            (kept, truncated)
        })
    };
    let stdout_reader = reader(Box::new(stdout_pipe));
    let stderr_reader = reader(Box::new(stderr_pipe));

    // The 05b runtime-adapter pattern: poll every 50 ms against the deadline; on expiry kill
    // and reap, never leaving a zombie.
    let deadline = Instant::now() + limits.timeout;
    let mut timed_out = false;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let status = child.wait().ok();
                    timed_out = true;
                    break status;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };

    if let Some(writer) = stdin_writer {
        let _ = writer.join();
    }
    let (stdout, stdout_truncated) = stdout_reader.join().unwrap_or_default();
    let (stderr, stderr_truncated) = stderr_reader.join().unwrap_or_default();

    Ok(CapturedProcess {
        exit_code: exit_status.and_then(|status| status.code()),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        truncated: stdout_truncated || stderr_truncated,
        timed_out,
    })
}

/// The elision sentence, built in ONE place so the budgeted length and the emitted length can never
/// be computed from different values of `elided` (D's finding 2 on #582).
fn elision_marker(elided: u64) -> Vec<u8> {
    format!(
        "
[... {elided} bytes elided ...]
"
    )
    .into_bytes()
}
