# Keel plugin

Keel is a construction pattern for agents that write code: plan, execute, review and merge skills
that need no Runtime, MCP server or journeys. Install this plugin, then run the `keel` skill once in
a project to copy `KEEL.md` and the `keel/` step files into it.

## Agents

The plugin ships three subagent types (`agents/`), one per model role in `KEEL.md` ("Model roles"):

| Agent | Model | Does |
|---|---|---|
| `keel-author` | sonnet | Writes one change inside its card; loads `keel:keel`, then `keel:keel-exec`. |
| `keel-reviewer` | opus (low effort) | Reviews one pull request blind; loads `keel:keel-review`, then `keel:keel-merge` if it approves. |
| `keel-scout` | haiku | Read-only (Read, Glob, Grep): answers one named question with file paths and line numbers. |

They name only skills this plugin ships. They take effect after the plugin is reinstalled.

## Advisor (the owner's setting)

Claude Code has an experimental advisor: a stronger model the working model can consult. The owner
turns it on with `/advisor <model>`, with `advisorModel` in the Claude Code settings, or with
`--advisor`. This plugin does not write settings and its manifest cannot set it. When an advisor is
configured, `KEEL.md` asks the builder to consult it before the plan is fixed, when the same error
comes back a second time, and before reporting done; with none configured, nothing is asked.
