# Getting started — from clone to an execution you can act on

This is the one continuous path. Follow it top to bottom and you end with a Runtime on loopback,
a project provisioned by `graphhelm init`, a first execution running against it, the Studio
showing that run, and — if you want one — a chat harness that can operate it through MCP.

Every command here was executed as written. Windows commands are PowerShell; Linux/macOS commands
are bash. The clean-host transcript this page was checked against is
[`docs/acceptance/install-rehearsal-2026-09-13.md`](../acceptance/install-rehearsal-2026-09-13.md).

**What needs credentials and what does not.** Nothing on this page needs an account, an API key,
a model provider, a database, or the network after the clone (the Studio's `npm ci` fetches
packages once). Every execution below runs on fixtures. A run that calls a real model needs a
gateway manifest and credentials and is not covered here — see
[`docs/product/PROVIDER_LESS_MODE.md`](../product/PROVIDER_LESS_MODE.md) for what works without a
provider and what changes when you add one.

**One port.** `graphhelm init` defaults to `127.0.0.1:8791`, and every command on this page uses
that number. `install/install.sh` (the VPS systemd path) binds `127.0.0.1:8080` instead; if you
follow that path, substitute the port. Nothing GraphHelm serves is ever bound beyond loopback.

## Prerequisites

| | Needed for | Check |
|---|---|---|
| Rust `1.97.1` (pinned) with `rustfmt` and `clippy` | building the binary | `cargo +1.97.1 --version` |
| git | the clone, and `init`'s `.gitignore` step | `git --version` |
| Node 22+ and npm | **only** the Studio (sections 4 and 5) | `node --version` |
| PowerShell (Windows) or bash (Linux/macOS) | running the commands | — |

Install the toolchain when missing:

```powershell
# Windows
winget install --id Rustlang.Rustup --exact --source winget --accept-source-agreements --accept-package-agreements
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
rustup toolchain install 1.97.1 --profile minimal --component rustfmt clippy
```

```bash
# Linux / macOS
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.97.1
. "$HOME/.cargo/env"
```

On a bare Ubuntu the build also needs `build-essential ca-certificates curl git pkg-config`
(`sudo apt-get install --yes --no-install-recommends build-essential ca-certificates curl git pkg-config`).

## 1. Build or install the binary

```powershell
git clone https://github.com/stabem/GraphHelm.git
cd GraphHelm
cargo +1.97.1 install --locked --path apps/cli
graphhelm --version
```

```bash
git clone https://github.com/stabem/GraphHelm.git
cd GraphHelm
cargo +1.97.1 install --locked --path apps/cli
graphhelm --version
```

Expected last line:

```
graphhelm 0.1.0
```

`cargo install` puts `graphhelm` on your PATH (`~/.cargo/bin`). If you would rather not install,
`cargo +1.97.1 build --locked -p graphhelm-cli` leaves the binary at `target/debug/graphhelm`
(`.exe` on Windows); use that path wherever this page says `graphhelm`, and pass it to
`studio-up.ps1` as `-GraphHelm`. The harness registration `init` writes (section 6) names the
binary that ran `init` by absolute path, so it works either way; re-run `init` after installing
or moving the binary and the registration follows. On the rehearsal host (a clean Ubuntu 24.04 container, `nproc` = 8)
this step took 186 s (`Finished \`release\` profile [optimized] target(s) in 3m 06s`, in the
recorded transcript); a laptop is similar.

## 2. `graphhelm init` in your project

Run it in the directory you want GraphHelm to work on — your own repository, or an empty folder.
The GraphHelm clone is not the project; it is where the binary and the Studio live.

```powershell
cd C:\path\to\your-project
graphhelm init --pretty
```

```bash
cd /path/to/your-project
graphhelm init --pretty
```

Expected: a JSON envelope with `"ok": true` and `"command": "init"`. `data` names every path it
provisioned and whether it was `created` or found `existing`:

