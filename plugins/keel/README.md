# Keel plugin

Keel is a construction pattern for agents that write code: plan, execute, review and merge skills
that need no Runtime, MCP server or journeys. Install this plugin, then run the `keel` skill once in
a project to copy `KEEL.md` and the `keel/` step files into it; running it again upgrades the files
the project never edited.

## Upgrading an installed copy

Run `keel_setup.py <project>` again after updating the plugin. Per file (`KEEL.md` and the four
`keel/` step files): equal to the bundled text, line endings aside, nothing happens; equal to a past
bundled version, so never edited, it is replaced by the bundled text and the run says
`updated <name>`, keeping the file's line endings; any other text is the project's own and is kept,
with a note. A copy newer than the running plugin is unknown, so it is kept. The past versions are
listed as sha256 digests in `setup/known_digests.json`, which `tools/keel-plugin/sync_keel_plugin.py`
only appends to; the list starts at this repository's public history, so a copy older than that is
kept. A project that edited a file back to the exact text of an older bundled version is treated as
unedited. A replacement that the file system refuses leaves the old file and no temporary file, and
the run exits 1 after its other steps.

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
