//! `graphhelm init` (#1062): the first command a stranger runs in a project.
//!
//! Before this command the first run was four hand-assembled steps spread over three documents:
//! `serve` minted the token (but only when it started), `gateway keyring init` created the
//! sealing key (but only if the operator knew the Studio's message box would be refused without
//! it), the MCP registration lived in `examples/chat-surface/` as a file to edit by hand, and
//! `.graphhelm/` reached `.gitignore` when somebody remembered. `init` does all of it, once, in
//! one directory, and prints the commands that come next.
//!
//! What it provisions under `<project>/.graphhelm/`:
//!
//! - `events/` — the store `serve` and `execution start` share.
//! - `events.token` — the bearer token, through `secret_file::ensure_token`: the SAME function
//!   `serve` reads with, so the token `init` minted is the token `serve` serves. Never rotated.
//! - `serve.key` — 32 OS-random bytes, lowercase hex, `create_new`, `0o600` on Unix, never
//!   overwritten. The value `GRAPHHELM_EVENTS_KEY` must carry when `serve` starts.
//! - `keyring/` — created by `gateway::keyring::create` with the passphrase read from
//!   `serve.key`. A keyring that already holds the key is success (`existing`), because a second
//!   `init` is a repeated setup step, not a rotation request.
//! - `codex.config.toml` — the `[mcp_servers.graphhelm]` snippet, when Codex is registered.
//!
//! And beside it: `<project>/.mcp.json` (merged, never clobbered) for Claude Code, and the
//! `.graphhelm/` line in `<project>/.gitignore` when the project is a git work tree.
//!
//! **What never appears on stdout or stderr:** the token's bytes and the key's bytes. `data`
//! reports paths and `created`/`existing` per artifact; the printed `next` commands read the key
//! from its file. `apps/cli/tests/init_cli.rs` greps the output for both values.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use graphhelm_protocols::Diagnostic;
use serde_json::{Map, Value, json};

use crate::args::{Harness, InitArgs};
use crate::commands::gateway::keyring::{CreateError, SEALING_KEY_ENVIRONMENT};
use crate::commands::{gateway, secret_file};
use crate::output::Outcome;

const COMMAND: &str = "init";
const SOURCE: &str = "init-cli";
const ARGUMENT_CODE: &str = crate::error_codes::GHCLI001_ARGUMENT_INVALID;
const REFUSED_CODE: &str = crate::error_codes::GHCLI027_INIT_REFUSED;

pub(super) const RUNTIME_DIRECTORY: &str = ".graphhelm";
const EVENTS_DIRECTORY: &str = "events";
pub(super) const KEY_FILE: &str = "serve.key";
pub(super) const KEYRING_DIRECTORY: &str = "keyring";
const CODEX_SNIPPET_FILE: &str = "codex.config.toml";
const CLAUDE_CODE_FILE: &str = ".mcp.json";
pub(super) const GITIGNORE_FILE: &str = ".gitignore";
const MCP_SERVER_NAME: &str = "graphhelm";
const MCP_ACTOR: &str = "agent-chat";
/// The block appended to `.gitignore`. The comment line is what makes a second run recognize its
/// own work without parsing; the pattern is what git reads.
const GITIGNORE_BLOCK: &str = "\n# GraphHelm Runtime working directory: bearer token, event store, sealing key.\n.graphhelm/\n";
/// `.mcp.json` and `.gitignore` are small hand-edited files; anything past this is not one, and
/// is refused before being read into memory (PR #1070 review: the ignore file had no bound).
const MAX_EDITED_FILE_BYTES: u64 = 1024 * 1024;

/// The per-family redaction-safe failure, same shape as `serve`'s and `gateway`'s own. The fields
/// are readable by `gateway setup` (#1139), which reuses this module's provisioning and reports
/// the same message under its own code family.
pub(super) struct Failure {
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) pointer: &'static str,
}

impl Failure {
    fn into_outcome(self) -> Outcome {
        Outcome::domain(
            COMMAND,
            vec![Diagnostic::error(
                self.code,
                self.message,
                self.pointer,
                SOURCE,
            )],
        )
    }
}

fn argument(message: &str, pointer: &'static str) -> Failure {
    Failure {
        code: ARGUMENT_CODE,
        message: message.to_owned(),
        pointer,
    }
}

fn refused(message: &str, pointer: &'static str) -> Failure {
    Failure {
        code: REFUSED_CODE,
        message: message.to_owned(),
        pointer,
    }
}

pub fn run(args: &InitArgs) -> Outcome {
    match execute(args) {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(failure) => failure.into_outcome(),
    }
}

