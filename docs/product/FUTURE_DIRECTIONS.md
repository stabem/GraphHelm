# Future directions

Status: approved as **candidate bets**, not scheduled work. These are the three survivors of
the first structured divergent-ideation run (issue #44: five isolated cognitive frames →
thirty ideas → scored, clustered, traps pruned → three deepened). Each entry names its
mechanism, its load-bearing risk, the cheap first step that de-risks it, and its admission
line — the measured metric it moves and the producer it consumes, the same gate
`ROADMAP_AND_ACCEPTANCE.md` §9.2 imposes on economy proposals. Promotion to a milestone is an
owner decision that starts from this document.

The anchor insight behind all three: **byte-identical replay is a perfect adjudicator.**
GraphHelm's replay guarantee is cryptographic-grade trust infrastructure currently used only
for debugging; these bets each spend it on something larger.

## 1. Divergence-from-baseline alarms

**What:** page on drift in the *shape* of an execution, not on errors. Every completed run's
log distills into an execution fingerprint (per-node tool-call n-grams, capsule sizes,
governor intervention counts, outcome-shape hashes) via a deterministic projection — so the
corpus of a graph's own prior replays is the baseline distribution, for free. A live run
streams through the same projection; crossing a divergence threshold raises a **shape-anomaly
event into the log itself**, which the Governor consumes through the existing mutation
machinery (pause the node, escalate to approval, ask a human). Baselines key on (graph hash,
node id, capsule schema version) so a deliberate edit resets the corpus instead of alarming;
cold-start below a minimum corpus stays silent.

**Why it matters:** the scariest agent incidents return 200 OK — prompt-injection detours,
tool-schema drift, model-update behavior shifts. Only a determinism-first system has the
known-good corpus to detect "weird but green", and no orchestrator ships this.

**Load-bearing risk:** false-positive collapse. Agent runs are legitimately high-variance;
if fingerprint features do not separate benign input variation from behavioral drift,
operators mute the alarm within a week and the feature dies as noise.

**Cheap first step:** a pure `fingerprint(event_log) -> RunFingerprint` projection over the
existing safe-replay machinery; extract fingerprints from ~50 historical runs of one real
graph and measure whether known-good runs actually cluster — before building any scorer.

**Admission line:** moves incident detection latency and adds a false-hit-rate metric;
consumes the safe-replay projection machinery (exists) and the event log's attribution
contract (exists, 05a).

**Descendants worth holding:** drift-gated deploys (canary analysis over behavior instead of
error rates); Governor auto-tightening (anomaly → approval-required mode for the rest of the
run); injection forensics by nearest-known-good diff; **fingerprint exchange** — shapes carry
no payloads, so deployments can share anonymized baselines: a community immune system where a
prompt-injection campaign detected on one install inoculates the others; cost drift unified
in the same mechanism.

## 2. The flight recorder and replay certificates

**What:** one signed, content-addressed export per incident — the run's event-log slice,
compiled capsules, governor decisions and tool transcripts — replayable byte-identically on a
maintainer's laptop with every external call stubbed from the recorded transcript. Credentials
are structurally absent (the log only ever held references; the register's hard constraint
does the work). Sensitive Evidence passes a per-blob redaction step — include, redact to its
hash (replay integrity still verifies), or rely on prior cryptographic erasure. A successful
replay emits a **signed replay certificate** (engine version, tarball hash, final-state hash)
attachable to the bug report or the fix's PR. In CI, an **adversarial determinism fuzzer**
permanently plays the attacker — perturbed schedulers, shuffled orderings, injected
clock/locale noise, truncated tool responses — failing the build on any byte divergence, so
the flagship claim is continuously re-earned rather than asserted.

**Why it matters:** support collapses to "send me the flight recorder", shareable without a
security review; and the determinism claim gains a standing institutional attacker instead of
waiting for a hostile demo.

**Load-bearing risk:** the redaction-vs-determinism tension. Replay stays byte-identical
under redaction only if Evidence *plaintext* never influences a logged event — which is
exactly what D-036 already mandates. One code path that leaks plaintext into an event breaks
the safe-to-share claim; the fuzzer must hunt that path specifically.

**Cheap first step:** the end-to-end failing test — record a three-node run with one tool
call and one sensitive Evidence blob, export, redact the blob, replay in a clean directory,
assert byte-identical emission.

