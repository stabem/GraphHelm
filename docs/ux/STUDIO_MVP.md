# Local Studio MVP — what shipped, and what it is not

`docs/ux/STUDIO_SPEC.md` specifies the whole Studio: graph canvas, DSL editing, chat, agents
panel, documents, Dreams, policies, settings. This document records the **first slice that
exists**, so a reader can tell the built surface from the specified one without reading code.

Status: shipped, issue #105. Source: [`apps/studio`](../../apps/studio). Operator instructions:
[`apps/studio/README.md`](../../apps/studio/README.md).

---

## 1. The journey it delivers

One operator question, answered end to end:

> *Which of my runs needs me, why, and can I unblock it from here — together with an agent?*

1. Connect to the local Runtime with the bearer token `graphhelm serve` wrote.
2. See every execution the store holds, each with its attention verdict.
3. Land on the run that needs attention, with the blocking node named.
4. Read the aggregate state, all sixteen node lifecycle states, and the event evidence.
5. Name the graph file the run started from and see the run's actual shape drawn — verified
   against the hash the run recorded, or refused with the reason.
6. Click a node to narrow the thread to that node alone.
7. Pause the run, approve the blocked node, resume it.
8. See, for each of those, what actually changed: head before, head after, the re-read status,
   the events appended, and who they are attributed to.
9. Reload the page and find the Runtime untouched and the token forgotten.

An agent holding the page's WebMCP site tools performs the read and write steps through the same
client, and the human view moves with it.

## 2. Architectural position

- **The Studio is not the monitor.** D-040 stands unchanged: the monitor stays local, read-only,
  simple, and gains no operational action. Nothing in this work adds a mutating verb to a
  `/monitor` route — that sub-router is still GET-only by construction.
- **Public contracts only.** The Studio imports no Rust crate and reads no event file. Every read
  and every write is an HTTP request to the Public Runtime API, the same one the CLI and the MCP
  server use.
- **WebMCP is not a second path.** The adapter in `apps/studio/src/webmcp/adapter.ts` builds no
  request and interprets no diagnostic; it calls the same `RuntimeClient` the buttons call. One
  place holds the rules, so the two surfaces cannot drift into disagreeing.

## 3. The contracts this added

### `POST /v1/graph/topology` — the shape, with a proof of which graph it is

An execution's log records which graph it started from as a HASH
(`execution_started.graphHash`) and never records its shape, so nothing public could say which
node feeds which. This route reads a graph FILE on the Runtime host and returns its entrypoints,
nodes and edges — plus the `semanticHash` that settles whether those edges belong to the run in
front of you.

**The hash is the contract, not a convenience.** A file path is a guess: the operator types it,
the file may have been edited since the run began, or it may be a different graph entirely. The
Studio compares this reply's hash against the one the run recorded and draws edges on a match and
only on a match — `verifyTopology` returns an empty edge list on anything else, so a component
that forgets to branch still cannot draw the wrong graph. A drawn arrow reads as evidence.

It grants no reach the API did not already have: `start` and `resume` both take a `file` and load
it. A POST rather than a GET because a filesystem path in a URL lands in request logs, browser
history and any `Referer` the page sends onward. Nodes carry identity only — id, type, name,
optionality — never objectives, agent blocks or completion controls, which are content.

Reached from all three surfaces: `graphhelm graph topology <file>`, the route, and the MCP tool
`topology`.

### `GET /v1/executions` — the execution index

It exists because the only prior way to discover which executions a store held was `GET /monitor`,
an HTML page for a human browser. Parsing its anchors made a presentation surface into an API.

- Authenticated like every other `/v1` route; unauthenticated and wrong-token callers get 401.
- Ordered by execution id; `after` is an **exclusive** cursor naming the last id already read;
  `limit` is 1–100 and defaults to 20. An over-large limit is **refused**, not clamped — a caller
  that asked for 500 and silently received 100 could not tell a clamp from a short store.
- Each row is a **key subset of the status reply for the same stream**, from the same replay and
  the same attention judgement: `executionId`, `mode`, `status`, `attention`, `startedAt`,
  `lastEventAt`, `headSequence`. It can carry fewer fields than the detail view; it can never
  disagree with it.
- Reached from all three surfaces: `graphhelm execution list`, `GET /v1/executions`, and the MCP
  tool `list`.

