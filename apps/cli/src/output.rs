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
