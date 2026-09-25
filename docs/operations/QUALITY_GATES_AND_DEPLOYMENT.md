# Quality gates and deployment

> **Status: target deployment architecture.** This document describes the intended verification
> graph and deployment path. It is not the current contribution or merge procedure. Today, use
> [the delivery process](../process/DELIVERY.md): run the checks the change reaches, obtain one
> independent review, and merge at the reviewed head. `ci/gate.ps1` is an optional full local
> check; a merge does not wait for it. The pre-push and VPS tiers below are design targets, not
> checks currently required of contributors.

## 1. Objective

The pipeline that makes "excellent code" a measured property and makes a future change unable to
silently break an existing behavior. It is local/VPS-first by decision, not by limitation:

- **GitHub is versioning only.** GitHub Actions is permanently disabled — a deliberate posture,
  never an incident to fix. No workflow is ever enabled, triggered, or used as a fallback.
- **The developer machine runs the fast tier** before every push (pre-push hook).
- **The VPS runs the full tier and the deploy** (D-001/D-002: runtime on the user's VPS,
  installed and updated via Docker). The connected agent operates the pipeline through the same
  public API/CLI every other action uses.
- **Endgame: the pipeline is a GraphHelm execution graph.** Each gate below maps to a §21
  gate-node (`HARNESS_SPEC.md`); the runtime that ships is the runtime that verifies and deploys
  itself. Until that graph exists, `ci/gate.ps1`-style stage scripts are the interim carrier.

## 2. Topology

```text
dev machine                      GitHub                    VPS (runtime + agent)
──────────────                   ──────                    ─────────────────────
fast tier (pre-push) ──push──▶ versioning ──pull──▶ full tier gate ──▶ deploy ──▶ post-deploy verify
                                                        │ red: stop, report, no deploy
                                                        └ rollback: redeploy pinned ref
```

- The VPS pulls a ref; nothing on GitHub executes.
- The full gate runs against the pulled ref **as merged** (rebase-then-gate: the gate always
  evaluates the combination that will actually ship, never a stale branch state — this is the
  structural answer to "one change lands and breaks another").
- Secrets live on the VPS (`.env` on the server is the source of truth); the pipeline never
  writes them, and `--dry-run` and `DEPLOY_REF=<sha>` (deploy/rollback to a pinned ref) are
  mandatory affordances of the deploy step.

## 3. The gates

Deterministic-first ordering is binding (§21 as amended): within and across gates, deterministic
methods run before model-based ones, and a deterministic failure stops the spend.

### Tier 1 — fast, deterministic, every push (target: under 10 minutes)

| # | Gate | Verifies | Carrier |
|---|---|---|---|
| 1 | Build + format + lint | compiles; `fmt` clean; workspace clippy `-D warnings` | cargo, existing |
| 2 | Unit and property tests | behavior + invariants (proptest) | cargo, existing |
| 3 | Source invariants | crate purity, exact dependency pins, dependency containment (every crate that declares a boundary carries the test — no ungated crate) | `source_invariants.rs` pattern, existing; coverage to be completed |
| 4 | Schema and baseline integrity | frozen `1.0.0` byte-identical, catalog digests, closed event set | existing suites |

### Tier 2 — structural quality, pre-merge

| # | Gate | Verifies | Carrier |
|---|---|---|---|
| 5 | Patch coverage | coverage of the **diff** meets the bar (global % is not a gate — it hides regressions) | `cargo-llvm-cov` + diff filter |
| 6 | Mutation testing (incremental) | injected bugs in changed code are killed by tests — the house's manual sabotage ritual, mechanized; a surviving mutant is a guard nobody proved | `cargo-mutants --in-diff` |
| 7 | Supply chain | advisories, license policy, **duplicate crate versions**, unexpected dependency-graph mass | `cargo-audit` + `cargo-deny` |
| 8 | Redaction and secret scan | sentinel discipline mechanized: no secret-shaped content in any test output, log, or error path; the redaction suites as a named stage | grep patterns + existing redaction tests |

### Tier 3 — system behavior, pre-merge full / milestone close

