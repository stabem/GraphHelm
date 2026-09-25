# GraphHelm MVP — integrated verification on `origin/main` (2026-09-14)

Verified head: `533df7b30ffd16166b7dc09ef69e0a93beacdeb0` (`origin/main`, "feat(1066): a useful change
lands … (#1073)"), checked out detached in `F:/projects/GraphHelm/.claude/worktrees/mvp-verify`
(removed at the end of this run).

Machine: Windows 11 Pro 10.0.26200, AMD Ryzen 9 9950X (16 cores), 62 GB RAM, git 2.47.1.windows.1,
Node v24.13.1, cargo 1.97.1 (pinned toolchain). Shell: PowerShell 5.1 for every command below
(Python 3.11 only to drive the MCP stdio processes).

Method: a stranger following only the written documents. Every command is the document's command,
paths substituted; where a document's command failed, that is recorded as a finding, not worked
around silently. Outputs are real and trimmed (`…`). Nothing here is redacted except the bearer
token, which never appears. Runtimes were bound on `127.0.0.1:8810`/`8811`/`8812` only; all
scratch lives under `C:/gh-target/mvp-verify/` (kept). Every process started here was stopped by
pid at the end.

Follow-up: the Studio and Runtime findings below (F1, F2, F5, F6, F7, F8, F9, F3) are tracked
in #1083; F4 is fixed by the PR that lands this record. The findings table is at the end.

Build (once):

```
PS> $env:CARGO_TARGET_DIR = 'C:/gh-target/mvp-verify'; cargo +1.97.1 build --locked -p graphhelm-cli -j 6
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 07s
C:\gh-target\mvp-verify\debug\graphhelm.exe --version   → graphhelm 0.1.0
npm --prefix apps/studio ci                             → exit 0
```

---

## 1. Promise 1 — `docs/install/GETTING_STARTED.md` (PowerShell path)

Fresh git project: `C:\gh-target\mvp-verify\project` (`git init`, one commit `init`).

### §2 `graphhelm init`

```
PS C:\gh-target\mvp-verify\project> graphhelm init --pretty --bind 127.0.0.1:8810
{
  "ok": true, "command": "init",
  "data": {
    "bind": "127.0.0.1:8810",
    "events":   { "path": ".graphhelm/events",      "state": "created" },
    "gitignore":{ "path": ".gitignore",             "state": "created" },
    "harnesses": [
      { "harness": "claude-code", "path": ".mcp.json", "state": "created", "note": "Claude Code reads this file from the project root; restart the session to pick it up." },
      { "harness": "codex", "path": ".graphhelm/codex.config.toml", "state": "created", "note": "Append this file's contents to ~/.codex/config.toml; init never writes to your home directory." }
    ],
    "key":     { "environment": "GRAPHHELM_EVENTS_KEY", "path": ".graphhelm/serve.key", "state": "created" },
    "keyring": { "keyId": "studio", "path": ".graphhelm/keyring", "state": "created" },
    "token":   { "path": ".graphhelm/events.token", "state": "created" },
    "next": [
      { "step": "export the sealing key …", "powershell": "$env:GRAPHHELM_EVENTS_KEY = (Get-Content -Raw 'C:\\gh-target\\mvp-verify\\project\\.graphhelm\\serve.key').Trim()" },
      { "step": "start the Runtime on loopback …", "powershell": "graphhelm serve --events 'C:\\gh-target\\mvp-verify\\project\\.graphhelm\\events' --bind 127.0.0.1:8810 --keyring 'C:\\gh-target\\mvp-verify\\project\\.graphhelm\\keyring' --key-id studio" },
      { "step": "start the Studio …", "powershell": "powershell -File apps/studio/tools/studio-up.ps1 -Events '…\\events' -Bind 127.0.0.1:8810 -Keyring '…\\keyring' -KeyId studio -GraphHelm graphhelm" },
      { "step": "start the first execution …", "powershell": "Set-Content -Path '…\\fixtures.json' -Value '{\"nodeOutcomes\":{\"implementation\":\"failure\"}}'; graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events '…\\events' --fixtures '…\\fixtures.json' --mode supervised --execution demo" }
    ],
    "project": ".", "root": ".graphhelm"
  },
  "diagnostics": []
}
```

`.mcp.json` as written (command is the absolute path of the binary that ran `init`, token as a file path):

```
{"mcpServers":{"graphhelm":{"command":"C:\\gh-target\\mvp-verify\\debug\\graphhelm.exe",
 "args":["mcp","--url","http://127.0.0.1:8810","--token-file","C:\\gh-target\\mvp-verify\\project\\.graphhelm\\events.token","--actor","agent-chat"]}}}
```

Matches the page (§2 expected shape; no token/key values printed; `next` filled in). Note the
`next` strings say bare `graphhelm` while the binary here is a built path — the page says so in §1.

### §3 Start the Runtime and prove it

```
PS> $env:GRAPHHELM_EVENTS_KEY = (Get-Content -Raw '…\project\.graphhelm\serve.key').Trim()
PS> Start-Process graphhelm.exe -ArgumentList serve --events '…\events' --bind 127.0.0.1:8810 --keyring '…\keyring' --key-id studio     # pid 69000
{"ok":true,"command":"serve.started","data":{"address":"127.0.0.1:8810","executors":{"model":false,"tools":false}},"diagnostics":[]}

PS> Invoke-RestMethod http://127.0.0.1:8810/health | ConvertTo-Json -Compress
{"ok":true,"command":"serve.health","data":{},"diagnostics":[]}
PS> try { Invoke-WebRequest http://127.0.0.1:8810/v1/executions -UseBasicParsing } catch { $_.Exception.Response.StatusCode.value__ }
401
PS> Invoke-RestMethod "http://127.0.0.1:8810/v1/executions?limit=5" -Headers @{ Authorization = "Bearer $token" } | ConvertTo-Json -Compress
{"ok":true,"command":"execution.list","data":{"executions":[],"hasMore":false,"nextCursor":null},"diagnostics":[]}
```

All three answers are the page's expected answers (no `warning` diagnostic on `serve.started`: the
key opened the keyring).

Extra probe (asked for by the brief — bearer on an unknown execution id, expected 404):

```
PS> Invoke-WebRequest "http://127.0.0.1:8810/v1/executions/nope" -Headers @{ Authorization = "Bearer $token" }
HTTP/1.1 200 OK
{"ok":true,"command":"execution.status","data":{"acceptedMutations":0,"attention":"can_sleep","attentionReasons":[],
 "customs":{…},"executionId":null,"executor":null,"headSequence":0,"lastEventAt":null,"mode":null,"nodeLastEventAt":{},
 "nodeStateCounts":{…all 0…},"nodeStates":{},"signalsRecorded":0,"silenceUnevaluated":[],"startedAt":null,"status":null,
 "untriagedInterruptions":[]},"diagnostics":[]}
```

→ **Finding F1**: an unknown id is answered `200` with an empty status (`executionId: null`,
`status: null`, `attention: can_sleep`), not `404`. The page does not claim 404, so this is not a
doc divergence, but a reader who typos an id is told the run "can sleep".

### §5 The first execution, then pause → approve → resume

```
PS F:\…\mvp-verify> Set-Content -Path '…\fixtures.json' -Value '{"nodeOutcomes":{"implementation":"failure"}}'
PS> graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events '…\events' --fixtures '…\fixtures.json' --mode supervised --execution demo
{"ok":true,"command":"execution.start","data":{"attention":"needs_you","attentionReasons":[{"kind":"blocked_node","node":"implementation"}],
 "executionId":"demo","executor":"fixture","mode":"supervised","nodeStates":{"deploy":"ready","implementation":"blocked"},"status":"running",…},
 "diagnostics":[GHG102_UNBOUNDED_CUSTOMS ×2, GHG101_DEFAULT_TIMEOUT ×2 (warnings)]}
```

The CLI verbs, in the page's order, trimmed to the page's own columns:

```
execution.pause    ok  status=paused   attention=needs_you  [{"kind":"blocked_node","node":"implementation"}]  deploy=paused implementation=blocked
execution.approve  ok  status=paused   attention=can_sleep  []                                                  deploy=paused implementation=ready
execution.resume   ok  status=running  attention=needs_you  [{"kind":"blocked_node","node":"implementation"}]  deploy=paused implementation=blocked
execution.status   ok  status=running  attention=needs_you  [{"kind":"blocked_node","node":"implementation"}]
```

Identical to the page's "Measured output" block, line for line.

### §4 + §5 The Studio

```
PS F:\…\mvp-verify> $env:GRAPHHELM_EVENTS='…\project\.graphhelm\events'; $env:GRAPHHELM_RUNTIME_URL='http://127.0.0.1:8810'; $env:GRAPHHELM_STUDIO_SESSION_NONCE='mvpverify0914nonce'
PS> npm --prefix apps/studio run dev -- --port 5190 --strictPort        # npm pid 57688, vite/node pid 67472
  VITE v8.2.2  ready in 565 ms
  ➜  Local:   http://127.0.0.1:5190/
```

No `Studio auto-connect:` line: with the nonce injected by the launcher variable the dev server
deliberately does not echo it (`apps/studio/vite.config.ts`), so the page's "Expected, among the
dev server's output" holds only without that variable (**F10**, info). Opened
`http://127.0.0.1:5190/?session=mvpverify0914nonce` in the Browser pane (desktop viewport 1280×720).

Observed, against the page's nine numbered steps:

1. Connected: `GraphHelm · LIVE` in the rail; `/__studio/session?nonce=…` → 200, `/health` → 200.
   The rail shows **no Runtime address and no actor** (F3).
2. Rail: `demo · 12:33 AM · demonstration` with an orange attention dot; the verdict text
   `RUNNING · NEEDS YOU` is in the header pill, not in the rail row.
3. Header: `demo  RUNNING · NEEDS YOU`; the blocking node is **not named in the header** — it is the
   `implementation` card marked `blocked` / `retryable failure` (F3).
4. Side panel: `succeeded 6 · nothing in the other 15 states`-style line (the sixteen counts,
   folded), and the event thread with `system-cli · 12:37 AM` lines — the "event log" of the page.
5. `Graph file on the Runtime host…` box exists (Free canvas) → filled with the absolute path of
   `examples/graphs/manual-override-deploy.yaml` → **connect** → `2 nodes · 1 connection drawn ·
   Connections verified: this file hashes to exactly the graph this run recorded.`
6. Clicking a node narrows the thread (not exercised further).
7. Buttons, in the page's order: `pause · finish in-flight` → toast `Paused · done (hq 26 → 27)`,
   header `PAUSED · NEEDS YOU`; `approve implementation` → toast `Approved implementation — done
   (log 27 → 28)`, header `PAUSED · CAN SLEEP`, card `implementation ready · approved ·
   studio-operator · now`; `resume` (first click switched to the Free canvas and highlighted the
   empty graph-file box — "the dock asks for the graph file path", as the page says; after step 5
   the Run actions menu offered `resume` enabled) → toast `Resumed — done (log 28 → 32)`, header
   `RUNNING · NEEDS YOU`.
8. After the Studio resume the run reads `implementation: waiting input / needs input · system-cli`
   and the dock says `Nothing is blocked. A waiting node wants an answer in the thread, not an
   approval.` CLI confirms:
   ```
   execution.status  status=running attention=needs_you [{"kind":"waiting_input_node","node":"implementation"}] nodes={"deploy":"paused","implementation":"waiting_input"} head=32
   ```
   → **Finding F2**: the page says "what happens next is decided by the fixture … with
   `implementation: failure` it fails again and the run is back at `blocked_node`". The Studio's
   resume has no fixture input, so the fixture executor gets no outcome and parks the node at
   `waiting_input` — a different end state from the one the page describes for this exact step.
9. Reload: the page reconnected through the `?session=` URL; the Runtime was untouched.

Screenshots (Browser pane, not saved as files): run list with `demo` selected; header
`demo RUNNING · NEEDS YOU`; after pause/approve/resume, `RUNNING · NEEDS YOU` with
`implementation waiting input`.

Console: `GET /v1/gateway/routes → 400 Bad Request` on every connect (the fixture-only Runtime has
no manifest); surfaces as a red "Failed to load resource" in the console (**F6**, low).

---

## 2. Promise 3 — the two-harness briefing (`CHANGELOG.md` "One rendering, three doors"; `apps/cli/tests/mcp_stdio.rs::a_second_harness_reads_the_same_briefing_the_first_one_left_in_the_store`)

Runtime: the 8810 one from §1. Driver: `scratchpad/mcp_drive.py` — spawns
`graphhelm mcp --url http://127.0.0.1:8810 --token-file <project>\.graphhelm\events.token --actor <actor>`
and speaks newline-delimited JSON-RPC on stdio.

### Harness A — `--actor claude-code` (pid 55632)

```
>>> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"mvp-verify","version":"0"}}}
<<< {"id":1,"jsonrpc":"2.0","result":{"capabilities":{"tools":{}},"protocolVersion":"2025-06-18","serverInfo":{"name":"graphhelm","version":"0.1.0"}}}
>>> {"jsonrpc":"2.0","method":"notifications/initialized"}
>>> {"jsonrpc":"2.0","id":2,"method":"tools/list"}
<<< 28 tools: start, list, topology, status, briefing, events, evidence, signal, approve, pause, resume, cancel, routes,
    wake_arm, wake_status, amend_budget, wake_wait, probe, resolve_contract, memory_status, present, compile_context,
    memory_propose, accounting, sweep, claim, clear, synthesize
>>> {"jsonrpc":"2.0","id":"claude-start","method":"tools/call","params":{"name":"start","arguments":{"executionId":"exec-two-harness","file":"F:\\…\\examples\\graphs\\manual-override-deploy.yaml","fixtures":"C:\\…\\project\\.graphhelm\\fixtures.json","mode":"supervised"}}}
<<< {"id":"claude-start","result":{"isError":false,"content":[{"type":"text","text":"{\"command\":\"execution.start\",\"data\":{\"attention\":\"needs_you\",\"attentionReasons\":[{\"kind\":\"blocked_node\",\"node\":\"implementation\"}],\"executionId\":\"exec-two-harness\",\"executor\":\"fixture\",\"headSequence\":13,\"mode\":\"supervised\",\"nodeStates\":{\"deploy\":\"ready\",\"implementation\":\"blocked\"},\"status\":\"running\",…},\"ok\":true}"}]}}
>>> {"jsonrpc":"2.0","id":"claude-approve","method":"tools/call","params":{"name":"approve","arguments":{"executionId":"exec-two-harness","node":"implementation"}}}
<<< {"id":"claude-approve","result":{"isError":false,"content":[{"type":"text","text":"{\"command\":\"execution.approve\",\"data\":{\"attention\":\"needs_you\",\"attentionReasons\":[{\"kind\":\"wedged_quiescence\"}],\"executionId\":\"exec-two-harness\",\"headSequence\":14,\"nodeStates\":{\"deploy\":\"ready\",\"implementation\":\"ready\"},\"status\":\"running\",…},\"ok\":true}"}]}}
--- claude-code exit 0 stderr: ''
```

(Approve without a pause leaves `wedged_quiescence` — exactly the order GETTING_STARTED §5 warns
about; kept as-is because the brief asked for `start` then `approve`.)

### Harness B — a fresh `--actor codex` process

```
>>> initialize (as above) … notifications/initialized
>>> {"jsonrpc":"2.0","id":"codex-briefing","method":"tools/call","params":{"name":"briefing","arguments":{"executionId":"exec-two-harness"}}}
<<< {"id":"codex-briefing","result":{"isError":false,"content":[{"type":"text","text":"{\"command\":\"execution.briefing\",\"data\":{…},\"ok\":true}"}]}}
--- codex exit 0 stderr: ''
```

`data` over MCP (harness B) — and `graphhelm execution briefing --events '…\project\.graphhelm\events' --execution exec-two-harness --pretty` on the CLI — are **byte-identical** (`C:\gh-target\mvp-verify\briefing-mcp.json` vs `briefing-cli.json`):

```json
{
  "asOfSequence": 14,
  "decisions": [
    { "actor": { "id": "claude-code", "type": "agent" }, "detail": "implementation approved -> ready",
      "kind": "approval", "node": "implementation", "sequence": 14 }
  ],
  "executor": "fixture",
  "graphHash": "sha256:aa9b0715df457c2a1a364c1ab5ddee2c88bc390742c078fe6725b4b823e8a9bb",
  "graphVersion": 13,
  "name": "Deploy com override manual",
  "nextStep": { "kind": "diagnose", "reason": { "kind": "wedged_quiescence" } },
  "objective": "Produzir build implantável.",
  "pending": [ { "kind": "wedged_quiescence" } ],
  "unevaluated": [],
  "workDone": []
}
```

The two non-English strings are verbatim store values of an execution that already lived in the shared store before this run: `name` "Deploy com override manual" means "Deploy with manual override", and `objective` "Produzir build implantável." means "Produce a deployable build.".

`objective` is the operator's words persisted at start; the one decision names `claude-code`
(agent); `nextStep` is `diagnose` carrying the hazard whole — the CHANGELOG's ordering
(`diagnose` before a resume is offered) holds.

---

## 3. Promise 4 — `docs/product/PROVIDER_LESS_MODE.md`

Empty directory `C:\gh-target\mvp-verify\plm` (0 items before), every `GRAPHHELM_*` variable
removed (`env GRAPHHELM_* count: 0`), commands run from the repository root (`F:\…\mvp-verify`).

```
PS> '{"nodeOutcomes":{"implementation":"success","deploy":"success"}}' | Set-Content -Path <tmp>/fixtures.json
PS> graphhelm execution start --file examples/graphs/provider-less-demo.yaml --events <tmp>/events --fixtures <tmp>/fixtures.json --execution demo --mode supervised
{"ok":true,"command":"execution.start","data":{"attention":"can_sleep","executionId":"demo","executor":"fixture","mode":"supervised",
 "nodeStates":{"deploy":"succeeded","implementation":"succeeded"},"status":"completed",…},"diagnostics":[4 lint warnings]}

PS> (journal.jsonl, the execution_form_declared line)
…"kind":{"data":{"executionId":"demo","executor":"fixture","name":"Provider-less demonstration deploy","nodeIds":["deploy","implementation"],"nodeTimeoutSeconds":{},"objective":"Produce a deployable build."},"type":"execution_form_declared"}…

PS> graphhelm serve --events <tmp>/events --bind 127.0.0.1:8811          # pid 66684
{"ok":true,"command":"serve.started","data":{"address":"127.0.0.1:8811","executors":{"model":false,"tools":false}},"diagnostics":[]}
```

Monitor, with the cookie bootstrap the page describes (`<tmp>/events.token` was written beside the store):

```
GET /monitor/demo?token=<token>     → 303, Location: /monitor/demo   (cookie set)
GET /monitor/demo (with cookie)     → 200
   demo Demonstration run — started under the fixture executor: outcomes at start were supplied by a fixture file, not produced by a model or a tool.
   status: completed · can sleep — nothing is waiting on you · head: 11 · rendered: 2026-09-14T03:35:52… · read-only (D-040): this page mutates nothing …
```

```
PS> graphhelm execution status --events <tmp>/events --execution demo --html <tmp>/demo.html
{"ok":true,"command":"execution.status","data":{…"executionId":"demo","executor":"fixture","headSequence":11,"status":"completed"…}}
   demo.html contains the sentence: True; contains a refresh tag: False

PS> graphhelm execution list --events <tmp>/events
{"ok":true,"command":"execution.list","data":{"executions":[{"attention":"can_sleep","executionId":"demo","executor":"fixture","headSequence":11,"lastEventAt":"2026-09-14T03:35:20.807535400+00:00","mode":"supervised","startedAt":"2026-09-14T03:35:20.636097+00:00","status":"completed"}],"hasMore":false,"nextCursor":null},"diagnostics":[]}

PS> graphhelm events backup --repository <tmp>/events --output <tmp>/demo-backup.json     (server still up)
{"ok":true,"command":"events.backup","data":{"blobCount":0,"journalBytes":11973,"lockHeld":true},"diagnostics":[]}
   demo-backup.json: {"archiveVersion":"1.0.0","blobs":{},"journal":"{\"artifacts\":[]…
   /health after the backup → {"ok":true,"command":"serve.health"…};  GET /v1/executions (bearer) → row "executor":"fixture"

PS> Stop-Process 66684
PS> graphhelm graph synthesize --goal "check that the repository builds and summarize the result" --fixture core/architect/fixtures/first-compile/replies.json --allow-program cargo --out <tmp>/synthesized.json
ok=True command=graph.synthesize rounds=1 metadata={"executionId":"exec_27fe0b8b","id":"arch_27fe0b8b_v1","labels":{"origin":"architect","template":"32236956…"},"name":"check that the repository builds and summarize the result","version":1} terminal=summarize nodes=build_check,summarize
```

Every fenced command of the page ran and answered as the page shows (ids, `rounds: 1`, terminal
`summarize`).

The brief also asked for `execution start` of the synthesized document → `completed`:

```
PS> graphhelm execution start --file <tmp>/synthesized.json --events <tmp>/events2 --fixtures <tmp>/fixtures.json --mode autopilot --execution first-compile
ok=True status=running attention=needs_you nodes={"build_check":"waiting_input","summarize":"ready"}
```

→ not completed with the page's fixture (it names `implementation`/`deploy`, not `build_check`/
`summarize`), which is the page's own rule ("a node the file does not name gets `waiting_input`").
The page never claims this start completes (**F12**, info). With a fixture naming both nodes:

```
PS> '{"nodeOutcomes":{"build_check":"success","summarize":"success"}}' → fixtures-compile.json
PS> graphhelm execution start --file <tmp>/synthesized.json --events <tmp>/events3 --fixtures <tmp>/fixtures-compile.json --mode autopilot --execution first-compile
ok=True status=completed attention=can_sleep executor=fixture nodes={"build_check":"succeeded","summarize":"succeeded"}
```

---

## 4. Main journey — `docs/operations/TOOLS_ONLY_RUNTIME.md` + `docs/acceptance/useful-change-2026-09-13.md`

`<tmp>` = `C:\gh-target\mvp-verify\uc`.

§0 The project:

```
git init project; src/lib.rs = "// scratch\n"; commit "scratch"
HEAD before: d8e8b62496375534c8d59a0ec4ffa4ab9d9042eb
git -C project grep -n FIXED -- src/lib.rs   → exit 1
```

§1 The key — the doc's command as written, in a fresh `<tmp>`:

```
PS> $env:GRAPHHELM_EVENTS_KEY = <64 hex>
PS> graphhelm gateway keyring init --keyring <tmp>/keyring --key-id useful-change
{"ok":false,"command":"gateway.keyring.init","data":null,"diagnostics":[{"code":"GHCLI010_GATEWAY_CREDENTIAL","severity":"error","message":"the keyring directory does not exist","path":"/keyring","source":"gateway-cli"}]}
```

