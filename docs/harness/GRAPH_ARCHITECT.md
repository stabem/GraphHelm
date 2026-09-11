# Graph Architect — what shipped in the first compile (#107)

Base for every citation: the `issue-107-graph-architect` branch as of this document. Design:
`docs/superpowers/specs/2026-09-11-graph-architect-design.md` (decisions D1–D10). Acceptance
run: `docs/acceptance/m11-first-compile-2026-09-11.md`. Decisions on record: D-051 and D-052 in
`docs/DECISION_REGISTER.md`.

## 1. One box of the pipeline, not the pipeline

`HARNESS_SPEC.md` §4 draws fourteen boxes from *Task Request* to *Harness Manifest + Graph v1*.
This slice builds ONE of them — box I, the **Graph Architect** — and reaches it by two
shortcuts the spec does not draw:

| §4 box | in this slice |
|---|---|
| B Normalize & Resolve Scope, C Task Profiler, D Project State Snapshot | **not built.** The profile is caller-supplied (D4): `TaskProfile { goal, mode, maxNodes, waitWithinSeconds, clearanceWithinSeconds }`, defaults `supervised` / 6 / 86400 / 3600, bounded (`goal` ≤ 4 KiB, `maxNodes` 1..=50, three modes). |
| E Capability Discovery | **replaced by a code-derived catalog** (D3): `CapabilityCatalog::from_runtime(programs)` lists the node types `core/runtime/src/classify.rs::work_kind` executes (`agent`, `classifier`, `evaluator`, `gate`, `planner`, `tool`), the three `ToolCall` families (`repository`, `shell`, `tests`), and the operator's program allowlist — sorted, deduplicated, never defaulted. |
| F Agent Match / Synthesis, G Context Strategy Planner, H Model Candidate Planner | **not built.** Synthesized `agent` nodes are ephemeral (inline `agent.ephemeral` contract); the route is chosen by the operator (`--route`) or is the recorded fixture. |
| **I Graph Architect** | **built**: `core/architect`, `synthesize(profile, catalog, model)`. |
| J Policy Engine, M Graph Linter | the SAME chain an authored graph takes — `load_graph_json` → `lint` → executor viability — run on every draft; nothing beyond lint + viability is enforced here. |
| K Isolation Planner, L Budget Optimizer, N Graph Simulator | **not built**; the document is not simulated before it is written. |
| O Harness Manifest + Graph v1 | **not produced.** The output is a Graph DSL document, bytes for `execution start --file`, on the one road every authored graph takes (D-039). The architect never publishes and never starts. |

The compiler shape (D1): assemble a deterministic prompt, ask the model for ONE draft, wrap
the model's `spec` in the compiler's own metadata, stamp customs, validate, and either emit or
feed the diagnostics back — at most `MAX_REPAIR_ROUNDS = 2` repairs, so a third invalid draft is
the refusal. The loop is in `core/architect/src/synthesize.rs`.

## 2. Inputs and outputs

