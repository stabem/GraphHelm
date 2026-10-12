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

#[cfg(test)]
mod tests {
    #[test]
    fn the_helper_keeps_program_and_capture() {
        let output = super::command("git").arg("--version").output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.starts_with(b"git version "));
    }
}