```
<project>/.graphhelm/events/         the store serve and execution start share
<project>/.graphhelm/events.token    the bearer token (64 hex chars, owner-only)
<project>/.graphhelm/serve.key       the sealing key (64 hex chars, owner-only)
<project>/.graphhelm/keyring/        the keyring holding key id "studio"
<project>/.gitignore                 gains ".graphhelm/" when the project is a git work tree
<project>/.mcp.json                  Claude Code registration (when Claude Code is detected)
<project>/.graphhelm/codex.config.toml   Codex snippet (when ~/.codex exists)
```

and `data.next` lists the exact commands for sections 3 to 5 with your paths filled in, in
PowerShell and bash. **The token's and the key's values are never printed** — the next commands
read the key from its file. Paths in `data` are relative to the project (`.graphhelm/events`),
so the envelope never carries your home directory; the `next` command strings are the one place
absolute paths appear, because you copy them into a shell from wherever you are.

If the project is a subdirectory of a larger repository, `init` still recognizes the enclosing
work tree and writes the ignore line into the subdirectory's own `.gitignore`; the repository
root's file is never edited. A `.gitignore` whose last matching line un-ignores the directory
(`!.graphhelm/`) gets the block appended after it, so git's last-match rule lands on the ignore.

Running `init` again is safe: every artifact reports `existing`, the token and key bytes are
unchanged, `.gitignore` gains nothing, and an existing `.mcp.json` keeps every other server in it.
Options: `--bind` (default `127.0.0.1:8791`), `--key-id` (default `studio`), `--harness
claude-code|codex` (repeatable; overrides detection), `--project <dir>` (default: the current
directory).

## 3. Start the Runtime

Copy the two commands `init` printed under `next`, or type them. The sealing key must be in
`GRAPHHELM_EVENTS_KEY` when `serve` starts; it is never passed as a flag.

```powershell
$env:GRAPHHELM_EVENTS_KEY = (Get-Content -Raw "C:\path\to\your-project\.graphhelm\serve.key").Trim()
graphhelm serve --events "C:\path\to\your-project\.graphhelm\events" --bind 127.0.0.1:8791 --keyring "C:\path\to\your-project\.graphhelm\keyring" --key-id studio
```

```bash
export GRAPHHELM_EVENTS_KEY="$(cat /path/to/your-project/.graphhelm/serve.key)"
graphhelm serve --events /path/to/your-project/.graphhelm/events --bind 127.0.0.1:8791 --keyring /path/to/your-project/.graphhelm/keyring --key-id studio
```

Expected first line on stdout (the process then stays in the foreground; leave this terminal
open, or start it with `Start-Process`/`nohup` as the rehearsal did):

```
{"ok":true,"command":"serve.started","data":{"address":"127.0.0.1:8791", ...}}
```

Prove it from a second terminal. `/health` answers without a token; everything else refuses a
bare request with `401` and lets the token through to the router:

```powershell
Invoke-RestMethod http://127.0.0.1:8791/health | ConvertTo-Json -Compress
try { Invoke-WebRequest http://127.0.0.1:8791/v1/executions -UseBasicParsing } catch { $_.Exception.Response.StatusCode.value__ }   # 401
$token = (Get-Content -Raw 'C:\path\to\your-project\.graphhelm\events.token').Trim()
Invoke-RestMethod "http://127.0.0.1:8791/v1/executions?limit=5" -Headers @{ Authorization = "Bearer $token" } | ConvertTo-Json -Compress
```

```bash
curl --silent --fail http://127.0.0.1:8791/health; echo
curl --silent --output /dev/null --write-out '%{http_code}\n' http://127.0.0.1:8791/v1/executions            # 401
curl --silent --header "Authorization: Bearer $(cat /path/to/your-project/.graphhelm/events.token)" 'http://127.0.0.1:8791/v1/executions?limit=5'; echo
```

Expected: `{"ok":true,"command":"serve.health",...}`, then `401`, then
`{"ok":true,"command":"execution.list","data":{"executions":[],...}}` — an empty store, so far.

