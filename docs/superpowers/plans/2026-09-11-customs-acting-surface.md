# Customs Acting Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the customs pipeline its operator verbs (`claim`, `clear`) on CLI, HTTP and MCP, and publish the fold's scan history on the status surface, so a graph that parked at `waiting_input` can be finished honestly — closing #132, #163 and the anchor #159.

**Architecture:** The fold already decides everything (`core/events/src/projection.rs`); this plan adds a decide-then-append verb module beside `sweep.rs`, a typed `CustomsView` in `core/execution` that `render()` embeds, and three thin doors per verb (CLI file, HTTP route, MCP tool) that all call the one verb. A clearance that clears runs the same drive `resume` runs.

**Tech Stack:** Rust 1.97.1 (`cargo +1.97.1`), serde/serde_json, clap, axum (existing), assert_cmd tests. No new dependencies. No schema changes.

**Spec:** `docs/superpowers/specs/2026-09-11-customs-acting-surface-design.md` (read it first; every decision D1–D8 below is argued there).

## Global Constraints

- Toolchain: `cargo +1.97.1`; every command `--locked`. Set `CARGO_TARGET_DIR=E:/o-159-target` before any cargo command (never the worktree default, never `F:`, never a shared dir).
- Work only inside the worktree `D:/o-159` on branch `issue-159-customs-acting-surface`. Never `cd` to `F:/github/GraphHelm`.
- No `TODO`, `unimplemented!`, empty handlers, or stubs (AGENTS.md). Every public fn has behaviour in the same commit.
- Wire names are `snake_case` event kinds and `camelCase` JSON fields; optional fields are `#[serde(default, skip_serializing_if = ...)]` so absent stays absent.
- Tests never read the wall clock, never require network, Docker, credentials.
- Closing keywords: ONLY the final PR body carries `Closes #159`, `Closes #132`, `Closes #163`. Commit messages use `Refs #N`. Never write `close|closes|closed|fix|fixes|fixed|resolve|resolves|resolved #N` for any other number anywhere (title, body, commits).
- Commit body footer on every commit:
  ```
  Session: subagent-<your name> of projeto-status-graphhelm-migration-e008b-f1 [7034f3] | Head: <sha8 of the parent commit>

  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  ```
- Run `cargo +1.97.1 fmt --all` before every commit and `cargo +1.97.1 clippy -p <crate> --all-targets --all-features --locked -- -D warnings` for every crate you touched. `git add` files by name, never `-A`.
- Do not run `ci/gate.ps1` yourself; the orchestrator enqueues it on the registered runner.

---

### Task 1: The verb module in `core/events` — `claim` and `clear`

**Files:**
- Create: `core/events/src/customs.rs`
- Modify: `core/events/src/lib.rs` (add `mod customs;` and `pub use customs::{ClaimError, ClaimOutcome, ClaimRequest, ClearError, claim, clear, refusal};`)
- Test: `core/events/tests/customs_verbs.rs`

**Interfaces:**
- Consumes: `LocalEventRepository::{read_replay_stream, next_sequence, append_atomic}`, `replay`, `PreparedAppend::new`, `ExecutionProjection::{node_states, open_waits, open_claims, customs_scans, clearances, execution_id}`, `claim_evidence_digest`, protocol payloads `CompletionClaimed`, `CompletionCleared`, `CompletionRefused`, `ClearanceVerifier::MachineReplay`, `SafeCode`, `REFUSAL_REASON_CODES`.
- Produces (used by Tasks 3–4):
  ```rust
  pub mod refusal {
      pub const NOT_WAITING: &str = "not_waiting";
      pub const STALE_RENDEZVOUS: &str = "stale_rendezvous";
      pub const UNKNOWN_WAIT: &str = "unknown_wait";
      pub const DUPLICATE_COMPLETION: &str = "duplicate_completion";
      pub const EVIDENCE_BUDGET_UNMET: &str = "evidence_budget_unmet";
  }
  pub struct ClaimRequest<'a> {
      pub node: &'a str,
      pub completes_wait_seq: Option<u64>,
      pub evidence: Vec<graphhelm_protocols::ClaimEvidence>,
      pub attestation: graphhelm_protocols::ClaimAttestation,
      pub required_proof_kinds: &'a [String],
  }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum ClaimOutcome {
      Claimed { claim_seq: u64, wait_seq: u64 },
      Refused { reason_code: &'static str, wait_seq: u64 },
  }
  #[derive(Debug)]
  pub enum ClaimError { NotStarted, Repository(EventRepositoryError) }
  #[derive(Debug)]
  pub enum ClearError { NotStarted, NotAnOpenClaim { claim_seq: u64 }, Repository(EventRepositoryError) }
  pub fn claim(repository: &LocalEventRepository, scope: &RepositoryScope, stream: &str,
               actor: &PersistedActor, key: &OpaqueId, request: ClaimRequest<'_>)
      -> Result<(ClaimOutcome, Vec<EventEnvelope>), ClaimError>;
  pub fn clear(repository: &LocalEventRepository, scope: &RepositoryScope, stream: &str,
               actor: &PersistedActor, key: &OpaqueId, claim_seq: u64, manifest_hash: &WireHash)
      -> Result<(ClearanceOutcome, Vec<EventEnvelope>), ClearError>;
  ```

- [ ] **Step 1: Write the failing tests**

