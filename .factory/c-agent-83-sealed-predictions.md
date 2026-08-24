> **PROVENANCE: this document sealed the predictions for #83 before its run.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# #83 — predictions SEALED before any run

**Base: main `0f4e7fe`.** Design v2 `1a9b6819…` (B-approved), B freeze `a72214ac…`.
**Sealed before the red is built or run.** Typed-unbuilt at seal time — no cargo has touched this.

Written in the previsor's own hard terms so the postmortem is decidable rather than negotiable.

## The arrangement, and why it needs no failpoint machinery

`build_sealer` (`ports.rs:247`) is a pure read of `GRAPHHELM_EVENTS_KEY` validating 64 lowercase
hex. It is the issue's own observation 2. Injecting its failure needs only the environment.

Two preconditions the arrangement MUST satisfy, both verified by source read at `0f4e7fe`:

- **P1 — sealing must be configured.** `build_sealer` returns `Ok(RefusingSealer)` when
  `state.sealing` is `None` and cannot fail. `sealing` is `Some` only when `keyring_all` — both
  `--keyring` and `--key-id` given (`mod.rs:209-218`). Without them the injection is a no-op.
- **P2 — the async drive path must be taken.** `routes.rs:676` branches on `drive_is_viable_for`;
  the else-branch calls the sync `execute`, which has no drive setup at all and therefore cannot
  exhibit the defect. A tool-node graph is the template's route to viability without a real executor.

## Positive control FIRST, guard SECOND — assertion order is deliberate

The response assertion runs **before** the journal assertion:

1. the resume answers **500 / `GHCLI016_DRIVER_FAILURE`** — proves the injection fired AND the drive
   path ran;
2. **then** the journal assertion — the actual guard.

If (1) fails the run is **UNINFORMATIVE / HARNESS-BROKE, not a result.** This is the M09 lesson
enforced structurally: an exit code cannot tell "the guard saw the defect" from "the arrangement
never reached the guard", so the arrangement proves itself before the guard is allowed to speak.

## SEALED: what the red does TODAY (pre-fix)

**Prediction R1 — the red FAILS, at assertion (2), named by panic site in my new test file.**
Signature: `ExecutionResumed` IS present in the journal for the refused call. The panic must name my
journal assertion, NOT a request helper, NOT a fixture builder, NOT the `pause` setup. A red
anywhere upstream = the arrangement broke = **not a result**.

**Prediction R2 — assertions (3) and (4) never execute today**, because (2) fails first. Their
pre-fix behaviour is therefore UNMEASURED at this seal and I am not claiming it. Post-fix all four
must pass. (Stated so nobody later reads "the second-resume assertion passed" as evidence of
anything about today.)