/// Where a path came from: made by this run or found already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    Created,
    Existing,
    /// `.mcp.json`: the file existed and the `graphhelm` entry was added or replaced in it.
    Merged,
    /// `.gitignore`: the file existed and the block was appended to it.
    Appended,
    /// `.gitignore`: the project is not a git work tree, so nothing was written.
    NotAGitWorkTree,
}

impl State {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Existing => "existing",
            Self::Merged => "merged",
            Self::Appended => "appended",
            Self::NotAGitWorkTree => "not_a_git_work_tree",
        }
    }
}

impl From<secret_file::Provision> for State {
    fn from(provision: secret_file::Provision) -> Self {
        match provision {
            secret_file::Provision::Created => Self::Created,
            secret_file::Provision::Existing => Self::Existing,
        }
    }
}

/// `data` shows every path RELATIVE TO THE PROJECT, with `/` on every platform: normal CLI JSON
/// must not expose user-home paths (AGENTS.md security rules), and `~/project` is the normal
/// layout. The absolute spellings live only where a shell needs them — inside the `next` command
/// strings — because a stranger copies those verbatim; that exception is deliberate and named in
/// `GETTING_STARTED.md` (PR #1070 review).
fn artifact(relative: &str, state: State) -> Value {
    json!({ "path": relative, "state": state.as_str() })
}

fn under_root(name: &str) -> String {
    format!("{RUNTIME_DIRECTORY}/{name}")
}

/// A side-effect-free description of init's writes. Adoption may review this description;
/// it must never call the legacy writer before its backup/journal boundary.
pub(super) struct Provisioning {
    project: PathBuf,
    root: PathBuf,
    events: PathBuf,
    bind: SocketAddr,
    harnesses: Vec<Harness>,
}

impl Provisioning {
    pub(super) fn public_description(&self) -> Value {
        let mut writes = vec![
            json!({"path": under_root(EVENTS_DIRECTORY), "operation": "ensure_directory"}),
            json!({"path": under_root(&format!("{EVENTS_DIRECTORY}/{}", secret_file::token_path(&self.events).file_name().unwrap_or_default().to_string_lossy())), "operation": "ensure_private_token"}),
            json!({"path": under_root(KEY_FILE), "operation": "ensure_private_sealing_key"}),
            json!({"path": under_root(KEYRING_DIRECTORY), "operation": "ensure_keyring"}),
            json!({"path": GITIGNORE_FILE, "operation": "append_ignore_if_git"}),
        ];
        for harness in &self.harnesses {
            writes.push(match harness {
                Harness::ClaudeCode => json!({"path": CLAUDE_CODE_FILE, "operation": "merge_mcp_registration"}),
                Harness::Codex => json!({"path": under_root(CODEX_SNIPPET_FILE), "operation": "write_registration_snippet"}),
            });
        }
        json!({"root": RUNTIME_DIRECTORY, "bind": self.bind.to_string(), "writes": writes})
    }
}

pub(super) fn describe(args: &InitArgs) -> Result<Provisioning, Failure> {
    let bind = parse_bind(&args.bind)?;
    validate_key_id(&args.key_id)?;
    let project = resolve_project(args.project.as_deref())?;
    let root = project.join(RUNTIME_DIRECTORY);
    refuse_symlink(&root, "/root")?;
    let events = root.join(EVENTS_DIRECTORY);
    let harnesses = if args.harness.is_empty() {
        detect_harnesses(&project)
    } else {
        let mut chosen = args.harness.clone();
        chosen.dedup();
        chosen
    };
    Ok(Provisioning {
        project,
        root,
        events,
        bind,
        harnesses,
    })
}