Why the keyring flags: without `--keyring`/`--key-id` the Runtime cannot seal a message envelope,
and the Studio's message box and `graphhelm_send_message` are refused on every send. `init`
created the keyring so that this pair is always available; `serve` still runs without it if you
only need reads and the pause/approve/resume verbs. With the flags given, `serve` opens the
keyring once at start; when `GRAPHHELM_EVENTS_KEY` is unset, malformed, or does not open the
keyring under that `--key-id`, it still starts (a fixture-only run never seals) but the
`serve.started` line carries a `warning` diagnostic (`GHCLI006_SERVE_INVALID` at `/keyring`)
naming the consequence: messages and real executors will be refused until you restart it with
the right key. You learn at start, not at the first message.

## 4. Start the Studio

Node 22+ and npm are needed from here on, and only from here on. Run these from the **GraphHelm
clone** (the Studio lives in `apps/studio`), not from your project.

Windows, one command — reuses the Runtime from section 3 if it is already answering, otherwise
starts one; installs dependencies on the first run; opens the browser connected:

```powershell
cd C:\path\to\GraphHelm
powershell -File apps/studio/tools/studio-up.ps1 -Events "C:\path\to\your-project\.graphhelm\events" -Bind 127.0.0.1:8791 -Keyring "C:\path\to\your-project\.graphhelm\keyring" -KeyId studio -GraphHelm graphhelm
```

Expected lines: `[up] Runtime already answering at http://127.0.0.1:8791 and this project's token
opens it - reusing it` (or `[up] starting: graphhelm serve ...` then `[up] Runtime up`), and
`[up] Studio starting at http://127.0.0.1:5183 - the page opens already connected`.

By hand, on any platform:

```powershell
$env:GRAPHHELM_EVENTS = "C:\path\to\your-project\.graphhelm\events"
$env:GRAPHHELM_RUNTIME_URL = "http://127.0.0.1:8791"
npm --prefix apps/studio ci
npm --prefix apps/studio run dev
```

```bash
export GRAPHHELM_EVENTS=/path/to/your-project/.graphhelm/events
export GRAPHHELM_RUNTIME_URL=http://127.0.0.1:8791
npm --prefix apps/studio ci
npm --prefix apps/studio run dev
```

Expected, among the dev server's output:

```
  Studio auto-connect: http://127.0.0.1:4173/?session=<nonce>
```

**Open that exact URL.** The page auto-connects only through the printed `?session=` link: the
dev server reads the token beside `GRAPHHELM_EVENTS` and hands it to the page under that one-time
session, so nobody pastes a token. Opening `http://127.0.0.1:4173/` without the query shows the
connect gate instead, where you can paste the contents of `events.token` by hand. With
`GRAPHHELM_EVENTS` unset there is no session endpoint at all, only the gate.

The Studio's `/v1` and `/health` requests are proxied by the dev server to `GRAPHHELM_RUNTIME_URL`,
so they stay same-origin; the default is `http://127.0.0.1:8080`, which is why the variable must
say `8791` here.

## 5. The first execution — and acting on it

Start a run against the store `init` created, offline. The fixture file decides what each node
"does" so no model is called; `supervised` parks the run at the first thing that needs a human.
Run from the GraphHelm clone (the graph file is an example in it):

```powershell
cd C:\path\to\GraphHelm
Set-Content -Path "C:\path\to\your-project\.graphhelm\fixtures.json" -Value '{"nodeOutcomes":{"implementation":"failure"}}'
graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events "C:\path\to\your-project\.graphhelm\events" --fixtures "C:\path\to\your-project\.graphhelm\fixtures.json" --mode supervised --execution demo
```

```bash
cd /path/to/GraphHelm
echo '{"nodeOutcomes":{"implementation":"failure"}}' > /path/to/your-project/.graphhelm/fixtures.json
graphhelm execution start --file examples/graphs/manual-override-deploy.yaml --events /path/to/your-project/.graphhelm/events --fixtures /path/to/your-project/.graphhelm/fixtures.json --mode supervised --execution demo
```

Expected (trimmed): `"command":"execution.start"`, `"attention":"needs_you"`, and
`"attentionReasons":[{"kind":"blocked_node","node":"implementation"}]`. The run is parked on the
`implementation` node. [`QUICKSTART.md`](../../QUICKSTART.md) explains the four `attention`
values; this page moves on to acting on the run.

