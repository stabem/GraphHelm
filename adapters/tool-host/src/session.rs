//! The broker-owned provider session (#552, D-042's primary clause), constructed so that an
//! uncontained session is UNREPRESENTABLE.
//!
//! The three qualifier doorways already exist and cannot be bypassed by forgetting a call:
//! `VerifiedExecutable` (#540, which binary), `PinnedSnapshot` (#539, what it reads over), and
//! the workspace whose spawn funnel confines `CBM_CACHE_DIR` (#538, where it writes). This type
//! is their composition one floor up — its constructor REQUIRES all three, so every session in
//! existence is contained by construction, the same shape each doorway used below.
//!
//! **What "session" means here, declared:** a containment contract plus a one-invocation-per-call
//! transport through the ONE Tier 1 spawn funnel (`run_verified_in_workspace` — no second spawn
//! path, per the acceptance). The MCP protocol framing (initialize, JSON-RPC) belongs to the
//! consumer that speaks it (#543's SourceReader / #219's producer): no SDK dependency enters the
//! lock here (M06 freeze), and a live long-running child would be a second spawn path this crate
//! refuses to grow. Each call re-verifies the snapshot pin AT THE INSTANT OF USE — an address
//! that can move is re-verified when read — then copies the verified pin into the funnel's
//! confined `CBM_CACHE_DIR`, the one address the real provider reads its store from (measured
//! against codebase-memory-mcp 0.10.8). The provider never sees the pin itself: it writes into
//! its cache dir, and a written-into pin would fail its own next re-verification, so the
//! serving copy is disposable and the pin stays the reference.

use std::collections::BTreeMap;
use std::path::PathBuf;

use graphhelm_tool_broker::record::{ContainedSessionIdentity, digest_hex};

use crate::process::{
    CancelSignal, CapturedProcess, HostError, ProcessLimits, run_verified_in_workspace,
};
use crate::snapshot::{PinnedSnapshot, verify_pinned};
use crate::verified::VerifiedExecutable;
use crate::workspace::Tier1Workspace;

/// A contained provider session. Constructible ONLY from the three doorway types.
#[derive(Clone, Debug)]
pub struct ContainedProviderSession {
    workspace: PathBuf,
    /// Serialises `call`, because the SERVING COPY is shared mutable state.
    ///
    /// Each call clears and re-copies the pin into the workspace's one `CBM_CACHE_DIR`, so two
    /// concurrent calls on one session would have the first deleting the tree the second's child
    /// is reading. No caller does this today — the port is consulted once per plan — but
    /// `StructuralCodeIndex` is declared `Send + Sync`, so the TYPE authorises exactly this and
    /// nothing would warn the caller who first tries it (Codex P1 on #579). Closed structurally
    /// rather than by a note about current callers, because the note is what goes stale.
    ///
    /// No deterministic red exists for a data race at reasonable cost; this is stated rather than
    /// dressed up as a tested behaviour.
    serving: std::sync::Arc<std::sync::Mutex<()>>,
    executable: VerifiedExecutable,
    snapshot: PinnedSnapshot,
    identity: ContainedSessionIdentity,
}

impl ContainedProviderSession {
    /// Compose a session from the three doorway TYPES -- `Tier1Workspace` included, so a raw
    /// path can never stand in for a provisioned, containment-checked workspace (L's #553 fold:
    /// two-thirds of the sentence was true; this makes it three-for-three). Infallible on
    /// purpose: every failure
    /// mode lives in the doorways that build the arguments, so there is nothing left here to
    /// check — a session you can NAME is already contained.
    #[must_use]
    pub fn open(
        workspace: &Tier1Workspace,
        executable: VerifiedExecutable,
        snapshot: PinnedSnapshot,
    ) -> Self {
        let workspace = workspace.root();
        // Derived and deterministic: the same composition is the same identity, so an auditor
        // can re-derive a receipt's session id from the record's own fields without trusting
        // any runner state.
        let composition = format!(
            "{}\n{}\n{}",
            executable.sha256(),
            snapshot.generation(),
            workspace.display()
        );
        let identity = ContainedSessionIdentity {
            session_id: format!("mcps-{}", &digest_hex(composition.as_bytes())[..16]),
            snapshot_generation: snapshot.generation().to_owned(),
            executable_sha256: executable.sha256().to_owned(),
        };
        Self {
            workspace: workspace.to_path_buf(),
            serving: std::sync::Arc::new(std::sync::Mutex::new(())),
            executable,
            snapshot,
            identity,
        }
    }

    /// The identity a `ToolCallRecord` carries beside `verified_executable`.
    #[must_use]
    pub fn identity(&self) -> &ContainedSessionIdentity {
        &self.identity
    }

    /// The verified executable this session runs — exposed so a producer can put the FULL
    /// `VerifiedExecutableIdentity` (path + digest) on the broker record beside the session's.
    #[must_use]
    pub fn executable(&self) -> &VerifiedExecutable {
        &self.executable
    }

    /// The pinned snapshot this session reads over.
    #[must_use]
    pub fn snapshot(&self) -> &PinnedSnapshot {
        &self.snapshot
    }

    /// One provider invocation: re-verify the pin at the instant of use, copy the verified pin
    /// into the funnel's confined `CBM_CACHE_DIR`, then run the verified binary through the one
    /// spawn funnel.
    ///
    /// # Errors
    /// [`HostError::SnapshotMismatch`] BEFORE any spawn when the pinned copy moved (proven by
    /// the #544 observable: the funnel's sandbox dirs stay uncreated);
    /// [`HostError::CaptureLost`] when the run produced bytes no reader could read; otherwise
    /// exactly the funnel's errors.
    pub fn call(
        &self,
        arguments: &[String],
        stdin_bytes: Option<&[u8]>,
        limits: &ProcessLimits,
        cancel: Option<&CancelSignal>,
    ) -> Result<CapturedProcess, HostError> {
        // Held across BOTH the re-copy and the child's run: releasing after the copy would let
        // the next call clear the tree mid-read, which is the race itself.
        let _serving = self
            .serving
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        verify_pinned(self.snapshot.root(), self.snapshot.generation())?;
        // The address the provider actually reads (measured against codebase-memory-mcp 0.10.8:
        // the binary consults CBM_CACHE_DIR and no other name) is the funnel's confined
        // `.cbm-cache` — so the verified pin is COPIED there, fresh, before every spawn. The pin
        // itself is never handed to the child: the provider writes into its cache dir (logs, at
        // minimum), and a written-into pin would fail its own re-verification on the next call.
        // The serving copy is disposable; the pin stays the reference that keeps verifying.
        let serving = crate::process::cbm_cache_dir(&self.workspace);
        if serving.exists() {
            std::fs::remove_dir_all(&serving).map_err(|source| HostError::Prepare { source })?;
        }
        crate::snapshot::copy_tree(self.snapshot.root(), &serving)?;
        let extra = BTreeMap::new();
        // A capture nobody could read is refused here rather than handed back. This seam returns a
        // bare `CapturedProcess`, so its consumers -- the provider in `codebase-memory-mcp` and the
        // benchmark generator -- have no disposition to carry "the bytes are missing"; they read the
        // exit code and hash what they were given. `ToolHost::invoke` keeps REPORTING the same
        // condition instead, because a disposition is the right vocabulary there.
        crate::process::reject_lost_capture(run_verified_in_workspace(
            &self.workspace,
            &self.executable,
            arguments,
            &extra,
            &[],
            stdin_bytes,
            limits,
            cancel,
        )?)
    }
}
