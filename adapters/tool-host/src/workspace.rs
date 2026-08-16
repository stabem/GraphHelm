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

use crate::process::HostError;

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
    pub fn provision(config: &WorkspaceConfig, call_id: &str) -> Result<Self, HostError> {
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
        if root.exists() {
            return Err(HostError::Config {
                rule: "the workspace directory already exists",
            });
        }
        let no_hooks = config.staging.join(format!("ghtool-{call_id}-nohooks"));
        std::fs::create_dir_all(&no_hooks).map_err(|source| HostError::Prepare { source })?;

        // The provision git runs under the SAME scrubbed config posture as execution
        // (process.rs): user-level config must not shape the checkout the scrubbed tools
        // will then judge — an autocrlf smudge here makes every text file look dirty to
        // `git apply --index` there ("does not match index"), because the smudged worktree
        // no longer re-hashes to its index entry once the filter is gone. Consistency of
        // config IS the correctness condition, so HOME points at the empty no-hooks scratch.
        let status = Command::new("git")
            .arg("-c")
            .arg(format!("core.hooksPath={}", git_safe(&no_hooks)))
            .arg("-C")
            .arg(git_safe(&config.project))
            .args(["worktree", "add", "--detach"])
            .arg(git_safe(&root))
            .arg("HEAD")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &no_hooks)
            .env("USERPROFILE", &no_hooks)
            .env("XDG_CONFIG_HOME", no_hooks.join("xdg"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|source| HostError::Spawn { source })?;
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
        })
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
            let _ = Command::new("git")
                .arg("-C")
                .arg(git_safe(&self.project))
                .args(["worktree", "prune"])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        if self.root.exists() {
            return Err(HostError::Config {
                rule: "the workspace could not be removed",
            });
        }
        Ok(())
    }

    fn try_git_remove(&self) -> bool {
        // Same scrubbed posture as provision, for the same reason; the redirected HOME may
        // not exist, which git treats as "no user config" — exactly the point.
        Command::new("git")
            .arg("-C")
            .arg(git_safe(&self.project))
            .args(["worktree", "remove", "--force"])
            .arg(git_safe(&self.root))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", self.root.join(".home"))
            .env("USERPROFILE", self.root.join(".home"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
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
