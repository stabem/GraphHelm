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

#[must_use]
pub fn coverage_complete(scopes: &[Coverage]) -> bool {
    !scopes.is_empty() && scopes.iter().all(|scope| *scope == Coverage::Complete)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionReason {
    CoverageIncomplete,
    PathUnsafe,
    LimitExceeded,
    BackupCorrupt,
    InvalidConfiguration,
}

impl AdoptionReason {
    #[must_use]
    pub const fn pointer(self) -> &'static str {
        match self {
            Self::CoverageIncomplete => "/adoption/coverage_incomplete",
            Self::PathUnsafe => "/adoption/path_unsafe",
            Self::LimitExceeded => "/adoption/limit_exceeded",
            Self::BackupCorrupt => "/adoption/backup_corrupt",
            Self::InvalidConfiguration => "/adoption/invalid_configuration",
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
            AdoptionReason::CoverageIncomplete => "host inventory coverage is incomplete",
            AdoptionReason::PathUnsafe => "host inventory path is unsafe",
            AdoptionReason::LimitExceeded => "host inventory exceeds a configured limit",
            AdoptionReason::BackupCorrupt => "backup verification failed",
            AdoptionReason::InvalidConfiguration => "host configuration is invalid",
        })
    }
}

impl std::error::Error for AdoptionError {}

#[cfg(test)]
mod tests {
    use super::{Coverage, coverage_complete};

    #[test]
    fn incomplete_scope_is_not_complete_inventory() {
        assert!(!coverage_complete(&[]));
        assert!(!coverage_complete(&[
            Coverage::Complete,
            Coverage::Unsupported
        ]));
        assert!(coverage_complete(&[Coverage::Complete]));
    }
}