**Admission line:** moves incident time-to-resolution and turns the replay guarantee into a
measured property (fuzzer survival rate); consumes D-036 Evidence separation (exists), the
sealed-erasure machinery (exists) and the acceptance export (ROADMAP §3.4 step 15).

**Descendants worth holding:** the incident corpus as a permanent regression suite (every
certified fix's tarball becomes a CI fixture production minted); a time-travel debugger over
stubbed replay; compliance export with selectively disclosed Evidence; **differential replay
as an upgrade oracle** — replay the incident corpus against the next engine version, every
byte divergence is either a release-noted change or a caught regression.

## 3. Graph distillation — the model as compiler

**What:** the terminal form of "cheaper with use". An offline distiller aligns N
byte-identical successful replays of a recurring workflow, separates invariant structure from
input-derived spans (typed holes), and emits a compiled artifact — a fixed tool-call plan or
generated deterministic code — installed as an alternate node implementation behind
precondition guards (input-shape schema, tool version pins, intermediate assertions). The
Governor routes matching runs to the compiled path, which emits events in the same grammar,
so replay and audit hold regardless of tier. A failed guard **deoptimizes**: the model
resumes at that node with the compiled capsule intact, and the divergent trace feeds the next
distillation cycle. Shadow-sampling occasionally runs the model in parallel and auto-revokes
artifacts whose divergence rate climbs. Steady-state marginal cost of a mature workflow
approaches zero model calls — inference becomes a one-time compile cost.

**Why it matters:** every other orchestrator pays the model on every run forever. Replay
determinism proves a run's logic was fully captured, which makes the model itself removable
for stabilized paths — a structural cost advantage nobody else's architecture can copy
without first having the replay guarantee.

**Load-bearing risk:** generalization from N samples. Replay proves the logic for inputs
*seen*, not for the induced parameterization on unseen inputs; the guard schema approximates
the model's real decision boundary crudely, so an input can pass the shape check and need
semantically different behavior — confident, wrong, and silent. Semantic drift, not crashes,
is what kills this.

**Cheap first step:** a read-only trace-diff analyzer over existing logs — align K completed
runs, classify spans invariant / input-derived / unexplained, report a **distillability
score**. Zero engine changes; the score is the go/no-go before any compiler exists.

**Admission line:** accelerates the slope KPI (graded cost-per-task non-increasing, ROADMAP
§11.2) directly; consumes the `ReuseDecision` ledger (05c), the utilization statistics
(CONTEXT §6.4) as its profiler, and the divergence detector (direction 1) as its safety net.

**Descendants worth holding:** the tiered ladder (big model → small local model → decision
table → pure code, promoted per node like JIT tiers — the #35 memoization queue entry is
rung one of this ladder); deoptimization with on-stack replacement; signed compiled-skill
artifacts verifiable by replaying their evidence; profile-guided compile scheduling;
**guard synthesis as the real product** — the model writes, at compile time, the
property-based tests for its own decision boundary, attacking the silent-drift risk head-on.

## Considered and rejected (do not re-litigate without new evidence)

- **Internal context markets** (futures on token budgets, negative pricing for context
  handoff): a market needs counterparties; a single-user local-first system has a central
  optimizer that solves the same allocation more simply. Reconsider only in a genuinely
  multi-tenant future.
- **Credential leases with slashing collateral:** bonded markets need parties to slash;
  same single-user objection, plus the broker's structural isolation already provides the
  guarantee the bond would price.
- **Transferable operational estates** (fork/sell/bequeath a running agent organization):
  provocative, but personal-context leakage risks dominate before Knowledge-Graph scoping
  matures. Phase-4+ material at the earliest.
- **The governor's mutation stream as the only UI** (kill chat): contradicts D-039, decided
  deliberately and recently. The mutation-diff surface may *augment* Studio later; it does
  not replace the chat surface.

## Relationship to the current roadmap

Nothing here blocks or reorders Milestone 05. Direction 1's first step and direction 3's
analyzer are both read-only studies over logs that Milestone 05 executions will produce —
natural post-05 candidates. Direction 2's fuzzer half naturally joins the quality-gate
pipeline (`QUALITY_GATES_AND_DEPLOYMENT.md` gate 10 hardens into it); its export half extends
the acceptance export. Wave-3 admission (ROADMAP §9.2) applies to every promotion out of this
document.
