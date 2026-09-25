//! #1172: the colour half of the terminal face.
//!
//! Colour is a property of the STREAM, not of the command: the same envelope rendered into a pipe
//! must carry no escape byte at all, because a file or a `grep` that receives one has been handed
//! decoration it cannot read. So the whole decision is [`enabled`], a pure function over the
//! terminal answer and the three environment variables the ecosystem already agreed on, and every
//! method below is the identity function when it says no.
//!
//! WHAT IS COLOURED IS WHAT CARRIES MEANING. A verdict (ready, refused), a label (the word that
//! names a field), and a command the operator is meant to run: three roles, three colours. A
//! renderer that coloured every word would be decoration, and decoration is what makes the one
//! coloured thing that matters unfindable.

/// The three environment variables that decide colour, read once so the decision below is a pure
/// function of values rather than of the process environment.
#[derive(Clone, Copy, Debug, Default)]
pub struct Environment<'a> {
    /// `NO_COLOR`: set to ANY value, including the empty string, means no colour
    /// (<https://no-color.org>).
    pub no_color: Option<&'a str>,
    /// `TERM`: the single value `dumb` names a terminal that cannot render escapes.
    pub term: Option<&'a str>,
    /// `CLICOLOR_FORCE`: any value but `0` forces colour on, terminal or not — the variable a
    /// caller uses when it captures output on purpose and wants the escapes.
    pub clicolor_force: Option<&'a str>,
}

impl Environment<'static> {
    /// Reads the three variables from the process environment.
    ///
    /// Leaks the three strings deliberately: they live for the whole process, they are read once
    /// per run, and the alternative is threading a lifetime through every caller of a decision
    /// that is made exactly once.
    #[must_use]
    pub fn from_process() -> Self {
        fn read(name: &str) -> Option<&'static str> {
            std::env::var(name)
                .ok()
                .map(|value| &*Box::leak(value.into_boxed_str()))
        }
        Self {
            no_color: read("NO_COLOR"),
            term: read("TERM"),
            clicolor_force: read("CLICOLOR_FORCE"),
        }
    }
}

/// Whether escapes may be written, given the stream and the environment.
///
/// ORDER IS THE CONTRACT. `NO_COLOR` wins over `CLICOLOR_FORCE`: a person who asked a whole
/// machine for no colour must not be overridden by a variable a single script exported. Everything
/// else follows from "is there a terminal there".
#[must_use]
pub fn enabled(is_terminal: bool, environment: Environment<'_>) -> bool {
    if environment.no_color.is_some() {
        return false;
    }
    if environment.term == Some("dumb") {
        return false;
    }
    if environment.clicolor_force.is_some_and(|value| value != "0") {
        return true;
    }
    is_terminal
}

/// The colours the renderers may use, or nothing at all.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    enabled: bool,
}

const RESET: &str = "\u{1b}[0m";
const BOLD_GREEN: &str = "\u{1b}[1;32m";
const BOLD_RED: &str = "\u{1b}[1;31m";
const BOLD: &str = "\u{1b}[1m";
const DIM: &str = "\u{1b}[2m";
const CYAN: &str = "\u{1b}[36m";

impl Palette {
    /// A palette that writes escapes.
    #[must_use]
    pub const fn coloured() -> Self {
        Self { enabled: true }
    }

    /// A palette that writes none. Every method is the identity function.
    #[must_use]
    pub const fn plain() -> Self {
        Self { enabled: false }
    }

    /// The palette this stream and environment allow.
    #[must_use]
    pub fn decide(is_terminal: bool, environment: Environment<'_>) -> Self {
        if enabled(is_terminal, environment) {
            Self::coloured()
        } else {
            Self::plain()
        }
    }

    /// Whether this palette writes escapes. Callers use it to decide whether the platform's
    /// console needs its virtual-terminal mode turned on before anything is printed.
    #[must_use]
    pub const fn writes_escapes(self) -> bool {
        self.enabled
    }

    fn wrap(self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("{code}{text}{RESET}")
        } else {
            text.to_owned()
        }
    }

    /// A verdict that went the operator's way.
    #[must_use]
    pub fn good(self, text: &str) -> String {
        self.wrap(BOLD_GREEN, text)
    }

    /// A verdict that did not: a refusal, a failed check.
    #[must_use]
    pub fn bad(self, text: &str) -> String {
        self.wrap(BOLD_RED, text)
    }

    /// A name the operator gave or must recognise — a route id, a project path.
    #[must_use]
    pub fn name(self, text: &str) -> String {
        self.wrap(BOLD, text)
    }

    /// The word that labels a field, and the prose that explains a step.
    #[must_use]
    pub fn label(self, text: &str) -> String {
        self.wrap(DIM, text)
    }

    /// A command the operator is meant to run.
    #[must_use]
    pub fn command(self, text: &str) -> String {
        self.wrap(CYAN, text)
    }
}

