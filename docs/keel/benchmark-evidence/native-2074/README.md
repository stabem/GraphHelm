# Native workflow rounds (#2074): collected results

Twelve native rounds were run with the Claude subscription CLI (`claude-sonnet-5-5`, effort
low) between 2026-10-26 and 2026-10-28. Each round is 3 real GraphHelm tasks × 6 methods
(direct, superpowers, speckit, openspec, bmad, graphhelm = JPD + Keel), every method running
its own installed native assets. Each round froze its controller, runner, preflight,
evaluators, method assets and schedule (`frozen.json`) before its first cell.

This directory is the first checked-in export. No model was run to produce it.
Per round: `summary.json` (every cell: comparability, acceptance, public/hidden/reached
result, review verdict, tokens by phase, seconds, retries, invalid reasons) and
`frozen.json`. Host paths and account names are replaced by `<host-path>` / `<user>`;
`exportOf.sha256` binds each file to the private original.

A cell is **accepted** only when public tests, the hidden oracle and the reached tests all
pass on the committed candidate head and a separate native review session returned a verdict.
**Comparable** means the flow completed with no controller defect. Tokens are author +
review sessions: input + output + cache read + cache write (provider counters, not a bill).

## Rounds 001-003: invalid as A/B

Controller defects (a worktree deleted mid-round, preflight not run, GraphHelm MCP not
connected) left many cells not comparable. Kept for the record; not used below.

## Rounds 004-006, three repeats each

Each round changed the frozen inputs (004: evaluator v5; 005: task v5.1 and the plugin's
Keel gate fix; 006: task v5.2 adds the `.graphhelm/team.json` the real repository has, without
which even the known fix failed 80 old tests). The repeats r2/r3 only change the lane name.
Rounds are therefore separate cohorts and are not pooled.

Columns: comparable cells per repeat (of 3), accepted per repeat, accepted total, tokens per
repeat summed over the 3 tasks (mean ± standard deviation over repeats in which all 3 cells
of that method were comparable).

### Round 004

| method | comparable | accepted | total | tokens |
|---|---|---|---|---|
| graphhelm | 3/3/2 | 1/1/1 | 3/9 | 1.75 ± 0.00 M (n=2) |
| direct | 3/3/3 | 1/1/1 | 3/9 | 1.62 ± 0.42 M |
| superpowers | 3/3/3 | 1/1/1 | 3/9 | 1.69 ± 0.19 M |
| openspec | 3/3/3 | 1/1/1 | 3/9 | 2.63 ± 0.33 M |
| speckit | 3/2/2 | 1/1/1 | 3/9 | 3.22 M (n=1) |
| bmad | 3/3/3 | 0/0/0 | 0/9 | 0.92 ± 0.14 M |

### Round 005

| method | comparable | accepted | total | tokens |
|---|---|---|---|---|
| graphhelm | 3/3/3 | 2/2/2 | 6/9 | 2.06 ± 0.10 M |
| direct | 3/3/3 | 2/2/2 | 6/9 | 1.63 ± 0.19 M |
| superpowers | 3/3/3 | 1/1/2 | 4/9 | 1.81 ± 0.03 M |
| openspec | 3/3/3 | 2/2/2 | 6/9 | 2.56 ± 0.20 M |
| speckit | 3/3/3 | 2/2/2 | 6/9 | 2.93 ± 0.11 M |
| bmad | 3/3/3 | 0/0/0 | 0/9 | 0.82 ± 0.03 M |

### Round 006 (latest frozen inputs)

| method | comparable | accepted | total | tokens |
|---|---|---|---|---|
| graphhelm | 3/3/3 | 3/2/3 | 8/9 | 2.06 ± 0.19 M |
| direct | 3/3/3 | 2/3/2 | 7/9 | 1.63 ± 0.19 M |
| superpowers | 3/3/3 | 3/3/3 | 9/9 | 1.68 ± 0.10 M |
| openspec | 3/3/3 | 2/3/3 | 8/9 | 2.09 ± 0.25 M |
| speckit | 3/3/3 | 2/3/3 | 8/9 | 2.44 ± 0.16 M |
| bmad | 3/3/3 | 0/0/0 | 0/9 | 0.98 ± 0.22 M |

## Reading (Claude stratum only)

- **Quality gain of JPD + Keel: not shown.** It ties openspec and speckit, is within one cell
  of direct, and in round 006 is one cell below superpowers. The only consistent difference
  is against bmad, which accepted nothing in rounds 004-006 (all 27 cells failed the hidden oracle and got a BLOCK review) while using
  the fewest tokens.
- **Cost:** in rounds 005 and 006, JPD + Keel used fewer tokens than speckit (−30% and −16%)
  at equal acceptance, and more than direct (+26% in both) and superpowers (+14% and +23%).
  Against openspec the gap fell from −20% (005) to −1% (006), inside the spread.
- Superpowers had the best cost/acceptance balance in round 006.

## Limits

- **Codex stratum not run.** The round controller drives only the Claude CLI; #2074 asks for
  both. No Codex native round exists, so nothing here speaks for Codex.
- Three tasks, three repeats: small samples; one cell is about 11 points of acceptance.
- These rounds used a dedicated round controller, not `native_methodology.py`'s frozen
  manifest, and a local Git endpoint with a local native review, not a GitHub PR review.
- Nested-agent token reconciliation is the controller's per-phase sum; it is not the
  `native_pilot_ledger.py` replay.
- Billed USD is unknown.
