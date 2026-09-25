# Provider-less mode — the guarantee

**A clean GraphHelm installation with no gateway manifest, no keyring, no credentials and no
network runs a complete execution, shows it on the monitor and the Studio, and exports it.**

That is a promise, not a description of what happens to work today. It is held by
`apps/cli/tests/providerless_journey.rs`, which runs every command on this page from the
repository root against an empty store directory, with the process environment scrubbed — no
`GRAPHHELM_*` variable reaches the binary — and then reads this page back and refuses if a fenced
command here is not one the journey ran, or one the journey ran is missing here, in either shell.
The commands below are the test's commands, and the outputs are real, trimmed to the part that
matters.

"No network" is a statement about what the flow is GIVEN, not a socket-level sandbox: no
manifest, no key and no route enter any command, and the executor the run declares
(`core/simulation/src/executor.rs`) answers from a map by node id and has no network code path.

This page is MVP promise 4 of `docs/product/ROADMAP_AND_ACCEPTANCE.md` §3.2.1 (issue #302,
declared by #1064).

## What works with nothing

| Surface | Command | What you get |
|---|---|---|
| The fixture executor | `execution start --fixtures <file>` | every node's outcome comes from the fixture file; the run completes, parks, or fails exactly as scripted, and the journal records `executor: fixture` |
| The server, fixture-only | `serve --events <dir> --bind <addr>` | the HTTP API, the read-only monitor, and async starts over the same store — with no manifest, no broker, no keyring |
| The monitor | `GET /monitor/{id}` | the run as a page that refreshes itself and mutates nothing |
| The incident snapshot | `execution status --html <path>` | the same page, frozen to a file |
| The index | `execution list`, `GET /v1/executions` | one row per run, each carrying the executor it was declared under — what the Studio rail marks |
| The export | `events backup --repository <dir> --output <file>` | the whole store as one JSON archive (`archiveVersion`, `journal`, `blobs`), taken under the store's own lock, key-free because a local store is not encrypted at rest |
| The compiler | `graph synthesize --goal … --fixture <replies>` | a graph from a goal, with the model's replies read from a recorded fixture instead of a gateway |

## What does not work, and says so

Nothing on this page reaches a model, a tool, or the network. Where the product would need one,
it refuses in words rather than pretending:

- **A real model call.** `execution start` never calls a model: under `--fixtures` the executor
  answers from the file, and a node the file does not name gets `waiting_input` — in the
  executor's own words, "nobody said what this node does. Waiting is honest; inventing a success
  is not." The road to a real model is `serve` with the real-executor group, and a half-given
  group is refused before anything starts: `GHCLI006_SERVE_INVALID` with
  `--manifest, --broker, --route and --staging must be given together or not at all`, then
  `the real-executor flags require --keyring and --key-id as well`, then
  `the real-executor flags require --allow-program: the set of programs an execution may spawn is
  declared per run, never defaulted`.
- **A real tool node.** The same executor answers a `tool` node by its id like any other node; no
  program is spawned. A program allowlist exists only on the real-executor road above, and it has
  no default (#583).
- **Sending a sealed message without a keyring.** `execution signal` refuses with
  `GHCLI001_ARGUMENT_INVALID` at `/keyring`, and the message is the remedy:

  > this signal would be recorded with no seal. If this store has no keyring, make one: create an
  > empty directory, then run `gateway keyring init --keyring <dir> --key-id <id>` with
  > GRAPHHELM_EVENTS_KEY set to 64 lowercase hex characters, then pass the same "keyring" and
  > "key-id" here. That command does not create the directory

  The demonstration below sends no signal.

## How a demonstration run is labelled

A completed fixture run used to be byte-indistinguishable from a real one. Since #1063 the
executor is declared at start and persisted with the run's shape: the journal's
`execution_form_declared` event carries `"executor":"fixture"` (or `"gateway"` when `serve` was
given the real-executor group). Every surface that shows the run reads that fact:

- `execution start`, `execution status` and `GET /v1/executions/{id}` publish it as
  `data.executor`; `execution list` and `GET /v1/executions` carry it on every row —
  `"fixture"`, `"gateway"`, or `null` on a stream recorded before the field existed.
- The monitor page and the `--html` snapshot print, between the run's title and its status line,
  exactly:

  > Demonstration run — started under the fixture executor: outcomes at start were supplied by a fixture file, not produced by a model or a tool.

- The Studio's run panel prints the same sentence under the run's headline, and the rail writes
  "demonstration" beside the run's name; nothing of the kind appears for a `gateway` run or an
  undeclared one.

The sentence says **"started under"** and **"at start"** deliberately. The fact it reads is the
executor declared when the run began. A fixture file that omits a node makes the fixture executor
answer `waiting_input` for it, and a run resumed later through a server that was given real wiring
is not re-declared on the form today — the label claims the start-time provenance and nothing
beyond it.

## The flow

Build once — `cargo build --locked -p graphhelm-cli` — which produces `target/debug/graphhelm`
(`target\debug\graphhelm.exe` on Windows) and puts nothing on `PATH`. Every command below is
written as `graphhelm …`; either run `cargo install --locked --path apps/cli` first, which installs
that name, or replace `graphhelm` with the built path (`./target/debug/graphhelm`,
`.\target\debug\graphhelm.exe`). Run the commands **from the repository root**: the flow names
repository-relative inputs (`examples/…`, `core/architect/fixtures/…`). `<tmp>` is any empty
directory you choose for the store and the outputs; that directory is the only thing the flow
writes outside `target/`. Add `--pretty` to any command to read its reply.

Each step is given in bash and in PowerShell; the two spell the same command.

Write the fixture — every node succeeds:

```bash
echo '{"nodeOutcomes":{"implementation":"success","deploy":"success"}}' > <tmp>/fixtures.json
```

```powershell
'{"nodeOutcomes":{"implementation":"success","deploy":"success"}}' | Set-Content -Path <tmp>/fixtures.json
```

Start a run. No key, no account, no network:

```bash
graphhelm execution start \
  --file examples/graphs/provider-less-demo.yaml \
  --events <tmp>/events \
  --fixtures <tmp>/fixtures.json \
  --execution demo \
  --mode supervised
```

```powershell
graphhelm execution start `
  --file examples/graphs/provider-less-demo.yaml `
  --events <tmp>/events `
  --fixtures <tmp>/fixtures.json `
  --execution demo `
  --mode supervised
```

```json
{
  "ok": true,
  "command": "execution.start",
  "data": {
    "attention": "can_sleep",
    "executionId": "demo",
    "executor": "fixture",
    "mode": "supervised",
    "nodeStates": { "deploy": "succeeded", "implementation": "succeeded" },
    "status": "completed"
  }
}
```

The journal says the same thing, as a recorded fact:

```
$ grep execution_form_declared <tmp>/events/journal.jsonl
…"kind":{"data":{"executionId":"demo","executor":"fixture","name":"Provider-less demonstration deploy","nodeIds":["deploy","implementation"],"nodeTimeoutSeconds":{},"objective":"Produce a deployable build."},"type":"execution_form_declared"}…
```

Serve the store — nothing but the directory and a bind address:

```bash
graphhelm serve \
  --events <tmp>/events \
  --bind 127.0.0.1:0
```

```powershell
graphhelm serve `
  --events <tmp>/events `
  --bind 127.0.0.1:0
```

```json
{"ok":true,"command":"serve.started","data":{"address":"127.0.0.1:65080"},"diagnostics":[]}
```

The monitor's token is written beside the store (`<tmp>/events.token`). Open
`http://127.0.0.1:<port>/monitor/demo?token=<token>` once; the server sets a cookie, redirects
to the clean URL, and the page reads:

```
demo
Demonstration run — started under the fixture executor: outcomes at start were supplied by a fixture file, not produced by a model or a tool.
status: completed · can sleep — nothing is waiting on you · head: 11 · read-only (D-040) …
```

`GET /v1/executions/demo` with `Authorization: Bearer <token>` answers the same `data.executor`
the CLI printed, and `GET /v1/executions` lists `demo` with `"executor": "fixture"` on its row;
the Studio, connected to this server, shows the sentence on the run's panel and the word
"demonstration" on the rail.

Leave the server running for the next three steps: each of them is proven with it up.

Freeze the page to a file (`status` only reads):

```bash
graphhelm execution status \
  --events <tmp>/events \
  --execution demo \
  --html <tmp>/demo.html
```

```powershell
graphhelm execution status `
  --events <tmp>/events `
  --execution demo `
  --html <tmp>/demo.html
```

`<tmp>/demo.html` is the monitor page minus its refresh tag, sentence included, and the JSON on
stdout carries `"executor": "fixture"` like `start` did.

List the store's runs — the rows the API index serves and the rail draws:

```bash
graphhelm execution list \
  --events <tmp>/events
```

```powershell
graphhelm execution list `
  --events <tmp>/events
```

```json
{"ok":true,"command":"execution.list","data":{"executions":[{"attention":"can_sleep","executionId":"demo","executor":"fixture","headSequence":11,"lastEventAt":"2026-09-13T22:36:00.112868+00:00","mode":"supervised","startedAt":"2026-09-13T22:35:59.906962+00:00","status":"completed"}],"hasMore":false,"nextCursor":null},"diagnostics":[]}
```

Export the store. The backup takes the store's own read lock, so it is consistent while `serve`
is running — the journey takes it with the server up and checks the server is still alive after:

```bash
graphhelm events backup \
  --repository <tmp>/events \
  --output <tmp>/demo-backup.json
```

```powershell
graphhelm events backup `
  --repository <tmp>/events `
  --output <tmp>/demo-backup.json
```

```json
{"ok":true,"command":"events.backup","data":{"blobCount":0,"journalBytes":11873,"lockHeld":true},"diagnostics":[]}
```

`<tmp>/demo-backup.json` is one JSON object with `archiveVersion` (`"1.0.0"`), `journal` (the
JSONL text, `execution_form_declared` included) and `blobs`. `events restore --repository <dir>
--archive <file>` brings it back into any empty directory.

Stop the server (Ctrl-C). Compile a graph from a goal, with the model's replies read from the
recorded fixture:

```bash
graphhelm graph synthesize \
  --goal "check that the repository builds and summarize the result" \
  --fixture core/architect/fixtures/first-compile/replies.json \
  --allow-program cargo \
  --out <tmp>/synthesized.json
```

```powershell
graphhelm graph synthesize `
  --goal "check that the repository builds and summarize the result" `
  --fixture core/architect/fixtures/first-compile/replies.json `
  --allow-program cargo `
  --out <tmp>/synthesized.json
```

```json
{
  "ok": true,
  "command": "graph.synthesize",
  "data": {
    "document": {
      "metadata": { "id": "arch_27fe0b8b_v1", "executionId": "exec_27fe0b8b", "version": 1 },
      "spec": { "completion": { "terminalNodes": ["summarize"] } }
    },
    "rounds": 1
  }
}
```

That document enters the system through `execution start --file` exactly as an authored one does.

## What this page does not promise

A run whose outcomes a model or a tool actually produced. That road exists — `serve` with
`--manifest --broker --route --staging --keyring --key-id --allow-program` — and it needs a
gateway manifest and credentials. The per-graph portability manifest of #116 and a `graphhelm
export` verb are separate surfaces and are not built. The Studio's demonstration label is held
only by the component tests beside `panel.tsx` and `rail.tsx`, which run under jsdom
(`apps/studio/vite.config.ts`), not in a browser: they prove the sentence is rendered from the
`executor` field, nothing more. Rendering in a real browser is a browser-driven journey, which
runs only in an observer-enabled environment and is otherwise `OBSERVER_MISSING` by the
repository's own rule — the Studio half of this page is therefore an observed-by-hand claim, not a
gated one.