Create `core/events/tests/customs_verbs.rs`. Copy the fixture helpers `scope()`, `event()`, `outcome_event()`, `append()`, `parked_batch()`, `sequence_of()` from `core/events/tests/execution_projection.rs` (lines ~1400–1560; read them, copy verbatim, keep their doc comments' substance). `STREAM` and `CUSTOMS_NODE` constants as there. Then:

```rust
use graphhelm_events::{ClaimError, ClaimOutcome, ClaimRequest, ClearError, claim, clear, refusal, replay};
use graphhelm_protocols::{ClaimAttestation, ClaimAttestationMode, ClaimEvidence, EventKind, NodeState, OpaqueId, WireHash};

fn attestation() -> ClaimAttestation {
    ClaimAttestation {
        asserter: OpaqueId::parse("agent-claimer").unwrap(),
        mode: ClaimAttestationMode::OperatorAttested,
    }
}

fn evidence(kind: &str) -> ClaimEvidence {
    ClaimEvidence {
        kind: kind.to_owned(),
        content_hash: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        size: 7,
    }
}

fn request<'a>(wait: Option<u64>, kinds: &[&str], required: &'a [String]) -> ClaimRequest<'a> {
    ClaimRequest {
        node: CUSTOMS_NODE,
        completes_wait_seq: wait,
        evidence: kinds.iter().map(|kind| evidence(kind)).collect(),
        attestation: attestation(),
        required_proof_kinds: required,
    }
}

/// The positive control every refusal cell below is measured against.
#[test]
fn a_claim_against_the_open_wait_is_journaled_and_enters_quarantine() {
    let (repository, _dir) = fresh_repository();   // helper: LocalEventRepository in a tempdir, same as execution_projection.rs's `append` uses
    let history = append_to(&repository, parked_batch());
    let wait_seq = sequence_of(&history, history.len() - 1);
    let required: Vec<String> = vec!["test_report".to_owned()];
    let (outcome, appended) = claim(&repository, &scope(), STREAM, &actor(), &key("claim-1"), request(Some(wait_seq), &["test_report"], &required)).unwrap();
    let claim_seq = appended[0].sequence;
    assert_eq!(outcome, ClaimOutcome::Claimed { claim_seq, wait_seq });
    assert!(matches!(appended[0].kind, EventKind::CompletionClaimed(_)));
    assert_eq!(claim_seq, history.len() as u64 + 1, "appended at the sequence the decision read");
    let projection = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert!(projection.open_claims.contains_key(&claim_seq), "quarantine holds the claim");
    assert_eq!(projection.node_states.get(CUSTOMS_NODE), Some(&NodeState::WaitingInput), "a claim is testimony, not a transition");
}

/// Absent `completes_wait_seq` answers the node's OPEN wait — the CLI's default.
#[test]
fn a_claim_without_a_named_wait_answers_the_open_one() { /* same arrangement, wait None, assert Claimed { wait_seq == open wait } */ }

/// THE TRAP GUARD (#159): the arrangement must be constructable BEFORE the refusal is demanded.
#[test]
fn a_claim_naming_a_superseded_wait_is_refused_stale_rendezvous_and_the_node_stays_parked() {
    let (repository, _dir) = fresh_repository();
    let mut batch = parked_batch();
    let first_wait_index = batch.len() - 1;
    batch.push(outcome_event("park-again", Outcome::NeedsInput, NodeState::WaitingInput)); // re-park: (WaitingInput, NeedsInput) -> WaitingInput
    let history = append_to(&repository, batch);
    let first_wait = sequence_of(&history, first_wait_index);
    let projection = replay(&scope(), STREAM, &history).unwrap();
    assert_ne!(projection.open_waits[CUSTOMS_NODE].at_sequence, first_wait, "PRECONDITION: the first wait is superseded");
    let (outcome, appended) = claim(&repository, &scope(), STREAM, &actor(), &key("claim-stale"), request(Some(first_wait), &[], &[])).unwrap();
    assert_eq!(outcome, ClaimOutcome::Refused { reason_code: refusal::STALE_RENDEZVOUS, wait_seq: first_wait });
    match &appended[0].kind { EventKind::CompletionRefused(payload) => { assert_eq!(payload.claimed_wait_seq, first_wait); assert_eq!(payload.reason_code.as_str(), "stale_rendezvous"); } other => panic!("expected completion_refused, got {other:?}") }
    let after = replay(&scope(), STREAM, &read_all(&repository)).unwrap();
    assert_eq!(after.node_states.get(CUSTOMS_NODE), Some(&NodeState::WaitingInput));
    assert!(after.open_claims.is_empty());
}

#[test]
fn a_claim_naming_a_sequence_that_never_parked_is_refused_unknown_wait() { /* wait Some(1) (the execution_started event) — never a Parked scan → UNKNOWN_WAIT */ }

#[test]
fn a_claim_on_a_node_that_is_not_waiting_is_refused_not_waiting() { /* history: started + dispatch only (node Queued) → NOT_WAITING; assert precondition node_states != WaitingInput first */ }

#[test]
fn a_second_claim_on_a_claimed_wait_is_refused_duplicate_completion() { /* claim once (Claimed), claim again → DUPLICATE_COMPLETION; assert open_claims.len()==1 after */ }

#[test]
fn a_claim_missing_a_declared_proof_kind_is_refused_evidence_budget_unmet_and_an_extra_kind_is_not_stronger() {
    /* required ["test_report","diff"], presented ["test_report"] → EVIDENCE_BUDGET_UNMET;
       then presented ["test_report","diff","screenshot"] → Claimed (extra accepted) */
}

/// Every code this module can produce is in the frozen registry, and nothing else is.
#[test]
fn every_refusal_code_the_verb_produces_is_in_the_frozen_vocabulary() {
    for code in [refusal::NOT_WAITING, refusal::STALE_RENDEZVOUS, refusal::UNKNOWN_WAIT, refusal::DUPLICATE_COMPLETION, refusal::EVIDENCE_BUDGET_UNMET] {
        assert!(graphhelm_protocols::REFUSAL_REASON_CODES.contains(&code), "{code} is not in REFUSAL_REASON_CODES");
    }
}

#[test]
fn a_clearance_with_the_claimed_bundles_digest_clears_and_releases_the_node() {
    /* park, claim with evidence ["test_report"]; digest = graphhelm_events::claim_evidence_digest(&request_evidence);
       clear(.., claim_seq, &digest) → (ClearanceOutcome::Cleared, appended[0] is CompletionCleared);
       replay: node_states[CUSTOMS_NODE] == Succeeded, open_claims empty, open_waits has no CUSTOMS_NODE */
}

#[test]
fn a_clearance_with_a_foreign_digest_is_journaled_as_rejected_and_spends_the_claim() {
    /* wrong digest → ClearanceOutcome::Refused { reason_code: "hash_mismatch" }; node still WaitingInput; open_claims empty (spent);
       the wait survives so a NEW claim is accepted afterwards (assert Claimed) */
}

#[test]
fn a_clearance_naming_a_sequence_that_is_not_an_open_claim_appends_nothing() {
    /* clear(.., 999, ..) → Err(ClearError::NotAnOpenClaim { claim_seq: 999 }); head sequence unchanged (read_all len equal) */
}

#[test]
fn a_verb_on_a_stream_with_no_execution_is_not_started() { /* empty stream → Err(ClaimError::NotStarted) and Err(ClearError::NotStarted) */ }
```

Write helper fns `fresh_repository() -> (LocalEventRepository, tempfile::TempDir)`, `append_to(&repo, batch) -> Vec<EventEnvelope>`, `read_all(&repo) -> Vec<EventEnvelope>` (via `read_replay_stream(&scope(), STREAM)`), `actor() -> PersistedActor`, `key(&str) -> OpaqueId` by reading how `execution_projection.rs` opens its repository (`LocalEventRepository::create`/`open` with the fixed test key — copy exactly).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-events --test customs_verbs --locked`
Expected: compile error — `graphhelm_events::claim` etc. do not exist.

- [ ] **Step 3: Implement `core/events/src/customs.rs`**

```rust
//! The customs claim and clear verbs (#159): decide against the replayed projection, then
//! append AT THE SEQUENCE THE DECISION READ. `append_atomic` refuses with `SequenceConflict`
//! when the stream moved in between, so a decision can never be applied to a different world
//! than the one it was made in (the #74 pattern `sweep.rs` uses). A refused claim is a JOURNAL
//! EVENT with a registry code — a graph that cannot finish leaves a legible trail.

use graphhelm_protocols::{
    ClaimAttestation, ClaimEvidence, ClearanceVerifier, CompletionClaimed, CompletionCleared,
    CompletionRefused, EventEnvelope, EventKind, NewEvent, NodeState, OpaqueId, PersistedActor,
    RepositoryScope, SafeCode, Sensitivity, WireHash,
};

use crate::projection::{ClearanceOutcome, CustomsStage};
use crate::store::EventRepositoryError;
use crate::{ExecutionProjection, LocalEventRepository, PreparedAppend, replay};

pub mod refusal {
    pub const NOT_WAITING: &str = "not_waiting";
    pub const STALE_RENDEZVOUS: &str = "stale_rendezvous";
    pub const UNKNOWN_WAIT: &str = "unknown_wait";
    pub const DUPLICATE_COMPLETION: &str = "duplicate_completion";
    pub const EVIDENCE_BUDGET_UNMET: &str = "evidence_budget_unmet";
}

pub struct ClaimRequest<'a> { /* as in Interfaces */ }
pub enum ClaimOutcome { /* as in Interfaces */ }
pub enum ClaimError { NotStarted, Repository(EventRepositoryError) }
pub enum ClearError { NotStarted, NotAnOpenClaim { claim_seq: u64 }, Repository(EventRepositoryError) }
impl From<EventRepositoryError> for ClaimError { fn from(e: EventRepositoryError) -> Self { Self::Repository(e) } }
impl From<EventRepositoryError> for ClearError { fn from(e: EventRepositoryError) -> Self { Self::Repository(e) } }

fn decide_claim(projection: &ExecutionProjection, request: &ClaimRequest<'_>) -> Result<u64, (&'static str, u64)> {
    let node = request.node;
    let state = projection.node_states.get(node).copied().unwrap_or(NodeState::Draft);
    let named = request.completes_wait_seq.unwrap_or(0);
    if state != NodeState::WaitingInput {
        return Err((refusal::NOT_WAITING, named));
    }
    let open = projection.open_waits.get(node).map(|wait| wait.at_sequence);
    let wait_seq = match (request.completes_wait_seq, open) {
        (None, Some(open)) => open,
        (Some(named), Some(open)) if named == open => open,
        (Some(named), _) => {
            let parked_there = projection.customs_scans.get(node).is_some_and(|scans| {
                scans.iter().any(|scan| scan.stage == CustomsStage::Parked && scan.at_sequence == named)
            });
            return Err((if parked_there { refusal::STALE_RENDEZVOUS } else { refusal::UNKNOWN_WAIT }, named));
        }
        (None, None) => return Err((refusal::UNKNOWN_WAIT, 0)),
    };
    if projection.open_claims.values().any(|claim| claim.node == node) {
        return Err((refusal::DUPLICATE_COMPLETION, wait_seq));
    }
    let presented: std::collections::BTreeSet<&str> = request.evidence.iter().map(|item| item.kind.as_str()).collect();
    if request.required_proof_kinds.iter().any(|kind| !presented.contains(kind.as_str())) {
        return Err((refusal::EVIDENCE_BUDGET_UNMET, wait_seq));
    }
    Ok(wait_seq)
}

pub fn claim(repository: &LocalEventRepository, scope: &RepositoryScope, stream: &str, actor: &PersistedActor, key: &OpaqueId, request: ClaimRequest<'_>) -> Result<(ClaimOutcome, Vec<EventEnvelope>), ClaimError> {
    let history = repository.read_replay_stream(scope, stream)?;
    let projection = replay(scope, stream, &history).map_err(|_| EventRepositoryError::Invalid)?;
    let execution_id = projection.execution_id.as_deref().and_then(|id| OpaqueId::parse(id).ok()).ok_or(ClaimError::NotStarted)?;
    let node_id = OpaqueId::parse(request.node).map_err(|_| EventRepositoryError::Invalid)?;
    let at = repository.next_sequence(scope, stream)?;
    let (kind, outcome_of) = match decide_claim(&projection, &request) {
        Ok(wait_seq) => (
            EventKind::CompletionClaimed(CompletionClaimed { execution_id, node: node_id, completes_wait_seq: wait_seq, evidence: request.evidence.clone(), attestation: request.attestation.clone() }),
            Ok(wait_seq),
        ),
        Err((code, wait_seq)) => (
            EventKind::CompletionRefused(CompletionRefused { execution_id, node: node_id, claimed_wait_seq: wait_seq, reason_code: SafeCode::parse(code).expect("registry codes are valid SafeCodes") }),
            Err((code, wait_seq)),
        ),
    };
    let appended = append_one(repository, scope, stream, at, key, actor, kind)?;
    let claim_seq = appended[0].sequence;
    let outcome = match outcome_of {
        Ok(wait_seq) => ClaimOutcome::Claimed { claim_seq, wait_seq },
        Err((reason_code, wait_seq)) => ClaimOutcome::Refused { reason_code, wait_seq },
    };
    Ok((outcome, appended))
}

pub fn clear(repository: &LocalEventRepository, scope: &RepositoryScope, stream: &str, actor: &PersistedActor, key: &OpaqueId, claim_seq: u64, manifest_hash: &WireHash) -> Result<(ClearanceOutcome, Vec<EventEnvelope>), ClearError> {
    let history = repository.read_replay_stream(scope, stream)?;
    let projection = replay(scope, stream, &history).map_err(|_| EventRepositoryError::Invalid)?;
    let execution_id = projection.execution_id.as_deref().and_then(|id| OpaqueId::parse(id).ok()).ok_or(ClearError::NotStarted)?;
    // REFUSED WITHOUT APPENDING: the fold reads a clearance with no claim under it as Corrupt, and a
    // verb that journaled one would poison every later replay of this stream.
    if !projection.open_claims.contains_key(&claim_seq) {
        return Err(ClearError::NotAnOpenClaim { claim_seq });
    }
    let at = repository.next_sequence(scope, stream)?;
    let kind = EventKind::CompletionCleared(CompletionCleared { execution_id, claim_seq, verifier: ClearanceVerifier::MachineReplay { manifest_hash: manifest_hash.clone() } });
    let appended = append_one(repository, scope, stream, at, key, actor, kind)?;
    // The VERDICT is the fold's, read back from the journal that now holds the clearance — never
    // recomputed here, which would be a second oracle.
    let mut full = history;
    full.extend(appended.iter().cloned());
    let after = replay(scope, stream, &full).map_err(|_| EventRepositoryError::Invalid)?;
    let outcome = after.clearances.get(&claim_seq).cloned().ok_or(EventRepositoryError::Invalid)?;
    Ok((outcome, appended))
}

fn append_one(repository: &LocalEventRepository, scope: &RepositoryScope, stream: &str, at: u64, key: &OpaqueId, actor: &PersistedActor, kind: EventKind) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
    let event = NewEvent::new(key.clone(), actor.clone(), Sensitivity::Internal, kind, Vec::new(), Vec::new());
    let request = PreparedAppend::new(scope.clone(), OpaqueId::parse(stream).map_err(|_| EventRepositoryError::Invalid)?, at, vec![event], Vec::new(), Vec::new())?;
    repository.append_atomic(&request)
}
```

Adapt names to the real signatures you read (e.g. `read_replay_stream`'s exact name and return type are in `sweep.rs`; `ClearanceOutcome` derives `Clone`). Keep the module doc and the two load-bearing comments.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-events --test customs_verbs --locked`
Expected: all tests PASS. Then `cargo +1.97.1 test -p graphhelm-events --locked` (whole crate) still green.

