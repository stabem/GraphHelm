//! Executable identity, verified before the broker runs it (#540, D-042's clause).
//!
//! Two rules carry the whole property:
//!
//! - **The path is absolute or it is refused.** Program resolution (PATH order, PATHEXT,
//!   CreateProcess search) is exactly how one name runs two different binaries; re-implementing
//!   that search here to "verify what would run" would be a second, weaker copy of the OS with
//!   silent wrongness. An absolute path is an identity; a name is a question.
//! - **Absence of an expected digest is unrepresentable.** [`verify_executable`] requires the
//!   pin; there is no variant of this API that runs an unpinned provider, so "absence is a
//!   refusal, not a permission" holds by construction one layer up, where the pin is read.
//!
//! **The declared residue:** verification reads the bytes at the path at one instant, and the
//! spawn opens the same absolute path at a later instant. A writer with filesystem access to
//! the binary between those instants defeats the pin. CLOSING that window needs a handle-based
//! exec this crate does not have — but NARROWING it does not: a read handle opened with writers
//! denied (`share_mode` is stable std on Windows) and held across the spawn blocks the
//! write-swap and the delete-replace for the whole window. That narrowing is a real API change
//! deferred on purpose, not an impossibility — this paragraph is what a future reader uses to
//! decide whether revisiting is worth it, and it must not make the window read as less closable
//! than it is (L's #550 review). D-042's sentence — *verified before the broker runs it* — is
//! met at its letter, and the swap-sabotage cell measures the half that IS closed (bytes
//! swapped before verification refuse by name).

use std::path::{Path, PathBuf};

use graphhelm_tool_broker::record::digest_hex;

use crate::process::HostError;

/// An executable the broker has identified: the absolute path it will spawn and the SHA-256 of
/// the bytes that were there when it looked. Construction only via [`verify_executable`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedExecutable {
    path: PathBuf,
    sha256: String,
}

impl VerifiedExecutable {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Resolve `program` to a verified identity, or refuse.
///
/// # Errors
/// [`HostError::ExecutableNotPinned`] for a relative path (a name is not an identity);
/// [`HostError::Prepare`] if the bytes cannot be read; [`HostError::ExecutableMismatch`] when
/// the bytes at the path do not hash to `expected_sha256` — carrying both values, so the
/// operator can decide between re-pinning and investigating.
pub fn verify_executable(
    program: &Path,
    expected_sha256: &str,
) -> Result<VerifiedExecutable, HostError> {
    if !program.is_absolute() {
        return Err(HostError::ExecutableNotPinned {
            rule: "program path must be absolute",
        });
    }
    let bytes = std::fs::read(program).map_err(|source| HostError::Prepare { source })?;
    let actual = digest_hex(&bytes);
    if actual != expected_sha256 {
        return Err(HostError::ExecutableMismatch {
            expected: expected_sha256.to_owned(),
            actual,
        });
    }
    Ok(VerifiedExecutable {
        path: program.to_path_buf(),
        sha256: actual,
    })
}
