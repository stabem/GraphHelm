//! Wire-neutral contracts for a local, reversible host-adoption inventory.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Inaccessible,
    Unsupported,
    Truncated,
}

/// Durable progress of a journaled adoption operation.
///
/// A multi-file adoption is journaled, not one atomic filesystem transaction. In particular,
/// `InstalledUnverified` is file-state evidence only; trusted host observation is required before
/// a later task may record `Verified`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    Planned,
    BackedUp,
    Applying,
    InstalledUnverified,
    Verified,
    Restoring,
    Restored,
    RecoveryRequired,
}

impl TransactionState {
    /// Whether the transaction's changes are installed: `InstalledUnverified` or `Verified`, and
    /// nothing else. `InstalledUnverified` is file-state evidence only (see the type's warning),
    /// so this does not mean the host observed the result. `Restored` is excluded on purpose:
    /// the changes were removed again. Callers that also accept `Restored` (the journal's
    /// active-transaction check) keep their own wider set.
    #[must_use]
    pub const fn is_installed(self) -> bool {
        matches!(self, Self::InstalledUnverified | Self::Verified)
    }
}

#[must_use]
pub fn coverage_complete(scopes: &[Coverage]) -> bool {
    !scopes.is_empty() && scopes.iter().all(|scope| *scope == Coverage::Complete)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionReason {
    ObserverMissing,
    ActivationInvalid,
    CoverageIncomplete,
    ReviewRequired,
    PlanStale,
    PathUnsafe,
    LimitExceeded,
    BackupUnverified,
    BackupCorrupt,
    Busy,
    RecoveryRequired,
    InvalidConfiguration,
    HostActionRequired,
    HostUnsupported,
    HostPolicyRequired,
    HostContainmentUnavailable,
    HostCleanupUnconfirmed,
}

impl AdoptionReason {
    #[must_use]
    pub const fn pointer(self) -> &'static str {
        match self {
            Self::ObserverMissing => "/adoption/observer_missing",
            Self::ActivationInvalid => "/adoption/activation_invalid",
            Self::CoverageIncomplete => "/adoption/coverage_incomplete",
            Self::ReviewRequired => "/adoption/review_required",
            Self::PlanStale => "/adoption/plan_stale",
            Self::PathUnsafe => "/adoption/path_unsafe",
            Self::LimitExceeded => "/adoption/limit_exceeded",
            Self::BackupUnverified => "/adoption/backup_unverified",
            Self::BackupCorrupt => "/adoption/backup_corrupt",
            Self::Busy => "/adoption/busy",
            Self::RecoveryRequired => "/adoption/recovery_required",
            Self::InvalidConfiguration => "/adoption/invalid_configuration",
            Self::HostActionRequired => "/adoption/host_action_required",
            Self::HostUnsupported => "/adoption/host_unsupported",
            Self::HostPolicyRequired => "/adoption/host_action_required",
            Self::HostContainmentUnavailable => "/adoption/host_containment_unavailable",
            Self::HostCleanupUnconfirmed => "/adoption/host_cleanup_unconfirmed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdoptionError {
    pub reason: AdoptionReason,
}

impl fmt::Display for AdoptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.reason {
            AdoptionReason::ObserverMissing => "No trusted host observer is available. Installation remains unverified; user-authored receipts cannot prove activation.",
            AdoptionReason::ActivationInvalid => "activation evidence is missing, stale, or does not match the accepted transaction",
            AdoptionReason::CoverageIncomplete => "host inventory coverage is incomplete",
            AdoptionReason::ReviewRequired => "the plan has not been accepted exactly",
            AdoptionReason::PlanStale => "the reviewed source state has changed",
            AdoptionReason::PathUnsafe => "host inventory path is unsafe",
            AdoptionReason::LimitExceeded => "host inventory exceeds a configured limit",
            AdoptionReason::BackupUnverified => "original backup was not verified",
            AdoptionReason::BackupCorrupt => "backup verification failed",
            AdoptionReason::Busy => "another adoption operation is in progress",
            AdoptionReason::RecoveryRequired => {
                "adoption recovery is required before another apply"
            }
            AdoptionReason::InvalidConfiguration => "host configuration is invalid",
            AdoptionReason::HostActionRequired => "Open the Codex plugin browser, install graphhelm-jpd and graphhelm-development-contracts from the reviewed local release bundle, then restart Codex and run graphhelm setup --dry-run again. No host files were changed.",
            AdoptionReason::HostUnsupported => "the detected host does not provide a supported reversible plugin interface",
            AdoptionReason::HostPolicyRequired => "In a fresh Claude Code session, open /status and inspect all managed setting sources. Load the reviewed graphhelm-jpd and graphhelm-development-contracts package directories with --plugin-dir, then rescan. Automatic plugin disabling requires complete effective-policy observation; no host files were changed.",
            AdoptionReason::HostContainmentUnavailable => "This platform has no supported process-containment backend for host execution. No host process was started and no host configuration files were changed. Automatic host operations require enforced descendant containment.",
            AdoptionReason::HostCleanupUnconfirmed => "The host process was started, but bounded cleanup could not be confirmed before the operation deadline. Host results are discarded and no successful host reply is returned.",
        })
    }
}

impl std::error::Error for AdoptionError {}

#[cfg(test)]
mod tests {
    use super::{Coverage, TransactionState, coverage_complete};

    #[test]
    fn incomplete_scope_is_not_complete_inventory() {
        assert!(!coverage_complete(&[]));
        assert!(!coverage_complete(&[
            Coverage::Complete,
            Coverage::Unsupported
        ]));
        assert!(coverage_complete(&[Coverage::Complete]));
    }

    /// The installed set feeds data-loss decisions (reuse a journal, skip compensation, accept a
    /// restore). The match below has no wildcard, so a new variant fails to compile here until
    /// someone answers deliberately whether it counts as installed.
    #[test]
    fn exactly_installed_unverified_and_verified_are_installed() {
        let expected = |state: TransactionState| match state {
            TransactionState::InstalledUnverified | TransactionState::Verified => true,
            TransactionState::Planned
            | TransactionState::BackedUp
            | TransactionState::Applying
            | TransactionState::Restoring
            | TransactionState::Restored
            | TransactionState::RecoveryRequired => false,
        };
        for state in [
            TransactionState::Planned,
            TransactionState::BackedUp,
            TransactionState::Applying,
            TransactionState::InstalledUnverified,
            TransactionState::Verified,
            TransactionState::Restoring,
            TransactionState::Restored,
            TransactionState::RecoveryRequired,
        ] {
            assert_eq!(state.is_installed(), expected(state), "{state:?}");
        }
    }

    #[test]
    fn installed_unverified_is_a_public_snake_case_state() {
        assert_eq!(
            serde_json::to_value(TransactionState::InstalledUnverified).unwrap(),
            serde_json::json!("installed_unverified")
        );
    }
}
