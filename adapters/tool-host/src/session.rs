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
//! that can move is re-verified when read — and hands the provider `CBM_SNAPSHOT_DIR` pointing
//! at the pinned copy, never at the host index.

use std::collections::BTreeMap;
use std::path::PathBuf;

use graphhelm_tool_broker::record::{ContainedSessionIdentity, digest_hex};

use crate::process::{CapturedProcess, HostError, ProcessLimits, run_verified_in_workspace};
use crate::snapshot::{PinnedSnapshot, verify_pinned};
use crate::verified::VerifiedExecutable;
use crate::workspace::Tier1Workspace;

/// A contained provider session. Constructible ONLY from the three doorway types.
#[derive(Clone, Debug)]
pub struct ContainedProviderSession {
    workspace: PathBuf,
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

    /// One provider invocation: re-verify the pin at the instant of use, then run the verified
    /// binary through the one spawn funnel with the snapshot handed over confined.
    ///
    /// # Errors
    /// [`HostError::SnapshotMismatch`] BEFORE any spawn when the pinned copy moved (proven by
    /// the #544 observable: the funnel's sandbox dirs stay uncreated); otherwise exactly the
    /// funnel's errors.
    pub fn call(
        &self,
        arguments: &[String],
        stdin_bytes: Option<&[u8]>,
        limits: &ProcessLimits,
    ) -> Result<CapturedProcess, HostError> {
        verify_pinned(self.snapshot.root(), self.snapshot.generation())?;
        let mut extra = BTreeMap::new();
        extra.insert(
            "CBM_SNAPSHOT_DIR".to_owned(),
            self.snapshot.root().display().to_string(),
        );
        run_verified_in_workspace(
            &self.workspace,
            &self.executable,
            arguments,
            &extra,
            &[],
            stdin_bytes,
            limits,
        )
    }
}
