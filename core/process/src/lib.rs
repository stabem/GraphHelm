//! The one way GraphHelm starts a child process.
//!
//! Owner rule: every child runs hidden in the terminal that started GraphHelm and never opens a
//! console window of its own. On Windows a console child of a process without a visible console
//! (an agent harness, a hidden runner) gets a fresh, flashing window; `CREATE_NO_WINDOW` gives it
//! a hidden console instead, which its own children inherit. Stdio is untouched, so capture and
//! inheritance behave exactly as with a bare `std::process::Command`. Elsewhere this is a no-op.
//!
//! The workspace source guard (`tests/no_console_window.rs`) fails when any other `.rs` file
//! constructs a command directly or sets creation flags without `CREATE_NO_WINDOW`.

use std::ffi::OsStr;
use std::process::Command;

/// Win32 `CREATE_NO_WINDOW`. OR it into any other creation flags a spawn site sets.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A `Command` for `program` that never opens a console window.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    hide_window(&mut command);
    command
}

/// Marks an existing command so it never opens a console window.
#[cfg(windows)]
pub fn hide_window(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW)
}

/// Marks an existing command so it never opens a console window (no-op off Windows).
#[cfg(not(windows))]
pub fn hide_window(command: &mut Command) -> &mut Command {
    command
}

/// Filesystem facts every guard reads the same way (#1338).
pub mod fs {
    /// Whether `metadata` describes a Windows reparse point (a junction, a symlink, a mount point
    /// or any other tag): the `FILE_ATTRIBUTE_REPARSE_POINT` bit (`0x400`), a literal so this
    /// crate keeps no dependencies. Always `false` off Windows, where a caller that must refuse
    /// links tests `is_symlink()` itself. It reads only the metadata it is given: whether that
    /// metadata followed a link (`metadata` vs `symlink_metadata`) stays the caller's decision.
    #[cfg(windows)]
    #[must_use]
    pub fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    /// Always `false` off Windows (see the Windows definition).
    #[cfg(not(windows))]
    #[must_use]
    pub fn is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    /// #1338: the one reparse test every guard now calls. A junction read with `symlink_metadata`
    /// is a reparse point; the plain directory it targets and a plain file are not. A credible
    /// regression is a wrong bit or a helper that reads followed metadata. Cost: one temp folder.
    #[cfg(windows)]
    #[test]
    fn a_junction_is_a_reparse_point_and_plain_entries_are_not() {
        use super::fs::is_reparse_point;
        let root = std::env::temp_dir().join(format!(
            "graphhelm-process-reparse-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let target = root.join("target");
        let file = root.join("file.txt");
        let junction = root.join("junction");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(&file, b"x").unwrap();
        let made = super::command("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&junction)
            .arg(&target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let read = |path: &std::path::Path| std::fs::symlink_metadata(path).unwrap();
        let junction_is = is_reparse_point(&read(&junction));
        let target_is = is_reparse_point(&read(&target));
        let file_is = is_reparse_point(&read(&file));
        // Remove the junction itself, never what it points at, before asserting.
        std::fs::remove_dir(&junction).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(junction_is, "a junction is a reparse point");
        assert!(!target_is, "a plain directory is not");
        assert!(!file_is, "a plain file is not");
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_nothing_is_a_reparse_point() {
        let here = std::fs::symlink_metadata(".").unwrap();
        assert!(!super::fs::is_reparse_point(&here));
    }

    #[test]
    fn the_helper_keeps_program_and_capture() {
        let output = super::command("git").arg("--version").output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.starts_with(b"git version "));
    }
}
