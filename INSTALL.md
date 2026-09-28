# Install GraphHelm (checklist for AI agents)

This page is for an AI agent that was handed this repository and asked to install GraphHelm. It
is also fine for a person. Do **every** step, in order, and run each step's **Verify** before
moving on. A step whose verify fails is not done; fix it before the next step.

GraphHelm is only useful when all of its parts are connected: the `graphhelm` CLI, a project
provisioned by `graphhelm init`, a running Runtime, the agent plugin, and the `graphhelm` MCP
server registered in the agent. Stopping after one or two of them is a **partial install**; see
[Common partial installs](#common-partial-installs).

Placeholders used below:

| Placeholder | Meaning |
|---|---|
| `<clone>` | this repository, cloned locally |
| `<project>` | the directory GraphHelm will work on (the user's repository, or an empty folder); not the clone |
| `<home>` | the user's home directory (`$HOME`, or `$env:USERPROFILE` on Windows) |
| `<private>` | a private directory outside `<project>` and `<home>/.claude`, for setup plans and backups |

Ask the user for `<project>` if it is not obvious. Never print the contents of `events.token` or
`serve.key`, and never paste them into a prompt, a file, or a command argument.

## 1. Prerequisites

Install what is missing. The commands are in
[`docs/install/GETTING_STARTED.md`](docs/install/GETTING_STARTED.md#prerequisites).

- Rust `1.97.1` with `rustfmt` and `clippy` (the pinned toolchain).
- git.
- Claude Code (`claude`) and/or Codex (`codex`), whichever agent the user runs.
- Node 22+ and npm, only if the user wants the Studio (optional, not needed below).

**Verify:**

```sh
cargo +1.97.1 --version
git --version
claude --version    # and/or: codex --version
```

## 2. Build and install the CLI to a stable path

Install from source so the binary includes the `register-mcp` and `graphhelm-block` setup
decisions. `cargo install` puts it in `~/.cargo/bin`, a stable path that later registrations can
point at.

```sh
cd <clone>
cargo +1.97.1 install --locked --path apps/cli
```

On Windows, if an agent session is running `graphhelm mcp`, the old `graphhelm.exe` is locked and
the install fails. Rename the old binary first, then install again:

```powershell
Rename-Item "$env:USERPROFILE\.cargo\bin\graphhelm.exe" graphhelm.old.exe
cargo +1.97.1 install --locked --path apps/cli
```

**Verify:** `graphhelm --version` prints `graphhelm <version>`, and the path it runs from is in
`~/.cargo/bin` (`command -v graphhelm`, or `(Get-Command graphhelm).Source` on Windows).

## 3. Provision the project: keys, keyring, token

```sh
cd <project>
graphhelm init --pretty
```

`init` creates, under `<project>/.graphhelm/`: the event store (`events/`), the Runtime bearer
token (`events.token`), the sealing key (`serve.key`), and the keyring holding key id `studio`
(`keyring/`). It also adds `.graphhelm/` to `.gitignore` and writes a project `.mcp.json` when
Claude Code is detected. Running it again is safe.

**Verify:** the envelope has `"ok": true` and `"command": "init"`, and these paths exist:
`<project>/.graphhelm/events/`, `<project>/.graphhelm/events.token`,
`<project>/.graphhelm/serve.key`, `<project>/.graphhelm/keyring/`.

## 4. Start the Runtime (port 8791)

The sealing key goes in `GRAPHHELM_EVENTS_KEY`, never in a flag. Start it in a terminal that stays
open (or in the background).

```bash
export GRAPHHELM_EVENTS_KEY="$(cat <project>/.graphhelm/serve.key)"
graphhelm serve --events <project>/.graphhelm/events --bind 127.0.0.1:8791 --keyring <project>/.graphhelm/keyring --key-id studio
```

```powershell
$env:GRAPHHELM_EVENTS_KEY = (Get-Content -Raw "<project>\.graphhelm\serve.key").Trim()
graphhelm serve --events "<project>\.graphhelm\events" --bind 127.0.0.1:8791 --keyring "<project>\.graphhelm\keyring" --key-id studio
```

The first line is `{"ok":true,"command":"serve.started",...}`. If it carries a `warning`
(`GHCLI006_SERVE_INVALID` at `/keyring`), the key variable was wrong: stop, fix, restart.

**Verify** from another terminal:

```bash
curl --silent --fail http://127.0.0.1:8791/health; echo
curl --silent --header "Authorization: Bearer $(cat <project>/.graphhelm/events.token)" 'http://127.0.0.1:8791/v1/executions?limit=5'; echo
```

```powershell
Invoke-RestMethod http://127.0.0.1:8791/health | ConvertTo-Json -Compress
$token = (Get-Content -Raw "<project>\.graphhelm\events.token").Trim()
Invoke-RestMethod "http://127.0.0.1:8791/v1/executions?limit=5" -Headers @{ Authorization = "Bearer $token" } | ConvertTo-Json -Compress
```

Expected: `"command":"serve.health"`, then `"command":"execution.list"` (the token is accepted).

## 5. Install the agent plugin

Claude Code:

```sh
claude plugin marketplace add stabem/GraphHelm
claude plugin install graphhelm@graphhelm
```

Codex (the hooks companion is needed on Codex versions whose plugin loader skips hooks):

```sh
codex plugin marketplace add stabem/GraphHelm
codex plugin add graphhelm@graphhelm
codex plugin add graphhelm-codex-hooks@graphhelm
```

The plugin brings the `graphhelm-guide`, `graphhelm-setup` and `graphhelm-resume` skills. It does
**not** install the CLI, start the Runtime, or register the MCP with a token. Steps 2, 4 and 6 do
that. See [`plugins/graphhelm/README.md`](plugins/graphhelm/README.md).

**Verify:** the agent's plugin list shows `graphhelm` installed and enabled (in Claude Code, `/plugin`), and a new session offers the `graphhelm-guide` skill.

For Codex, `init` also wrote `<project>/.graphhelm/codex.config.toml`. Append it to
`<home>/.codex/config.toml` (see
[`GETTING_STARTED.md` section 6](docs/install/GETTING_STARTED.md#6-connect-a-chat-harness)).

## 6. Register the MCP and write the instruction block (`graphhelm setup`)

`setup` never changes a file until you apply an exact plan digest you reviewed. Use the same
`<project>` as in step 3: the MCP entry it registers points at that project's token file and at
`http://127.0.0.1:8791`.

**6a. Preview.**

```sh
graphhelm setup --project <project> --home <home> --dry-run --json
```

In the output, read:

- `data.plan.spec.decisions`: every item whose `decision` is `unresolved` needs an answer.
- `data.suggestedResolutions`: the suggested answers. Typically
  `home/.claude.json=register-mcp` and `home/.claude/CLAUDE.md=graphhelm-block` (also
  `home/AGENTS.md=graphhelm-block` when that file exists).

**6b. Resolve every unresolved item and write the private plan.** Pass each suggested resolution.
Answer every other unresolved item with `<item>=keep`, unless the user asked to change it. One
unanswered item refuses the whole plan.

```sh
graphhelm setup --project <project> --home <home> \
  --resolve home/.claude.json=register-mcp \
  --resolve home/.claude/CLAUDE.md=graphhelm-block \
  --resolve <other-unresolved-item>=keep \
  --out <private>/plan.json --json
```

Notes: `--out` is required with any `--resolve`. `graphhelm-block` needs the instruction file to
exist; `register-mcp` needs `<home>/.claude.json` to exist (Claude Code creates it on first run).

**Verify:** `"ok": true`, and `data.acceptance.digest` is a `sha256:...` value.

**6c. Review the plan.**

```sh
graphhelm setup --project <project> --home <home> --plan <private>/plan.json --json
```

**Verify:** the operations touch only the files you resolved, and `data.plan.digest` equals the
digest from 6b. In `~/.claude.json` only `mcpServers.graphhelm` may change; in `CLAUDE.md` only
the text between `<!-- graphhelm:begin -->` and `<!-- graphhelm:end -->`.

**6d. Apply exactly that digest.** A backup is written under `--state-root` before any change.

```sh
graphhelm setup --project <project> --home <home> --state-root <private>/state \
  --apply <private>/plan.json --accept 'sha256:<reviewed-digest>' --json
```

**Verify:** `"ok": true`. `<home>/.claude.json` now has `mcpServers.graphhelm`, and
`<home>/.claude/CLAUDE.md` has the marked block. To undo, use `graphhelm restore` (see the
[adoption rehearsal](docs/acceptance/adoption-rehearsal.md)).

## 7. Restart the agent session and check the MCP tools

The agent reads its MCP registration only at start. Close the session and open a new one (in
`<project>` for Claude Code).

**Verify:** in the new session, tools named `mcp__graphhelm__*` are available (for example
`mcp__graphhelm__list`, `mcp__graphhelm__status`, `mcp__graphhelm__briefing`). Call
`mcp__graphhelm__list`: it answers with the executions in the Runtime (an empty list is fine). If
the tools are missing, or calls fail, see the pitfalls below.

## 8. Tell the human to set claude.ai preferences

No tool can edit claude.ai settings. Tell the user to paste the block from the README section
[Make Claude always use GraphHelm + Keel](README.md#make-claude-always-use-graphhelm--keel)
(step 2) into claude.ai → Settings → "Instructions for Claude".

**Verify:** the user confirms it is pasted. Until then, say this step is pending; do not report
the install as complete.

## Done means all of these pass

- [ ] `graphhelm --version` works, from `~/.cargo/bin`.
- [ ] `<project>/.graphhelm/` has `events/`, `events.token`, `serve.key`, `keyring/`.
- [ ] `http://127.0.0.1:8791/health` answers, and the token is accepted on `/v1/executions`.
- [ ] The `graphhelm` plugin is installed in the user's agent (Claude Code and/or Codex).
- [ ] `setup --apply` succeeded: `~/.claude.json` has `mcpServers.graphhelm`, and
      `~/.claude/CLAUDE.md` has the `<!-- graphhelm:begin -->` block.
- [ ] A new agent session lists `mcp__graphhelm__*` tools and `mcp__graphhelm__list` answers.
- [ ] The user has pasted the claude.ai instructions block, or you told them it is still pending.

Report each item as passed, failed or not checked. Do not call the install done while any item
is failed or not checked.

## Common partial installs

| Symptom | Cause | Fix |
|---|---|---|
| Skills like `graphhelm-guide` work, but no `mcp__graphhelm__*` tools | Plugin installed, MCP not registered with a token | Do step 6, then step 7 |
| `mcp__graphhelm__*` tools exist but every call fails | MCP registered, but the Runtime is not running, is on another port, or serves another project's store | Do step 4 with the same `<project>` as step 6; check `/health` and the token |
| `CLAUDE.md` says to use GraphHelm, but the agent has no GraphHelm tools | Rules written, MCP not registered or session not restarted | Include `home/.claude.json=register-mcp` in step 6, then restart (step 7) |
| `cargo install` fails on Windows with "Access is denied" | `graphhelm.exe` is locked by a running `graphhelm mcp` | Rename the old binary, then install again (step 2) |
| `setup --apply` refused with `/adoption/plan_stale` (often for `~/.claude.json`) | The file changed after `--out` (Claude Code rewrites `~/.claude.json` often) | Redo 6b (`--out`), 6c (`--plan`) and 6d (`--apply`) right after each other |
| `setup --resolve` refused | An unresolved item was left unanswered, or `--out` is missing | Answer every unresolved item (`=keep` when unsure) and pass `--out` |
| `serve.started` carries a `/keyring` warning | `GRAPHHELM_EVENTS_KEY` unset or wrong in that terminal | Set it from `serve.key` in the same terminal and restart `serve` |
| `serve` refused at `/bind` | Port 8791 already in use, often by an earlier `serve` | Reuse it if its token matches, or stop it and restart |

More detail: [`docs/install/GETTING_STARTED.md`](docs/install/GETTING_STARTED.md) (the full
path, including the Studio) and [`docs/acceptance/adoption-rehearsal.md`](docs/acceptance/adoption-rehearsal.md)
(setup, backup and restore).
