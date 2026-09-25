# Milestone 06 — Quality Gates, as built

Status: M06 COMPLETE. GraphHelm's correctness story (TDD, sabotage, the acceptance map)
gained its USEFULNESS story: gates that cannot be gamed because each one first proved it
knows how to reject, a blind judge that scores stories against the running system, and the
dev factory as their first production user — judged by its own product and honestly
refused. Design: `docs/superpowers/plans/2026-08-16-quality-gates.md` (the five binding
decisions from the 2026-08-16 divergent pass). Issue #54; hotfix #55/#56 landed mid-flight.

## What M06 shipped

### The verdict vocabulary (kinds 29 and 30)

`GateVerdict` is refusal-with-findings BY CONSTRUCTION: the envelope schema's `if/then`
refuses a failing verdict with an empty findings list — a bare fail cannot exist on the
wire, and `judge::parse_reply` refuses it even transiently in-process. Findings carry
severity, the cited claim, evidence refs and a remediation. `GateCertified` is the thymus
receipt AND replayable state: the fold records `gate_certifications[gate_id] =
suite_digest`, which is exactly what the execution precondition reads; growing the
pathogen suite changes the digest and voids old immunity by comparison, never by cleanup.

### The thymus (binding decision 1)

`tools/pathogens` breeds ten specimens — one per uselessness mode (dead feature,
unreachable UI, tautological journey, blank screen, orphan view, gutted assertion,
happy-path-only, spec claim without artifact, minimal diff without behavior, label-swapped
UI) — each GREEN by correctness measures and bred to fool a specific plausible gate,
paired and pinned per specimen. `certify()` refuses on ANY pass, naming who fooled the
candidate. Two philosophical pins are tests: the correctness battery itself fails
certification on all ten (correctness alone certifies nothing), and `reject_everything`
certifies (the thymus is necessary, not sufficient — the geometry evaluators must ALSO
pass real deliverables, pinned on the genuine 05f monitor render).

### The deterministic evaluators (binding decision 4)

`core/quality` (pure — no clock, no entropy, no IO): the spec-derived content manifest
(presence, reachability, label binding, claim backing, journey grounding, test honesty,
diff substance) and the layout grammar over STRIPPED html (empty sections, density
budgets, WCAG contrast over declared style pairs, link-orphans, row-shape variance).
Builder-authored text is stripped before anything is scored — the praise-stuffing sentinel
pins identical findings — with one carve the implementation taught: a stylesheet is
geometry DECLARATION, not prose, so style bodies survive the strip. The COMPOSED evaluator
earned certification against all ten pathogens; the layout grammar ALONE is REFUSED
(a gutted assertion has flawless geometry) — geometry must never gate by itself, as a test.

### Gate nodes execute — certified or not at all

