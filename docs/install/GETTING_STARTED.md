# Getting started — from clone to an execution you can act on

This is the one continuous path. Follow it top to bottom and you end with a Runtime on loopback,
a project provisioned by `graphhelm init`, a first execution running against it, the Studio
showing that run, and — if you want one — a chat harness that can operate it through MCP.

Every command here was executed as written. Windows commands are PowerShell; Linux/macOS commands
are bash. The clean-host transcripts this page was checked against are
[`docs/acceptance/install-rehearsal-2026-09-13.md`](../acceptance/install-rehearsal-2026-09-13.md)
and, Studio included, [`docs/acceptance/clean-machine-2026-09-14.md`](../acceptance/clean-machine-2026-09-14.md).

**What needs credentials and what does not.** A public clone needs no GitHub account; while this
repository remains private, cloning requires repository access. Nothing after the clone needs an
account, an API key, a model provider, a database, or the network (the Studio's `npm ci` fetches
packages once). Every execution below runs on fixtures. A run that calls a real model needs a
gateway manifest and a credential; the one command that wires both is at the end of section 2,
and [`docs/product/PROVIDER_LESS_MODE.md`](../product/PROVIDER_LESS_MODE.md) says what works
without a provider and what changes when you add one.

**One port.** `graphhelm init` defaults to `127.0.0.1:8791`, and every command on this page uses
that number. `install/install.sh` (the VPS systemd path) binds `127.0.0.1:8080` instead; if you
follow that path, substitute the port. Nothing GraphHelm serves is ever bound beyond loopback.

**Existing host configuration.** `graphhelm setup --project <project> --home <disposable-profile>`
previews supported files without changing them. An exact reviewed plan and `--accept` digest are
required to apply; backup precedes mutation, and `graphhelm restore` previews a reversible return.
Terminal output renders the same fields that pipes receive as JSON. Installation remains
`installed_unverified`: no trusted host observer ships yet, and `--verify` cannot turn a
user-authored ActivationReceipt into proof. Follow the separate
[disposable adoption rehearsal](../acceptance/adoption-rehearsal.md) for supported scope, exact
commands, retained recovery guards, and the missing-observer limit. Those rehearsal commands are
a recipe, not part of the historical executed transcripts cited above.

## Prerequisites

| | Needed for | Check |
|---|---|---|
| Rust `1.97.1` (pinned) with `rustfmt` and `clippy` | building the binary | `cargo +1.97.1 --version` |
| git | the clone, and `init`'s `.gitignore` step | `git --version` |
| Node 22+ and npm | **only** the Studio (sections 4 and 5) | `node --version` |
| PowerShell (Windows) or bash (Linux/macOS) | running the commands | — |

On a bare Ubuntu, install the system packages **first**: the rustup line below needs `curl`, and
the build needs `build-essential ca-certificates git pkg-config`. A fresh image has no package
index, so fetch it before installing (a root shell in a container has no `sudo`; drop the word
there):

```bash
sudo apt-get update
sudo apt-get install --yes --no-install-recommends build-essential ca-certificates curl git pkg-config
```

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

Node 22+ for the Studio, when missing. Linux (x64), the official tarball, checksum-verified, as
the clean-machine run installed it:

```bash
cd /tmp
curl -sSfO https://nodejs.org/dist/latest-v22.x/SHASUMS256.txt
F=$(grep -oE 'node-v22\.[0-9.]+-linux-x64\.tar\.gz' SHASUMS256.txt | head -1)
curl -sSfO "https://nodejs.org/dist/latest-v22.x/$F"
grep " $F\$" SHASUMS256.txt | sha256sum -c - && mkdir -p ~/.local/node && tar -xzf "$F" -C ~/.local/node --strip-components=1   # extracts only after the digest matched
export PATH="$HOME/.local/node/bin:$PATH"
echo 'export PATH="$HOME/.local/node/bin:$PATH"' >> ~/.profile   # section 4 runs in a second terminal
```

Windows (not exercised by the clean-machine run; any Node 22 or later works):

```powershell
winget install --id OpenJS.NodeJS.LTS --exact --source winget --accept-source-agreements --accept-package-agreements
```

## 1. Build or install the binary

Clone over HTTPS using the command below. Once the repository is public, an HTTPS read-only clone
needs no GitHub account. SSH cloning requires a GitHub account with an authenticated key. While
the repository remains private, either method requires an account with repository access.
Contributors need a GitHub account to open issues and pull requests.

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

### Optional: wire a model provider in one command

Nothing below needs this. When you want a run that calls a real model, `gateway setup` does in
one command what used to be four: it adds the provider's route to
`<project>/.graphhelm/manifest.json`, asks for the API key once, stores it in the Credential
Broker (`<project>/.graphhelm/broker`) under the keyring and `serve.key` that `init` made, probes
the route, and prints the next commands. You fill in only the key.

```powershell
graphhelm gateway setup --provider typesafe --pretty
```

```bash
graphhelm gateway setup --provider typesafe --pretty
```

On a terminal it prompts `Paste the typesafe API key (input hidden):` on stderr with echo off.
In a script, pipe the key instead — it is never an argument:

```bash
printf '%s\n' "$TYPESAFE_API_KEY" | graphhelm gateway setup --provider typesafe
```

