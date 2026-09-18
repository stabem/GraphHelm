//! The Graph Architect: a compiler with a model in the middle (#107).
//!
//! `synthesize(profile, catalog, model)` assembles a deterministic prompt, asks a model for ONE
//! draft, validates it through the SAME chain authored graphs take (`load_graph_json` → `lint` →
//! executor viability), repairs a bounded number of times by feeding the diagnostics back, and
//! refuses with those diagnostics attached when the last draft is still invalid. It never
//! publishes and never starts an execution: the document it emits enters the system through
//! `execution start --file` exactly as an authored file does.
//!
//! Two properties are constitutional here rather than incidental:
//!
//! - **Every synthesized graph is born completable (#183).** The compiler stamps
//!   `completion.customs` onto every node that can park for input, and treats the lint warning
//!   `GHG102_UNBOUNDED_CUSTOMS` as a FAILURE of synthesis rather than a warning.
//! - **The catalog is derived from the runtime, and goals outside it are refused (#184).** The
//!   node types offered to the model are the ones `classify::work_kind` executes; the programs a
//!   shell call may name are the operator's allowlist and nothing else, and the architect never
//!   widens it.
//!
//! This crate is pure apart from the model port a caller supplies: no clock, no network, no
//! credentials.

pub mod catalog;
pub mod judge;
pub mod judgment;
pub mod library;
pub mod model;
pub mod profile;
pub mod refusal;
pub mod synthesize;
pub mod template;

pub use catalog::{CapabilityCatalog, TOOL_FAMILIES};
pub use judge::{JudgeModel, RecordedJudgeModel};
pub use judgment::nodes::{NODE_KIND_MISMATCH_CODE, NODE_OFF_GOAL_CODE};
pub use judgment::ranking::COVERAGE_LEVELS;
pub use judgment::red::{KnownFlake, RED_CLASSES, RedClassification, RedExcerpt};
pub use judgment::reuse::Road;
pub use judgment::{Candidate, Extras, JudgmentReport, NodeJudgment, RankingReport, ReuseReport};
pub use library::{GraphLibrary, MAX_TEMPLATES, Parameter, SIDECAR_SUFFIX, Template};
pub use model::{DraftModel, DraftReply, MAX_FIXTURE_BYTES, RecordedDraftModel};
pub use profile::{
    DEFAULT_CLEARANCE_WITHIN_SECONDS, DEFAULT_MAX_NODES, DEFAULT_WAIT_WITHIN_SECONDS,
    MAX_GOAL_BYTES, MAX_MAX_NODES, MODES, TaskProfile,
};
pub use refusal::ArchitectRefusal;
pub use synthesize::{
    API_VERSION, BUDGET_EXCEEDS_PROFILE_CODE, DRAFT_SOURCE, KIND, MAX_NAME_CHARS,
    MAX_REPAIR_ROUNDS, MAX_REPLY_BYTES, NODE_TYPE_NOT_EXECUTABLE_CODE, NOT_JSON_CODE,
    NodeRationale, ORIGIN_LABEL, SynthesizedGraph, TOOL_CALL_MISSING_CODE, stamp_customs,
    synthesize, synthesize_with,
};
pub use template::{
    REPAIR_HEAD, RepairContext, Stance, TEMPLATE, assemble_prompt, prompt_sha256, template_sha256,
};
