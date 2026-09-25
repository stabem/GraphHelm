//! Shared coordination for integration tests that temporarily rewrite embodied source mtimes.

use std::fs::{File, OpenOptions};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Serializes tests that fabricate the shared source timeline.
///
/// These tests are separate binaries, so a Rust `Mutex` cannot protect them. The lock is keyed
/// by the absolute workspace root so independent worktrees do not block one another. The file is
/// intentionally retained in the system temporary directory; the OS releases the advisory lock
/// when the guard is dropped, including when a test panics.
pub struct SubjectTimelineLock {
    file: File,
}

impl SubjectTimelineLock {
    pub fn acquire(root: &Path) -> Self {
        let mut digest = Sha256::new();
        digest.update(root.to_string_lossy().as_bytes());
        let lock_name = format!(
            "graphhelm-pathogens-subject-{}.lock",
            hex::encode(digest.finalize())
        );
        let path = std::env::temp_dir().join(lock_name);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .expect("the subject timeline lock is creatable");
        file.lock()
            .expect("the subject timeline lock is obtainable");
        Self { file }
    }
}

impl Drop for SubjectTimelineLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