- [ ] **Step 5: Commit**

```bash
git add core/events/src/customs.rs core/events/src/lib.rs core/events/tests/customs_verbs.rs
git commit -m "feat(events): the claim and clear verbs decide against the replayed fold and append at the sequence they read

Refs #159, #132"
```
(plus the footer from Global Constraints).

---

### Task 2: `CustomsView` in `core/execution`, embedded by `render()`

**Files:**
- Create: `core/execution/src/customs.rs`
- Modify: `core/execution/src/lib.rs` (add `mod customs; pub use customs::{CustomsView, NodeCustomsView, OpenClaimView, customs_view};`)
- Modify: `apps/cli/src/commands/execution/mod.rs:775-840` (`render()` gains `"customs"`)
- Modify: `apps/cli/src/commands/execution/list.rs` (confirm list rows do NOT carry `customs`, the way they drop `nodeStates`; add the key to whatever strip list exists)
- Test: unit tests inside `core/execution/src/customs.rs`; one assertion added to `apps/cli/src/commands/execution/mod.rs` tests that `render()` output has a `customs` object with `quarantinedNodes` and `nodes`.

**Interfaces:**
- Consumes: `graphhelm_events::{ExecutionProjection, CustomsScan, OpenWait, OpenClaim, ClearanceOutcome}`.
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct OpenClaimView { pub claim_seq: u64, pub completes_wait_seq: u64, pub stage_entered_at: u64,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub deadline: Option<PersistedTimestamp>,
      pub evidence_digest: WireHash }
  #[derive(...)] #[serde(rename_all = "camelCase")]
  pub struct NodeCustomsView { pub scans: Vec<CustomsScan>,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub open_wait: Option<OpenWait>,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub open_claim: Option<OpenClaimView> }
  #[derive(...)] #[serde(rename_all = "camelCase")]
  pub struct CustomsView { pub quarantined_nodes: Vec<String>, pub nodes: BTreeMap<String, NodeCustomsView>,
      pub clearances: BTreeMap<u64, ClearanceOutcome> }
  pub fn customs_view(projection: &ExecutionProjection) -> CustomsView;
  ```

- [ ] **Step 1: Write the failing unit tests** (in `core/execution/src/customs.rs` `#[cfg(test)] mod tests`)

```rust
#[test]
fn a_projection_with_no_customs_activity_renders_an_empty_view() {
    let projection = ExecutionProjection::default();
    let view = customs_view(&projection);
    assert!(view.quarantined_nodes.is_empty());
    assert!(view.nodes.is_empty());
    assert!(view.clearances.is_empty());
}

#[test]
fn an_open_claim_names_its_node_as_quarantined_and_the_view_copies_the_folds_deadline_verbatim() {
    let mut projection = ExecutionProjection::default();
    let deadline = PersistedTimestamp::parse("2026-09-11T00:00:00Z").unwrap();
    projection.open_claims.insert(7, OpenClaim { node: "implementation".to_owned(), completes_wait_seq: 4, stage_entered_at: 7, deadline: Some(deadline.clone()), evidence_digest: WireHash::parse(format!("sha256:{}", "c".repeat(64))).unwrap() });
    projection.customs_scans.insert("implementation".to_owned(), vec![CustomsScan { at_sequence: 7, stage: CustomsStage::Claimed, claim_seq: Some(7), reason_code: None, deadline: Some(deadline.clone()) }]);
    let view = customs_view(&projection);
    assert_eq!(view.quarantined_nodes, vec!["implementation".to_owned()]);
    let node = &view.nodes["implementation"];
    assert_eq!(node.open_claim.as_ref().unwrap().claim_seq, 7);
    assert_eq!(node.open_claim.as_ref().unwrap().deadline, Some(deadline));
    assert_eq!(node.scans.len(), 1);
}

#[test]
fn serialization_is_camel_case_and_omits_absent_optionals() {
    /* build a view with open_wait None, open_claim None; to_value; assert keys == ["scans"] for that node and the top-level keys are exactly ["clearances","nodes","quarantinedNodes"] */
}
```

- [ ] **Step 2: Run to verify failure** — `cargo +1.97.1 test -p graphhelm-execution customs --locked` → compile error (module missing).

- [ ] **Step 3: Implement**