fn execute(args: &InitArgs) -> Result<Value, Failure> {
    let Provisioning {
        project,
        root,
        events,
        bind,
        harnesses,
    } = describe(args)?;
    let events_state = ensure_directory(&events, "/events")?;

    // THE token: `serve`'s own function, so what `init` mints is what `serve` reads. The value is
    // dropped here and never bound to a name this function could format.
    let (token_state, _) =
        secret_file::ensure_token(&events).map_err(|error| refused(error.message(), "/token"))?;
    let token_path = secret_file::token_path(&events);

    let key_path = root.join(KEY_FILE);
    let keyring = root.join(KEYRING_DIRECTORY);
    let sealing = ensure_sealing_keyring(&root, &args.key_id)?;
    let (key_state, keyring_state) = (sealing.key_state, sealing.keyring_state);

    let gitignore_path = project.join(GITIGNORE_FILE);
    let gitignore_state = ensure_gitignore(&project, &gitignore_path)?;

    let url = format!("http://{bind}");
    let mut registrations = Vec::new();
    for harness in harnesses {
        registrations.push(match harness {
            Harness::ClaudeCode => {
                let path = project.join(CLAUDE_CODE_FILE);
                let state = ensure_claude_code(&path, &url, &token_path)?;
                json!({
                    "harness": "claude-code",
                    "path": CLAUDE_CODE_FILE,
                    "state": state.as_str(),
                    "note": "Claude Code reads this file from the project root; restart the session to pick it up.",
                })
            }
            Harness::Codex => {
                let path = root.join(CODEX_SNIPPET_FILE);
                let state = ensure_codex(&path, &url, &token_path)?;
                json!({
                    "harness": "codex",
                    "path": under_root(CODEX_SNIPPET_FILE),
                    "state": state.as_str(),
                    "note": "Append this file's contents to ~/.codex/config.toml; init never writes to your home directory.",
                })
            }
        });
    }

    let token_name = token_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(json!({
        // The project as the operator spelled it (or `.` when defaulted): never made absolute
        // here, so a home directory reaches this envelope only if the operator typed it.
        "project": args
            .project
            .as_deref()
            .map_or_else(|| ".".to_owned(), |given| given.to_string_lossy().into_owned()),
        "root": RUNTIME_DIRECTORY,
        "bind": bind.to_string(),
        "events": artifact(&under_root(EVENTS_DIRECTORY), events_state),
        "token": artifact(&under_root(&token_name), token_state.into()),
        "key": {
            "path": under_root(KEY_FILE),
            "state": key_state.as_str(),
            "environment": SEALING_KEY_ENVIRONMENT,
        },
        "keyring": {
            "path": under_root(KEYRING_DIRECTORY),
            "keyId": args.key_id,
            "state": keyring_state.as_str(),
        },
        "gitignore": artifact(GITIGNORE_FILE, gitignore_state),
        "harnesses": registrations,
        "next": next_steps(&NextPaths {
            bind: &bind,
            root: &root,
            events: &events,
            key: &key_path,
            keyring: &keyring,
            key_id: &args.key_id,
        }),
    }))
}

/// What [`ensure_sealing_keyring`] provisioned: the two states `init` reports, and the passphrase
/// itself for a caller that goes on to open the keyring (`gateway setup` opens the Credential
/// Broker with it). `init` drops the passphrase unused.
pub(super) struct SealingKeyring {
    pub(super) key_state: State,
    pub(super) keyring_state: State,
    pub(super) passphrase: graphhelm_events::SecretBytes,
}

/// `serve.key` and the keyring under `root`, exactly as `init` provisions them — the ONE place
/// both `init` and `gateway setup` (#1139) go through, so the two commands cannot disagree on
/// which file is the passphrase, which directory is the keyring, or what a second run means
/// (`existing`, never a rotation). The key file is created only when absent (`create_new`), the
/// keyring only when the passphrase does not already open `key_id` in it.
pub(super) fn ensure_sealing_keyring(root: &Path, key_id: &str) -> Result<SealingKeyring, Failure> {
    let key_path = root.join(KEY_FILE);
    let (key_state, key_hex) = secret_file::ensure(&key_path, "sealing key")
        .map_err(|error| refused(error.message(), "/key"))?;
    let key_hex = zeroize::Zeroizing::new(key_hex.into_bytes());
    let passphrase = gateway::decode_key(&key_hex, KEY_FILE)
        .map_err(|failure| refused(&failure.message, "/key"))?;
    // `create` consumes a passphrase; the broker needs one too. Copying it is not a widening:
    // the plaintext is already in this process, and the copy lives no longer than the caller.
    let for_caller = passphrase.expose(|bytes| graphhelm_events::SecretBytes::new(bytes.to_vec()));

    let keyring = root.join(KEYRING_DIRECTORY);
    refuse_symlink(&keyring, "/keyring")?;
    ensure_owner_only_directory(&keyring, "/keyring")?;
    let keyring_state = match gateway::keyring::create(&keyring, key_id, passphrase) {
        Ok(()) => State::Created,
        Err(CreateError::AlreadyHoldsKey) => State::Existing,
        Err(CreateError::Uncreatable) => {
            return Err(refused(
                "the keyring could not be created, or exists and does not open with the key in serve.key under this --key-id; pass the --key-id it was created with, or move the keyring directory aside (on Unix the directory must also be owner-only, 0700)",
                "/keyring",
            ));
        }
    };
    Ok(SealingKeyring {
        key_state: key_state.into(),
        keyring_state,
        passphrase: for_caller,
    })
}

