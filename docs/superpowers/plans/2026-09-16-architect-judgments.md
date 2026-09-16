# Graph Architect typed judgments (System One) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Graph Architect a second, typed model port (a System One judge, TypeSafe's Jev) and use it at four sites: per-node judgments, ranking N drafts, deciding reuse/adapt/create, and filling a library template's closed-set parameters.

**Architecture:** The judge is a new sync port `JudgeModel` beside `DraftModel`, with wire types in `core/gateway`, an HTTP adapter in `adapters/model-gateway`, and a recorded fixture door for every test. Judgments enter `compile_round` only as repairable diagnostics, a fixed ranking policy, or report fields; `synthesize` keeps its signature and golden, `synthesize_with(.., &Extras)` is the new entry. CLI, HTTP and MCP gain three fields and stay byte-identical.

**Tech Stack:** Rust 1.97.1 workspace (`serde`, `serde_json`, `sha2`, `ureq`, `thiserror`), fake-TCP-server tests, recorded JSON fixtures, `ci/gate.ps1`.

**Spec:** `docs/superpowers/specs/2026-09-16-architect-judgments-design.md` (D1–D10). Parent: `docs/superpowers/specs/2026-09-11-graph-architect-design.md`.

**Issue:** epic #1109 (one child issue per task, branch `issue-<N>-…`, PR `Closes #<N>`, two review passes, merge by a third lane — `AGENTS.md`, `.factory/MERGE-CHECKLIST.md`).

## Global Constraints

