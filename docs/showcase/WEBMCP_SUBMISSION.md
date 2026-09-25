# GraphHelm Studio — submission package (ABANDONED, 2026-09-03)

Drafted for the OpenAI Developer Showcase, whose WebMCP apps view stated at the time that examples
were coming soon (<https://developers.openai.com/showcase?view=webmcp-apps>); projects were to be
submitted through the developer community at <https://developers.openai.com/community>. Both are
recorded in the past tense on purpose -- they were true when this was written, they are not a route
anyone is taking, and neither was re-checked afterwards.

> **Disposition: ABANDONED, 2026-09-03.** The owner decided on 2026-09-03 to abandon this
> submission and refocus on the product and the MVP. **Nothing was ever submitted, and nothing is
> queued.** The document is retained as a record of the work and as reusable product material --
> the description, the tool inventory and the limitations below are accurate and are worth lifting
> elsewhere. It is not a deliverable waiting on a step. Every instruction-shaped passage further
> down (a capture checklist, a video script, form text to paste) describes what a submission
> *would* have needed. None of it is outstanding work, and none of it should be acted on.
>
> **If the submission is ever revived, that is a new decision, and this line is the thing it has to
> overturn.**
>
> And the paragraphs below are not contradicted by this one, so do not "tidy" the overlap away: every
> sentence under here was, and remains, literally TRUE. Nothing was submitted; no browser proof was
> captured. **The tense was the defect, not the facts** — a true statement of absence carrying no date
> and no disposition reads as *not yet* rather than *not ever*. Remove the dates and the past tense
> and the file goes straight back to reading as a queued deliverable.

This is **not** a hackathon, a competition, a prize, or a partnership, and nothing in this
document should be read as claiming one. It **was** a submission draft. **Nothing here was ever
submitted**, and the draft was abandoned rather than held open.

**Validation status, as it stood when the draft was abandoned:** repository tests exercised the
adapter, and no compatible browser host was ever observed registering and invoking these Site
tools. That browser proof was never captured -- and it is not owed. The video script and
screenshot list below record what a capture would have covered; they were never evidence that one
happened, and with the submission abandoned they are not a task anyone inherits.

---

## 1. Name

GraphHelm Studio

## 2. Tagline

Operate real agent runs together with Codex, from the same local control room.

## 3. Short description (≤ 300 characters)

A local-first control room for agent executions. Codex and the operator share one connected page
and one authenticated Runtime session: Codex can see which run needs attention, read the evidence,
pause it, approve a blocked node, and resume — every action recorded and verified.

## 4. Long description

GraphHelm Studio is a local-first control room for agent executions. The operator and Codex share
the same connected page and the same authenticated Runtime session. Through WebMCP site tools,
Codex can inspect attention, read execution evidence, pause work, approve a blocked node, and
resume the run. Every mutation uses GraphHelm's existing Public Runtime API, actor records,
idempotency keys, optimistic concurrency, and append-only event evidence.

Nothing about the page is a demonstration shell. The seven tools call the exact client the human
buttons call, against a Runtime running on the operator's own machine, writing to an append-only
event store that the CLI and the MCP server read from too. When Codex pauses a run, the page the
person is looking at changes — same execution, same head sequence, same event.

## 5. The problem

Agent frameworks are getting good at *running* work and bad at *handing it back*. When an
execution stops, someone has to find out which of many runs is waiting, what it is waiting for,
and what the safe next move is. Today, in GraphHelm, that is a CLI, an HTTP API, and an MCP
server — three surfaces, all textual, none of which a person and an assistant can look at
together.

The failure mode is specific: the assistant that could triage the problem is reading a transcript,
while the operator is reading a terminal. Neither can see what the other just did, and the
assistant has no safe, recorded way to act.

## 6. The solution

One page, two operators.

The Studio shows every run and the Runtime's own attention verdict — *needs you*, *can sleep*, or
*unknown* — with the blocking node named, the sixteen node lifecycle states, and the append-only
timeline. It then offers exactly three mutations: pause, approve a node, resume.

The same seven capabilities are registered as WebMCP site tools when the browser supports them.
Codex reads the same API, writes through the same client, and the human view follows. There is no
second code path to keep honest.

## 7. Why WebMCP matters here

Operating an agent system is not a chat problem — it is a **shared-state** problem. The two things
that have to be true are that the assistant sees exactly what the operator sees, and that the
operator can see exactly what the assistant did.

WebMCP gives both, and it gives them without the thing an operations tool must not have: a second
credential path. The page already holds an authenticated session the person opened. Site tools let
the assistant act inside that session — scoped to the page, revoked when the page disconnects,
never a token handed to a model, never a server-side integration to provision.

The alternative shapes are worse in specific ways. A remote MCP server needs its own credential
and cannot see the page. A screenshot-and-click agent has no contract and no audit trail. WebMCP
is the only one where the assistant's action and the person's action land in the same log, with
different names on them.

That last point is the design decision this project is most opinionated about. A browser
confirmation is **consent, not authorship**: a pause Codex asked for and the person approved is
written as actor type `agent`, id `studio-webmcp-adapter` — never as the owner. An audit log that
cannot tell those apart cannot answer the question it exists for.

## 8. How Codex and OpenAI tooling were used

- Codex is the intended operator-side agent: the journey below is driven by asking Codex, in a
  WebMCP-capable browser, which run needs attention and then to act on it.
- This first slice was built in a Codex worktree alongside Claude Code, against the repository's
  issue-first pipeline. It partially delivers open issue #105; it does not close the full Studio
  scope.
- The Studio makes **no** model API calls of its own. It has no provider SDK, no key, and no
  network destination other than the local Runtime. The demo costs nothing to run and needs no
  credentials.

## 9. The tools

| Tool | Kind | Input | What it does |
|---|---|---|---|
| `graphhelm_list_executions` | read | `after?`, `limit?` | One summary row per run: id, mode, status, attention, head sequence, instants. Exclusive cursor. |
| `graphhelm_get_attention` | read | `executionId` | The attention verdict and the reasons behind it, with the node named. |
| `graphhelm_get_execution_status` | read | `executionId` | Aggregate state and all sixteen node-state counts, zeroes included. |
| `graphhelm_get_execution_events` | read | `executionId`, `after?`, `limit?` | A page of the append-only log, oldest first, exclusive cursor. |
| `graphhelm_pause_execution` | **write** | `executionId` | Holds dispatch at the next safe boundary. Records a directly attributable pause decision. |
| `graphhelm_approve_node` | **write** | `executionId`, `node` | Readies one blocked or proposed node. |
| `graphhelm_resume_execution` | **write** | `executionId`, `file`, `fixtures?` | Lifts the hold and lets the run continue. |

Every schema is closed (`additionalProperties: false`) with bounded strings and integers. Reads
carry `readOnlyHint: true`. `cancel` is deliberately absent: it is the destructive verb, and the
journey does not need it.

Each write tool returns evidence, not an acknowledgement: the action, the execution, the node, the
actor, the idempotency key, the status heads before and after, the re-read status, the directly
attributable decision event, structured diagnostics, and a `result` of `succeeded`, `refused`, or
`unknown`. Concurrent writes and later driver events stay in the timeline without being claimed
as part of this action.

## 10. Architecture

```
Browser page (React + TypeScript, Vite)
├── RuntimeClient ─────────────── the only thing that speaks HTTP
│     auth · reads · mutations · structured errors · idempotency · If-Match
│     read head → mutate → re-read status → re-read events → evidence
├── Human interface  ──── calls RuntimeClient  (actor: owner / studio-operator)
└── WebMCP adapter   ──── calls RuntimeClient  (actor: agent / studio-webmcp-adapter)
          │
          ▼
GraphHelm Public Runtime API  (loopback only, bearer token)
  GET  /v1/executions                     ← added by this work
  GET  /v1/executions/{id}
  GET  /v1/executions/{id}/events
  POST /v1/executions/{id}/pause | approve | resume
          │
          ▼
Append-only event store (local, file-locked, replayable)
```

The Runtime is Rust. The Studio imports none of it: Studio code may use only public Runtime
API/CLI contracts, an architectural invariant the repository enforces. The same verbs are
reachable from the `graphhelm` CLI and from the stdio MCP server, and cross-surface tests compare
their answers byte for byte.

## 11. Security and privacy

- **Local only.** The Runtime binds loopback and refuses anything else. No cloud, no hosted
  service, no external destination.
- **The token lives in memory.** Never in `localStorage`, `sessionStorage`, a cookie, a URL, a log
  line, or an error message; it cannot be serialised out of the client object. Disconnect and page
  reload both forget it.
- **Agent actions are recorded as agent actions.** Consent in the browser is not authorship.
- **Every mutation is attributed and idempotent**, with optimistic concurrency via `If-Match`, and
  is verified by reading the store back.
- **Untrusted by default.** The page treats API responses and event payloads as untrusted content:
  rendered as text, never as markup, never evaluated. No `dangerouslySetInnerHTML` exists in the
  application.
- **Closed schemas, bounded inputs**, on every tool and in the client.
- **No telemetry. No analytics. No third-party requests. No automatic paid fallback.**
- The `resume` file path is relayed to the Runtime, never read or echoed by the page.

## 12. How to run it

No credentials, no model spend, no network.

```powershell
# 1. Seed a deterministic run that stops on a blocked node
$demo = Join-Path $env:TEMP 'graphhelm-studio-demo'
New-Item -ItemType Directory -Force -Path $demo | Out-Null
'{"nodeOutcomes":{"implementation":"failure"}}' | Set-Content -Encoding utf8 "$demo\blocked.json"
cargo +1.97.1 run --locked -p graphhelm-cli -- execution start `
  --file examples/graphs/manual-override-deploy.yaml `
  --events "$demo\events" --fixtures "$demo\blocked.json" `
  --mode supervised --execution demo-deploy

# 2. Start the Runtime (writes the bearer token to "$demo\events.token")
cargo +1.97.1 run --locked -p graphhelm-cli -- serve --events "$demo\events" --bind 127.0.0.1:8080

# 3. Start the Studio
npm --prefix apps/studio ci
npm --prefix apps/studio run dev
```

Open <http://127.0.0.1:4173>, paste the token, and connect.

## 13. Video script (60–90 seconds)

*Never recorded. Kept because it is an accurate walk through the demo path, which is worth having
written down; it is not a shot list anyone owes.*

| Time | Beat |
|---|---|
| 0:00–0:10 | **The problem.** Three terminals. "Something stopped. Which one, and why?" |
| 0:10–0:18 | Open GraphHelm Studio, connect with the local token. Two runs; one says *needs you*. |
| 0:18–0:26 | The blocked node is named. Show the sixteen state counts and the event timeline. |
| 0:26–0:34 | Show the seven tools under **Site tools**. Ask Codex: *"Which execution needs my attention and why?"* |
| 0:34–0:44 | Codex lists the runs, reads attention, names `demo-deploy` and its blocked node. |
| 0:44–0:56 | *"Pause it."* Confirm in the browser. Head 13 → 15; the pause event appears, attributed to the agent. The page moved with it. |
| 0:56–1:08 | *"Approve the blocked node."* Head 15 → 16; the node becomes ready. |
| 1:08–1:18 | *"Resume."* Head 16 → 20; the run moves again; the timeline fills in. |
| 1:18–1:26 | Show the audit: who did what, with idempotency keys. Reload — the Runtime is untouched, the token is gone. |
| 1:26–1:30 | Close: local-first, open source, no proprietary cloud dependency. |

## 14. Screenshots the submission would have carried

*Never captured, and not outstanding. The list stays because it names the five things worth
showing about this surface, which is useful independently of any submission.*

1. Connected control room: run list, attention verdict, state counts, timeline.
2. A run in `needs_you`, with the blocking node named in the reasons.
3. The seven site tools listed as available in the browser's Site tools surface.
4. The evidence card after a pause: `pause → succeeded`, head 13 → 15, actor `agent ·
   studio-webmcp-adapter`.
5. The evidence card after approve and resume, with the new events and their attribution.

## 15. Limitations

- The Studio is an **operator surface**, not the full Studio the repository specifies. Its board
  can draw a hash-verified execution topology, but it has no Graph DSL editor, graph mutation,
  drag-and-drop editing, embedded chat, or collaboration.
- No cloud, no hosted deployment, no remote login, no VPS story, no Enterprise or Education
  offering.
- No mobile layout.
- `cancel` is not exposed to tools.
- `resume` requires the graph file path as resolved **on the Runtime host**.
- WebMCP is a proposal in active flight; a host that changes the registration shape will show the
  tools as unavailable until the adapter is updated. The page keeps working without them.
- The demo runs on deterministic fixtures, not on live model calls.

## 16. Repository

<https://github.com/stabem/GraphHelm>

## 17. Pull request

Recorded as a partial delivery of open issue #105:
Internal development issue #105 (private archive).

## 18. Licence

MIT.

## 19. The form text that was drafted

*There is no form to paste it into. Retained because the wording is the tightest description of
the surface anyone wrote, and it is reusable in a README or a page.*

> **GraphHelm Studio — operate real agent runs together with Codex, from the same local control
> room.**
>
> GraphHelm Studio is a local-first control room for agent executions. The operator and Codex
> share the same connected page and the same authenticated Runtime session. Through WebMCP site
> tools, Codex can inspect attention, read execution evidence, pause work, approve a blocked node,
> and resume the run. Every mutation uses GraphHelm's existing Public Runtime API, actor records,
> idempotency keys, optimistic concurrency, and append-only event evidence.
>
> Seven site tools: four reads (list runs, attention, status, events) and three writes (pause,
> approve node, resume). Every schema is closed; reads are marked read-only; the destructive verb
> is deliberately not exposed. No write tool reports success it has not verified — each one reads
> the head, mutates with `If-Match`, then reads the status and its attributable decision event
> back, and answers
> `succeeded`, `refused`, or `unknown`.
>
> A browser confirmation is consent, not authorship: an action Codex chose is recorded as actor
> type `agent`, never as the owner, so the audit log can always tell the two apart.
>
> Everything runs on the operator's own machine over loopback. The bearer token lives in page
> memory only — never in storage, a URL, or a log — and is forgotten on disconnect and on reload.
> No telemetry, no third-party requests, no model spend to run the demo. MIT.
>
> Repository: https://github.com/stabem/GraphHelm