/// `--key-id` is interpolated into the printed `next` commands, so it is restricted to a vocabulary
/// no shell interprets (`[A-Za-z0-9._-]`, non-empty) rather than quoted per shell: `OpaqueId`
/// would accept `$(id)` or `x;id`, and a copied command would then run them (PR #1070 review).
pub(super) fn validate_key_id(key_id: &str) -> Result<(), Failure> {
    let acceptable = !key_id.is_empty()
        && key_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if acceptable {
        Ok(())
    } else {
        Err(argument(
            "--key-id may contain only letters, digits, '.', '_' and '-'; it is written into shell commands",
            "/key_id",
        ))
    }
}

/// A `.graphhelm` (or `.graphhelm/keyring`) that is a symbolic link points the whole provisioning
/// somewhere else: `create_dir_all` follows it, the token and key land outside the project, and
/// `set_permissions(0o700)` tightens a directory nobody asked to be touched. Refused, never
/// followed (PR #1070 review; AGENTS.md: repository files are untrusted input). A missing path is
/// fine — it is about to be created.
pub(super) fn refuse_symlink(path: &Path, pointer: &'static str) -> Result<(), Failure> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(refused(
            "this path is a symbolic link; init provisions only real directories and files beneath the project",
            pointer,
        )),
        _ => Ok(()),
    }
}

/// `serve`'s own rule (`parse_loopback_bind`), plus "not port 0": `init` writes the address into
/// files a harness will dial, and an OS-assigned port cannot be written down in advance.
fn parse_bind(bind: &str) -> Result<SocketAddr, Failure> {
    let address: SocketAddr = bind
        .parse()
        .map_err(|_| argument("--bind must be a numeric host:port address", "/bind"))?;
    if !address.ip().is_loopback() {
        return Err(argument(
            "--bind must name a loopback address; the Public Runtime API is never exposed beyond localhost",
            "/bind",
        ));
    }
    if address.port() == 0 {
        return Err(argument(
            "--bind must name a fixed port; the address is written into the harness registration",
            "/bind",
        ));
    }
    Ok(address)
}

/// Absolute, but not canonical: `std::path::absolute` keeps the operator's own spelling, where
/// `canonicalize` on Windows would write `\\?\C:\...` into `.mcp.json` for a harness to choke on.
pub(super) fn resolve_project(project: Option<&Path>) -> Result<PathBuf, Failure> {
    let path = match project {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()
            .map_err(|_| refused("the current directory could not be determined", "/project"))?,
    };
    let metadata = std::fs::metadata(&path)
        .map_err(|_| refused("--project does not name an existing directory", "/project"))?;
    if !metadata.is_dir() {
        return Err(refused("--project is not a directory", "/project"));
    }
    std::path::absolute(&path)
        .map_err(|_| refused("--project could not be made absolute", "/project"))
}

fn ensure_directory(path: &Path, pointer: &'static str) -> Result<State, Failure> {
    refuse_symlink(path, pointer)?;
    let state = if path.is_dir() {
        State::Existing
    } else {
        State::Created
    };
    std::fs::create_dir_all(path)
        .map_err(|_| refused("the directory could not be created", pointer))?;
    Ok(state)
}

/// The keyring directory, owner-only. `SealedKeyProvider::create`/`open` refuse a directory any
/// other user can enter (`validate_secure_directory`: `0700` and owned by the caller on Unix), so
/// a directory made under the default umask (`0755`) fails with `KeyError::Storage` — found by the
/// clean-host rehearsal (`docs/acceptance/install-rehearsal-2026-09-13.md`), where `init` passed
/// on Windows and was refused on Ubuntu. The directory is GraphHelm's own, under `.graphhelm/`,
/// so tightening an existing one is safe and is done on every run.
fn ensure_owner_only_directory(path: &Path, pointer: &'static str) -> Result<State, Failure> {
    let state = ensure_directory(path, pointer)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| refused("the directory could not be made owner-only", pointer))?;
    }
    Ok(state)
}

/// A git work tree has `.git` at its root: a directory for a plain clone, a FILE for a worktree
/// or submodule (`gitdir: ...`). Both count, and so does an ANCESTOR carrying one: a project that
/// is a subdirectory of a repository (a monorepo) is inside that work tree even though it has no
/// `.git` of its own, and `git add -A` from the root would stage the secrets (PR #1070 review;
/// verified with a nested directory). The block still goes into `<project>/.gitignore` — a nested
/// ignore file governs its own subtree, so the root's file is never edited. Nothing is shelled out.
fn is_git_work_tree(project: &Path) -> bool {
    project
        .ancestors()
        .any(|directory| directory.join(".git").exists())
}

