# #110 blueprint — Project Agent Registry (smallest version)

H Agent, 2026-08-19. Read-only design; base `origin/main @ 0fb0e66`, all citations
against it. Feeds M11 beside B's #107 work — cross-reference:
`.factory/b-agent-107-blueprint.md` (B's #107 blueprint, which supersedes the earlier
harness survey; its §5b "Seam with #110" records the shelf/searcher contract from the
other side — this file deliberately stops where the Matcher starts).

## 0. The load-bearing discovery: the wire already promised this

`schemas/node.schema.json:14` (and `schemas/releases/1.0.0/node.schema.json:14`)
defines the agent binding as `oneOf`:

- `{ "ref": "<string>" }` — legal on the wire TODAY, and
- `{ "ephemeral": <agent.schema.json> }` — the only variant any example or code uses.

Grep across `core/protocols/src`, `core/schema/src`, `core/execution/src` for
ref-resolution: zero hits — `agent.ref` validates and then resolves to NOTHING.
The smallest registry is therefore not a new concept: **it is the missing resolution
target for a contract the schema already ships.** No node-schema change is required
to introduce it (wire-compat rule preserved; see §5's one open question).

## 1. What an agent DEFINITION is (PRD §12.1/§12.2 read exactly)

PRD §12.1: "Agent Definition: persistent, versioned configuration" — distinct from
Agent Runtime (instance in a node) and Agent Experience (history). §12.2 lists the
declared fields; **`schemas/agent.schema.json` already normatively encodes that list**
(purpose, capabilities, allowedTools, prohibitedActions, input/outputSchema,
instructions|instructionsRef, modelRequirements, contextStrategy, completionContract,
evidenceRequirements, memoryPolicy, isolationMinimum).

**Decision: the registry invents NO new definition shape.** A registry entry is an
IDENTITY + PROVENANCE envelope around an unmodified `agent.schema.json` document:

```
AgentRecord {
  name:         string            # unique within project, kebab-case
  version:      u64               # monotonic per name, registry-assigned
  definition:   <agent.schema.json document, verbatim>
  contentHash:  sha256 of canonicalized definition
  registeredBy: PersistedActor    # who (core/protocols persistence types)
  status:       active | deprecated
}
```

Model binding stays INSIDE the definition (`modelRequirements`) as a requirements
profile, never a hard provider pin — route selection remains the gateway's job
(core/gateway eligibility), same as for ephemeral agents. Tool grants likewise stay
`allowedTools` + broker leases at execution time; the registry GRANTS nothing
(PRD §4.7: permission by capability/scope/time — a catalog row is not a grant).

## 2. Where it lives: files author, events own

Two layers, one truth:

- **Authoring surface: files in-repo** (`agents/<name>.yaml`, sibling to
  `examples/manifests/` style) — human-diffable, reviewed like code. Files are INPUT
  to registration, never a runtime source.
- **Operational truth: the Event Store.** Registration appends a new event kind
  (`agent_registered { name, version, contentHash, definition }`;
  `agent_deprecated { name, version, reason }`). The registry catalog is a replay
  projection — disposable and rebuildable, per the store invariant (AGENTS.md:
  projections never rewrite history). Re-registering an identical definition is a
  no-op (contentHash match); a changed definition bumps `version`.

Why not files-only: an execution that reads `agents/*.yaml` at run time answers "which
agent ran?" from a mutable working tree — unprovable after the fact, and exactly the
class of silent drift the store exists to kill. Why not events-only: authoring in an
append-only store without a diffable surface makes review impossible. Both, with the
event as truth, is the smallest honest shape.

**Scope note (marked):** whether `agent_registered` lands in the project's execution
store or a project-scoped registry stream is an implementation choice for the
implementer; the invariants (append-only, replayable, hash-linked) apply either way.

## 3. How an execution references one

`type: agent` node, `agent: { ref: "implementer@3" }` — name@version, both halves
required in v1 (**no floating refs**: `ref: "implementer"` resolving to "latest" makes
the same graph mean different things on different days, violating PRD §4.10
reproducibility). Resolution happens ONCE, at graph publish/draft-apply time
(core/governor): the governor resolves name@version against the registry projection,
and records the resolved `contentHash` in the graph version's metadata, so the
published version is self-describing and replay never re-resolves. An unresolvable
ref is a publish-time diagnostic (lint class), never a run-time surprise.

Execution then treats a resolved ref EXACTLY as it treats `ephemeral` today — same
`agent.schema.json` document flowing into the same node machinery. The executor gains
no new branch; the governor gains a resolution step.

## 4. D-039: no second path (decision register D-039, routes.rs:2 doc comment)

- Register/list/show/deprecate are commands in the ONE command layer
  (`apps/cli/src/commands/`), exposed identically over CLI, HTTP serve routes, and
  MCP — same pattern as the execution lifecycle. Nothing the registry does is
  reachable only from one surface.
- Execution-time resolution reads the store projection through the same open-store
  path every other read uses (`resolve_stream`-class reads), NOT the filesystem and
  NOT a private cache with its own lifecycle.
- The governor remains the only writer of graph mutations; registry events are not
  graph mutations, but their CONSUMPTION (ref → contentHash pinning) happens inside
  the governor's existing publish transaction, so no mutation path bypasses it.

## 5. Explicitly NOT in the smallest version (named, per order)

- **Marketplace / remote registries / federation** — RA §3.3 puts marketplace out of
  Phase 1 entirely.
- **Signing / trust chains** — a registry entry's integrity rides the store's own
  hash-linking; cryptographic authorship is a later ADR.
- **Agent Matcher** (PRD §12.3: search/reuse/parameterize/derive) — that intelligence
  is #107's harness; this registry is the shelf it searches, not the searcher.
  Cross-ref: B's #107 file should consume `AgentRecord` + the projection's list/query
  as its discovery interface and add nothing beneath it.
- **Agent Experience** (PRD §12.1's third leg — memories, evaluations, history) —
  depends on Knowledge Graph (#111) machinery that does not exist.
- **Performance-history-driven selection** — PRD §12.3 itself warns history never
  replaces compatibility verification; v1 records nothing to be tempted by.
- **Definition editing/inheritance** (`derive`) — new version = whole new document.

## 6. Open contract question (the one thing needing an owner/ADR decision)

Pinning the resolved `contentHash` into a published graph version touches the graph
version's metadata surface. If it rides `labels`/metadata (UI-only class), the
semantic hash is unchanged but the pin is semantically load-bearing — uncomfortable.
If it enters the hashed surface, the same DSL text produces different semantic hashes
under different registry states — correct for reproducibility, but a canonicalization
change (AGENTS.md: preserve `p50.dev` wire identifiers until a compatibility ADR
rules). **Marked: needs the compatibility-ADR route either way; this blueprint takes
no position beyond naming the fork.**

*Cite-or-mark: schema/line citations verified at 0fb0e66; "zero hits" greps are
best-effort negatives; PRD line references from the 712-line file at the same base.*