- "LLMs may classify and propose. Deterministic code enforces schemas, graph invariants, policies, permissions, and state transitions." A judge answer never removes a diagnostic, edits a draft, widens the allowlist, or skips `compile_round`.
- "The Policy Engine has no dependency on an LLM, prompt, provider SDK, model runtime, network, or browser." Nothing here touches `core/policy`.
- "Never add automatic paid BYOK/OpenRouter fallback." A judge route is named explicitly or absent; absent means today's behaviour.
- "Secrets never appear in Graph DSL, Context Capsules, artifacts, logs, fixtures, crash output, or exported manifests." The TypeSafe key is a leased `SecretBytes`, exposed only while building the `Authorization` header.
- `core/architect` stays pure: no clock, no network, no credentials. Only the caller supplies ports.
- `core/architect/fixtures/first-compile/expected.json` must not change (spec D4). Every new `SynthesizedGraph` field is `Option` with `#[serde(skip_serializing_if = "Option::is_none")]`.
- All documentation in English. Commits: `type(scope): description`, body `Closes #N`, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`, plus the identity line `Session: <name> | Head: <sha8>`.
- Run the gate before every push: `./ci/gate.ps1` (or `-SkipPostgres` for a change that cannot touch persistence, reported as such). One gate on the HDD at a time; see `AGENTS.md` for the target-dir rules.
- Clippy runs with `-D warnings`; `rustfmt` pinned. `cargo fmt --all` before every commit.
- Pinned exact dependency versions in `Cargo.toml` (`=x.y.z`). This plan adds no new dependency.

---

## File structure

| Path | Responsibility |
|---|---|
| `core/gateway/src/judgment.rs` (new) | Wire types `Question`, `JudgeRequest`, `Answer`, `JudgeReply`, `request_sha256`. Plain serde, no logic beyond the digest. |
| `core/gateway/src/lib.rs` (modify) | `pub mod judgment;` |
| `core/gateway/src/manifest.rs` (modify, `validate_direct_api` ~L474-483) | Admit provider `typesafe` for `direct_api`. |
| `adapters/model-gateway/src/systemone.rs` (new) | `SystemOneAdapter::call(key, &JudgeRequest) -> Result<JudgeReply, GatewayError>` over `HttpTransport`. |
| `adapters/model-gateway/src/byok.rs` (modify, `call` ~L68-86) | A `typesafe` route on the draft door refuses `UnsupportedCapability` explicitly. |
| `adapters/model-gateway/src/lib.rs` (modify) | `pub mod systemone;` |
| `adapters/model-gateway/tests/systemone_adapter.rs` (new) | Fake-server contract tests. |
| `core/architect/src/judge.rs` (new) | `JudgeModel` trait, `RecordedJudgeModel`, `JudgeMissing` plumbing. |
| `core/architect/src/judgment/mod.rs` (new) | `Extras`, `JudgmentReport`, shared state builders. |
| `core/architect/src/judgment/policy.rs` (new) | Threshold constants and the pure decision functions. |
| `core/architect/src/judgment/nodes.rs` (new) | Site 3: per-node questions → diagnostics. |
| `core/architect/src/judgment/ranking.rs` (new) | Site 2: stances, candidate ranking. |
| `core/architect/src/judgment/reuse.rs` (new) | Site 4 + 1: road decision, template choice, parameter fill. |
| `core/architect/src/library.rs` (new) | `GraphLibrary`, `Template`, `Parameter`, loading and substitution. |
| `core/architect/src/template.rs` (modify) | `assemble_prompt` gains `stance: Option<&Stance>`; `<seed>` block. |
| `core/architect/src/synthesize.rs` (modify) | `synthesize_with`, new report fields, judge hook in `compile_round`. |
| `core/architect/src/refusal.rs` (modify) | `JudgeMissing`, `JudgeUnavailable`, `LibraryInvalid`. |
| `core/architect/src/lib.rs` (modify) | Re-exports. |
| `core/architect/tests/judge.rs`, `judgment_nodes.rs`, `judgment_ranking.rs`, `library.rs` (new) | Tier A cells. |
| `core/architect/fixtures/judge/*.json`, `core/architect/fixtures/library/*` (new) | Recorded judge replies; a two-template library. |
| `apps/cli/src/args.rs` (modify, `Synthesize` ~L1012) | `--judge-route`, `--drafts`, `--library`. |
| `apps/cli/src/commands/architect.rs` (modify) | `SynthesizeRequest` fields, `GatewayJudgeModel`, `build_judge`. |
| `apps/cli/src/commands/serve/routes.rs` (modify, `synthesize` ~L196) | Fields `judgeRoute`, `drafts`, `library`; `ServeJudgeModel`. |
| `apps/cli/src/commands/mcp/tools.rs` (modify, `synthesize_schema` ~L676, dispatch ~L1337) | Same three fields. |
| `docs/harness/GRAPH_ARCHITECT.md`, `docs/DECISION_REGISTER.md`, `CHANGELOG.md`, `docs/acceptance/` | Records. |

---

### Task 1: Judgment wire types in `core/gateway`

**Files:**
- Create: `core/gateway/src/judgment.rs`
- Modify: `core/gateway/src/lib.rs` (add `pub mod judgment;` beside `pub mod call;`)
- Test: `core/gateway/tests/judgment_wire.rs`

**Interfaces:**
- Consumes: `graphhelm_gateway::call::Usage`.
- Produces: `Question::{Noul{instructions, criteria: Option<NoulCriteria>}, Choice{instructions, criteria: BTreeMap<String, Option<String>>}, Score{instructions, criteria: Vec<String>}}`, `JudgeRequest { state: Value, model: String, questions: BTreeMap<String, Question> }`, `Answer::{Noul{noul: f64}, Choice{choice: String, probabilities: BTreeMap<String,f64>, confidence: f64}, Score{score: f64, legend: BTreeMap<String,String>, probabilities: BTreeMap<String,f64>, confidence: f64}}`, `JudgeReply { model: String, answers: BTreeMap<String, Answer>, usage: Usage }`, `pub const JEV_LATEST: &str = "jev-latest"`, `pub fn request_sha256(request: &JudgeRequest) -> String`.

- [ ] **Step 1: Write the failing wire-pin test**

`core/gateway/tests/judgment_wire.rs`:

```rust
//! The judgment types serialize to exactly the TypeSafe `/v1/systemone` request and response
//! bodies (docs.typesafe.ai/api, read 2026-09-16). These are the documented examples, verbatim.

use std::collections::BTreeMap;

use graphhelm_gateway::call::Usage;
use graphhelm_gateway::judgment::{
    Answer, JEV_LATEST, JudgeReply, JudgeRequest, NoulCriteria, Question, request_sha256,
};

fn documented_request() -> JudgeRequest {
    let mut questions = BTreeMap::new();
    questions.insert(
        "department".to_owned(),
        Question::Choice {
            instructions: "Which team should handle this?".to_owned(),
            criteria: BTreeMap::from([
                ("billing".to_owned(), Some("Payments, invoicing, refunds".to_owned())),
                ("technical".to_owned(), Some("Bugs, outages, integrations".to_owned())),
                ("sales".to_owned(), Some("Pricing, upgrades, new accounts".to_owned())),
            ]),
        },
    );
    questions.insert(
        "frustration".to_owned(),
        Question::Score {
            instructions: "How frustrated is the customer?".to_owned(),
            criteria: vec!["Calm".to_owned(), "Frustrated".to_owned(), "Very angry".to_owned()],
        },
    );
    questions.insert(
        "is_urgent".to_owned(),
        Question::Noul {
            instructions: "Does this convey urgency?".to_owned(),
            criteria: Some(NoulCriteria {
                r#true: "Explicitly time-sensitive".to_owned(),
                r#false: "No urgency expressed".to_owned(),
            }),
        },
    );
    JudgeRequest {
        state: serde_json::json!("Help! My payouts have been failing for 3 days."),
        model: JEV_LATEST.to_owned(),
        questions,
    }
}

#[test]
fn the_request_serializes_to_the_documented_body() {
    let value = serde_json::to_value(documented_request()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "state": "Help! My payouts have been failing for 3 days.",
            "model": "jev-latest",
            "questions": {
                "department": {
                    "type": "choice",
                    "instructions": "Which team should handle this?",
                    "criteria": {
                        "billing": "Payments, invoicing, refunds",
                        "technical": "Bugs, outages, integrations",
                        "sales": "Pricing, upgrades, new accounts"
                    }
                },
                "frustration": {
                    "type": "score",
                    "instructions": "How frustrated is the customer?",
                    "criteria": ["Calm", "Frustrated", "Very angry"]
                },
                "is_urgent": {
                    "type": "noul",
                    "instructions": "Does this convey urgency?",
                    "criteria": { "true": "Explicitly time-sensitive", "false": "No urgency expressed" }
                }
            }
        })
    );
}

#[test]
fn a_noul_without_criteria_omits_the_field() {
    let question = Question::Noul {
        instructions: "Does this convey urgency?".to_owned(),
        criteria: None,
    };
    assert_eq!(
        serde_json::to_value(question).unwrap(),
        serde_json::json!({ "type": "noul", "instructions": "Does this convey urgency?" })
    );
}

#[test]
fn the_documented_response_parses_to_typed_answers() {
    let body = r#"{
      "model": "jev-latest",
      "answers": {
        "department": {
          "type": "choice",
          "choice": "technical",
          "probabilities": { "billing": 0.08, "technical": 0.85, "sales": 0.07 },
          "confidence": 0.82
        },
        "frustration": {
          "type": "score",
          "score": 1.6,
          "legend": { "0": "Calm", "1": "Frustrated", "2": "Very angry" },
          "probabilities": { "0": 0.05, "1": 0.3, "2": 0.65 },
          "confidence": 0.78
        },
        "is_urgent": { "type": "noul", "noul": 0.92 }
      },
      "usage": { "input_tokens": 312, "output_tokens": 48 }
    }"#;
    let reply: JudgeReply = serde_json::from_str(body).unwrap();
    assert_eq!(reply.model, "jev-latest");
    assert_eq!(
        reply.usage,
        Usage { input_tokens: Some(312), output_tokens: Some(48) }
    );
    match &reply.answers["department"] {
        Answer::Choice { choice, probabilities, confidence } => {
            assert_eq!(choice, "technical");
            assert_eq!(probabilities["technical"], 0.85);
            assert_eq!(*confidence, 0.82);
        }
        other => panic!("{other:?}"),
    }
    match &reply.answers["frustration"] {
        Answer::Score { score, legend, confidence, .. } => {
            assert_eq!(*score, 1.6);
            assert_eq!(legend["2"], "Very angry");
            assert_eq!(*confidence, 0.78);
        }
        other => panic!("{other:?}"),
    }
    match &reply.answers["is_urgent"] {
        Answer::Noul { noul } => assert_eq!(*noul, 0.92),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_answer_of_an_unknown_type_is_refused_not_defaulted() {
    let body = r#"{"model":"jev-latest","answers":{"x":{"type":"essay","text":"no"}},"usage":{"input_tokens":1,"output_tokens":1}}"#;
    assert!(serde_json::from_str::<JudgeReply>(body).is_err());
}

#[test]
fn the_request_digest_is_a_pure_function_of_canonical_bytes() {
    let a = request_sha256(&documented_request());
    let b = request_sha256(&documented_request());
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
    let mut other = documented_request();
    other.state = serde_json::json!("a different state");
    assert_ne!(a, request_sha256(&other));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p graphhelm-gateway --test judgment_wire`
Expected: compile error, `could not find judgment in graphhelm_gateway`.

- [ ] **Step 3: Write the types**

`core/gateway/src/judgment.rs`:

```rust
//! Wire-neutral types for one typed judgment call and its reply: a System One request
//! (`docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2.6). The JSON these serialize to IS the TypeSafe
//! `POST /v1/systemone` body and reply (docs.typesafe.ai/api), pinned by
//! `tests/judgment_wire.rs` against the documented examples. Like `call.rs`: plain serde, no
//! validation, no network, nothing here knows an adapter.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::call::Usage;

/// TypeSafe's flagship System One model id.
pub const JEV_LATEST: &str = "jev-latest";

/// What a yes and a no mean, when the question needs saying.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub r#true: String,
    #[serde(rename = "false")]
    pub r#false: String,
}

/// One closed question. The id it travels under is the caller's key in
/// [`JudgeRequest::questions`]; it is not sent to the model as meaning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Whether a condition holds: the reply is the probability of yes.
    Noul {
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// One option of a closed set; `None` when an option needs no rubric.
    Choice {
        instructions: String,
        criteria: BTreeMap<String, Option<String>>,
    },
    /// A position on ordered levels; at least two levels.
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JudgeRequest {
    pub state: serde_json::Value,
    pub model: String,
    pub questions: BTreeMap<String, Question>,
}

/// One answer, under the id of its question. `Choice` and `Score` carry a `confidence` derived
/// from their distribution; `Noul` carries none (docs.typesafe.ai/confidence).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JudgeReply {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    #[serde(with = "usage_wire")]
    pub usage: Usage,
}

/// TypeSafe spells usage `input_tokens`/`output_tokens`; [`Usage`] is camelCase on every other
/// wire in this workspace, so the reply carries its own field names here and nowhere else.
mod usage_wire {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::call::Usage;

    #[derive(Serialize, Deserialize)]
    struct Wire {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    }

    pub fn serialize<S: Serializer>(usage: &Usage, serializer: S) -> Result<S::Ok, S::Error> {
        Wire {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Usage, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        Ok(Usage {
            input_tokens: wire.input_tokens,
            output_tokens: wire.output_tokens,
        })
    }
}

/// The lowercase hex SHA-256 of the request's canonical JSON: `serde_json`'s `Map` is a
/// `BTreeMap`, so keys serialize sorted and the same request always yields the same bytes. This
/// is the key a recorded judge fixture is looked up under.
#[must_use]
pub fn request_sha256(request: &JudgeRequest) -> String {
    let bytes = serde_json::to_vec(request).expect("JudgeRequest is plain data and always serializes");
    hex::encode(Sha256::digest(bytes))
}
```

Add to `core/gateway/Cargo.toml` `[dependencies]` if absent: `sha2.workspace = true` and `hex.workspace = true` (check with `grep -n "sha2\|hex" core/gateway/Cargo.toml`; both are already workspace deps used by `core/architect`).

`core/gateway/src/lib.rs`: add `pub mod judgment;`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p graphhelm-gateway --test judgment_wire`
Expected: 5 passed.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy -p graphhelm-gateway --all-targets -- -D warnings
git add core/gateway && git commit -m "feat(gateway): judgment wire types for System One requests

Closes #<task-issue>

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: The judge port and its recorded door in `core/architect`

**Files:**
- Create: `core/architect/src/judge.rs`
- Modify: `core/architect/src/refusal.rs` (two variants), `core/architect/src/lib.rs` (module + re-exports)
- Test: `core/architect/tests/judge.rs`

**Interfaces:**
- Consumes: Task 1 types.
- Produces: `pub trait JudgeModel { fn judge(&self, request: &JudgeRequest) -> Result<JudgeReply, ArchitectRefusal>; }`, `RecordedJudgeModel::{single(request_sha256, reply), from_json(bytes), from_file(path), recorded_requests()}`, `ArchitectRefusal::JudgeMissing { request_sha256: String }`, `ArchitectRefusal::JudgeUnavailable { message: String }`.

- [ ] **Step 1: Write the failing tests**

`core/architect/tests/judge.rs`:

```rust
use std::collections::BTreeMap;

use graphhelm_architect::{ArchitectRefusal, JudgeModel, RecordedJudgeModel};
use graphhelm_gateway::call::Usage;
use graphhelm_gateway::judgment::{Answer, JEV_LATEST, JudgeReply, JudgeRequest, Question, request_sha256};

fn request() -> JudgeRequest {
    JudgeRequest {
        state: serde_json::json!({ "goal": "check that the repository builds" }),
        model: JEV_LATEST.to_owned(),
        questions: BTreeMap::from([(
            "on_goal".to_owned(),
            Question::Noul { instructions: "Does the node serve the goal?".to_owned(), criteria: None },
        )]),
    }
}

fn reply() -> JudgeReply {
    JudgeReply {
        model: JEV_LATEST.to_owned(),
        answers: BTreeMap::from([("on_goal".to_owned(), Answer::Noul { noul: 0.9 })]),
        usage: Usage::default(),
    }
}

#[test]
fn a_recorded_judge_answers_only_the_request_it_recorded() {
    let key = request_sha256(&request());
    let model = RecordedJudgeModel::single(&key, &reply());
    assert_eq!(model.judge(&request()).unwrap(), reply());
    let mut other = request();
    other.state = serde_json::json!({ "goal": "something else" });
    match model.judge(&other) {
        Err(ArchitectRefusal::JudgeMissing { request_sha256 }) => {
            assert_eq!(request_sha256, graphhelm_gateway::judgment::request_sha256(&other));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_recorded_judge_file_is_bounded_and_shaped() {
    let key = request_sha256(&request());
    let json = serde_json::json!({ "answers": { key: reply() } }).to_string();
    let model = RecordedJudgeModel::from_json(json.as_bytes()).unwrap();
    assert_eq!(model.recorded_requests(), vec![key.as_str()]);

    let not_hex = serde_json::json!({ "answers": { "nope": reply() } }).to_string();
    match RecordedJudgeModel::from_json(not_hex.as_bytes()) {
        Err(ArchitectRefusal::JudgeUnavailable { message }) => {
            assert!(message.contains("sha256"), "{message}");
        }
        other => panic!("{other:?}"),
    }

    let wrong_shape = serde_json::json!({ "replies": {} }).to_string();
    assert!(matches!(
        RecordedJudgeModel::from_json(wrong_shape.as_bytes()),
        Err(ArchitectRefusal::JudgeUnavailable { .. })
    ));

    let too_big = vec![b' '; graphhelm_architect::MAX_FIXTURE_BYTES + 1];
    assert!(matches!(
        RecordedJudgeModel::from_json(&too_big),
        Err(ArchitectRefusal::JudgeUnavailable { .. })
    ));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p graphhelm-architect --test judge`
Expected: compile error, `JudgeModel` not found.

- [ ] **Step 3: Implement**

`core/architect/src/refusal.rs`, add two variants to `ArchitectRefusal` (after `FixtureMissing`):

```rust
    /// The judge door is unreachable, or a recorded judge file cannot be read. Static prose only.
    #[error("judge unavailable: {message}")]
    JudgeUnavailable { message: String },
    /// The recorded judge has no reply for this request; names the digest so
    /// `ARCHITECT_RECORD=1` can record it.
    #[error("no recorded judge reply for request sha256 {request_sha256}")]
    JudgeMissing { request_sha256: String },
```

`core/architect/src/judge.rs`:

```rust
//! The judge port: the compiler's SECOND model door (spec D1). A `JudgeModel` answers closed
//! questions over state the compiler hands it and never drafts. `RecordedJudgeModel` is the
//! keyless door every test uses, keyed by `request_sha256` exactly as `RecordedDraftModel` is
//! keyed by the prompt hash.

use std::collections::BTreeMap;
use std::path::Path;

use graphhelm_gateway::judgment::{JudgeReply, JudgeRequest, request_sha256};

use crate::model::MAX_FIXTURE_BYTES;
use crate::refusal::ArchitectRefusal;

pub trait JudgeModel {
    /// # Errors
    /// `JudgeUnavailable` when the door cannot answer; `JudgeMissing` from the recorded door.
    fn judge(&self, request: &JudgeRequest) -> Result<JudgeReply, ArchitectRefusal>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordedJudgeModel {
    answers: BTreeMap<String, JudgeReply>,
}

impl RecordedJudgeModel {
    #[must_use]
    pub fn single(request_sha256: &str, reply: &JudgeReply) -> Self {
        Self {
            answers: BTreeMap::from([(request_sha256.to_owned(), reply.clone())]),
        }
    }

    /// # Errors
    /// `JudgeUnavailable` for a file over 4 MiB, not JSON, not `{"answers": {<sha256>: reply}}`.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ArchitectRefusal> {
        let refuse = |message: &str| ArchitectRefusal::JudgeUnavailable {
            message: format!("recorded judge: {message}"),
        };
        if bytes.len() > MAX_FIXTURE_BYTES {
            return Err(refuse("the file exceeds the 4 MiB limit"));
        }
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| refuse("the file is not JSON"))?;
        let answers = value
            .as_object()
            .and_then(|object| object.get("answers"))
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| refuse("expected an object with an \"answers\" object"))?;
        let mut parsed = BTreeMap::new();
        for (key, reply) in answers {
            if !crate::model::is_sha256_hex(key) {
                return Err(refuse(
                    "a key of \"answers\" must be the lowercase hex sha256 of the request",
                ));
            }
            let reply: JudgeReply = serde_json::from_value(reply.clone())
                .map_err(|_| refuse("a value of \"answers\" must be a judge reply"))?;
            parsed.insert(key.clone(), reply);
        }
        Ok(Self { answers: parsed })
    }

    /// # Errors
    /// As [`Self::from_json`], plus a path that is not a regular file.
    pub fn from_file(path: &Path) -> Result<Self, ArchitectRefusal> {
        let unavailable = |message: String| ArchitectRefusal::JudgeUnavailable {
            message: format!("recorded judge: {message}"),
        };
        let metadata = std::fs::metadata(path)
            .map_err(|error| unavailable(format!("cannot inspect the file: {}", error.kind())))?;
        if !metadata.is_file() {
            return Err(unavailable("not a regular file".to_owned()));
        }
        if metadata.len() > MAX_FIXTURE_BYTES as u64 {
            return Err(unavailable("the file exceeds the 4 MiB limit".to_owned()));
        }
        let bytes = std::fs::read(path)
            .map_err(|error| unavailable(format!("cannot read the file: {}", error.kind())))?;
        Self::from_json(&bytes)
    }

    #[must_use]
    pub fn recorded_requests(&self) -> Vec<&str> {
        self.answers.keys().map(String::as_str).collect()
    }
}

impl JudgeModel for RecordedJudgeModel {
    fn judge(&self, request: &JudgeRequest) -> Result<JudgeReply, ArchitectRefusal> {
        let key = request_sha256(request);
        self.answers
            .get(&key)
            .cloned()
            .ok_or(ArchitectRefusal::JudgeMissing { request_sha256: key })
    }
}
```

In `core/architect/src/model.rs` change `fn is_sha256_hex` to `pub(crate) fn is_sha256_hex`.

`core/architect/src/lib.rs`: add `pub mod judge;` and `pub use judge::{JudgeModel, RecordedJudgeModel};`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p graphhelm-architect --test judge`
Expected: 2 passed. Also `cargo test -p graphhelm-architect` — every existing test still green (nothing else changed).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p graphhelm-architect --all-targets -- -D warnings
git add core/architect && git commit -m "feat(architect): JudgeModel port and the recorded judge door

Closes #<task-issue>

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: The System One HTTP adapter and the `typesafe` provider

**Files:**
- Create: `adapters/model-gateway/src/systemone.rs`
- Modify: `adapters/model-gateway/src/lib.rs` (`pub mod systemone;`), `adapters/model-gateway/src/byok.rs:68-86` (explicit `typesafe` refusal arm), `core/gateway/src/manifest.rs:474-483` (admit `typesafe`)
- Test: `adapters/model-gateway/tests/systemone_adapter.rs`, extend `core/gateway/tests/manifest.rs` (or wherever `validate_direct_api` is tested: `grep -rn "must use provider" core/gateway/tests`)

**Interfaces:**
- Consumes: Task 1 types; `HttpTransport`, `TransportRequest`, `UreqTransport` (`adapters/model-gateway/src/transport.rs`); `SecretBytes` (`graphhelm_events`); `ModelRoute` (`core/gateway/src/manifest.rs`).
- Produces: `SystemOneAdapter::new(route: &ModelRoute, transport: Arc<dyn HttpTransport>) -> Self`, `SystemOneAdapter::call(&self, key: &SecretBytes, request: &JudgeRequest) -> Result<JudgeReply, GatewayError>`, `pub const SYSTEMONE_PATH: &str = "/v1/systemone"`, `pub const TYPESAFE_PROVIDER: &str = "typesafe"`.

- [ ] **Step 1: Write the failing adapter tests**

`adapters/model-gateway/tests/systemone_adapter.rs` — copy the `fake_server`, `read_request`, `CapturedRequest`, `header_value`, `SENTINEL`, `sentinel_key` helpers verbatim from `adapters/model-gateway/tests/byok_adapters.rs` (they are file-private there; `apps/cli` and this crate have no shared test lib), then:

```rust
fn build_manifest(base_url: &str, provider: &str) -> RouteManifest {
    let json = serde_json::json!({
        "manifestVersion": 1,
        "routes": [{
            "id": "judge",
            "provider": provider,
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": base_url,
            "model": "jev-latest",
            "credentialRef": "secret_typesafe",
            "profiles": ["balanced_reasoning"],
            "enabled": true
        }]
    });
    RouteManifest::from_json(&json.to_string())
        .unwrap_or_else(|error| panic!("fixture manifest must be valid: {error}"))
}

fn request() -> JudgeRequest {
    JudgeRequest {
        state: serde_json::json!("Help! My payouts have been failing for 3 days."),
        model: JEV_LATEST.to_owned(),
        questions: BTreeMap::from([(
            "is_urgent".to_owned(),
            Question::Noul { instructions: "Does this convey urgency?".to_owned(), criteria: None },
        )]),
    }
}

const OK_BODY: &str = r#"{"model":"jev-latest","answers":{"is_urgent":{"type":"noul","noul":0.92}},"usage":{"input_tokens":312,"output_tokens":48}}"#;

#[test]
fn a_success_posts_the_documented_body_with_a_bearer_and_parses_the_answers() {
    let (base_url, captured) = fake_server(200, OK_BODY);
    let manifest = build_manifest(&base_url, "typesafe");
    let adapter = SystemOneAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
    let reply = adapter.call(&sentinel_key(), &request()).unwrap();
    assert_eq!(reply.answers["is_urgent"], Answer::Noul { noul: 0.92 });
    assert_eq!(reply.usage.input_tokens, Some(312));

    let seen = captured.recv().unwrap();
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.path, "/v1/systemone");
    assert_eq!(header_value(&seen.headers, "authorization"), Some(format!("Bearer {SENTINEL}").as_str()));
    assert_eq!(header_value(&seen.headers, "content-type"), Some("application/json"));
    let body: serde_json::Value = serde_json::from_str(&seen.body).unwrap();
    assert_eq!(body, serde_json::to_value(request()).unwrap());
    assert!(!seen.body.contains(SENTINEL), "the key travels in the header only");
}

#[test]
fn every_documented_status_maps_to_its_taxonomy_error() {
    for (status, expected) in [
        (401, GatewayError::AuthRequired),
        (403, GatewayError::PolicyDenied),
        (422, GatewayError::MalformedOutput),
        (429, GatewayError::RateLimited),
        (529, GatewayError::ProviderUnavailable),
        (503, GatewayError::ProviderUnavailable),
    ] {
        let (base_url, _captured) = fake_server(status, r#"{"error":"x"}"#);
        let manifest = build_manifest(&base_url, "typesafe");
        let adapter = SystemOneAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
        assert_eq!(adapter.call(&sentinel_key(), &request()).unwrap_err(), expected, "status {status}");
    }
}

#[test]
fn a_success_whose_body_is_not_a_reply_is_malformed_output() {
    let (base_url, _captured) = fake_server(200, r#"{"model":"jev-latest","answers":{"is_urgent":{"type":"essay"}},"usage":{}}"#);
    let manifest = build_manifest(&base_url, "typesafe");
    let adapter = SystemOneAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
    assert_eq!(adapter.call(&sentinel_key(), &request()).unwrap_err(), GatewayError::MalformedOutput);
}

#[test]
fn a_chat_provider_on_the_judge_door_is_unsupported_and_sends_nothing() {
    let (base_url, captured) = fake_server(200, OK_BODY);
    let manifest = build_manifest(&base_url, "anthropic");
    let adapter = SystemOneAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
    assert_eq!(adapter.call(&sentinel_key(), &request()).unwrap_err(), GatewayError::UnsupportedCapability);
    assert!(captured.recv_timeout(Duration::from_millis(200)).is_err(), "no request must reach the server");
}

#[test]
fn a_typesafe_route_on_the_draft_door_is_unsupported_and_sends_nothing() {
    let (base_url, captured) = fake_server(200, OK_BODY);
    let manifest = build_manifest(&base_url, "typesafe");
    let adapter = ByokAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
    let call = ModelCall { prompt: "draft a graph".to_owned(), max_tokens: 16 };
    assert_eq!(adapter.call(&sentinel_key(), &call).unwrap_err(), GatewayError::UnsupportedCapability);
    assert!(captured.recv_timeout(Duration::from_millis(200)).is_err());
}
```

(`CapturedRequest` in `byok_adapters.rs` has `method`, `path`, `headers`; add a `body: String` field to the copy here and read it after the headers in `read_request` using `Content-Length`.)

Manifest test (in the file that today asserts `direct_api routes must use provider "anthropic" or "openai"`): change that assertion to the new rule text and add:

```rust
#[test]
fn typesafe_is_a_legal_direct_api_provider() {
    let json = serde_json::json!({ "manifestVersion": 1, "routes": [{
        "id": "judge", "provider": "typesafe", "transport": "direct_api",
        "authentication": "api_key", "billingMode": "per_token",
        "baseUrl": "https://api.typesafe.ai", "model": "jev-latest",
        "credentialRef": "secret_typesafe", "profiles": ["balanced_reasoning"], "enabled": true
    }]});
    assert!(RouteManifest::from_json(&json.to_string()).is_ok());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p graphhelm-model-gateway --test systemone_adapter` → compile error (`systemone` missing). `cargo test -p graphhelm-gateway` → the `typesafe` manifest test fails with the structural violation.

- [ ] **Step 3: Implement**

`core/gateway/src/manifest.rs` (~L478):

```rust
    if !matches!(route.provider.as_str(), "anthropic" | "openai" | "typesafe") {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "direct_api routes must use provider \"anthropic\", \"openai\" or \"typesafe\"",
        });
    }
```

Update the comment above it: `typesafe` is served by `adapters/model-gateway/src/systemone.rs` on the JUDGE door only.

`adapters/model-gateway/src/byok.rs` `call` (~L72):

```rust
        match self.route.provider() {
            "anthropic" => self.call_anthropic(key, request),
            "openai" => self.call_openai(key, request),
            // A System One model answers typed questions and cannot draft text; it is served by
            // `systemone.rs` on the judge door. Refused here before any request is built.
            crate::systemone::TYPESAFE_PROVIDER => Err(GatewayError::UnsupportedCapability),
            _ => Err(GatewayError::UnsupportedCapability),
        }
```

`adapters/model-gateway/src/systemone.rs`:

```rust
//! The System One adapter: `POST {baseUrl}/v1/systemone` with a Bearer key, a `JudgeRequest`
//! body and a `JudgeReply` reply (docs.typesafe.ai/api). Mirrors `byok.rs`'s shape over the
//! same `HttpTransport`; serves the JUDGE door only (spec D1): a chat provider here, or this
//! provider on the draft door, is `UnsupportedCapability` before any request is built.

use std::sync::Arc;
use std::time::Duration;

use graphhelm_events::SecretBytes;
use graphhelm_gateway::judgment::{JudgeReply, JudgeRequest};
use graphhelm_gateway::manifest::{ModelRoute, Transport};
use graphhelm_gateway::taxonomy::GatewayError;

use crate::transport::{HttpTransport, TransportRequest};

pub const TYPESAFE_PROVIDER: &str = "typesafe";
pub const SYSTEMONE_PATH: &str = "/v1/systemone";

pub struct SystemOneAdapter<'a> {
    route: &'a ModelRoute,
    transport: Arc<dyn HttpTransport>,
}

impl<'a> SystemOneAdapter<'a> {
    #[must_use]
    pub fn new(route: &'a ModelRoute, transport: Arc<dyn HttpTransport>) -> Self {
        Self { route, transport }
    }

    /// # Errors
    /// `UnsupportedCapability` for a route that is not `direct_api`/`typesafe`; the documented
    /// statuses mapped by [`map_status`]; `MalformedOutput` for a 2xx body that is not a reply;
    /// `ProviderUnavailable`/`Timeout` from the transport.
    pub fn call(&self, key: &SecretBytes, request: &JudgeRequest) -> Result<JudgeReply, GatewayError> {
        if self.route.transport() != Transport::DirectApi || self.route.provider() != TYPESAFE_PROVIDER {
            return Err(GatewayError::UnsupportedCapability);
        }
        let base_url = self
            .route
            .base_url()
            .expect("direct_api routes carry baseUrl — enforced by manifest validation");
        let mut request = request.clone();
        if let Some(model) = self.route.model() {
            request.model = model.to_owned();
        }
        let body = serde_json::to_vec(&request).expect("JudgeRequest is plain data and always serializes");
        let response = self
            .transport
            .execute(&TransportRequest {
                method: "POST",
                url: format!("{base_url}{SYSTEMONE_PATH}"),
                headers: bearer_headers(key),
                body,
                timeout: Duration::from_secs(self.route.timeout_seconds()),
            })
            .map_err(|error| crate::byok::map_transport_error(&error))?;
        if (200..300).contains(&response.status) {
            serde_json::from_slice::<JudgeReply>(&response.body).map_err(|_| GatewayError::MalformedOutput)
        } else {
            Err(map_status(response.status))
        }
    }
}

fn bearer_headers(key: &SecretBytes) -> Vec<(String, String)> {
    key.expose(|bytes| {
        vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("authorization".to_owned(), format!("Bearer {}", String::from_utf8_lossy(bytes))),
        ]
    })
}

/// docs.typesafe.ai/api "Errors": 401, 422, 429, 529; 403 and 5xx by the same rule the
/// Anthropic adapter uses. A 422 is the compiler having built a bad question — a defect to see,
/// never a retry — so it is `MalformedOutput`, the taxonomy's "could not be understood" arm.
fn map_status(status: u16) -> GatewayError {
    match status {
        401 => GatewayError::AuthRequired,
        403 => GatewayError::PolicyDenied,
        429 => GatewayError::RateLimited,
        529 => GatewayError::ProviderUnavailable,
        status if status >= 500 => GatewayError::ProviderUnavailable,
        _ => GatewayError::MalformedOutput,
    }
}
```

Check how `byok.rs` turns a `TransportError` into a `GatewayError` (`grep -n "TransportError" adapters/model-gateway/src/byok.rs`); if the mapping is a private fn, make it `pub(crate) fn map_transport_error(error: &TransportError) -> GatewayError` and reuse it, do not duplicate the table.

`adapters/model-gateway/src/lib.rs`: `pub mod systemone;`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p graphhelm-model-gateway && cargo test -p graphhelm-gateway`
Expected: all green, including the 5 new adapter cells.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p graphhelm-model-gateway -p graphhelm-gateway --all-targets -- -D warnings
git add adapters/model-gateway core/gateway && git commit -m "feat(model-gateway): System One adapter and the typesafe direct_api provider

Closes #<task-issue>

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Site 3 — per-node judgments as repairable diagnostics

**Files:**
- Create: `core/architect/src/judgment/mod.rs`, `core/architect/src/judgment/policy.rs`, `core/architect/src/judgment/nodes.rs`
- Modify: `core/architect/src/synthesize.rs` (`synthesize_with`, `Extras`, report fields, hook in the round loop), `core/architect/src/lib.rs`
- Test: `core/architect/tests/judgment_nodes.rs`, fixtures `core/architect/fixtures/judge/nodes-*.json`

**Interfaces:**
- Consumes: Tasks 1–2; `compile_round`'s `Compiled` and `loaded.graph.spec.nodes` (`GraphNode { objective, node_type, .. }`; check field names with `grep -n "pub struct GraphNode" -A 30 core/protocols/src/*.rs`).
- Produces:
  - `pub struct Extras<'a> { pub judge: Option<&'a dyn JudgeModel>, pub drafts: u8, pub library: Option<&'a GraphLibrary> }` with `Default` (`judge: None, drafts: 1, library: None`). (`library` is added in Task 6; declare it now as `Option<&'a ()>`-free by adding the field in Task 6 — do NOT pre-declare.)
  - `pub fn synthesize_with(profile, catalog, model, extras: &Extras<'_>) -> Result<SynthesizedGraph, ArchitectRefusal>`; `synthesize` becomes `synthesize_with(.., &Extras::default())`.
  - `SynthesizedGraph.judgments: Option<JudgmentReport>` where `JudgmentReport { nodes: Vec<NodeJudgment>, unresolved: Vec<String>, usage: Usage }` and `NodeJudgment { node: String, on_goal: f64, kind: String, kind_confidence: f64 }`.
  - Diagnostic codes `pub const NODE_OFF_GOAL_CODE: &str = "GHA005_NODE_OFF_GOAL"`, `pub const NODE_KIND_MISMATCH_CODE: &str = "GHA006_NODE_KIND_MISMATCH"`.
  - `policy::{ACT_THRESHOLD: f64 = 0.80, NOUL_NO_THRESHOLD: f64 = 0.35, fn noul_is_no(p: f64) -> bool, fn acts(confidence: f64) -> bool}`.
  - `nodes::{fn request(profile, catalog, graph) -> JudgeRequest, fn diagnostics(reply: &JudgeReply, graph) -> (Vec<Diagnostic>, Vec<NodeJudgment>, Vec<String>)}`.

- [ ] **Step 1: Write the failing tests**

`core/architect/tests/judgment_nodes.rs` (reuse `golden.rs`'s `fixtures()`, `profile()`, `catalog_with_cargo()` helpers by copying them; they are file-private):

```rust
use graphhelm_architect::judgment::policy::{ACT_THRESHOLD, NOUL_NO_THRESHOLD, acts, noul_is_no};
use graphhelm_architect::{Extras, RecordedDraftModel, RecordedJudgeModel, synthesize, synthesize_with};

#[test]
fn policy_edges_are_exact() {
    assert!(noul_is_no(NOUL_NO_THRESHOLD - 1e-9));
    assert!(!noul_is_no(NOUL_NO_THRESHOLD + 1e-9));
    assert!(acts(ACT_THRESHOLD + 1e-9));
    assert!(!acts(ACT_THRESHOLD - 1e-9));
}

#[test]
fn no_judge_is_todays_bytes() {
    let model = RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    let with = synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras::default()).unwrap();
    assert_eq!(serde_json::to_vec(&plain).unwrap(), serde_json::to_vec(&with).unwrap());
    assert_eq!(serde_json::to_vec(&plain).unwrap(), expected_document_bytes());
    assert!(plain.judgments.is_none());
}

/// Fixture `judge/nodes-off-goal.json`: the judge says node `summarize` is NOT on goal
/// (`noul` 0.10) with the first draft, then everything on goal with the repaired draft. The draft
/// recording `judge/nodes-off-goal-replies.json` carries both prompts (round 1, and round 2
/// whose repair block quotes GHA005). Recorded with `ARCHITECT_RECORD=1` (Step 4).
#[test]
fn an_off_goal_node_is_a_repairable_gha005_and_round_two_wins() {
    let model = RecordedDraftModel::from_file(&fixtures().join("judge/nodes-off-goal-replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/nodes-off-goal.json")).unwrap();
    let extras = Extras { judge: Some(&judge), ..Extras::default() };
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras).unwrap();
    assert_eq!(out.rounds, 2);
    let judgments = out.judgments.expect("a judge was named");
    assert!(judgments.nodes.iter().all(|node| !noul_is_no(node.on_goal)));
    assert!(judgments.unresolved.is_empty());
}

/// Fixture `judge/nodes-below-threshold.json`: the judge answers `kind` = `tool` for the
/// cognitive node `summarize` with confidence 0.60 (< ACT_THRESHOLD). Nothing changes: one
/// round, the golden document, and the node is listed under `unresolved`.
#[test]
fn a_low_confidence_kind_changes_nothing_and_is_reported_unresolved() {
    let model = RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/nodes-below-threshold.json")).unwrap();
    let extras = Extras { judge: Some(&judge), ..Extras::default() };
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras).unwrap();
    assert_eq!(out.rounds, 1);
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    assert_eq!(out.document, plain.document, "a judgment below threshold edits nothing");
    assert_eq!(out.judgments.unwrap().unresolved, vec!["summarize".to_owned()]);
}

/// The judge's diagnostics are appended to the round's diagnostics, never replace them: a draft
/// that is BOTH schema-invalid and off-goal is refused for the schema (the judge is not even
/// asked, because `compile_round` failed before the judgment hook).
#[test]
fn the_judge_is_asked_only_about_a_draft_that_passed_every_deterministic_check() {
    let judge = RecordedJudgeModel::from_json(br#"{"answers":{}}"#).unwrap();
    let extras = Extras { judge: Some(&judge), ..Extras::default() };
    let model = RecordedDraftModel::from_file(&fixtures().join("sabotage/edge-to-missing-node.json")).unwrap();
    match synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras) {
        Err(graphhelm_architect::ArchitectRefusal::Invalid { .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_judge_that_cannot_answer_names_the_request_and_ends_the_run() {
    let model = RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_json(br#"{"answers":{}}"#).unwrap();
    let extras = Extras { judge: Some(&judge), ..Extras::default() };
    match synthesize_with(&profile(), &catalog_with_cargo(), &model, &extras) {
        Err(graphhelm_architect::ArchitectRefusal::JudgeMissing { request_sha256 }) => assert_eq!(request_sha256.len(), 64),
        other => panic!("{other:?}"),
    }
}
```

(`sabotage/edge-to-missing-node.json` is the fixture behind `an_edge_to_a_missing_node_is_invalid_under_ghg003_after_every_repair`; confirm the filename with `ls core/architect/fixtures/sabotage`.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p graphhelm-architect --test judgment_nodes` → compile errors (`Extras`, `synthesize_with`, `judgments`).

- [ ] **Step 3: Implement**

`core/architect/src/judgment/policy.rs`:

```rust
//! Named thresholds (spec D6). These are VALUES TO BE MEASURED, chosen conservatively; a change
//! needs a recorded Jev run under `docs/acceptance/` and a cell on each side of the new value.

/// A `Choice`/`Score` answer acts on the compiler only at or above this confidence.
pub const ACT_THRESHOLD: f64 = 0.80;
/// A `Noul` at or below this is read as "no"; between the two thresholds it is unresolved.
pub const NOUL_NO_THRESHOLD: f64 = 0.35;
/// A `Noul` at or above this is read as "yes".
pub const NOUL_YES_THRESHOLD: f64 = 0.65;

#[must_use]
pub fn acts(confidence: f64) -> bool {
    confidence >= ACT_THRESHOLD
}

#[must_use]
pub fn noul_is_no(probability: f64) -> bool {
    probability <= NOUL_NO_THRESHOLD
}

#[must_use]
pub fn noul_is_yes(probability: f64) -> bool {
    probability >= NOUL_YES_THRESHOLD
}
```

`core/architect/src/judgment/mod.rs`:

```rust
//! Typed judgments over a draft (spec §1). Every site here may only (a) append a repairable
//! diagnostic, (b) rank under a fixed policy, or (c) report; nothing here edits a draft.

pub mod nodes;
pub mod policy;

use graphhelm_gateway::call::Usage;

use crate::judge::JudgeModel;

/// What a caller may add to `synthesize` beyond the three inputs of the first compile.
#[derive(Clone, Copy)]
pub struct Extras<'a> {
    pub judge: Option<&'a dyn JudgeModel>,
    /// How many drafts to ask for and rank (Task 5); `1` is today's road. Bounded to `1..=3`.
    pub drafts: u8,
}

impl Default for Extras<'_> {
    fn default() -> Self {
        Self { judge: None, drafts: 1 }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeJudgment {
    pub node: String,
    /// The probability the node serves the goal.
    pub on_goal: f64,
    /// The node type the judge would give this objective, from the catalog.
    pub kind: String,
    pub kind_confidence: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgmentReport {
    /// One entry per node of the accepted draft, in node-id order.
    pub nodes: Vec<NodeJudgment>,
    /// Nodes whose answers fell between the thresholds: nothing was done, and that is visible.
    pub unresolved: Vec<String>,
    /// Summed over every judge call of the run.
    pub usage: Usage,
}
```

`core/architect/src/judgment/nodes.rs`:

```rust
//! Site 3: per-node judgments. State = the goal and every node's `{id, type, objective}`;
//! questions = per node, `on_goal:<id>` (Noul) and `kind:<id>` (Choice over the catalog's node
//! types). A "no" on `on_goal` is `GHA005_NODE_OFF_GOAL`; a confident `kind` that differs from
//! the draft's type is `GHA006_NODE_KIND_MISMATCH`. Both are REPAIRABLE: they go back to the
//! draft model with the round's other diagnostics. Nothing else is read.

use std::collections::BTreeMap;

use graphhelm_gateway::judgment::{Answer, JEV_LATEST, JudgeReply, JudgeRequest, NoulCriteria, Question};
use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use super::policy::{acts, noul_is_no, noul_is_yes};
use super::NodeJudgment;
use crate::catalog::CapabilityCatalog;
use crate::profile::TaskProfile;
use crate::synthesize::DRAFT_SOURCE;

pub const NODE_OFF_GOAL_CODE: &str = "GHA005_NODE_OFF_GOAL";
pub const NODE_KIND_MISMATCH_CODE: &str = "GHA006_NODE_KIND_MISMATCH";

#[must_use]
pub fn request(profile: &TaskProfile, catalog: &CapabilityCatalog, graph: &ExecutionGraph) -> JudgeRequest {
    let nodes: Vec<serde_json::Value> = graph
        .spec
        .nodes
        .iter()
        .map(|(id, node)| {
            serde_json::json!({ "id": id, "type": node.node_type.as_str(), "objective": node.objective })
        })
        .collect();
    let kinds: BTreeMap<String, Option<String>> = catalog
        .node_types
        .iter()
        .map(|kind| (kind.clone(), None))
        .collect();
    let mut questions = BTreeMap::new();
    for id in graph.spec.nodes.keys() {
        questions.insert(
            format!("on_goal:{id}"),
            Question::Noul {
                instructions: format!(
                    "Does the node with id `{id}` (see `nodes`) do work that the `goal` needs? \
                     Judge the node's `objective` against the `goal`."
                ),
                criteria: Some(NoulCriteria {
                    r#true: "The goal cannot be met without this node's objective".to_owned(),
                    r#false: "The objective is unrelated to the goal, or duplicates another node".to_owned(),
                }),
            },
        );
        questions.insert(
            format!("kind:{id}"),
            Question::Choice {
                instructions: format!(
                    "Which node type from the options best executes the `objective` of the node \
                     with id `{id}`? `agent` reasons and writes; tool types run one program, \
                     repository read, or test call; pick by what the objective actually does."
                ),
                criteria: kinds.clone(),
            },
        );
    }
    JudgeRequest {
        state: serde_json::json!({ "goal": profile.goal, "nodes": nodes }),
        model: JEV_LATEST.to_owned(),
        questions,
    }
}

/// Reads the reply into diagnostics (repairable), one judgment per node, and the unresolved
/// ids. A missing or mistyped answer is treated as unresolved, never as a verdict.
#[must_use]
pub fn read(reply: &JudgeReply, graph: &ExecutionGraph) -> (Vec<Diagnostic>, Vec<NodeJudgment>, Vec<String>) {
    let mut diagnostics = Vec::new();
    let mut judgments = Vec::new();
    let mut unresolved = Vec::new();
    for (id, node) in &graph.spec.nodes {
        let on_goal = match reply.answers.get(&format!("on_goal:{id}")) {
            Some(Answer::Noul { noul }) => *noul,
            _ => f64::NAN,
        };
        let (kind, kind_confidence) = match reply.answers.get(&format!("kind:{id}")) {
            Some(Answer::Choice { choice, confidence, .. }) => (choice.clone(), *confidence),
            _ => (String::new(), f64::NAN),
        };
        let mut resolved = true;
        if on_goal.is_nan() || (!noul_is_no(on_goal) && !noul_is_yes(on_goal)) {
            resolved = false;
        } else if noul_is_no(on_goal) {
            diagnostics.push(Diagnostic::error(
                NODE_OFF_GOAL_CODE,
                format!("node {id} does not serve the goal (judged {on_goal:.2}); remove it or give it an objective the goal needs"),
                format!("/spec/nodes/{}/objective", crate::synthesize::escape(id)),
                DRAFT_SOURCE,
            ));
        }
        if kind_confidence.is_nan() || !acts(kind_confidence) {
            resolved = false;
        } else if kind != node.node_type.as_str() {
            diagnostics.push(Diagnostic::error(
                NODE_KIND_MISMATCH_CODE,
                format!("node {id} is typed {} but its objective reads as {kind} (confidence {kind_confidence:.2})", node.node_type.as_str()),
                format!("/spec/nodes/{}/type", crate::synthesize::escape(id)),
                DRAFT_SOURCE,
            ));
        }
        if !resolved {
            unresolved.push(id.clone());
        }
        judgments.push(NodeJudgment { node: id.clone(), on_goal, kind, kind_confidence });
    }
    (diagnostics, judgments, unresolved)
}
```

Make `escape` in `synthesize.rs` `pub(crate)`. If `GraphNode`'s type field is not `node_type`, use the real name.

`core/architect/src/synthesize.rs` changes:

1. `SynthesizedGraph` gains, after `usage`:
```rust
    /// Present only when a judge was named (spec D4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judgments: Option<JudgmentReport>,
```
2. `Compiled` gains `graph: ExecutionGraph` (the `loaded.graph` moved out) so the hook can ask about it.
3. `synthesize` becomes:
```rust
pub fn synthesize(profile: &TaskProfile, catalog: &CapabilityCatalog, model: &dyn DraftModel) -> Result<SynthesizedGraph, ArchitectRefusal> {
    synthesize_with(profile, catalog, model, &Extras::default())
}

pub fn synthesize_with(profile: &TaskProfile, catalog: &CapabilityCatalog, model: &dyn DraftModel, extras: &Extras<'_>) -> Result<SynthesizedGraph, ArchitectRefusal> {
    profile.validate()?;
    if !(1..=3).contains(&extras.drafts) {
        return Err(ArchitectRefusal::InvalidProfile { pointer: "/drafts".to_owned(), message: "drafts must be 1, 2 or 3".to_owned() });
    }
    // Task 5 branches here on `extras.drafts > 1`; Task 4 runs the single-draft loop below.
    single_draft(profile, catalog, model, extras.judge, None)
}
```
4. The loop body moves into `fn single_draft(profile, catalog, model, judge: Option<&dyn JudgeModel>, stance: Option<&Stance>)` (stance arrives in Task 5; pass `None` now and `assemble_prompt(profile, catalog, repair.as_ref())` unchanged). After `Ok(compiled)` from `compile_round` and before returning, the hook:
```rust
            Ok(compiled) => {
                let mut judgments = None;
                if let Some(judge) = judge {
                    let request = judgment::nodes::request(profile, catalog, &compiled.graph);
                    let reply = judge.judge(&request)?;
                    judge_usage = add_usage(judge_usage, reply.usage);
                    let (diagnostics, nodes, unresolved) = judgment::nodes::read(&reply, &compiled.graph);
                    if !diagnostics.is_empty() {
                        if round > MAX_REPAIR_ROUNDS {
                            return Err(ArchitectRefusal::Invalid { rounds: round, diagnostics });
                        }
                        previous = Some((reply_text, diagnostics));
                        round += 1;
                        continue;
                    }
                    judgments = Some(JudgmentReport { nodes, unresolved, usage: judge_usage });
                }
                return Ok(SynthesizedGraph { document: compiled.document, rationale: compiled.rationale, stamped_customs: compiled.stamped, template_sha256, rounds: round, prompt_sha256s, usage: reply.usage, judgments });
            }
```
where `add_usage` sums the two `Option<u64>` pairs (`Some(a) + Some(b)`, else whichever is `Some`).

`lib.rs`: `pub mod judgment;` and `pub use judgment::{Extras, JudgmentReport, NodeJudgment};` plus `pub use synthesize::synthesize_with;`.

- [ ] **Step 4: Record the two judge fixtures**

Extend `golden.rs`'s `record()` idea for the judge: in `tests/judgment_nodes.rs` add a `#[test] #[ignore]` recorder gated on `ARCHITECT_RECORD=1` that runs the cell, catches `JudgeMissing { request_sha256 }`, prints the digest and the request JSON (`judgment::nodes::request(..)` serialized) so the author writes the reply by hand into `fixtures/judge/nodes-off-goal.json`:

```json
{ "answers": { "<sha256 of round-1 request>": { "model": "jev-latest", "answers": { "on_goal:build_check": {"type":"noul","noul":0.95}, "kind:build_check": {"type":"choice","choice":"<its type>","probabilities":{},"confidence":0.9}, "on_goal:summarize": {"type":"noul","noul":0.10}, "kind:summarize": {"type":"choice","choice":"agent","probabilities":{},"confidence":0.9} }, "usage": {"input_tokens":1,"output_tokens":1} },
               "<sha256 of round-2 request>": { "...": "all on goal, kinds matching" } } }
```

The round-2 draft reply goes into `fixtures/judge/nodes-off-goal-replies.json` under the round-2 prompt hash (the repair block quotes `GHA005_NODE_OFF_GOAL`); copy the round-1 reply from `first-compile/replies.json`. Authored fixtures are hand-written replies, exactly as `sabotage/*.json` are; document them in `fixtures/README.md`.

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p graphhelm-architect`
Expected: every existing cell green (the golden untouched), the 6 new cells green.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy -p graphhelm-architect --all-targets -- -D warnings
git add core/architect && git commit -m "feat(architect): per-node typed judgments as repairable diagnostics (site 3)

Closes #<task-issue>

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Site 2 — rank N drafts

**Files:**
- Create: `core/architect/src/judgment/ranking.rs`
- Modify: `core/architect/src/template.rs` (`assemble_prompt` gains `stance`), `core/architect/src/synthesize.rs` (`synthesize_with` branches on `drafts > 1`), `core/architect/src/judgment/mod.rs` (`RankingReport`), `core/architect/src/lib.rs`
- Test: `core/architect/tests/judgment_ranking.rs`, `core/architect/tests/template.rs` (one new cell), fixtures `core/architect/fixtures/judge/ranking-*.json`

**Interfaces:**
- Consumes: Task 4's `single_draft`, `Extras`, `JudgmentReport`.
- Produces: `pub enum Stance { Minimal, Verified, Explicit }` with `Stance::ALL: [Stance; 3]` and `fn text(&self) -> &'static str`; `assemble_prompt(profile, catalog, previous, stance: Option<&Stance>)`; `RankingReport { candidates: Vec<Candidate>, chosen: u8, unresolved: bool }`, `Candidate { index: u8, stance: String, coverage: f64, waste: f64, confidence: f64, composite: f64 }`; `SynthesizedGraph.ranking: Option<RankingReport>`; `ranking::{request(profile, candidates: &[ExecutionGraph]) -> JudgeRequest, read(reply, n) -> RankingReport}`.

- [ ] **Step 1: Write the failing tests**

`core/architect/tests/template.rs`, add:

```rust
#[test]
fn a_prompt_without_a_stance_is_byte_identical_to_the_first_compile() {
    let before = assemble_prompt(&profile(), &catalog(), None, None);
    assert_eq!(prompt_sha256(&before), FIRST_COMPILE_PROMPT_SHA256);
    let with = assemble_prompt(&profile(), &catalog(), None, Some(&Stance::Minimal));
    assert!(with.starts_with(&before[..before.len() - "</goal>".len()]) || with.contains("<stance>"));
    assert!(with.contains("</goal>\n<stance>"), "the stance follows the goal fence");
    assert_ne!(prompt_sha256(&before), prompt_sha256(&with));
}
```

(`FIRST_COMPILE_PROMPT_SHA256` = the round-1 key already in `fixtures/first-compile/replies.json`; read it from the file in the test rather than hard-coding.)

`core/architect/tests/judgment_ranking.rs`:

```rust
#[test]
fn drafts_outside_one_to_three_are_refused_before_any_prompt() {
    let model = RecordedDraftModel::from_json(br#"{"replies":{}}"#).unwrap();
    for drafts in [0u8, 4] {
        match synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { drafts, ..Extras::default() }) {
            Err(ArchitectRefusal::InvalidProfile { pointer, .. }) => assert_eq!(pointer, "/drafts"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn more_than_one_draft_needs_a_judge() {
    let model = RecordedDraftModel::from_json(br#"{"replies":{}}"#).unwrap();
    match synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { drafts: 2, ..Extras::default() }) {
        Err(ArchitectRefusal::InvalidProfile { pointer, message }) => {
            assert_eq!(pointer, "/drafts");
            assert!(message.contains("judge"));
        }
        other => panic!("{other:?}"),
    }
}

/// `judge/ranking-three-replies.json` holds one valid draft per stance (three prompts);
/// `judge/ranking-three.json` holds the per-node judgments of each (all on goal) and the ranking
/// reply: candidate 2 covers best (`score` 2.0 of levels 0..=2, confidence 0.9), no waste.
#[test]
fn the_best_covered_candidate_is_chosen_and_the_report_says_why() {
    let model = RecordedDraftModel::from_file(&fixtures().join("judge/ranking-three-replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/ranking-three.json")).unwrap();
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { judge: Some(&judge), drafts: 3 }).unwrap();
    let ranking = out.ranking.expect("three drafts were ranked");
    assert_eq!(ranking.chosen, 2);
    assert!(!ranking.unresolved);
    assert_eq!(ranking.candidates.len(), 3);
    assert_eq!(out.prompt_sha256s.len(), 3, "one prompt per stance, no repair needed");
    assert_eq!(out.document["metadata"]["labels"]["stance"], "explicit");
}

/// `judge/ranking-unresolved.json`: same drafts; the top candidate's confidence is 0.50.
/// Draft 1 (today's road) is kept and the report says `unresolved`.
#[test]
fn a_low_confidence_ranking_keeps_the_first_draft() {
    let model = RecordedDraftModel::from_file(&fixtures().join("judge/ranking-three-replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/ranking-unresolved.json")).unwrap();
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { judge: Some(&judge), drafts: 3 }).unwrap();
    let ranking = out.ranking.unwrap();
    assert_eq!(ranking.chosen, 0);
    assert!(ranking.unresolved);
}

#[test]
fn a_tie_on_composite_goes_to_the_lower_index() {
    // pure-function cell over `ranking::read`: two candidates, identical scores, confidence 0.95
    let reply = JudgeReply { model: JEV_LATEST.to_owned(), usage: Usage::default(), answers: BTreeMap::from([
        ("coverage:0".to_owned(), Answer::Score { score: 2.0, legend: BTreeMap::new(), probabilities: BTreeMap::new(), confidence: 0.95 }),
        ("waste:0".to_owned(), Answer::Noul { noul: 0.1 }),
        ("coverage:1".to_owned(), Answer::Score { score: 2.0, legend: BTreeMap::new(), probabilities: BTreeMap::new(), confidence: 0.95 }),
        ("waste:1".to_owned(), Answer::Noul { noul: 0.1 }),
    ])};
    let report = graphhelm_architect::judgment::ranking::read(&reply, 2);
    assert_eq!(report.chosen, 0);
    assert!(!report.unresolved);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p graphhelm-architect --test judgment_ranking --test template` → compile errors (`Stance`, `ranking`, fourth argument).

- [ ] **Step 3: Implement**

`template.rs`: add

```rust
/// A drafting stance for one of N ranked drafts (spec D7). Three fixed texts are the whole
/// vocabulary; `None` yields the first compile's exact prompt bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance { Minimal, Verified, Explicit }

impl Stance {
    pub const ALL: [Stance; 3] = [Stance::Minimal, Stance::Verified, Stance::Explicit];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self { Self::Minimal => "minimal", Self::Verified => "verified", Self::Explicit => "explicit" }
    }

    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Minimal => "Prefer the fewest nodes that can meet the goal; merge steps that one node can do.",
            Self::Verified => "Prefer a draft where every claim the goal makes is checked by a tool node before the run ends.",
            Self::Explicit => "Prefer one node per distinct step, each with a narrow objective, even if that means more nodes.",
        }
    }
}
```

and change `assemble_prompt(profile, catalog, previous, stance: Option<&Stance>)`: after substituting `{{GOAL}}` (inside the `<goal>…</goal>` fence), append `\n<stance>\n{text}\n</stance>` when `Some`, treating the text as data like the goal (it is a constant; still fenced so the fence grammar is uniform). Update every existing caller (`synthesize.rs`, `tests/template.rs`, `tests/golden.rs`, `apps/cli` if any) to pass `None`.

`judgment/ranking.rs`:

```rust
//! Site 2: rank N valid drafts. State = the goal and, per candidate, its nodes
//! `{id, type, objective}`. Questions per candidate: `coverage:<i>` (Score over three levels)
//! and `waste:<i>` (Noul: does it do work the goal did not ask for). Composite =
//! `coverage - waste`; highest wins; ties go to the lower index; a top confidence below
//! `ACT_THRESHOLD` keeps candidate 0 and reports `unresolved` (spec D7).

pub const COVERAGE_LEVELS: [&str; 3] = [
    "Misses at least one thing the goal explicitly asks for",
    "Covers the goal but leaves a stated outcome unchecked",
    "Covers every stated outcome and checks it",
];

#[must_use]
pub fn request(profile: &TaskProfile, candidates: &[ExecutionGraph]) -> JudgeRequest { /* state {goal, candidates:[{index, stance?, nodes:[..]}]}, questions per index as above */ }

