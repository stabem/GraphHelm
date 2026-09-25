# Clean-machine run of GETTING_STARTED, Studio included — 2026-09-14

One run, PARTIAL (see the Result row), of [`docs/install/GETTING_STARTED.md`](../install/GETTING_STARTED.md) on a
machine that held only the operating system, from the prerequisites to the Studio in a browser,
`events verify` and a double replay (#1094, promise 1 of #302). It is top to bottom **after the
reorder this PR makes**, not a strict pass of the page as it stood: the page ran rustup's `curl`
line before the apt line that installs `curl` (F0), and this run installed the apt prerequisites
first. The earlier
[`install-rehearsal-2026-09-13.md`](install-rehearsal-2026-09-13.md) stopped before the Studio;
`mvp-integrated-2026-09-14.md` re-ran the documents on a development machine with the toolchain
already present. This record is neither.

| | |
|---|---|
| Commit under test | `34208846f66c6c6c89442b8dcd991e079195ca0a` (origin/main) |
| Machine | a fresh `docker.io/library/ubuntu:24.04` container `gh-clean-1094`, run with Podman 5.8.2 (WSL machine `podman-machine-default`) on a Windows 11 host, started with `sleep infinity` |
| Image | index digest `sha256:a61567bd31828687156d735ea8eb01ba4e37636e225dd6a48ba94136a70d9d61`; linux/amd64 manifest `sha256:224a1869083a311ef3f13648a154ba79832fbef6364d31493642ca03082da254`; image id `b2b7ea366714…`, created 2026-09-07 |
| Inside | `Ubuntu 24.04.4 LTS`, `x86_64`, `nproc` = 8; no git, curl, cargo, rustup, node, npm, graphhelm or sudo at start |
| Ports | container `4173` (Studio) → host `127.0.0.1:15173`; container `8791` (Runtime) → host `127.0.0.1:18791` |
| Source | the page's `git clone` failed (the repository is private, F1); declared method, as in the 2026-09-13 rehearsal: `git archive` of the commit above (tar sha256 `f95fc6b5…8c57`, identical on host and in the container), extracted to `~/GraphHelm` |
| User | every page command after `apt-get` ran as the non-root user `rehearsal` |
| Started / ended | `2026-09-14T14:41:52Z` / `2026-09-14T14:58:42Z` (container side) |
| Result | **PARTIAL — `OBSERVER_MISSING` for the page as reordered.** Every command of the page ran on a clean machine, in the order this PR documents (apt before rustup) and with a source archive standing in for the authenticated clone (F1); the reordered page itself and the authenticated clone were not re-executed on a clean machine. Findings below |
| Secrets | token and key appear as SHA-256 digests only; the Studio session nonce is `<nonce>`; a sweep of every log for the raw token and key found 0 occurrences |
| Cost | none |

**How the transcript was taken.** Four scripts (phases A–D) ran as root in the container, echoing
each command before running it and its exit status after. The terminal half ran from the
orchestrating session's subagent `rehearsal-1094`; the browser half was driven by the
orchestrator from the Windows host. The container logs were meant to be copied out before the
container was removed, and that copy failed: the host shell expanded `$(basename …)` before it
reached WSL. The container was already gone. Every block below is therefore taken from the
session's captured tool output of the same runs, trimmed: progress output of `apt-get`, `rustup`
and `cargo` is cut to its last lines, long JSON envelopes are cut where marked `…`.

## A. Prerequisites and the binary (§ Prerequisites, §1)

**Order.** At `34208846` the page gives the rustup line (`curl … sh.rustup.rs`) before the bare-Ubuntu
apt line, and a fresh image has no `curl` (`command -v curl` → `curl: not found` at the start of
this run). Followed as written, the rustup step fails with `curl: command not found` (F0). This
run did not execute that failing order: it ran the apt prerequisites first, then rustup, as below.
This PR moves the apt block above the rustup block so the page's order is the order that ran.

```text
$ sudo apt-get install --yes --no-install-recommends build-essential ca-certificates curl git pkg-config
/phase-a.sh: line 24: sudo: command not found
[exit 127]
(no sudo in a root container; the same line without sudo, after the package index is fetched)

$ apt-get update -qq
[exit 0]

$ apt-get install --yes --no-install-recommends build-essential ca-certificates curl git pkg-config
done.
[exit 0]

$ git --version
git version 2.43.0

$ useradd --create-home --shell /bin/bash rehearsal
[exit 0]

$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.97.1
[exit 0]

$ . "$HOME/.cargo/env"; cargo +1.97.1 --version
cargo 1.97.1 (c980f4866 2026-06-30)

$ git clone https://github.com/stabem/GraphHelm.git   (GIT_TERMINAL_PROMPT=0: no terminal to type a username into)
Cloning into 'GraphHelm'...
fatal: could not read Username for 'https://github.com': terminal prompts disabled
[exit 128]
(declared source method: git archive of the commit, extracted to ~/GraphHelm)

$ tar -xf /src.tar -C /tmp && mv /tmp/src /home/rehearsal/GraphHelm && chown -R rehearsal:rehearsal /home/rehearsal/GraphHelm
[exit 0]

$ cd GraphHelm && cargo +1.97.1 install --locked --path apps/cli
   Compiling graphhelm-cli v0.1.0 (/home/rehearsal/GraphHelm/apps/cli)
    Finished `release` profile [optimized] target(s) in 5m 23s
  Installing /home/rehearsal/.cargo/bin/graphhelm
   Installed package `graphhelm-cli v0.1.0 (/home/rehearsal/GraphHelm/apps/cli)` (executable `graphhelm`)
[exit 0]
build wall time: 323 s

$ graphhelm --version
graphhelm 0.1.0
```

## B. `init`, `serve`, the first execution (§2, §3, §5) and MCP (§6)

```text
$ cd /path/to/your-project   (= /home/rehearsal/project, a fresh git work tree)
$ graphhelm init --pretty
{
  "ok": true,
  "command": "init",
  "data": {
    "bind": "127.0.0.1:8791",
    "events": { "path": ".graphhelm/events", "state": "created" },
    "gitignore": { "path": ".gitignore", "state": "created" },
    "harnesses": [],
    "key": { "environment": "GRAPHHELM_EVENTS_KEY", "path": ".graphhelm/serve.key", "state": "created" },
    "keyring": { "keyId": "studio", "path": ".graphhelm/keyring", "state": "created" },
    "next": [ … the four bash/powershell commands, with /home/rehearsal/project paths … ],
    "project": ".",
    "root": ".graphhelm",
    "token": { "path": ".graphhelm/events.token", "state": "created" }
  },
  "diagnostics": []
}
[exit 0]

$ stat --format '%U:%G %a %n' .graphhelm/events.token .graphhelm/serve.key .graphhelm/keyring
rehearsal:rehearsal 600 .graphhelm/events.token
rehearsal:rehearsal 600 .graphhelm/serve.key
rehearsal:rehearsal 700 .graphhelm/keyring

$ cat .gitignore
# GraphHelm Runtime working directory: bearer token, event store, sealing key.
.graphhelm/
token_sha256=c964293718f6a5111accb31bbc4012d410316dbb14cdd30a90243d47d5f7c7a9
key_sha256=23b5e6c2ce0e541611d0ad2cd904bb17a410beded33703a7fa4cbe1ee0bc21aa

$ graphhelm init --harness claude-code      (§6's documented step when no harness was detected)
{"ok":true,"command":"init","data":{…"harnesses":[{"harness":"claude-code",…"path":".mcp.json","state":"created"}],…"token":{…"state":"existing"}},…}
[exit 0]

$ cat .mcp.json
{ "mcpServers": { "graphhelm": {
    "args": ["mcp","--url","http://127.0.0.1:8791","--token-file","/home/rehearsal/project/.graphhelm/events.token","--actor","agent-chat"],
    "command": "/home/rehearsal/.cargo/bin/graphhelm" } } }
token and key unchanged

$ export GRAPHHELM_EVENTS_KEY="$(cat /home/rehearsal/project/.graphhelm/serve.key)"
$ nohup graphhelm serve --events /home/rehearsal/project/.graphhelm/events --bind 127.0.0.1:8791 --keyring /home/rehearsal/project/.graphhelm/keyring --key-id studio > ~/serve.log 2>&1 &
$ cat ~/serve.log
{"ok":true,"command":"serve.started","data":{"address":"127.0.0.1:8791","executors":{"model":false,"tools":false}},"diagnostics":[]}

$ curl --silent --fail http://127.0.0.1:8791/health; echo
{"ok":true,"command":"serve.health","data":{},"diagnostics":[]}
$ curl --silent --output /dev/null --write-out '%{http_code}\n' http://127.0.0.1:8791/v1/executions
401
$ curl --silent --header "Authorization: Bearer $(cat …/events.token)" 'http://127.0.0.1:8791/v1/executions?limit=5'; echo
{"ok":true,"command":"execution.list","data":{"executions":[],"hasMore":false,"nextCursor":null},"diagnostics":[]}

$ cd /path/to/GraphHelm   (= /home/rehearsal/GraphHelm)
$ echo '{"nodeOutcomes":{"implementation":"failure"}}' > /home/rehearsal/project/.graphhelm/fixtures.json
$ graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events /home/rehearsal/project/.graphhelm/events --fixtures /home/rehearsal/project/.graphhelm/fixtures.json --mode supervised --execution demo
{"ok":true,"command":"execution.start","data":{"acceptedMutations":0,"attention":"needs_you","attentionReasons":[{"kind":"blocked_node","node":"implementation"}],…"executionId":"demo","executor":"fixture",…"mode":"supervised",…"nodeStates":{"deploy":"ready","implementation":"blocked"},…"status":"running",…},"diagnostics":[GHG102_UNBOUNDED_CUSTOMS ×2, GHG101_DEFAULT_TIMEOUT ×2 (warnings)]}
[exit 0]

$ curl … 'http://127.0.0.1:8791/v1/executions?limit=5'
{"ok":true,"command":"execution.list","data":{"executions":[{"attention":"needs_you","executionId":"demo","executor":"fixture","headSequence":13,…,"mode":"supervised",…,"status":"running"}],"hasMore":false,"nextCursor":null},"diagnostics":[]}

$ command -v claude codex || echo "no chat harness installed"
no chat harness installed
(not the page's command: the registered stdio server driven by hand — initialize, then tools/call graphhelm_list_executions as §6 said)
{"id":1,"jsonrpc":"2.0","result":{"capabilities":{"tools":{}},"protocolVersion":"2025-06-18","serverInfo":{"name":"graphhelm","version":"0.1.0"}}}
{"error":{"code":-32602,"message":"no tool named \"graphhelm_list_executions\" is part of this server"},"id":2,"jsonrpc":"2.0"}
(tools/list on the same server: accounting, amend_budget, approve, briefing, cancel, claim, clear, compile_context, events, evidence, list, memory_propose, memory_status, pause, present, probe, resolve_contract, resume, routes, signal, start, status, sweep, synthesize, topology, wake_arm, wake_status, wake_wait)
```

## C. The Studio (§4)

```text
$ node --version
bash: line 1: node: command not found
[exit 127]
(not on the page at this commit: the official nodejs.org v22 tarball, checksum-verified, into ~/.local/node)
tarball: node-v22.23.2-linux-x64.tar.gz
node-v22.23.2-linux-x64.tar.gz: OK
$ node --version; npm --version
v22.23.2
10.9.8

$ export GRAPHHELM_EVENTS=/home/rehearsal/project/.graphhelm/events
$ export GRAPHHELM_RUNTIME_URL=http://127.0.0.1:8791
$ npm --prefix apps/studio ci
found 0 vulnerabilities
[exit 0]
npm ci wall time: 13 s

$ npm --prefix apps/studio run dev   (nohup, > ~/studio.log)
> @graphhelm/local-studio@0.1.0 dev
> vite --host 127.0.0.1
  Studio auto-connect: http://127.0.0.1:4173/?session=<nonce>
  VITE v8.2.2  ready in 363 ms
  ➜  Local:   http://127.0.0.1:4173/

(inside the container)
$ curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:4173/
200
$ curl -s http://127.0.0.1:4173/health; echo
{"ok":true,"command":"serve.health","data":{},"diagnostics":[]}

(from the Windows host, through the published ports)
http://127.0.0.1:15173/        -> code=000 exit=52
http://127.0.0.1:15173/health  -> code=000 exit=52
http://127.0.0.1:18791/health  -> code=000 exit=52
```

The dev server bound `127.0.0.1` inside the container, which a published port cannot reach (F4).
It was stopped by PID and restarted with the smallest change that works, inside the container only:

```text
$ npm --prefix apps/studio run dev -- --host   (nohup, > ~/studio.log)
> vite --host 127.0.0.1 --host
  Studio auto-connect: http://127.0.0.1:4173/?session=<nonce>
  VITE v8.2.2  ready in 343 ms
  ➜  Local:   http://localhost:4173/
  ➜  Network: http://172.24.33.103:4173/  eth0

(from the Windows host)
http://127.0.0.1:15173/                   -> code=200   <title>GraphHelm Local Studio</title>
http://127.0.0.1:15173/?session=<nonce>   -> code=200
http://127.0.0.1:15173/health             -> code=200   {"ok":true,"command":"serve.health","data":{},"diagnostics":[]}
http://127.0.0.1:15173/v1/executions      -> 401 (no token: the proxy reaches the Runtime, which refuses)
http://127.0.0.1:18791/health             -> code=000 exit=52
```

`18791` stays unreachable by design: `serve` binds loopback and the page says nothing GraphHelm
serves is bound beyond it. The Studio reaches it through its own proxy inside the container.

## D. The browser half, from the Windows host (§5, the Studio steps)

Driven by the orchestrator against `http://127.0.0.1:15173/?session=<nonce>` at 1280×720, commit
`34208846`. No screenshot files were kept; what follows is what was observed, in the page's order.

1. **Connect.** The Studio connected (LIVE). Run `demo` · RUNNING · NEEDS YOU; `deploy` ready /
   approved, `implementation` blocked / retryable failure; polling healthy, every `/v1/...` 200.
2. **Pause.** `pause · finish in-flight` → toast `Paused — done (log 13 → 15)`, header
   `PAUSED · NEEDS YOU`, `deploy` paused by `studio-operator`.
3. **Approve.** `approve implementation` → header `PAUSED · CAN SLEEP`, `implementation` ready ·
   approved · studio-operator. The toast read `Approved implementation — done (log 16 → 16)` while
   the node's event count went from 10 to 11 (B2).
4. **Resume.** `resume` switched to the Free canvas and highlighted the empty `Graph file on the
   Runtime host…` box. Typing `/home/rehearsal/GraphHelm/examples/graphs/manual-override-deploy.yaml`
   and pressing Enter did nothing; only the `connect` button connects (B1). After `connect`:
   `2 nodes · 1 connection drawn · Connections verified: this file hashes to exactly the graph
   this run recorded.` Then Run actions → `resume` → header `RUNNING · NEEDS YOU`,
   `implementation · waiting input`, dock `Nothing is blocked. A waiting node wants an answer in
   the thread, not an approval.` The page at this commit promises `blocked_node` with the failure
   fixture (B3).

## E. After the browser half: status, `events verify`, double replay

Neither command is on GETTING_STARTED at this commit (F8); the forms are those of
[`useful-change-2026-09-13.md`](useful-change-2026-09-13.md) §9.

```text
$ graphhelm execution status --events /home/rehearsal/project/.graphhelm/events --execution demo   (fields extracted)
"attention":"needs_you"
"attentionReasons":[{"kind":"waiting_input_node","node":"implementation"}]
"headSequence":20
"nodeStates":{"deploy":"paused","implementation":"waiting_input"}
"status":"running"
[exit 0]

$ graphhelm events verify --repository /home/rehearsal/project/.graphhelm/events
{"ok":true,"command":"events.verify","data":{"formatSupported":true,"verified":false},"diagnostics":[]}
[exit 0]

$ graphhelm graph replay --events /home/rehearsal/project/.graphhelm/events > /tmp/replay-1.json   (twice, to -1 and -2)
[exit 0]
[exit 0]
$ sha256sum /tmp/replay-1.json /tmp/replay-2.json; wc -c; cmp
68cec042c158ea70341db93b8618baa020e8f35a9116e067a3540018f183f32d  /tmp/replay-1.json
68cec042c158ea70341db93b8618baa020e8f35a9116e067a3540018f183f32d  /tmp/replay-2.json
939
byte-identical
$ head -c 400 /tmp/replay-1.json
{"ok":true,"command":"graph.replay","data":{"acceptedMutations":0,"appliedDrafts":[],"currentGraph":null,"customsScans":{"implementation":[{"atSequence":20,"stage":"parked"}]},"declaredForm":{"executionId":"demo","executor":"fixture","name":…,"nodeIds":["deploy","implementation"],…

$ secret sweep (raw token and key, grep -c)
/phase-a.log token=0 key=0
/phase-b.log token=0 key=0
/phase-c.log token=0 key=0
/home/rehearsal/serve.log token=0 key=0
/home/rehearsal/studio.log token=0 key=0
/home/rehearsal/studio-127.log token=0 key=0
/tmp/replay-1.json token=0 key=0
```

`verified:false` is the documented answer for a local repository (`--repository` recognizes the
format; chain verification is the PostgreSQL adapter's, under `--config`). The head moved from 13
to 20 across the browser actions, and the replay of those 20 events is the same bytes twice.
The sweep's `/phase-c2.log` entry (the `--host` restart) had no file on disk, so it was not swept;
its captured output above holds only the nonce, redacted.

## F. Cleanup

```text
podman rm -f gh-clean-1094                 -> gh-clean-1094
podman ps -a --filter ancestor=ubuntu:24.04 -> (none)
podman rmi docker.io/library/ubuntu:24.04  -> Untagged … / Deleted: b2b7ea366714…   (pulled fresh for this run)
podman machine stop                        -> Machine "podman-machine-default" stopped successfully   (started for this run)
```

Other containers in that Podman machine (`graphhelm-task10-pg`, `graphhelm-task10-pg17`,
`buildx_buildkit_default`) were not touched. The saved Podman connection pointed at a stale port
(`55351`, refused, while sshd listened on `53052`); it was not edited, and every podman command
ran through `wsl -d podman-machine-default -u user -- podman …`.

## Findings

| # | Severity | Finding | Evidence | Status |
|---|---|---|---|---|
| F0 | MAJOR (at `34208846`) | Prerequisites ran `curl … sh.rustup.rs` before the bare-Ubuntu apt line that installs `curl`; on a fresh image the rustup step fails with `curl: command not found` | `curl: not found` at start (§A); this run installed apt first, so the failing order was not executed | **fixed in this PR**: the apt block now precedes the rustup block. **Declared gap (2026-09-14):** the reordered page was not re-executed; the container had already been removed when the order bug was found in review |
| F1 | MAJOR | §1's `git clone` fails for anyone without access: the repository is private | `fatal: could not read Username for 'https://github.com'` [exit 128] | **resolved**: owner decision 2026-09-14 (#1094, #302) — the repository stays private and promise 1 is measured with access; GETTING_STARTED §1 now states the invitation and `gh auth login` + `gh repo clone`, or a token over HTTPS (this PR) |
| F2 | MAJOR | §6 told a chat harness to `use graphhelm_list_executions`; `graphhelm mcp` has no such tool — the `graphhelm_*` names are the Studio's WebMCP page tools | `-32602 no tool named "graphhelm_list_executions"`; `tools/list` above | **fixed in this PR**: §6 names `list`, `status`, `approve`, … and says which surface the `graphhelm_*` names belong to |
| F3 | MINOR | The page required Node 22+ and gave no command to install it | `node: command not found` [exit 127] | **fixed in this PR**: the checksum-verified tarball as run here (Linux), a `winget` line (Windows, not exercised) |
| F4 | MINOR | In a container, the dev server's `127.0.0.1` bind is unreachable through a published port | host probes `exit=52`, then `200` with `-- --host` | **fixed in this PR**: a "container or VM" note in §4; the Runtime stays on loopback |
| F5 | MINOR | The apt line used `sudo` (absent in a root container) and had no `apt-get update` before it | `sudo: command not found` [exit 127] | **fixed in this PR**: `apt-get update` first, and the note to drop `sudo` as root. Install without the update was not measured |
| F6 | INFO | `init` detected no harness in a bare container (`"harnesses": []`); §6's documented `--harness claude-code` wrote `.mcp.json` | §B | by design, documented |
| F7 | INFO | Build took 323 s here against the 186 s the page quotes; the auto-connect line prints the container port (`4173`), not the host's published port | §A, §C | open, informational |
| F8 | MINOR | GETTING_STARTED has no `events verify` or replay step; the issue's shape asks for both, and they came from another record | §E | open (not changed here: #1091 edits §5) |
| B1 | MINOR | In the Studio, Enter in the `Graph file on the Runtime host…` box does nothing; only the `connect` button connects | §D step 4 | open |
| B2 | INFO | The approve toast read `log 16 → 16` while the node gained an event (10 → 11); the delta looks stale | §D step 3 | open |
| B3 | known | After pause → approve → resume, `implementation` reads `waiting_input` / `waiting_input_node`, not the `blocked_node` the page promises with the failure fixture | §D step 4, §E status | known; fixed by PR #1091 (GETTING_STARTED names both outcomes and the Studio passes a fixture file) |

## Promise 1 verdict

**Partial, `OBSERVER_MISSING` for the corrected page (Codex on PR #1096, 2026-09-15).** The
commands of the page ran top to bottom on a clean machine in the order this PR documents (apt
before rustup, F0), with a source archive standing in for the authenticated clone (F1). Two
things were not observed and this record does not claim them: the reordered page executed as
published, and a clone with repository access. Promise 1 of #302 therefore stays partial until
one run of the published page, clone included, is recorded on a clean machine. What was
observed: From an image holding only Ubuntu, the page's own commands (apt before rustup, and
the declared source archive standing in for an authorized clone) built and installed the binary, provisioned the project, started the Runtime, ran the first
fixture execution, installed the Studio, served it to a browser on the host through the session
URL, carried the operator through connect, pause, approve and resume, and left a store that
verifies its format and replays byte-identically.

Open after this PR: no MAJOR. F8 and B1 (MINOR), F7 and B2 (INFO), and B3 until #1091 lands.
Whether roadmap §3.2.1 row 1 moves to **held** also depends on #1083, which this record does not
decide.
