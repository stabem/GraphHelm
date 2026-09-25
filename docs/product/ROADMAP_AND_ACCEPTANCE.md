# Roadmap, acceptance criteria and metrics

## 1. Delivery principle

The architecture is general-purpose from the start, but future implementation must evolve through provable vertical slices. The goal is not to produce a visual editor without a real engine, nor a powerful engine without a control experience.

This roadmap began as a pre-implementation plan. It remains an acceptance target, not a live
progress tracker. See [README.md](../../README.md) and the
[implemented milestones](../INDEX.md) for what currently runs.

## 2. Phase 0 — specification

**Outcome:** full documentation, initial schemas, Graph DSL, threat model, decision register and examples.

Criteria:

- decisions do not contradict each other;
- every module has boundaries;
- no fixed workflow per domain is required;
- user override is defined;
- open-source/licensing strategy documented;
- source references verified;
- schemas/examples pass basic validation.

## 3. Phase 1 — developer-first vertical slice

### 3.1 Objective

Execute a software change end to end:

```text
prompt
→ task profile
→ dynamic harness
→ visual graph
→ context capsules
→ code edit
→ tests/review
→ docs update
→ audited result
```

### 3.2 Scope

- local Studio;
- SSH/Docker bootstrap;
- single-node Runtime;
- Workspace/Project/Subproject;
- Codex/Claude native adapters where officially supported;
- BYOK/OpenRouter adapter;
- complete Graph DSL subset for core nodes;
- Graph Engine;
- Graph Draft;
- Autopilot/Supervised/Manual;
- Project Agent Registry;
- Context Compiler;
- Event Store;
- initial Knowledge Graph;
- Living Docs;
- Tier 0/1;
- Tool Broker for repository/shell/tests;
- Policy Engine;
- quality gates;
- basic Dreams;
- export/replay;
- public API/CLI.

### 3.2.1 The MVP bar inside that scope (owner decision, 2026-08-24)

Section 3.2 lists twenty-two items and orders none of them, which is how a scope becomes a
wish: every item is in Phase 1, so no item is next. The owner's decision on PR #62, recorded
as issue #302, cuts an MVP bar through it — **four promises, and a release is the MVP when all
four hold**. That decision was taken on 2026-08-24 and this paragraph is the amendment #302
asked for as its first task; it arrives late, and the sixteen days are themselves the finding:
a decision that never reaches the document that governs the work does not govern it.