/// The spellings a hand-written `.gitignore` may already use for the same directory. Read line by
/// line rather than through `git check-ignore`, which would need git on PATH and a subprocess for
/// a five-line answer.
///
/// Git's rule is THE LAST MATCHING LINE WINS, and a `!` line un-ignores: `.graphhelm/` followed by
/// `!.graphhelm/` leaves the directory tracked, and a first-match predicate would have reported
/// `existing` and left both secrets stageable (PR #1070 review). So this folds every line in order
/// and answers with the final state; when that state is "not ignored" the block is appended, and
/// because it is appended LAST it is the rule git applies.
///
/// Two more details follow git rather than intuition (PR #1070 review, measured with
/// `git check-ignore` and `git status`):
/// - only TRAILING whitespace is stripped; a leading space is part of the pattern, so
///   `  .graphhelm/` ignores nothing and must not count;
/// - a negation BELOW the directory (`!.graphhelm/serve.key` after `.graphhelm/**`) un-ignores
///   that file, so any `!` line whose pattern lives under `.graphhelm/` also flips the answer to
///   "not ignored". Appending the block last then wins the file back, which is the conservative
///   outcome: a spare block is harmless, a stageable key is not.
fn already_ignored(gitignore: &str) -> bool {
    let mut ignored = false;
    for line in gitignore.lines().map(str::trim_end) {
        let (negated, pattern) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        if matches!(
            pattern,
            ".graphhelm/" | ".graphhelm" | "/.graphhelm/" | "/.graphhelm" | ".graphhelm/**"
        ) {
            ignored = !negated;
        } else if negated
            && pattern
                .strip_prefix('/')
                .unwrap_or(pattern)
                .starts_with(".graphhelm/")
        {
            ignored = false;
        }
    }
    ignored
}

pub(super) fn ensure_gitignore(project: &Path, path: &Path) -> Result<State, Failure> {
    if !is_git_work_tree(project) {
        return Ok(State::NotAGitWorkTree);
    }
    let unwritable = || refused("the .gitignore file could not be written", "/gitignore");
    refuse_symlink(path, "/gitignore")?;
    if let Ok(metadata) = std::fs::metadata(path)
        && metadata.len() > MAX_EDITED_FILE_BYTES
    {
        return Err(refused(
            "the .gitignore file is larger than init will read; add the .graphhelm/ line by hand",
            "/gitignore",
        ));
    }
    match std::fs::read_to_string(path) {
        Ok(existing) => {
            if already_ignored(&existing) {
                return Ok(State::Existing);
            }
            let mut text = existing;
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(GITIGNORE_BLOCK);
            std::fs::write(path, text).map_err(|_| unwritable())?;
            Ok(State::Appended)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::write(path, GITIGNORE_BLOCK.trim_start_matches('\n'))
                .map_err(|_| unwritable())?;
            Ok(State::Created)
        }
        Err(_) => Err(refused(
            "the .gitignore file exists and could not be read",
            "/gitignore",
        )),
    }
}

/// Simple and stated: Claude Code when the project or the home directory carries `.claude/`;
/// Codex when the home directory carries `.codex/`. `--harness` overrides.
fn detect_harnesses(project: &Path) -> Vec<Harness> {
    let home = std::env::home_dir();
    let mut found = Vec::new();
    let claude_here = project.join(".claude").is_dir();
    let claude_home = home.as_ref().is_some_and(|h| h.join(".claude").is_dir());
    if claude_here || claude_home {
        found.push(Harness::ClaudeCode);
    }
    if home.as_ref().is_some_and(|h| h.join(".codex").is_dir()) {
        found.push(Harness::Codex);
    }
    found
}

/// The `graphhelm` server entry both harnesses register: the token travels as a FILE PATH, never
/// as a value (`mcp --token-file`'s own contract).
fn mcp_arguments(url: &str, token_path: &Path) -> Vec<String> {
    vec![
        "mcp".to_owned(),
        "--url".to_owned(),
        url.to_owned(),
        "--token-file".to_owned(),
        token_path.to_string_lossy().into_owned(),
        "--actor".to_owned(),
        MCP_ACTOR.to_owned(),
    ]
}