```rust
//! #163: the scan history as ONE typed value, rendered by `render()` for both `execution status`
//! and `GET /v1/executions/{id}`. Nothing here derives: every number is copied from the fold.
//! `quarantined_nodes` IS `open_claims`' key set — the fold maintains that map (inserted on
//! `completion_claimed`, removed on cleared/rejected), and a second computation over the scan
//! history would be a duplicate oracle that could disagree with it after a fold change.

use std::collections::{BTreeMap, BTreeSet};
use graphhelm_events::{ClearanceOutcome, CustomsScan, ExecutionProjection, OpenWait};
use graphhelm_protocols::{PersistedTimestamp, WireHash};
use serde::{Deserialize, Serialize};

/* structs as in Interfaces */

#[must_use]
pub fn customs_view(projection: &ExecutionProjection) -> CustomsView {
    let mut names: BTreeSet<&str> = projection.customs_scans.keys().map(String::as_str).collect();
    names.extend(projection.open_waits.keys().map(String::as_str));
    names.extend(projection.open_claims.values().map(|claim| claim.node.as_str()));
    let mut nodes = BTreeMap::new();
    for name in names {
        let open_claim = projection.open_claims.iter().find(|(_, claim)| claim.node == name).map(|(seq, claim)| OpenClaimView {
            claim_seq: *seq, completes_wait_seq: claim.completes_wait_seq, stage_entered_at: claim.stage_entered_at,
            deadline: claim.deadline.clone(), evidence_digest: claim.evidence_digest.clone(),
        });
        nodes.insert(name.to_owned(), NodeCustomsView {
            scans: projection.customs_scans.get(name).cloned().unwrap_or_default(),
            open_wait: projection.open_waits.get(name).cloned(),
            open_claim,
        });
    }
    let mut quarantined: Vec<String> = projection.open_claims.values().map(|claim| claim.node.clone()).collect();
    quarantined.sort(); quarantined.dedup();
    CustomsView { quarantined_nodes: quarantined, nodes, clearances: projection.clearances.clone() }
}
```

Then in `render()` (`apps/cli/src/commands/execution/mod.rs`), after `"nodeStates"`, add:

```rust
        // #163: the scan history, ONE typed value shared by every door that calls render().
        "customs": serde_json::to_value(graphhelm_execution::customs_view(projection))
            .expect("a view built from already-serializable fold types serializes"),
```

Read `list.rs` to find how `nodeStates` is kept off list rows and apply the same to `customs`.

- [ ] **Step 4: Run** `cargo +1.97.1 test -p graphhelm-execution --locked` and `cargo +1.97.1 test -p graphhelm-cli --test execution_cli --locked` (existing status tests must still pass; any that assert exact key sets of `render()` output need `customs` added).

- [ ] **Step 5: Commit** — `git add core/execution/src/customs.rs core/execution/src/lib.rs apps/cli/src/commands/execution/mod.rs apps/cli/src/commands/execution/list.rs` (+ any test file touched); message `feat(execution): the scan history is one typed view, rendered by the door both surfaces share` / `Refs #163`.

---

### Task 3: CLI verbs `execution claim` and `execution clear`, the example graph, and the acting journey test

**Files:**
- Modify: `apps/cli/src/args.rs:310-530` (two variants on `ExecutionCommand`)
- Modify: `apps/cli/src/commands/mod.rs:239-244` (dispatch arms)
- Modify: `apps/cli/src/commands/execution/mod.rs` (declare `pub(crate) mod claim; pub(crate) mod clear;`, add `verify_graph_matches_execution` helper, add `load_claim_evidence(path) -> Result<Vec<ClaimEvidence>, Failure>`)
- Modify: `apps/cli/src/commands/execution/resume.rs:145-185` (call the helper instead of its inline copy; behaviour byte-identical)
- Create: `apps/cli/src/commands/execution/claim.rs`, `apps/cli/src/commands/execution/clear.rs`
- Create: `examples/graphs/customs-acting.yaml`
- Test: `apps/cli/tests/customs_cli.rs`

**Interfaces:**
- Consumes: Task 1 verbs; Task 2 `render()`; `drive_to_quiescence` (`driver.rs`), `Release`, `FixtureExecutor`, `load_fixtures`, `resolve_stream`, `replay_projection`, `owner_actor`, `system_actor`, `idempotency_key`, `execution_state`, `argument`, `finish`, `publish_loaded`, `graphhelm_protocols::GraphNode::customs()`.
- Produces (used by Task 4):
  ```rust
  // execution/mod.rs
  pub(crate) fn verify_graph_matches_execution(version: &GraphVersion, initial: &ExecutionProjection, history: &[EventEnvelope], verb: &'static str) -> Result<(), Failure>;
  pub(crate) fn load_claim_evidence(path: &Path) -> Result<Vec<ClaimEvidence>, Failure>;
  pub(crate) fn parse_claim_evidence(value: &serde_json::Value) -> Result<Vec<ClaimEvidence>, Failure>;
  // claim.rs
  pub(crate) fn execute(version: &GraphVersion, events: &Path, execution: Option<&str>, node: &str, wait_seq: Option<u64>, evidence: Vec<ClaimEvidence>, attestation: ClaimAttestation, actor: PersistedActor, key: OpaqueId) -> Result<serde_json::Value, Failure>;
  // clear.rs
  pub(crate) enum Verifier { MachineReplay(WireHash) }   // countersign is refused before this type is built
  pub(crate) fn decide(version: &GraphVersion, events: &Path, fixtures: Option<&Path>, execution: Option<&str>, claim_seq: u64, verifier: &Verifier, actor: PersistedActor, key: OpaqueId) -> Result<(ClearanceOutcome, PreparedDrive), Failure>;
  pub(crate) fn execute(version, events, fixtures, execution, claim_seq, verifier, actor, key) -> Result<serde_json::Value, Failure>;  // decide + sync drive when Cleared + render + "clearance" key
  pub(crate) fn annotate(value: &mut serde_json::Value, outcome: &ClearanceOutcome, claim_seq: u64);
  ```

- [ ] **Step 1: Write the example graph** `examples/graphs/customs-acting.yaml`

```yaml
apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_customs_acting_v1
  name: External work completed through customs
  executionId: exec_customs_acting
  version: 1
  labels:
    origin: user
    mode: supervised
spec:
  entrypoints:
    - implementation
  nodes:
    implementation:
      type: agent
      name: Implementation done outside the runtime
      objective: Land the change in the repository and present the test report.
      optionality: required
      agent:
        ephemeral:
          purpose: Implement the change.
          capabilities:
            - repository.write_patch
          inputSchema: schema://ImplementationPlan@1
          outputSchema: schema://ImplementationResult@1
          instructions: Produce the patch and the test report.
          completionContract:
            requires:
              - test_report
          isolationMinimum: tier_1
      completion:
        requires:
          - outputSchemaValid: true
        customs:
          proofKinds:
            - test_report
          budgets:
            waitWithinSeconds: 86400
            clearanceWithinSeconds: 3600
    release_notes:
      type: agent
      name: Release notes
      objective: Summarize the landed change for the changelog.
      optionality: required
      agent:
        ephemeral:
          purpose: Write the release note.
          capabilities:
            - docs.write
          inputSchema: schema://ImplementationResult@1
          outputSchema: schema://ReleaseNote@1
          instructions: One paragraph, cite the test report.
          completionContract:
            requires:
              - release_note
          isolationMinimum: tier_0
      completion:
        requires:
          - outputSchemaValid: true
        customs:
          proofKinds: []
          budgets:
            waitWithinSeconds: 86400
            clearanceWithinSeconds: 3600
  edges:
    - id: implementation_to_release_notes
      from: implementation
      to: release_notes
      type: control
  budgets:
    maxNodes: 4
    maxDepth: 2
    maxMutations: 1
    maxRetriesPerNode: 1
  policies: []
  completion:
    terminalNodes:
      - release_notes
    allowWaivers: false
```

Verify: `cargo +1.97.1 run --locked -p graphhelm-cli -- graph lint examples/graphs/customs-acting.yaml` → `ok: true` with ZERO `GHG102` warnings (if the schema rejects `proofKinds: []`, drop the key on `release_notes` — `proof_kinds` is `default`).

- [ ] **Step 2: Write the failing CLI journey test** `apps/cli/tests/customs_cli.rs`

Copy `root()`, `command()`, `write_json`, `replay_projection` from `apps/cli/tests/execution_cli.rs`, and the journal reader (`journal_kinds`, opening the repository directly) from `apps/cli/tests/sweep_cli.rs`. Then:

