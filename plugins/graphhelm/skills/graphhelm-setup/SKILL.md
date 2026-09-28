---
name: graphhelm-setup
description: Use when the owner wants to inspect or adopt GraphHelm in a Claude Code or Codex host, asks about graphhelm setup, or invokes graphhelm-setup to review host configuration and recovery.
---

# GraphHelm setup

This chat skill guides the existing `graphhelm setup` CLI; it does not install the executable or grant permission to change host files. Read the [setup guide](https://github.com/stabem/GraphHelm/blob/main/docs/install/GETTING_STARTED.md) and the [adoption rehearsal](https://github.com/stabem/GraphHelm/blob/main/docs/acceptance/adoption-rehearsal.md) when concrete commands or host limits matter. Check the installed CLI's help before using version-specific flags.

1. Identify the exact project, host profile, CLI executable, and intended scope. If any essential path is missing, ask for it. Inventory with `graphhelm setup --project <project> --home <profile> --dry-run --json` when the CLI is available and the paths are authorized. Treat the preview as read-only and report supported items, unresolved instructions, protected settings, and discovery limits. Do not claim effective managed policy from fixed-file inventory.
2. Show a concrete keep/replace decision for each unresolved instruction, preserving personal preferences and deny rules. Do not choose a replacement silently. A reviewed replacement is supplied as a separate file through `--resolve <item>=replace:<file>`; `--resolve <item>=keep` preserves it. Write the candidate plan with `graphhelm setup --project <project> --home <profile> --resolve <decision> --out <private-plan>` to an owner-only file outside the project and profile, then inspect the exact decisions and bytes there. `--plan <file>` shows a redacted preview; it does not approve the plan.
   Two generated decisions build the reviewed bytes for you and travel the same reviewed path: `--resolve home/.claude.json=register-mcp` adds only the `mcpServers.graphhelm` entry to Claude Code's user-scope registration (every other key is kept, and apply refuses any other change), and `--resolve <instruction item>=graphhelm-block` inserts or refreshes a marked GraphHelm + Keel block in that file, keeping everything outside the markers. `--dry-run` lists them under `suggestedResolutions`. Use them only when the installed CLI's `setup --help` names them.
3. Only after the owner has reviewed the exact plan and authorized its digest, use `graphhelm setup --project <project> --home <profile> --state-root <private-root> --apply <plan> --accept <exact-digest>`. Keep the state root private and outside both target roots. The CLI must create a recoverable backup before mutation; never hand-edit host files to bypass a refusal, stale plan, conflict, or unsupported policy. If applying was not requested or authorized, stop at the preview and give the next command without executing it. When stopping an apply request, explicitly state that no backup exists yet, that apply must create one before mutation, and that recovery is previewed with `graphhelm restore --state-root <private-root> --backup original --json`.
4. Report the CLI result accurately. Installed files are `installed_unverified` while a trusted fresh-session observer is missing; neither a successful exit nor a user-authored receipt proves activation. Say this even when declining an unreviewed apply request, so the owner knows what an eventual apply would and would not prove. Show `graphhelm restore --state-root <private-root> --backup original --json` as the recovery preview. A restore apply also needs its own reviewed plan and exact digest. Never print secret values or private replacement bytes.

## Inspect the bundled session hooks

The installed Claude plugin bundles Claude session hooks. Affected Codex versions use the separate
`graphhelm-codex-hooks` compatibility companion. Include both in the setup inventory; installing
the CLI and applying its adoption plan do not prove that a host loaded or trusted those hooks.
Keep the plugin as the single source of hook registration. Do not copy its commands into user or
project settings, which would add another handler.

1. Resolve the actual installed plugin directory from the host's plugin inventory. Verify its
   version and hook files. Use Python 3 to run `python <installed-graphhelm>/hooks/session_hook.py
   inspect --host claude`, or run the same command from the actual installed
   `graphhelm-codex-hooks` directory with `--host codex`. This inspection is local and read-only; it does not
   contact the Runtime or read the token contents. Report its configuration and local observation
   results separately from host trust and activation, which remain unverified without a trusted
   fresh-session observer.
2. Check the intended Runtime origin and exact execution binding in the environment of the host
   that will launch the session. `GRAPHHELM_SESSION_ID` can pin an expected session; a mismatch must
   not read or write another run. `GRAPHHELM_NODE_ID`, when supplied, is an explicit configured
   node reference, not proof that the Runtime assigned that node to the agent. A desktop process
   already running does not inherit environment changes from a later terminal.
   Unbound sessions are silent by default; use `GRAPHHELM_KEEL_CONTEXT=1` only when the owner wants
   the standalone Keel reminder without a GraphHelm execution.
3. Inventory direct legacy registrations such as `graphhelm-identity.ps1` alongside plugin hooks.
   Describe their actual behavior before proposing removal: the old identity hook also supplied
   branch and delivery identity reminders. Preserve unrelated memory and security hooks. The
   adoption CLI treats hook commands as opaque data; do not claim its plan migrates those commands.
   Any separate authorized migration needs a configuration backup, exact handler selection, and a
   restoration path. Never delete all hooks by event name or bypass a refused adoption plan.
4. After the host starts a fresh trusted session, inspect with `--session-id <actual-session-id>`
   to narrow local observations. A local record proves only that this script ran; a manual invocation
   can create one too. Confirm host activation from the host's own hook event, then distinguish a
   live Runtime read, cached context not refreshed, and acknowledged end-signal delivery. Session
   end never proves task completion. Do not trigger a paid model turn merely to check installation.

Prefer the GraphHelm MCP tools (`mcp__graphhelm__*`, or `mcp__plugin_graphhelm_graphhelm__*` from this plugin) for Runtime reads once they are connected; the adoption commands above stay CLI-only.

If the CLI is absent, point to the [CLI install path](https://github.com/stabem/GraphHelm/blob/main/docs/install/GETTING_STARTED.md)
and report that CLI setup did not run. Local hook inspection can still run with Python 3.