#[must_use]
pub fn read(reply: &JudgeReply, count: u8) -> RankingReport {
    let mut candidates = Vec::new();
    for index in 0..count {
        let (coverage, confidence) = match reply.answers.get(&format!("coverage:{index}")) {
            Some(Answer::Score { score, confidence, .. }) => (*score, *confidence),
            _ => (f64::NAN, 0.0),
        };
        let waste = match reply.answers.get(&format!("waste:{index}")) {
            Some(Answer::Noul { noul }) => *noul,
            _ => f64::NAN,
        };
        let composite = if coverage.is_nan() || waste.is_nan() { f64::NEG_INFINITY } else { coverage - waste };
        candidates.push(Candidate { index, stance: String::new(), coverage, waste, confidence, composite });
    }
    let best = candidates.iter().fold(None::<&Candidate>, |best, c| match best {
        Some(b) if b.composite >= c.composite => Some(b),
        _ => Some(c),
    });
    let (chosen, unresolved) = match best {
        Some(c) if acts(c.confidence) && c.composite.is_finite() => (c.index, false),
        _ => (0, true),
    };
    RankingReport { candidates, chosen, unresolved }
}
```

`synthesize.rs`, in `synthesize_with`, when `extras.drafts > 1`:

```rust
    let Some(judge) = extras.judge else {
        return Err(ArchitectRefusal::InvalidProfile { pointer: "/drafts".to_owned(), message: "more than one draft needs a judge to rank them".to_owned() });
    };
    let mut compiled = Vec::new();
    let mut prompt_sha256s = Vec::new();
    let mut usage = None;
    for stance in Stance::ALL.iter().take(usize::from(extras.drafts)) {
        let one = single_draft(profile, catalog, model, Some(judge), Some(stance))?; // each draft is itself judged per node (site 3) and repaired
        prompt_sha256s.extend(one.prompt_sha256s.iter().cloned());
        usage = add_usage(usage, one.usage);
        compiled.push((stance, one));
    }
    let graphs: Vec<ExecutionGraph> = compiled.iter().map(|(_, one)| one.graph.clone()).collect(); // keep `graph` on the single-draft result (private field) for this
    let reply = judge.judge(&judgment::ranking::request(profile, &graphs))?;
    let mut ranking = judgment::ranking::read(&reply, extras.drafts);
    for (candidate, (stance, _)) in ranking.candidates.iter_mut().zip(&compiled) { candidate.stance = stance.label().to_owned(); }
    let (stance, mut chosen) = compiled.swap_remove(usize::from(ranking.chosen));
    chosen.document["metadata"]["labels"]["stance"] = Value::String(stance.label().to_owned());
    chosen.prompt_sha256s = prompt_sha256s;
    chosen.usage = usage;
    chosen.ranking = Some(ranking);
    Ok(chosen)