/// The program a harness will spawn: THIS binary, by absolute path. `"graphhelm"` bare would
/// only work once the binary is on PATH, and `GETTING_STARTED.md` §1 explicitly allows running
/// `target/debug/graphhelm` without installing — a registration that named a program the harness
/// cannot find would break §6 for exactly that reader (PR #1070 review). The path that ran `init`
/// is a path that exists, which is the one fact `init` can vouch for; re-run `init` after moving
/// or installing the binary and the registration follows (the merge replaces the entry).
fn command_for_registration() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| std::path::absolute(exe).ok())
        .map_or_else(
            || "graphhelm".to_owned(),
            |exe| exe.to_string_lossy().into_owned(),
        )
}

fn ensure_claude_code(path: &Path, url: &str, token_path: &Path) -> Result<State, Failure> {
    let entry = json!({
        "command": command_for_registration(),
        "args": mcp_arguments(url, token_path),
    });
    let unparseable = || {
        refused(
            ".mcp.json exists and is not a JSON object with an optional \"mcpServers\" object; fix or move it aside",
            "/mcp_json",
        )
    };
    refuse_symlink(path, "/mcp_json")?;
    let (mut document, state) = match std::fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.len() > MAX_EDITED_FILE_BYTES {
                return Err(unparseable());
            }
            let text = std::fs::read_to_string(path).map_err(|_| unparseable())?;
            // A UTF-8 BOM (Notepad's default on Windows) is not JSON; strip it rather than
            // refuse the file. The merge re-serializes the whole document with `serde_json`'s
            // default (sorted) key order and two-space indentation — other servers' entries
            // keep their values, not their formatting.
            let text = text.trim_start_matches('\u{feff}');
            let value: Value = serde_json::from_str(text).map_err(|_| unparseable())?;
            match value {
                Value::Object(map) => (map, State::Merged),
                _ => return Err(unparseable()),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Map::new(), State::Created),
        Err(_) => return Err(unparseable()),
    };
    let servers = match document
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()))
    {
        Value::Object(servers) => servers,
        _ => return Err(unparseable()),
    };
    if servers.get(MCP_SERVER_NAME) == Some(&entry) {
        return Ok(State::Existing);
    }
    servers.insert(MCP_SERVER_NAME.to_owned(), entry);
    let mut text = serde_json::to_string_pretty(&Value::Object(document))
        .map_err(|_| refused(".mcp.json could not be serialized", "/mcp_json"))?;
    text.push('\n');
    std::fs::write(path, text)
        .map_err(|_| refused(".mcp.json could not be written", "/mcp_json"))?;
    Ok(state)
}

/// TOML basic-string escaping for the two characters a path can carry that matter: `\` (every
/// Windows path) and `"`.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn ensure_codex(path: &Path, url: &str, token_path: &Path) -> Result<State, Failure> {
    refuse_symlink(path, "/codex")?;
    let arguments = mcp_arguments(url, token_path)
        .iter()
        .map(|argument| toml_string(argument))
        .collect::<Vec<_>>()
        .join(", ");
    let command = toml_string(&command_for_registration());
    let snippet = format!(
        "# GraphHelm MCP server registration for Codex. Append to ~/.codex/config.toml.\n\
         # The token travels via a file path, never inline and never via argv.\n\
         [mcp_servers.{MCP_SERVER_NAME}]\n\
         command = {command}\n\
         args = [{arguments}]\n"
    );
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == snippet => Ok(State::Existing),
        Ok(_) => {
            std::fs::write(path, snippet)
                .map_err(|_| refused("the Codex snippet could not be written", "/codex"))?;
            Ok(State::Merged)
        }
        Err(_) => {
            std::fs::write(path, snippet)
                .map_err(|_| refused("the Codex snippet could not be written", "/codex"))?;
            Ok(State::Created)
        }
    }
}

struct NextPaths<'a> {
    bind: &'a SocketAddr,
    root: &'a Path,
    events: &'a Path,
    key: &'a Path,
    keyring: &'a Path,
    key_id: &'a str,
}