```rust
fn graph() -> PathBuf { root().join("examples/graphs/customs-acting.yaml") }

fn start_parked(directory: &Path, execution: &str) -> (PathBuf, PathBuf) {
    let events = directory.join("events");
    let fixtures = write_json(directory, "fixtures.json", serde_json::json!({"nodeOutcomes": {"implementation": "unknown", "release_notes": "success"}}));
    let output = command().args(["execution", "start", "--file", graph().to_str().unwrap(), "--events", events.to_str().unwrap(), "--fixtures", fixtures.to_str().unwrap(), "--mode", "supervised", "--execution", execution]).output().unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["data"]["nodeStates"]["implementation"], "waiting_input", "PRECONDITION: the node parked");
    (events, fixtures)
}

fn evidence_file(directory: &Path, name: &str, kinds: &[&str]) -> PathBuf {
    let items: Vec<Value> = kinds.iter().map(|kind| serde_json::json!({"kind": kind, "contentHash": format!("sha256:{}", "d".repeat(64)), "size": 42})).collect();
    write_json(directory, name, Value::Array(items))
}

fn claim(events: &Path, execution: &str, evidence: &Path, wait_seq: Option<u64>) -> Value {
    let mut args = vec!["execution".to_owned(), "claim".to_owned(), "--file".into(), graph().to_string_lossy().into_owned(), "--events".into(), events.to_string_lossy().into_owned(), "--execution".into(), execution.to_owned(), "--node".into(), "implementation".into(), "--evidence".into(), evidence.to_string_lossy().into_owned()];
    if let Some(seq) = wait_seq { args.push("--wait-seq".into()); args.push(seq.to_string()); }
    let output = command().args(&args).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

fn clear(events: &Path, fixtures: &Path, execution: &str, claim_seq: u64, evidence: &Path) -> Value { /* execution clear --file graph --events --fixtures --execution --claim-seq N --evidence <file> */ }

fn clear_with_hash(events: &Path, fixtures: &Path, execution: &str, claim_seq: u64, hash: &str) -> Value { /* --manifest-hash */ }

/// THE SEALED ACCEPTANCE (#159): acting 4/4 on one surface.
#[test]
fn a_parked_node_is_claimed_cleared_and_the_graph_finishes() {
    let directory = tempfile::tempdir().unwrap();
    let (events, fixtures) = start_parked(directory.path(), "exec-acting");
    // 1. Refused when the budget is unmet — named code, node untouched.
    let short = evidence_file(directory.path(), "short.json", &["diff"]);
    let refused = claim(&events, "exec-acting", &short, None);
    assert_eq!(refused["ok"], true);
    assert_eq!(refused["data"]["claim"]["outcome"], "refused");
    assert_eq!(refused["data"]["claim"]["reasonCode"], "evidence_budget_unmet");
    assert_eq!(refused["data"]["nodeStates"]["implementation"], "waiting_input");
    // 2. Claimed — quarantine visible, downstream not released.
    let full = evidence_file(directory.path(), "full.json", &["test_report"]);
    let claimed = claim(&events, "exec-acting", &full, None);
    assert_eq!(claimed["data"]["claim"]["outcome"], "claimed", "{claimed}");
    let claim_seq = claimed["data"]["claim"]["claimSeq"].as_u64().unwrap();
    assert_eq!(claimed["data"]["customs"]["quarantinedNodes"], serde_json::json!(["implementation"]));
    assert_eq!(claimed["data"]["nodeStates"]["implementation"], "waiting_input");
    assert_ne!(claimed["data"]["nodeStates"]["release_notes"], "succeeded");
    // 3. A wrong digest is REJECTED, spends the claim, releases nothing.
    let rejected = clear_with_hash(&events, &fixtures, "exec-acting", claim_seq, &format!("sha256:{}", "0".repeat(64)));
    assert_eq!(rejected["data"]["clearance"]["outcome"], "rejected", "{rejected}");
    assert_eq!(rejected["data"]["clearance"]["reasonCode"], "hash_mismatch");
    assert_ne!(rejected["data"]["nodeStates"]["release_notes"], "succeeded");
    assert_eq!(rejected["data"]["customs"]["quarantinedNodes"], serde_json::json!([]));
    // 4. Claim again (the wait survived), clear with the bundle → the drive finishes the graph.
    let claimed_again = claim(&events, "exec-acting", &full, None);
    let claim_seq = claimed_again["data"]["claim"]["claimSeq"].as_u64().unwrap();
    let cleared = clear(&events, &fixtures, "exec-acting", claim_seq, &full);
    assert_eq!(cleared["data"]["clearance"]["outcome"], "cleared", "{cleared}");
    assert_eq!(cleared["data"]["nodeStates"]["implementation"], "succeeded");
    assert_eq!(cleared["data"]["nodeStates"]["release_notes"], "succeeded");
    assert_eq!(cleared["data"]["status"], "completed");
    // The journal, read directly: the family in order, and the scan history that replays it.
    let kinds = journal_kinds(&events);
    let customs: Vec<&str> = kinds.iter().map(String::as_str).filter(|k| k.starts_with("completion_")).collect();
    assert_eq!(customs, ["completion_refused", "completion_claimed", "completion_cleared", "completion_claimed", "completion_cleared"]);
    let replayed = replay_projection(&events);
    let stages: Vec<&str> = replayed["customsScans"]["implementation"].as_array().unwrap().iter().map(|s| s["stage"].as_str().unwrap()).collect();
    assert_eq!(stages, ["parked", "refused", "claimed", "rejected", "claimed", "cleared"]);
}

/// THE TRAP GUARD on this surface: a claim naming a superseded wait is refused stale_rendezvous.
#[test]
fn a_claim_naming_a_superseded_wait_is_refused_and_the_node_stays_parked() {
    /* arrange: start_parked; read the open wait's at_sequence from `status` (data.customs.nodes.implementation.openWait.atSequence);
       supersede it: `execution resume` will not re-dispatch a waiting node, so supersede through the journal the way
       execution_projection.rs does — NOT available from the CLI. Instead name a sequence that is a PARKED scan of another
       arrangement: run `execution pause` then `execution resume` (waiting node untouched) — still one wait. So: use
       `--wait-seq 1` (the execution_started event): expect `unknown_wait`; then assert the stale case at the verb level is
       covered by core/events/tests/customs_verbs.rs and name that test in a comment. Assert here: reasonCode == "unknown_wait",
       nodeStates.implementation == "waiting_input", journal ends with completion_refused. */
}

#[test]
fn a_countersign_clearance_is_refused_at_the_door_and_appends_nothing() {
    /* start_parked, claim; head = replay_projection(&events)["headSequence"] (or journal len);
       run `execution clear ... --verifier countersign --manifest-hash sha256:…` → ok == false, diagnostics[0].code == "GHCLI005_EXECUTION_STATE", message contains "#529";
       journal len unchanged */
}

#[test]
fn a_clearance_naming_a_sequence_that_is_not_an_open_claim_is_refused_without_a_journal_entry() { /* --claim-seq 999 → GHCLI005_EXECUTION_STATE, journal unchanged */ }

#[test]
fn a_claim_against_a_graph_that_is_not_the_one_the_execution_started_from_is_refused() { /* --file examples/graphs/manual-override-deploy.yaml → GHCLI005_EXECUTION_STATE; journal unchanged */ }

#[test]
fn replay_of_the_acting_journal_is_byte_identical_and_a_rejected_clearance_replays_as_rejected() {
    /* run the journey to the rejected step; `graph replay --events` twice; assert stdout bytes equal; assert clearances[claimSeq].type == "refused" */
}
```

- [ ] **Step 3: Run to verify failure** — `cargo +1.97.1 test -p graphhelm-cli --test customs_cli --locked` → the `claim` subcommand is unknown (exit 2, JSON parse fails).

- [ ] **Step 4: Implement the args**

In `apps/cli/src/args.rs`, inside `ExecutionCommand` after `Sweep`:

```rust
    /// Claim that a `waiting_input` node's external work is done, presenting evidence. Testimony
    /// only: nothing is released until `execution clear` countersigns. A claim the pipeline
    /// cannot accept is journaled as `completion_refused` with its registry code.
    Claim {
        /// The graph this execution started from — checked against the recorded hash, and the
        /// source of the node's declared `completion.customs.proofKinds`.
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long)]
        node: String,
        /// The envelope sequence of the exact open wait this answers. Absent: the node's open wait.
        #[arg(long = "wait-seq")]
        wait_seq: Option<u64>,
        /// A JSON array of `{"kind","contentHash","size"}` — the wire shape of `ClaimEvidence`.
        #[arg(long)]
        evidence: PathBuf,
        /// Who asserts the completion. Absent: the owner actor this command runs as.
        #[arg(long)]
        asserter: Option<String>,
        /// `operator_attested` (default) or `machine_verified`.
        #[arg(long, default_value = "operator_attested")]
        mode: String,
    },
    /// Countersign a claim by machine replay: present the digest of the evidence bundle you hold.
    /// A matching digest clears the node and drives its dependents; a mismatch is journaled as a
    /// rejection and drives nothing.
    Clear {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        fixtures: Option<PathBuf>,
        #[arg(long)]
        execution: Option<String>,
        #[arg(long = "claim-seq")]
        claim_seq: u64,
        /// `sha256:<64 hex>` — the bundle digest computed elsewhere. Exactly one of this and `--evidence`.
        #[arg(long = "manifest-hash", conflicts_with = "evidence")]
        manifest_hash: Option<String>,
        /// The bundle itself (same JSON shape as `claim --evidence`); its digest is computed here.
        #[arg(long, conflicts_with = "manifest_hash")]
        evidence: Option<PathBuf>,
        /// Only `machine_replay` exists today; `countersign` is refused and names the decision that
        /// will supply it.
        #[arg(long, default_value = "machine_replay")]
        verifier: String,
    },
```