```

`metadata.labels.stance` is written by the compiler AFTER validation; it is a label, and `graph.schema.json` admits free labels (confirm with `grep -n '"labels"' -A 6 schemas/graph.schema.json`; if labels are a closed map, drop the label and keep the stance in the report only).

- [ ] **Step 4: Record fixtures**

Three stance prompts → three draft replies in `judge/ranking-three-replies.json` (author two small variants of the first-compile draft: the minimal one drops `summarize`, the explicit one splits it into two nodes; all three must pass lint). Judge fixture: per-node judgments for each (all on goal, kinds matching) + the ranking reply. Same `ARCHITECT_RECORD=1` digest-printing recorder as Task 4.

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p graphhelm-architect` → all green, golden untouched.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy -p graphhelm-architect --all-targets -- -D warnings
git add core/architect && git commit -m "feat(architect): rank up to three stance drafts with a typed judge (site 2)

Closes #<task-issue>

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Sites 4 and 1 — the graph library, the road decision, typed parameters

**Files:**
- Create: `core/architect/src/library.rs`, `core/architect/src/judgment/reuse.rs`, fixtures `core/architect/fixtures/library/{build-and-summarize.yaml,build-and-summarize.template.json,run-tests.yaml,run-tests.template.json}`, `core/architect/fixtures/judge/reuse-*.json`
- Modify: `core/architect/src/judgment/mod.rs` (`Extras.library`, `ReuseReport`), `core/architect/src/synthesize.rs` (decision step before drafting; `adapt` seeds the prompt), `core/architect/src/template.rs` (`<seed>` block), `core/architect/src/refusal.rs` (`LibraryInvalid`), `core/architect/src/lib.rs`
- Test: `core/architect/tests/library.rs`