| direction | name | shape | source |
|---|---|---|---|
| in | goal | text, ≤ 4096 bytes, quoted into the prompt as data (§7) | `--goal`, HTTP `goal`, MCP `goal` |
| in | mode | `autopilot` / `supervised` / `manual`, default `supervised` | `--mode`, `mode` |
| in | node ceiling | `maxNodes`, 1..=50, default 6; the draft's own `budgets.maxNodes` may not exceed it | `--max-nodes`, `maxNodes` |
| in | customs budgets | `waitWithinSeconds` 86400, `clearanceWithinSeconds` 3600 (profile defaults; not exposed on the CLI) | `TaskProfile` |
| in | program allowlist | the ONLY programs a synthesized shell call may name; no default | `--allow-program P` (repeatable), HTTP/MCP `allowPrograms` (defaults to the Runtime's `serve --allow-program` wiring, else `[]`) |
| in | model door | a recording, or a gateway route | `--fixture <replies.json>` / `--manifest <m> --route <id>`; HTTP/MCP `fixture` / `route` |
| out | `document` | the complete Graph DSL document (`apiVersion p50.dev/graph/v1`, `kind ExecutionGraph`, compiler-owned `metadata`, the model's `spec` with customs stamped) | every door; the CLI also writes it to `--out <path.json>` (`create_new`: an existing file is never overwritten) |
| out | `rationale` | `[{node, reason}]` in node-id order: the node's objective, plus the stamp when the compiler added one | every door |
| out | `stampedCustoms` | the node ids the compiler stamped `completion.customs` onto, sorted | every door |
| out | `templateSha256` | the version of the template that assembled every prompt of the run | every door, and `metadata.labels.template` |
| out | `rounds` | the round whose draft was accepted (1 = no repair) | every door |
| out | `promptSha256s` | the hash of every prompt asked, in order; `len() == rounds` | every door |
| out | `usage` | `{inputTokens, outputTokens}` when the door reported any; absent for a recording (a figure nobody measured is never invented) | every door |

Metadata is the compiler's, not the model's (D5): `id: arch_<sha8 of goal>_v1`, `name: <goal,
first 80 chars>`, `executionId: exec_<sha8>`, `version: 1`, `labels { origin: architect,
template: <sha256> }`. Two runs on one goal produce one identity, and the golden is byte-stable
because `serde_json`'s default map sorts keys — enabling `preserve_order` anywhere in the
workspace reddens it.

## 3. The refusal vocabulary

`ArchitectRefusal` (`core/architect/src/refusal.rs`) is one `kind`-tagged enum, camelCase on
the wire, and every door prints the same shape: the CLI as ONE diagnostic
`GHCLI026_ARCHITECT_REFUSED` at `/goal` whose message is the refusal as compact JSON, HTTP and
MCP with the CLI's own codes. The arms are kept apart on purpose — a refusal laundered into a
neighbouring kind hides the cause from the person who has to act on it.

| kind | when | repaired? |
|---|---|---|
| `invalidProfile { pointer, message }` | the caller's profile is outside its own bounds; refused before any prompt is assembled | no model is asked |
| `modelUnavailable { message }` | the door could not be reached or answered with an error; the message names the failure class, never a credential | no |
| `fixtureMissing { promptSha256 }` | the recording holds no reply for this prompt; the hash is the key to record under | no |
| `notJson { round, message }` | the LAST round's reply was not a JSON object (earlier rounds are fed back as `GHA001`) | up to 2 rounds |
| `invalid { rounds, diagnostics }` | every round's draft failed schema, lint or viability; the diagnostics are the last draft's, verbatim | up to 2 rounds |
| `capabilityMissing { node, program }` | a shell call names a program outside the allowlist; the model cannot authorize a program and the architect never widens the list (#184) | no |
| `tooManyNodes { count, max }` | the draft has more nodes than the ceiling stated in the prompt; counted off the parsed reply BEFORE schema and lint run | no |
| `notCompletable { nodes }` | `GHG102_UNBOUNDED_CUSTOMS` survived stamping — the lint's park set and the compiler's stamp set drifted; a defect with a kind, never a warning (#183) | no |

The repairable diagnostics the compiler adds to the lint's own, each fed back to the model
with its pointer (`source: architect-draft`, never a path):

| code | meaning |
|---|---|
| `GHA001_NOT_JSON` | the reply was not a JSON object (or exceeded the 4 MiB reply bound) |
| `GHA002_NODE_TYPE_NOT_EXECUTABLE` | a node's type is legal for the schema but `work_kind` refuses to execute it (a `deploy` node named innocently is refused here, under its own code) |
| `GHA003_TOOL_CALL_MISSING` | a tool node carries no `tool.call` the driver could assemble: no call, a call the tool broker's checked parser (`ToolCall::from_json`) refuses, a shell call without a program, or a repository WRITE (`apply_patch`, `commit`) the template never offered |
| `GHA004_BUDGET_EXCEEDS_PROFILE` | the draft's `budgets.maxNodes` is above the profile's ceiling (the node COUNT above the ceiling is `tooManyNodes` and is not repaired) |

## 4. Born completable, and the catalog as a security surface

**D-051.** After the model's draft parses, the compiler stamps `completion.customs`
(`proofKinds: []`, the profile's two budgets) onto every node `work_kind` dispatches as
cognitive or tool work and that carries none (an existing `customs` block is kept). The stamp
set is derived from the runtime's classification, not a hand list; `tests/park_witness.rs`
holds it against the lint's park set for every `NodeType` variant, so a type that parks without
being stamped goes red by name. A `GHG102` that survives stamping is `notCompletable`, a failure
of synthesis — the 0/4 defect of #183 is refused, not demonstrated.

**D-052.** The programs in the catalog are the operator's allowlist and nothing else: the same
surface `serve --allow-program` feeds the tool lease. A draft naming any other program is
`capabilityMissing { node, program }` — the refusal names what the operator would have to
authorize, and the architect never authorizes it. The first compile's own goal needs `cargo`,
so the golden run passes `--allow-program cargo` explicitly.

## 5. The ten named non-goals (spec §6, blueprint §4, unchanged)

1. Task Profiler (the profile is caller-supplied).
2. Capability Discovery beyond the static, code-derived catalog.
3. Agent Matching / Agent Synthesis (synthesized agents are ephemeral; the registry seam, #110, is untouched).
4. Context Plan.
5. Policy Enforcement beyond lint + viability.
6. Simulation before publish.
7. Ghost nodes.
8. Studio rendering of the rationale.
9. Autopilot auto-publish (nothing publishes, nothing starts).
10. Multi-draft judge panels (one draft per round, one road).

## 6. Tier A is the gate; Tier B is unmeasured (D9)

**Tier A** — deterministic golden and sabotage, keyless, in the commit loop: the recorded reply
compiles to a byte-stable document; every sabotage fixture is refused under its OWN diagnostic;
the repair loop repairs; stamping is load-bearing; the catalog is exactly what the runtime
executes; the template hash rides the reply and the document; the CLI journey runs the document
to `completed`; HTTP and MCP return the CLI's bytes. The cell-to-test map is in
`docs/acceptance/m11-first-compile-2026-09-11.md`.

**Tier B** — semantic quality (is the graph USEFUL for the goal?) is out of the commit loop and
has not been measured. The golden suite proves determinism and validity, not usefulness. The
next measurement is a paid judge run over a set of goals, recorded under `docs/acceptance/` the
way the M08/M09 judge runs were.

## 7. Prompt-injection posture

The goal and, on a repair round, the previous draft are embedded verbatim but FENCED (`<goal>`,
`<previous-draft>`), and the template states that the fenced text is the operator's request
quoted as data, not an instruction. Substitution is ONE left-to-right pass over a fixed
placeholder set — a substituted value is never scanned again — so a goal that spells `{{REPAIR}}`
is quoted, not expanded. The goal is bounded before any prompt is assembled, the reply is bounded
before it is parsed (4 MiB), and the node count is read off the parsed reply before the schema
walk and the lint run. The template also forbids secrets, credentials and absolute paths in the
output, and every diagnostic a draft carries names `architect-draft` as its source rather than a
disk a remote caller cannot see. None of this makes the model trustworthy; it makes the
compiler's checks the thing that decides, which is the constitutional split (LLMs propose,
deterministic code enforces).

## 8. The two model doors (D7), and how a fixture is recorded

`DraftModel` is the architect's port: `fn draft(&self, prompt: &str) -> Result<DraftReply,
ArchitectRefusal>`, `DraftReply { text, usage }`.

- **Recorded (keyless).** `RecordedDraftModel` reads `{"replies": {"<prompt sha256>": "<text>"}}`
  (≤ 4 MiB, keys required to be lowercase hex sha256). It is the model in every test and is
  available to operators as `--fixture` / `fixture`. It reports `usage: None`.
- **Gateway.** On the CLI, `--manifest --route` builds `GatewayDraftModel` the way `gateway
  probe` builds its lease (keyring directory, `GRAPHHELM_GATEWAY_KEY`, `CredentialBroker::open`
  + `lease`) and calls the way `serve` does (`ByokAdapter` for `direct_api`, `RuntimeAdapter`
  for `native_runtime`). On `serve`, the route is resolved through the Runtime's own wiring and
  `ServeModelPort`. No new credential path exists; a gateway error is `modelUnavailable` carrying
  the taxonomy's static text, never a path or a key.

**Recording** (`core/architect/fixtures/README.md`). Every fixture file has an AUTHORED half,
`rounds: [<reply for round 1>, <round 2>, …]`, and a DERIVED half, `replies`, the same texts
filed under the sha256 of the exact prompt each round assembled. Round 2's prompt embeds round
1's draft and the compiler's own diagnostics, so the keys cannot be computed without running the
compiler. The template's sha256 is substituted into every prompt (D6), so editing
`core/architect/src/template.rs` moves every key at once and the golden suite fails with
`fixtureMissing { promptSha256 }` naming the hash it needed — a named event, never drift. To
re-record every file from its `rounds`:

```powershell
$env:ARCHITECT_RECORD = "1"
cargo +1.97.1 test -p graphhelm-architect --test golden --locked
Remove-Item Env:ARCHITECT_RECORD
cargo +1.97.1 test -p graphhelm-architect --test golden --locked
```

The recorder does what an operator does by hand: ask with an empty recording, read the hash from
the refusal, file the round's text under it, ask again, at most `MAX_REPAIR_ROUNDS + 1` times.
The second run proves the committed files answer. `first-compile/expected.json` is rewritten by
the same variable and is reviewed in the commit like any golden.

## 9. Surfaces (D8)

| door | invocation | returns |
|---|---|---|
| CLI | `graphhelm graph synthesize --goal <text> --out <path.json> [--mode M] [--max-nodes N] [--allow-program P]* (--fixture <replies.json> \| --manifest <m> --route <id> [--broker --keyring --key-id])` | the D8 JSON plus `out` |
| HTTP | `POST /v1/graphs/synthesize` `{goal, mode?, maxNodes?, allowPrograms?, fixture?, route?}` (closed body; no `Idempotency-Key`, no execution id) | the D8 JSON, `command: graph.synthesize` |
| MCP | tool `synthesize`, same closed schema, posting only the fields given | the route's reply, byte for byte |

The three return the same `data` (`api_http::the_api_and_the_cli_compile_the_same_goal_to_the_same_bytes`,
`mcp_stdio::the_synthesize_tool_reaches_the_architect_route_and_relays_its_document`). None of
them publishes or starts an execution.
