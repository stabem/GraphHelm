//! The closed builtin call vocabulary of this slice: repository, shell, tests — the three the
//! runtime design's §7 bullet names. Every variant's effect and capability are declared here,
//! in one place, so a new action must decide both deliberately or fail to compile.

use crate::effect::ToolEffect;
use crate::lease::Capability;
use crate::path::RelativePath;

/// Re-exported from `graphhelm-protocols`: it travels on the wire inside `ReuseDecision`, and
/// a wire enum lives in protocols (the `SignalSeverity` precedent).
pub use graphhelm_protocols::FreshnessClass;

/// One tool call. Serialized form is internally tagged twice (`tool`, then `action`), flat for
/// operator ergonomics: `{"tool":"repository","action":"read_file","path":"src/lib.rs"}`.
///
/// Deserialization at trust boundaries goes through [`ToolCall::from_json`], which rejects
/// unknown fields — serde's `deny_unknown_fields` has no effect inside internally-tagged
/// enums (the tag machinery buffers and ignores leftovers), so the checked helper walks the
/// raw JSON against a per-action key table first. The derived `Deserialize` stays available
/// for already-trusted values (round-trips of records this crate itself serialized).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    Repository(RepositoryAction),
    Shell(ShellAction),
    Tests(TestsAction),
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RepositoryAction {
    /// Read one file's bytes. Tier 0: no workspace, no mutation possible by construction —
    /// the host's read path opens the file and does nothing else.
    ReadFile { path: RelativePath },
    /// List files under an optional prefix (git ls-files semantics in the host).
    ListFiles {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<RelativePath>,
    },
    /// Worktree-vs-HEAD diff.
    Diff,
    /// Apply a unified diff inside the Tier 1 workspace.
    ApplyPatch { patch: String },
    /// Commit staged-and-unstaged workspace changes with a message.
    Commit { message: String },
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellAction {
    /// A bare program name; `validate_program_name` at parse boundaries, the lease at
    /// authorize.
    pub program: String,
    #[serde(default)]
    pub arguments: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestsAction {
    /// Arguments to the host-configured runner. The runner program itself is host
    /// configuration, never caller input — a caller cannot rename its way around the lease.
    #[serde(default)]
    pub arguments: Vec<String>,
}

/// Why a serialized call was refused before deserialization. Content-free by the same rule as
/// every refusal in this crate: the message names the rule, never the payload.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum CallParseError {
    #[error("the call is not a JSON object")]
    NotAnObject,
    #[error("the call names no known tool/action")]
    UnknownShape,
    #[error("the call carries a field the named action does not declare")]
    UnknownField,
    #[error("the call's declared fields do not deserialize")]
    Invalid,
    #[error("a commit message is at most {MAX_COMMIT_MESSAGE_BYTES} bytes with no control bytes")]
    MessageBound,
}

/// Upper bound on a commit message, in bytes. The message travels in argv (`git commit -m`,
/// the Task 7 repository tool), so it stays small and printable by rule: argv is visible to
/// every process inspector and has platform limits this bound keeps far away.
pub const MAX_COMMIT_MESSAGE_BYTES: usize = 512;

/// Whether a commit message satisfies the argv bound. Public because the impure host re-checks
/// it as defense in depth: `from_json` guards the trust boundary, but a directly-constructed
/// `ToolCall` never passed through it.
#[must_use]
pub fn valid_commit_message(message: &str) -> bool {
    message.len() <= MAX_COMMIT_MESSAGE_BYTES && !message.bytes().any(|byte| byte < 0x20)
}

impl ToolCall {
    /// Checked deserialization for trust boundaries: rejects unknown fields that the derived
    /// `Deserialize` would silently ignore (internal tagging buffers the object, so serde's
    /// own `deny_unknown_fields` cannot fire — verified by `an_unknown_field_in_a_serialized_
    /// call_is_refused`).
    ///
    /// # Errors
    /// A [`CallParseError`] naming the rule violated, never the content.
    pub fn from_json(text: &str) -> Result<Self, CallParseError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|_| CallParseError::Invalid)?;
        let object = value.as_object().ok_or(CallParseError::NotAnObject)?;
        let tool = object
            .get("tool")
            .and_then(serde_json::Value::as_str)
            .ok_or(CallParseError::UnknownShape)?;
        // The allowed key set per (tool, action). `deny_unknown_fields` on the plain structs
        // covers shell/tests once tagging is stripped, but the table treats every shape the
        // same way so the rule has one home.
        let allowed: &[&str] = match tool {
            "repository" => {
                let action = object
                    .get("action")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(CallParseError::UnknownShape)?;
                match action {
                    "read_file" => &["tool", "action", "path"],
                    "list_files" => &["tool", "action", "prefix"],
                    "diff" => &["tool", "action"],
                    "apply_patch" => &["tool", "action", "patch"],
                    "commit" => &["tool", "action", "message"],
                    _ => return Err(CallParseError::UnknownShape),
                }
            }
            "shell" => &["tool", "program", "arguments"],
            "tests" => &["tool", "arguments"],
            _ => return Err(CallParseError::UnknownShape),
        };
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(CallParseError::UnknownField);
        }
        let call: Self = serde_json::from_value(value).map_err(|_| CallParseError::Invalid)?;
        if let Self::Repository(RepositoryAction::Commit { message }) = &call
            && !valid_commit_message(message)
        {
            return Err(CallParseError::MessageBound);
        }
        Ok(call)
    }

    /// The declared effect of this call. One exhaustive match, no wildcard: a new action must
    /// classify itself here or the crate does not compile. Every spawned program is
    /// `ReversibleWrite` — a process can write, and the broker cannot know less; the ephemeral
    /// workspace is what makes the write reversible (discarding it reverses it).
    #[must_use]
    pub fn effect(&self) -> ToolEffect {
        match self {
            Self::Repository(
                RepositoryAction::ReadFile { .. }
                | RepositoryAction::ListFiles { .. }
                | RepositoryAction::Diff,
            ) => ToolEffect::ReadOnly,
            Self::Repository(
                RepositoryAction::ApplyPatch { .. } | RepositoryAction::Commit { .. },
            )
            | Self::Shell(_)
            | Self::Tests(_) => ToolEffect::ReversibleWrite,
        }
    }

    /// The freshness class of this call's result, when it is cache-eligible at all. One
    /// exhaustive match, the same rule as `effect`: a new action decides deliberately.
    /// Repository reads are exact within a source snapshot (the project HEAD pins them);
    /// everything that writes or spawns caller-shaped work returns `None` — never cached.
    #[must_use]
    pub fn freshness(&self) -> Option<FreshnessClass> {
        match self {
            Self::Repository(
                RepositoryAction::ReadFile { .. }
                | RepositoryAction::ListFiles { .. }
                | RepositoryAction::Diff,
            ) => Some(FreshnessClass::SnapshotClosed),
            Self::Repository(
                RepositoryAction::ApplyPatch { .. } | RepositoryAction::Commit { .. },
            )
            | Self::Shell(_)
            | Self::Tests(_) => None,
        }
    }

    /// The capability this call consumes. Same exhaustiveness rule as [`ToolCall::effect`].
    #[must_use]
    pub fn capability(&self) -> Capability {
        match self {
            Self::Repository(
                RepositoryAction::ReadFile { .. }
                | RepositoryAction::ListFiles { .. }
                | RepositoryAction::Diff,
            ) => Capability::RepositoryRead,
            Self::Repository(
                RepositoryAction::ApplyPatch { .. } | RepositoryAction::Commit { .. },
            ) => Capability::RepositoryWrite,
            Self::Shell(_) => Capability::ShellExecute,
            Self::Tests(_) => Capability::TestsExecute,
        }
    }
}