**Interfaces:**
- Consumes: Tasks 4–5.
- Produces:
  - `GraphLibrary::load(dir: &Path) -> Result<GraphLibrary, ArchitectRefusal>` (`LibraryInvalid { path, message }` on any bad file; a directory with no templates is an empty library), `GraphLibrary::templates(&self) -> &[Template]`, `Template { id: String, summary: String, parameters: BTreeMap<String, Parameter>, document: Value }`, `Parameter { question: String, options: BTreeMap<String, String> }`, `Template::fill(&self, values: &BTreeMap<String, String>) -> Result<Value, ArchitectRefusal>` (substitutes `{{name}}` in every string leaf; refuses a value outside `options` or a leftover placeholder).
  - `ArchitectRefusal::LibraryInvalid { path: String, message: String }`.
  - `pub enum Road { Reuse, Adapt, Create }`; `ReuseReport { road: String, template: Option<String>, parameters: BTreeMap<String, String>, confidence: f64, unresolved: bool }`; `SynthesizedGraph.reuse: Option<ReuseReport>`.
  - `reuse::{decide_request(profile, library) -> JudgeRequest, read_decision(reply, library) -> (Road, Option<&Template>, f64)`, `fill_request(profile, template) -> JudgeRequest`, `read_fill(reply, template) -> (BTreeMap<String,String>, Vec<String> unresolved)}`.
  - `assemble_prompt(profile, catalog, previous, stance, seed: Option<&Value>)` — the fifth argument appends `<seed>\n{json}\n</seed>` after the goal (and after the stance when both).
  - `Extras.library: Option<&'a GraphLibrary>`.

