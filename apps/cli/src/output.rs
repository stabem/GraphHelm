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
