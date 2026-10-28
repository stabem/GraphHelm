# Methodology profile benchmark (#2051): collected results

Two subscription cohorts were collected with `tools/token-bench/methodology.py` (adapted
pilot) and `tools/token-bench/real_methodology.py` (real-project cohort), both frozen before
the first cell ran. Neither was retried, retuned or restarted after outcomes were seen. This
directory is the first checked-in export of those receipts; no model was run to produce it.

Files:

- `adapted-pilot-84/report.json`, `manifest.json`: 6 tasks × 7 profiles × 2 CLIs × 1 repeat.
- `real-project-42/report.json`, `manifest.json`: 3 real tasks × 7 profiles × 2 CLIs × 1 repeat.

The export keeps every attempt and every structured field. Raw native stdout/stderr and
evaluator output are replaced by their SHA-256 and byte length (`stdoutSha256`, ...). Host
paths and account names are replaced by `<host-path>` / `<user>`. `exportOf.sha256` is the
digest of the private original report, which stays outside the repository.

## Adapted-prompt pilot (84 cells, complete)

Codex 0.157.1 (requested model `gpt-6-sol`, observed model not exposed) and Claude Code
2.1.310 (`claude-sonnet-5-5`, observed). Effort low, 180 s per session, serial.

| CLI | profile | accepted | agent s | tokens (see note) |
|---|---|---|---|---|
| codex | direct | 6/6 | 171.9 | in 954,721 (cached 863,616) / out 6,751 |
| codex | tdd | 6/6 | 164.6 | in 1,053,689 (cached 947,712) / out 7,972 |
| codex | spec-kit | 6/6 | 187.4 | in 1,113,170 (cached 1,017,344) / out 8,394 |
| codex | openspec | 6/6 | 212.9 | in 1,147,327 (cached 1,026,176) / out 8,694 |
| codex | bmad | 6/6 | 198.0 | in 1,056,400 (cached 949,376) / out 8,250 |
| codex | superpowers | 6/6 | 214.8 | in 1,183,408 (cached 1,096,192) / out 9,124 |
| codex | jpd-keel | 6/6 | 154.7 | in 780,346 (cached 701,824) / out 6,892 |
| claude | direct | 6/6 | 89.6 | 149,911 |
| claude | tdd | 6/6 | 108.4 | 216,552 |
| claude | spec-kit | 6/6 | 73.3 | 145,108 |
| claude | openspec | 6/6 | 114.6 | 176,070 |
| claude | bmad | 6/6 | 115.3 | 141,670 |
| claude | superpowers | 6/6 | 111.2 | 180,834 |
| claude | jpd-keel | 6/6 | 76.1 | 146,233 |

Every cell was accepted (restored public checks + hidden oracle + completion witness). That
is a **ceiling**: the tasks do not separate the profiles on correctness. Claude token figures
in this cohort are the deduplicated transcript subtotal (input + cache read + cache write +
output); the assistant-message output is a placeholder (#2063), so they are diagnostic, not
native totals. Codex cached input is a subset of input and is not added again.

## Real-project cohort (42 cells, complete)

Three real GraphHelm defects (`task-record-binding`, `native-token-accounting`, `studio-get-dedupe`),
hidden oracle outside the agent directory, same controls.

| CLI | profile | accepted | agent s | tokens |
|---|---|---|---|---|
| codex | direct | 1/3 | 124.0 | in 640,274 (cached 562,816) / out 5,252 |
| codex | tdd | 1/3 | 159.5 | in 913,403 (cached 797,696) / out 7,161 |
| codex | spec-kit | 1/3 | 152.6 | in 723,298 (cached 650,624) / out 7,558 |
| codex | openspec | 2/3 | 184.3 | in 886,002 (cached 778,496) / out 9,012 |
| codex | bmad | 1/3 | 176.4 | in 980,657 (cached 857,216) / out 8,664 |
| codex | superpowers | 3/3 | 210.9 | in 1,257,320 (cached 1,172,352) / out 9,571 |
| codex | jpd-keel | 2/3 | 165.1 | in 848,250 (cached 762,112) / out 7,775 |
| claude | direct | 2/3 | 125.3 | 330,472 |
| claude | tdd | 2/3 | 153.1 | 806,849 |
| claude | spec-kit | 1/3 | 150.1 | 508,792 |
| claude | openspec | 3/3 | 168.9 | 641,963 |
| claude | bmad | 2/3 | 129.4 | 474,478 |
| claude | superpowers | 1/3 | 161.1 | 596,865 |
| claude | jpd-keel | 2/3 | 146.0 | 446,614 |

Claude counts here are native `modelUsage` totals (input + cache read + cache write + output).

## What this does and does not show

- No winner. With 3 cells per profile and CLI, a 1-cell difference is noise. The best profile
  differs by CLI (codex: superpowers 3/3; claude: openspec 3/3), which is what noise looks like.
- JPD+Keel was never worst and never best on acceptance; its token use sat in the middle.
  No gain is claimed.
- Profiles are adapted prompts, not native framework installations (#2074 measures those).
  Method adherence and OS-enforced oracle secrecy are unobserved.
- CLIs are separate strata and never pooled. Billed USD is unknown (`billedUsd: null`);
  token counts are provider wire counters, not a bill.
- One repeat; a stronger claim needs a new frozen design with repeats.