`Gate` routes to `NodeWorkKind::GateCheck` (a genuinely different transport: deterministic
evaluation, NO model port — proven by panicking ports); the other fifteen node types stay
byte-identical to 05d, table-pinned. The precondition reads the fold's certification
against the CURRENT suite digest injected as driver configuration (`core` never depends on
`tools`); no digest, stale digest, or no receipt refuse identically, BEFORE dispatch. A
failing verdict maps to `TerminalFailure` (deterministic evaluation: retrying the same
deliverable cannot change the answer — routing on it is the graph's decision) with the
full findings sealed as evidence and the verdict appended beside the outcome.

### The blind judge (binding decision 2, the FIX-1 decision)

No new work kind: the judge is one model-call shape on the cognitive transport, its
blindness an INPUT DISCIPLINE with three test-pinned fences — the type is the diet
(`JudgeWork` = judgeId + userStory + mcpSurface; `deny_unknown_fields` refuses a smuggled
rubric as unassemblable), the assembler's signature (`assemble(&JudgeWork)` alone), and
the source fence (judge.rs speaks no leak vocabulary, source-scanned). The charter
instructs doorbell waits (wake_arm between probe steps, never timed polling) and the
refusal-with-findings contract with stepsOverPar and stallPoints. A malformed reply is
`RetryableFailure` (the MODEL flaked); a well-formed refusal is `TerminalFailure` (the
DELIVERABLE failed judgment). The parser tolerates fences and prose around the verdict —
learned from the live run — while the contract stays whole and bare-fail refusal survives
unwrapping.

### Demonstrations — the acceptance map's third binding (binding decision 3)

Clauses now bind to recorded journeys: a demonstration artifact (event store + `demo.json`
+ `SHA256SUMS`) that the grounding test REPLAYS against the current build — a fold
refusal, a divergent terminal state, or a projection digest off the frozen one each fail
the map. The seed is sampled AT RECORDING from entropy handed TO the recorder (the crate
generates none), frozen inside the artifact; replay re-derives from the frozen seed — what
a build cannot precompute is the next recording, never the replay. The artifact verifier
gained tracked-vs-named (a named-but-gitignored artifact is a grounding failure — the 05f
journal lesson paid as machinery).

### The dogfood (binding decision 5) — the factory judged by its product

One real run (2026-08-17, artifacts + SHA256SUMS under
`docs/acceptance/m06-run-2026-08-17/`): prepare ran real git in a Tier 1 worktree; the
UNCERTIFIED gate refused to dispatch live; `graphhelm quality certify` ran the thymus and
stamped the receipt; the certified gate ran and appended `gate_verdict passed=true`
(kind 29 in production); and the blind judge — real model on the owner subscription,
probing the live serve through the MCP tools alone — REFUSED the one-glance story with
four substantive findings: the status surface answers green on a wedged execution
(critical); nodeStateCounts omits zero-valued failure buckets; retryable_failure events
carry no cause (the exact opacity this milestone's own debugging suffered, found
independently); wake_status cannot say whether the alarm fired. The execution ended failed
BECAUSE the judge refused. Those findings are the recorded backlog seed for M07.

The gate-freeze rule ships as a pure check (`freeze_violation`): a diff touching gate
machinery together with anything outside it is a hard violation — the judge and the judged
never move in one PR. The rule's charter lives inside the freeze at `docs/gates/freeze.md`,
and the authoritative prefix list is the `GATE_MACHINERY` constant beside `freeze_violation`.
(This paragraph used to restate the list and drifted — it still said three prefixes after
`tools/source-invariants/` made four (#361, found in #282). A restated list is a second
producer of one set, and the copy is the side nothing checks.)

`graphhelm quality certify` debuts GHCLI018_GATE_INVALID; its registry is CLOSED
(gate-geometry; adding a gate means adding its thymus adapter — no generic door), and the
blind judge is deliberately NOT certifiable there: its discipline is blindness, proven by
the runtime's fences, not pathogen rejection.

`gate_http` joined the gate as the twelfth CLI suite (22 stages), proving over HTTP the
digest thread no suite covered, with the model route permanently jammed as proof gate work
never touches a model.

## Honest limits, stated (M06)

- **The judge costs a real model call and varies.** One certified transcript per
  judgment; the verdict's findings vary run to run in wording and count — the CONTRACT
  (refusal-with-findings, steps, stalls) is stable, the prose is not. Judgment is sampled,
  not proven.
- **Geometry catches broken, not sublime.** The grammar refuses ragged, washed-out, empty
  and orphaned; it cannot see elegant. Taste stays human until proven otherwise.
- **Pathogens are ten, not infinity.** The suite grows per escape (a pathogen that fools a
  certified gate in the wild is a new specimen, and the digest change voids all
  immunity). The thymus is necessary, not sufficient.
- **The judge's zero-poll discipline is charter-instructed, counter-proven only at the
  05g layer.** The live judge probed briskly and never armed a lease (nothing to wait
  on); a long-wait judgment measuring zero polls end-to-end remains for the first
  long-running judged story.
- **POSIX-form paths bite Windows children twice removed.** `/c/...` in route args and
  MCP configs resolves as relative in spawned CLIs (two live bites in the dogfood run);
  route authors on Windows must write drive-letter forms. A path-shape lint is a
  plausible M07 knob.
- **A restarted serve with a fresh `GRAPHHELM_EVENTS_KEY` cannot open the prior keyring**
  (the 05d double-duty limit, third bite) — and sealed evidence under a lost key is
  unreadable BY DESIGN: persist run keys or lose run evidence.
- **The factory's own pipeline graph is one story, not a library.** The dogfood proved
  the shape end to end once; PR-gating every factory change through a graph awaits the
  M07 wiring the judge's own findings call for.

## The 3am praise (the pair loop's own verdict)

Built doorbell-native end to end: every handoff of this milestone was a durable append,
every wait a blocked pipe, zero polling on either side. Mid-flight the loop's own store
found #55 (concurrent double-consume bricking a stream — hotfixed as #56 with the
deterministic interleave), and the blind judge closed the circle by refusing the very
surface the pair relied on. The product is now the factory's sharpest reviewer.