Dispatch in `apps/cli/src/commands/mod.rs` next to `ExecutionCommand::Sweep`:

```rust
            ExecutionCommand::Claim { file, events, execution, node, wait_seq, evidence, asserter, mode } =>
                execution::claim::run(&file, &events, execution.as_deref(), &node, wait_seq, &evidence, asserter.as_deref(), &mode),
            ExecutionCommand::Clear { file, events, fixtures, execution, claim_seq, manifest_hash, evidence, verifier } =>
                execution::clear::run(&file, &events, fixtures.as_deref(), execution.as_deref(), claim_seq, manifest_hash.as_deref(), evidence.as_deref(), &verifier),
```

- [ ] **Step 5: Factor the file-trust seam and the evidence loader in `execution/mod.rs`**

Move `resume.rs:145-185` (from `let supplied_hash = ...` through the `match recorded_hash { ... }`) into:

```rust
/// The file-trust seam `resume` established (05d Task 7) and `claim`/`clear` share: the supplied
/// graph's content hash must equal the hash this execution recorded, checked BEFORE any append.
pub(crate) fn verify_graph_matches_execution(version: &GraphVersion, initial: &ExecutionProjection, history: &[EventEnvelope], verb: &'static str) -> Result<(), Failure> {
    /* body verbatim from resume.rs, with the two messages built as format!("{verb} refused: …") */
}

pub(crate) fn parse_claim_evidence(value: &serde_json::Value) -> Result<Vec<ClaimEvidence>, Failure> {
    serde_json::from_value(value.clone()).map_err(|_| argument("evidence must be a JSON array of {kind, contentHash, size}", "/evidence"))
}

pub(crate) fn load_claim_evidence(path: &Path) -> Result<Vec<ClaimEvidence>, Failure> {
    let bytes = std::fs::read(path).map_err(|_| argument("--evidence does not name a readable file", "/evidence"))?;
    if bytes.len() > 1 << 20 { return Err(argument("--evidence exceeds 1 MiB", "/evidence")); }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| argument("--evidence is not JSON", "/evidence"))?;
    parse_claim_evidence(&value)
}
```

`resume.rs` calls `verify_graph_matches_execution(version, &initial, &history, "resume")?;` in place of the moved block. Run `cargo +1.97.1 test -p graphhelm-cli --test execution_cli --locked` — resume's own refusal tests must stay green with identical messages.

- [ ] **Step 6: Implement `claim.rs`**

```rust
use std::path::Path;
use graphhelm_events::{ClaimError, ClaimOutcome, ClaimRequest};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{ClaimAttestation, ClaimAttestationMode, ClaimEvidence, OpaqueId, PersistedActor};
use super::{Failure, argument, execution_state, finish, idempotency_key, load_claim_evidence, owner_actor, render, replay_failure, replay_projection, repository_failure, resolve_stream, verify_graph_matches_execution};
use crate::commands::{event_store, owner, publish_loaded};
use crate::output::Outcome;

const COMMAND: &str = "execution.claim";

pub fn run(file: &Path, events: &Path, execution: Option<&str>, node: &str, wait_seq: Option<u64>, evidence: &Path, asserter: Option<&str>, mode: &str) -> Outcome {
    let loaded = match graphhelm_schema::load_graph(file) { Ok(l) => l, Err(d) => return Outcome::domain(COMMAND, d) };
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    if !report.errors.is_empty() { let mut d = report.errors; d.extend(report.warnings); return Outcome::domain(COMMAND, d); }
    let warnings = report.warnings;
    let version = match publish_loaded(&loaded, owner("owner-local")) { Ok(v) => v, Err(e) => return Outcome::internal(COMMAND, e).with_warnings(warnings) };
    let actor = owner_actor();
    let result = (|| {
        let evidence = load_claim_evidence(evidence)?;
        let attestation = attestation(asserter, mode, &actor)?;
        execute(&version, events, execution, node, wait_seq, evidence, attestation, actor.clone(), idempotency_key("completion-claim"))
    })();
    finish(COMMAND, result, |value| value).with_warnings(warnings)
}

pub(crate) fn attestation(asserter: Option<&str>, mode: &str, actor: &PersistedActor) -> Result<ClaimAttestation, Failure> {
    let mode = match mode { "operator_attested" => ClaimAttestationMode::OperatorAttested, "machine_verified" => ClaimAttestationMode::MachineVerified, _ => return Err(argument("--mode must be operator_attested or machine_verified", "/mode")) };
    let asserter = OpaqueId::parse(asserter.map(str::to_owned).unwrap_or_else(|| actor.id().to_string()))   // read PersistedActor's accessor name in core/protocols
        .map_err(|_| argument("--asserter is not a valid identifier", "/asserter"))?;
    Ok(ClaimAttestation { asserter, mode })
}

pub(crate) fn execute(version: &GraphVersion, events: &Path, execution: Option<&str>, node: &str, wait_seq: Option<u64>, evidence: Vec<ClaimEvidence>, attestation: ClaimAttestation, actor: PersistedActor, key: OpaqueId) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|e| repository_failure(&e))?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let initial = graphhelm_events::replay(&scope, &stream, &history).map_err(|e| replay_failure(&e))?;
    verify_graph_matches_execution(version, &initial, &history, "claim")?;
    let required: Vec<String> = match version.graph().spec.nodes.get(node) {
        None => return Err(execution_state("the graph has no node with that id", "/node")),
        Some(graph_node) => graph_node.customs().map_err(|_| execution_state("the node's completion.customs block is malformed", "/node"))?.map(|c| c.proof_kinds).unwrap_or_default(),
    };
    let (outcome, _appended) = graphhelm_events::claim(&store, &scope, &stream, &actor, &key, ClaimRequest { node, completes_wait_seq: wait_seq, evidence, attestation, required_proof_kinds: &required })
        .map_err(|error| match error { ClaimError::NotStarted => execution_state("no execution has started on this stream", "/execution"), ClaimError::Repository(e) => repository_failure(&e) })?;
    let projection = replay_projection(&store, &scope, &stream)?;
    let mut value = render(&projection, &graphhelm_execution::AttentionInputs::default(), &super::Liveness::from_store(&store, &scope, &stream));
    value["claim"] = match outcome {
        ClaimOutcome::Claimed { claim_seq, wait_seq } => serde_json::json!({"outcome": "claimed", "claimSeq": claim_seq, "reasonCode": null, "waitSeq": wait_seq}),
        ClaimOutcome::Refused { reason_code, wait_seq } => serde_json::json!({"outcome": "refused", "claimSeq": null, "reasonCode": reason_code, "waitSeq": wait_seq}),
    };
    Ok(value)
}
```

- [ ] **Step 7: Implement `clear.rs`**