/// Turns on virtual-terminal processing for this process's stdout, so a legacy Windows console
/// renders escapes instead of printing them. A no-op everywhere else, and a no-op when the call
/// fails: a console that refuses the mode is one the operator sees escapes on, which is ugly and
/// is not a reason to fail a command that otherwise worked.
#[cfg(windows)]
pub fn prepare_stream() {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE, SetConsoleMode,
    };

    // Both streams, because either one can be the terminal this run writes escapes to: the
    // rendering goes to stdout and the #1150 summary goes to stderr.
    for stream in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: every call below is a console API taking a handle this process owns and a mode
        // read from that same handle. `GetStdHandle` returns a borrowed handle, never closed here.
        unsafe {
            let handle = GetStdHandle(stream);
            if handle.is_null() {
                continue;
            }
            let mut mode = 0;
            if GetConsoleMode(handle, &raw mut mode) == 0 {
                continue;
            }
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

/// No console mode to turn on outside Windows.
#[cfg(not(windows))]
pub const fn prepare_stream() {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stream decides when nothing in the environment speaks. This is the row that keeps a
    /// pipe clean: no terminal, no variables, no escapes.
    #[test]
    fn a_pipe_with_a_silent_environment_gets_no_colour() {
        assert!(!enabled(false, Environment::default()));
        assert!(enabled(true, Environment::default()));
    }

    /// `NO_COLOR` is honoured at ANY value, the empty string included — the spelling the standard
    /// asks for and the one a shell produces with `NO_COLOR=`.
    #[test]
    fn no_color_at_any_value_beats_a_terminal() {
        for value in ["", "0", "1", "please"] {
            let environment = Environment {
                no_color: Some(value),
                ..Environment::default()
            };
            assert!(
                !enabled(true, environment),
                "NO_COLOR={value:?} should have refused colour"
            );
        }
    }

    /// And it beats the force variable too. A person who set `NO_COLOR` on their machine outranks
    /// a script that exported `CLICOLOR_FORCE`.
    #[test]
    fn no_color_beats_clicolor_force() {
        let environment = Environment {
            no_color: Some("1"),
            clicolor_force: Some("1"),
            ..Environment::default()
        };
        assert!(!enabled(true, environment));
        assert!(!enabled(false, environment));
    }

    /// `TERM=dumb` is the terminal that cannot render escapes; every other `TERM` says nothing.
    #[test]
    fn term_dumb_refuses_and_other_terms_do_not() {
        let dumb = Environment {
            term: Some("dumb"),
            ..Environment::default()
        };
        assert!(!enabled(true, dumb));
        let xterm = Environment {
            term: Some("xterm-256color"),
            ..Environment::default()
        };
        assert!(enabled(true, xterm));
    }

    /// `CLICOLOR_FORCE` turns colour on without a terminal, and `0` is the one value that does
    /// not — the convention every tool that reads it follows.
    #[test]
    fn clicolor_force_turns_colour_on_without_a_terminal_except_at_zero() {
        let forced = Environment {
            clicolor_force: Some("1"),
            ..Environment::default()
        };
        assert!(enabled(false, forced));
        let zero = Environment {
            clicolor_force: Some("0"),
            ..Environment::default()
        };
        assert!(!enabled(false, zero));
    }

    /// A plain palette is the identity function on every role, and a coloured one is not. The
    /// second half is what keeps a "colour support" that silently renders nothing from passing.
    #[test]
    fn a_plain_palette_writes_no_escape_and_a_coloured_one_does() {
        let plain = Palette::plain();
        for rendered in [
            plain.good("ready"),
            plain.bad("refused"),
            plain.name("route"),
            plain.label("provider"),
            plain.command("graphhelm serve"),
        ] {
            assert!(
                !rendered.contains('\u{1b}'),
                "a plain palette wrote an escape: {rendered:?}"
            );
        }
        let coloured = Palette::coloured();
        for rendered in [
            coloured.good("ready"),
            coloured.bad("refused"),
            coloured.name("route"),
            coloured.label("provider"),
            coloured.command("graphhelm serve"),
        ] {
            assert!(
                rendered.starts_with('\u{1b}') && rendered.ends_with(RESET),
                "a coloured palette left text unwrapped: {rendered:?}"
            );
        }
    }

    /// The five roles are five different codes. A palette that mapped two roles to one colour
    /// would render a verdict and a label alike, which is the failure this file exists to avoid.
    #[test]
    fn the_roles_do_not_share_a_colour() {
        let palette = Palette::coloured();
        let rendered = [
            palette.good("x"),
            palette.bad("x"),
            palette.name("x"),
            palette.label("x"),
            palette.command("x"),
        ];
        let mut distinct = rendered.clone().to_vec();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            rendered.len(),
            "two roles share one colour: {rendered:?}"
        );
    }
}
