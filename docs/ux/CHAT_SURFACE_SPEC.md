# Chat surface: the host plugin and its operator skills

## 1. Objective

D-039 made chat a first-class surface: a coding agent (Claude Code, Codex — anything that
speaks MCP) operates GraphHelm from inside the conversation. This document specifies that
surface as a product: the layering, the operator skill catalog, the multi-agent choreography,
and the onboarding flows. The Milestone 05e plan implements the contract layer; the skills
arrive with it and grow after it.

The rule above every section: **the chat surface is never a second Runtime path.** Every Runtime
read, mutation, or privileged capability used by a skill maps to public Runtime API calls the CLI
can make identically. Local advisory artifact authoring follows checked-in schemas and skill
instructions; it creates no Runtime authority and is not a private operational capability.

## 2. Layering

| Layer | What it is | What it may contain |
|---|---|---|
| MCP server | Public Runtime API adapters plus the local CLI-parity `wake_wait` helper; stateless — no store handle or driver, with notifications derived from the events tail | protocol adaptation and bounded local waiting only |
| Claude Code plugin | packaging: the MCP server registration plus the operator skills below | skills, slash-commands, docs |
| Codex configuration | packaging: MCP registration for Codex | configuration only |

Wrappers are thin by construction. A capability that exists only in one host's wrapper is a
defect; the test is deleting the wrapper and losing nothing but convenience.

## 3. MCP tool vocabulary (the 05e base)

> **This section records the 05e BASELINE, not the present surface.** The vocabulary has
> grown since: #223 added the six development operations, #288 added `sweep`, and #105 added
> `list`. The authoritative list is `TOOLS` in `apps/cli/src/commands/mcp/tools.rs`, pinned by
> name in `apps/cli/tests/mcp_stdio.rs`; a count written in prose here cannot be guarded and
> so is not maintained. What the paragraph below still states correctly is the SHAPE every
> tool obeys, which has not changed.

As of 05e the server exposed fourteen tools: `start`, `status`, `events` (paged tail), `signal`,
`approve`, `pause` (graceful and immediate), `resume`, `cancel`, `routes`, `wake_arm`,
`wake_status`, `amend_budget`, `wake_wait`, and `probe`. Thirteen adapt public Runtime API
operations. `wake_wait` is the one bounded local helper and preserves CLI parity. Every mutating
tool carries the three mutation headers (idempotency key, actor, actor type) and optional
`If-Match`; the MCP layer generates idempotency keys per logical act, never per retry. Future Living
Documentation tools remain out of this current vocabulary until their public API exists.

## 4. Operator skill catalog

Skills are named, versioned, and shipped with the plugin. Each one states what it reads, what
it mutates, and which API calls it choreographs.

### 4.1 `onboard-new-project`

Guided zero-to-first-execution for a repository that does not know GraphHelm yet:

1. verify runtime reachability (health) or guide the VPS connect (D-002);
2. register credentials through the broker flow — values via the CLI's stdin path, never
   through chat text (a pasted secret in a conversation is a leak; the skill says so and
   refuses to accept one inline);
3. author the first graph from the project's objective (template-assisted, linted);
4. seed rule documents: interview the operator for the 3–5 business rules that matter,
   materialize them as `docs/rules/` candidates (§12.5) with `verified_by` stubs;
5. first execution, observed end to end through `status`/`events`.

### 4.2 `onboard-existing-project`

For a repository with history: everything above, plus discovery before authoring — map the
repo (conventions, test layout, deploy scripts), propose rule-document candidates from what the
code already enforces, and propose the initial graph from the discovered shape. Discovered
claims enter as candidates, never as validated truth (§11.3 lifecycle; the threat model's
knowledge-poisoning rules apply to onboarding exactly as to Dreams).

### 4.3 `operate-execution`

The operator loop as one skill: start → watch the tail → surface triage items (blocked nodes,
pending approvals, capacity waits) → act (`approve`/`signal`/`pause`/`resume`/`cancel`) →
report outcome with evidence references. Immediate-stop is explicit and confirmed; cancel
states its partial-effects consequence (§13) before acting.

### 4.4 `observe-agents`

Multi-agent visibility, read-only: who is acting on this project right now and what did they
do — derived entirely from the events tail's attribution (every mutation carries its actor,
05a's contract). The skill renders the interleaved actor timeline, flags conflicts (409s,
stale `If-Match` losers), and never impersonates: it reads other actors' work, it cannot act
as them.

### 4.5 `invoke-agent`

