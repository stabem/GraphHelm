# Graph Architect — typed judgments (System One) design

Companion plan: the architect-judgments plan (`docs/process/DELIVERY.md`, History). Parent: the first
compile, `docs/specs/2026-09-11-graph-architect-design.md`. Provider reference:
`docs/reference/PROVIDER_AND_LICENSE_REFERENCES.md` (TypeSafe), route family:
`docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2.6.

## 1. What this adds, and what it does not

The architect today is a compiler with ONE model in the middle: a chat model drafts, the compiler
validates through the authored-graph chain (`load_graph_json` → `lint` → viability → stamp →
allowlist), repairs at most twice, refuses otherwise. Every *judgment* about a draft is either
deterministic (schema, lint, allowlist) or absent (semantic quality, Tier B, "unmeasured").

This design adds a SECOND model port to the compiler, a **judge**, whose answers are typed and
carry a probability: a System One model (TypeSafe's Jev) that cannot generate text and only
answers closed questions over state the compiler hands it. Four sites use it, in dependency
order:

| # | Site (owner's numbering) | Question shape | What code does with the answer |
|---|---|---|---|
| 3 | Per-node judgments | `Noul` on-goal, `Choice` node kind | A repairable diagnostic fed back to the draft model |
| 2 | Rank N drafts | `Score` coverage, `Noul` waste, per candidate | Deterministic composite picks one; low confidence keeps draft 1 |
| 4 | Decide reuse / adapt / create | `Choice` over the three roads, `Choice` over templates | Selects the road before any draft is asked |
| 1 | Template + typed parameters | `Choice` per closed-set parameter | Fills a library graph without a draft model |

It does NOT: let the judge author a node, widen the allowlist, skip lint, publish, start, or fall
back to a paid route on its own. The judge classifies and proposes; deterministic code enforces
(`AGENTS.md`, constitutional invariants).

## 2. Decisions (orchestrator, owner's delegated authority)

D1. **Two ports, not one wider port.** `DraftModel::draft(prompt) -> text` stays as it is. A new
`JudgeModel::judge(&JudgeRequest) -> JudgeReply` is the judge's own port. A `typesafe` route
handed to the DRAFT door refuses `GatewayError::UnsupportedCapability` (Jev cannot draft); a chat
route handed to the JUDGE door refuses the same way (the adapter speaks only `/v1/systemone`).
Neither refusal is repaired or laundered.

D2. **Wire types live in `core/gateway`, the adapter in `adapters/model-gateway`.**
`core/gateway/src/judgment.rs` holds `Question` (`Noul | Choice | Score`), `JudgeRequest`,
`Answer`, `JudgeReply`, and `request_sha256` (canonical bytes → digest), exactly as `call.rs`
holds `ModelCall`/`ModelReply`: plain serde, provider-agnostic, no network. The JSON these
serialize to IS the TypeSafe request/response body (`docs.typesafe.ai/api`), pinned by tests
against the documented examples. `adapters/model-gateway/src/systemone.rs` holds
`SystemOneAdapter` over the existing `HttpTransport`/`UreqTransport`; the manifest admits
provider `"typesafe"` for `direct_api` (base URL `https://api.typesafe.ai`, model
`jev-latest`), key leased from the Credential Broker exactly as the Anthropic key is.

D3. **A judgment can only enter the compiler through three doors.** (a) a REPAIRABLE diagnostic
under its own `GHA0xx` code, appended to the round's diagnostics and fed back to the draft model
like any lint error; (b) a RANKING the code reads under a fixed, named policy; (c) a REPORT field.
It never removes a diagnostic, never edits the draft, never bypasses `compile_round`. Allowlist,
GHG102 and node-count stay deterministic and un-judged (D-051, D-052 unchanged).

D4. **Absent judge = today's bytes.** `synthesize(profile, catalog, model)` keeps its signature and
its golden. The new entry point is `synthesize_with(profile, catalog, model, &Extras)` where
`Extras { judge: Option<&dyn JudgeModel>, drafts: u8, library: Option<&GraphLibrary> }` and
`Extras::default()` reproduces `synthesize`. Every new field of `SynthesizedGraph` is
`Option` + `skip_serializing_if`, so the three doors (D8 of the parent) return byte-identical
`data` for a call that names no judge. The existing `expected.json` golden must not change.

D5. **The judge has a recorded door too.** `RecordedJudgeModel` is a JSON file
`{"answers": {"<request sha256>": {<JudgeReply>}, …}}`; `ARCHITECT_RECORD=1` records judge
replies beside draft replies through the same `FixtureMissing`-driven loop. Every Tier A test is
keyless. A `JudgeMissing { request_sha256 }` refusal names the request, as `FixtureMissing`
names the prompt.