```rust
const COMMAND: &str = "execution.clear";

pub(crate) enum Verifier { MachineReplay(WireHash) }

pub(crate) fn verifier(kind: &str, manifest_hash: Option<&str>, evidence: Option<&[ClaimEvidence]>) -> Result<Verifier, Failure> {
    match kind {
        "machine_replay" => {}
        "countersign" => return Err(execution_state("countersign clearance is not available: the wire carries no signature to verify (#529, D-047); use --verifier machine_replay", "/verifier")),
        _ => return Err(argument("--verifier must be machine_replay", "/verifier")),
    }
    match (manifest_hash, evidence) {
        (Some(hash), None) => WireHash::parse(hash).map(Verifier::MachineReplay).map_err(|_| argument("--manifest-hash must be sha256:<64 hex>", "/manifestHash")),
        (None, Some(bundle)) => Ok(Verifier::MachineReplay(graphhelm_events::claim_evidence_digest(bundle))),
        _ => Err(argument("give exactly one of --manifest-hash and --evidence", "/manifestHash")),
    }
}

pub fn run(file, events, fixtures, execution, claim_seq, manifest_hash, evidence, verifier_kind) -> Outcome {
    /* load + lint + publish_loaded as in claim::run; evidence = evidence.map(load_claim_evidence).transpose()?;
       let verifier = verifier(verifier_kind, manifest_hash, evidence.as_deref())?;   -- NOTE: the countersign refusal must run BEFORE any store access
       finish(COMMAND, execute(&version, events, fixtures, execution, claim_seq, &verifier, owner_actor(), idempotency_key("completion-clear")), |v| v).with_warnings(warnings) */
}

pub(crate) fn decide(version: &GraphVersion, events: &Path, fixtures: Option<&Path>, execution: Option<&str>, claim_seq: u64, verifier: &Verifier, actor: PersistedActor, key: OpaqueId) -> Result<(ClearanceOutcome, PreparedDrive), Failure> {
    let store = event_store(events).map_err(|e| repository_failure(&e))?;
    let fixtures = load_fixtures(fixtures)?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let initial = graphhelm_events::replay(&scope, &stream, &history).map_err(|e| replay_failure(&e))?;
    verify_graph_matches_execution(version, &initial, &history, "clear")?;
    let Verifier::MachineReplay(manifest_hash) = verifier;
    let (outcome, _appended) = graphhelm_events::clear(&store, &scope, &stream, &actor, &key, claim_seq, manifest_hash).map_err(|error| match error {
        ClearError::NotStarted => execution_state("no execution has started on this stream", "/execution"),
        ClearError::NotAnOpenClaim { claim_seq } => execution_state(&format!("sequence {claim_seq} is not an open claim on this execution"), "/claimSeq"),
        ClearError::Repository(e) => repository_failure(&e),
    })?;
    let stream_id = OpaqueId::parse(&stream).map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let execution_id = OpaqueId::parse(initial.execution_id.as_deref().unwrap_or_default()).map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;
    Ok((outcome, PreparedDrive { scope, stream: stream_id, execution_id, spec: version.graph().spec.clone(), fixtures, release: BTreeSet::new() }))
}

pub(crate) fn annotate(value: &mut serde_json::Value, outcome: &ClearanceOutcome, claim_seq: u64) {
    value["clearance"] = match outcome {
        ClearanceOutcome::Cleared => serde_json::json!({"outcome": "cleared", "claimSeq": claim_seq, "reasonCode": null}),
        ClearanceOutcome::Refused { reason_code } => serde_json::json!({"outcome": "rejected", "claimSeq": claim_seq, "reasonCode": reason_code.as_str()}),
    };
}

pub(crate) fn execute(version, events, fixtures, execution, claim_seq, verifier, actor, key) -> Result<serde_json::Value, Failure> {
    let (outcome, prepared) = decide(version, events, fixtures, execution, claim_seq, verifier, actor, key)?;
    let store = event_store(events).map_err(|e| repository_failure(&e))?;
    let projection = if outcome == ClearanceOutcome::Cleared {
        // THE RELEASE IS THE FOLD'S; the drive only lets the now-satisfied dependents run, exactly as `resume` does.
        drive_to_quiescence(&store, &prepared.scope, prepared.stream.as_str(), &prepared.spec, &FixtureExecutor::new(prepared.fixtures.clone()), &system_actor(), &Release { nodes: &prepared.release, actor: &owner_actor() })?
    } else {
        replay_projection(&store, &prepared.scope, prepared.stream.as_str())?
    };
    let mut value = render(&projection, &AttentionInputs::default(), &Liveness::from_store(&store, &prepared.scope, prepared.stream.as_str()));
    annotate(&mut value, &outcome, claim_seq);
    Ok(value)
}
```

Note the `(ClearanceOutcome == ClearanceOutcome::Cleared)` comparison needs `PartialEq` (it derives it).

- [ ] **Step 8: Run** `cargo +1.97.1 test -p graphhelm-cli --test customs_cli --locked` → all PASS; then `--test execution_cli`, `--test cli_smoke`, `--test sweep_cli` still green. Also run `cargo +1.97.1 run --locked -p graphhelm-cli -- graph lint examples/graphs/customs-acting.yaml` and confirm no `GHG102`.

- [ ] **Step 9: Commit** — `git add` the eight files by name; message `feat(cli): execution claim and clear finish a parked node through customs, and a clearance that clears drives` / `Refs #159, #132`.

---

### Task 4: HTTP routes, MCP tools, and the parity cells

