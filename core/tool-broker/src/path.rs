//! Lexical path and program-name rules.
//!
//! Everything here is form, not truth: these rules refuse every escape *shape* without touching
//! a filesystem, and the impure host re-checks containment against the real tree (symlinks,
//! junctions, canonical prefixes) after resolution. Both layers exist on purpose — a lexical
//! refusal is cheap and total; a physical check is authoritative but needs I/O this crate
//! forbids itself.

/// Upper bound on a workspace-relative path, in bytes. Generous for any legitimate repository
/// layout while keeping a hostile caller from smuggling megabytes through a path field.
pub const MAX_PATH_BYTES: usize = 4096;

/// A workspace-relative path in exactly one spelling: forward slashes, no empty/dot/dotdot
/// components, no absolute or drive or UNC form, no control bytes. Purely lexical — symlink
/// resolution needs the filesystem and lives in the host (`workspace.rs`), which re-checks
/// containment after canonicalization. Both layers exist on purpose: form here, truth there.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize)]
#[serde(transparent)]
pub struct RelativePath(String);

/// Why a candidate path (or program name) was refused. `Display` names the rule and never the
/// candidate: a refused path may be operator content, and refusals travel into diagnostics.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum PathRuleError {
    #[error("the path is empty")]
    Empty,
    #[error("the path is {bytes} bytes, over the {MAX_PATH_BYTES}-byte bound")]
    TooLong { bytes: usize },
    #[error("backslash separators are refused; use forward slashes")]
    BackslashSeparator,
    #[error("absolute, drive or UNC paths are refused")]
    NotRelative,
    #[error("empty, `.` and `..` components are refused")]
    DotComponent,
    #[error("control bytes are refused")]
    ControlByte,
    #[error("a program is a bare [a-z0-9_-] name of at most 64 bytes")]
    InvalidProgramName,
}

impl RelativePath {
    /// Parses one canonical spelling or refuses. Rule order: empty → length → control bytes →
    /// backslash → absolute/drive/UNC form → per-component `.`/`..`/empty refusal.
    ///
    /// # Errors
    /// Returns the first [`PathRuleError`] the candidate violates, in the order above.
    pub fn parse(candidate: &str) -> Result<Self, PathRuleError> {
        if candidate.is_empty() {
            return Err(PathRuleError::Empty);
        }
        if candidate.len() > MAX_PATH_BYTES {
            return Err(PathRuleError::TooLong {
                bytes: candidate.len(),
            });
        }
        if candidate.bytes().any(|byte| byte < 0x20) {
            return Err(PathRuleError::ControlByte);
        }
        if candidate.contains('\\') {
            return Err(PathRuleError::BackslashSeparator);
        }
        // A drive form (`C:/...` or `C:relative`) is any second byte `:`; a rooted or UNC form
        // starts with `/`. All are refused as one class: not workspace-relative.
        if candidate.starts_with('/') || candidate.as_bytes().get(1) == Some(&b':') {
            return Err(PathRuleError::NotRelative);
        }
        if candidate
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        {
            return Err(PathRuleError::DotComponent);
        }
        Ok(Self(candidate.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// Deserialize goes through `parse` so a record read back from disk cannot smuggle an
// unvalidated path into the host's resolution step.
impl<'de> serde::Deserialize<'de> for RelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let candidate = String::deserialize(deserializer)?;
        Self::parse(&candidate).map_err(serde::de::Error::custom)
    }
}

/// A program is a bare name resolved by the host OS's PATH: 1..=64 bytes of `[a-z0-9_-]`,
/// nothing else. No slashes (a path would bypass the lease's allowlist by naming anything on
/// disk), no dots (Windows resolution appends `.exe`/`.cmd` itself via PATHEXT; accepting an
/// explicit extension would give one program two allowlist spellings).
///
/// # Errors
/// Returns [`PathRuleError::InvalidProgramName`] for anything outside that shape.
pub fn validate_program_name(candidate: &str) -> Result<(), PathRuleError> {
    let shape_ok = !candidate.is_empty()
        && candidate.len() <= 64
        && candidate.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        });
    if shape_ok {
        Ok(())
    } else {
        Err(PathRuleError::InvalidProgramName)
    }
}