**Prediction R3 — a second resume today answers `not_paused`** (the judge's recorded symptom). This
is a claim about production, not about my test, and it is measurable independently. If a second
resume today is ACCEPTED, the issue's observation 1 does not reproduce in this arrangement and the
whole red needs re-deriving before it means anything.

## THE UNINFORMATIVE CELL — outcomes that measure nothing

Written before the numbers, per standing rule. If any of these occurs, the run is scored
UNINFORMATIVE and no conclusion about #83 may be drawn from it:

- **U1** — assertion (1) fails: no 500/`GHCLI016`. Either P1 or P2 was not met (sealing `None`, or
  the sync path taken). The defect was never exercised.
- **U2** — the server fails to start, or health never comes up. Harness, not product.
- **U3** — the red fails at any line that is not assertion (2). Includes fixture/graph-build/pause
  failures. Proves the arrangement broke, not that the guard sees. **This is the vacuous-red trap
  that cost me the M09 gate afternoon; it gets its own cell rather than being folded into "failed".**
- **U4** — `ExecutionResumed` is absent today for a reason other than the fix, e.g. the resume was
  refused at preconditions before reaching the commit. Then the guard would pass pre-fix and prove
  nothing. Distinguished by assertion (1)'s error code: a precondition refusal is
  `GHCLI005_EXECUTION_STATE`, not `GHCLI016`.

U4 is the one that would produce a **false green** and is the reason assertion (1) checks the
specific code rather than merely "not 200".

## SEALED: sabotage casualties

Each must fall **at its own assertion, named by panic site**. Predicted casualties, one row per
sabotage, written before any run:

| # | sabotage | predicted casualty |
|---|---|---|
| 1 | re-order back to commit-then-setup | assertion (2) falls — the primary guard |
| 2 | assert on rendered status instead of journal | assertion (2) falls; if it does NOT, the projection agreed for the wrong reason and the blade was decoration |
| 3 | setup failure from a source other than the sealer | assertion (2) still falls — guard is about ordering, not one injectable |
| 4 | restore commit-before-setup, same-key retry (B's S4) | the idempotency assertion falls with `not_paused`/conflict |
| 5 | move triage out of its pre-decision position (B's S5) | the triage assertion falls — refused resume no longer appends `Interrupted` |
| 6 | positive control: setup SUCCEEDS | nothing falls; `ExecutionResumed` lands and the resume completes. **If anything falls here the guard is inverted** |
| 7 | `start`'s thin guard, same arrangement | falls at the `ExecutionStarted` grain |

**Death condition for this whole seal:** if sabotage 1 does NOT drop assertion (2), the guard does
not measure the ordering and the design is wrong, regardless of how green everything else is.

## Rate discipline

No zero or rate from this lane gets cited at the count grain. Any "N/N" I report names the
**named-test grain** — the runner's own `test <name> ... ok` line — per H's correction. B has
undertaken to hold me to this and I am restating it inside the seal so it is not optional later.

---

# AMENDMENT 1 — before any run, at reviewer's attack

**Provenance:** B attacked U4 on request and returned two further false-green routes. Amended
**before the red was built or run** — my worktree still has no `target/`. The text above is
UNCHANGED; this section is additive. Original seal `67d04821a77f6dab3f4e2f55903b91ae4e653ebdd62881a37ad250116732644d`
(5762 bytes) remains the record of what was sealed blind to this attack.

Recorded as an amendment rather than a rewrite because a seal quietly edited after review is not a
seal — the value is in what it said BEFORE, including where it was insufficient.

## FG2 — the watermark hole (B)

"No `ExecutionResumed`" must be anchored to a **sequence watermark taken before the injected call**,
never to whole-journal presence/absence and never to a last-event formulation.

- **(i)** If the arrangement's flow ever contains a legitimate resume, whole-journal absence is
  unassertable and a `count == 0` guard fails on correct behaviour. The temptation is then to weaken
  it, and the weakened form is where the false green lives.
- **(ii) Live TODAY, and the sharper one:** the `Started` redispatches append **after**
  `ExecutionResumed` (`resume.rs:230-243`, verified by my own read). So any *"the last event is not
  `ExecutionResumed`"* formulation **passes PRE-FIX** — today's last event is a `Started` outcome.
  A guard that green today proves nothing and would have been believed.

## FG3 — vacuous absence from a wrong read (B)

If the journal-reading half opens the wrong store root, the wrong stream id, or a fresh tempdir —
one typo in a path helper — replay returns few-or-no events and "no `ExecutionResumed`" passes
**vacuously, pre-fix and post-fix, forever.**

This is the zero-counts rule in its original form: absence is evidence only when the same read
proves it is looking at the right place. **Requirement:** the SAME journal read that asserts absence
must also assert the arrangement's own landmarks — the pause event at its known sequence and the
held node's `Paused` state. A wrong read then collapses the landmarks first and reports
HARNESS-BROKE instead of green.

## Amended assertion (2) — one sentence closing U4, FG2 and FG3 together

> **In the journal that provably contains my pause at sequence W, nothing after W is
> `ExecutionResumed`.**

- *provably contains my pause* → closes FG3 (wrong-read collapses the landmark first).
- *nothing after W* → closes FG2 (watermark, not last-event, not whole-journal).
- assertion (1)'s `GHCLI016` vs `GHCLI005_EXECUTION_STATE` discriminator → closes U4.

## R3's own uninformative cell (B)

If R3 comes back **accepted** rather than `not_paused`, that refutes **the arrangement's fidelity to
observation 1** — not the defect. #83's observation-1 journal evidence stands on its own. Correct
response: **re-derive the arrangement; do not downgrade the issue.** Sealed so the tempting
inference is closed in advance.

## Amended death condition

Unchanged in substance, sharpened in grain: if sabotage 1 does not drop the **watermark-anchored,
landmark-proving** assertion (2), the guard does not measure the ordering and the design is wrong.

---

# ARRANGEMENT NOTE — where typed-unbuilt stops

Added before any build. Records what is settled from source and the one thing that is not, so the
build slot is spent iterating on the real unknown rather than rediscovering the settled parts.

## Settled by source read at `0f4e7fe` (no compiler needed)

| fact | evidence |
|---|---|
| injection point | `build_sealer` (`ports.rs:247`) — pure read of `GRAPHHELM_EVENTS_KEY`, 64 lowercase hex |
| P1 arrangement | `--keyring` + `--key-id` only; `keyring_all` alone sets `sealing` (`mod.rs:209-218`). Executor flags NOT needed |
| server env is inherited | `serve_with` spawns via `Command` with no env call — so the test MUST `env_remove("GRAPHHELM_EVENTS_KEY")`, or the result depends on whose machine runs it |
| P2 viability rule | `state.runtime.is_some() \|\| all nodes classify` (`routes.rs:765`) |
| classifiable types | Agent, Planner, Classifier, Evaluator, Tool, Gate. **Deploy is refused** (`classify.rs:28-46`) |
| watermark source | `GET /v1/executions/{id}/events?after=W&limit=N` → `{events, head}` (`routes.rs:70-103`) |
| event grain | `event["kind"]["type"] == "execution_resumed"` — `EventKind` is `#[serde(tag="type")]`, `kind` a named field on the envelope |

## The one genuine unknown — the pausable arrangement

`cli_start`'s graph is `examples/graphs/manual-override-deploy.yaml`, which carries a **`deploy`**
node. By the rule above that graph is **NOT viable** → the sync `execute` branch → **the defect is
never exercised**. That is UNINFORMATIVE cell **U1**, and it would have been invisible: the test
would have gone green against a code path that cannot contain the bug.

So the arrangement needs a graph that is simultaneously:
1. **all-classifiable** (no `deploy`) — so the async drive path is taken; and
2. **stalled, not terminal** — so `pause` has something to hold and `resume` is not refused at
   preconditions (which would be false-green **U4**).

`blocked_fixtures` (`{"implementation": "failure"}`) achieves (2) for the existing two-node graph by
failing the first node so the second never dispatches. The open question is the exact node-type
substitution that keeps (2) while gaining (1) — `agent`/`gate` carry different required sub-blocks
(`agent:` vs `gate:`), and authoring that YAML blind risks a schema failure that would surface as a
**U3 vacuous red**: a failure at graph-build, upstream of every assertion, proving only that my
fixture was wrong.

**Decision: do not guess it.** Authoring unverifiable YAML plus ~400 lines of unbuildable harness
trades one cheap build iteration for a large speculative diff and a likely vacuous red — the exact
failure this seal exists to prevent. The graph/fixture pair gets settled in the first build
iteration, where a wrong guess costs seconds and is visible.

**What this does NOT change:** every assertion, grain and cell above is fixed and stays fixed. The
arrangement is the plumbing that carries the test to the guard; settling it is not licence to
re-open what the guard asserts. If the arrangement turns out to require a change to an assertion,
that is an amendment with its own provenance, not a quiet edit.

---

# AMENDMENT 2 — U1 closed by precedent, before any build

**Provenance:** B closed the arrangement's open piece read-only. Verified by me at `0f4e7fe` rather
than accepted, because it is the claim the whole arrangement now rests on.

**Fixture: `docs/acceptance/m09-judge-run-2026-08-19/release.yaml`** (committed in-tree; byte-identical
copy at `.factory/m09-paid-run-archive/release.yaml` — confirmed with `diff -q`).

Verified facts:
- Three nodes — `preflight`, `deploy`, `verify` — **every one `type: tool`**.
- The two `type: data` occurrences are **edge** types (`edges:` at lines 61-63), not nodes.
- `classify.rs:33` maps `NodeType::Tool -> Ok(NodeWorkKind::Tool)`, so *all nodes classify* and
  `drive_is_viable_for` (`routes.rs:765`) answers **true with `runtime: None`**. P2 satisfied as a
  code fact, not a hope.
- **This is the graph the M09 paid run started, paused and resumed over the serve API** — the graph
  that produced the issue's observation 1. The red reproduces the defect on the fixture that
  discovered it, which is the strongest arrangement-fidelity claim available and answers R3's
  uninformative cell better than fresh YAML could.

**So no YAML is authored, no schema is guessed, and no build iteration is spent discovering the
graph.** What remains compiler-dependent is harness plumbing only.

## The name/type trap — recorded because it nearly cut both ways

`manual-override-deploy.yaml` is non-viable because it carries a genuine `NodeType::Deploy`.
This graph contains a node **named** `deploy` whose **type is `tool`** — viable.

Same word, opposite implications. Pattern-matching on the node NAME would have rejected the correct
fixture just as surely as it would have accepted the wrong one. The name is one level above the
thing that decides; **`type:` is the finest grain and the only one `classify.rs` reads.**

This is [[assert-at-the-finest-grain]] applied to a *fixture selection* rather than an assertion,
and a flattening in the READER rather than in the code: the word "deploy" collapses two distinct
node types into one signal, and the consumer that trusts it gets a confident wrong answer in
whichever direction it happens to be wrong.
