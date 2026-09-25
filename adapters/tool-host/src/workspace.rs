//! The Tier 1 workspace: an ephemeral, detached git worktree with containment guarantees.
//!
//! Credentials are kept out structurally, not procedurally: [`WorkspaceConfig::validated`] is
//! the only constructor the host accepts downstream, it has no credential field, and it
//! refuses any staging area that overlaps a protected directory in either direction — so the
//! trees that hold sealed material and the trees that run tools can never nest. The lexical
//! path rules (`graphhelm_tool_broker::path`) said what a path may look like; [`Tier1Workspace::resolve`]
//! says what it actually reaches, walking the real filesystem against links and junctions.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use graphhelm_tool_broker::path::RelativePath;

use crate::process::{
    CancelHold, CancelSignal, HostError, SupervisedOutcome, run_supervised, scrub_environment,
};

/// Retry backoffs for [`Tier1Workspace::remove`]: a freshly written tree can hold transient
/// Permission-denied locks on Windows (indexer, antivirus), and a single-shot removal WILL
/// flake — the #19 class. Only transient lock friction is absorbed; a tree that survives every
/// attempt is still an error, because a leaked workspace is a leaked write capability.
const REMOVE_BACKOFFS: &[Duration] = &[
    Duration::from_millis(50),
    Duration::from_millis(250),
    Duration::from_millis(1000),
];

/// A canonicalized path in the spelling git accepts. Windows `canonicalize` returns verbatim
/// (`\\?\C:\...`) paths, which git refuses outright — so the canonical form is kept for
/// containment comparisons and stripped only at the git boundary.
pub(crate) fn git_safe(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map_or(text.clone(), str::to_owned)
}

/// Where workspaces come from (`project`) and where they live (`staging`). Construction is
/// validation: there is no other door.
#[derive(Clone, Debug)]
pub struct WorkspaceConfig {
    project: PathBuf,
    staging: PathBuf,
}

impl WorkspaceConfig {
    /// Canonicalizes both directories (creating `staging` if absent), refuses a staging area
    /// inside the project (a worktree inside the repo confuses git and every containment
    /// rule), and refuses overlap with any protected path in either direction — the caller
    /// passes its keyring/broker/events directories and the workspace tree can then never
    /// contain or be contained by them.
    ///
    /// # Errors
    /// [`HostError::Config`] naming the violated rule (never a path).
    pub fn validated(
        project: &Path,
        staging: &Path,
        protected: &[PathBuf],
    ) -> Result<Self, HostError> {
        let project = project
            .canonicalize()
            .map_err(|source| HostError::Prepare { source })?;
        std::fs::create_dir_all(staging).map_err(|source| HostError::Prepare { source })?;
        let staging = staging
            .canonicalize()
            .map_err(|source| HostError::Prepare { source })?;
        if staging.starts_with(&project) || project.starts_with(&staging) {
            return Err(HostError::Config {
                rule: "the staging area must not overlap the project",
            });
        }
        for path in protected {
            let path = path
                .canonicalize()
                .map_err(|source| HostError::Prepare { source })?;
            if staging.starts_with(&path) || path.starts_with(&staging) {
                return Err(HostError::Config {
                    rule: "the staging area must not overlap a protected directory",
                });
            }
        }
        Ok(Self { project, staging })
    }

    /// The canonical project root (Tier 0 reads aim here).
    #[must_use]
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// The canonical staging area (Tier 1 workspaces and read-scratch live here).
    #[must_use]
    pub fn staging(&self) -> &Path {
        &self.staging
    }
}

/// One ephemeral, detached worktree. Detached on purpose: no branch leaks into the project's
/// ref namespace, and commits made inside are reachable only by the worktree HEAD until
/// [`Tier1Workspace::remove`] — which is exactly the ephemeral contract.
pub struct Tier1Workspace {
    root: PathBuf,
    project: PathBuf,
    /// Whether `provision_from` found a stale tree at this root and reclaimed it first (#1073):
    /// the leftover of a drive whose server died before `release`. Surfaces on the record.
    recovered: bool,
    /// The counted span that makes `cancel` wait for this workspace's TEARDOWN (#617).
    ///
    /// Taken in `provision` and released when `remove` returns, so the in-flight count never
    /// reaches zero between the tool child being reaped and the `git worktree remove` that
    /// follows it. Counted per child instead, the count hit zero at the reap, `cancel` returned,
    /// and the removal spawned afterwards -- outside the promise it had just answered.
    ///
    /// `None` when the caller passed no signal, which is every direct test caller.
    hold: Option<CancelHold>,
}