| # | Gate | Verifies | Carrier |
|---|---|---|---|
| 9 | **Business-rule E2E** | every `current` rule document is exercised by the E2E scenarios it names — see §4 | rule-doc `verified_by` + E2E runner |
| 10 | Replay determinism | double replay of every E2E-produced history is byte-identical | existing assertion, promoted to stage |
| 11 | Surface parity + contract snapshot | CLI ↔ API (↔ MCP once 05e lands) identical for the scripted story; public API envelope diff is explicit, never accidental | 05a parity test + snapshot |
| 12 | Concurrency storm + flake policy | the storm holds; three consecutive green runs for concurrency suites; flaky tests are **quarantined** (run and report, do not block) with a mandatory tracking issue — never deleted, never silenced | storm tests + quarantine list |
| 13 | PostgreSQL matrix | both locale passes | existing |
| 14 | Performance budget | measured throughput/latency against the recorded baseline within a declared tolerance (a calibrated claim: the baseline is versioned, drift raises a finding, prediction error is tracked) | baseline records (05a's ≈3 req/s exists for this) |
| 15 | Docs freshness + acceptance map | cross-references resolve; the milestone acceptance map covers the design's criteria; CHANGELOG touched when code is | link/ref checker + map check |

### Post-deploy verify (after every VPS deploy)

Health endpoint, one scripted-story smoke through the public API, replay check on the produced
history. A red post-deploy verify triggers rollback to the previous pinned ref.

## 4. Business-rule E2E traceability

The gate that ties Living Documentation to executable proof:

- every rule document (`CONTEXT_KNOWLEDGE_DREAMS.md` §12.5) with `status: current` carries a
  `verified_by:` list naming E2E scenario ids;
- the gate derives the traceability matrix **as a byproduct of the run**, never as a maintained
  artifact: a current rule with no passing named scenario **fails**; a scenario naming no rule is
  a warning (orphan); a rule whose source-claim digest changed while its scenarios did not is a
  **staleness failure** — the rule moved and its proof did not;
- the scenario set is therefore derived from the docs at gate time: documenting a new business
  rule *forces* its test into existence, and deleting a rule retires its scenarios explicitly.

Each scenario also declares the user promise and the observation used to prove it. A green lower-
level test is sufficient only when it observes that promise directly. Loading, error, recovery,
focus, navigation, delivery, and external-effect states require their own adequate observers when
the contract names them. Missing capability is reported as `OBSERVER_MISSING`; it is never converted
to a skipped green gate.

## 5. Dynamic gate selection

The gate set is the deterministic union of the mandatory change-profile gates and the compiled
journey's observation obligations, risks, and effects. The Task Profiler / Policy Engine applies
both inputs to the pipeline:

| Change profile | Gate set |
|---|---|
| docs-only | 1 (fmt/link surface), 15 |
| core crates, no schema | 1–8, 10–12, affected E2E |
| `core/schema/`, baselines, wire enums | everything, no skips |
| adapters | 1–8, full E2E (9), 11–14 as applicable |
| CLI/API surface | 1–8, 9 affected, 11 always |
| deploy configuration | 1, 7, 8, post-deploy verify rehearsal |

Selection and its inputs are recorded with the gate report. Journey obligations may add a proof
method or refuse an inadequate one; they never remove a gate required by paths or change profile.
Skipped gates are named in the report — a skip is a decision, never an absence. Escalation is
one-way: either input may add gates mid-run, never drop them.

## 6. Anti-regression discipline

- **Rebase-then-gate, serialized:** one full gate at a time per machine, on the merged state,
  before merge (the machine-wide gate rule already in practice, made normative).
- **Mutation + patch coverage** protect the tests themselves: a change that weakens a test
  surfaces as a surviving mutant or a coverage drop on the diff.
- **Rule E2E** protects documented behavior; **parity** protects surfaces; **replay** protects
  history; **budgets** protect performance — each future change must pass the proofs of every
  behavior it did not intend to change.
- **Quarantine is visible:** the quarantine list lives in the repo, each entry with its issue;
  the gate report counts quarantined tests so trust in the signal is measured (flake rate is a
  tracked metric, target well under 1%).

## 7. Cadence and cost

- Tier 1 on every push (pre-push hook), target under 10 minutes.
- Tiers 1–3 before every merge, on the merged state.
- Expensive full sweeps — full-tree mutation run, extended storm, full E2E matrix — at milestone
  close, announced before running (gates saturate the machine; never two at once).
- The pipeline measures itself: gate wall-clock, spend and flake rate are recorded per run under
  the measurement-overhead budget (`HARNESS_SPEC.md` §14.1), so the cost of quality is a number,
  not a feeling.

## 8. Acceptance criteria

- no GitHub workflow is enabled or executed anywhere in the pipeline;
- the full gate runs on the VPS from a pulled ref and blocks deploy when red;
- a current rule document without a passing named E2E scenario fails gate 9;
- a rule-document change without a scenario change fails as stale;
- an injected mutant in changed code that no test kills fails gate 6;
- a duplicate crate version or advisory fails gate 7 unless explicitly waived with a recorded
  reason;
- the gate report names every skipped gate and the profile that skipped it;
- post-deploy verify exists, runs, and a red one rolls back to the pinned previous ref;
- the quarantine list is in-repo, every entry carrying an open tracking issue;
- gate cost and flake rate are recorded per run.