→ **Finding F4**: neither `TOOLS_ONLY_RUNTIME.md` nor the acceptance record says the keyring
directory must exist first (`PROVIDER_LESS_MODE.md` does, in a quoted refusal). A `serve` started
after that refusal came up with `GHCLI006_SERVE_INVALID` at `/keyring` ("the keyring could not be
opened … sealed operations … will refuse") — honest, and stopped (pid 66628). After `mkdir keyring`:

```
{"ok":true,"command":"gateway.keyring.init","data":{"createdUnder":"GRAPHHELM_EVENTS_KEY","keyId":"useful-change"},"diagnostics":[]}
```

§4 `serve`, tools only (no manifest/broker/route, no `GRAPHHELM_GATEWAY_KEY`):

```
PS> graphhelm serve --events <tmp>/events --bind 127.0.0.1:8812 --staging <tmp>/staging --allow-program git --tests-runner git --keyring <tmp>/keyring --key-id useful-change     # pid 55348
{"ok":true,"command":"serve.started","data":{"address":"127.0.0.1:8812","executors":{"model":false,"tools":true}},"diagnostics":[]}
```

§3/§5 The change graph (the document's YAML verbatim → `<tmp>\useful-change.yaml`), started over HTTP:

```
POST /v1/executions/exec-useful-change/start  (Authorization: Bearer, Idempotency-Key: useful-change-start, X-GraphHelm-Actor: owner-local, X-GraphHelm-Actor-Type: owner)
     body {"file":"<tmp>\\useful-change.yaml","mode":"autopilot","project":"<tmp>\\project"}
200 {"ok":true,"command":"execution.start","data":{"attention":"can_sleep","executionId":"exec-useful-change","executor":"gateway","headSequence":15,"mode":"autopilot",
     "nodeStateCounts":{…"succeeded":3…},"nodeStates":{"apply_fix":"succeeded","land":"succeeded","run_tests":"succeeded"},"status":"completed",…},"diagnostics":[]}
```

§6 The ref, in the project:

```
git rev-parse refs/graphhelm/executions/exec-useful-change      → fb355785eec0263934fe22813249067f5df8248f
git log --format='%H %s' refs/graphhelm/executions/exec-useful-change
   fb355785eec0263934fe22813249067f5df8248f fix: add the FIXED constant (landed by the execution)
   d8e8b62496375534c8d59a0ec4ffa4ab9d9042eb scratch
git show refs/graphhelm/executions/exec-useful-change:src/lib.rs
   // scratch
   pub const FIXED: bool = true;
git rev-parse HEAD        → d8e8b62496375534c8d59a0ec4ffa4ab9d9042eb   (unchanged)
git branch --list         → * master ;  git status --short → (clean) ;  staging entries: 0
```

§7 The sealed evidence through the API — each of the three nodes' `node_outcome_recorded` carries
four refs (`…-a1-record`, `-stdout`, `-stderr`, `-accounting-receipt`):

```
GET /v1/executions/exec-useful-change/evidence/exec-exec-useful-change-land-a1-record → data.content:
{"tool":"repository","action":"commit","actor":"runtime","programAllowlist":["git"],"tier":"tier_1","disposition":{"kind":"completed","exit_code":0},
 "stdoutSha256":"e3b0c442…","stdoutBytes":0,"stderrSha256":"e3b0c442…","stderrBytes":0,"truncated":false,"reused":false,
 "commit":"fb355785eec0263934fe22813249067f5df8248f","landedRef":"refs/graphhelm/executions/exec-useful-change"}
GET …/evidence/exec-exec-useful-change-run_tests-a1-stdout → data.content:
src/lib.rs:2:pub const FIXED: bool = true;
```

§9 Verify and double replay; §8 the synthesized proof on the same server:

```
PS> graphhelm events verify --repository <tmp>/events
{"ok":true,"command":"events.verify","data":{"formatSupported":true,"verified":false},"diagnostics":[]}       (the documented local answer)
PS> graphhelm graph replay --events <tmp>/events | sha256   (twice; `sha256` was a helper of the recording session, see the note below)
c2ec9887d12f96fda60a41f4fabea2135cdb5706130152d4b3c6204e3d8438b5
c2ec9887d12f96fda60a41f4fabea2135cdb5706130152d4b3c6204e3d8438b5
serve stderr: (empty)

PS> git -C <tmp>/project worktree add --detach <tmp>/landed refs/graphhelm/executions/exec-useful-change
HEAD is now at fb35578 fix: add the FIXED constant (landed by the execution)
PS> graphhelm graph synthesize --goal "$(cat core/architect/fixtures/useful-change/GOAL.txt)" --out <tmp>/prove-fixed.json --allow-program git --fixture core/architect/fixtures/useful-change/replies.json
synth ok=True id=exec_1c792165 nodes=run_tests rounds=1
PS> graphhelm graph lint <tmp>/prove-fixed.json
{"ok":true,"command":"graph.lint","data":{"errors":[],"warnings":[{"code":"GHG101_DEFAULT_TIMEOUT",…"path":"/spec/nodes/run_tests/timeoutSeconds"…}]},"diagnostics":[]}
POST /v1/executions/exec_1c792165-landed/start    {"file":"<tmp>\\prove-fixed.json","mode":"autopilot","project":"<tmp>\\landed"}
   http=200 ok=True status=completed nodes={"run_tests":"succeeded"}
POST /v1/executions/exec_1c792165-unchanged/start {"…","project":"<tmp>\\project"}
   http=200 ok=True status=failed    nodes={"run_tests":"failed"}
```

**On `sha256` (Codex on PR #1084):** it is not a PowerShell 5.1 command and not a declared prerequisite; the recording session had it as a shell helper that hashed the piped bytes with SHA-256. The reproducible form is `graphhelm graph replay --events <tmp>/events | Set-Content -NoNewline -Encoding utf8 replay.json; (Get-FileHash replay.json -Algorithm SHA256).Hash`. The two digests above were NOT re-derived with it (declared gap, 2026-09-15): what the record proves is that the two runs matched each other, which the helper's identical output on both runs shows regardless of its exact framing of the bytes.

Every row of the acceptance record's "What this run establishes" table reproduced on this head:
real tools with no model credential; one tree for three nodes; ref landed (`fb355785…`, parent
`d8e8b624…`); HEAD/branches/tree untouched; staging empty; runner's real stdout sealed; record
names `commit` + `landedRef`; synthesized graph completes on the landed tree and fails on the
unchanged one; replay byte-identical twice.

---

## 5. Studio growth and legibility on `main` (#1056, #1079)

8810 Runtime holding: `demo`, `exec-two-harness`, `exec_feature` (six nodes,
`examples/graphs/software-feature.yaml`, started over HTTP with an all-success fixture →
`status=completed`, all six `succeeded`), plus two tasks started from the composer.

Composer (`+ New task in this runtime`): `MODEL: none wired yet — No model is wired to this
Runtime, so the first node will wait instead of thinking.`; textbox `Describe the work in your own
words. It becomes the first node's objective, verbatim.`; hint `ENTER SENDS · SHIFT+ENTER FOR A NEW
LINE`.

- Task 1 `Summarize the README and list the three next steps`: **Enter did not send** (no POST in
  the network log); `start this task` sent `POST /v1/executions/run-157122f3-…/start → 200`. The
  Studio opened the run: header `Summarize the README and list the three next steps  RUNNING ·
  NEEDS YOU`, side panel `This run needs you — WAITING FOR YOUR ANSWER`, the demonstration sentence,
  `OBJECTIVE` = the typed text, node `start · waiting input`, `answer in the thread` offered.
- Task 2 `Draft release notes for version 0.1.0 from the changelog`: Enter again did nothing; the
  button sent `POST /v1/executions/run-133395ee-…/start → 200`. → **Finding F5** (observed twice
  through the Browser pane's synthetic key event).

Rail (screenshot): `demo · 12:33 AM · demonstration` (orange dot), `exec-two-harness · 12:34 AM ·
demonstration` (orange), `exec_feature · 12:37 AM · demonstration` (green), `Draft release no… ·
12:38 AM · demonstration` (orange), `Summarize the RE… · 12:37 AM · demonstration` (orange).
Composer-started tasks are named by objective (truncated to ~16 chars at the default rail width);
CLI/HTTP-started runs are named by execution id, not by the objective the store holds for them
(`exec_feature`'s objective, the store value `Localizar componentes, dependências e testes relacionados.`
— English: "Locate related components, dependencies and tests." — is shown only after selecting it)
(**F7**). The non-English values in this section are verbatim contents of executions that already
lived in the shared store before this run; they are quoted as data, not written as prose. Every row says `demonstration` — correct: all were fixture runs.

Overview of the 6-node run (`exec_feature`, 1280×720): header `exec_feature  COMPLETED · CAN
SLEEP`; `6 nodes · 0 active · 1 agents · 0 conversations`; an amber banner `Evidence needs
attention · 6 findings` (a completed fixture run has no evidence; the banner reads as an alarm on a
run that needs nothing) (**F9**); the six cards (`docs`, `implement`, `map_repository`, `plan`,
`review`, `tests` — alphabetical, not graph order) are mostly hidden behind the fixed action dock;
a wheel scroll over the main area did not reveal them (`get_page_text` shows all six with
`succeeded · 4 events · Dependencies awaiting evidence.`). Legibility: the first row of cards is
readable; the grid below the dock is not reachable at this viewport without resizing.

Free canvas, first framing (screenshot): `0 running / 0 blocked`, zoom `60%`, two of six cards
(`docs`, `implement`) visible at the right edge, the other four off-canvas; `▸ 6 log disagreements`
line. `fit` → zoom `15%`: all six as unreadable thumbnails (**F8**). #1079 claims "frames the
canvas to its content"; on this run neither the first framing nor `fit` gives a readable whole.

Mobile preset (375×812, reload through the session URL — screenshot): single column, rail
collapsed behind the hamburger, header pill `demo RUNNING · NEEDS YOU`, toolbar wraps onto a
second row, `Work overview` and the `implementation waiting input` card readable, dock buttons
wrap to three rows and stay tappable. Legible; only the header's second row is wasted space.

---

## Findings

| # | Where | What failed or diverged from the document | Severity |
|---|---|---|---|
| F1 | Runtime HTTP (§1) | `GET /v1/executions/<unknown id>` with a valid bearer answers `200 execution.status` with `executionId:null, status:null, attention:can_sleep` instead of `404`. | medium |
| F2 | GETTING_STARTED §5 step 8 vs Studio | The Studio's `resume` cannot pass a fixture file; after a Studio resume the node parks at `waiting_input`, while the page says the fixture decides (`blocked_node` again with `failure`). Same verb on the CLI (with `--fixtures`) matches the page. | medium |
| F3 | GETTING_STARTED §5 steps 1–4 | Rail shows no Runtime address or actor; header names the verdict, not the blocking node; "sixteen lifecycle counts" and "event log" live in the side panel in folded form. Doc drift against the #1056/#1079 Studio. | low |
| F4 | TOOLS_ONLY_RUNTIME.md §"A tools-only server", acceptance §1 | `gateway keyring init` as written refuses with `GHCLI010_GATEWAY_CREDENTIAL: the keyring directory does not exist`; the precondition is stated only in PROVIDER_LESS_MODE.md. | low |
| F5 | Studio composer | `ENTER SENDS` is printed; Enter did not send (twice); only the button did. | medium |
| F6 | Studio on a fixture-only Runtime | `GET /v1/gateway/routes → 400` on every connect, red console error. | low |
| F7 | Studio rail | CLI/HTTP-started runs are listed by execution id although the store holds their objective; only composer-started tasks are named by objective (and truncated). | low |
| F8 | Studio free canvas, 6 nodes | First framing shows 2 of 6 cards at 60%; `fit` drops to 15% (unreadable). | medium |
| F9 | Studio overview, 6 nodes | Card grid hidden behind the fixed action dock at 1280×720, wheel scroll did not reveal it; `Evidence needs attention · 6 findings` banner on a completed fixture run. | low |
| F10 | GETTING_STARTED §4 | The `Studio auto-connect:` line is not printed when `GRAPHHELM_STUDIO_SESSION_NONCE` is set (by design); the page's expected output holds only for the by-hand path. | info |
| F11 | `init` `next` block | Commands say bare `graphhelm` even when `init` ran from a built path; `.mcp.json` carries the absolute path. The page says so in §1. | info |
| F12 | PROVIDER_LESS_MODE flow | Starting the synthesized graph with the page's fixture parks `build_check` at `waiting_input` (the page's own rule; it never claims completion). Completes with a fixture naming its nodes. | info |
| F13 | Promise 3 order | `approve` before `pause` over MCP leaves `wedged_quiescence`; briefing's `nextStep` is `diagnose` — as GETTING_STARTED warns. Not a defect. | info |

Nothing in the four documents' fenced commands failed except F4; no command had to be replaced.

## Promise matrix

| promise | evidence (this record + merged PR) | open gaps |
|---|---|---|
| 1 — clone → `init` → Runtime → first execution → Studio → act on it | §1; #1070 (init + journey), #1056/#1079 (Studio) | F2 (Studio resume ≠ page's fixture-decided outcome), F3 (page text vs redesigned Studio), F5 (Enter does not send), F1 (unknown id 200) |
| 3 — continuity across harnesses (briefing on CLI/HTTP/MCP, actors named) | §2; #1071 | none found — MCP and CLI briefings byte-identical, decision names `claude-code`; `workDone` naming the landed ref is declared out of scope in #1073 |
| 4 — provider-less guarantee (start, serve, monitor, html, list, backup, synthesize) | §3; #1069 | none in the documented flow; the Studio half remains an observed-by-hand claim (the page says so) — observed here: sentence on the panel, `demonstration` on the rail |
| useful change with no model credential (tools-only Runtime, ref landing) | §4; #1073 | F4 (keyring dir precondition undocumented in the two tools-only docs); merging the ref stays the operator's act (declared) |
| Studio growth/legibility | §5; #1056, #1079 | F7, F8, F9 (naming, framing, dock occlusion), F6 (routes 400 noise) |

## Processes and cleanup

Started and stopped by pid: `graphhelm serve` 69000 (8810), 66684 (8811), 66628 → 55348 (8812);
`npm` 57688 + vite/node 67472 (Studio 5190); the two `graphhelm mcp` processes exited on stdin
close (55632, and the codex one). Verify worktree removed by name; `C:/gh-target/mvp-verify/`
kept (binary, `project/`, `plm/`, `uc/`, logs, `briefing-*.json`, `mcp-*.log`).