D6. **Thresholds are named constants with a sabotage cell on each side.** They live in
`core/architect/src/judgment/policy.rs`, start conservative (act only above `0.80`, treat a
`Noul` below `0.35` as "no"), and are documented as *values to be measured*, not truths. A
threshold moves only after a recorded run of real Jev over the goal set under
`docs/acceptance/`, the way the M08/M09 judge runs were recorded. A judge answer below the
acting threshold does nothing (D3: no diagnostic, no reorder) and is reported as
`unresolved` so the absence is visible.

D7. **Ranking asks for variants without touching the single-draft prompt.** `assemble_prompt`
gains `stance: Option<&Stance>`; `None` yields exactly today's bytes (every existing fixture
survives). `Some` appends one fenced `<stance>` block after the goal. Three fixed stances
(`Minimal`, `Verified`, `Explicit`) are the whole vocabulary; `drafts` is bounded to `1..=3`.
Candidates are ranked by `coverage_score - waste_penalty` with index as the deterministic
tiebreak; when the top candidate's `confidence < ACT_THRESHOLD` the FIRST draft (today's road)
is kept and `ranking.unresolved = true`.

D8. **The library is caller-supplied, closed-set only, and fills nothing the schema would not
accept.** A `GraphLibrary` is a directory of graph documents (`*.yaml`/`*.json`, the authored
format) with sidecars `<name>.template.json` declaring `parameters: { <name>: { question,
options: { <value>: description } } }` and where each value substitutes `{{name}}` in string
fields of the document. Only closed sets are parameters (a `Choice` per parameter); free text
is never a parameter. `reuse` fills and validates through the SAME chain as a draft (a filled
template that fails lint is refused, not repaired); `adapt` seeds the draft prompt with the
template inside a fenced `<seed>` block; `create` is today's road. There is no default
library directory and no bundled templates: an empty or absent library makes the decision
step a no-op.

D9. **Non-goal 10 is retired, on record.** `docs/harness/GRAPH_ARCHITECT.md` §5 item 10 ("multi-
draft judge panels") stops being a non-goal when Task 5 lands; the doc says so with the PR
number. Items 1–9 stand.

D10. **Three doors, one execute.** CLI `graph synthesize` gains `--judge-route`, `--drafts`,
`--library`; HTTP `POST /v1/graphs/synthesize` gains `judgeRoute`, `drafts`, `library`; the MCP
tool `synthesize` gains the same three fields. All three funnel through
`apps/cli/src/commands/architect.rs::execute` and return the same bytes.

## 3. The judge contract (what the compiler sends and reads)

Request: `{ "state": <object>, "model": "jev-latest", "questions": { "<id>": Question } }`.
Reply: `{ "model", "answers": { "<id>": Answer }, "usage": { input_tokens, output_tokens } }`.
`Choice` and `Score` answers carry `confidence` in `[0, 1]`; `Noul` carries only `noul`.
Question ids are the compiler's own, never sent to the model as meaning: every question carries
its full meaning in `instructions` and `criteria`. Errors: `401 → AuthRequired`,
`403 → PolicyDenied`, `422 → MalformedOutput` (the compiler built a bad question: a defect, not a
retry), `429 → RateLimited`, `529 | 5xx → ProviderUnavailable`.

## 4. Tests (Tier A, keyless, in the commit loop)

- Wire pin: the documented request/response examples round-trip byte-for-byte.
- Adapter: fake TCP server captures the request (Bearer header, path `/v1/systemone`, body) and
  answers each status; the key never appears anywhere but the header.
- Manifest: `typesafe` accepted for `direct_api`; the draft door refuses it; a non-`typesafe`
  route refuses the judge door.
- Golden untouched: `synthesize` and `synthesize_with(.., Extras::default())` produce the bytes
  of `expected.json`.
- Each judgment site: one fixture where the judge changes the outcome and one where it is below
  threshold and changes nothing (both arms informative).
- Policy edges: a test per threshold constant, `value - ε` and `value + ε`.
- Three-door byte identity for a call WITH a judge (recorded), extending the existing
  `api_http.rs` / `mcp_stdio.rs` cells.

## 5. Declared gaps

- Semantic usefulness of the judgments (does `on_goal` track what a person would say?) is Tier B:
  unmeasured until a recorded Jev run under `docs/acceptance/`. Thresholds are placeholders.
- The library has no templates; the first templates are authored by whoever first needs them.
- No Studio rendering of judgments or rankings (non-goal 8 stands).
- The MCP tool still posts to HTTP; no direct crate call.
