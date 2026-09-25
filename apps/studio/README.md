# GraphHelm Local Studio

A local-first control room where **people and their agents supervise AI agents together**, over an
append-only, hash-chained execution log. One page, one authenticated Runtime session: whether a
human clicks or an agent calls a [WebMCP](https://github.com/webmachinelearning/webmcp) tool, both
see the same result, and the log can always tell the two apart.

Studio talks to the **public Runtime API** and nothing else. It imports no Rust crate, reads no
event file, and holds no second copy of any rule. Every action it can take is an action the CLI
and the MCP surface can also take.

---

## What it does

- **Project display names and reversible run removal.** The project pencil renames the display
  label in this browser; it does not rename a folder or Runtime. A run's trash button removes it
  from the list after confirmation. It does not cancel execution or erase the event history.
  Removed runs remain available under **Removed from this browser** and can be restored. These
  preferences are scoped to the browser origin and configured project name. An unnamed/manual
  connection supports view-only changes and says so; failed storage writes also report view-only
  status. Disconnect resets the active view; named-project preferences survive reconnection.
- **Readable, navigable work.** The initial canvas frames the current nodes, agents and conversations.
  `fit` restores the overview; `fit selected` frames a node. **Find on board** opens and frames an
  exact node, agent or conversation, including on small screens. Node state, most recent actor and
  event sequence come from the log. An observed actor is not a claim of current task ownership.
  Conversation lines and verified dependencies have different styles. Project and conversation
  toggles free canvas space; hiding a draft composer preserves its text.

- **A messenger over the log.** The run's thread IS the event log — every line is composed from
  the event's own fields, with no model in the path and nothing paraphrased. Humans and agents
  talk by posting signals; personas can be chartered inside the log itself and appear on the
  canvas as crew, with their charters as birth certificates.
- **Group and pair conversations**, derived from envelope addressing (`to` / `replyTo`): the room,
  each agent pair's exchange, and your direct line with one agent — each its own window.
- **An attention verdict that explains itself.** "This run needs you" quotes the actual unanswered
  question (derived from the envelopes, never guessed), offers "answer X" that carries the
  question's id so the reply settles the ledger — and confesses "nothing has asked you anything
  yet" when that is the truth.
- **A canvas** with the run's nodes, the crew, and the conversation bubbles: drag, draw, annotate;
  positions and ink are the operator's own browser-local notes. Edges are drawn **only** when a
  graph file's semantic hash matches the hash the run itself recorded — a drawn arrow reads as
  evidence, so an unproven one is never drawn.
- **Search and citation.** The thread is searchable (filtering what the page already holds, and
  announcing what it hid), and every spoken turn carries its sequence — click to copy
  `executionId#sequence`, the log's own immutable address for that line.
- **A live tail that tells the truth**: incremental paged reads, an events identity that only
  changes when the log does, and a rail badge that flips to "stale" when the Runtime stops
  answering instead of wearing "live" over aging data.
- **Mutations, verified after the fact**: pause (both of the runtime's pauses — graceful and
  immediate), approve, resume, cancel (behind an in-place confirmation), sweep, amend a node's
  silence budget, start task, send message — each re-read from the log before being reported as
  done. A verb the current state makes illegal renders disabled with the reason; a button never
  pretends.

## WebMCP site tools

When the browser exposes a model-context surface (`document.modelContext` or
`navigator.modelContext`), the Studio registers **thirteen tools** after a connection exists and
removes them on disconnect. Support is optional: without it the page says so and every button
keeps working — the human interface is complete on its own. Phase 2's rule is parity: every
action button on the page has a page-tool twin, so an agent can do what the operator can — the
destructive one says so in its first word and still passes the host's own confirmation prompt.

| Tool | Effect |
|---|---|
| `graphhelm_list_executions` | Read. One summary row per run; exclusive `after` cursor, `limit` 1–100. |
| `graphhelm_get_attention` | Read. The attention verdict and the reasons behind it. |
| `graphhelm_get_execution_status` | Read. Aggregate state and the sixteen lifecycle state counts. |
| `graphhelm_get_execution_events` | Read. A page of the event log; exclusive `after` cursor. |
| `graphhelm_read_evidence` | Read. Opens one sealed content item by id. |
| `graphhelm_pause_execution` | **Write.** The runtime's two pauses by explicit `mode`: `graceful` (default — in-flight work finishes and is joined) or `immediate` (interrupts). |
| `graphhelm_approve_node` | **Write.** Readies one blocked or proposed node. |
| `graphhelm_resume_execution` | **Write.** Lifts the hold, given the graph path on the Runtime host. |
| `graphhelm_start_task` | **Write.** Starts a new supervised run from an objective. |
| `graphhelm_send_message` | **Write.** Posts a signal into the run's thread, with optional `to` / `replyTo` addressing. |
| `graphhelm_cancel_execution` | **Write, destructive.** Cancels the run; every unfinished node is recorded Cancelled. No undo on an append-only log. |
| `graphhelm_sweep_execution` | **Write.** Evaluates the run's customs stages now (`asOf` optional) and journals the result. |
| `graphhelm_amend_node_budget` | **Write.** Declares a silence budget for one node — the attention verdict's own remedy; `seconds` has no default anywhere on the path. |

Every schema is closed (`additionalProperties: false`) with bounded inputs. **A write tool never
claims success it has not verified**: each one reads the head, mutates, then reads the status and
the new events back, and returns `result` as `succeeded`, `refused` (with the diagnostic, and
nothing changed), or `unknown` (accepted but unverified — **not** done; re-read before acting).

A tool call confirmed by the person in the browser is still the agent's action: it is recorded
with actor type `agent` and id `studio-webmcp-adapter`, never as the operator. A refused call is
shown refused on the page, with the Runtime's own reason.

### Trying the WebMCP surface without an agent browser

The dev server (and only the dev server) can stand in for the browser's model context:

1. Take the auto-connect URL the dev server printed (`Studio auto-connect: http://127.0.0.1:<port>/?session=<nonce>` — port 4173 under plain `npm run dev`, 5183 under `studio-up.ps1`) and append `&webmcp-shim`.
2. In the console: `window.__webmcpShim.list()` — the ten tools, as the page registered them.
3. `await window.__webmcpShim.call("graphhelm_list_executions", {})` — a real read.

The tools register on connect, so the session nonce is part of the journey — a page opened
without it sits at the connect gate with nothing registered.

The shim never installs when the browser has a real `modelContext`, and does not exist in the
production bundle.

---

## Running it

One command, cold start included — starts (or reuses) the Runtime, installs dependencies on the
first run, starts the dev server, opens the browser connected:

```powershell
powershell -File apps/studio/tools/studio-up.ps1 -Events C:\path\to\.graphhelm\events
```

Or by hand: you need a Runtime (`graphhelm serve`) and the Studio dev server. `graphhelm init`
in the project creates every path below and prints these commands with the paths filled in
(`docs/install/GETTING_STARTED.md`).

```powershell
# 1 — start the Runtime on loopback. It writes the bearer token to <events-dir>.token. The
#     keyring pair is what lets it seal a message; without it the message box is refused:
$env:GRAPHHELM_EVENTS_KEY = (Get-Content -Raw C:\path\to\.graphhelm\serve.key).Trim()
graphhelm serve --events C:\path\to\.graphhelm\events --bind 127.0.0.1:8791 --keyring C:\path\to\.graphhelm\keyring --key-id studio

# 2 — start the Studio with the environment that lets it find both:
$env:GRAPHHELM_EVENTS = "C:\path\to\.graphhelm\events"
$env:GRAPHHELM_RUNTIME_URL = "http://127.0.0.1:8791"
npm --prefix apps/studio ci
npm --prefix apps/studio run dev
```

With `GRAPHHELM_EVENTS` set, the dev server hands the page its session and **the Studio opens
connected — nobody pastes a token.** Without it, the page shows a connect gate and the token can
be pasted from `<events-dir>.token`. The dev server proxies `/v1` and `/health` to
`GRAPHHELM_RUNTIME_URL` (default `http://127.0.0.1:8080`); requests stay same-origin so the token
never rides a cross-origin request.

### Production build

```powershell
npm --prefix apps/studio run build
```

`dist/` is a static bundle. Serve it from the same origin as the Runtime. The dev-only session
endpoint and WebMCP shim do not exist in the bundle; the connect gate is the entrance.

---

## Security

- **The token lives in memory only.** Never storage, a cookie, a URL, a log line, or an error
  message. Disconnect and reload both forget it. Disconnect also erases the browser-local board
  notes, unsent drafts, and the opened-content cache — erasure means erasure.
- **Agent actions are recorded as agents**, button presses as the operator. The audit log can
  always tell them apart, and the Studio never writes in a voice it does not have.
- **Everything the Runtime returns is untrusted content**, rendered as text through JSX. No
  `dangerouslySetInnerHTML`, no path that evaluates a payload.
- **Closed schemas and bounded inputs** on every tool; client-side bounds on ids, cursors, page
  sizes, message length, and the resume path (which is relayed to the Runtime, never read).
- **No telemetry, no external service, no webfonts fetched at runtime** (faces are vendored into
  the bundle).

---

## Development

```powershell
npm --prefix apps/studio test            # 218 unit + page-level tests (vitest + jsdom)
npm --prefix apps/studio run typecheck
npm --prefix apps/studio run build
```

The suite is behavior-first: page-level tests drive the real `App` with a stubbed Runtime client,
and most of them encode a defect found by an adversarial multi-agent review of the live page —
the comment above each test names the failure it pins. If you change behavior, expect a test to
say so.

Architecture notes live where the code is: each module opens with a comment stating what it owns
and the decisions it encodes (`src/App.tsx` for the shell and the live tail, `components/panel.tsx`
for the thread/ledger/composer, `graph/` for the board model and topology proof, `webmcp/adapter.ts`
for the tool surface). `docs/ux/STUDIO_SPEC.md` is the larger product it grows toward.

---

## Troubleshooting

The Studio now opens in **Overview**, a normal scrolling workspace with grouped agents,
conversations and node cards. Text stays at its reading size as the graph grows; there is no
automatic zoom in this view. Recorded agent activity and verified dependencies link to the
existing detail panels. Incomplete rosters and event-log disagreements remain visible.
**Free canvas** retains the existing draggable graph, drawings and saved positions. Switching
views does not rewrite those positions or mutate the operational graph. On narrow screens,
project and conversation drawers start closed so the workspace is immediately visible.

| Symptom | Cause and fix |
|---|---|
| Connect gate with "Runtime replied 502" | The dev proxy points at the wrong port. Set `GRAPHHELM_RUNTIME_URL` to the Runtime's real address and restart the dev server. |
| Gate instead of auto-connect | `GRAPHHELM_EVENTS` was not set when the dev server started. |
| "The bearer token was refused." | The pasted token is not the one at `<events-dir>.token`. The Runtime rewrites it only when it is missing. |
| Rail badge shows "stale" | Background reads have failed 3+ times in a row — the Runtime stopped answering. The badge recovers on the next successful read. |
| WebMCP shows no tool chips | This browser exposes no model-context surface. Expected outside an agent browser; use `?webmcp-shim` on the dev server to exercise the tools. |
| Resume is refused | The `file` path resolves **on the Runtime host**, not in the browser. |

---

## License

MIT, the same as the rest of the repository. See [`LICENSE`](../../LICENSE).


Free canvas starts with compact agent, conversation, and work-node lanes. Saved manual positions
still take precedence. **Organize** restores the default arrangement; **Undo layout** restores
the previous positions in the current view without deleting notes or drawings. Whole-board fit
never enlarges the map above 100%; selecting an item can zoom closer. Conversation links remain
separate from verified Runtime dependencies. Log disagreements expand on demand.


The execution map separates people, conversation previews, and work into labeled regions.
Previews reuse already-opened signal envelopes; they do not add Runtime calls. Empty node cards
show one honest waiting state; active cards show the last observed actor, update, and event
receipt. An observed actor is not an assignment. Initial framing keeps dense maps at a readable
scale; explicit Fit can show the whole map. Mobile opens one readable node and provides People,
Chats, Work, and graph-verification shortcuts. Run actions open separately from canvas tools.
Visual validation used fresh-context screenshot judges for idle, active, and mobile states.
