# Getting started — from clone to an execution you can act on

This is the one continuous path. Follow it top to bottom and you end with a Runtime on loopback,
a project provisioned by `graphhelm init`, a first execution running against it, the Studio
showing that run, and — if you want one — a chat harness that can operate it through MCP.

Windows commands are PowerShell; Linux/macOS commands are bash. The Linux commands were executed as
written on a clean Ubuntu 24.04 machine, Studio included: the last run is
[`docs/acceptance/clean-machine-2026-11-01.md`](../acceptance/clean-machine-2026-11-01.md). The Windows
commands are the same steps in PowerShell and were not part of that run. Earlier clean-host transcripts are
[`docs/acceptance/install-rehearsal-2026-09-13.md`](../acceptance/install-rehearsal-2026-09-13.md)
and [`docs/acceptance/clean-machine-2026-09-14.md`](../acceptance/clean-machine-2026-09-14.md).

**What needs credentials and what does not.** A public clone needs no GitHub account. Several steps use the network:
the package managers (apt, winget, rustup), the Node download (`curl` from nodejs.org), the clone, the build's Rust
crates, the Studio's npm packages, and `graphhelm update`. None of them needs an account, an API key, a model provider or a database. Every execution below runs on fixtures. A run that calls a real model needs a
gateway manifest and a credential; the one command that wires both is at the end of section 2,
and [`docs/product/PROVIDER_LESS_MODE.md`](../product/PROVIDER_LESS_MODE.md) says what works
without a provider and what changes when you add one.

**One port.** `graphhelm init` defaults to `127.0.0.1:8791`, and every command on this page uses
that number. `install/install.sh` (the VPS systemd path) binds `127.0.0.1:8080` instead; if you
follow that path, substitute the port. The Runtime is bound to loopback. The Studio's dev server is too, except in a container or VM, where section 4 tells you to start it with `-- --host`.

**Existing host configuration.** `graphhelm setup --project <project> --home <disposable-profile>`
previews supported files without changing them. An exact reviewed plan and `--accept` digest are
required to apply; backup precedes mutation, and `graphhelm restore` previews a reversible return.
Terminal output renders the same fields that pipes receive as JSON. Installation remains
`installed_unverified`: no trusted host observer ships yet, and `--verify` cannot turn a
user-authored ActivationReceipt into proof. Follow the separate
[disposable adoption rehearsal](../acceptance/adoption-rehearsal.md) for supported scope, exact
commands, retained recovery guards, and the missing-observer limit. Those rehearsal commands are
a recipe, not part of the historical executed transcripts cited above.

## Words used on this page

- **Runtime**: the local `graphhelm serve` process. It keeps the event store and answers the API.
- **Execution**: one run of a graph, started with `graphhelm execution start`.
- **Harness**: the agent host you chat with, such as Claude Code or Codex.
- **MCP** (Model Context Protocol): the protocol a harness uses to call tools. `graphhelm mcp` is GraphHelm's server for it.
- **Loopback**: `127.0.0.1`, this machine only. The Runtime binds it. The Studio's dev server binds it too, unless you start it with `-- --host` in a container or VM (section 4).
- **Bearer token**: the secret in `events.token`. It goes in an `Authorization: Bearer` header; never print it.
- **Sealing key**: the key in `serve.key`. Everything `serve` seals depends on it: if you lose `serve.key`, nothing sealed can be opened again, so keep a copy.
- **Keyring**: `keyring.v1.json` names the key id (`studio` by default; `--key-id` changes it) and carries an authentication tag. It holds no key; `serve` needs `serve.key` to open it.
- **Gateway** and **route**: a gateway connects to a model provider. A route is one named entry for it in `manifest.json`.
- **Manifest**: `<project>/.graphhelm/manifest.json`, where the routes are kept.
- **Journey**: a user-visible flow written as screens and checks, in `.graphhelm/journeys/`.
- **Observer**: the tool that runs a journey and records what it saw. Without one, a journey reads `OBSERVER_MISSING`.
- **Attention**: the field that says whether a run needs you. Its four values are explained in [QUICKSTART.md](../../QUICKSTART.md).