/// bash: single quotes are literal for EVERY character — backslashes (Windows paths), `$`, backtick,
/// and `!`, which an interactive shell history-expands even inside double quotes (PR #1070 review).
/// The one character single quotes cannot hold is `'` itself, spelled as `'\''` (close, escaped
/// quote, reopen). The result is safe as a word and inside `"$(cat ...)"`.
pub(super) fn quoted_bash(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// PowerShell: single quotes are literal — a `$` in the project path is not expanded (PR #1070
/// review) — and the one character that needs escaping inside them is `'`, doubled.
pub(super) fn quoted_powershell(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// The commands that come next, in order, with the paths this run just wrote already filled in.
/// The key is READ FROM ITS FILE by the first step; its value is not in this output. These strings
/// are the ONE place absolute paths appear in the envelope: a stranger copies them into a shell
/// from any working directory, so a project-relative spelling would be wrong the moment they `cd`.
fn next_steps(paths: &NextPaths<'_>) -> Vec<Value> {
    let fixtures_path = paths.root.join("fixtures.json");
    let (events, key, keyring, fixtures) = (
        quoted_bash(paths.events),
        quoted_bash(paths.key),
        quoted_bash(paths.keyring),
        quoted_bash(&fixtures_path),
    );
    let (ps_events, ps_key, ps_keyring, ps_fixtures) = (
        quoted_powershell(paths.events),
        quoted_powershell(paths.key),
        quoted_powershell(paths.keyring),
        quoted_powershell(&fixtures_path),
    );
    let key_id = paths.key_id;
    let bind = paths.bind;
    let fixture = r#"{"nodeOutcomes":{"implementation":"failure"}}"#;
    vec![
        json!({
            "step": "export the sealing key from serve.key (the Runtime needs it in the environment; it is never passed as a flag)",
            "powershell": format!("$env:{SEALING_KEY_ENVIRONMENT} = (Get-Content -Raw {ps_key}).Trim()"),
            "bash": format!("export {SEALING_KEY_ENVIRONMENT}=\"$(cat {key})\""),
        }),
        json!({
            "step": "start the Runtime on loopback (the serve.started line warns at once if the key or the key id does not open the keyring)",
            "powershell": format!("graphhelm serve --events {ps_events} --bind {bind} --keyring {ps_keyring} --key-id {key_id}"),
            "bash": format!("graphhelm serve --events {events} --bind {bind} --keyring {keyring} --key-id {key_id}"),
        }),
        json!({
            "step": "start the Studio, from your GraphHelm clone (Node 22+ and npm are needed only for this step)",
            "powershell": format!("powershell -File apps/studio/tools/studio-up.ps1 -Events {ps_events} -Bind {bind} -Keyring {ps_keyring} -KeyId {key_id} -GraphHelm graphhelm"),
            "bash": format!("npm --prefix apps/studio ci && GRAPHHELM_EVENTS={events} GRAPHHELM_RUNTIME_URL=\"http://{bind}\" npm --prefix apps/studio run dev"),
        }),
        json!({
            "step": "start the first execution, offline, from your GraphHelm clone (the fixture stands in for a model)",
            "powershell": format!("Set-Content -Path {ps_fixtures} -Value '{fixture}'; graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events {ps_events} --fixtures {ps_fixtures} --mode supervised --execution demo"),
            "bash": format!("echo '{fixture}' > {fixtures} && graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events {events} --fixtures {fixtures} --mode supervised --execution demo"),
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_written_ignore_line_in_any_common_spelling_counts() {
        for spelling in [
            ".graphhelm/",
            "/.graphhelm",
            ".graphhelm  ",
            ".graphhelm/\t",
            ".graphhelm/**",
        ] {
            assert!(
                already_ignored(&format!("target/\n{spelling}\n")),
                "{spelling:?}"
            );
        }
        // Git keeps LEADING whitespace as part of the pattern: `  .graphhelm/` ignores nothing
        // (`git check-ignore` exits 1), so it must not count as existing coverage.
        assert!(!already_ignored("target/\n  .graphhelm  \n"));
        assert!(!already_ignored("target/\n  .graphhelm/\n"));
        assert!(!already_ignored("target/\n.graphhelm-other/\n"));
        assert!(!already_ignored(""));
    }

    #[test]
    fn a_later_negation_wins_and_a_later_ignore_wins_back() {
        assert!(!already_ignored(".graphhelm/\n!.graphhelm/\n"));
        assert!(already_ignored(".graphhelm/\n!.graphhelm/\n/.graphhelm\n"));
        assert!(!already_ignored("!.graphhelm/\n"));
        // A negation BELOW the directory un-ignores that file (`git status` shows
        // `?? .graphhelm/serve.key`), so the fold answers "not ignored" and the block is appended.
        assert!(!already_ignored(".graphhelm/**\n!.graphhelm/serve.key\n"));
        assert!(!already_ignored(".graphhelm/\n!/.graphhelm/keyring\n"));
        assert!(!already_ignored(
            ".graphhelm/\n!.graphhelm/serve.key\n!.graphhelm/\n"
        ));
        // ...and a later whole-directory ignore wins the file back.
        assert!(already_ignored(
            ".graphhelm/**\n!.graphhelm/serve.key\n.graphhelm/\n"
        ));
        // Trailing whitespace on the negation is not part of the pattern either.
        assert!(!already_ignored(".graphhelm/\n!.graphhelm/serve.key  \n"));
        // A negation of some other directory's file does not touch ours.
        assert!(already_ignored(".graphhelm/\n!other/.graphhelm/x\n"));
        assert!(already_ignored(".graphhelm/\n!.graphhelm-other/x\n"));
    }

    #[test]
    fn key_ids_are_shell_safe_or_refused() {
        for ok in ["studio", "key.1", "a-b_c"] {
            assert!(validate_key_id(ok).is_ok(), "{ok}");
        }
        for bad in ["", "$(id)", "x;id", "a b", "k\"", "ü"] {
            assert_eq!(
                validate_key_id(bad).unwrap_err().code,
                ARGUMENT_CODE,
                "{bad}"
            );
        }
    }

    #[test]
    fn bind_must_be_a_fixed_loopback_port() {
        assert!(parse_bind("127.0.0.1:8791").is_ok());
        assert!(parse_bind("[::1]:8791").is_ok());
        assert_eq!(parse_bind("0.0.0.0:8791").unwrap_err().code, ARGUMENT_CODE);
        assert_eq!(parse_bind("127.0.0.1:0").unwrap_err().code, ARGUMENT_CODE);
        assert_eq!(
            parse_bind("localhost:8791").unwrap_err().code,
            ARGUMENT_CODE
        );
    }

    #[test]
    fn a_dollar_in_the_project_path_survives_both_shells() {
        let path = Path::new(r"C:\Users\me\$work\proj");
        assert_eq!(quoted_powershell(path), r"'C:\Users\me\$work\proj'");
        // Single quotes are literal in bash: no escaping of `\`, `$`, backtick, or `"`.
        assert_eq!(quoted_bash(path), r"'C:\Users\me\$work\proj'");
        assert_eq!(quoted_bash(Path::new("a`b\"c")), "'a`b\"c'");
        assert_eq!(quoted_powershell(Path::new("it's")), "'it''s'");
        assert_eq!(quoted_bash(Path::new("it's")), r"'it'\''s'");
    }

    #[test]
    fn a_bang_in_the_project_path_is_not_history_expanded_by_bash() {
        // `!` survives double quotes only by luck of the shell's history setting; inside single
        // quotes it is always literal (PR #1070 review).
        let path = Path::new("/home/me/proj!v2");
        let quoted = quoted_bash(path);
        assert_eq!(quoted, "'/home/me/proj!v2'");
        assert!(!quoted.contains('"'), "{quoted}");
        // The export step wraps the quoted key path in `"$(cat ...)"`; single quotes nest there.
        let step = format!("export {SEALING_KEY_ENVIRONMENT}=\"$(cat {quoted})\"");
        assert_eq!(
            step,
            "export GRAPHHELM_EVENTS_KEY=\"$(cat '/home/me/proj!v2')\""
        );
    }

    #[test]
    fn toml_strings_escape_backslashes_and_quotes() {
        assert_eq!(
            toml_string(r"C:\p\events.token"),
            r#""C:\\p\\events.token""#
        );
        assert_eq!(toml_string(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn next_steps_carry_the_key_path_and_never_a_value() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".graphhelm");
        let steps = next_steps(&NextPaths {
            bind: &"127.0.0.1:8791".parse().unwrap(),
            root: &root,
            events: &root.join("events"),
            key: &root.join("serve.key"),
            keyring: &root.join("keyring"),
            key_id: "studio",
        });
        assert_eq!(steps.len(), 4);
        let text = serde_json::to_string(&steps).unwrap();
        assert!(text.contains("serve.key"));
        assert!(text.contains("--bind 127.0.0.1:8791"));
        assert!(text.contains("-KeyId studio"));
        // Both shells get single-quoted (literal) paths; bash's `$(cat ...)` stays double-quoted
        // around the single-quoted key path so the export step is one valid word.
        assert!(
            steps[1]["powershell"]
                .as_str()
                .unwrap()
                .contains("--events '")
        );
        assert!(steps[1]["bash"].as_str().unwrap().contains("--events '"));
        let export = steps[0]["bash"].as_str().unwrap();
        assert!(export.starts_with("export GRAPHHELM_EVENTS_KEY=\"$(cat '"));
        assert!(export.ends_with("serve.key')\""), "{export}");
    }
}