Start or resume work under this chat's own actor identity: dispatch an execution, approve a
ghost, hand a blocked node an owner decision. Constraints carried from the register: a
replacement agent never starts automatically after a manual intervention (D-020) — invocation
is always an explicit act with the actor recorded; sovereignty acts (waivers, gate removals)
follow D-019 with the waiver recorded.

### 4.6 `share-context`

Sharing between chats and agents without pretending the runtime is a chat store:

- the durable sharing channel is the event log: signals, decisions, evidence references —
  what another agent needs travels as recorded acts, readable by every other session through
  `events` (the 05a two-agent loop is the reference behavior);
- a chat transcript worth preserving is **externalized as Evidence** attached to the
  execution it informed (sealed, sensitivity-classified, D-036: free-form never in an event
  payload) and referenced by id — another chat retrieves it by reference, under scope;
- the skill never copies transcript text into event payloads, rule documents, or claims; it
  proposes candidate claims extracted from the conversation instead, each pointing at the
  transcript Evidence as provenance.

### 4.7 `triage-and-approve`

The approval queue as a conversation: list everything awaiting an owner decision across
executions (blocked nodes, ghosts, gate failures, waiver requests), show each item's evidence
refs, act one by one. Refuses bulk-approve without per-item display — an empty approval
without inspection fails the completion contract (§22.1), and the skill is not a bypass.

### 4.8 `deploy-and-verify`

The `QUALITY_GATES_AND_DEPLOYMENT.md` flow driven from chat: profile the change, name the gate
set it selects, run the full tier on the VPS, deploy on green, run post-deploy verify, report
— and on red, stop with the failing gate's evidence, never "deploy anyway" without the
explicit waiver path (D-019, recorded).

### 4.9 Journey-Proven Development bundle

The optional built-in `graphhelm-jpd` data extension adds eight entry skills: `journey-contract`,
`observation-compiler`, `plan-council`, `defect-bounty`, `skill-synthesizer`, `skill-evaluator`,
`retry-provenance`, and `journey-verifier`.

Runtime reads, mutations, and privileged capabilities obey the same deletion and parity rule as the
operator skills. Local advisory artifacts are reproducible from the published package schemas and
instructions; they are not represented as Runtime operations that do not exist. MCP is preferred
when the chat is attached to a Runtime; the CLI is the local/offline path. A skill never switches
surfaces after an uncertain mutation. Browser behavior requires an independently installed observer
capability; without it the skill returns `OBSERVER_MISSING`, not a guessed success.

## 5. Multi-agent choreography

The rules that let N chats work one project without torn state — all inherited from 05a's
contract, restated here as operator-facing behavior:

- every chat session is a distinct actor; every mutation is attributed;
- optimistic concurrency by default: mutate with `If-Match`, re-read and retry once on 409;
- the events tail is the coordination channel — agents learn of each other's acts by reading,
  never by side channels;
- long or machine-saturating operations (full gates, deploys) are announced as signals before
  starting, one at a time per machine;
- a chat never holds state the API cannot reconstruct: closing the conversation loses nothing
  but the conversation.

## 6. Constraints

- **Parity (D-039):** every Runtime read, mutation, and privileged skill capability must be
  reproducible as documented CLI/API calls. Local advisory authoring is reproducible from published
  schemas and instructions and grants no Runtime authority.
- **Statelessness (§6.4):** the MCP server holds no store handle and no driver.
- **No secret through chat:** credential values travel only through the broker's stdin/env
  paths; a skill that receives a pasted secret refuses and instructs, and never echoes it back.
- **Subscriptions (D-016):** capacity exhaustion waits; no skill silently switches to BYOK
  spend.
- **Sovereignty with a record (D-019/D-020):** overrides always possible, always recorded,
  never automatic.
- **Free-form discipline (D-036):** transcripts and model text externalize as Evidence, never
  into payloads.

## 7. Acceptance criteria

- deleting the plugin loses no Runtime or privileged capability; advisory local artifacts remain
  reproducible from the published schemas and instructions;
- two chat sessions on one project observe each other's acts through `events` alone, fully
  attributed, and resolve a mutation race through a 409 + retry;
- both onboarding skills produce a running first execution and at least one rule document
  candidate; the existing-project flow proposes claims as candidates only;
- a shared transcript exists as sealed Evidence referenced from its execution, and no event
  payload contains transcript text;
- a pasted credential in chat is refused and never echoed;
- every skill's documentation names each API/CLI call it actually choreographs.
