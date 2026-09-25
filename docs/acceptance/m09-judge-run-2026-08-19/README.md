# M09 second-story judge run — transcript, not a store

Transcript, not a store: no committed event store here, no `format.json`, no `blobs/`. The
files below are a raw HTTP audit log, a judge verdict, an execution-trace journal, the two
graph specs the run used, and two replay attempts that both errored identically. Nothing here
can be opened with `graphhelm execution status`; nothing here reproduces a running store. No
`mkdir` repairs this — the same distinction the eight M08 transcript directories draw
(`docs/acceptance/m08-run-2026-08-18/` and siblings, see their own READMEs and `PR #77`).

## What this run was

The paid judge run for the second-story spec (`.factory/draft-second-story.md`, reviewed and
`[APPROVED]` before the run — see the M09 close record for the review). Three executions on
one `serve` process: `exec-m09-release` (primary, deliberately blocked after four identical
`deploy` failures), `exec-m09-release-dup` (a duplicate scheduled job, same block), and
`exec-m09-judge` (the judge itself, playing an on-call operator). Coverage goal: exercise the
seven MCP tools no M08 judge run ever touched (`signal`, `approve`, `pause`, `resume`,
`cancel`, `probe`, `wake_wait`).

## What is intact and checksummed (`SHA256SUMS`)

- `read-audit.jsonl` — every HTTP request/response to every MCP-tool-backed route,
  unconditionally (auth-refused requests included; recorded OUTSIDE the auth middleware layer
  by construction — `apps/cli/src/commands/serve/mod.rs:288-317`). This is the coverage
  evidence: all 7 previously-untouched tools fired (`signal` 400, `approve`/`pause`/`cancel`
  200, `resume` 200/409/500, `wake-lease` POST+GET 200, `gateway/routes`+`gateway/probe` 200).
- `verdict.json` / `verdict-raw-reply.json` — the judge's verdict. `passed: false`, 8 findings
  (2 critical, 2 high, 2 medium, 2 low). The redesign's own mechanism (`MAX_IDENTICAL_OUTCOMES`
  block) fired correctly in this real run; the judge triaged the resulting incident correctly;
  the release did not ship, on real MCP-surface defects the story's own design surfaced
  (tracked as internal issues #82 and #83 in the private development archive) — not on a flaw in the story or
  the state machine it exercises.
- `journal.txt` — `exec-m09-judge`'s own execution trace.
- `release.yaml` / `release-dup.yaml` / `judge.yaml` — the graph specs the run used, verbatim.
- `replay-1.txt` / `replay-2.txt` — two `graph replay` attempts. **Byte-identical to each
  other, and both worthless as a determinism check**: both are the SAME error
  (`GHE010_STREAM_SELECTION_REQUIRED` — replay against a multi-execution store needs an
  explicit scope/stream selection, never supplied). This confirms the FAILURE was
  deterministic across two attempts; it does NOT confirm anything about replay of the actual
  recorded history. Do not cite these two files as "replay determinism confirmed."

## What is absent, and why no `mkdir` fixes it

No event store, no evidence blobs, no `format.json`. This run's evidence lives in the HTTP
audit log and the judge's own verdict, not in a directly-openable local repository — the same
shape as the eight M08 transcript directories, for the same reason: what was captured was the
judge's interaction with a live `serve` process, not a store snapshot.