Guards: `every_index_row_field_equals_the_status_reply_for_the_same_execution`,
`the_index_cursor_is_exclusive_and_pages_reconstruct_the_whole_list`,
`the_index_refuses_an_over_large_limit_rather_than_clamping_it`,
`the_cli_index_and_the_api_index_agree_byte_for_byte` (all in `apps/cli/tests/api_http.rs`), and
`the_list_tool_reaches_the_execution_index_and_relays_it_verbatim` in
`apps/cli/tests/mcp_stdio.rs`.

## 4. Evidence, not acknowledgement

Every mutation the Studio performs — from a button or from a tool — follows the same four steps:
read the head, mutate with `If-Match`, read the status back, read the events the mutation
appended. The result is reported as one of three values, never as an HTTP status:

| `result` | Meaning |
|---|---|
| `succeeded` | The store moved and the re-read agrees. |
| `refused` | The Runtime said no. The diagnostic is carried; nothing changed. |
| `unknown` | The mutation was accepted but the verification read failed. **Not** done. |

The third value is the one that earns the design. Reporting an unverifiable write as `succeeded`
would make the single state an operator has to act on look exactly like the state they can ignore.

## 5. Actor attribution

| Who acted | `X-GraphHelm-Actor-Type` | `X-GraphHelm-Actor` |
|---|---|---|
| The person, via a button | `owner` | `studio-operator` |
| An agent, via a site tool | `agent` | `studio-webmcp-adapter` |

A browser's confirmation prompt for a site tool is **consent**, not authorship. The person agreed
to an action the agent chose; recording it as `owner` would destroy the only distinction the audit
log exists to preserve.

**The immediate pause, stated rather than assumed (Phase 2, #105):** `{"mode": "immediate"}` is
signalled on the cancel channel and the driver appends `execution_paused` later, asynchronously —
since #681 the request's actor and idempotency key ride that channel (`ImmediateCancelRequest`),
so the record names the caller like every other verb. The Studio does not take that on faith: it
reads the pause event back and reports the actor the append-only record holds, whatever it says
— which is how a regression on the Runtime side would surface as a visible mismatch rather than
as a claim the evidence repeats.

## 6. Deliberate omissions

- `cancel` was not a site tool in the MVP. Phase 2 (#105) makes it one, behind two confirmations
  (the page asks; the WebMCP host prompts) and a DESTRUCTIVE-first description — the contract change
  is recorded as ADR-036 in `docs/reference/REFERENCE_STACK_AND_ADRS.md` §41.
- No Graph DSL editor and no graph creation. The board DRAWS a run's graph and lets the operator
  arrange, annotate and draw on it; it never edits the graph itself. Editing one is a Graph Draft,
  which is a transactional, governed operation and not a canvas gesture.
- No embedded chat, no collaboration, no multi-user, no marketplace, no billing.
- No cloud, no hosted deployment, no remote login, no VPS story.
- No telemetry, no external service, no automatic paid fallback.
- No mobile layout. The surface targets a desktop control room.

## 7. Known limits

- **The Studio's own test suite is not part of `ci/gate.ps1`.** The gate covers the Rust
  workspace, schemas, and PostgreSQL; `npm --prefix apps/studio test` and `run build` must be run
  explicitly. Tracked separately as tech debt rather than bolted onto the gate here.
- **`resume` needs a graph file path on the Runtime host.** The Studio relays it and never reads
  it, which is the correct security posture and also means the operator must know the Runtime's
  working directory. There is no server-side "resume from the graph you started with" verb to
  call instead.
- **WebMCP is a proposal in flight.** The adapter probes `document.modelContext` and
  `navigator.modelContext` and removes its tools through an `AbortController` signal. A host that
  changes either shape will report `unavailable` until the adapter is updated — the page keeps
  working.
- **`GET /v1/executions` replays every listed stream** to decide each row's attention. That is
  what makes the index answer the operator's real question, and it bounds the page at 100 rows;
  it is not a design for a store with tens of thousands of streams.
- **Edges need the operator to name the graph file.** The Runtime cannot supply it: the topology
  is not persisted, only its hash. So the board is edgeless until someone types the path the run
  started from — a real step, and the reason the field sits in the board's own toolbar rather
  than buried in the action panel. The same field is what `resume` uses.
- **A verified topology is a claim about one run.** Selecting another run clears it; the Studio
  will not carry one run's shape onto another's nodes.
