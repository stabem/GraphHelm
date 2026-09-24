# Graph Architect — what shipped in the first compile (#107)

Base for every citation: the `issue-107-graph-architect` branch as of this document; §10 cites
the epic #1109 branches. Design:
`docs/specs/2026-09-11-graph-architect-design.md` (decisions D1–D10); the typed
judgments of #1109: `docs/specs/2026-09-16-architect-judgments-design.md`.
Acceptance run: `docs/acceptance/m11-first-compile-2026-09-11.md`; the keyed Tier B recipe:
`docs/acceptance/architect-judgments-recipe.md`. Decisions on record: D-051, D-052 and D-054 in
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
| in | judge door (#1109, §10) | a recording of typed answers, or a `typesafe` gateway route; absent means no judgment is asked | `--judge-fixture <answers.json>` / `--judge-route <id>`; HTTP/MCP `judgeFixture` / `judgeRoute` |
| in | drafts (#1109, §10) | how many stance drafts to ask for and rank, 1..=3, default 1; more than one needs a judge | `--drafts N`, `drafts` |
| in | library (#1109, §10) | a directory of graph templates with `.template.json` sidecars the judge may reuse or adapt; read only when a judge is named | `--library <dir>`, `library` |
| out | `document` | the complete Graph DSL document (`apiVersion p50.dev/graph/v1`, `kind ExecutionGraph`, compiler-owned `metadata`, the model's `spec` with customs stamped) | every door; the CLI also writes it to `--out <path.json>` (`create_new`: an existing file is never overwritten) |
| out | `rationale` | `[{node, reason}]` in node-id order: the node's objective, plus the stamp when the compiler added one | every door |
| out | `stampedCustoms` | the node ids the compiler stamped `completion.customs` onto, sorted | every door |
| out | `templateSha256` | the version of the template that assembled every prompt of the run | every door, and `metadata.labels.template` |
| out | `rounds` | the round whose draft was accepted (1 = no repair) | every door |
| out | `promptSha256s` | the hash of every prompt asked, in order; `len() == rounds` | every door |
| out | `usage` | `{inputTokens, outputTokens}` when the door reported any; absent for a recording (a figure nobody measured is never invented) | every door |
| out | `judgments` | `{nodes: [{node, onGoal, kind, kindConfidence}], unresolved: [node ids], usage}` — the accepted draft's per-node answers verbatim, the nodes the judge left between the thresholds, and the judge's summed usage; **absent when no judge was named** (#1109, §10) | every door |
| out | `ranking` | `{candidates: [{index, stance, coverage, waste, confidence, composite}], chosen, unresolved}`; **absent unless `drafts > 1`**; the chosen document also carries `metadata.labels.stance` (#1109, §10) | every door |
| out | `reuse` | `{road, template, parameters, confidence, unresolved, usage}` — `reuse`, `adapt` or `create`, the template used or seeded, the parameter values a `reuse` filled, and the judge usage of the decision and fill calls (`usage` is summed here on every road; `judgments.usage` carries only the per-node and ranking calls); **absent unless a judge AND a non-empty library were named** (#1109, §10) | every door |

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

## 5. The ten named non-goals (spec §6, blueprint §4; items 1–9 unchanged, item 10 retired)

1. Task Profiler (the profile is caller-supplied).
2. Capability Discovery beyond the static, code-derived catalog.
3. Agent Matching / Agent Synthesis (synthesized agents are ephemeral; the registry seam, #110, is untouched).
4. Context Plan.
5. Policy Enforcement beyond lint + viability.
6. Simulation before publish.
7. Ghost nodes.
8. Studio rendering of the rationale.
9. Autopilot auto-publish (nothing publishes, nothing starts).
10. ~~Multi-draft judge panels (one draft per round, one road).~~ **RETIRED by epic #1109
    (spec D9): PR #1125 ranks up to three stance drafts with one typed judge call (§10, site 2).**
    What stays retired is the panel of chat models arguing; what exists is one System One judge
    answering closed questions under a fixed policy, and one draft per round on the default road.

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
way the M08/M09 judge runs were. Since #1109 the judge is a typed one (§10), so the run has a
recipe: `docs/acceptance/architect-judgments-recipe.md` names the prerequisites, the exact
invocation per goal, and the table a threshold change must cite. The judge's own semantic
usefulness (does `on_goal` track what a person would say?) is part of what that run measures,
not something the Tier A cells prove.

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
| CLI | `graphhelm graph synthesize --goal <text> --out <path.json> [--mode M] [--max-nodes N] [--allow-program P]* (--fixture <replies.json> \| --manifest <m> --route <id> [--broker --keyring --key-id]) [--judge-fixture <answers.json> \| --judge-route <id>] [--drafts N] [--library <dir>]` | the D8 JSON plus `out` |
| HTTP | `POST /v1/graphs/synthesize` `{goal, mode?, maxNodes?, allowPrograms?, fixture?, route?, judgeFixture?, judgeRoute?, drafts?, library?}` (closed body; no `Idempotency-Key`, no execution id) | the D8 JSON, `command: graph.synthesize` |
| MCP | tool `synthesize`, same closed schema, posting only the fields given | the route's reply, byte for byte |

The three return the same `data` (`api_http::the_api_and_the_cli_compile_the_same_goal_to_the_same_bytes`,
`mcp_stdio::the_synthesize_tool_reaches_the_architect_route_and_relays_its_document`), with a
judge too (`api_http::the_api_and_the_cli_agree_with_a_judge_and_report_the_unresolved_node`,
`mcp_stdio::the_synthesize_tool_forwards_the_judge_fixture_and_relays_the_judgments`). None of
them publishes or starts an execution. The judge fields are described in §10.

## 10. The judge door and its four sites (#1109)

Design: `docs/specs/2026-09-16-architect-judgments-design.md` (decisions D1–D10 of
that spec; cited below as "spec D*n*"). Decision on record: D-054. Landed by PRs #1114 (wire
types), #1116 (judge port), #1118 (adapter), #1120 (site 3), #1125 (site 2), #1126 (sites 4 and
1), #1127 (three doors).

### 10.1 Two ports, not one wider port (spec D1)

`DraftModel::draft(prompt) -> text` (§8) is untouched. Beside it, `JudgeModel::judge(&JudgeRequest)
-> JudgeReply` (`core/architect/src/judge.rs`) is the compiler's SECOND model door: a System One
model (TypeSafe's Jev; route family `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2.6) that cannot
generate text and only answers closed questions over state the compiler hands it. The wire types
— `Question` (`Noul | Choice | Score`), `JudgeRequest`, `Answer`, `JudgeReply`, `request_sha256`
— live in `core/gateway/src/judgment.rs` and serialize to the documented `POST /v1/systemone`
body. The adapter is `adapters/model-gateway/src/systemone.rs` (`SystemOneAdapter`, provider
`typesafe`, transport `direct_api`, base URL `https://api.typesafe.ai`, model `jev-latest`), the
key leased from the Credential Broker exactly as the Anthropic key is. The two doors refuse each
other's routes: a `typesafe` route on the draft door and a chat route on the judge door are both
`GatewayError::UnsupportedCapability`, and nothing is sent before the refusal.

### 10.2 The three doors a judgment may enter through (spec D3)

A judgment reaches the compiler ONLY as:

1. a **repairable diagnostic** under its own code, appended to the round's diagnostics and fed
   back to the draft model like any lint error — `GHA005_NODE_OFF_GOAL` (the judge read the node's
   objective as not serving the goal) and `GHA006_NODE_KIND_MISMATCH` (the judge, confidently,
   would type the objective differently from the draft);
2. a **ranking** the code reads under one fixed, named policy (§10.4, site 2);
3. a **report field**: `judgments`, `ranking`, `reuse` (§2), every one `Option` and absent when
   the run named no judge.

It never removes a diagnostic, never edits a draft, never bypasses `compile_round`. The
allowlist (D-052), `GHG102` (D-051) and the node count stay deterministic and un-judged. The
judge is asked about a draft only AFTER every deterministic check has passed, so a schema-, lint-
or allowlist-broken draft never reaches it.

### 10.3 Absent judge = today's bytes (spec D4)

`synthesize(profile, catalog, model)` keeps its signature and its golden. The entry point with a
judge is `synthesize_with(profile, catalog, model, &Extras)`, `Extras { judge: Option<&dyn
JudgeModel>, drafts: u8, library: Option<&GraphLibrary> }`; `Extras::default()` (`drafts: 1`)
reproduces `synthesize` byte for byte and `first-compile/expected.json` did not change.

### 10.4 The four sites and their question shapes

Question ids are the compiler's own and carry no meaning to the model; every question carries its
whole meaning in `instructions` and `criteria`. The state is always the goal plus the node
summaries `{id, type, objective}` — never the prompt, never the template.

| site | module | question shape | what the code does with the answer |
|---|---|---|---|
| 3 — per-node judgments | `core/architect/src/judgment/nodes.rs` | per node: `on_goal:<id>` (`Noul`: does this node do work the goal needs?) and `kind:<id>` (`Choice` over the catalog's node types) | a "no" on `on_goal` is `GHA005`; a confident `kind` that differs from the draft's type is `GHA006`; both go back to the draft model with the round's other diagnostics; answers between the thresholds are listed under `judgments.unresolved` and do nothing |
| 2 — rank N drafts | `core/architect/src/judgment/ranking.rs` | per candidate `i`: `coverage:<i>` (`Score` over the three `COVERAGE_LEVELS`, a position `0.0..=2.0`) and `waste:<i>` (`Noul`: does it do work the goal did not ask for?) | composite `coverage - waste`; the highest wins, a tie goes to the lower index; a top candidate under the acting threshold keeps draft 1 and sets `ranking.unresolved` |
| 4 — reuse / adapt / create | `core/architect/src/judgment/reuse.rs` (`decide_request`, `read_decision`) | one `Choice` over the three roads, one `Choice` over the library's templates | selects the road BEFORE any draft is asked; `create`, and every unresolved answer, is today's road with `reuse` saying so |
| 1 — template + typed parameters | `core/architect/src/judgment/reuse.rs` (`fill_request`, `read_fill`) | one `Choice` per closed-set parameter, its instructions the sidecar's `question`, its criteria the sidecar's `options` | fills the template without a draft model and validates it through the SAME chain a draft takes; one unresolved parameter falls to `create` |

Site 2 asks for variants without touching the single-draft prompt (spec D7): `assemble_prompt`
gains `stance: Option<&Stance>`; `None` yields exactly today's bytes; `Some` appends one fenced
`<stance>` block after `</goal>`. The three fixed stances `minimal`, `verified`, `explicit`
(`Stance::ALL`, in that order: draft 1 is always `minimal`) are the whole vocabulary; `drafts`
is bounded to `1..=3` and refused as `invalidProfile` at `/drafts` outside it; more than one
draft without a judge is refused before any prompt. Each draft is judged and repaired on its
own, then ranked by ONE judge call.

### 10.5 The thresholds (spec D6)

`core/architect/src/judgment/policy.rs` holds three named constants:

| constant | value | meaning |
|---|---|---|
| `ACT_THRESHOLD` | `0.80` | a `Choice`/`Score` answer acts on the compiler only at or above this confidence |
| `NOUL_NO_THRESHOLD` | `0.35` | a `Noul` at or below this reads as "no" |
| `NOUL_YES_THRESHOLD` | `0.65` | a `Noul` at or above this reads as "yes"; between the two it is unresolved |

They are conservative starting VALUES TO BE MEASURED, not truths. A threshold moves only after a
recorded run of real Jev over the goal set under `docs/acceptance/` (the recipe in
`docs/acceptance/architect-judgments-recipe.md`), and the new value lands with a cell on each side
of it (`policy_edges_are_exact` pins `value - ε` and `value + ε` today). An answer below the
acting threshold does nothing — no diagnostic, no reorder, no fill — and is reported as
`unresolved`, so the absence is visible rather than silent.

### 10.6 The recorded judge door (spec D5)

`RecordedJudgeModel` reads `{"answers": {"<request sha256>": <JudgeReply>, …}}` (≤ 4 MiB, keys
lowercase hex sha256, keyed by `request_sha256` over the canonical request bytes exactly as
`RecordedDraftModel` is keyed by the prompt hash). It is the judge in every Tier A test and is
available to operators as `--judge-fixture` / `judgeFixture`; every Tier A cell is keyless. A
recording that holds no reply refuses `judgeMissing { requestSha256 }`, naming the request the
way `fixtureMissing` names the prompt; a door that cannot answer is `judgeUnavailable { message }`
with the failure class, never a path or a key. `ARCHITECT_RECORD=1` re-records judge replies beside
draft replies through the same missing-fixture loop (§8); the judge fixtures live under
`core/architect/fixtures/judge/` and each carries an authored half (`rounds`) and a derived half
(`answers`).

### 10.7 The library contract (spec D8)

A `GraphLibrary` (`core/architect/src/library.rs`, `GraphLibrary::load(dir)`) is a directory of
graph documents in the authored format (`<id>.yaml` or `<id>.json`, at most `MAX_TEMPLATES = 64`)
each beside a sidecar `<id>.template.json` (`SIDECAR_SUFFIX`) declaring `parameters: { <name>:
{ question, options: { <value>: description } } }`; each chosen value substitutes `{{name}}` in
the document's string fields. Only closed sets are parameters; free text never is. `reuse` fills
and validates through the same chain as a draft — a filled template that fails lint or names a
program outside the allowlist is refused (`capabilityMissing`), not repaired; `adapt` seeds the
draft prompt with the template inside a fenced `<seed>` block; `create` is today's road. There is
no default library directory and no bundled template: an absent or empty library makes the
decision step a no-op, and a library without a judge is ignored and reports nothing. The fixture
library under `core/architect/fixtures/library/` is test data, not a shipped catalog.

### 10.8 The flags and fields on the three surfaces (spec D10)

| surface | judge door | drafts | library |
|---|---|---|---|
| CLI (`apps/cli/src/args.rs`) | `--judge-route <id>` (needs `--manifest`; leased through `--broker --keyring --key-id` like `--route`) or `--judge-fixture <answers.json>`; mutually exclusive | `--drafts N` | `--library <dir>` |
| HTTP `POST /v1/graphs/synthesize` | `judgeRoute` (a `direct_api` `typesafe` route of the server's manifest) or `judgeFixture` (a path on the Runtime host); mutually exclusive, refused at `/judgeFixture` | `drafts` (integer 1..=3) | `library` (a directory on the Runtime host) |
| MCP tool `synthesize` (`apps/cli/src/commands/mcp/tools.rs`) | `judgeRoute` / `judgeFixture` | `drafts` | `library` |

All three funnel through `apps/cli/src/commands/architect.rs::execute` and return the same
bytes; a judge route that is missing or disabled is `GHCLI009` at `/judgeRoute`.

The rule across the three surfaces is **capability parity, not argument parity**: the same doors
are reachable everywhere, spelled the way each surface spells things.

A recorded draft pairs with a real judge on the CLI as it always has over HTTP: `--fixture` with
`--manifest --judge-route` (#1137). `--manifest` is not a draft door on its own, so `--fixture`
beside it is not two doors — it carries the JUDGE's route. What is refused is the real conflict,
`--fixture` with `--route`, and `--fixture` with a `--manifest` that serves no judge, because the
manifest would then serve nothing and a silent no-op is the worse answer to a typo.

Until #1137 that pairing was refused on the CLI and accepted over HTTP, which is why the
judge-route cells in `apps/cli/tests/architect_cli.rs` (#1127) draft through a fake gateway route;
`a_recorded_draft_pairs_with_a_real_judge_route_and_asks_the_provider_only_to_judge` is the cell
that holds it now, and it discriminates on the REQUEST COUNT — one call, and it is the judge's —
because a run that fell back to the gateway for its draft would also exit 0.

### 10.9 Cell map — the guard test that holds each claim

| claim | test | file |
|---|---|---|
| Wire pin: the documented request and response round-trip byte for byte; an unknown answer type is refused, not defaulted; the digest is a pure function of canonical bytes | `the_request_serializes_to_the_documented_body`; `the_documented_response_parses_to_typed_answers`; `an_answer_of_an_unknown_type_is_refused_not_defaulted`; `the_request_digest_is_a_pure_function_of_canonical_bytes` | `core/gateway/tests/judgment_wire.rs` |
| Manifest: `typesafe` is a legal `direct_api` provider | `typesafe_is_a_legal_direct_api_provider` | `core/gateway/tests/manifest_contract.rs` |
| Adapter: Bearer header, path `/v1/systemone`, documented body; every documented status maps to its taxonomy error; a body that is not a reply is `MalformedOutput` | `a_success_posts_the_documented_body_with_a_bearer_and_parses_the_answers`; `every_documented_status_maps_to_its_taxonomy_error`; `a_success_whose_body_is_not_a_reply_is_malformed_output` | `adapters/model-gateway/tests/systemone_adapter.rs` |
| D1, both refusal arms, nothing sent | `a_chat_provider_on_the_judge_door_is_unsupported_and_sends_nothing`; `a_typesafe_route_on_the_draft_door_is_unsupported_and_sends_nothing` | `adapters/model-gateway/tests/systemone_adapter.rs` |
| D5, the recorded door answers only what it recorded and is bounded and shaped | `a_recorded_judge_answers_only_the_request_it_recorded`; `a_recorded_judge_file_is_bounded_and_shaped` | `core/architect/tests/judge.rs` |
| D4, absent judge = today's bytes; one draft adds no stance and no `ranking` key; a library without a judge says nothing | `no_judge_is_todays_bytes`; `one_draft_adds_no_stance_and_no_ranking_key`; `a_library_without_a_judge_is_ignored_and_says_nothing` | `core/architect/tests/judgment_nodes.rs`; `core/architect/tests/judgment_ranking.rs`; `core/architect/tests/library.rs` |
| Site 3: an off-goal node is a repairable `GHA005` and round two wins; a low-confidence kind changes nothing and is reported unresolved; the judge is asked only after every deterministic check; a judge that cannot answer names the request and ends the run | `an_off_goal_node_is_a_repairable_gha005_and_round_two_wins`; `a_low_confidence_kind_changes_nothing_and_is_reported_unresolved`; `the_judge_is_asked_only_about_a_draft_that_passed_every_deterministic_check`; `a_judge_that_cannot_answer_names_the_request_and_ends_the_run` | `core/architect/tests/judgment_nodes.rs` |
| D6, a cell on each side of every threshold; `drafts` outside `1..=3` is an invalid profile before any prompt | `policy_edges_are_exact`; `drafts_outside_one_to_three_is_an_invalid_profile_before_any_prompt` | `core/architect/tests/judgment_nodes.rs` |
| Site 2: the best-covered candidate is chosen and the report says why; low confidence keeps the first draft; a tie goes to the lower index; more than one draft needs a judge | `the_best_covered_candidate_is_chosen_and_the_report_says_why`; `a_low_confidence_ranking_keeps_the_first_draft`; `a_tie_on_composite_goes_to_the_lower_index`; `drafts_outside_one_to_three_are_refused_before_any_prompt`; `more_than_one_draft_needs_a_judge` | `core/architect/tests/judgment_ranking.rs` |
| Sites 4 and 1: the library loads every template with its sidecar and refuses a bad one; fill substitutes only declared closed values; `reuse` never asks the draft model; a filled template outside the allowlist is `capabilityMissing`; `adapt` seeds the prompt; `create` is today's road; an unsure road falls to `create` and says so; an unacted decision or fill is unresolved, never guessed | `a_library_loads_every_template_with_its_sidecar_and_refuses_a_bad_one`; `fill_substitutes_only_declared_closed_values_and_refuses_the_rest`; `reuse_fills_a_template_and_never_asks_the_draft_model`; `a_filled_template_outside_the_allowlist_is_capability_missing`; `adapt_seeds_the_draft_prompt_with_the_template`; `create_is_todays_road`; `an_unsure_road_falls_to_create_and_says_so`; `an_unacted_decision_or_fill_is_unresolved_never_guessed` | `core/architect/tests/library.rs` |
| D10: the CLI relays the compiler's refusal of drafts without a judge; HTTP and MCP return the CLI's bytes WITH a judge and relay the unresolved node | `more_than_one_draft_without_a_judge_is_refused_by_the_compiler`; `the_api_and_the_cli_agree_with_a_judge_and_report_the_unresolved_node`; `the_synthesize_tool_forwards_the_judge_fixture_and_relays_the_judgments` | `apps/cli/tests/architect_cli.rs`; `apps/cli/tests/api_http.rs`; `apps/cli/tests/mcp_stdio.rs` |

What no cell proves: whether the judge's answers track what a person would say. That is Tier B
(§6) and waits for the recipe's recorded run.

### 10.10 Shadow classification of a red gate (#1138)

The first gate edge the judge door may own, in SHADOW MODE: `graphhelm gate classify-red` puts a
RED run's log to the judge and RECORDS what it said. The rule, before the mechanism:

- **Never the verdict.** The gate's colour is `ci/gate.ps1`'s and stays deterministic
  (`AGENTS.md`: deterministic policy and evidence decide gates; agent agreement is advisory). The
  command has no handle on a manifest's `runClass`, so a red is never relabelled; its vocabulary is
  disjoint from `ci/classify-run.ps1`'s on purpose.
- **Never re-queues.** Nothing reads `acts` to do anything. Shadow mode exists to produce the
  confusion table that sets this edge's threshold (Tier B calibration, the issue's precondition)
  before any edge gets the power to re-queue a head.
- **Output beside the manifest.** `--out <path.json>` writes the record next to the run manifest
  it describes and refuses to overwrite; without `--out` the record is only printed.
- **The command never exits non-zero for a classification** — `real_defect`, low confidence and
  `unresolved` all exit 0. Only its own failures do: an unreadable log, a bad flakes file, an
  `--out` it may not write, a judge that cannot answer (a recording without the reply names the
  request digest on stderr).

```text
graphhelm gate classify-red --log <gate log> --known-flakes <flakes.json>     (--judge-fixture <answers.json> | --manifest <m> --judge-route <id> --broker <b> --keyring <k> --key-id <id>)     [--out <path.json>]
```

The log is read as UTF-8 or UTF-16 (BOM-detected; the runner's transcripts are UTF-16). The
known-flake list is `[{"issue": 886, "test": "<cell name>", "summary": "<why it flakes>"}, …]`.
The judge door is the same pair `graph synthesize` opens (§10.8): the recorded door, keyless, or a
`direct_api` `typesafe` route leased through the broker.

**State** (`core/architect/src/judgment/red.rs`, pure): a bounded excerpt — the stages the gate
named as failed (`[gate] RED - failed stages: …` and `[gate] FAILED: <stage>`), the
`test <name> ... FAILED` names (≤ 20), the first `panicked at` line with its location, the first
`error:` line, and the last 40 non-blank lines, every string ≤ 400 characters — plus the
known-flake list. **Questions:** `class`, a Choice over the closed vocabulary
`known_flake | environment_void | real_defect | harness_broke` with one rubric each, and one
`same_as:<issue>` Noul per known flake. **Reading:** the same named constants as every other site
(§10.5) — `acts` is `ACT_THRESHOLD` on the class, `sameAs` is `NOUL_YES_THRESHOLD` per flake; a
class outside the vocabulary, a missing answer, or a confidence under the threshold is
`unresolved`, never a class acted on.

**Output:** `{class, confidence, sameAs, contradicted, acts, unresolved, excerptDigest, judgeUsage}` (`out`
added when written). `excerptDigest` is the sha256 of the excerpt, so two runs that failed the
same way are visibly the same row of the table.

`sameAs` names a known flake only when the EXCERPT names that flake's test: the judge's yes is grounded in the prompt, and the excerpt is the log's own evidence, so an answer the evidence does not support is reported under `contradicted` and counted as no match (#1140 review). A probability outside `[0, 1]` never acts and never names a flake. A `known_flake` must NAME a flake the excerpt supports, or it does not act: a contradiction is evidence against the class rather than a field beside it, and so is silence — naming none reaches the same zero evidence. The boundary is the operator's list: when no known flakes were supplied, nothing was asked and the class is not denied on that ground. The test comparison is the common suffix of the two paths — every segment both sides carry must agree — so a bare name matches a qualified one while two differently-qualified paths with the same leaf do not.

| claim | test | file |
|---|---|---|
| The excerpt reads the three log shapes and is bounded | `a_flake_log_yields_its_stage_test_and_wrapped_panic_location`; `a_disk_full_log_yields_the_aborted_stage_and_the_first_error_line`; `a_real_defect_log_yields_every_stage_and_test_once`; `the_excerpt_is_bounded_whatever_the_log_holds` | `core/architect/src/judgment/red.rs` |
| One `class` and one `same_as` per known flake; the reading acts only in-vocabulary and at the threshold | `the_request_asks_one_class_and_one_same_as_per_known_flake`; `an_in_class_confident_answer_acts_and_names_the_flake`; `a_low_confidence_answer_is_unresolved_and_a_no_names_no_flake`; `a_class_outside_the_vocabulary_is_unresolved_however_confident` | `core/architect/src/judgment/red.rs` |
| The three journeys exit 0 whatever the class; UTF-16 is the same excerpt; `--out` never overwrites; a missing reply names the digest on stderr | `the_known_flake_is_classified_known_flake_and_named_same_as_886`; `the_disk_full_canary_abort_is_environment_void`; `a_low_confidence_answer_is_unresolved_and_still_exits_zero`; `a_utf16_log_is_the_same_excerpt_as_its_utf8_twin`; `out_is_written_once_and_never_overwritten`; `a_recording_without_the_reply_names_the_request_digest_on_stderr` | `apps/cli/tests/gate_classify_red.rs` |

Not here, on purpose: a runner hook that calls the command after a red (the queued gate runner was retired on 2026-09-24),
flake dedup as a count per issue (edge 2), and finding triage (edge 3). Each is its own change
under #1138.
