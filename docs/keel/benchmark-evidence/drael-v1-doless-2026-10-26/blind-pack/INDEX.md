# Blind review pack - exhibits R01-R14

Each exhibit is an independent final diff produced by an agent session on one frozen GraphHelm task.
For every exhibit, the file contains: task metadata (id, kind, split), the exact prompt the agent received,
the Keel card (when a card was part of the task definition), the diff (or '(empty diff)' when no files changed),
and, where the agent wrote a textual answer, that answer.

The same task appears under several exhibit ids. You do NOT know which agent configuration produced any exhibit.
Do not assume anything about which exhibit came from which configuration.

- R01: task `om-win-arm64` (empty diff)
- R02: task `py-actor-session-header` (25 lines)
- R03: task `py-sessionend-cap` (75 lines)
- R04: task `py-sessionend-cap` (73 lines)
- R05: task `docs-doc-comments` (66 lines)
- R06: task `docs-config-risk` (45 lines)
- R07: task `docs-config-risk` (47 lines)
- R08: task `docs-config-risk` (48 lines)
- R09: task `py-mcp-meta` (60 lines)
- R10: task `py-mcp-meta` (68 lines)
- R11: task `rs-signal-id-retry` (54 lines)
- R12: task `rs-signal-id-retry` (280 lines)
- R13: task `rs-signal-id-retry` (168 lines)
- R14: task `rs-signal-id-retry` (271 lines)