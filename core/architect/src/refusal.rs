//! Why the architect declined to emit a document. Every arm names what the operator would have
//! to change — a fixture to record, a program to authorize, a model to reach — and the arms are
//! kept apart on purpose: a refusal laundered into a neighbouring kind hides the cause from the
//! person who has to act on it.

use graphhelm_protocols::Diagnostic;

/// One refusal, serialized with a `kind` tag so every door (CLI, HTTP, MCP) prints the same
/// shape and a caller can match on the kind without parsing prose. Variants AND fields are
/// camelCase on the wire (`promptSha256`, not `prompt_sha256`): `rename_all` alone renames the
/// tags, and a wire field spelled by the Rust identifier was the one snake_case key in an
/// otherwise camelCase envelope.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, thiserror::Error)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ArchitectRefusal {
    /// The caller-supplied profile is outside its own bounds (an empty goal, a node ceiling of
    /// zero, a mode nobody defined). Refused before any prompt is assembled, so no model is
    /// asked to draft against a request the compiler could not validate.
    #[error("profile refused at {pointer}: {message}")]
    InvalidProfile { pointer: String, message: String },
    /// The model door could not be reached or answered with an error (a provider refusal, a
    /// transport failure). The message names the failure class, never a credential.
    #[error("model unavailable: {message}")]
    ModelUnavailable { message: String },
    /// The recorded model holds no reply for this prompt. The hash printed here is the key an
    /// operator records a reply under; a template change moves it, which is how drift becomes a
    /// named event instead of a silent one (D6).
    #[error("no recorded reply for prompt sha256 {prompt_sha256}")]
    FixtureMissing { prompt_sha256: String },
    /// The judge door is unreachable, or a recorded judge file cannot be read. Static prose only.
    #[error("judge unavailable: {message}")]
    JudgeUnavailable { message: String },
    /// The recorded judge has no reply for this request; names the digest so
    /// `ARCHITECT_RECORD=1` can record it.
    #[error("no recorded judge reply for request sha256 {request_sha256}")]
    JudgeMissing { request_sha256: String },
    /// A caller-supplied graph library (spec D8) cannot be read as one: a sidecar without its
    /// document, a file that is not YAML/JSON, a parameter value outside its options, a
    /// placeholder no parameter declares. `path` is the offending file's NAME (or the template
    /// id at fill time) — never a directory, never contents — and `message` is static prose.
    #[error("library {path}: {message}")]
    LibraryInvalid { path: String, message: String },
    /// The last round's reply was not a JSON object at all.
    #[error("round {round}: the reply is not JSON: {message}")]
    NotJson { round: u8, message: String },
    /// Every round's draft failed schema validation, lint, or executor viability; the diagnostics
    /// are those of the LAST draft, verbatim.
    #[error("the draft was still invalid after {rounds} rounds ({} diagnostics)", diagnostics.len())]
    Invalid {
        rounds: u8,
        diagnostics: Vec<Diagnostic>,
    },
    /// A shell call names a program outside the operator's allowlist. Not repaired: the model
    /// cannot authorize a program, and the allowlist is never widened by the architect (#184).
    ///
    /// Reached only by a draft that is otherwise valid: a draft carrying both a lint error and a
    /// foreign program is repaired for the lint error first (the program is not among the
    /// diagnostics fed back), and may end `Invalid` after the last round without this refusal
    /// ever naming the program.
    #[error("node {node} needs program {program}, which the operator has not allowed")]
    CapabilityMissing { node: String, program: String },
    /// The draft has more nodes than the profile permits. Not repaired: the ceiling was stated in
    /// the prompt, and a model that ignores it once is not asked to try again.
    #[error("the draft has {count} nodes; the profile allows {max}")]
    TooManyNodes { count: usize, max: usize },
    /// `GHG102_UNBOUNDED_CUSTOMS` survived stamping. This must be unreachable — the compiler
    /// stamps every node that can park — and it is named so that when the two lists (what parks,
    /// what is stamped) drift, the failure has a kind rather than passing as a warning (#183).
    #[error("nodes {} can still park without customs budgets after stamping", nodes.join(", "))]
    NotCompletable { nodes: Vec<String> },
}
