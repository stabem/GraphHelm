//! The one way this CLI starts a child process.
//!
//! Owner rule: every child runs hidden in the terminal that started GraphHelm and never opens a
//! console window of its own. On Windows a console child of a process without a visible console
//! (an agent harness, a hidden runner) gets a fresh, flashing window; `CREATE_NO_WINDOW` gives it
//! a hidden console instead, which its own children then inherit. Stdio is untouched, so capture
//! and inheritance behave exactly as with a bare `Command::new`. Elsewhere this is `Command::new`.

use std::ffi::OsStr;
use std::process::Command;

/// `CREATE_NO_WINDOW` from the Win32 process creation flags.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A `Command` for `program` that never opens a console window.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    hide_window(&mut command);
    command
}

#[cfg(windows)]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Source guard: a new spawn site in the CLI must go through `command`, or it reopens the
    /// flashing-console defect on Windows.
    #[test]
    fn every_cli_spawn_site_uses_the_hidden_window_helper() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && path != root.join("process.rs")
                {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for (index, line) in text.lines().enumerate() {
                        if line.contains(concat!("Command", "::new(")) {
                            offenders.push(format!("{}:{}", path.display(), index + 1));
                        }
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "spawn through crate::process::command, not Command::new: {offenders:?}"
        );
    }

    #[test]
    fn the_helper_keeps_program_and_capture() {
        let output = super::command("git").arg("--version").output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.starts_with(b"git version "));
    }
}
