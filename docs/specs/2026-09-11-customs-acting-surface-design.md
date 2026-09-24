# Customs acting surface — design (closes the acting half of M11)

Base for every citation: `origin/main` @ `585fa0c2` (2026-09-11). Issue anchor: #159 (design
sealed there; this document does not reopen it). Constituents: #132 (no verb completes external
work), #163 (status as scan history). Closed constituents consumed as-is: #160 (event family +
fold), #161 (clearance fold + registry), #162/#288 (dead-letter + sweep, three surfaces), #133
(per-node state map).

## 1. What is measured to exist, and what is measured to be missing

| piece | state | receipt |
|---|---|---|
| Eleven customs event kinds on the wire | exists | `core/protocols/src/event.rs:443-453` |
| Fold: open waits, open claims, clearances, scan history | exists | `core/events/src/projection.rs:447-489`, arms at `:1504` (`CompletionClaimed`), `:1552` (`CompletionCleared`) |
| Machine-replay clearance verifies the journaled evidence digest | exists | `projection.rs` `MachineReplay` arm; `claim_evidence_digest` at `:307` |
| Sweep verb on CLI, HTTP, MCP | exists | `core/events/src/sweep.rs`, `apps/cli/src/commands/execution/sweep.rs`, `serve/routes.rs:1251`, `mcp/tools.rs:182` |
| The false-ready cell (claimed-not-cleared releases nothing) | exists, green | `core/events/tests/execution_projection.rs:1617` |
| Stale-rendezvous fold behaviour (claim against a superseded wait is spent testimony) | exists | `execution_projection.rs:1561` |
| **A verb that appends `completion_claimed`** | **absent** | `grep -rn CompletionClaimed apps/cli/src core/runtime/src` → 0 producers; `REFUSAL_REASON_CODES` doc at `event.rs:77-81` says "NOTHING PRODUCES THESE YET" |
| **A verb that appends `completion_cleared`** | **absent** | same census; `ExecutionCommand` (`apps/cli/src/args.rs:310`) has Start/List/Status/Signal/Approve/AmendBudget/Pause/Resume/Sweep/Cancel |
| **A verb that appends `completion_refused`** | **absent** | no producer of any of the nine refusal codes |
| **Scan history on the status surface** | **absent** | `apps/cli/src/commands/execution/mod.rs:775` `render()` publishes no customs field; two stale branches (`origin/issue-163-*`, 286-466 commits behind, no PR) carry only a `quarantined_nodes` accessor and a typed stub |
| Deadlines on the CLI/HTTP start path | **None on every entry** | `stage_deadline` (`projection.rs:1223`) reads `current_graph`, which only the governor's draft publication sets (`core/governor/src/apply.rs:270`); `execution start` publishes a `GraphVersion`, never a `PersistedGraphVersion` (`apps/cli/src/commands/execution/start.rs:49`) |

The gap is therefore the SURFACE, exactly as the 2026-08-31 amendment on #159 states: zero claim
or clear verbs on any of the three surfaces, and no rendering of the timeline the fold already
keeps.

## 2. Decisions (orchestrator, owner's delegated authority — recorded here, restated nowhere)

D1. **Two verbs land: `claim` and `clear`.** `reject`, `dlq-redrive`, `dlq-return`,
`identity register/revoke` and any `Countersign` clearance do NOT land in this slice. Countersign
is honestly unavailable until D-047 / #529 gives the wire a signature; the `#527` trap goes red
the moment a production surface appends one, so the `clear` verb refuses `countersign` AT THE DOOR
with a stable diagnostic and appends nothing. DLQ verbs ride the same deferral (they have no
operator journey until a graph declares a dead-letter node).

D2. **Both verbs take the graph the execution started from** (`--file`, or `file`/`graph` on
HTTP), through the SAME file-trust seam `execution resume` already uses (`resume.rs:145-185`:
supplied content hash must equal the recorded `graph_hash`). The graph is needed for two facts
the projection cannot supply on the start path: the node's declared `proof_kinds` (the
evidence-budget refusal) and the spec the drive needs after a clearance. A second mechanism to
carry those facts would be a second trust seam.