## Prerequisites

| | Needed for | Check |
|---|---|---|
| Rust `1.97.1` (pinned) with `rustfmt` and `clippy` | building the binary | `cargo +1.97.1 --version` |
| Visual Studio C++ build tools and a Windows SDK (Windows MSVC only) | linking the Windows binary | `where.exe link.exe` |
| git | the clone, and `init`'s `.gitignore` step | `git --version` |
| curl (Linux/macOS) | the health and `401` checks in section 3; a bare Ubuntu image does not have it (`sudo apt-get install --yes curl`) | `curl --version` |
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

For the default Windows MSVC toolchain, install Visual Studio or Build Tools with the **Desktop
development with C++** workload and a Windows SDK. Rust uses the MSVC linker and Windows libraries
to build the executable. See the [Rustup MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

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

When 1.0.0 is published, its [release page](https://github.com/stabem/GraphHelm/releases) will list
Windows and Ubuntu x86-64 CLI archives, avoiding the Rust build. Download the matching archive and
`SHA256SUMS.txt` from that release. If 1.0.0 is not listed yet, use the source-build steps below. The asset names are `graphhelm-windows-x86_64.zip` and
`graphhelm-ubuntu-24.04-x86_64.tar.gz`.

On Windows, compare the archive digest with its line in `SHA256SUMS.txt`, then extract and add the
binary directory to this shell's `PATH`:

```powershell
$expected = ((Get-Content .\SHA256SUMS.txt | Where-Object { $_ -match 'graphhelm-windows-x86_64\.zip$' }) -split '\s+')[0]
$actual = (Get-FileHash .\graphhelm-windows-x86_64.zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "SHA-256 mismatch" }
Expand-Archive .\graphhelm-windows-x86_64.zip -DestinationPath "$HOME\graphhelm-1.0.0"
$env:Path = "$HOME\graphhelm-1.0.0\graphhelm-windows-x86_64;$env:Path"
graphhelm --version
```

On Ubuntu, check and extract the archive, then add its binary directory to `PATH`:

```bash
sha256sum -c --ignore-missing SHA256SUMS.txt
mkdir -p "$HOME/.local/graphhelm-1.0.0"
tar -xzf graphhelm-ubuntu-24.04-x86_64.tar.gz -C "$HOME/.local/graphhelm-1.0.0"
export PATH="$HOME/.local/graphhelm-1.0.0/graphhelm-ubuntu-24.04-x86_64:$PATH"
echo 'export PATH="$HOME/.local/graphhelm-1.0.0/graphhelm-ubuntu-24.04-x86_64:$PATH"' >> "$HOME/.profile"
graphhelm --version
```

On Linux, `SHA256SUMS.txt` lists both archives, so `--ignore-missing` skips the one you did not
download. You still need the repository for this guide's example graphs, Studio, and installation
assets. On macOS or another platform, build from source using the commands below.

Clone over HTTPS using the command below; a read-only clone needs no GitHub account. SSH cloning
requires an authenticated key. Contributors need a GitHub account to open issues and pull requests.
Archive users also need the source clone for the example graphs, Studio, and installation assets;
they can skip the build command and use the binary extracted above.

```sh
git clone https://github.com/stabem/GraphHelm.git
cd GraphHelm
```

To build the CLI from source instead of using an archive:

```sh
cargo +1.97.1 install --locked --path apps/cli
graphhelm --version
```

Expected output has two lines: `graphhelm <version>`, the release number, then `commit <sha>`, the commit the
binary was built from. Compare the first line with the release you installed:

```
graphhelm <version>
commit <sha>
```

`cargo install` puts `graphhelm` on your PATH (`~/.cargo/bin`). If you would rather not install,
`cargo +1.97.1 build --locked -p graphhelm-cli` leaves the binary at `target/debug/graphhelm`
(`.exe` on Windows); use that path wherever this page says `graphhelm`, and pass it to
`studio-up.ps1` as `-GraphHelm`. The harness registration `init` writes (section 6) names the
binary that ran `init` by absolute path, so it works either way; re-run `init` after installing
or moving the binary and the registration follows. On the rehearsal host (a clean Ubuntu 24.04 container, `nproc` = 8)
the release build took 186 seconds. Build time varies with the number and speed of available CPU cores and whether build artifacts are cached.

## 2. `graphhelm init` in your project

Run it in the directory you want GraphHelm to work on — your own repository, or an empty folder.
The GraphHelm clone is not the project; it is where the binary and the Studio live.

```powershell
cd C:\path\to\your-project
graphhelm init --pretty
```

If `init` reports `"harnesses": []`, rerun it with `--harness claude-code` or `--harness codex` to
write that host's registration. Use the name of the host you have installed.

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
graphhelm serve --events "C:\path\to\your-project\.graphhelm\events" --bind 127.0.0.1:8791 --project "C:\path\to\your-project" --keyring "C:\path\to\your-project\.graphhelm\keyring" --key-id studio
```

```bash
export GRAPHHELM_EVENTS_KEY="$(cat /path/to/your-project/.graphhelm/serve.key)"
graphhelm serve --events /path/to/your-project/.graphhelm/events --bind 127.0.0.1:8791 --project /path/to/your-project --keyring /path/to/your-project/.graphhelm/keyring --key-id studio
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

Shortest path, from your project folder: `graphhelm studio start`. It finds the GraphHelm clone
(`--source`, else `GRAPHHELM_SOURCE`, else GraphHelm's own install clone, `~/.graphhelm/source`, described below), fast-forwards it
to `origin/main` when it is on a clean `main` (otherwise it warns how many commits it is behind and
leaves it alone). Pass `--no-update` to skip checking and updating the source. The Studio can be newer
than the CLI archive. To use a specific checkout, pass `--source <clone> --no-update`; the command
will use that checkout as it is. It reruns `npm ci` when the Studio's lock file moved,
and then runs the launcher below (`studio-up.ps1` on Windows, `studio-up.sh` on Linux and macOS) with this
project's events, keyring and key. It needs no PowerShell on Linux or macOS.

```powershell
cd C:\path\to\your-project
graphhelm studio start
```

To update the CLI, run `graphhelm update`. It builds from GraphHelm's private install clone and
uses a separate build directory. The first update builds from scratch. To use your own clone, pass
`--source <clone>` or set `GRAPHHELM_SOURCE`; it is fast-forwarded only when clean and on `main`.
`--keep-local` builds that clone as it is. On Windows, the running executable is restored if the
update fails.

`graphhelm update --check` builds nothing. It checks whether the installed binary matches
`origin/main`; a development build without an install stamp also fails this check.

The same launch by hand. Windows, one command — reuses the Runtime from section 3 if it is already answering, otherwise
starts one; installs dependencies on the first run; opens the browser connected:

```powershell
cd C:\path\to\GraphHelm
powershell -File apps/studio/tools/studio-up.ps1 -Events "C:\path\to\your-project\.graphhelm\events" -Bind 127.0.0.1:8791 -Keyring "C:\path\to\your-project\.graphhelm\keyring" -KeyId studio -GraphHelm graphhelm
```

Expected lines: `[up] Runtime already answering at http://127.0.0.1:8791 and this project's token
opens it - reusing it` (or `[up] starting: graphhelm serve ...` then `[up] Runtime up`), and
`[up] Studio starting at http://127.0.0.1:5183 - the page opens already connected`. The launcher prints no login
link. It listens on `127.0.0.1` only, so from a container its port cannot be published; use the manual path below
there.

Manual dev server, on any platform. It listens on port 5183, the same port as the launchers above, so stop one
before you start the other. Run these commands from the GraphHelm clone:

```powershell
cd C:\path\to\GraphHelm
$env:GRAPHHELM_EVENTS = "C:\path\to\your-project\.graphhelm\events"
$env:GRAPHHELM_RUNTIME_URL = "http://127.0.0.1:8791"
npm --prefix apps/studio ci
npm --prefix apps/studio run dev
```

```bash
cd /path/to/GraphHelm
export GRAPHHELM_EVENTS=/path/to/your-project/.graphhelm/events
export GRAPHHELM_RUNTIME_URL=http://127.0.0.1:8791
npm --prefix apps/studio ci
npm --prefix apps/studio run dev
```

Expected, among the dev server's output:

```
  Studio: http://127.0.0.1:5183/
```

**Open the loopback URL.** Bookmarks, reloads, new tabs and deep links need no login ticket.
The server creates an HttpOnly, SameSite=Strict browser session and keeps Runtime credentials
server-side. Both Studio and Runtime must bind to loopback. Remote/container port publishing
is not supported by this local authentication contract.

`graphhelm studio open` registers the local project using its existing Runtime credential and
opens a reusable URL. `--no-browser` returns that URL in `data.url`. Use `--port <port>` for a
non-default Studio port. Unknown or unavailable projects show an error; they never fall back
to another project's data. With `GRAPHHELM_EVENTS` unset, configure the project before opening it.

The Studio's `/v1` and `/health` requests are proxied by the dev server to `GRAPHHELM_RUNTIME_URL`,
so they stay same-origin; the default is `http://127.0.0.1:8080`, which is why the variable must
say `8791` here.

To stop a foreground Runtime or Studio dev server, press `Ctrl+C` in the terminal where it is
running. If you started it in the background, stop it with the same process manager you used to
start it.

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

Open the Studio to see the run in the Graph tab. It shows the run's objective and its status. A fixture run has no
owner request, so the Graph lists no work for it. A run that waits on you reads `Status: waiting for you`, with a
`Waits on:` row such as `implementation is blocked. Approve it from the CLI: graphhelm execution approve ...`. The Lanes
tab shows the same text under Needs action. The Studio has no approve button: approve from the CLI, as below.

The verbs below run against a fresh store (`E` is the events directory, `F` the fixture file). Set
these paths first. Run from the GraphHelm clone. Pause before approving: a supervised run accepts
`resume` only from `paused`.

```powershell
$E = "C:\path\to\your-project\.graphhelm\events"
$F = "C:\path\to\your-project\.graphhelm\fixtures.json"
graphhelm execution pause   --events $E --execution demo
graphhelm execution approve --events $E --execution demo --node implementation
graphhelm execution resume  --events $E --execution demo --file examples/graphs/manual-override-deploy.yaml --fixtures $F
graphhelm execution status  --events $E --execution demo
```

```bash
E=/path/to/your-project/.graphhelm/events
F=/path/to/your-project/.graphhelm/fixtures.json
graphhelm execution pause   --events $E --execution demo
graphhelm execution approve --events $E --execution demo --node implementation
graphhelm execution resume  --events $E --execution demo --file examples/graphs/manual-override-deploy.yaml --fixtures $F
graphhelm execution status  --events $E --execution demo
```

Approving before pausing is refused with `resume refused: not_paused` (`GHCLI005_EXECUTION_STATE`), and the run
then reads `wedged_quiescence`. Measured on a fresh store:

```
execution.approve  ok     status=running  deploy=ready implementation=ready
execution.resume   FAILED GHCLI005_EXECUTION_STATE: resume refused: not_paused
execution.status   ok     status=running  attention=needs_you  [{"kind":"wedged_quiescence"}]
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

The Studio and the CLI write to the same append-only store; the Runtime serves it; nothing is
duplicated.

## 6. Connect a chat harness

`init` wrote the registration; the harness only has to read it. Session hooks are not in the release archive and `init` does not write them: they come from the GraphHelm plugin, and they need Python 3. See [`INSTALL.md`](../../INSTALL.md#5-install-the-plugin-in-each-requested-host). The active project selects its
Runtime by identity. The token stays in its private file; neither the port nor the token path is
copied into the MCP registration. Every mutation the chat makes is attributed to `agent-chat`.

If no supported chat host is installed, skip this section and use the CLI steps above. A real
chat-host run is not proven here (`OBSERVER_MISSING: real chat host`).

**Claude Code** — `<project>/.mcp.json` was written (or merged, keeping any other server in it).
Start Claude Code in the project directory; it reads `.mcp.json` from the project root. If a
session was already open, restart it. Then, in the chat:

```
use the graphhelm MCP tool list
```

Expected: one row, `demo`, with its attention verdict — the same answer the Studio shows.

**One registration for every project (Claude Code, user scope).** To opt into one user-wide registration, resolve `home/.claude.json=register-mcp` in a reviewed
`graphhelm setup` plan. `--resolve` creates the plan; it does not apply it. Review that exact plan,
save it with `--out <private-plan-file>`, then apply it with `--apply <private-plan-file> --accept sha256:<exact-reviewed-digest>` and a private `--state-root`. See the
[adoption rehearsal](../acceptance/adoption-rehearsal.md) for the complete command sequence. The
registration runs `graphhelm mcp --discover --actor agent-chat` from `~/.claude.json`. The MCP process uses its active project directory (or `--project <path>` when
the host does not launch it there) to find that project's Runtime. Start that Runtime with the `serve`
command from section 3, which passes `--project`: without it only a port record is written, and discovery, which reads the project record, finds nothing.
Until that Runtime is running, `graphhelm mcp --discover` lists no tools. An empty list is expected, not broken: the
tools appear once the Runtime from section 3 is running, and the server then tells the host to list them again.
`serve --project <path>` writes an owner-only record under `~/.graphhelm/runtime/projects/<project-id>/` (or
`GRAPHHELM_RUNTIME_DIR`) with the actual loopback URL, token-file path, and random instance id.
Before reading the token file, MCP checks that `/health` reports both the same project identity
and the same live instance. Multiple projects can therefore use different ports at once. An
ambiguous or stale mapping is refused; MCP never guesses. `--bind 127.0.0.1:0` is supported when
the project is supplied. The older `--url ... --discover` form still follows whichever Runtime
occupies that one port for existing registrations.

**Codex** — `init` wrote `<project>/.graphhelm/codex.config.toml` and does not touch your home
directory. Append its contents to `~/.codex/config.toml` yourself:

```bash
cat /path/to/your-project/.graphhelm/codex.config.toml >> ~/.codex/config.toml
```

```powershell
Get-Content "C:\path\to\your-project\.graphhelm\codex.config.toml" | Add-Content "$env:USERPROFILE\.codex\config.toml"
```

The server `graphhelm mcp` exposes a closed list of tools named after the Runtime verbs (the table is `TOOLS` in `apps/cli/src/commands/mcp/tools.rs`; its size changes with the release) —
`list`, `status`, `briefing`, `events`, `evidence`, `approve`, `pause`, `resume`, `signal`,
`cancel`, `sweep`, … (`apps/cli/src/commands/mcp/tools.rs`); a harness shows them under the
server name, e.g. `mcp__graphhelm__list` in Claude Code. The Studio's WebMCP page tools (the
`graphhelm_*` names) were retired with the old Studio (#1808, #1870): agents use this MCP server,
which serves every Runtime verb. If `init` detected neither harness
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
- [`apps/studio/README.md`](../../apps/studio/README.md) — the Studio in detail: its three views,
  how it logs in, and how to run it.
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
| the Studio shows Project unavailable | the project or Runtime is unavailable, or `GRAPHHELM_EVENTS` was unset | start the project Runtime and reload; check the Studio project configuration |
| the Studio says `Runtime replied 502` | `GRAPHHELM_RUNTIME_URL` points at the wrong port (the default is `8080`) | set it to `http://127.0.0.1:8791` and restart the dev server |
| `studio open` refuses with `GHCLI032_STUDIO_REFUSED` | the requested port does not match the Studio dev server | pass `--port <the-port-from-the-dev-server>`; see [section 4](#4-start-the-studio) |
| `cargo` is not found after installing Rust | this shell does not have Cargo's bin directory on `PATH` | open a new shell; on PowerShell run `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`, or on bash run `. "$HOME/.cargo/env"` |
| a container connection to Studio is reset | the dev server is bound to container loopback | start it with `-- --host` and publish port `5183`; see [section 4](#4-start-the-studio) |
| the message box is refused | the Runtime started without `--keyring`/`--key-id` | restart it with the command `init` printed |
