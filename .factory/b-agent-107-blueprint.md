# #107 — Graph Architect blueprint (design-only)

Author: B, 2026-08-19. Feeds the M11 decision. Base for all citations: main `e0849e8`
(PRD sections by number; code by path). CITE-or-MARK: uncited design choices are marked
DECISION with grounds; anything unverified is marked.

The gap, in the issue's words: nothing synthesizes a graph from a prompt — the product's
first move ("user requests a feature; system generates Graph v1") is unstartable (#107).
The PRD promises it at §1 (harness "compiles a graph specific to that task"), §9 (the
main-flow box `G[Graph Architect]`), §10 (the harness manifest), and the vision-fulfilled
criterion §25 steps 5–6 ("see a customized graph get compiled"; "understand each agent,
model, context, permission, and gate").

## 0. The one-sentence design

**The Graph Architect is a compiler with a model in the middle: it consumes a goal and a
capability catalog, asks the gateway for a DRAFT, and emits an ExecutionGraph DOCUMENT
that enters the system through the exact load → lint → publish path every authored file
already uses — the model can propose anything and bypass nothing.**

## 1. What exists today (the parts the architect snaps into)

| capability | where | state |
|---|---|---|
| Graph DSL + schema validation | `core/schema` `load_graph(file)` (used by serve's `load_and_publish`, routes.rs:169) | EXISTS |
| Graph lint | `graphhelm_graph::lint(&graph, &source)` (routes.rs:175) | EXISTS |
| Safe publication (the governor's path) | `publish_loaded(&loaded, owner)` → `GraphVersionPublished` events, semantic hash, active version | EXISTS |
| Node-type viability | `core/runtime/src/classify.rs:28` `work_kind` (agent/planner/classifier/evaluator=Cognitive, tool, gate; ten types refused) | EXISTS |
| Execution | `execution start --file --mode`, `drive_to_quiescence(_async)`, serve routes | EXISTS |
| Model access | gateway (PRD §15), BYOK routes, `ServeModelPort::build` + `CredentialBroker` (serve/ports.rs; read-only lease semantics measured in #81's lane) | EXISTS |
| Judge pattern (semantic evaluation) | evaluator node with `judge` payload (judgeId, userStory, mcpSurface) — the M07–M09 blind-judge runs; `serve --read-audit` makes the judge auditable | EXISTS |
| Capability catalog | PRD §10.1 names it as a harness input | DOES NOT EXIST as data — v1 stubs it (see §4) |
| Task profiler | PRD §9 box `C` | DOES NOT EXIST — v1 folds a minimal profile into the architect (see §3) |
| Agent synthesis / registry | PRD §12.2, schemas/agent.schema.json | schema EXISTS, runtime does not — OUT of slice 1 |

The load-bearing fact: **the trust boundary already exists and is already guarded.** Serve
will not drive a graph that does not load, lint, and publish (routes.rs `load_and_publish`),
and `classify.rs` refuses node types the runtime cannot execute. The architect does not need
its own safety story at the boundary — it needs to be FORBIDDEN from having one (D-039: one
path; a second entry path would put generated graphs on a less-guarded road than authored
ones, which is backwards).

## 2. Component design

### 2a. What consumes the prompt

`GraphArchitect::synthesize(goal: &str, profile: TaskProfile, catalog: &CapabilityCatalog,
model: &dyn ModelPort) -> Result<SynthesizedGraph, ArchitectRefusal>`

- **Lives in a new crate `core/architect`** — DECISION, grounds: it is product logic
  (prompt assembly, draft validation, repair loop) that both surfaces (CLI, serve) must
  share; putting it in apps/cli would force serve to shell out or duplicate (the
  resume-path lesson: shared shape or two drifting copies), and putting it in an existing
  core crate would couple it to a layer (schema, graph, runtime) it merely CONSUMES.
- **TaskProfile v1 is minimal and honest**: `{goal, mode (autopilot/supervised/manual),
  risk_hint: Option<enum>, max_nodes: u8}` — the PRD's full profiler (§9 box C, §10.1's
  ten inputs) is explicitly NOT built in slice 1; the profile is caller-supplied with
  defaults. MARKED: the profiler is its own component later; folding a stub in now and
  naming it a stub beats faking classification.
- **CapabilityCatalog v1 is a static description of what the runtime can actually run**:
  the classify.rs-admitted node types, the registered tools (the tool broker's program
  allowlist from the serve wiring), and the gateway's route ids. Generated FROM code
  facts, not hand-authored — a catalog that drifts from `classify.rs` produces graphs the
  runtime refuses. (Survey note, cited: composio's meta-tools runtime discovery is the
  eventual shape — `.factory/b-agent-harness-survey.md` B1 — but discovery-at-synthesis
  only matters when catalogs are big; slice 1's catalog fits in one prompt.)

### 2b. What it emits, and via which validation path

**It emits the DSL document** (the `ExecutionGraph` YAML the repo already speaks —
apiVersion p50.dev/graph/v1), never an in-memory spec struct handed directly to
execution. DECISION, grounds:

1. **D-039 single path**: document → `load_graph` → `lint` → `publish_loaded` →
   `GraphVersionPublished` → execution — byte-for-byte the road authored graphs take.
   Nothing downstream learns a new entry point; the sealing/publication guarantees hold
   because the input class is unchanged.
2. **Auditability**: the emitted file IS the artifact. §25 step 6 ("understand each
   agent, model, context, permission, and gate") is served by a document a human reads
   and diffs, and the export/reproducibility principle (§4.10) gets the graph for free.
3. **The repair loop needs the validators' own diagnostics**, and those speak
   document+source-span (lint takes `&source`).

The struct `SynthesizedGraph { document: String, rationale: Vec<NodeRationale> }` — the
rationale (why each node exists, which capability it consumed) is architect metadata that
does NOT enter the graph document (the document stays schema-clean) but rides the reply
for the Studio/operator surface later. MARKED: rationale format is a v1 sketch; the
Studio consumer does not exist yet.

### 2c. The synthesis loop (compiler discipline)

```
prompt-assembly(goal, profile, catalog)         deterministic, versioned template
  -> model.draft()                               ONE gateway call, structured output
  -> validate: load_graph(bytes)                 schema — deterministic, existing
           && lint                               structure — deterministic, existing
           && all nodes classify-viable          runtime honesty — existing fn
           && node_count <= profile.max_nodes    smallest-sufficient (§4.2)
  -> if invalid: repair(diagnostics) -> model    at most K=2 repair rounds
  -> if still invalid: ArchitectRefusal          REFUSED, diagnostics attached, never
                                                 a silently "best effort" publish
```

- The model call goes through the EXISTING gateway/BYOK seam (`ModelPort`), same as node
  execution — no second credential path (§4.7; the #81 lane just measured this seam's
  lease semantics as read-only).
- The refusal arm is load-bearing: a draft that cannot pass the validators after K
  repairs is refused WITH the diagnostics, because "the model could not produce a valid
  graph for this goal" is an answer the operator must see, not a graph quietly degraded
  until it parses. Absence never laundered into calm — the product's own rule.
- Synthesis is a PURE FUNCTION of (template, goal, profile, catalog, model responses).
  That sentence is the entire test story's foundation (§3).

### 2d. Surfaces

- **CLI**: `graphhelm graph synthesize --goal "<text>" [--profile ...] --out graph.yaml`
  — writes the document, prints diagnostics + rationale. Does NOT publish or start.
- **Serve**: `POST /v1/graphs/synthesize {goal, profile?}` → `{document, rationale,
  diagnostics}`. Does NOT auto-publish in slice 1 — DECISION, grounds: §4.6 human
  sovereignty ("the harness governs the default; the owner can force") and §25 step 5's
  verb is SEE the graph compiled, not have it executed behind your back. The existing
  `start`/`resume` routes then take the document exactly as they take authored files.
  Autopilot-mode auto-publish is a later, policy-gated addition (§9.2's shape).

## 3. The guard story — testing a component whose output is model-generated

Two tiers, priced separately, and the boundary between them stated so neither pretends to
be the other:

### Tier A — deterministic, cheap, every commit (the compiler half)

The purity sentence (§2c) makes this possible: record the model's responses, replay
synthesis without a model.

- **Golden-prompt suite with keyless replay fixtures** (the deepseek-harness pattern,
  survey §1 ALREADY-HAVE-adjacent): N recorded (goal, model_response) pairs; assert the
  emitted document loads, lints, classifies viable, respects max_nodes, and is
  BYTE-STABLE against the fixture. Runs in CI with zero keys, zero cost, deterministic.
- **Validator-refusal sabotages, observed red at their own assertions** (the standing
  factory rule): fixtures where the recorded model response is (i) schema-broken, (ii)
  lint-dirty, (iii) contains a `deploy`-typed node (classify-refused — and the fixture
  should use a node NAMED innocently, per the reader-flattening lesson), (iv) exceeds
  max_nodes. Each must surface as ArchitectRefusal after K repairs — and one fixture
  where repair round 1 SUCCEEDS, proving the loop repairs rather than only refuses
  (the absence-family presence member).
- **Template versioning**: the prompt template is content-hashed and the hash rides the
  rationale; a template change that shifts fixture outputs is a NAMED event, not drift.
  (The digest-catalog discipline, applied to prompts.)
- Cost: milliseconds per test; the entire tier is free forever. PRICED: ~0.

### Tier B — semantic quality, expensive, per-milestone (the judge half)

Schema-valid says nothing about USEFUL. The existing blind-judge pattern is the
instrument the repo already trusts for exactly this question (M07–M09: nine paid runs,
auditable via serve --read-audit).

- **Shape**: judge receives the goal + the synthesized graph (+ the catalog) and scores:
  does this graph, executed, plausibly satisfy the goal; is every node earning its place
  (§4.2 smallest-sufficient); is anything missing that the goal demands. Blind: the
  judge never sees the architect's rationale (independence, §4.4).
- **Cadence**: per milestone close, not per commit — PRICED: one paid judge run per
  milestone (the M09 economics; m09-seeds.md's own warning that the recorder makes the
  judge auditable but NOT free is the governing citation).
- **What Tier B must never become**: a CI gate. A paid, nondeterministic judgment in the
  commit loop is a flake generator with an invoice. Its findings become issues/seeds
  (the M07–M09 practice), not reds.

MARKED (honest limit): between A and B there is a gap — deterministic tests cannot say
"this graph is good", the judge cannot run often. The gap is narrowed, not closed, by
property assertions in Tier A that encode judgeable structure (every graph has ≥1
verification-shaped node — gate or evaluator — when profile.risk_hint ≥ medium, etc.).
These encode POLICY, not quality, and are listed as such.

## 4. Smallest end-to-end slice (M11 proposal: "The First Compile")

**Slice: prompt → 2-node graph → executed.**

`graphhelm graph synthesize --goal "check that the repo builds and summarize the result"`
→ document with `build_check` (type: tool, the runtime's shell/tool path) →
`summarize` (type: agent, one cognitive node) → operator runs
`graphhelm execution start --file <it>` (or the serve route) → drive to quiescence →
status shows completion with evidence.

Measured acceptance (RA §25 step 5, made falsifiable): from a clean checkout with a BYOK
key, one CLI command produces a graph document that loads, lints, publishes, and
executes to quiescence — demonstrated in an acceptance run recorded like the M0x
demos (docs/acceptance pattern), with the golden suite green keylessly in the gate.

**Named NON-goals of the slice** (each is real work the slice must not pretend to
include):
1. Task Profiler as a component (§9 box C) — profile is caller-supplied v1.
2. Capability Discovery beyond the static generated catalog (§9 box D; survey B1).
3. Agent Matching/Synthesis (§12.2) — nodes use the runtime's existing executor kinds.
4. Context Plan / per-node capsules (§10.2, §13.1) — nodes get what execution gives
   them today.
5. Policy Enforcement beyond lint + classify (§9 box H).
6. Simulation before publish (§9 box I).
7. Ghost nodes / governor expansions mid-run (§9.2).
8. Studio rendering of rationale (§17).
9. Autopilot auto-publish (policy-gated, later).
10. Multi-draft judge panels at synthesis time (a quality lever, priced out of slice 1).

Each non-goal maps to a PRD box that stays unstartable until its own slice — the point
of naming them is that #107's gap-map successor issues can cite this list instead of
rediscovering the boundary.

## 5. Risks the design must carry visibly

- **The catalog lies** (drifts from classify.rs / tool allowlist) → generated graphs the
  runtime refuses at dispatch. Mitigation: catalog GENERATED from the same functions the
  runtime consults, plus a Tier-A test asserting catalog ⊆ classify-admitted. (The
  flattening family: a catalog is a boundary; its drift is a confident wrong reading.)
- **Repair-loop laundering**: K rounds of "fix it" can converge on a graph that
  satisfies validators while abandoning the goal (the vacuous-green shape, one level
  up). Mitigation: the rationale must restate the goal-to-node mapping and Tier B judges
  it; Tier A asserts the repair loop never DROPS nodes below the goal's stated minimum.
  MARKED: this is the design's weakest guard — priced honestly as judge-only.
- **Prompt-injection via goal text**: the goal is operator-supplied in slice 1 (trust
  boundary = the owner), but the moment goals arrive from issues/webhooks, the
  architect's template must treat goal text as data (the ai-memory survey lesson:
  retrieved text never gains instruction authority). Named now so the surface that
  changes the trust boundary meets a written warning.
- **Cost surprise**: one synthesize = one model call + up to K repairs. The reply
  carries token counts (gateway already meters); the CLI prints them. §10.2's cost
  estimate output is NOT in slice 1 — marked.

## 5b. Seam with #110 (agent registry) — recorded on both sides

H's registry blueprint (.factory/h-agent-110-blueprint.md) defines the boundary we agreed:
**registry = the SHELF** (AgentRecord as identity envelope over unmodified
agent.schema.json; catalog as store projection; `name@version` refs resolved once at
governor publish), **architect/matcher = the SEARCHER**. Consequences for this design:

- When Capability Discovery enters (non-goal 2) it CONSUMES the registry projection's
  list/query as the discovery interface and adds nothing beneath it — PRD §12.3's
  reuse/parameterize/derive intelligence belongs to the searcher, not the shelf.
- `node.schema.json:14` already ships the `{ref}` binding variant unresolved (H's cite).
  Slice 1's synthesized agent nodes are EPHEMERAL (today's inline path). The
  register-then-ref option — synthesized agents gaining provenance for reuse — is a
  later architect decision that H's shelf merely makes possible; it is named here so
  neither lane invents the other's half.

Milestone sequencing (owner + orchestrator's call, per the issue), the prompt template's
actual text (an implementation artifact that will iterate under the golden suite), crate
naming beyond `core/architect` (bikeshed), and whether slice 1's serve route ships behind
a feature flag (deployment posture — orchestrator's lane).