D3. **A clearance that clears DRIVES.** After `completion_cleared` folds to `Cleared`, the node
is `Succeeded` and its dependents become dispatchable, but nothing in the tree dispatches them:
`resume_preconditions` refuses a non-paused execution (`core/execution/src/recovery.rs:57`), and
`start` refuses a started stream. So `clear` runs the same drive `resume` runs — sync
`drive_to_quiescence` with the fixture executor on the CLI and in fixture-only serve, the async
driver when serve has real wiring — with an EMPTY release set. A rejected clearance drives nothing.

D4. **Refusals are journal events, decided before the append and pinned by sequence.** The verb
replays, decides, and appends at the sequence it read (`PreparedAppend::new(.., at, ..)` +
`append_atomic`, which answers `SequenceConflict` when the stream moved — the #74 pattern
`core/events/src/sweep.rs` already uses). A refused claim appends `completion_refused` with the
named registry code; the node's state is untouched. Order of decision, first match wins:

| condition | code |
|---|---|
| node not in `WaitingInput` | `not_waiting` |
| a `--wait-seq` was given and it is not the node's open wait, but IS a `Parked` entry in the node's scan history | `stale_rendezvous` |
| a `--wait-seq` was given and no scan of this node ever parked at it | `unknown_wait` |
| an open claim already names this node | `duplicate_completion` |
| a declared `proof_kinds` entry is not among the presented `evidence[].kind` | `evidence_budget_unmet` |

`hash_mismatch` and `unknown_identity` are clearance verdicts the FOLD already produces;
`clearance_expired` has no producer until deadlines exist on this path (declared, §5);
`signature_unverifiable` is the door refusal of D1, which appends nothing (the code names the
diagnostic, not an event).

D5. **`clear` refuses a sequence that is not an open claim WITHOUT appending.** The fold treats a
clearance naming a non-claim as `Corrupt` (`projection.rs:1557-1560`); a verb that appended one
would poison every later replay. The refusal is a `GHCLI005_EXECUTION_STATE` diagnostic.

D6. **Scan history is ONE typed value rendered by `render()`** — the function both `execution
status` and `GET /v1/executions/{id}` already call (`status.rs:12-17`). The value is
`graphhelm_execution::CustomsView`, built from `projection.customs_scans`, `open_waits`,
`open_claims` and `clearances` with no second derivation. Deadlines are copied from the fold,
never recomputed. `quarantinedNodes` is the key set of `open_claims`, which IS the quarantine.

D7. **No schema changes.** Every event and node field this slice needs already exists in
`schemas/`. The slice adds surfaces, a view, tests, an example graph and records — it changes no
catalog and pins nothing.

D8. **Lanes.** The planner (this session) is neither author nor reviewer: implementer subagents
write the code, two independent reviewer subagents with fresh context write the passes, the
gate runs through the registered runner before the passes, and the planner presses. The
identity line names the subagent and the spawning session so the comment is addressable
(`Lane: <letter> · Session: subagent-<name> of <ListAgents name> [ref] · Head: <sha8>`). This
mapping was written into the lane loop §0 by this slice (retired 2026-09-24; see
`docs/process/DELIVERY.md`, History), because a rule that lives only in chat is the defect that
file existed to end.

## 3. The surfaces, exactly

### CLI

```
graphhelm execution claim  --file <graph> --events <dir> [--execution <id>] --node <id>
                           [--wait-seq <n>] --evidence <json-file> [--asserter <actor-id>]
                           [--mode operator_attested|machine_verified]
graphhelm execution clear  --file <graph> --events <dir> [--execution <id>] --claim-seq <n>
                           (--manifest-hash sha256:<64hex> | --evidence <json-file>)
                           [--fixtures <json>] [--verifier machine_replay]
```

`--evidence` is a JSON array of `{"kind": "...", "contentHash": "sha256:<64hex>", "size": <u64>}`
— the exact wire shape of `ClaimEvidence`. On `clear`, `--evidence` means "I hold this bundle;
compute its digest with `claim_evidence_digest` and present it", which is what a machine replay
IS; `--manifest-hash` presents a digest computed elsewhere. `--verifier` accepts only
`machine_replay`; `countersign` is refused with `GHCLI005_EXECUTION_STATE` and the message names
#529.

Both verbs reply with the same `render()` envelope every other mutation replies with, plus:

- `claim`: `"claim": {"outcome": "claimed"|"refused", "claimSeq": <n>|null, "reasonCode": <code>|null, "waitSeq": <n>}`
- `clear`: `"clearance": {"outcome": "cleared"|"rejected", "claimSeq": <n>, "reasonCode": <code>|null}`; when cleared, the drive's own status follows in the same envelope (as `resume` does).

### HTTP

`POST /v1/executions/{id}/claim` and `POST /v1/executions/{id}/clear`, mutation headers as every
other verb (`Idempotency-Key`, `X-GraphHelm-Actor`, `X-GraphHelm-Actor-Type`, optional
`If-Match`). Bodies:

```json
{"file": "<path>" | "graph": {...}, "node": "implementation", "waitSeq": 4,
 "evidence": [{"kind":"test_report","contentHash":"sha256:…","size":123}],
 "asserter": "agent-x", "mode": "operator_attested"}
{"file": "<path>" | "graph": {...}, "claimSeq": 5, "manifestHash": "sha256:…" | "evidence": [...],
 "fixtures": "<path>", "route": "<id>"}
```

`asserter` defaults to the `X-GraphHelm-Actor` header. Idempotency suffixes: `["claim"]` and
`["clear"]` — one decision event per request, the drive's own hops keep their own keys, exactly
as `resume` splits `resumed` from its redispatches.

### MCP

Tools `claim` and `clear`, appended to `TOOLS` after `sweep`; descriptions name the routes
(`POST /v1/executions/{executionId}/claim` / `/clear`) because `surface_completeness.rs` joins on
that text. Schemas are `mutating_schema` (closed, with `ifMatch`).

## 4. Tests that hold the sealed acceptance (#159 "Sealed acceptance", all four)

| cell | where | what goes red |
|---|---|---|
| Acting 4/4 on the CLI: park → claim refused (`evidence_budget_unmet`) → claim → deploy NOT dispatched → clear (wrong hash → `rejected`, deploy still not dispatched) → clear (right hash) → deploy runs → execution `completed` | `apps/cli/tests/customs_cli.rs` | any link of the chain |
| The same chain over HTTP, and the journal's event-kind sequence is identical to the CLI's | `apps/cli/tests/api_http.rs` | a surface that appends a different event, or in a different order |
| Trap guard: claim naming a superseded wait → `completion_refused{stale_rendezvous}` in the journal, node still `waiting_input`, on CLI and on HTTP | both files | a verb that answers "whichever wait is open" |
| Replay: `graph replay` of the journal twice is byte-identical, and a wrong-hash clearance replays as `rejected` deterministically | `customs_cli.rs` | replay reading a clock or a map order |
| Scan history parity: `data.customs` from `execution status` and from `GET /v1/executions/{id}` are byte-identical | `api_http.rs` | a second derivation on either door |
| The quarantined node renders `waiting_input` with `stage: claimed` and the downstream renders not `succeeded` | `customs_cli.rs` | a view that reads `open_claims` as released |
| Every refusal code the verb can produce is produced by exactly one arrangement, and the arrangement is asserted before the refusal is demanded (no cheap companion passes for the headline) | `core/events/tests/customs_verbs.rs` | a decision order that reaches the wrong code first |
| Countersign at the door: `--verifier countersign` appends nothing (head sequence unchanged) and names #529 | `customs_cli.rs` | a surface that lets the #527 trap fire |

## 5. Declared gaps (named here and in the PR body; no new issues, owner order 2026-09-05)

- **Deadlines are `null` on every stream `execution start` creates**, so `clearance_expired`
  has no producer and the sweep raises no `overdue_exception` on those streams. Cause, measured:
  `current_graph` is set only by `GraphVersionPublished`, which only the governor's draft
  publication emits. The clearance-sweep mechanism itself is proven at fold level
  (`core/events/tests/sweep_verb.rs`). Closing this needs the start path to publish a
  `PersistedGraphVersion` through the D-036 externalizer, which is its own lane (sealing key,
  evidence slots) and is not attempted here.
- **Countersign clearance, rejection, DLQ redrive/return, identity registry verbs** — D1.
- **The Studio does not render scan history** — `apps/studio/src` has no customs reader; the
  API now publishes it and the Studio consumes it in its own slice.
- **`#153` (gate-as-graph)** stays open; this slice supplies the completion verb its addendum
  requires and nothing else of it.
