# Journey-first Keel: journeys prove user-visible change, small tests guard invisible invariants, a step opens live, a planner decides the route

Status: design, owner-approved direction relayed by the coordinator on 2026-10-07, issue #382.
Docs only; no mechanism reads this file until the update proposal in §9 lands.

## 1. Promise

1. A change that touches a screen is proven by **its journey replaying green at the head**, not by
   a new unit test. Keel's test-per-change rule is relaxed for UI and product behaviour.
2. Small tests stay **mandatory only for invisible invariants**: security and permissions,
   persistence and journal integrity, concurrency, destructive operations.
3. Any step of a journey can be **opened live**: a real browser at that step, reached by replaying
   the cached acts of the earlier steps with no model call, kept open for the owner or an agent to
   continue, with that step's pass/fail shown while it is open.
4. A **task planner** decides, per task and deterministically first, the review count, the proof
   type, the skills, the agent tier and the shortest path to done, and records that decision where
   every agent reads it.
5. The Studio **Team tab shows each task as its own small live graph** (issue → plan → implement →
   proof → review → merge) with the agent on each node and the current step lit, from structured
   records the lanes emit under their own names.

## 2. What exists (the ground)

| Thing | Where | Status on `main` |
|---|---|---|
| Journey flow (`graphhelm.journey-flow/1`): screens with `scope`, edges with `acts`, named paths | `.graphhelm/journeys/<id>.journey.yaml`; `schemas/journey-flow.schema.json`; design `docs/specs/2026-10-06-journey-explore-design.md` | shipped (#342) |
| Compiled contract: linear steps, `screen.scopePaths`, promises | `.graphhelm/journeys/<contractId>.json`; `journey compile` / `journey approve` (D-057) | shipped |
| Replay cache: per edge, one resolved locator per act (`testId`, `context`, role + exact name); per screen, fingerprint and controls; `flowDigest`, `viewport` | `.graphhelm/journey-cache/<id>.json`; `schemas/journey-replay-cache.schema.json` | shipped (#348) |
| Deterministic driver (`open`, `snapshot`, `act`, `capture`, `close`), local hosts only, secrets by name, masked captures | `tools/journey-driver/driver.mjs`, installed as `.graphhelm/observers/journey_driver.mjs` | shipped (#348) |
| `journey replay <id>`: every path of an approved flow, records `jpd.screen_captured` and `jpd.transition_walked` | `apps/cli/src/commands/journey_replay.rs` | shipped; first green run not yet observed (`docs/guides/journeys.md` §4) |
| Journeys read: per step the newest capture, `fresh` / `stale` / `unknown` against git history, walked arrows | `GET /v1/journeys`, `graphhelm journeys`, `apps/studio/src/runtime/journeys.ts` | shipped (#332) |
| Studio Journey tab: cards per step, arrow state, detail dialog, before/after rows | `apps/studio/src/components/journey-canvas.tsx`, `right-panel.tsx` | shipped; read-only (Approve is #353) |
| Keel policy: card bounds, scope rule, test proof, surface budgets, `surfaceEnforcement: signal` | `extensions/builtin/graphhelm-development-contracts/policies/keel.yaml` v1.3.0 | shipped |
| Keel card `journeys` and advisory `keel.journey.no_fresh_capture` | `schemas/keel-card.schema.json`, `graphhelm keel check` | shipped (#328) |
| Proportionality table (what a change must carry) | `AGENTS.md` "Keel: how code is written here"; `docs/process/DELIVERY.md` §2 | shipped |
| Delegation record: `delegation_chosen` with `kind`, `tier` (`Small`/`Standard`/`Large`), `effort` (`Low`/`Medium`/`High`), `policy`, `basis` | `core/protocols/src/delegation.rs`; sessions reused since #298 | shipped |
| Briefing: objective, decisions, work done, pending, next step | `core/execution/src/briefing.rs`; `execution briefing`, MCP `briefing` | shipped |
| Jev: a System One judge that answers typed questions with a probability and cannot draft | `docs/specs/2026-09-16-architect-judgments-design.md` | shipped (routes may be absent) |

Nothing below invents a second copy of any of these. Each rule names the field or file it reads.

## 3. Rule 1: journeys are the primary proof for user-visible change

**Definition.** A diff path is *user-visible* when it equals, is under, or contains a
`screen.scopePaths` entry of any compiled contract in `.graphhelm/journeys/` (the comparison
`keel check` already makes for `keel.journey.no_fresh_capture`). The *journeys touched* by a diff
are every contract with such a screen; the *steps touched* are those screens.

**Proof.** For every journey touched, `graphhelm journey replay <flowId>` run on the head, in the
project, with the observer installed, ends with:

- every path that contains a touched step observed (`outcome: observed`), its `expect` pairs met
  on each screen, and
- a `jpd.screen_captured` for every touched step with `revision` = head and `dirty: false`, and
  `jpd.transition_walked` for the arrows into and out of it.

That is "replay green at the head". `graphhelm journeys` then shows the touched steps `fresh` and
their arrows `walked`.

**What is relaxed.** For a user-visible change, Keel Law 3 ("a test is born against a named
defect") no longer asks for a new unit or component test. The journey is the test: the defect it
kills is "the user cannot complete this journey after the change". A PR may still add a small test
when the author can name a defect the journey cannot observe (a layout rule at a width no journey
pins, a keyboard-only path); it is then weighed as today, never required.

**What is not relaxed.** Replay observes the real app; a proxy is not an observation (Keel rule 5).
A change to a screen with no journey has no journey proof: the author maps one first
(`journey-map` skill, flows under `.graphhelm/journeys/`) or the review says `OBSERVER_MISSING`
and treats the change as unproven. A red replay is a red result and is reported as such; a later
green never erases it (Keel rule 5).

**Card.** When the scope touches a screen, the Keel card's `journeys` field is required, not
optional, and lists every journey touched. `keel check` reports `keel.journey.card_missing_journey`
(the scope touches a screen the card does not name) and `keel.journey.replay_not_green` (no
capture at the head, or the newest replay cache for that flow records a drift for a touched edge).
Both start as signals under `surfaceEnforcement: signal`, like every Keel count today.

## 4. Rule 2: small tests stay mandatory for invisible invariants

A change is *invariant-bearing* when it touches one of these classes. The classes are path sets in
`keel.yaml` (§9.3), compared the same way as scope paths, so the classification is deterministic
and reviewable in a diff of the policy file.

| Class | Why a journey cannot prove it | Example from this repository, 2026-10-07 |
|---|---|---|
| Security and permissions | The user who is allowed sees the same screen as the one who is not; the defect is who else can act. | #380: owner-only actions (`journey approve`, workspace sweep) trust a self-declared `--actor-type`; any agent can claim `owner`. Only a test that presents an agent-scoped token and is refused observes the invariant. |
| Persistence and journal integrity | The screen reads a projection; the defect is in what the journal keeps or drops between reads. | #363: a budget-cut journal verification discarded its progress and `serve` livelocked on a 20 MB journal. The Studio showed one slow read; the defect was a loop over genesis. |
| Concurrency | A journey is one actor in order; the defect needs two requests at once. | #343: two first requests on a cold event store raced to create `format.json` and one got HTTP 500 on Windows (sharing violation). No single-user journey reaches it. |
| Destructive operations | A journey that proves deletion works proves nothing about what else it deleted. | #374: owner-only sweep of agent workspaces; the review asked for junction handling (a junction under the root must be unlinked, never recursed into) and for a dry run. |
| Wire compatibility (kept from today's expanded route) | A reader on another version is not in the browser. | `p50.dev` identifiers, event-envelope and contract schemas (`AGENTS.md`, constitutional invariants). |

For these classes Law 3 applies in full: a new test names the defect, fails on the parent, asserts
against an independent oracle, and passes the `test-audit` gate. A journey may accompany it; it
never replaces it. A change that is both user-visible and invariant-bearing carries both proofs.

## 5. Rule 3: open live at this step

**Promise.** In the Studio Journey tab, a step card gains **Open live**. Clicking it opens a real
browser at that step, reached by replaying the cached acts of the earlier steps of the chosen path
with no model call, keeps it open, and shows that step's pass/fail while it is open. An agent
reaches the same step the same way and continues from it.

**Mechanism** (reuses `journey replay`'s supervisor and driver; adds no second browser path):

1. `graphhelm journey open <flowId> --step <screenId> [--path <name>] [--events … --execution …
   --keyring … --key-id …]`. The path defaults to the first path that contains the step. The
   supervisor loads the flow and its replay cache, refuses a draft or stale-approved flow and a
   missing cache exactly as `replay` does (`replay.cache_missing` is new: open needs a prior replay,
   because the cache is what makes the walk deterministic), opens the driver **headed** (a new
   driver `open` field `headed: true`; headless stays the default for replay), and sends the cached
   acts of every edge before the step, checking each screen's `expect` on the way. The acts are the
   cache's locators (test id, then role + exact name within context, then global), never a model.
2. On reaching the step it runs the step's `expect`, records one `jpd.screen_captured` with
   `phase: live` (the capture document already carries an optional `phase`; `live` joins `before`
   and `after`), and prints one JSON line `{"step": …, "state": "pass" | "fail", "code": …}`.
   On a drift before the step it stops at the broken edge, prints its `drift.*` code, and leaves the
   browser open there: the owner sees where the journey broke.
3. The browser stays open until `journey close <session>` or the whole-run timeout (the same bound
   `replay` has). The driver session is addressable: `journey act <session> --kind … --role …
   --name … [--text … | --secret …]` sends one act through the same protocol and prints the new
   screen's `expect` state. That is how an agent continues; the owner continues by hand in the
   same window. Acts whose accessible name matches the destructive deny list
   (`delete|remove|pay|purchase|transfer|send`, design §11) are refused on a live session unless the
   approved flow contains that act on that edge.
4. Runtime: `POST /v1/journeys/{contractId}/open` with `{stepId, path?}` spawns the supervisor on
   the Runtime host and returns the session id; `POST /v1/journeys/sessions/{id}/act`,
   `DELETE /v1/journeys/sessions/{id}`. CLI and MCP twins (`journey_open`, `journey_act`,
   `journey_close`) exist in the same PR (every Studio action exists on the public API/CLI).
   Owner-only, by the owner credential #380 introduces; an agent gets a session only through its
   scoped token and may not open a flow it is the author of under review.
5. Studio: the step card's **Open live** calls the route; the card shows a live chip (`opening`,
   `at step · pass`, `at step · fail · <code>`, `drift at <edge>`) read from the session's records
   (`jpd.screen_captured` with `phase: live` and the drift line), never from the button's own
   success. The chip disappears when the session closes. The detail dialog lists the acts the
   session has taken, from the cache plus `journey act` records.

**Why cached acts and not the flow's acts.** The cache holds the locator that resolved uniquely
last time, so the walk is the same every time and costs no model; the flow's acts are the intent
the cache was derived from. If the cache is void (flow digest changed), `open` refuses and names
`journey replay` as the remedy, which rebuilds it.

**Agents traverse journeys the same way.** An agent that must reach "the pay screen with a filled
cart" does not script a browser: it opens the journey at that step and acts from there. The
session records are the agent's proof of what it saw. `journey explore` (#356) is the one place a
model proposes acts; `open`/`act` execute approved or owner-given acts only.

## 6. Rule 4: the task planner

**Promise.** For every task the planner answers five questions before any agent writes:

| Question | Answer set |
|---|---|
| Reviews | `0` docs-only and inert config; `1` user-visible or bounded code; `2` any invariant class of §4 |
| Proof | `journey` (Rule 1), `tests` (Rule 2), `both`, or `none` (docs) |
| Skills and tools | the skills the proof needs: `journey-map` when a touched screen has no journey, `test-audit` when `tests`, `keel` always for code; tools: the observer when `journey` |
| Agent tier | `DelegationTier` × `DelegationEffort` from `core/protocols/src/delegation.rs`: `Small/Low` for docs and inert config, `Standard/Medium` for bounded or user-visible code, `Large/High` for any invariant class or a change that compiles a new public surface |
| Shortest path | the ordered steps: card → (map journey) → change → proof command(s) → review(s) → merge, each named with its command |

**Deterministic first.** The input is the set of paths the task names (its card `scopePaths`, or
the paths of a diff when the task is a fix). The planner classifies each path, in this order and
stopping at the first match: invariant class (§4 path sets) → user-visible (screen `scopePaths`) →
code (a source file under a crate or app) → docs/inert (everything else, plus the proportionality
table's first row). The task's class is the highest class of any path; reviews, proof, skills and
tier follow from the class by the tables above. The same paths always give the same answer, and
the answer is reproducible from the policy file at the task's revision.

**Jev only when the rules are ambiguous.** Ambiguity is one of: a path under no class and not
plainly docs (a new directory), a task whose stated promise names a class its paths do not (the
card says "permission" but touches only a screen), or two classes that disagree on the tier. Then
the planner asks Jev the typed questions `task_class`, `review_count`, `proof_type` with the
paths and the promise as state (the judgment door of `docs/specs/2026-09-16-architect-judgments-design.md`),
takes the answer above the configured threshold, and records it. With no Jev route the planner
takes the stricter answer and records `decidedBy: fallback_strict`.

**The record.** One signal, kind `keel.plan`, description `graphhelm-task-plan-v1`, recorded on
the task's execution by the planner (actor: the session that planned, `system` when the Runtime
planned at `start`):

```json
{
  "schema": "graphhelm-task-plan-v1",
  "taskId": "issue-382",
  "revision": "<40-hex>",
  "paths": ["apps/studio/src/components/journey-canvas.tsx"],
  "classes": ["user_visible"],
  "journeys": ["studio-journey-tab"],
  "proof": "journey",
  "reviews": 1,
  "skills": ["keel", "journey-contract"],
  "tools": ["observer:journey_driver"],
  "delegation": {"kind": "implementer", "tier": "standard", "effort": "medium"},
  "path": ["card", "change", "graphhelm journey replay studio-journey-tab", "review x1", "merge"],
  "decidedBy": "rules",
  "jev": null
}
```

With Jev: `"decidedBy": "jev"` and `"jev": {"route": "<route id>", "questions": {"task_class":
{"answer": "user_visible", "probability": 0.91}}, "judgmentSequence": <n>}` where the sequence
cites the `architect_judgment` record Jev's answer already produces, so the decision is auditable
and the same state replays to the same record. The schema lives in
`extensions/builtin/graphhelm-development-contracts/schemas/task-plan.schema.json`, next to the
Keel card schema.

**Where agents read it.** The briefing (`core/execution/src/briefing.rs`) gains `plan`: a copy of
the newest `keel.plan` on the execution, the same way `pending` copies the attention answer, never
recomputed. `execution briefing`, MCP `briefing` and `compile_context` carry it; the Studio run
details show it as the "route" line (reviews, proof, tier). An agent that starts work without a
plan in its briefing asks the planner (`graphhelm keel plan --paths … --promise …`, MCP `keel_plan`)
before writing; a review that finds the PR off its recorded route says so by name.

**What the planner does not do.** It does not choose a model by name (that is the gateway's route
manifest and `delegation_chosen`), does not merge, and does not override a reviewer: a reviewer may
raise the review count, never lower it.

## 7. Rule 5: the Team tab shows per-task graphs

**Today.** `apps/studio/src/runtime/team.ts` folds one bot per `actorId` that recorded a signal,
plus "Steps without a bot" for graph nodes nobody is assigned to. Every Claude lane registers the
MCP server with the same `--actor agent-chat` (the project's `.mcp.json`), so six lanes are one
card named `agent-chat`, and no record says which PR a lane is on or what step that PR is in. The
owner reads the lanes' prose notes to find out.

**Promise.** One small graph per task (issue or PR), live:

```
issue #382 ──> plan (reviews 1 · proof journey) ──> implement [gh-claude-6] ──> proof [replay studio-journey-tab: green]
          ──> review ×1 [gh-claude-1: APPROVE-WITH-RISK] ──> merge [6dc60a01]
```

The current step is lit; each node names the agent on it; a `BLOCK` or a blocked state sits on the
edge into the next step with its reason; a red proof sits on the proof node. Several tasks are
several graphs side by side (a list that expands to the graph on narrow widths, like the mobile
tabs). Every task node links to its GitHub issue or PR, and the proof node links to the journeys
the Keel card named (`journeys`), which open the Journey tab on that journey.

**Source of truth: records, not prose.** Five signal kinds, each a `graphhelm-task-event-v1`
description document on the execution the lanes work in (`gh-team` here), recorded by the lane
through the existing `signal` door (CLI, HTTP, MCP) under its own actor:

| Kind | Fields (besides `schema`, `taskId`, `revision`, `at`) | Recorded when |
|---|---|---|
| `task.claimed` | `issue`, `lane`, `branch`, `plan?` (a copy of the `keel.plan` summary: reviews, proof, journeys) | the lane takes the issue (the `workspace claim` of #374 records it for free when it runs) |
| `task.pr_opened` | `pr`, `headSha`, `journeys: []`, `lane` | `gh pr create` succeeded |
| `task.review_assigned` | `pr`, `headSha`, `reviewer`, `ordinal` (1 or 2) | the author or the coordinator asks a reviewer |
| `task.review_verdict` | `pr`, `headSha`, `reviewer`, `verdict` (`APPROVE` / `APPROVE-WITH-RISK` / `BLOCK`), `commentUrl` | the reviewer posts the verdict (the same line it writes on the PR) |
| `task.merged` | `pr`, `mergeSha`, `closes: []`, `merger` | `gh pr merge` succeeded and the merger read what landed |

A `BLOCK` with no later `task.review_verdict` on a newer `headSha` keeps the edge red; a new
`task.pr_opened` with a new `headSha` re-arms the review step. The graph is folded from these
records alone (`apps/studio/src/runtime/team-tasks.ts` already folds task records; it gains this
kind); GitHub is linked, never polled, so the view works offline and the records are the audit
trail when GitHub and the lanes disagree. The schema lives in the development-contracts package
beside `task-plan.schema.json`.

**Per-lane identity.** The MCP server already refuses to start without an actor and already reads
`GRAPHHELM_ACTOR` as the second door for `--actor` (`apps/cli/src/args.rs`). The change is
procedural plus one guard: `.mcp.json` stops pinning `--actor agent-chat`; each lane sets
`GRAPHHELM_ACTOR=<lane>` (the ListAgents name of `AGENTS.md`'s identity line) in its session
environment; a signal whose `source.id` differs from the authenticated actor is refused
(`GHCLI0xx_ACTOR_MISMATCH`), so a lane cannot record under another lane's name. `team.ts` then
shows one bot per lane with no code change; the `agent-chat` card disappears when the last shared
session closes. `actor_alias` stays the owner's way to rename a lane for display.

**Skill hook.** The identity line every comment and commit already carries is the same data as
`task.review_verdict` and `task.pr_opened`; the `keel` and review skills record the signal in the
same step that writes the line, so the view costs the lane one call per step it already performs.

## 8. Phases and proof

| Phase | Delivers | Observer |
|---|---|---|
| A | §9 text changes: `AGENTS.md` table, `KEEL_SPEC.md` Law 3 and Law 6 notes, `DELIVERY.md` §2–§4, `keel.yaml` 1.4.0 (`invariants`, `journeyFirst`), card `journeys` required when a screen is touched; `keel check` signals `card_missing_journey`, `replay_not_green` | CLI tests: a diff touching a screen's scope path with a card that omits `journeys` reports the signal; the same diff with a green replay cache reports nothing; a diff under `core/events/` reports `invariant: persistence` |
| B | `keel plan` (CLI, MCP), `keel.plan` record and schema, briefing `plan` | CLI test: fixed paths give a byte-identical record across two runs; a path under two classes takes the higher; with a recorded Jev the record cites the judgment sequence |
| C | `journey open`, `journey act`, `journey close`; driver `headed`; `phase: live` captures; Runtime routes and MCP twins; owner credential (#380) | Fixture app from phase 2: open at step 2 of `checkout` replays one cached edge headed, prints `pass`; rename the button → `drift.locator_missing` at `cart.checkout/0` and the browser stays open on `cart` |
| D | Studio **Open live** chip and acts list | vitest for the card and chip states; the chip reads records, not the click |
| E | First end-to-end run on this repository: the `studio-*` flows replay green, captures fresh in the Studio (#381) | `graphhelm journeys` shows every touched step `fresh`; a Journey tab screenshot |
| F | `graphhelm-task-event-v1` schema, the five `task.*` kinds, `GRAPHHELM_ACTOR` per lane and the actor-mismatch refusal, skills record the events | CLI test: a signal whose `source.id` is not the authenticated actor is refused; the five kinds validate; `team-tasks.ts` folds a PR from `pr_opened` → `review_verdict: BLOCK` → `pr_opened` (new head) → `review_verdict: APPROVE` → `merged` into the expected step sequence |
| G | Studio per-task graphs in the Team tab, links to PR and journeys | vitest: the fold of F renders the lit step, the agent per node and the red edge; a screenshot with two lanes on two PRs |

Phase E is the proof that Rule 1 is usable at all; A does not ship as a gate before E has run once.

## 9. Update proposal (the exact changes)

### 9.1 `AGENTS.md`, "Keel: how code is written here", the proportionality table

Add one row after "A bounded code change on the direct route":

| The change | What Keel asks |
|---|---|
| User-visible: the scope touches a screen's `scopePaths` of a compiled journey | The three-line card naming the journeys touched (`journeys:`), and the journeys replayed green at the head (`graphhelm journey replay <id>`, captures fresh). No new unit test is asked for; one may be added when it names a defect the journey cannot observe. |

Amend the expanded-route row: "persistence, permissions, compatibility, security, external
effects, runtime-affecting config, concurrency, destructive operations: the full card, the JPD flow,
**and a test that names the defect (Law 3), whether or not a journey also covers the change**".

### 9.2 `docs/keel/KEEL_SPEC.md`

- Law 3, add: "For a user-visible change (a screen's scope path in the diff) the journey is the
  proof instrument: its replay at the head kills the defect 'the user cannot complete this
  journey'. A unit test is admitted on the same terms as before but is not required. For the
  invariant classes in `keel.yaml` `invariants` the law stands unchanged."
- Law 6, add: "Verification reach is read from the path classes the policy declares, not argued per
  PR; the planner's record (`keel.plan`) is the reach decision, and a review that disagrees raises
  it."

### 9.3 `keel.yaml` → version `1.4.0`

```yaml
# JOURNEY-FIRST PROOF (1.4.0, #382). A diff path that equals, is under or contains a compiled
# contract's screen scopePaths is user-visible. Its proof is the journey replayed green at the head.
journeyFirst: true

# INVISIBLE INVARIANTS (1.4.0, #382). Paths whose defects no journey can observe; Law 3 applies in
# full. Compared like scope paths. A repository extends the lists in its own keel.yaml.
invariants:
  security:      [core/policy/, apps/cli/src/commands/serve/auth.rs, core/events/src/key.rs]
  persistence:   [core/events/]
  concurrency:   [core/events/src/local.rs, apps/cli/src/commands/serve/]
  destructive:   [apps/cli/src/commands/workspace/]
  compatibility: [schemas/, core/protocols/]

# PLANNER (1.4.0, #382). Deterministic routing per class; Jev only on ambiguity.
plan:
  reviews:    {docs: 0, user_visible: 1, code: 1, invariant: 2}
  delegation: {docs: [small, low], user_visible: [standard, medium], code: [standard, medium], invariant: [large, high]}
  jevThreshold: 0.8
```

(The security list above is illustrative: the PR that lands 8.3 names the real files of the auth
seam, `agent_route_allowed` in `apps/cli/src/commands/serve/mod.rs` today.)

### 9.4 `docs/process/DELIVERY.md`

- §2 table: the new row of 8.1; §3: "Before/after captures" becomes "Journey proof": a PR whose
  scope touches a screen runs `journey replay` for each journey it names and pastes the JSON
  summary (paths observed, captured signal ids); before/after captures remain for a visual diff.
- §4: "the reviewer runs the tests the change reaches" gains "and, for a user-visible change,
  `graphhelm journey replay` for each journey the card names, on the head, and pastes its summary";
  the review count is the plan's (0 for docs-only is still one reader before merge: the merger
  reads what lands, as §5.4 already says).

### 9.5 Schemas and records

- `schemas/keel-card.schema.json`: `journeys` stays optional in the schema (a card without screens
  has none); `keel check` enforces presence when a screen is touched.
- New `task-plan.schema.json` (§6) in the development-contracts package; package digests re-pinned
  (`extensions/releases/adoption-0.1.1.json`, the guard test from #323).
- `graphhelm-screen-capture-v1`: `phase` enum gains `live`.
- New `task-event.schema.json` (§7) in the development-contracts package.

### 9.6 Lane identity and records

- `.mcp.json` (project): remove `--actor agent-chat`; `AGENTS.md` multi-agent section: "every session exports `GRAPHHELM_ACTOR=<ListAgents name>` before starting the MCP server".
- `docs/process/DELIVERY.md` §3–§5: each step that writes an identity line also records the matching `task.*` signal; the merge step records `task.merged` after reading what landed.
- The `keel` skill and the review brief name the commands (`graphhelm execution signal --signal <file>` or the MCP `signal` tool with the `task.*` document).

## 10. Out of scope and risks

- `journey explore` (#356) and the Studio Approve button (#353) are separate; this spec reads
  their artefacts and adds none.
- Rule 1 is only as good as the journey map. A screen with no journey is unproven, and the planner
  says so (`proof: journey` with `journeys: []` is reported as `OBSERVER_MISSING`); the temptation
  is to call it docs-only. The path classes make that a policy diff, not a judgment call.
- Replay is slower than a unit test (seconds per path, a browser per path). Phase E measures it on
  this repository before Rule 1 becomes more than a signal.
- Open live keeps a browser and a driver process alive on the Runtime host; the whole-run timeout
  and owner-only access bound it. An agent session may not open a flow it authored under review,
  so "I opened it and it passed" is never the author's own proof.
- Jev's thresholds are placeholders until a recorded run (the architect-judgments spec says the
  same); until then `fallback_strict` is the common case and is recorded as such.
