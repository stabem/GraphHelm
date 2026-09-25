# Graph Architect — design (the first compile)

Base for every citation: `origin/main` @ `585fa0c2` (2026-09-11). Issue anchor: #107 ("nothing
synthesizes a graph from a prompt"). Consumed as decided: design note #107
(B, 2026-08-19; `docs/process/DELIVERY.md`, History) and the two contradictions the M11 consolidation named against it, #183 and
#184. PRD §10 (dynamic harness), §24 (first vertical slice), §25 steps 5–6; `docs/harness/
HARNESS_SPEC.md` §4 (the compilation pipeline, of which this slice builds ONE box, the Graph
Architect, with a caller-supplied profile and a code-derived catalog).

## 1. What exists, and what does not

| capability | state | receipt |
|---|---|---|
| Graph DSL load + schema validation | exists | `core/schema/src/document.rs:156` `load_graph_json(bytes, source)`, `:161` `load_graph(path)` (`.json`/`.yaml`/`.yml`) |
| Lint, including `GHG102_UNBOUNDED_CUSTOMS` on parkable nodes with no customs block | exists | `core/graph/src/lint/mod.rs:21`, `:206-213` |
| Node viability for the real executor | exists | `core/runtime/src/classify.rs:28` `work_kind` — Agent/Planner/Classifier/Evaluator/Tool/Gate; ten types refused |
| Tool node contract | exists | `core/tool-broker/src/call.rs:23` `ToolCall` (`repository`/`shell`/`tests`), `ShellAction { program, arguments }`; the driver reads `properties["tool"]["call"]` (`core/runtime/src/driver.rs:492-500`) |
| The program allowlist as a security surface | exists | `serve --allow-program` → `RuntimeWiring::allow_programs` → the tool lease's `programs` (`serve/routes.rs:1602`) |
| Model access | exists | `ModelPort` (`core/runtime/src/ports.rs:23`), `ServeModelPort::build` (`serve/ports.rs:93`), `ByokAdapter::{new,call}` and `RuntimeAdapter::{new,call}` (`adapters/model-gateway/src/{byok,runtime}.rs`), `ModelCall { prompt, max_tokens }` / `ModelReply { text, usage }` (`core/gateway/src/call.rs`) |
| Execution of an authored document | exists | `execution start --file`, `POST /v1/executions/{id}/start` |
| The customs block a synthesized node must carry to be completable | exists | `schemas/node.schema.json:124` `completion.customs { proofKinds, budgets { waitWithinSeconds, clearanceWithinSeconds } }` |
| **Anything that reads a goal and emits a graph** | **absent** | `grep -ril architect core apps/cli/src` → only unrelated hits in governor/graph persistence |
| Capability catalog as data | absent | PRD §10.1 names it; nothing produces it |
| Task profiler | absent | HARNESS_SPEC §6; not built here (caller-supplied profile, D3) |

## 2. Decisions (orchestrator, owner's delegated authority)

D1. **The architect is a compiler with a model in the middle, in a new crate `core/architect`.**
`synthesize(profile, catalog, model)` assembles a deterministic prompt, asks the model for ONE
draft, validates through the SAME chain authored graphs take (`load_graph_json` → `lint` →
viability), repairs at most K=2 times by feeding the diagnostics back, and REFUSES with the
diagnostics attached when the third draft is still invalid. It never publishes and never starts.
The document it emits enters the system through `execution start --file` exactly as an authored
file does (D-039: one road).

D2. **Every synthesized graph is born completable (resolves #183).** After the model's draft
parses, the compiler STAMPS `completion.customs` — `proofKinds: []` and the profile's two budgets
— onto every node that can park for input and has no customs block, and records which nodes it
stamped in the rationale. The lint warning `GHG102_UNBOUNDED_CUSTOMS` is then treated as a
FAILURE of synthesis (never a warning): a synthesized graph carrying the 0/4 defect is refused,
not demonstrated. A test that removes the stamping step must go red on the golden goal.

D3. **The catalog is read from the runtime and the architect REFUSES goals outside it
(resolves #184).** `CapabilityCatalog::from_runtime(programs)` derives the executable node types
from `classify::work_kind` over every `NodeType` variant (an exhaustive list guarded by a test that
breaks when a variant is added), the three `ToolCall` families, and the operator's declared
program allowlist. A draft whose shell call names a program outside the allowlist is refused with
`ArchitectRefusal::CapabilityMissing { node, program }` — the allowlist is never widened, and the
refusal names what the operator would have to authorize. The slice's own example goal ("check
that the repository builds and summarize the result") needs `cargo`, so the golden run passes
`--allow-program cargo` explicitly rather than relying on a default.

D4. **Profile v1 is caller-supplied and minimal.** `TaskProfile { goal, mode, max_nodes (default
6), wait_within_seconds (default 86400), clearance_within_seconds (default 3600) }`. The PRD's
profiler (§9 box C) is named as not built.

D5. **Metadata is the compiler's, not the model's.** The model is asked for `spec` only
(`entrypoints`, `nodes`, `edges`, `budgets`, `completion`). The compiler writes `apiVersion`,
`kind`, and `metadata { id: "arch_<sha8 of goal>_v1", name: <goal, first 80 chars>, executionId:
"exec_<sha8>", version: 1, labels { origin: "architect", template: <template sha256> } }`, so two
runs on one goal produce one identity and the fixture suite is byte-stable.

D6. **The prompt template is versioned by content hash.** `template::TEMPLATE` is a `const &str`;
`template_sha256()` rides every reply and the emitted document's labels. The golden fixtures are
keyed by the sha256 of the FULL assembled prompt, so a template change invalidates the fixtures
by name (a named event, never drift): the refusal `FixtureMissing { prompt_sha256 }` prints the
hash the operator must record.

D7. **Two model doors, both real, one keyless.** `DraftModel` is the architect's own port:
`fn draft(&self, prompt: &str) -> Result<DraftReply, ArchitectRefusal>` with `DraftReply { text,
usage }`. Adapters: (a) `RecordedDraftModel` — a JSON file `{"replies": {"<prompt sha256>":
"<text>", …}}`, the provider-less door, used by every test and available to operators as
`--fixture`; (b) in `apps/cli`, an adapter over the gateway's `ByokAdapter`/`RuntimeAdapter`
(same construction `gateway probe` uses; passphrase from the environment, credential leased from
the broker) for `--manifest --route`; on `serve`, over `ServeModelPort` built from the runtime
wiring. No new credential path.

D8. **Surfaces.** CLI `graphhelm graph synthesize`; HTTP `POST /v1/graphs/synthesize`; MCP tool
`synthesize` (appended after the customs verbs Track A adds). All three return the same JSON:
`{ document, rationale: [{node, reason}], stampedCustoms: [node…], templateSha256, rounds,
promptSha256s: [..], usage: {inputTokens, outputTokens}|null }`. The CLI additionally writes the
document to `--out <path.json>` (refusing to overwrite an existing file). None of the three
publishes or starts.

D9. **Tier B (semantic quality by a paid judge) is out of the commit loop** and named in
`docs/harness/GRAPH_ARCHITECT.md` as the next measurement; Tier A (deterministic golden +
sabotage) is the gate.

D10. **No schema changes, no new dependencies.** The emitted document uses only fields the
checked-in schemas already accept.

## 3. The prompt contract (what the model must return)

The template asks for a JSON object with exactly the `spec` keys, and states: the allowed node
types and the shape each needs (`agent` → `agent.ephemeral { purpose, capabilities[],
inputSchema, outputSchema, completionContract { requires[] }, instructions }` — the fields
`agent.schema.json` requires; `tool` → `tool.call` as one of `{"tool":"shell","program":P,
"arguments":[…]}` with P from the catalog, `{"tool":"tests","arguments":[…]}`,
`{"tool":"repository","action":"read_file"|"list_files"|"diff",…}`); every node needs `name`,
`objective`, `optionality: required`; edges are `{id, from, to, type: "control"|"data"}`;
`budgets.maxNodes` ≤ the profile's; `completion.terminalNodes` names the last node(s); the
smallest graph that satisfies the goal; output JSON only, no prose. On a repair round the
template appends the previous draft's diagnostics (`code`, `pointer`, `message`) verbatim.

## 4. The first compile — end to end

Goal: *check that the repository builds and summarize the result.* Expected shape: a `tool`
node `build_check` (`shell cargo build`) → a `agent` node `summarize`, both stamped with customs.
The acceptance is a CLI journey test: synthesize with the recorded fixture → `--out` document →
`execution start --file <it> --fixtures {build_check: success, summarize: success}` → status
`completed`, and `graph lint` on the document reports zero `GHG102`. Recorded as
`docs/acceptance/m11-first-compile-2026-09-11.md`, with the exact fixture and the document's
content hash.

## 5. Tests (Tier A)

| cell | where | what goes red |
|---|---|---|
| Golden: the recorded reply for the goal compiles to a byte-stable document (`expected.json`) | `core/architect/tests/golden.rs` | any nondeterminism in assembly, stamping or metadata |
| Sabotage fixtures, each refused after K repairs UNDER ITS OWN diagnostic: schema-broken, lint-dirty (an edge to a missing node), a `deploy`-typed node named innocently (`publish_summary`), more nodes than `max_nodes`, a shell program outside the catalog | `golden.rs` | a refusal arm that laundered one case into a neighbour |
| The repair loop repairs: round 1 reply invalid, round 2 reply valid → `rounds == 2`, document equals the round-2 golden | `golden.rs` | a loop that only refuses |
| Stamping is load-bearing: with the stamping step disabled (a `#[cfg(test)]` seam is NOT allowed — instead feed a reply whose nodes lack customs and assert `stampedCustoms` lists them AND the document lints with zero `GHG102`) | `golden.rs` | the #183 defect returning |
| Catalog ⊆ what the runtime executes: every node type in the catalog is `work_kind`-viable, and the catalog's construction enumerates every `NodeType` variant (exhaustive `match` in a test) | `core/architect/tests/catalog.rs` | a variant added to `NodeType` without a catalog decision |
| Template hash rides the reply and the document's labels, and a fixture keyed under a different prompt hash is `FixtureMissing` naming the hash | `golden.rs` | drift without a name |
| CLI: `graph synthesize --fixture … --out …` writes the file, refuses to overwrite, and the file starts and completes with fixtures (the first compile) | `apps/cli/tests/architect_cli.rs` | the road from prompt to completed execution |
| CLI refusal exit code and diagnostic shape for `CapabilityMissing` | `architect_cli.rs` | a refusal reported as success |
| HTTP and MCP return the same JSON the CLI does for the same fixture (byte-identical `data`) | `apps/cli/tests/api_http.rs`, `mcp_stdio.rs` (tool list) | a second serialization |

## 6. Declared gaps

- Task Profiler, Capability Discovery beyond the static catalog, Agent Matching/Synthesis,
  Context Plan, Policy Enforcement beyond lint+viability, Simulation before publish, Ghost
  nodes, Studio rendering of the rationale, autopilot auto-publish, multi-draft judge panels —
  the ten non-goals of the blueprint §4, unchanged and named in `docs/harness/GRAPH_ARCHITECT.md`.
- Semantic quality (Tier B) is unmeasured until a paid judge run; the golden suite proves
  determinism and validity, not usefulness.
- The synthesized `agent` nodes are ephemeral (inline contract); the registry seam (#110) is
  untouched.