Now in the Studio — the operator journey from
[`docs/ux/STUDIO_MVP.md`](../ux/STUDIO_MVP.md) §1, in order:

1. The page is connected (section 4) — the rail shows the Runtime address and the actor.
2. The execution list shows `demo` with its attention verdict, `needs_you`.
3. Click it: the blocking node `implementation` is named in the header.
4. Read the aggregate state, the sixteen lifecycle counts, and the event log — every event is
   there, with its sequence and who it is attributed to.
5. Type the graph path into the `Graph file on the Runtime host…` box (the absolute path of
   `examples/graphs/manual-override-deploy.yaml` in your clone — it is resolved on the Runtime
   host) and press **connect**: the run's shape is drawn, verified against the hash the run
   recorded, or refused with the reason.
6. Click a node to narrow the thread to it.
7. Press **pause**, then **approve implementation**, then **resume** (the dock asks for the graph
   file path from step 5 if it is empty) — in that order. A supervised run accepts `resume` only
   from `paused`; approving first and resuming would be refused with `resume refused:
   not_paused` and leave the run reading `wedged_quiescence` (measured, see the CLI block below).
8. Each verb reports what changed — `Paused`, `Approved`, `Resumed`: head before, head after, the
   re-read status and the events appended, attributed to you. While paused, the verdict reads
   `can_sleep` (nothing is waiting: the node is `ready`, the run is held). On resume the node
   **runs again**, and what happens next is decided by the fixture, because the fixture stands in
   for the model: with `implementation: failure` it fails again and the run is back at
   `needs_you` / `blocked_node: implementation` — the loop a real failing node would produce; with
   `implementation: success` in the fixture file the node succeeds and the run parks at the
   next human step, `needs_you` / `waiting_input_node: deploy`. Either way, `needs_you` is the
   honest answer: this graph is built to need a person.
9. Reload the page: the Runtime is untouched and the token is forgotten (it lived in the session
   only).

The same verbs from the terminal, executed as written against a fresh store (`E` is the events
directory, `F` the fixture file; same flags in PowerShell with the Windows paths):

```bash
graphhelm execution pause   --events $E --execution demo
graphhelm execution approve --events $E --execution demo --node implementation
graphhelm execution resume  --events $E --execution demo --file examples/graphs/manual-override-deploy.yaml --fixtures $F
graphhelm execution status  --events $E --execution demo
```

Measured output, trimmed to the fields that matter:

```
execution.pause    ok  status=paused   attention=needs_you  [{"kind":"blocked_node","node":"implementation"}]  deploy=paused implementation=blocked
execution.approve  ok  status=paused   attention=can_sleep  []                                                  deploy=paused implementation=ready
execution.resume   ok  status=running  attention=needs_you  [{"kind":"blocked_node","node":"implementation"}]  deploy=paused implementation=blocked
execution.status   ok  status=running  attention=needs_you  [{"kind":"blocked_node","node":"implementation"}]
```

With `{"nodeOutcomes":{"implementation":"success"}}` written to `$F` before the `resume`:

```
execution.resume   ok  status=running  attention=needs_you  [{"kind":"waiting_input_node","node":"deploy"}]  deploy=waiting_input implementation=succeeded
```

And the order this page used to give — approve, then resume, without a pause — measured:

```
execution.approve  ok     status=running  deploy=ready implementation=ready
execution.resume   FAILED GHCLI005_EXECUTION_STATE: resume refused: not_paused
execution.status   ok     status=running  attention=needs_you  [{"kind":"wedged_quiescence"}]
```

The Studio and the CLI write to the same append-only store; the Runtime serves it; nothing is
duplicated.

## 6. Connect a chat harness

`init` wrote the registration; the harness only has to read it. The token travels as a **file
path** (`--token-file`), never inline and never through argv, and every mutation the chat makes is
attributed to the actor `agent-chat`.

**Claude Code** — `<project>/.mcp.json` was written (or merged, keeping any other server in it).
Start Claude Code in the project directory; it reads `.mcp.json` from the project root. If a
session was already open, restart it. Then, in the chat:

