//! Bounded, advisory Dreams shadow contracts.

use serde::{Deserialize, Serialize};

use crate::{OpaqueId, RawSha256, RepositoryScope, SafeCode};

pub const MAX_DREAM_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_DREAM_EVIDENCE: usize = 32;

macro_rules! dream_vocab {
    ($name:ident { $( $variant:ident => $wire:literal ),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $( $variant ),+ }
        impl $name {
            #[must_use]
            pub const fn wire_name(self) -> &'static str { match self { $(Self::$variant => $wire),+ } }
        }
    };
}

dream_vocab!(DreamTrigger {
    Manual => "manual",
    Idle => "idle",
    Incident => "incident",
});

dream_vocab!(DreamCategory {
    MemoryExpiry => "memory_expiry",
    ConflictReport => "conflict_report",
    RetrievalOptimization => "retrieval_optimization",
    CodeFinding => "code_finding",
});

dream_vocab!(DreamOutcome {
    AdvisoryProposal => "advisory_proposal",
    Discarded => "discarded",
});

dream_vocab!(DreamCriticVerdict {
    Accepted => "accepted",
    Rejected => "rejected",
});

dream_vocab!(DreamRefusalCode {
    InputEmpty => "input_empty",
    InputTooLarge => "input_too_large",
    MissingEvidence => "missing_evidence",
    WriteCritical => "write_critical",
    ScopeMismatch => "scope_mismatch",
    CriticNotIndependent => "critic_not_independent",
    CriticRejected => "critic_rejected",
    TaskMappingMissing => "task_mapping_missing",
});

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DreamTaskRequest {
    pub task_id: OpaqueId,
    pub dream_run_id: OpaqueId,
    pub scope: RepositoryScope,
    pub finding_sha256: RawSha256,
    pub evidence_sha256: Vec<RawSha256>,
    pub origin: SafeCode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DreamShadowRecorded {
    pub run_id: OpaqueId,
    pub scope: RepositoryScope,
    pub trigger: DreamTrigger,
    pub category: DreamCategory,
    pub input_sha256: RawSha256,
    pub evidence_sha256: Vec<RawSha256>,
    pub outcome: DreamOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_code: Option<DreamRefusalCode>,
    pub planner_id: OpaqueId,
    pub critic_id: OpaqueId,
    pub critic_verdict: DreamCriticVerdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_request: Option<DreamTaskRequest>,
}