- [ ] **Step 1: Author the fixture library**

`fixtures/library/build-and-summarize.yaml`: the first-compile golden document converted to the authored YAML form, with `metadata.labels.origin: library` and the shell program written as `{{program}}` in the build node's call, and the summary node's objective containing `{{audience}}`.

`fixtures/library/build-and-summarize.template.json`:

```json
{
  "id": "build-and-summarize",
  "summary": "Build the repository with one program and summarize the result for one audience.",
  "parameters": {
    "program": {
      "question": "Which build program does the goal name or imply?",
      "options": { "cargo": "a Rust workspace", "npm": "a Node package", "make": "a Makefile project" }
    },
    "audience": {
      "question": "Who reads the summary?",
      "options": { "maintainer": "the person who fixes the build", "operator": "the person who runs the pipeline" }
    }
  }
}
```

`run-tests.yaml` / `.template.json`: a one-node `tests` call graph with parameter `program` only.

- [ ] **Step 2: Write the failing tests**

`core/architect/tests/library.rs`:

```rust
#[test]
fn a_library_loads_every_template_with_its_sidecar_and_refuses_a_bad_one() {
    let library = GraphLibrary::load(&fixtures().join("library")).unwrap();
    let ids: Vec<&str> = library.templates().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["build-and-summarize", "run-tests"]);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.yaml"), "not: [valid").unwrap();
    std::fs::write(dir.path().join("x.template.json"), r#"{"id":"x","summary":"","parameters":{}}"#).unwrap();
    assert!(matches!(GraphLibrary::load(dir.path()), Err(ArchitectRefusal::LibraryInvalid { .. })));
    let empty = tempfile::tempdir().unwrap();
    assert!(GraphLibrary::load(empty.path()).unwrap().templates().is_empty());
}

#[test]
fn fill_substitutes_only_declared_closed_values_and_refuses_the_rest() {
    let library = GraphLibrary::load(&fixtures().join("library")).unwrap();
    let template = &library.templates()[0];
    let values = BTreeMap::from([("program".to_owned(), "cargo".to_owned()), ("audience".to_owned(), "maintainer".to_owned())]);
    let filled = template.fill(&values).unwrap();
    assert!(!serde_json::to_string(&filled).unwrap().contains("{{"));
    let bad = BTreeMap::from([("program".to_owned(), "rm".to_owned()), ("audience".to_owned(), "maintainer".to_owned())]);
    assert!(matches!(template.fill(&bad), Err(ArchitectRefusal::LibraryInvalid { .. })));
    let missing = BTreeMap::from([("program".to_owned(), "cargo".to_owned())]);
    assert!(matches!(template.fill(&missing), Err(ArchitectRefusal::LibraryInvalid { .. })));
}

/// `judge/reuse-reuse.json`: road `reuse` (0.92), template `build-and-summarize` (0.90),
/// program `cargo` (0.95), audience `maintainer` (0.88). No draft model is asked: the recorded
/// draft model is EMPTY and the run still succeeds.
#[test]
fn reuse_fills_a_template_and_never_asks_the_draft_model() {
    let model = RecordedDraftModel::from_json(br#"{"replies":{}}"#).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/reuse-reuse.json")).unwrap();
    let library = GraphLibrary::load(&fixtures().join("library")).unwrap();
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { judge: Some(&judge), library: Some(&library), ..Extras::default() }).unwrap();
    let reuse = out.reuse.unwrap();
    assert_eq!(reuse.road, "reuse");
    assert_eq!(reuse.template.as_deref(), Some("build-and-summarize"));
    assert_eq!(reuse.parameters["program"], "cargo");
    assert!(out.prompt_sha256s.is_empty());
    assert_eq!(out.rounds, 0);
    assert_eq!(out.document["metadata"]["labels"]["origin"], "architect");
    assert!(out.stamped_customs.len() >= 1, "a filled template is stamped like a draft");
}

/// A filled template that names a program outside the allowlist is `CapabilityMissing`, exactly
/// as a draft would be: the library does not widen the allowlist.
#[test]
fn a_filled_template_outside_the_allowlist_is_capability_missing() {
    let model = RecordedDraftModel::from_json(br#"{"replies":{}}"#).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/reuse-npm.json")).unwrap();
    let library = GraphLibrary::load(&fixtures().join("library")).unwrap();
    match synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { judge: Some(&judge), library: Some(&library), ..Extras::default() }) {
        Err(ArchitectRefusal::CapabilityMissing { program, .. }) => assert_eq!(program, "npm"),
        other => panic!("{other:?}"),
    }
}

/// `judge/reuse-adapt.json`: road `adapt` (0.90), template `build-and-summarize`. The draft
/// prompt carries a `<seed>` block; `judge/reuse-adapt-replies.json` holds the reply to THAT prompt.
#[test]
fn adapt_seeds_the_draft_prompt_with_the_template() {
    let model = RecordedDraftModel::from_file(&fixtures().join("judge/reuse-adapt-replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/reuse-adapt.json")).unwrap();
    let library = GraphLibrary::load(&fixtures().join("library")).unwrap();
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { judge: Some(&judge), library: Some(&library), ..Extras::default() }).unwrap();
    assert_eq!(out.reuse.unwrap().road, "adapt");
    assert_eq!(out.rounds, 1);
}

/// `judge/reuse-create.json`: road `create` (0.85). Today's road, today's bytes (plus the report).
#[test]
fn create_is_todays_road() {
    let model = RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/reuse-create.json")).unwrap();
    let library = GraphLibrary::load(&fixtures().join("library")).unwrap();
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { judge: Some(&judge), library: Some(&library), ..Extras::default() }).unwrap();
    let plain = synthesize(&profile(), &catalog_with_cargo(), &model).unwrap();
    assert_eq!(out.document, plain.document);
    assert_eq!(out.reuse.unwrap().road, "create");
}

/// `judge/reuse-unsure.json`: road confidence 0.55. Below threshold → `create`, `unresolved`.
#[test]
fn an_unsure_road_falls_to_create_and_says_so() {
    let model = RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let judge = RecordedJudgeModel::from_file(&fixtures().join("judge/reuse-unsure.json")).unwrap();
    let library = GraphLibrary::load(&fixtures().join("library")).unwrap();
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { judge: Some(&judge), library: Some(&library), ..Extras::default() }).unwrap();
    let reuse = out.reuse.unwrap();
    assert_eq!(reuse.road, "create");
    assert!(reuse.unresolved);
}

#[test]
fn a_library_without_a_judge_is_ignored_and_says_nothing() {
    let model = RecordedDraftModel::from_file(&fixtures().join("first-compile/replies.json")).unwrap();
    let library = GraphLibrary::load(&fixtures().join("library")).unwrap();
    let out = synthesize_with(&profile(), &catalog_with_cargo(), &model, &Extras { library: Some(&library), ..Extras::default() }).unwrap();
    assert!(out.reuse.is_none());
    assert_eq!(serde_json::to_vec(&out).unwrap(), expected_document_bytes());
}
```

