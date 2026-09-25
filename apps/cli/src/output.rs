use graphhelm_protocols::Diagnostic;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutput {
    pub ok: bool,
    pub command: &'static str,
    pub data: Option<serde_json::Value>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Outcome {
    pub output: CommandOutput,
    pub exit_code: i32,
}

impl Outcome {
    pub fn success(command: &'static str, data: serde_json::Value) -> Self {
        Self {
            output: CommandOutput {
                ok: true,
                command,
                data: Some(data),
                diagnostics: Vec::new(),
            },
            exit_code: 0,
        }
    }

    pub fn domain(command: &'static str, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            output: CommandOutput {
                ok: false,
                command,
                data: None,
                diagnostics,
            },
            exit_code: 2,
        }
    }

    pub fn application(command: &'static str, diagnostic: Diagnostic) -> Self {
        Self {
            output: CommandOutput {
                ok: false,
                command,
                data: None,
                diagnostics: vec![diagnostic],
            },
            exit_code: 3,
        }
    }

    pub fn internal(command: &'static str, message: impl Into<String>) -> Self {
        Self {
            output: CommandOutput {
                ok: false,
                command,
                data: None,
                diagnostics: vec![Diagnostic::error(
                    "GHI001_INTERNAL",
                    message,
                    "/",
                    "graphhelm",
                )],
            },
            exit_code: 4,
        }
    }

    /// #192: a lint pass computed warnings and the caller must see them regardless of what this
    /// `Outcome` turns out to be — success, a later publish failure, or a driver failure. Five of
    /// six call sites through `graphhelm_graph::lint` only surfaced warnings when errors ALSO
    /// existed (folded into that branch's own `diagnostics.extend`), so a warning-only lint pass
    /// was computed and silently dropped on the success path. This appends rather than replaces:
    /// a failure already carrying its own diagnostics keeps them, with the lint warnings added.
    #[must_use]
    pub fn with_warnings(mut self, warnings: Vec<Diagnostic>) -> Self {
        self.output.diagnostics.extend(warnings);
        self
    }
}

/// Which face one run presents (#1172).
///
/// The JSON envelope is a CONTRACT and it belongs to readers that are not people: pipes, test
/// harnesses, the MCP tool, CI. The rendered summary belongs to the one reader who is a person. A
/// run presents exactly one of them on stdout, and which one is decided by [`face`] alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// The JSON envelope, one line or indented.
    Machine {
        /// Indented rather than one line.
        pretty: bool,
    },
    /// The rendered summary for a person.
    Human,
}

/// The face for this run.
///
/// THE FIRST ROW IS THE ONE THAT MATTERS: without a terminal the answer is always `Machine`, with
/// the caller's own `--pretty`, so every existing reader sees the bytes it saw before #1172. The
/// rest is what a person at a prompt gets: their own `--json`/`--pretty` if they asked for the
/// machine face, otherwise the rendered summary — and indented JSON when this command has no
/// renderer yet, because a wall of one-line JSON is what #1172 was filed about and a command
/// without a renderer must not be left at it.
#[must_use]
pub const fn face(is_terminal: bool, json: bool, pretty: bool, has_renderer: bool) -> Face {
    if !is_terminal {
        return Face::Machine { pretty };
    }
    if json || pretty {
        return Face::Machine { pretty };
    }
    if has_renderer {
        return Face::Human;
    }
    Face::Machine { pretty: true }
}

pub fn print(output: &CommandOutput, pretty: bool) {
    let serialized = if pretty {
        serde_json::to_string_pretty(output)
    } else {
        serde_json::to_string(output)
    }
    .unwrap_or_else(|_| {
        "{\"ok\":false,\"command\":\"internal\",\"data\":null,\"diagnostics\":[]}".into()
    });
    println!("{serialized}");
}

#[cfg(test)]
mod tests {
    use super::{Face, face};

    /// The contract row. No terminal means the machine face WHATEVER else is true — including a
    /// command that has a renderer, which is the row a "render when we can" reading would get
    /// wrong and every piped reader would pay for.
    #[test]
    fn without_a_terminal_every_combination_is_the_machine_face() {
        for json in [false, true] {
            for has_renderer in [false, true] {
                assert_eq!(
                    face(false, json, false, has_renderer),
                    Face::Machine { pretty: false },
                    "json={json} has_renderer={has_renderer}"
                );
                assert_eq!(
                    face(false, json, true, has_renderer),
                    Face::Machine { pretty: true },
                    "json={json} has_renderer={has_renderer}"
                );
            }
        }
    }

    /// A person who asked for the machine face gets it, renderer or not, and `--pretty` keeps
    /// meaning indented JSON rather than becoming a second word for the human face.
    #[test]
    fn a_terminal_that_asked_for_json_gets_json() {
        assert_eq!(
            face(true, true, false, true),
            Face::Machine { pretty: false }
        );
        assert_eq!(face(true, true, true, true), Face::Machine { pretty: true });
        assert_eq!(
            face(true, false, true, true),
            Face::Machine { pretty: true }
        );
    }

    /// The row #1172 exists for: a person, no flags, a command that can render itself.
    #[test]
    fn a_terminal_with_a_renderer_and_no_flag_gets_the_human_face() {
        assert_eq!(face(true, false, false, true), Face::Human);
    }

    /// And the row that keeps every other command from getting worse: no renderer yet, so the
    /// envelope is printed indented instead of as one line.
    #[test]
    fn a_terminal_without_a_renderer_gets_indented_json() {
        assert_eq!(
            face(true, false, false, false),
            Face::Machine { pretty: true }
        );
    }
}
