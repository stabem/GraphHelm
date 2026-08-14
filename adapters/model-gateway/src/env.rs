//! The environment allowlist a native-runtime CLI is spawned under, and the composition helper
//! that builds it from this process's own environment.
//!
//! Shared by `runtime.rs`'s real spawn path (`RuntimeAdapter::invoke`) and
//! `apps/cli/src/commands/gateway/probe.rs`'s `--version` liveness check (PR review MEDIUM 14):
//! the probe used to hand-copy [`ENV_ALLOWLIST`] verbatim into its own private constant with a
//! doc comment promising to keep the two in sync by hand. Importing this module instead makes
//! that promise the compiler's problem, not an operator's.

/// Environment variables copied verbatim from this process's own environment into a spawned
/// runtime's environment, on top of `env_clear()` — nothing else crosses that boundary except
/// whatever a caller passes as `extra_env` to [`compose`].
///
/// Every entry here is an ordinary interpreter/OS-location variable an official CLI needs to find
/// its own config directory, its own auth store, or a working shell environment — never a
/// gateway or broker value (§6.3: "their credential, their store, never ours"). The plan's own
/// list omitted `PATHEXT`; it is included here because Windows' `CreateProcess`/`Command::spawn`
/// program resolution (searching `PATH` for `claude`/`codex` without an explicit `.exe`
/// extension) depends on it being set, exactly the same class of "the CLI needs its own platform
/// plumbing" reasoning that justifies every other entry — this is a deliberate addition beyond
/// the plan's literal list, not an oversight.
pub const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "COMSPEC",
    "WINDIR",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "HOME",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
];

/// Builds the `(key, value)` pairs a spawned process's environment should carry: every
/// [`ENV_ALLOWLIST`] entry actually set in this process's own environment, in allowlist order,
/// followed by `extra_env` verbatim (test-only steering, e.g. `FAKE_RUNTIME_MODE` — see
/// `runtime.rs`'s `RuntimeAdapter::extra_env` doc comment). Composing the *pairs* is what is
/// shared here, not `Command` construction itself: `probe.rs`'s liveness check and `runtime.rs`'s
/// real invocation still build their own `Command`s differently (piped stdio vs null, the route's
/// configured `args` vs a fixed `--version`) — callers still call `env_clear()` themselves before
/// applying what this returns.
#[must_use]
pub fn compose(extra_env: &[(String, String)]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = ENV_ALLOWLIST
        .iter()
        .filter_map(|key| {
            std::env::var_os(key)
                .map(|value| ((*key).to_owned(), value.to_string_lossy().into_owned()))
        })
        .collect();
    pairs.extend(extra_env.iter().cloned());
    pairs
}