(`reuse` in the `create`/`unsure` cells: the per-node judgments of Task 4 also run because a judge is present; their answers must be in the same fixture file.)

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p graphhelm-architect --test library` → compile errors.

- [ ] **Step 4: Implement**

`library.rs`:

```rust
//! A caller-supplied directory of authored graph documents with closed-set parameters
//! (spec D8). `<name>.yaml|json` is the document (the authored format `load_graph_*` reads);
//! `<name>.template.json` is the sidecar. Only `{{name}}` in string leaves is substituted; a
//! value outside `options`, a missing parameter, or a leftover placeholder refuses.

pub struct GraphLibrary { templates: Vec<Template> }

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter { pub question: String, pub options: BTreeMap<String, String> }

#[derive(Clone, Debug)]
pub struct Template { pub id: String, pub summary: String, pub parameters: BTreeMap<String, Parameter>, pub document: Value }

impl GraphLibrary {
    pub fn load(dir: &Path) -> Result<Self, ArchitectRefusal> {
        // read_dir; for every `*.template.json` (sorted by name) require a sibling `<name>.yaml` or `<name>.json`;
        // parse the sidecar with deny_unknown_fields; parse the document as YAML→JSON (serde_yaml_ng, already a workspace dep) or JSON;
        // refuse (LibraryInvalid { path, message }) on any error; > MAX_FIXTURE_BYTES per file refuses; > 64 templates refuses.
    }
    pub fn templates(&self) -> &[Template] { &self.templates }
    pub fn template(&self, id: &str) -> Option<&Template> { self.templates.iter().find(|t| t.id == id) }
}

impl Template {
    pub fn fill(&self, values: &BTreeMap<String, String>) -> Result<Value, ArchitectRefusal> {
        for (name, parameter) in &self.parameters {
            let value = values.get(name).ok_or_else(|| invalid(&self.id, format!("parameter {name} has no value")))?;
            if !parameter.options.contains_key(value) { return Err(invalid(&self.id, format!("parameter {name} value {value} is not one of its options"))); }
        }
        let mut document = self.document.clone();
        substitute(&mut document, values);
        if serde_json::to_string(&document).map_or(true, |text| text.contains("{{")) {
            return Err(invalid(&self.id, "a placeholder was not declared as a parameter".to_owned()));
        }
        Ok(document)
    }
}