| # | Promise | What is measured | State at this PR baseline |
|---|---|---|---|
| 1 | **Installable by a stranger** | `init` first use, then `docs/install/GETTING_STARTED.md` performed top to bottom on a clean local machine, Studio included, by someone following only the written instructions (D-053; the Phase 1 scenario of §3.4 keeps its VPS install, FR-001) | **partial** (#1070, #1077): `graphhelm init` writes the token, the key and the Studio `.mcp.json` entry (`apps/cli/src/commands/init.rs`, held by `apps/cli/tests/init_cli.rs`); `docs/install/GETTING_STARTED.md` is the one page a stranger follows. No complete clean-machine run exists yet: the Ubuntu-container rehearsal (`docs/acceptance/install-rehearsal-2026-09-13.md`) ran the build, `init`, `serve` and a fixture execution but not the Studio, and the integrated re-run (`docs/acceptance/mvp-integrated-2026-09-14.md`) used a development machine with the toolchain already present. The complete run is #1094; the Studio findings of the re-run are #1083 |
| 2 | **Retrieval that scales** | the validated recipe — lexical, entity RRF, graph-neighbour RRF, optional vector, authority — carrying an execution's lifetime | **partial, and now measured** — since #1065 the lexical stage runs inside every execution: bounded search over the project root, a capsule in every plain cognitive node's prompt and its digest, `retrieval_pages`/`zero_result_queries`/`retrieval_fallbacks` measured and `compiled_input_tokens`/`eligible_candidate_tokens`/`tokens_saved` derived under `bytes-div-4/v1` in the sealed `context-provenance@1` record beside each reply (the receipt's own lines wait for the next frozen baseline); measured over a frozen corpus of fifteen files copied from the repository, hit rate@3 (success@3; with one relevant document per query precision@3 would max out at 1/3) = 9/10 = 0.90 (`adapters/tool-host/tests/context_quality.rs`, the floor the test asserts; the live tree is printed with no floor, 4/10 = 0.40 at `1ac2438e`). Entity RRF, graph-neighbour RRF, vectors and authority remain open under #302; the shipped `UnavailableStructuralCodeIndex` still returns `Unavailable` (`docs/context/CONTEXT_KNOWLEDGE_DREAMS.md` §1.1) |
| 3 | **Continuity across harnesses** | the event store consumed as a resume briefing: a second harness picks up an execution from the stream alone | **built** (#1063) — `execution briefing` / `GET /v1/executions/{id}/briefing` / MCP `briefing`, one `briefing_view` over the projection, the attention answer and the history; `start` records `name`, `objective` and `executor` on the declared form. Held by `a_second_harness_reads_the_same_briefing_the_first_one_left_in_the_store` (`mcp_stdio.rs`: harness A `claude-code` approves and pauses, harness B `codex` reads byte-for-byte what the CLI reads), `the_briefing_over_http_matches_the_cli_on_the_same_store` (`api_http.rs`), `the_briefing_reports_the_decisions_in_order_with_actors_and_names_resume_when_held` and `a_start_records_the_name_the_objective_and_the_executor_on_the_declared_form` (`execution_cli.rs`), and the `briefing::tests` cells in `core/execution` |
| 4 | **Provider-less mode as a declared guarantee** | the fixture route stated as a promise and held by a test, so the product runs with no credentials at all | **declared** in `docs/product/PROVIDER_LESS_MODE.md`, held by `apps/cli/tests/providerless_journey.rs` (#1064): a fixture run says so on every view, one documented flow, one test that runs the flow with no environment and reads the document back |

The acting half (#159) landed 2026-09-11; see `docs/milestones/acting-half.md`.
The first compile (#107) landed 2026-09-11 as `graph synthesize`; the Task Profiler and Capability Discovery remain unbuilt — see `docs/harness/GRAPH_ARCHITECT.md`.

**Order of work remains: 1, 3, 4, then #107.** Promise 2 is partial and remains open under #302:
the snapshot-bound retrieval guard is shipped in #219's slice, but the full validated recipe is not. Promises 1, 3 and 4 are what a stranger
meets first, and #107 — nothing in the tree turns a prompt into a graph — is the largest single
gap in the map and the first move §3.1 promises. It follows the three because a harness that
synthesises a graph nobody can install or resume is a demonstration, not a product.

**The rule this sets for everything else, and the measurement behind it.** On 2026-09-08 the
current wave held four issues (#901-#904) and all four were about the gate; in the same four
days the repository merged 116 pull requests. The capacity is real and it was pointed at the
factory. So: **new gate, runner or CI work is taken only when a gate is actually broken** — a
red that blocks work, a runner that has stopped, a false green. Everything else queues behind
the promises above. Gate work that improves throughput without unblocking anything is exactly
the work this rule defers.

**Debt gets a date, not a permanent exception.** A quality bar that can only be met or blocked
turns every honest gap into a stalled pull request, and the fleet has paid for that repeatedly.
The mechanism to adopt is a ratchet: violations frozen by key with a `review_by` date and a note,
in one file, so adding a key is a visible act in review rather than a silent widening. Until that
file exists, a declared gap in a pull request body is its stand-in, and the two carry the same
obligation — a gap with no date is a gap nobody will close.

### 3.3 Out of Phase 1

- multiuser UI;
- paid marketplace;
- multi-node scheduler;
- Tier 3 production-grade;
- all domains;
- hosted cloud;
- enterprise SSO;
- full visual plugin editor.

### 3.4 Acceptance scenario

1. user installs Runtime on a clean VPS;
2. connects Studio;
3. authenticates a model route;
4. imports a repository;
5. requests a feature;
6. system generates Graph v1;
7. execution maps, plans, changes, tests and reviews; (steps 5-7's "changes, tests" half
   is proven without a model credential in `docs/acceptance/useful-change-2026-09-13.md`:
   the change lands as `refs/graphhelm/executions/<id>` in the project, #1066)
8. user removes review and forces deploy to a test environment;
9. Graph Draft shows risks;
10. user confirms;
11. waiver is recorded;
12. execution pauses if a route hits its quota;
13. user resumes once capacity is available;
14. docs and claims are updated;
15. export reproduces the timeline;
16. the execution's context-efficiency figure (§9.2) and per-node token counts are visible in local observability alongside the recorded full-context estimate;
17. the user requests a second feature with a distinct objective signature on the same repository, and the system demonstrates measurably lower compiled-context cost through reuse — capsule compilation cache hits, provider prompt-cache hits, agent reuse — with the cited `ReuseDecision` events visible in the timeline (reuse across distinct work, not replay of identical work).

## 4. Phase 2 — generalization of capabilities

Add tools/capabilities for:

- web research;
- source verification;
- documents;
- data analysis;
- browser automation;
- product/PRD;
- marketing/copy;
- design/image;
- operations;
- external integrations.

Criterion: the core does not receive a code branch per domain; only extensions and schemas.

## 5. Phase 3 — ecosystem

- public extension registry;
- signing/trust;
- optional conformance cloud;
- package discovery;
- community agents/skills;
- alternate registries;
- documentation site;
- Graph Engineer tooling;
- benchmark packs.

## 6. Phase 4 — collaboration and enterprise

- multiuser;
- RBAC;
- approvals;
- comments;
- shared workspaces;
- SSO;
- audit export;
- policy administration;
- multi-VPS workers;
- HA stores;
- compliance controls;
- managed hosting operations.

## 7. Phase 5 — distributed agent operating system

- federated runtimes;
- edge/local GPU scheduling;
- organization-wide knowledge boundaries;
- graph exchange/market;
- optional hosted control plane;
- cross-workspace capability brokerage;
- advanced Dreams experiments;
- formal verification of policies/graphs where viable.

## 8. Global acceptance criteria

### 8.1 Harness

- structured task profile;
- customized graph, not a closed template;
- capability snapshot;
- deterministically applied policies;
- graph lint and simulation;
- minimal graph rationale;
- expansion limits.

### 8.2 Graph

- node/edge contracts;
- versioning;
- transactional draft;
- adaptive mutation;
- ghost nodes;
- pause/resume;
- user bypass;
- waiver;
- replay.

### 8.3 Context

- capsule per node;
- no full history by default;
- provenance;
- conflicts;
- expansion request;
- cache/invalidation;
- blind review.

### 8.4 Agents

- synthesize/reuse;
- Project Agent Registry;
- temporary overlays;
- memory with evidence/TTL;
- performance segmentation;
- no direct graph spawning.

### 8.5 Models

- multiple route types;
- official auth only;
- BYOK kept separate;
- credential isolation;
- capability routing;
- pause on quota;
- no automatic paid fallback.

### 8.6 Security

- Tier 0/1 minimum;
- no Docker socket;
- network policy;
- secret broker;
- tool leases;
- cross-project isolation;
- secret scanning;
- audit.

### 8.7 Knowledge/Dreams

- event immutability;
- claims with provenance;
- living docs diff;
- shadow dream;
- independent critic;
- rollback;
- code finding creates a normal task.

### 8.8 Open source

- public source;
- public APIs;
- self-host;
- opt-in telemetry;
- schemas/docs;
- MIT license and open governance;
- reproducible export.

## 9. North star metrics

### 9.1 Evidence-backed task success

Percentage of user-accepted executions that satisfy completion contracts with no known regression within the defined window.

### 9.2 Context efficiency

```text
1 - tokens_sent_with_compiler / estimated_tokens_full_context
```

Must not be optimized in isolation; track alongside quality.

The estimator for `estimated_tokens_full_context` is versioned and deterministic, and its
identity rules are fixed: the method id includes the retrieval-recipe version; a recipe change
starts a new series (no cross-series comparison); and the paired inline arm of §11.2 is the
calibration reference, with a declared error tolerance that flags — never blocks — per-execution
reporting. The v1 estimator is a free compiler byproduct: ranking already token-counts every
eligible candidate for its token-cost term, so the capsule manifest records
`eligible_candidate_tokens` and `tokens_saved = eligible − shipped` per node, stated explicitly
as a conservative lower bound on the true full-context baseline.

A future economy metric or mechanism proposal is admissible only if it names which measured
metric it will move and which producer it consumes from — unmeasurable-but-plausible does not
qualify.

### 9.3 Orchestration efficiency

- useful nodes / total nodes;
- evidence gain per node;
- coordination overhead;
- mutation count;
- time on critical path.

## 10. Secondary metrics

- time to first useful graph;
- time to first evidence;
- human intervention rate;
- manual override rate;
- user correction of Command Router;
- agent reuse precision;
- task cost;
- subscription wait time;
- schema repair rate;
- reviewer disagreement;
- gate catch rate;
- post-completion incident rate;
- docs freshness;
- memory validation rate;
- Dreams rollback rate;
- plugin conformance pass rate.

## 11. Benchmarks

### 11.1 Harness benchmark

Set of tasks of varying complexity. Evaluate:

- graph adequacy;
- required gates;
- redundant nodes;
- cost estimate;
- risk classification;
- context plan.

### 11.2 Context benchmark

- answer/task quality;
- relevant evidence recall;
- irrelevant token ratio;
- conflict detection;
- stale information avoidance;
- paired full-context arm: every benchmark task also runs as a single full-context inline agent, scored on tokens and quality together — the thesis's falsification arm (explicit runs only; D-016 stands, nothing fires automatically on live workloads);
- the slope: over the fixed corpus executed repeatedly, graded cost per task is non-increasing at paired-arm quality parity, with cited `ReuseDecision` event ids decomposing which reuse mechanisms produced each reduction.

### 11.3 Reviewer benchmark

- seeded defects;
- clean changes;
- false-positive;
- evidence citation;
- independence benefit.

### 11.4 Graph mutation benchmark

- unexpected dependency;
- auth boundary discovered;
- tool failure;
- quota exhaustion;
- user bypass;
- stale draft;
- no-progress loop.

### 11.5 Security benchmark

- prompt injection;
- secret file;
- postinstall exfiltration;
- symlink escape;
- malicious plugin;
- cross-project retrieval;
- Docker socket access;
- log leakage.

## 12. Quality bars for releases

### Alpha

- data loss and secret leak blockers;
- core flow works;
- APIs may change;
- explicit experimental warnings.

### Beta

- schema/API migration policy;
- conformance suite;
- backup/restore;
- security review;
- extension SDK;
- complete docs;
- capsule runs pass the paired-arm context benchmark (§11.2) at parity-or-better quality with materially fewer tokens.

### 1.0

- stable public API/DSL;
- self-host upgrade path;
- validated threat model;
- reliable replay/export;
- contributor governance;
- commercial/legal docs;
- compatibility tests;
- supported platforms.

## 13. Product execution risks

### 13.1 Systemic complexity

Mitigate with boundaries, vertical slice, contract-first and no fixed packs.

### 13.2 UI over an immature engine

Mitigate by developing every interaction against the real API and event stream.

### 13.3 Orchestrator overthinking

Mitigate with a minimal graph objective, budgets and deterministic gates.

### 13.4 Provider auth changes

Mitigate with adapters, official flows, health metadata, graceful disable and documented verification dates.

### 13.5 Token savings harming quality

Mitigate with expansion, evidence recall metrics and benchmarking.

### 13.6 Dreams corrupting knowledge

Mitigate with shadow, tests, critic, atomic commit and rollback.

### 13.7 Owner override causing an incident

Mitigate with clear impact, waiver, rollback tools and incident correlation, without removing sovereignty.

### 13.8 Open-source contribution friction

Mitigate with a short contributor guide, public governance, and clear value for contributors.

## 14. Documentation definition of done

This package is considered complete when:

- MASTER PRD exists;
- decision register contains all choices;
- screens and behaviors are specified;
- harness and Graph Engineer docs are thorough;
- DSL and schemas exist;
- context/knowledge/Dreams are defined;
- model gateway covers BYOK/subscription/local;
- security and threat model exist;
- operations/recovery exist;
- open-source/licensing/governance exist;
- examples demonstrate scenarios;
- official references and verification date exist;
- files pass local link, JSON and YAML validation.