**Files:**
- Modify: `apps/cli/src/commands/serve/routes.rs` (two handlers + two `const *_COMMAND`)
- Modify: `apps/cli/src/commands/serve/mod.rs:431` (two `.route(...)` lines after sweep)
- Modify: `apps/cli/src/commands/mcp/tools.rs` (two `ToolSpec`s after `sweep`, two schemas, two match arms)
- Modify: `apps/cli/tests/mcp_stdio.rs:541-566` (`MCP_TOOL_NAMES: [&str; 26]`, append `"claim"`, `"clear"`; rename the test at `:569` to `tools_list_names_exactly_the_registered_tools_with_closed_schemas`)
- Modify: `apps/cli/tests/development_surface_parity.rs:148` (`NON_DEVELOPMENT_TOOLS: [&str; 20]`, append both, with a `// #159` comment like sweep's)
- Test: `apps/cli/tests/api_http.rs` (three tests appended)

**Interfaces:**
- Consumes: Task 3 `claim::execute`, `claim::attestation`, `clear::{decide, execute, annotate, verifier}`, `parse_claim_evidence`; existing `parse_mutation_headers`, `run_idempotent_mutation`, `graph_source`, `load_and_publish`, `drive_is_viable_for`, `prepare_drive`, `drive`, `bad_request`, `MutationError`.

- [ ] **Step 1: Write the failing HTTP tests** in `apps/cli/tests/api_http.rs` (reuse its `serve`, `post_json`, `head_sequence`, `last_event_of_kind`, `cli_start`-style helpers; add `cli_start_customs` that starts `examples/graphs/customs-acting.yaml` with `{"implementation":"unknown","release_notes":"success"}` and a `customs_evidence()` JSON value):

```rust
#[test]
fn the_acting_chain_over_http_appends_the_same_journal_the_cli_does() {
    /* start via CLI; serve; POST claim with evidence [] → 200, data.claim.outcome == "refused", reasonCode == "evidence_budget_unmet";
       POST claim with [test_report] → claimed, claimSeq; POST clear with manifestHash zeros → rejected/hash_mismatch;
       POST claim again; POST clear with "evidence": [test_report bundle] → cleared, data.status == "completed";
       then read events via GET /v1/executions/{id}/events and assert the completion_* kinds sequence equals
       ["completion_refused","completion_claimed","completion_cleared","completion_claimed","completion_cleared"]
       — the same list customs_cli.rs asserts, cited by name in a comment. */
}

#[test]
fn a_claim_retry_appends_nothing_and_a_stale_wait_is_refused_over_http() {
    /* same Idempotency-Key twice → second 200 with unchanged head; then waitSeq: 1 → refused unknown_wait, node waiting_input */
}

#[test]
fn the_scan_history_is_byte_identical_on_the_cli_and_the_api() {
    /* after a claim: cli = cli_envelope(["execution","status","--events",…,"--execution",…])["data"]["customs"];
       api = GET /v1/executions/{id} ["data"]["customs"]; assert serde_json::to_vec(cli) == serde_json::to_vec(api);
       and assert cli["quarantinedNodes"] == ["implementation"] so an empty view cannot pass as parity */
}
```

- [ ] **Step 2: Run to verify failure** — `cargo +1.97.1 test -p graphhelm-cli --test api_http acting --locked` → 404 on the route.

- [ ] **Step 3: Implement the handlers** in `routes.rs` (after `sweep`):

```rust
const CLAIM_COMMAND: &str = "execution.claim";
const CLEAR_COMMAND: &str = "execution.clear";

/// `POST /v1/executions/{id}/claim`: testimony that a parked node's external work is done.
pub(super) async fn claim(State(state): State<ServeState>, UrlPath(execution_id): UrlPath<String>, headers: HeaderMap, body: Bytes) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) { Ok(v) => v, Err(_) => return bad_request(CLAIM_COMMAND, "the request body is not valid JSON", "/") };
    let identity = match parse_mutation_headers(&headers, CLAIM_COMMAND, &execution_id, &payload, &["claim"]) { Ok(i) => i, Err(r) => return r };
    let source = match graph_source(&payload, CLAIM_COMMAND) { Ok(s) => s, Err(r) => return r };
    let Some(node) = payload.get("node").and_then(serde_json::Value::as_str).map(str::to_owned) else { return bad_request(CLAIM_COMMAND, "the request body must carry \"node\"", "/node") };
    let wait_seq = match payload.get("waitSeq") { None | Some(serde_json::Value::Null) => None, Some(v) => match v.as_u64() { Some(n) => Some(n), None => return bad_request(CLAIM_COMMAND, "\"waitSeq\" must be a non-negative integer", "/waitSeq") } };
    let evidence = match payload.get("evidence") { Some(v) => match execution::parse_claim_evidence(v) { Ok(e) => e, Err(f) => return f.into_outcome(CLAIM_COMMAND) /* respond as the other handlers turn a Failure into a response */ }, None => Vec::new() };
    let asserter = payload.get("asserter").and_then(serde_json::Value::as_str).map(str::to_owned);
    let mode = payload.get("mode").and_then(serde_json::Value::as_str).unwrap_or("operator_attested").to_owned();
    let events = state.events.clone();
    let drive_execution_id = execution_id.clone();
    run_idempotent_mutation(&state.events, &execution_id, CLAIM_COMMAND, identity, ExecutorWiring::from_state(&state), |actor, key| Box::pin(async move {
        let version = load_and_publish(&source, CLAIM_COMMAND).map_err(MutationError::Prepared)?;
        let attestation = execution::claim::attestation(asserter.as_deref(), &mode, &actor)?;
        Ok(execution::claim::execute(&version, &events, Some(drive_execution_id.as_str()), &node, wait_seq, evidence, attestation, actor, key)?)
    })).await
}

/// `POST /v1/executions/{id}/clear`: machine-replay countersignature; a clearance that clears drives.
pub(super) async fn clear(...) -> Response {
    /* parse: claimSeq (u64, required), manifestHash (string) XOR evidence (array), fixtures, route, verifier (default "machine_replay");
       build `execution::clear::verifier(...)` BEFORE touching the store (countersign → 4xx via the Failure's outcome, nothing appended);
       run_idempotent_mutation(.., &["clear"], ..) with the resume-shaped body:
         let version = load_and_publish(&source, CLEAR_COMMAND)?;
         if drive_is_viable_for(&drive_state, &version.graph().spec) {
             let setup = prepare_drive(&drive_state, CLEAR_COMMAND, &payload).await?;
             let (outcome, prepared) = execution::clear::decide(&version, &drive_state.events, fixtures.as_deref(), Some(id), claim_seq, &verifier, actor, key)?;
             let mut value = if outcome == ClearanceOutcome::Cleared { drive(&drive_state, &id, prepared, setup).await? } else { /* status render without driving: execution::status::execute(&events, Some(id))? */ };
             execution::clear::annotate(&mut value, &outcome, claim_seq); Ok(value)
         } else {
             Ok(execution::clear::execute(&version, &drive_state.events, fixtures.as_deref(), Some(id), claim_seq, &verifier, actor, key)?)
         } */
}
```

Look at how `approve` turns a `Failure` from `execute` into a response inside the closure (`?` with a `From<Failure> for MutationError`) and use the same. Register both routes in `serve/mod.rs` right after the sweep line:

```rust
        .route("/v1/executions/{id}/claim", post(routes::claim))
        .route("/v1/executions/{id}/clear", post(routes::clear))
```

- [ ] **Step 4: MCP tools** in `tools.rs` — append after the `sweep` `ToolSpec`:

```rust
    ToolSpec {
        name: "claim",
        description: "Claim that a waiting_input node's external work is done, presenting evidence (POST \
                      /v1/executions/{executionId}/claim). Testimony only: nothing is released until \
                      \"clear\" countersigns. A claim the pipeline cannot accept is journaled as \
                      \"completion_refused\" with its registry code. Requires the graph the execution \
                      started from (\"file\" or inline \"graph\").",
        schema: claim_schema,
    },
    ToolSpec {
        name: "clear",
        description: "Countersign a claim by machine replay (POST /v1/executions/{executionId}/clear): \
                      present \"manifestHash\" or the \"evidence\" bundle whose digest is computed here. \
                      A match clears the node and drives its dependents; a mismatch is journaled as a \
                      rejection and drives nothing. Requires the graph (\"file\" or \"graph\").",
        schema: clear_schema,
    },
```

Schemas (closed, via `mutating_schema`): `claim_schema` properties `executionId, file, graph(object), node, waitSeq(integer), evidence(array), asserter, mode` required `["executionId","node"]`; `clear_schema` properties `executionId, file, graph, fixtures, route, claimSeq(integer), manifestHash, evidence(array), verifier` required `["executionId","claimSeq"]`. Match arms mirror `resume`'s (conditional body fields; `evidence` copied whole via an `array_arg` helper if none exists — add one beside `object_arg`).

- [ ] **Step 5: Update the two pinned lists** (`mcp_stdio.rs`, `development_surface_parity.rs`) and run:
`cargo +1.97.1 test -p graphhelm-cli --test mcp_stdio --locked`, `--test development_surface_parity`, `--test surface_completeness`, `--test mcp_capability`, `--test api_http`, `--test customs_cli`. All green.

- [ ] **Step 6: Commit** — message `feat(runtime-api): claim and clear reach HTTP and MCP, and the scan history is byte-identical on both doors` / `Refs #159, #163`.

---

### Task 5: Records — CHANGELOG, acceptance record, lane-loop clause, milestone note

**Files:**
- Modify: `CHANGELOG.md` (new top entry)
- Create: `docs/acceptance/m11-acting-2026-09-11.md`
- Modify: `.factory/lane-loop.md` (one clause in §0)
- Create: `docs/milestones/acting-half.md`
- Modify: `docs/product/ROADMAP_AND_ACCEPTANCE.md` §3.2.1 table row 3 is NOT this slice — leave it; instead add one sentence under the table: "The acting half (#159) landed 2026-09-11; see `docs/milestones/acting-half.md`."

- [ ] **Step 1: Run the CLI journey once by hand and capture the journal** — from `D:/o-159`, with a temp dir, run the exact commands `customs_cli.rs` runs (`execution start`, `claim` ×3, `clear` ×2, `graph replay`), and paste the printed `completion_*` kind sequence and the `customsScans.implementation` stages into the acceptance record. Every number in the record must come from that run's output, never from the plan.

- [ ] **Step 2: Write `docs/acceptance/m11-acting-2026-09-11.md`** with: the sealed acceptance table from #159 with each cell → the test name that holds it (`customs_cli.rs::…`, `api_http.rs::…`, `customs_verbs.rs::…`) and the surface; the captured journal; the declared gaps from the spec §5, verbatim; the H-protocol reading ("informing 4/4, acting 4/4 on CLI and HTTP, N=1 real run + N tests"); the commands to reproduce.

- [ ] **Step 3: CHANGELOG entry** (top of file, English, same voice as the entries below it): heading `## The acting half: claim, clear, and the scan history, #159 - 2026-09-11`, bullets for: the two verbs and their decision table; the door refusal for countersign; the drive after clearance and why (`resume_preconditions` refuses a non-paused execution); the typed `CustomsView`; the file-trust seam reuse; the declared gap on deadlines (with the measured cause); the lane mapping (D8).

- [ ] **Step 4: `.factory/lane-loop.md` §0 clause** (append after the three-condition paragraph):

```
**Subagent lanes (orchestrator, 2026-09-11).** A lane may be an independent subagent with a fresh
context, spawned by an orchestrating session: the implementer subagents are the AUTHOR, two
reviewer subagents with no implementation context are the two PASSES, the registered runner is
the GATE, and the spawning session — which planned and wrote no code — PRESSES. The identity
line names the subagent and its spawning session so the comment is addressable:
`Lane: <letter> · Session: subagent-<name> of <ListAgents name> [ref] · Head: <sha8>`. The
spawning session may not review what its subagents wrote; it may press, because a planner who
wrote no line of the diff is a third lane under the rule above.
```

- [ ] **Step 5: `docs/milestones/acting-half.md`** — 30 lines: status line, what shipped (verbs, view, example graph), the acceptance record path, the declared gaps, what stays open (#153, #529, deadlines on the start path).

- [ ] **Step 6: Commit** — `docs(159): the acting half's records — acceptance run, changelog, lane mapping` / `Refs #159`.

---

### Task 6: Final check before the PR

- [ ] `cargo +1.97.1 fmt --all -- --check`
- [ ] `cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo +1.97.1 test -p graphhelm-events -p graphhelm-execution -p graphhelm-cli --locked`
- [ ] `git diff --check`
- [ ] `git log origin/main..HEAD --format=%B | grep -inE "(close|closes|closed|fix|fixes|fixed|resolve|resolves|resolved) #"` → must print nothing.
- [ ] Push: `git push -u origin issue-159-customs-acting-surface`. Report the head sha. Do not open the PR; the orchestrator does.
