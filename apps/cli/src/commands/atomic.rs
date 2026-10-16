//! #1326: the CLI's one replace-in-place atomic write.
//!
//! A temporary file beside the target (same directory, so the rename never crosses a volume),
//! named from the process id, created with `create_new` (an existing file of that name is never
//! reused or clobbered), written, `sync_all`ed, then renamed over the target. Only a temporary
//! this call created is removed on failure. A symlinked target is refused; nothing follows it.
//!
//! **Variants that stay where they are, because their contract differs.** Each was compared on
//! temp name, mode, sync, rename and cleanup before this module existed:
//! - `gateway/route.rs` `write_atomically`: on Windows it replaces with
//!   `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` and retries sharing violations for 100 ms,
//!   which is stronger there than `std::fs::rename`.
//! - `schema/migrate.rs` `write_atomic` and `journey_explore.rs` `publish_new`: CREATE-only, the
//!   target must stay absent until publication (`renameat2(RENAME_NOREPLACE)` / `MoveFileW`, or a
//!   hard link). A replace here would be weaker.
//! - `runtime_record.rs` `publish`/`publish_project`: they refuse a symlink at every directory
//!   level they create, clear a stale temporary of their fixed hidden name first, and write
//!   0o600. A different directory contract, not a drifted copy.
//!
//! Neither this nor any variant fsyncs the parent directory; that is a property of every writer
//! in the crate today, named here so it is not mistaken for a difference.

use std::io::Write as _;
use std::path::Path;

/// Opt-in strengths beyond the default write.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Mode {
    /// Create the temporary owner-only (`0o600`) on unix. No effect elsewhere.
    pub(crate) private: bool,
    /// Refuse a target that exists and is not a regular file (a directory, a device), as well as a
    /// symlink. Without it a non-file target fails at the rename instead.
    pub(crate) regular_only: bool,
}

/// Replace `path` with `bytes` atomically (see the module doc).
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    atomic_write_with(path, bytes, Mode::default())
}

/// [`atomic_write`] with the strengths `mode` asks for.
pub(crate) fn atomic_write_with(path: &Path, bytes: &[u8], mode: Mode) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(std::io::Error::other("output symlink refused"));
        }
        Ok(meta) if mode.regular_only && !meta.is_file() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "output is not a regular file",
            ));
        }
        _ => {}
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut owned = false;
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if mode.private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // Windows has no mode bits to set; the request is accepted and has no effect there.
        #[cfg(not(unix))]
        let _ = mode.private;
        let mut file = options.open(&temp)?;
        owned = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    // Remove only this invocation's temporary file, never a pre-existing one.
    if owned && result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temps(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect()
    }

    #[test]
    fn atomic_write_creates_then_replaces_and_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.json");
        atomic_write(&target, b"first").unwrap();
        atomic_write(&target, b"second").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
        assert!(temps(dir.path()).is_empty(), "{:?}", temps(dir.path()));
    }

    #[test]
    fn atomic_write_removes_its_own_temporary_when_the_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("taken");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), b"x").unwrap();
        assert!(
            atomic_write(&target, b"bytes").is_err(),
            "a non-empty directory cannot be replaced"
        );
        assert!(temps(dir.path()).is_empty(), "{:?}", temps(dir.path()));
        assert!(
            target.join("keep").is_file(),
            "the target was left as it was"
        );
    }

    #[test]
    fn atomic_write_never_removes_a_pre_existing_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.json");
        let foreign = target.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&foreign, b"someone else's").unwrap();
        assert!(
            atomic_write(&target, b"bytes").is_err(),
            "create_new refuses an existing temp"
        );
        assert_eq!(std::fs::read(&foreign).unwrap(), b"someone else's");
    }

    #[test]
    fn atomic_write_with_regular_only_refuses_a_directory_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dir");
        std::fs::create_dir(&target).unwrap();
        let mode = Mode {
            private: true,
            regular_only: true,
        };
        let error = atomic_write_with(&target, b"bytes", mode).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(temps(dir.path()).is_empty(), "nothing was staged");
        // An absent target is fine with regular_only.
        atomic_write_with(&dir.path().join("new.json"), b"ok", mode).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_with_private_creates_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("secret.json");
        atomic_write_with(
            &target,
            b"x",
            Mode {
                private: true,
                regular_only: false,
            },
        )
        .unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
