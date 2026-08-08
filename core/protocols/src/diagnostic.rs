use serde::{Deserialize, Serialize};

/// Stable diagnostic severity used by CLI and policy gates.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// A deterministic, source-addressed domain diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub path: String,
    pub source: String,
}

impl Diagnostic {
    #[must_use]
    pub fn error(
        code: impl Into<String>,
        message: impl Into<String>,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            path: path.into(),
            source: source.into(),
        }
    }

    #[must_use]
    pub fn warning(
        code: impl Into<String>,
        message: impl Into<String>,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            path: path.into(),
            source: source.into(),
        }
    }
}
