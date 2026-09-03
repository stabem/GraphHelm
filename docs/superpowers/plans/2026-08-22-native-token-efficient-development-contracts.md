# Native Token-Efficient Development Contracts Implementation Plan

**Schema version:** `1.0`

**Parent issue:** [#216](https://github.com/stabem/GraphHelm/issues/216)

**Approved design:** [Native Token-Efficient Development Contracts](../specs/2026-08-22-native-token-efficient-development-contracts-design.md)

**Change class:** Feature

**Overall risk:** High

**Status:** Ready for issue-first execution after the dependency gate below

## Feature title

Native token-efficient development contracts

## Problem statement

GraphHelm has approved Journey-Proven Development (JPD), an Extension model, Context Capsules,
deterministic policy, governed publication, and append-only evidence. It does not yet have one
native system that resolves task-specific code rules, retrieves the smallest sufficient context,
learns reusable project knowledge safely, renders concise owner responses, and proves that token
savings did not weaken evidence or quality.

Adopting Caveman, codebase-memory-mcp, or ai-memory as governing ecosystems would add competing
authority and known failure modes. GraphHelm instead needs brand-neutral provider ports and
data-only skills whose outputs are validated by deterministic Runtime services.

## Solution overview

Deliver a built-in Extension bundle backed by versioned artifacts and deterministic Runtime
services. The Runtime will resolve layered code rules, bind retrieval to repository and index
snapshots, compile evidence-preserving Context Capsules, admit and transition opt-in memory through
the Governor, validate owner-facing presentation bytes, and measure complete-session cost. Public
CLI, API, and MCP surfaces expose the same contracts. A paired benchmark and JPD sabotage journey
prove both the owner experience and the failure boundaries.

This is a dependency flow, not a fixed domain pack or an eleven-stage workflow. The Task Profiler
selects the smallest required capability set for the task and risk.

## Authority and dependency gate

Implementation may begin only after one of these equivalent authority outcomes is recorded:

1. [PR #215](https://github.com/stabem/GraphHelm/pull/215) is accepted, making D-041 and the JPD
   schemas/bundle current authority; or
2. the design and this plan are reconciled through an accepted ADR/RFC against the then-current
   Decision Register.

Do not copy PR #215 artifacts into a competing format. Preserve its closed `JourneyContract` and
`JourneyVerificationResult` schemas byte-for-byte and bind them through `ArtifactBinding`.
Preserve the checked-in closed `ContextCapsule` schema the same way.

Existing dependency issues are reused:

- [#211](https://github.com/stabem/GraphHelm/issues/211): generic deterministic JPD certification;
- [#212](https://github.com/stabem/GraphHelm/issues/212): atomic Extension lifecycle and host CLI
  discovery; and
- [#213](https://github.com/stabem/GraphHelm/issues/213): per-contribution MCP capability authority.

## Architecture constraints

- Deterministic Runtime code exclusively enforces schemas, authority, scope, freshness, budgets,
  redaction, transitions, evidence sufficiency, refusals, and final serialized output.
- Existing Extension kinds and manifests are reused. No provider-specific kind or second plugin
  format is introduced.
- Skills are data-only orchestration guidance. They cannot waive policy, publish memory, mutate a
  graph, certify completion, or translate a refusal into success.
- Only the Graph Governor may publish operational graph or memory mutations.
- Graph and memory results are candidate evidence. Source remains authoritative; negative claims
  require coverage, extraction-gap evidence, complete pagination, and bounded source fallback.
- Event history and immutable versions are never rewritten. Projection data is disposable and
  rebuildable.
- Studio and host adapters use only public Runtime API, CLI, or MCP contracts.
- No fixed software-development pack is added. Atomic contributions are composed per task.
- No secret, raw prompt, raw chat, broad tool result, credential, cookie, or unrelated-project
  content may cross admission, logging, evidence, fixture, export, or presentation boundaries.

## Strategic tasks

### Task 001 — Publish development artifact contracts and shared types

- **GitHub issue:** [#217](https://github.com/stabem/GraphHelm/issues/217)

- **Change class:** Feature
- **Security risk:** High
- **Wave:** 1
- **Estimated complexity:** Large
- **Dependencies:** PR #215 accepted or reconciled
- **Files in scope (strict):**
  - `core/protocols/src/development.rs`
  - `core/protocols/src/lib.rs`
  - `extensions/builtin/graphhelm-development-contracts/extension.json`
  - `extensions/builtin/graphhelm-development-contracts/schemas/*.schema.json`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/contracts/**/*.json`
  - `apps/cli/tests/development_contract_schemas.rs`
- **Description:** Define the common envelope, `ArtifactBinding`, the nine new artifact kinds,
  compatibility rules, canonical digests, bounded sizes, snapshot bindings, stable refusal codes,
  and Rust wire types. Register them as ordinary schema contributions in one existing-format
  Extension manifest. Existing JPD and Context Capsule documents remain external closed artifacts.
- **Acceptance criteria:**
  - Every new artifact validates before typed deserialization and rejects unknown major versions.
  - Compatible minor-version unknown fields are preserved but never interpreted as authority.
  - `ArtifactBinding` verifies scope, schema ID/version, producer, digest, and required snapshots.
  - Current JPD and Context Capsule positive fixtures remain byte-identical.
  - Invalid cardinality, scope, size, digest, version, and refusal-code fixtures fail with stable
    diagnostics.
  - Canonical serialization is deterministic across input key order and Windows/Linux paths.
- **Threat assessment:** Untrusted schemas or envelopes may trigger reference expansion, oversized
  allocation, digest confusion, scope substitution, or compatibility downgrade. Bound all work,
  keep references offline/package-local, validate before deserialization, and fail closed.
- **RED -> GREEN -> REFACTOR:** Add failing valid/invalid fixture tests first; implement the minimum
  registry/types; then deduplicate canonical envelope helpers without changing diagnostics.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-protocols --locked`
  - `cargo +1.97.1 test -p graphhelm-schema --locked`
  - `cargo +1.97.1 test -p graphhelm-cli --test development_contract_schemas --locked`
  - `cargo +1.97.1 run --locked -p graphhelm-cli -- extension validate extensions/builtin/graphhelm-development-contracts`
- **Rollback:** Revert the task commit before any immutable release publication. After publication,
  leave the release immutable and roll forward with a new compatible schema version.
- **Out of scope:** Rule resolution, provider calls, memory persistence, rendering, benchmark logic,
  Extension activation, and changes to JPD/Context Capsule wire shapes.

### Task 002 — Resolve layered code rules deterministically

- **GitHub issue:** [#218](https://github.com/stabem/GraphHelm/issues/218)

- **Change class:** Feature
- **Security risk:** High
- **Wave:** 2
- **Estimated complexity:** Large
- **Dependencies:** Task 001
- **Files in scope (strict):**
  - `core/policy/src/code_contract.rs`
  - `core/policy/src/lib.rs`
  - `core/policy/Cargo.toml`
  - `core/policy/tests/code_contract.rs`
  - `extensions/builtin/graphhelm-development-contracts/policies/code-rule-resolution.yaml`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/code-contract/**/*.json`
- **Description:** Implement `CodeContractResolver` under Runtime authority using D-027 inheritance,
  selector conjunction/dominance, closed compatibility operators, validity windows, recorded
  waivers, stable conflict diagnostics, and immutable `ResolvedCodeContract` output.
- **Acceptance criteria:**
  - Structural rules accumulate and cannot be waived.
  - Quality waivers require the complete authorized record and preserve accurate status.
  - Preferences resolve only by explicit task decision, scope depth, selector dominance, and
    declared priority; remaining conflict refuses independently of load order.
  - Every included, excluded, expired, shadowed, waived, denied, and conflicting rule is recorded.
  - Identical inputs, clock, scopes, and snapshots emit byte-identical contracts.
- **Threat assessment:** A malicious descendant may weaken mandatory rules, exploit selector order,
  forge a waiver, or create nondeterministic conflict outcomes. Closed operators and authority
  validation must reject these paths before compilation.
- **RED -> GREEN -> REFACTOR:** Observe focused failures for each operator, permutation, invalid
  waiver, and selector conflict; implement; then property-test ordering and canonical identity.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-policy --test code_contract --locked`
  - `cargo +1.97.1 test -p graphhelm-policy --all-features --locked`
- **Rollback:** Revert the task commit; no resolved contract is mutable or migrated in place.
- **Out of scope:** Retrieval, memory, owner output, Extension activation, and owner-policy UI.

### Task 003 — Compile snapshot-bound retrieval plans and verified source fallback

- **GitHub issue:** [#219](https://github.com/stabem/GraphHelm/issues/219)

- **Change class:** Feature
- **Security risk:** High
- **Wave:** 3
- **Estimated complexity:** Large
- **Dependencies:** Tasks 001 and 002; #213 before MCP-backed provider authority is enabled
- **Files in scope (strict):**
  - `core/runtime/src/retrieval.rs`
  - `core/runtime/src/ports.rs`
  - `core/runtime/src/lib.rs`
  - `core/runtime/Cargo.toml`
  - `core/runtime/tests/retrieval.rs`
  - `extensions/builtin/graphhelm-development-contracts/policies/retrieval-admission.yaml`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/retrieval/**/*.json`
- **Description:** Add brand-neutral `StructuralCodeIndex` and `SourceReader` ports, deterministic
  `RetrievalPlan` compilation, evidence tiers, snapshot/generation separation, coverage checks,
  pagination, typed fallbacks, bounded representations, and negative-claim refusal behavior.
- **Acceptance criteria:**
  - A graph zero never becomes an absence claim without the full negative-proof contract.
  - Partial, skipped, excluded, stale, unknown, or unresolved coverage forces bounded source
    fallback or `NEGATIVE_CLAIM_UNVERIFIED`.
  - Stale coordinates never slice live bytes; snapshot-owned bytes, reindex, or `INDEX_STALE` is
    required.
  - Same inputs and capability receipts produce the same canonical plan or typed refusal.
  - Result/page/byte/token bounds and zero-result costs are explicit.
- **Threat assessment:** Providers may lie about freshness/completeness, return path escapes, flood
  results, or smuggle authority through summaries. Treat all provider output as untrusted typed
  evidence, enforce Tool Broker leases, and validate repository-relative paths and limits.
- **RED -> GREEN -> REFACTOR:** First fail silent-edge, stale-coordinate, partial-range, pagination,
  path-escape, and over-budget fixtures; implement ports/compiler; then consolidate fallback logic.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-runtime --test retrieval --locked`
  - `cargo +1.97.1 test -p graphhelm-runtime --all-features --locked`
- **Rollback:** Revert the task commit and disable its unactivated contribution declarations.
- **Out of scope:** A native code index, provider SDK, network calls, durable memory, final output,
  and claims of complete repository coverage.

### Task 004 — Enforce opt-in durable memory and governed publication

- **GitHub issue:** [#220](https://github.com/stabem/GraphHelm/issues/220)

- **Change class:** Feature
- **Security risk:** Critical
- **Wave:** 4 (parallel with Task 005; file ownership is disjoint)
- **Estimated complexity:** Large
- **Dependencies:** Task 001; #213 before MCP-backed store access; existing Event/Evidence/Governor
  invariants
- **Files in scope (strict):**
  - `core/governor/src/memory.rs`
  - `core/governor/src/lib.rs`
  - `core/governor/Cargo.toml`
  - `core/governor/tests/memory.rs`
  - `core/events/src/memory.rs`
  - `core/events/src/lib.rs`
  - `core/events/tests/memory.rs`
  - `extensions/builtin/graphhelm-development-contracts/policies/memory-admission.yaml`
  - `extensions/builtin/graphhelm-development-contracts/policies/memory-transition.yaml`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/memory/**/*.json`
- **Description:** Implement pre-admission scanning before candidate/provider access, project opt-in,
  authority ranking, independent validation, contradiction/supersession, dependency freshness, the
  closed semantic/publication state matrix, provisional Evidence custody, atomic publication
  re-sealing, audited withdrawal/erasure, handoff, and the Governor-only mutation path.
- **Acceptance criteria:**
  - Disabled capture touches no candidate, provider, log, event, or persistent boundary.
  - Raw prompt/chat, secrets, broad tool output, unknown fields, provider loops, and cross-scope
    content fail before candidate construction or persistence.
  - Canonical authority outranks episodic lexical overlap; contradictions remain visible.
  - Every allowed state/transition pair succeeds atomically and every other tuple refuses without
    changing the predecessor.
  - Published memory binds newly sealed Evidence; provisional metadata is never mutated in place.
  - Expired/stale/withdrawn memory is excluded by default but remains auditable.
- **Threat assessment:** This path can leak secrets, cross tenants, launder provider output, forge
  authority, erase corrections, or create split-brain publication. Require allowlisted fields,
  independent validation, immutable Evidence identity, transactionality, idempotency, scope locks,
  and append-only events. Produce the task-local threat-model section before implementation.
- **RED -> GREEN -> REFACTOR:** Start with capture-before-admission, scope bleed, self-validation,
  recapture loop, reseal failure, crash boundary, stale dependency, and invalid-transition failures;
  implement atomic behavior; then simplify only behind unchanged event/fixture bytes.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-governor --test memory --locked`
  - `cargo +1.97.1 test -p graphhelm-events --test memory --locked`
  - `cargo +1.97.1 test -p graphhelm-governor -p graphhelm-events --all-features --locked`
- **Rollback:** Before publication, revert. After any immutable event/schema release, disable new
  writes and roll forward; never delete/rewrite journal entries or mutate Evidence metadata.
- **Out of scope:** General memory-provider implementation, vector database, raw session capture,
  automatic publication, Dreams consolidation, and cross-project sharing.

### Task 005 — Validate and render safe owner-facing responses

- **GitHub issue:** [#221](https://github.com/stabem/GraphHelm/issues/221)

- **Change class:** Feature
- **Security risk:** High
- **Wave:** 4 (parallel with Task 004; file ownership is disjoint)
- **Estimated complexity:** Medium
- **Dependencies:** Task 001
- **Files in scope (strict):**
  - `core/runtime/src/owner_output.rs`
  - `core/runtime/src/lib.rs`
  - `core/runtime/Cargo.toml`
  - `core/runtime/tests/owner_output.rs`
  - `extensions/builtin/graphhelm-development-contracts/policies/owner-output-policy.yaml`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/owner-output/**/*.json`
- **Description:** Implement `OwnerOutputValidator`, the closed `OwnerPresentation` AST, exact slot
  copying, optional closed style plans, safe expansion, deterministic fallback, and exclusive final
  byte serialization. The policy applies only to final owner-facing projection.
- **Acceptance criteria:**
  - No-decision results emit zero options/recommendations and say `Nothing now` when no action exists.
  - Real decisions contain exactly two truthful options and one validated recommendation.
  - Failure, refusal, uncertainty, evidence limits, material consequences, exact paths/commands, and
    rollback cannot be compressed away or rewritten as success.
  - Malicious/malformed style plans are discarded; safe built-in rendering succeeds deterministically.
  - Machine JSON, docs, code, commits, PR bodies, and internal agent artifacts are unchanged.
- **Threat assessment:** A stylist or agent may suppress refusal, replace values, leak secrets, or
  fabricate a safe alternative. The Runtime owns slot values and serialization, scans before output,
  and uses a closed fallback rather than LLM repair.
- **RED -> GREEN -> REFACTOR:** Fail cardinality, immutable-slot, unsafe-compression, secret, and
  result-laundering cases first; implement AST/validator/serializer; then reduce duplication.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-runtime --test owner_output --locked`
  - `cargo +1.97.1 test -p graphhelm-runtime --all-features --locked`
- **Rollback:** Revert the task commit; callers fall back to existing typed output, never an
  Extension-authored prose renderer.
- **Out of scope:** Localization, arbitrary templates, Studio design, editing internal artifacts,
  and model-authored formatting.

### Task 006 — Add Context Capsule utilization, delta, cache, and full-session accounting

- **GitHub issue:** [#222](https://github.com/stabem/GraphHelm/issues/222)

- **Change class:** Feature
- **Security risk:** High
- **Wave:** 5
- **Estimated complexity:** Large
- **Dependencies:** Tasks 002, 003, and 005
- **Files in scope (strict):**
  - `core/runtime/src/context_compiler.rs`
  - `core/runtime/src/context_accounting.rs`
  - `core/runtime/src/lib.rs`
  - `core/runtime/Cargo.toml`
  - `core/runtime/tests/context_compiler.rs`
  - `core/runtime/tests/context_accounting.rs`
  - `extensions/builtin/graphhelm-development-contracts/policies/context-utilization.yaml`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/context/**/*.json`
- **Description:** Implement deterministic Context Capsule compilation from artifact bindings,
  stable item IDs, explicit required/optional evidence, utilization citations, bounded lazy handles,
  auditable expansion, delta capsules, snapshot-keyed caching, and complete acquisition/output cost
  accounting. Required evidence may never be removed to satisfy a token target.
- **Acceptance criteria:**
  - Every relied-on result cites stable capsule item IDs; missing required citations refuse.
  - Cache identity includes all semantic inputs and cannot cross scope, permission, snapshot, or
    schema version.
  - Delta/expansion retains provenance and budgets; insufficient required context yields
    `CONTEXT_BUDGET_INSUFFICIENT` plus an expansion request.
  - Orientation, zero results, pages, retries, fallbacks, summaries, compiled input, output, and
    formatting are counted; cold/amortized index cost is reported separately.
  - Same inputs produce byte-identical capsules and accounting receipts.
- **Threat assessment:** Cache poisoning, scope bleed, hidden retrieval cost, citation spoofing, and
  token-target evidence deletion can create false cheap success. Bind every cache/citation to scope,
  digest, producer, snapshots, and policy; fail closed on missing cost data.
- **RED -> GREEN -> REFACTOR:** Fail cache-key omission, cross-scope hit, uncited evidence, deleted
  required item, hidden retry, and nondeterministic delta cases first; implement; then optimize.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-runtime --test context_compiler --locked`
  - `cargo +1.97.1 test -p graphhelm-runtime --test context_accounting --locked`
  - `cargo +1.97.1 test -p graphhelm-runtime --all-features --locked`
- **Rollback:** Disable cache reads, preserve immutable receipts, and revert code before release;
  after release, roll forward without rewriting recorded accounting.
- **Out of scope:** Native index construction, provider billing, hidden reasoning inspection, and
  token estimates presented as observed usage.

### Task 007 — Expose one public Runtime/CLI/MCP contract

- **GitHub issue:** [#223](https://github.com/stabem/GraphHelm/issues/223)

- **Change class:** Feature
- **Security risk:** Critical
- **Wave:** 6
- **Estimated complexity:** Large
- **Dependencies:** Tasks 002–006 and #213; #212 for canonical executable discovery/activation
- **Files in scope (strict):**
  - `core/gateway/src/development.rs`
  - `core/gateway/src/lib.rs`
  - `core/gateway/Cargo.toml`
  - `apps/cli/src/args.rs`
  - `apps/cli/src/commands/development.rs`
  - `apps/cli/src/commands/mod.rs`
  - `apps/cli/src/commands/mcp/tools.rs`
  - `apps/cli/src/main.rs`
  - `apps/cli/tests/development_cli.rs`
  - `apps/cli/tests/development_mcp.rs`
  - `apps/cli/tests/development_api.rs`
- **Description:** Expose schema-equivalent commands/endpoints/tools for contract resolution,
  context compilation, retrieval plan compilation, memory proposal/status transitions, owner
  presentation, and accounting. CLI, HTTP, and MCP remain adapters over the same Runtime
  services.

  **"context compilation" and "retrieval plan compilation" are TWO compilers, not one activity.**
  They were written here as one phrase, `retrieval/context compilation`, and that phrasing sent a
  reader grepping for "retrieval" straight past the surface that exists. `context_compiler`
  (budget fitting and capsule serialisation) is served by `development.compile-context`, shipped.
  `retrieval::compile_plan_composed_against` is a different compiler with a different vocabulary
  and has no adapter; it is tracked in #724. Measured in #722.
- **Acceptance criteria:**
  - Equivalent inputs produce equivalent typed results, diagnostics, and refusals on all surfaces.
  - Per-contribution MCP tokens bind package digest, contribution, actor, tools, effects, and expiry.
  - Advisory operations cannot publish; mutation requires Governor authority and idempotency.
  - JSON output is stable and redacted; errors expose no home paths, secrets, or backtraces.
  - Bounded input, cancellation, timeout, and concurrency tests pass offline.
- **Threat assessment:** Public surfaces can become a second authority path, confused deputy, replay
  vector, denial-of-service target, or data-leak oracle. Reuse Runtime decisions, require scoped
  credentials/leases, bound work, redact, and audit allowed/refused calls.
- **RED -> GREEN -> REFACTOR:** Add parity/refusal/security smoke failures first; route each adapter
  to the shared Runtime operation; then deduplicate presentation-only code.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-cli --test development_cli --locked`
  - `cargo +1.97.1 test -p graphhelm-cli --test development_mcp --locked`
  - `cargo +1.97.1 test -p graphhelm-cli --test development_api --locked`
  - `cargo +1.97.1 test -p graphhelm-gateway --all-features --locked`
- **Rollback:** Revoke/disable the new public operations, preserve audit events, then revert adapter
  code. Never leave a host-only or CLI-only mutation path behind.
- **Out of scope:** Studio UI, remote registry, provider SDKs, browser automation, and GitHub Actions.

### Task 008 — Ship the three data-only entry skills and native bundle wiring

- **GitHub issue:** [#224](https://github.com/stabem/GraphHelm/issues/224)

- **Change class:** Feature
- **Security risk:** Medium
- **Wave:** 2 (parallel with Task 002; file ownership is disjoint)
- **Estimated complexity:** Medium
- **Dependencies:** Task 001; PR #215 Extension conventions; #212 before activation
- **Files in scope (strict):**
  - `extensions/builtin/graphhelm-development-contracts/extension.json`
  - `extensions/builtin/graphhelm-development-contracts/README.md`
  - `extensions/builtin/graphhelm-development-contracts/skills/code-contract/SKILL.md`
  - `extensions/builtin/graphhelm-development-contracts/skills/context-retrieval/SKILL.md`
  - `extensions/builtin/graphhelm-development-contracts/skills/memory-curator/SKILL.md`
  - `extensions/builtin/graphhelm-development-contracts/.claude-plugin/plugin.json`
  - `extensions/builtin/graphhelm-development-contracts/.codex-plugin/plugin.json`
  - `extensions/builtin/graphhelm-development-contracts/.mcp.json`
  - `apps/cli/tests/development_plugin.rs`
- **Description:** Complete the existing-format Extension bundle with three discoverable skill
  entries and atomic policy/evaluator/provider declarations. Host views remain derived, deletable,
  and limited to public MCP/CLI contracts.
- **Acceptance criteria:**
  - All contributions use existing Extension enum values and declared permissions/effects.
  - Skills request typed Runtime operations and contain no enforcement/publication logic.
  - `memory-curator` can emit only advisory candidates/proposals.
  - Host manifests cross-match package identity/version/public server and grant no extra authority.
  - Package/resource digests and `extension://` references validate deterministically.
- **Threat assessment:** Skill prose or host manifests may smuggle authority, over-request tools, or
  create a second plugin format. Validate the one authoritative manifest, minimize permissions, and
  exercise hostile package fixtures.
- **RED -> GREEN -> REFACTOR:** First fail authority, undeclared tool, host mismatch, bad digest,
  self-publication, and private-import fixtures; add the smallest data-only bundle; then deduplicate
  prose without changing machine contracts.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-cli --test development_plugin --locked`
  - `cargo +1.97.1 run --locked -p graphhelm-cli -- extension validate extensions/builtin/graphhelm-development-contracts`
- **Rollback:** Use #212 atomic rollback to the prior known-good version; remove only derived host
  views and never user-authored artifacts.
- **Out of scope:** General installer implementation, hot reload, remote registry, self-promotion,
  provider binaries, and a fixed workflow.

### Task 009 — Build the paired token-efficiency benchmark and quality gate

- **GitHub issue:** [#225](https://github.com/stabem/GraphHelm/issues/225)

- **Change class:** Feature
- **Security risk:** Medium
- **Wave:** 7
- **Estimated complexity:** Large
- **Dependencies:** Tasks 003, 005, 006, and 007
- **Files in scope (strict):**
  - `tools/development-benchmark/Cargo.toml`
  - `tools/development-benchmark/src/**/*.rs`
  - `tools/development-benchmark/tests/**/*.rs`
  - `tools/development-benchmark/fixtures/**/*.json`
  - `Cargo.toml`
  - `extensions/builtin/graphhelm-development-contracts/evaluators/token-efficiency.yaml`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/benchmark/**/*.json`
  - `apps/cli/tests/development_benchmark.rs`
- **Description:** Add a reproducible paired runner and committed corpus comparing traditional and
  compiled retrieval under identical snapshots, objectives, permissions, model route/settings,
  clean state, order, seeds, clock, budgets, and acceptance contracts.
- **Acceptance criteria:**
  - Median compiled input context is at most 40% of baseline.
  - Required-evidence recall is 100%; deterministic outcomes are equal or stronger; no critical
    answer is downgraded; blind mean quality falls by no more than 0.02.
  - Median full-session tokens are no greater than baseline and are the headline ratio.
  - Zero results, retries, pages, fallbacks, outputs, cold index cost, amortized index cost, CPU,
    memory, disk, latency, and cache behavior remain visible.
  - Invalid or asymmetric runs refuse comparison instead of producing a passing metric.
- **Threat assessment:** Benchmark gaming can omit hard cases, shift work outside the count, leak
  fixture answers, exploit cache asymmetry, or average away critical failures. Freeze the manifest
  before runs, bind artifacts/digests, use oracle IDs, and make any critical miss a hard failure.
- **RED -> GREEN -> REFACTOR:** Begin with intentionally gamed/incomplete manifests and failing
  thresholds; implement validation/accounting; then optimize compiled retrieval without weakening
  the oracle.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-development-benchmark --locked`
  - `cargo +1.97.1 test -p graphhelm-cli --test development_benchmark --locked`
  - `cargo +1.97.1 run --locked -p graphhelm-development-benchmark -- --manifest extensions/builtin/graphhelm-development-contracts/fixtures/benchmark/manifest.json`
- **Rollback:** Revert runner changes before release; published benchmark results remain immutable
  and a correction is a new manifest/run, never an overwritten result.
- **Out of scope:** Provider pricing claims, online models in repository tests, hidden-chain-of-thought
  inspection, hosted telemetry, and answer-only token success.

### Task 010 — Certify the owner journey with JPD and sabotage cases

- **GitHub issue:** [#226](https://github.com/stabem/GraphHelm/issues/226)

- **Change class:** Feature
- **Security risk:** Critical
- **Wave:** 8
- **Estimated complexity:** Large
- **Dependencies:** Tasks 001–009 and #211; #212/#213 for activated end-to-end host paths
- **Files in scope (strict):**
  - `extensions/builtin/graphhelm-development-contracts/graphs/development-contracts-self-validation.yaml`
  - `extensions/builtin/graphhelm-development-contracts/graphs/development-contracts-self-validation.fixtures.json`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/journeys/**/*.json`
  - `extensions/builtin/graphhelm-development-contracts/fixtures/sabotage/**/*.json`
  - `apps/cli/tests/development_journey.rs`
  - `apps/cli/tests/development_sabotage.rs`
  - `docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md`
- **Description:** Run the complete owner journey from versioned Journey Contract through code
  contract, retrieval plan, Context Capsule, implementation evidence, Journey Verification Result,
  optional memory branch, and owner response. Add the full named sabotage corpus and execute it
  through #211's generic deterministic certification gate.
- **Acceptance criteria:**
  - Capture-disabled and capture-enabled branches prove their distinct boundaries.
  - Every sabotage named in the approved design makes a named test fail before the corresponding
    behavior is accepted.
  - Semantic user actions and visible loading/error/recovery/success states are covered where the
    target journey exposes them; browser proof uses roles/labels rather than coordinates unless
    geometry is the behavior.
  - The journey refuses missing observers, false structural absence, stale snapshots, unsafe
    compression, secret capture, self-validation, scope bleed, and evidence deletion.
  - Replay preserves attempts, refusals, waivers, memory transitions, and final accurate status.
  - The generic gate certifies typed evidence; agent consensus remains advisory.
- **Threat assessment:** A happy-path-only harness may certify false success, while sabotage fixtures
  may contain secrets or executable instructions. Treat fixtures as untrusted data, use synthetic
  secret markers, require independent observers where specified, and keep all tests offline.
- **RED -> GREEN -> REFACTOR:** Pre-register each sabotage expectation, run it red against the
  unsabotaged or deliberately mutated boundary, implement the missing protection, run green, and
  only then consolidate reusable journey helpers.
- **Validation:**
  - `cargo +1.97.1 test -p graphhelm-cli --test development_journey --locked`
  - `cargo +1.97.1 test -p graphhelm-cli --test development_sabotage --locked`
  - `cargo +1.97.1 run --locked -p graphhelm-cli -- graph validate extensions/builtin/graphhelm-development-contracts/graphs/development-contracts-self-validation.yaml`
  - `cargo +1.97.1 run --locked -p graphhelm-cli -- graph simulate extensions/builtin/graphhelm-development-contracts/graphs/development-contracts-self-validation.yaml --events target/development-contracts-events.jsonl`
  - `cargo +1.97.1 run --locked -p graphhelm-cli -- graph replay --events target/development-contracts-events.jsonl`
- **Rollback:** Disable the unactivated dogfood graph and revert code/fixtures. Preserve immutable
  certification events; correct them with a new run and lineage rather than rewriting history.
- **Out of scope:** General browser/OS automation, production deployment, hosted evaluators,
  automatic skill promotion, and replacing project-specific journeys with a fixed pack.

## Dependency graph

```text
PR #215 accepted or authority reconciliation
  |
  v
Task 001 contracts/types
  |------------------------------|
  v                              v
Task 002 rule resolver       Task 008 data-only bundle
  |
  v
Task 003 retrieval + source fallback ---- #213
  |
  |-----> Task 004 governed memory ------- #213
  |-----> Task 005 owner output
  |                 |
  |-----------------+-----> Task 006 Context/accounting
                              |
                              v
Task 004 + Task 006 + Task 005 -> Task 007 public surfaces ---- #212/#213
                                      |
                                      v
                                 Task 009 benchmark
                                      |
                                      v
Task 008 + Task 009 + all runtime tasks + #211 -> Task 010 JPD certification
```

## Parallel waves and ownership

| Wave | Work | Parallel safety |
|---|---|---|
| 0 | Accept PR #215 or reconcile authority; progress #211–#213 | External dependencies only |
| 1 | Task 001 | Establishes contracts before behavior |
| 2 | Tasks 002 and 008 | `core/policy/**` versus bundle skills/host metadata; manifest overlap with Task 001 is sequential |
| 3 | Task 003 | Owns Runtime retrieval seam |
| 4 | Tasks 004 and 005 | `core/governor`/`core/events`/memory fixtures versus `core/runtime`/output fixtures |
| 5 | Task 006 | Owns Context Compiler/accounting after retrieval/output contracts |
| 6 | Task 007 | Central public-surface integration after service contracts |
| 7 | Task 009 | Benchmark tool and evaluator corpus |
| 8 | Task 010 | Final dogfood/certification graph and sabotage corpus |

Within a wave, agents must receive the exact file ownership above and must not edit another task's
files. A necessary out-of-scope edit stops that issue until its scope is explicitly amended.

## Owner-facing acceptance journey

1. The owner gives a real development request.
2. JPD binds or creates the complete user journey and observable promises.
3. `code-contract` requests a deterministic `ResolvedCodeContract` for the exact task/snapshot.
4. `context-retrieval` requests the smallest adequate, evidence-tiered `RetrievalPlan` and a
   `ContextCapsule`; material graph findings reach source, and absence claims prove coverage.
5. The selected harness implements and verifies behavior, including visible loading, failure,
   recovery, and success states when applicable.
6. The JPD verifier returns an accurate typed result with evidence and retry lineage.
7. If project memory is disabled, no capture boundary is touched. If enabled, `memory-curator`
   creates only an advisory candidate and the Governor controls any publication.
8. The Runtime validates and renders the owner response: summary, actual result, action, and either
   zero options or exactly two options plus one recommendation when a real decision exists.
9. The paired evaluation proves token reduction without missing evidence, false completion,
   security regression, or material quality loss.

## Repository-wide validation gate

Every behavior-changing issue runs its focused commands during RED -> GREEN -> REFACTOR. Before
the parent can close, run from a clean worktree:

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
./ci/gate.ps1
```

`./ci/gate.ps1` is authoritative. GitHub Actions must remain disabled and must never be used as a
fallback. `-SkipPostgres` is only a partial diagnostic run and cannot close the parent.

Known unrelated branch limitation: an earlier full gate run became red in
`tools/pathogens/tests/subject_refusals.rs` because the gate nonce was newer than
`target/debug/graphhelm.exe`; rebuilding `graphhelm-cli` made that focused test pass. Do not change
that harness in this feature. Record exact evidence if it recurs, rebuild the intended CLI, rerun the
focused test, and still obtain an authoritative clean full gate before completion.

## Global rollback

- Before immutable schema/event publication or activation, revert the relevant task commits in
  reverse dependency order.
- Activated Extension versions roll back atomically through #212 to the prior known-good digest.
- Published schemas, events, evidence identities, benchmark results, and Graph Versions are never
  edited or deleted in place. Corrections are new versioned artifacts and roll-forward events.
- Disable new public operations and memory writes before rolling back dependent Runtime behavior.
- Rebuild projections from the append-only journal after any roll-forward recovery.

## Exact feature out of scope

- Adopting, forking, distributing, or depending on Caveman, codebase-memory-mcp, or ai-memory.
- A second Extension/plugin/skill manifest format.
- Fixed domain packs or a mandatory eleven-stage development workflow.
- A native structural-index implementation or provider-specific memory/model adapter.
- General Extension installer, hot reload, remote registry, marketplace, billing, or telemetry.
- General browser/operating-system automation or Studio UI.
- Automatic paid provider fallback, automatic publication, self-promotion, or agent-created waiver.
- Raw prompt/chat/tool-output capture or cross-project memory pooling.
- Rewriting JPD, Context Capsule, Event Store, Evidence, or Graph Version history.
- Enabling or using GitHub Actions for CI or deployment.
- Production deployment as part of these implementation issues.

## Completion conditions

The parent issue remains open until every child issue and dependency is complete, the owner journey
and sabotage corpus pass, the paired benchmark meets all evidence/quality/cost thresholds, the full
local gate is green from a clean state, the full branch diff is independently reviewed, and rollback
evidence is recorded.
