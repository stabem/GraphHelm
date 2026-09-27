# GraphHelm guide

GraphHelm is an experimental, local-first agent operating system. Its plugin skills help an agent decide what to do; the CLI and Runtime provide typed contracts, deterministic checks, execution control, and evidence. A skill's text is never authority to bypass permissions, publish a graph, or claim an unobserved result.

```mermaid
flowchart LR
  A[User promise] --> B[Scope and risk]
  B --> C{Adequate observer?}
  C -->|No| M[Keep proof unresolved]
  C -->|Yes| D[Keel: smallest scoped change]
  D --> E[Run reached checks]
  E --> F[Independent review]
  F --> G[Merge and verify]
  G --> H[Resume: two next options]
```

For a small reversible change with a clear observer, use the direct route. For unclear proof, persistence, permissions, compatibility, security, or external effects, use Journey-Proven Development (JPD): describe the user journey, compile each promise into an observable obligation, and refuse a success claim if the required observer is missing. Keel keeps context and new code surface proportional, then asks for the **lowest total cost per proven delivery** after quality is preserved. Count repairs, reviews, and repeated work, not tokens alone. Neither method replaces the repository's review and merge rules.

GraphHelm's Governor owns operational graph publication. Agents send typed signals and proposals; deterministic code enforces schemas, policies, permissions, and state transitions. A plugin install does not start the Runtime, configure MCP credentials, activate an extension package, or change a user's existing instructions. `graphhelm setup` starts with inventory and a preview; applying a reviewed plan requires explicit acceptance and creates a backup before supported host files change. `graphhelm restore` previews recovery before applying it. See the [CLI preview](https://github.com/stabem/GraphHelm/releases/tag/v0.1.0), [getting started](https://github.com/stabem/GraphHelm/blob/main/docs/install/GETTING_STARTED.md), [skills map](https://github.com/stabem/GraphHelm/blob/main/docs/skills/README.md), and [current delivery process](https://github.com/stabem/GraphHelm/blob/main/docs/process/DELIVERY.md).

## Install from the repository

The `graphhelm` plugin contains this guide, `graphhelm-setup`, and `graphhelm-resume`. The same marketplace also lists the existing `graphhelm-jpd` and `graphhelm-development-contracts` plugins. Install those companions only when their skill families are needed; both can connect to the same local Runtime and require a separately configured trusted CLI, token file, and per-session actor. Installing any plugin alone does not install the GraphHelm executable.

In **Claude Code**:

```text
claude plugin marketplace add stabem/GraphHelm
claude plugin install graphhelm@graphhelm
```

In **Codex CLI**:

```text
codex plugin marketplace add stabem/GraphHelm
codex plugin add graphhelm@graphhelm
```

After installation, start a fresh session. In Claude Code, use `/graphhelm:graphhelm-guide`, `/graphhelm:graphhelm-setup`, or `/graphhelm:graphhelm-resume`; Claude namespaces plugin skills, so the installed commands are not bare `/graphhelm-setup` or `/graphhelm-resume`. In Codex, invoke `$graphhelm-guide`, `$graphhelm-setup`, or `$graphhelm-resume`. The setup skill guides the separate `graphhelm setup` CLI through inventory, a reviewed plan, backup, and restore; invoking the skill alone changes no host files. The resume skill reads current evidence and offers exactly two next actions with one recommendation; it does not take either action for you.

## Session hooks (version 0.1.4)

This installed plugin includes command hooks for Claude Code and Codex. Unbound sessions are
silent by default. When the session is explicitly bound to a GraphHelm execution, `SessionStart`
injects a short Keel reminder, and startup and
resume read one Runtime briefing and return a bounded summary of its next action and pending
work. There is no unconditional second briefing read: request detailed evidence only when the
next action needs it. Compaction restores cached context labeled as not refreshed, without an
HTTP read. A cached summary never authorizes a state transition; the Runtime still validates
the current state. `SessionEnd` records an attributed
`agent_session_ended` signal. It never marks a task or node complete. The hook does not create a
run, pick a run by project name, or forward the host's raw payload.

Python 3 must be on the host's `PATH`. Set these variables in the environment that launches the
agent host:

- `GRAPHHELM_EXECUTION_ID`: exact execution ID to bind. Without it, the hook makes no Runtime
  request and injects no context by default.
- `GRAPHHELM_KEEL_CONTEXT=1`: explicitly enable the short Keel reminder in an unbound session.
- `GRAPHHELM_TOKEN_FILE`: path to the local Runtime token file, required for a bound run.
- `GRAPHHELM_RUNTIME_URL`: Runtime API origin; defaults to `http://127.0.0.1:8791`.
- `GRAPHHELM_SESSION_ID`: optional expected host session ID. A mismatch leaves the operation
  unobserved rather than reading or writing a different session's binding.
- `GRAPHHELM_NODE_ID`: optional explicit node reference. It is labeled as configured; the hook
  does not infer an assignment from the next pending node or claim that the Runtime assigned it.

For example, in PowerShell before launching an agent in a terminal:

```powershell
$env:GRAPHHELM_EXECUTION_ID = 'run-example'
$env:GRAPHHELM_TOKEN_FILE = 'C:/path/to/events.token'
$env:GRAPHHELM_RUNTIME_URL = 'http://127.0.0.1:8793'
claude
```

The same environment variables apply to Codex. A desktop app that was already running will not
inherit variables set later in a terminal. Codex asks the user to review and trust new plugin
hooks before running them. Updating the plugin does not override that host decision.

The token stays in its file and travels only to a loopback HTTP or HTTPS Runtime. A redirect is
refused. The hook prints `UNOBSERVED` if a bound read or write fails, without blocking the agent
session or claiming a record landed. End signals require a Runtime keyring; this hook does not
write an unsealed `evidenceOut` file. A full signal budget also leaves the end unobserved.

A repeated end hook uses the same signal body and idempotency key. Once the Runtime acknowledges
the matching execution and signal IDs, the hook saves that confirmation and skips later sends.
An unconfirmed request remains retryable. A rejected graph mutation does not mean the signal
was not recorded: those are different results in the Runtime contract.

State is scoped to host, Runtime origin, execution and session, outside the repository under the
user's local state directory; `GRAPHHELM_HOOK_STATE_DIR` can override that path. Actors carry a
session-specific identity rather than only a host name. The end request also sends the host session
ID as `X-GraphHelm-Actor-Session` for the Runtime's structured presence record. These observations
do not assign or complete operational nodes, and session end is not proof that the work passed.

Since version 0.1.3, hooks use a new state and signal-key namespace: 0.1.2 timestamp files have
no delivery acknowledgment and are not migrated as proof. A corrupt record stays unobserved; it is
not silently replaced with a new signal under the same key. The local lock is released by the OS if the host
terminates the hook. Delivery deduplication is scoped to this protocol, not an exactly-once claim
across plugin upgrades or deletion of local state.

The hook calls no model. Cached compaction avoids a Runtime request, and confirmed repeated end
delivery avoids another request. These are request-count guarantees covered by local HTTP tests,
not a measured reduction in a complete task's billed tokens. The hook returns only bounded typed
summary fields, never the full objective, transcript or Runtime command text.

### Setup inspection

The `graphhelm-setup` skill includes a hook inspection step. Resolve the actual installed plugin
directory from the host's plugin inventory, then run:

```text
python <installed-plugin>/hooks/session_hook.py inspect --host claude
python <installed-plugin>/hooks/session_hook.py inspect --host codex --session-id <actual-session-id>
```

Inspection is read-only, makes no Runtime request, and does not read token contents. It reports
configuration and local script observations. It cannot establish that the host trusted or loaded
the hook; a manual invocation can also create a local observation. Keep activation unverified
until the host's own fresh-session event is observed. Do not register a second copy in settings.
The adoption CLI does not migrate opaque hook commands; the setup skill identifies legacy
registrations and requires an explicit, backed-up migration for those entries.

To add a companion, install `graphhelm-jpd@graphhelm` or `graphhelm-development-contracts@graphhelm` with the host's `plugin install` / `plugin add` command. The [skill catalog](https://github.com/stabem/GraphHelm/blob/main/docs/skills/README.md) shows when each is useful. Their MCP registration needs `GRAPHHELM_CLI` to be an absolute trusted executable path, `GRAPHHELM_TOKEN_FILE` to name a local token file, and `GRAPHHELM_ACTOR` to identify the chat session. Never paste the token value into a manifest or prompt.
