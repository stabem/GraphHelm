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
const REDIRECTED: &[&str] = &["HOME", "USERPROFILE", "TEMP", "TMP"];

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
    std::fs::create_dir_all(&home).map_err(|source| HostError::Prepare { source })?;
    std::fs::create_dir_all(&tmp).map_err(|source| HostError::Prepare { source })?;

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
            let mut kept = Vec::new();
            let mut truncated = false;
            let mut chunk = [0_u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let room = cap.saturating_sub(kept.len());
                        let take = count.min(room);
                        kept.extend_from_slice(&chunk[..take]);
                        if take < count {
                            truncated = true;
                        }
                    }
                }
            }
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
        truncated: stdout_truncated || stderr_truncated,
        timed_out,
    })
}