fn substitute(value: &mut Value, values: &BTreeMap<String, String>) {
    match value {
        Value::String(text) => { for (name, v) in values { *text = text.replace(&format!("{{{{{name}}}}}"), v); } }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute(item, values)),
        Value::Object(map) => map.values_mut().for_each(|item| substitute(item, values)),
        _ => {}
    }
}
```

`judgment/reuse.rs`: `decide_request` — state `{goal, templates:[{id, summary, parameters:[names]}]}`; questions `road` (Choice: `reuse` "a template fits the goal as is, once its parameters are filled", `adapt` "a template is the right starting point but the goal needs a node it lacks or one fewer", `create` "no template is close; draft from nothing") and `template` (Choice over template ids + `none`). `read_decision`: `acts(road.confidence)` else `(Create, None, conf)` with `unresolved`; `Reuse|Adapt` need `template != none` and `acts(template.confidence)`, else fall to `Create` unresolved. `fill_request` — state `{goal}`, one `Choice` per parameter (`instructions` = the sidecar's `question`, `criteria` = its options). `read_fill`: a parameter below `ACT_THRESHOLD` → unresolved → the whole road falls to `Create` (`unresolved: true`); `reuse` never guesses a value.

`synthesize.rs`, at the top of `synthesize_with` after validation, when `extras.judge.is_some() && extras.library.map_or(false, |l| !l.templates().is_empty())`:

```rust
    let reply = judge.judge(&judgment::reuse::decide_request(profile, library))?;
    let (road, template, confidence, unresolved) = judgment::reuse::read_decision(&reply, library);
    match (road, template) {
        (Road::Reuse, Some(template)) => {
            let fill = judge.judge(&judgment::reuse::fill_request(profile, template))?;
            let (values, unresolved_parameters) = judgment::reuse::read_fill(&fill, template);
            if unresolved_parameters.is_empty() {
                let document = template.fill(&values)?;
                // The filled document takes the SAME chain as a draft: strip apiVersion/kind/metadata, hand the
                // `spec` to `compile_round` as text, so count, schema, lint, viability, stamp, allowlist all run.
                let text = serde_json::to_string(&document["spec"]).expect("plain data");
                return match compile_round(profile, catalog, &text) {
                    Ok(compiled) => Ok(SynthesizedGraph { rounds: 0, prompt_sha256s: vec![], usage: None,
                        reuse: Some(ReuseReport { road: "reuse".into(), template: Some(template.id.clone()), parameters: values, confidence, unresolved: false }),
                        ..from_compiled(compiled) }),
                    Err(RoundFailure::Refused(refusal)) => Err(refusal),
                    Err(RoundFailure::Invalid(diagnostics)) => Err(ArchitectRefusal::Invalid { rounds: 0, diagnostics }),
                    Err(RoundFailure::NotJson(message)) => Err(ArchitectRefusal::NotJson { round: 0, message }),
                };
            }
            // fall through to Create, unresolved
        }
        (Road::Adapt, Some(template)) => { seed = Some(&template.document); reuse = Some(ReuseReport { road: "adapt".into(), template: Some(template.id.clone()), .. }); }
        _ => { reuse = Some(ReuseReport { road: "create".into(), template: None, parameters: BTreeMap::new(), confidence, unresolved }); }
    }
```

then the existing single/ranked road runs with `seed` threaded into `assemble_prompt`. `refusal.rs`: `#[error("library {path}: {message}")] LibraryInvalid { path: String, message: String }`. `Extras` gains `pub library: Option<&'a GraphLibrary>` (default `None`).

- [ ] **Step 5: Record fixtures and run**

Run: `cargo test -p graphhelm-architect` → all green, golden untouched.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy -p graphhelm-architect --all-targets -- -D warnings
git add core/architect && git commit -m "feat(architect): graph library, road decision and typed parameter fill (sites 4 and 1)

Closes #<task-issue>

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Three doors — CLI, HTTP, MCP

**Files:**
- Modify: `apps/cli/src/args.rs:1012-1041` (`Synthesize` gains `judge_route: Option<String>` `--judge-route`, `judge_fixture: Option<PathBuf>` `--judge-fixture`, `drafts: Option<u8>` `--drafts`, `library: Option<PathBuf>` `--library`), `apps/cli/src/commands/mod.rs:70-93` (pass them through), `apps/cli/src/commands/architect.rs` (`SynthesizeRequest` gains `drafts: Option<u8>`; `execute(request, model, judge: Option<&dyn JudgeModel>, library: Option<&GraphLibrary>)`; `GatewayJudgeModel` over `SystemOneAdapter` with the same `lease_credential`; `build_judge(source: &JudgeSource)`), `apps/cli/src/commands/serve/routes.rs:196-378` (`FIELDS` gains `judgeRoute`, `judgeFixture`, `drafts`, `library`; `ServeJudgeModel` bridging `ServeModelPort`? — no: `ServeModelPort` speaks `ModelCall`; add a serve-side `SystemOneAdapter` door leased through the same broker the serve wiring already opens, mirroring `ServeDraftModel`), `apps/cli/src/commands/mcp/tools.rs:676-708,1337-1360` (schema + forwarded fields)
- Test: `apps/cli/tests/execution_cli.rs` (or the file holding today's `graph synthesize --fixture` journey: `grep -rln "graph.*synthesize" apps/cli/tests`), `apps/cli/tests/api_http.rs`, `apps/cli/tests/mcp_stdio.rs`

**Interfaces:**
- Consumes: Tasks 2–6.
- Produces: the three surfaces; `pub(crate) enum JudgeSource<'a> { Fixture(&'a Path), Gateway { manifest, route, broker, keyring, key_id } }`.

- [ ] **Step 1: Write the failing three-door test**

In `apps/cli/tests/api_http.rs`, beside the existing synthesize cell that asserts HTTP `data` == CLI bytes, add a cell that runs the CLI with `--fixture first-compile/replies.json --judge-fixture judge/nodes-below-threshold.json`, then POSTs `{goal, allowPrograms:["cargo"], fixture, judgeFixture}` and asserts `data` byte-equal, `data.judgments.unresolved == ["summarize"]`. In `mcp_stdio.rs`, the same through the tool with `judgeFixture`. In the CLI journey file: `--drafts 2` without a judge → exit 2 with `GHCLI026` and the refusal text containing `judge`; `--library <fixtures/library> --judge-fixture judge/reuse-reuse.json` with an EMPTY `--fixture` → exit 0, `rounds == 0`, `--out` written.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p graphhelm-cli --test api_http --test mcp_stdio --test execution_cli` → clap rejects the unknown flags / HTTP 400 for an unread field.

- [ ] **Step 3: Implement**

`GatewayJudgeModel` (in `architect.rs`, beside `GatewayDraftModel`):

```rust
pub(crate) struct GatewayJudgeModel { route: ModelRoute, key: SecretBytes }

impl JudgeModel for GatewayJudgeModel {
    fn judge(&self, request: &JudgeRequest) -> Result<JudgeReply, ArchitectRefusal> {
        SystemOneAdapter::new(&self.route, Arc::new(UreqTransport::new()))
            .call(&self.key, request)
            .map_err(|error| ArchitectRefusal::JudgeUnavailable { message: error.to_string() })
    }
}
```

`build_judge`: `Fixture` → `RecordedJudgeModel::from_file`; `Gateway` → load manifest, find route, require `Transport::DirectApi` and provider `typesafe` (else `gateway::invalid("--judge-route must name a direct_api typesafe route", "/judgeRoute")`), `lease_credential`, `GatewayJudgeModel`. `execute` builds `Extras { judge, drafts: request.drafts.unwrap_or(1), library }` and calls `synthesize_with`. `--library` goes through `GraphLibrary::load` with the same `check_out_path`-style refusal to a path outside the working tree? — no: it is a read; accept any directory, refuse a file. HTTP: `library` is a path on the Runtime host, documented like `fixture`. MCP: forward `judgeRoute`, `judgeFixture`, `drafts`, `library` in the body loop.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p graphhelm-cli` → green, including the untouched three-door cells for a call without a judge.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy -p graphhelm-cli --all-targets -- -D warnings
git add apps/cli && git commit -m "feat(cli): judge route, drafts and library on the three synthesize doors

Closes #<task-issue>

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Records — harness doc, decision register, changelog, acceptance recipe

**Files:**
- Modify: `docs/harness/GRAPH_ARCHITECT.md` (§5 item 10 retired with the PR number; new §10 "The judge door and its four sites"; §6 gains the Tier B recipe for a real Jev run), `docs/DECISION_REGISTER.md` (row `D-054 | Typed judgments enter the architect only as diagnostics, a fixed ranking policy, or a report` carrying D1/D3/D4/D6/D8 of the spec in one paragraph, with the reopening door: thresholds move only with a recorded acceptance run), `CHANGELOG.md` (one entry per landed task, or one entry at the end naming all PRs), `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2.6 (add "served by `adapters/model-gateway/src/systemone.rs`; the manifest provider is `typesafe`"), `docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md` (replace "no Runtime adapter exists yet" with the adapter path and the manifest example below)
- Create: `docs/acceptance/architect-judgments-recipe.md` — the keyed run: a manifest route

```json
{ "id": "judge", "provider": "typesafe", "transport": "direct_api", "authentication": "api_key",
  "billingMode": "per_token", "baseUrl": "https://api.typesafe.ai", "model": "jev-latest",
  "credentialRef": "secret_typesafe", "profiles": ["balanced_reasoning"], "enabled": true }
```

  stored with `gateway credential store` (the key never in a file the repo tracks), then `graphhelm graph synthesize --goal ... --allow-program cargo --manifest m.json --route <draft route> --judge-route judge --drafts 3 --library <dir> --out g.json` over the ten goals of `docs/acceptance/m11-first-compile-2026-09-11.md`, recording per goal: road, chosen stance, `unresolved` counts, and a person's yes/no on each `on_goal` answer. That table is what a threshold change cites.

- [ ] **Step 1: Write the docs** (English; no code in prose beyond paths).
- [ ] **Step 2: `git diff --check`; all files LF.**
- [ ] **Step 3: Commit**

```bash
git add docs CHANGELOG.md && git commit -m "docs(architect): the judge door on record; non-goal 10 retired; Tier B recipe

Closes #<task-issue>

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review

**Spec coverage.** D1 → Tasks 2, 3 (both refusal arms tested). D2 → Tasks 1, 3. D3 → Task 4 (`the_judge_is_asked_only_about_a_draft_that_passed_every_deterministic_check`, diagnostics appended), Task 5 (fixed policy), Task 6 (filled template through `compile_round`). D4 → Task 4 `no_judge_is_todays_bytes`, Task 6 `a_library_without_a_judge_is_ignored_and_says_nothing`. D5 → Task 2, recorders in 4/5/6. D6 → `policy_edges_are_exact`, every site's below-threshold cell. D7 → Task 5 template cell. D8 → Task 6. D9 → Task 8. D10 → Task 7. §4 three-door identity WITH a judge → Task 7. §5 gaps → Task 8 recipe.

**Placeholder scan.** `request` in `ranking.rs` and `GraphLibrary::load` are given as comments describing exact behaviour rather than full bodies; the tests around them are complete and pin the behaviour. `<task-issue>` is filled at execution from the child issue number. No "TBD".

**Type consistency.** `Extras` is declared in Task 4 with `judge`, `drafts`; Task 6 adds `library` (stated in both). `synthesize_with` signature is the same in Tasks 4–7. `JudgeReply.usage` is `Usage` everywhere. `read`/`request` naming: `nodes::{request, read}`, `ranking::{request, read}`, `reuse::{decide_request, read_decision, fill_request, read_fill}` — used with those names in the tests. `assemble_prompt` grows to five arguments by Task 6; every caller passes `None, None` when unused.