impl Tier1Workspace {
    /// `git -c core.hooksPath=<fresh empty dir> -C <project> worktree add --detach <root> HEAD`.
    ///
    /// `core.hooksPath` pointed at an empty directory is the threat model §13 "disable hooks
    /// by default" control: `git worktree add` runs the repository's `post-checkout` hook, and
    /// a hostile project must never get code execution out of being provisioned. Spawned
    /// directly with argv (`run_in_workspace` needs an existing root, which provisioning is
    /// creating).
    ///
    /// # Errors
    /// [`HostError::Prepare`]/[`HostError::Spawn`] on filesystem or git failure; the git
    /// output never travels into the error (redaction-safe by construction).
    pub fn provision(
        config: &WorkspaceConfig,
        call_id: &str,
        cancel: Option<&CancelSignal>,
    ) -> Result<Self, HostError> {
        Self::provision_from(config, call_id, cancel, "HEAD")
    }

    /// [`Tier1Workspace::provision`] at an explicit start point instead of `HEAD` (#1066): an
    /// execution whose earlier drive already landed `refs/graphhelm/executions/<id>` continues
    /// from ITS OWN last commit, not from wherever the operator's checkout has moved since.
    ///
    /// `start_point` is a ref name or `HEAD`, never caller input: the host derives it from the
    /// execution id through `graphhelm_tool_broker::record::execution_ref`, and a ref name
    /// cannot begin with `-` or contain whitespace under that derivation, so it never reads as
    /// a git option.
    ///
    /// # Errors
    /// As [`Tier1Workspace::provision`]; a start point git cannot resolve refuses with
    /// [`HostError::Config`].
    pub fn provision_from(
        config: &WorkspaceConfig,
        call_id: &str,
        cancel: Option<&CancelSignal>,
        start_point: &str,
    ) -> Result<Self, HostError> {
        // Defense in depth: the id flows into a path join, so its shape is pinned even though
        // today's only caller is internal — "../x" must never become a staging escape.
        let id_ok = !call_id.is_empty()
            && call_id.len() <= 64
            && call_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !id_ok {
            return Err(HostError::Config {
                rule: "the call id must be 1..=64 bytes of [a-z0-9-]",
            });
        }
        let root = config.staging.join(format!("ghtool-{call_id}"));
        // A tree already at this root is a LEFTOVER, never a live workspace: a live one is held
        // by the host that provisioned it, and the host names its trees so two calls cannot
        // share one. A server killed mid-drive leaves the tree and its `.git/worktrees/…`
        // registration behind, and refusing here made every later drive of that execution
        // refuse forever (Codex, on #1073). The root is under OUR staging by construction
        // (`config.staging.join`), so reclaiming it removes nothing that is not ours.
        let mut recovered = root.exists();
        if recovered {
            reclaim_stale(&config.project, &root)?;
        }
        let no_hooks = config.staging.join(format!("ghtool-{call_id}-nohooks"));
        std::fs::create_dir_all(&no_hooks).map_err(|source| HostError::Prepare { source })?;

        // The span opens BEFORE the first spawn and closes in `remove`, so a cancellation
        // arriving at any point between them finds a non-zero count and waits (#617). A signal
        // that is already raised refuses here rather than provisioning a workspace for a call
        // that will not run -- the same fail-closed answer `attach` gives, for the same reason.
        let hold = match cancel {
            Some(signal) => match signal.hold() {
                Some(hold) => Some(hold),
                None => {
                    let _ = std::fs::remove_dir_all(&no_hooks);
                    return Err(HostError::Cancelled);
                }
            },
            None => None,
        };

        // The provision git runs under the SAME scrubbed config posture as execution
        // (process.rs): user-level config must not shape the checkout the scrubbed tools
        // will then judge — an autocrlf smudge here makes every text file look dirty to
        // `git apply --index` there ("does not match index"), because the smudged worktree
        // no longer re-hashes to its index entry once the filter is gone. Consistency of
        // config IS the correctness condition, so HOME points at the empty no-hooks scratch.
        // #617: this spawn IS inside `CancelSignal` now, and it is the INTERRUPTIBLE one. A
        // cancellation arriving mid-provision kills the tree: nothing downstream has run, so
        // there is nothing to leave half-done, and a workspace nobody will use is waste. Removal
        // takes the opposite policy for the opposite reason -- see `remove`.
        //
        // The reason `run_in_workspace` itself cannot be reused here is unchanged and still the
        // real one: it needs an existing root, and this is what creates it. What #617 changed is
        // that not-reusing it no longer means not being reachable.
        let mut command = Command::new("git");
        // SCRUBBED FIRST, then the git-specific variables below, which win (#491). Provisioning
        // ran with the parent's WHOLE environment: `git worktree add` performs a checkout, a
        // checkout runs any configured clean/smudge FILTER, and a filter is an arbitrary program
        // that could read whatever the host happened to hold -- tokens, passphrases, profile
        // paths. `core.hooksPath` closes the hook door and left the filter door open.
        //
        // The removal path's redirected HOME already demonstrated the intended posture; this is
        // the rest of it.
        scrub_environment(&mut command);
        command
            .arg("-c")
            .arg(format!("core.hooksPath={}", git_safe(&no_hooks)))
            .arg("-C")
            .arg(git_safe(&config.project))
            .args(["worktree", "add", "--detach"])
            .arg(git_safe(&root))
            .arg(start_point)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &no_hooks)
            .env("USERPROFILE", &no_hooks)
            .env("XDG_CONFIG_HOME", no_hooks.join("xdg"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut retried = false;
        let status = loop {
            let outcome = run_supervised(&mut command, cancel, true);
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&no_hooks);
                    return Err(error);
                }
            };
            let status = match outcome {
                SupervisedOutcome::Finished(status) => status,
                SupervisedOutcome::Cancelled => {
                    // The killed `worktree add` may have left a partial directory AND a
                    // registration in the project. Neither is this call's to keep: the
                    // directory goes, and the registration goes with it through the same
                    // reclaim the stale-tree path uses (`worktree remove --force` clears a
                    // registration whose directory is already gone — Codex, on #1073: without
                    // this, the next provision at the deterministic root was refused as
                    // "missing but already registered"). Best-effort, because the outcome is
                    // already decided.
                    let _ = std::fs::remove_dir_all(&no_hooks);
                    let _ = std::fs::remove_dir_all(&root);
                    let _ = reclaim_stale(&config.project, &root);
                    return Err(HostError::Cancelled);
                }
            };
            if status.success() {
                break status;
            }
            // A refusal with NO directory at the root is the registration-without-a-directory
            // case (a provision cancelled or killed after git registered the path): reclaim it
            // once and try again. A second refusal is git's own answer and is reported.
            if !retried && !recovered && !root.exists() {
                retried = true;
                recovered = true;
                if let Err(error) = reclaim_stale(&config.project, &root) {
                    let _ = std::fs::remove_dir_all(&no_hooks);
                    return Err(error);
                }
                continue;
            }
            break status;
        };
        // The no-hooks directory's whole role ends when `worktree add` returns; removing it
        // here keeps the staging area's contract simple — after a call completes, staging is
        // empty again (the Task 7 broker asserts exactly that).
        let _ = std::fs::remove_dir_all(&no_hooks);
        if !status.success() {
            return Err(HostError::Config {
                rule: "git worktree add refused the provision",
            });
        }
        let root = root
            .canonicalize()
            .map_err(|source| HostError::Prepare { source })?;
        Ok(Self {
            root,
            project: config.project.clone(),
            hold,
            recovered,
        })
    }

    /// Whether provisioning reclaimed a stale tree at this root first (#1073).
    #[must_use]
    pub fn recovered(&self) -> bool {
        self.recovered
    }

    /// Releases the counted cancellation span WITHOUT removing the tree (#1073): an execution's
    /// workspace outlives its calls, and a hold kept between them made `CancelSignal::cancel`
    /// — and so an immediate pause — wait its full grace for a tree with nothing running in it.
    /// The span exists to make a cancellation wait for a TEARDOWN in flight; between calls
    /// there is none. `unpark` retakes it before the next call.
    pub fn park(&mut self) {
        self.hold = None;
    }

    /// Retakes the counted span for a call about to run in this tree — idempotent, so a tree
    /// fresh from `provision` (which already holds one) is unchanged. A signal that is already
    /// raised refuses, the same fail-closed answer `provision` gives.
    ///
    /// # Errors
    /// [`HostError::Cancelled`] when the signal is already raised.
    pub fn unpark(&mut self, cancel: Option<&CancelSignal>) -> Result<(), HostError> {
        if self.hold.is_some() {
            return Ok(());
        }
        if let Some(signal) = cancel {
            self.hold = Some(signal.hold().ok_or(HostError::Cancelled)?);
        }
        Ok(())
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Lexically-validated relative path → real path inside this workspace.
    ///
    /// The walk vets every EXISTING component with `symlink_metadata`: any symlink or junction
    /// on the chain is refused (a junction resolves elsewhere, and "elsewhere" is exactly what
    /// containment forbids). Components that do not exist yet are fine — write targets are
    /// born later — because every ancestor that DOES exist has been proven link-free, the
    /// joined path cannot escape.
    ///
    /// # Errors
    /// [`HostError::Escape`] on any link in the chain.
    pub fn resolve(&self, path: &RelativePath) -> Result<PathBuf, HostError> {
        resolve_within(&self.root, path)
    }

    /// Removal with Windows honesty: `git worktree remove --force` under retry/backoff, then
    /// a `remove_dir_all` fallback plus `git worktree prune` for the registration, and only
    /// then an error — the semantics survive the retries; only transient lock friction is
    /// absorbed.
    ///
    /// **Not interruptible, and counted (#617).** A cancellation must WAIT for this rather than
    /// stop it: killing a `git worktree remove` halfway leaves the tree on disk, and this
    /// module's own contract calls a leaked workspace a leaked write capability. The counted
    /// span taken in `provision` is released at the end of this function, which is what makes
    /// `CancelSignal::cancel` wait for the teardown its own cancellation caused.
    ///
    /// # Errors
    /// [`HostError::Config`] if the tree still exists after every attempt.
    pub fn remove(self) -> Result<(), HostError> {
        let mut removed = self.try_git_remove();
        if !removed {
            for backoff in REMOVE_BACKOFFS {
                std::thread::sleep(*backoff);
                if self.try_git_remove() {
                    removed = true;
                    break;
                }
            }
        }
        if !removed && self.root.exists() {
            for backoff in REMOVE_BACKOFFS {
                if std::fs::remove_dir_all(&self.root).is_ok() {
                    break;
                }
                std::thread::sleep(*backoff);
            }
            let mut prune = Command::new("git");
            // The third spawn, and it is easy to miss because it is a fallback: the same scrub
            // applies (#491). A spawn that runs only when something already went wrong is exactly
            // the one that gets a weaker posture by accident.
            scrub_environment(&mut prune);
            prune
                .arg("-C")
                .arg(git_safe(&self.project))
                .args(["worktree", "prune"])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            let _ = run_supervised(&mut prune, None, false);
        }
        let exists = self.root.exists();
        // Released HERE, explicitly, and after the last filesystem answer this function needs.
        // Letting it fall out of scope would work today and would break the moment anyone adds a
        // line below it, because the release is the thing that lets `cancel` return.
        drop(self.hold);
        if exists {
            return Err(HostError::Config {
                rule: "the workspace could not be removed",
            });
        }
        Ok(())
    }

    fn try_git_remove(&self) -> bool {
        // Same scrubbed posture as provision, for the same reason; the redirected HOME may
        // not exist, which git treats as "no user config" — exactly the point.
        //
        // `run_supervised` with `interruptible = false`: this is the half a cancellation must
        // not cut. It still goes through the supervised path rather than `Command::status` so
        // the child is in a job object like every other spawn in this crate -- a `git worktree
        // remove` that wedges is then killable by the same mechanism as anything else, instead
        // of being the one spawn nothing can reach.
        let mut command = Command::new("git");
        // Same posture as provisioning, for the same reason: `worktree remove --force` deletes a
        // checked-out tree and git may consult configuration to do it (#491).
        scrub_environment(&mut command);
        command
            .arg("-C")
            .arg(git_safe(&self.project))
            .args(["worktree", "remove", "--force"])
            .arg(git_safe(&self.root))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", self.root.join(".home"))
            .env("USERPROFILE", self.root.join(".home"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        matches!(
            run_supervised(&mut command, None, false),
            Ok(SupervisedOutcome::Finished(status)) if status.success()
        )
    }
}

/// Removes a stale tree at `root` (a leftover of a drive whose server died before `release`)
/// and its registration in the project, so the root can be provisioned again (#1073).
/// `git worktree remove --force` under retry, then `remove_dir_all`, then `git worktree prune`
/// — the same three steps `Tier1Workspace::remove` takes, applied to a tree no `Tier1Workspace`
/// value holds. Same scrubbed posture, same disabled hooks.
///
/// # Errors
/// [`HostError::Config`] if the tree still exists after every attempt.
fn reclaim_stale(project: &Path, root: &Path) -> Result<(), HostError> {
    let git_remove = || {
        let mut command = Command::new("git");
        scrub_environment(&mut command);
        command
            .arg("-C")
            .arg(git_safe(project))
            .args(["worktree", "remove", "--force"])
            .arg(git_safe(root))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", root.join(".home"))
            .env("USERPROFILE", root.join(".home"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        matches!(
            run_supervised(&mut command, None, false),
            Ok(SupervisedOutcome::Finished(status)) if status.success()
        )
    };
    let mut removed = git_remove();
    if !removed {
        for backoff in REMOVE_BACKOFFS {
            std::thread::sleep(*backoff);
            if git_remove() {
                removed = true;
                break;
            }
        }
    }
    if !removed && root.exists() {
        for backoff in REMOVE_BACKOFFS {
            if std::fs::remove_dir_all(root).is_ok() {
                break;
            }
            std::thread::sleep(*backoff);
        }
    }
    // The registration goes regardless of which step freed the tree: `worktree add` refuses a
    // root that is still registered even when nothing is on disk.
    let mut prune = Command::new("git");
    scrub_environment(&mut prune);
    prune
        .arg("-C")
        .arg(git_safe(project))
        .args(["worktree", "prune"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let _ = run_supervised(&mut prune, None, false);
    if root.exists() {
        return Err(HostError::Config {
            rule: "a stale workspace at this root could not be reclaimed",
        });
    }
    Ok(())
}

/// The anti-link containment walk, shared by the workspace's `resolve` and the host's
/// Tier 0 project reads: same rules, one implementation, two roots.
pub(crate) fn resolve_within(root: &Path, path: &RelativePath) -> Result<PathBuf, HostError> {
    let mut current = root.to_path_buf();
    for component in path.as_str().split('/') {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                // On Windows a junction reports as a symlink through symlink_metadata's
                // file_type; is_symlink covers both reparse forms std models.
                if metadata.file_type().is_symlink() {
                    return Err(HostError::Escape);
                }
            }
            Err(_) => break, // not yet existing: remaining components are birth targets
        }
    }
    // Belt and braces, as the task text demands: the walk above should subsume this, but
    // containment code follows the house rule of two layers — the deepest EXISTING
    // ancestor must canonicalize back inside the canonical root, or something the walk
    // could not model (an exotic reparse form, a race) is redirecting the chain.
    let mut deepest = current.clone();
    while !deepest.exists() {
        if !deepest.pop() {
            return Err(HostError::Escape);
        }
    }
    let canonical = deepest
        .canonicalize()
        .map_err(|source| HostError::Prepare { source })?;
    if !canonical.starts_with(root) {
        return Err(HostError::Escape);
    }
    Ok(current)
}
