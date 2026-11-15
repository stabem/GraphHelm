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

## Advisor (set by the installer)

Claude Code has an experimental advisor: a stronger model the working model can consult.
`keel_setup.py <project>` writes `"advisorModel": "fable"` into `<project>/.claude/settings.json` on
every run: it creates the file, or adds only that key and keeps the rest; an existing `advisorModel`
is left alone. `--advisor <model>` picks another value, `--no-advisor` skips the step. A settings
file that is not a JSON object is not touched and the run exits 1 after its other steps. The plugin
manifest cannot set it, and user-level settings are never written.

Three risks. A committed project value overrides each collaborator's own advisor choice. `fable`
needs access to that model; when a pairing is unavailable Claude Code drops it without an error, so
the setting is then silently off. The advisor itself is experimental.

When an advisor is configured, `KEEL.md` asks the builder to consult it before the plan is fixed,
when the same error comes back a second time, and before reporting done; with none configured,
nothing is asked. The scout never calls it.