```
use graphhelm_list_executions
```

Expected: one row, `demo`, with its attention verdict — the same answer the Studio shows.

**Codex** — `init` wrote `<project>/.graphhelm/codex.config.toml` and does not touch your home
directory. Append its contents to `~/.codex/config.toml` yourself:

```bash
cat /path/to/your-project/.graphhelm/codex.config.toml >> ~/.codex/config.toml
```

```powershell
Get-Content "C:\path\to\your-project\.graphhelm\codex.config.toml" | Add-Content "$env:USERPROFILE\.codex\config.toml"
```

The tools are the ones in [`apps/studio/README.md`](../../apps/studio/README.md)'s table
(`graphhelm_list_executions`, `graphhelm_get_attention`, `graphhelm_approve_node`,
`graphhelm_resume_execution`, `graphhelm_send_message`, …). If `init` detected neither harness
(`"harnesses": []`), pass `--harness claude-code` or `--harness codex` explicitly.

## 7. Where to go next

- [`QUICKSTART.md`](../../QUICKSTART.md) — what a node does when it runs, the four `attention`
  values, and scripting the answer. Everything there is offline, like this page.
- [`install/VPS_REHEARSAL.md`](../../install/VPS_REHEARSAL.md) — the Runtime on a server: the
  Docker path (`docker compose`) and the native systemd path (`install/install.sh`, port `8080`).
  The clean-host run of its Runtime half is recorded in
  [`docs/acceptance/install-rehearsal-2026-09-13.md`](../acceptance/install-rehearsal-2026-09-13.md),
  including the one thing a plain container cannot prove (systemd).
- [`docs/product/PROVIDER_LESS_MODE.md`](../product/PROVIDER_LESS_MODE.md) — what GraphHelm does
  with no model provider at all, and what a provider adds.
- [`apps/studio/README.md`](../../apps/studio/README.md) — the Studio in detail: the WebMCP site
  tools, the production build, and the troubleshooting table.
- [`docs/ux/CHAT_SURFACE_SPEC.md`](../ux/CHAT_SURFACE_SPEC.md) and
  [`examples/chat-surface/`](../../examples/chat-surface/) — the operator skills a chat harness
  can carry alongside the MCP registration.

## If something refuses

| Symptom | Cause | Fix |
|---|---|---|
| `init` says `GHCLI027_INIT_REFUSED` at `/mcp_json` | `.mcp.json` exists and is not a JSON object | fix or move the file aside and re-run; everything else was provisioned |
| `init` says `GHCLI027_INIT_REFUSED` at `/keyring` | the keyring exists but `serve.key` no longer opens it (the key file was replaced) | pass the `--key-id` it was created with, or move `.graphhelm/keyring` aside — evidence sealed under the lost key cannot be recovered |
| the `serve.started` line carries a `warning` `GHCLI006_SERVE_INVALID` at `/keyring` (`the keyring could not be opened with GRAPHHELM_EVENTS_KEY (…); sealed operations (messages, real executors) will refuse until serve is restarted with the right key`) | the variable is unset in this terminal, or `--key-id` is not the id the keyring was created with; reads and pause/approve/resume still work | stop `serve`, run the first `next` command from `init` again in the same terminal, restart `serve` with the `--key-id` `init` printed |
| `serve` refuses at start with `GHCLI006_SERVE_INVALID` at `/bind` (`the requested address could not be bound`) | something already listens on `127.0.0.1:8791` — often a previous `serve` | stop it, or re-run `init --bind 127.0.0.1:<other port>` so every registration follows the new number |
| the Studio shows the connect gate | the page was opened without the printed `?session=` URL, or `GRAPHHELM_EVENTS` was unset | open the printed URL; or paste the contents of `events.token` |
| the Studio says `Runtime replied 502` | `GRAPHHELM_RUNTIME_URL` points at the wrong port (the default is `8080`) | set it to `http://127.0.0.1:8791` and restart the dev server |
| the message box is refused | the Runtime started without `--keyring`/`--key-id` | restart it with the command `init` printed |