Expected: `"ok": true`, `"command": "gateway.setup"`, `data.route` naming `judge` /
`secret_typesafe`, `data.probe.health` `available` (the probe proves the credential leases; it
places no model call), and `data.next` with the exact `graph synthesize --judge-route judge`
command. **The key is never printed** and is readable in no file: the broker's store is sealed.
Providers: `typesafe` (defaults `https://api.typesafe.ai`, model `jev-latest`, route `judge`),
`anthropic` and `openai` (route `anthropic` / `openai`; `--model` is required, setup does not
guess one). A second run with the same route id is refused and changes nothing; `--replace`
swaps the route. Options: `--route-id`, `--model`, `--base-url`, `--key-id` (as given to `init`),
`--project <dir>`.

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

**In a container or VM**, the dev server's `127.0.0.1` bind is unreachable through a published
port (measured: the connection is reset). Start it with `npm --prefix apps/studio run dev -- --host`
inside the container instead, publish port `4173`, and open the printed URL with the host's
published port in place of `4173`. Keep `serve` on `127.0.0.1`: the dev server proxies `/v1` and
`/health` to it from inside the container, so the Runtime never needs a wider bind.

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

1. The page is connected (section 4): the rail's header reads `GraphHelm` with a green `LIVE`
   mark (`STALE` when background reads keep failing, `OFFLINE` when disconnected). The rail does
   not print the Runtime address — the dev server proxies to `GRAPHHELM_RUNTIME_URL` — or an actor;
   every verb you press is attributed to the actor `studio-operator`, and the event lines and cards
   say so after you act.
2. Under the project folder (`this runtime`) the rail lists `demo`. A run whose briefing carries
   an objective is named by it, with the execution id (`demo`) on the line beneath; the next line
   is the time of its last event and `· demonstration` (a fixture run). The attention verdict is
   the coloured square at the row's right edge; hover it (or use a screen reader) for the words,
   `needs you`.
3. Click it. The top strip names the run (the objective; the id on hover) and a pill with the
   status and the verdict, `RUNNING · NEEDS YOU`. The blocking node is named in the conversation
   column's first block, `implementation is blocked`, beside an **approve implementation** button,
   and in the `Work overview` its card reads `blocked` / `retryable failure`.
4. Under that block the run panel reads `This run needs you`, the demonstration sentence and the
   `Objective`, then the lifecycle counts folded into chips — `ready 1`, `blocked 1`,
   `nothing in the other 14 states` (the sixteen states, the zeros stated together) — and the
   run's thread, where each event line carries who it is attributed to and when. The raw event
   log with sequences is `graphhelm execution status`/`GET /v1/executions/demo/events`; the page
   groups lifecycle events into strips you can open.
5. Press **Verify connections** (or switch the view to **Free canvas**), type the graph path into
   the `Graph file on the Runtime host…` box (the absolute path of
   `examples/graphs/manual-override-deploy.yaml` in your clone — it is resolved on the Runtime
   host) and press **connect**: the run's shape is drawn, verified against the hash the run
   recorded, or refused with the reason. On a demonstration run a second box beside it,
   `Fixture file for resume (optional)…`, takes a fixture path on the Runtime host for step 7.
6. Click a node to narrow the thread to it.
7. Press **pause · finish in-flight**, then **approve implementation**, then **resume** (the dock
   walks you to the graph file box from step 5 if it is empty) — in that order. A supervised run
   accepts `resume` only from `paused`; approving first and resuming would be refused with
   `resume refused: not_paused` and leave the run reading `wedged_quiescence` (measured, see the
   CLI block below).
8. Each verb reports what changed — `Paused`, `Approved`, `Resumed`: head before, head after, the
   re-read status and the events appended, attributed to you. While paused, the verdict reads
   `can_sleep` (nothing is waiting: the node is `ready`, the run is held). On resume the node
   **runs again**, and on a fixture run the fixture stands in for the model, so what happens next
   depends on whether the resume names one:
   - **With a fixture** — the CLI's `--fixtures`, or a path in the Studio's
     `Fixture file for resume (optional)…` box — the fixture decides: with
     `implementation: failure` the node fails again and the run is back at `needs_you` /
     `blocked_node: implementation`, the loop a real failing node would produce; with
     `implementation: success` the node succeeds and the run parks at the next human step,
     `needs_you` / `waiting_input_node: deploy`.
   - **Without a fixture** — the Studio's resume with that box left empty — the node has no
     outcome to take and parks at `needs_you` / `waiting_input_node: implementation`; the dock
     says `Nothing is blocked. A waiting node wants an answer in the thread, not an approval.`
     Resume again with a fixture file named to drive it on.

   Either way, `needs_you` is the honest answer: this graph is built to need a person.
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
use the graphhelm MCP tool list
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

The server `graphhelm mcp` exposes a closed list of 28 tools named after the Runtime verbs —
`list`, `status`, `briefing`, `events`, `evidence`, `approve`, `pause`, `resume`, `signal`,
`cancel`, `sweep`, … (`apps/cli/src/commands/mcp/tools.rs`); a harness shows them under the
server name, e.g. `mcp__graphhelm__list` in Claude Code. The `graphhelm_*` names in
[`apps/studio/README.md`](../../apps/studio/README.md)'s table are the Studio's WebMCP page tools,
a different surface: `graphhelm mcp` refuses them (`-32602 no tool named
"graphhelm_list_executions" is part of this server`, measured). If `init` detected neither harness
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
- [`docs/operations/TOOLS_ONLY_RUNTIME.md`](../operations/TOOLS_ONLY_RUNTIME.md) — the next step
  after this page: a Runtime that runs real tool nodes (patch, tests, commit) with no model
  credential, landing the change as a ref in your project.
- `graphhelm execution briefing --events <events> --execution <id>` — when you come back later, or
  from another harness: the objective, the decisions, the work done, what is pending and the next
  step, read from the store alone.
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
