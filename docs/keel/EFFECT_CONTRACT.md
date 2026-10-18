# Append effect contract: planted-defect observation (#1476)

`python tools/keel-effect/plant_defects.py --root <slot root> --lane <lane> --jobs 2 --output <report>.json`
plants each of five defects into `core/events/src/local.rs`, runs the contract tests
(`append_effect_contract`, `local_atomicity`, one slot run per file because `workspace slot test`
takes one cargo target) and restores the file byte-identically. Verdicts: `caught` (a test went
red), `missed` (all green), `UNOBSERVED` (anchor did not match once, did not compile, or baseline
not green). The issue's bar: pass at 5/5, kill below 4/5.

Measured on `main` at `e6ce369c` plus the tool (lane gh-claude-1476, 2026-10-18, `--jobs 2`):

| Defect | Verdict | Exit |
|---|---|---|
| drop-dedup | caught | 101 |
| reuse-key | caught | 101 |
| lose-ack | caught | 101 |
| concurrent-writers | caught | 101 |
| kill-mid-write | **missed** | 0 |

Result: 4/5, status `partial` (above the kill bar, below the pass bar). Baseline green.
Total added wall time for the observation: 92.4 s with a warm target (6 slot runs).

## Finding: kill-mid-write was not pinned (fixed by #1582)

With the unterminated-tail check in replay turned into a pass-through, every append contract test
and `local_atomicity` stayed green. The torn-write test in `append_effect_contract.rs` tears the
write mid-record, so the line parser refuses it before that check is reached. #1582 adds
`a_complete_record_missing_only_its_newline_is_refused_as_an_unterminated_tail`: one valid event is
appended, only the journal's final newline is removed, and `open` must refuse with exactly
`IntegrityAt("load_state:journal-tail-unterminated")`. The record parses and its hash chain holds,
so that check is the only refusal possible.

Measured again on `main` at `dd34ae68` plus that test (lane gh-claude-17, 2026-10-18, `--jobs 2`):

| Defect | Verdict | Exit |
|---|---|---|
| drop-dedup | caught | 101 |
| reuse-key | caught | 101 |
| lose-ack | caught | 101 |
| concurrent-writers | caught | 101 |
| kill-mid-write | caught | 101 |

Result: 5/5, status `pass`. Baseline green; `core/events/src/local.rs` restored byte-identically.
Total wall time for the observation: 630.2 s (a cold target for this worktree).

## After ADR-046: a torn tail is recovered, not refused (#41)

ADR-046 changes what kill-mid-write means. On open, the bytes after the journal's last newline
are now trimmed, under the exclusive lock and only after every earlier line verifies. They are
quarantined first in the sibling `<events>.quarantine/`, with a manifest. #1582's newline-only cell
now asserts that recovery too (renamed
`a_complete_record_missing_only_its_newline_is_trimmed_as_an_unacknowledged_tail`). Other cells
assert the rest: the interrupted operation's retry appends exactly one event; a torn first append
trims to an empty journal; a corrupt terminated line, and a tail longer than one batch, still
refuse. The planted kill-mid-write defect turns the unterminated-tail check into a pass-through.
The torn bytes then reach the line parser, open refuses instead of recovering, and the recovery
cells go red.

Measured on `main` at `21348962` plus #41's change (lane gh-claude-17, 2026-10-18, `--jobs 2`):

| Defect | Verdict | Exit |
|---|---|---|
| drop-dedup | caught | 101 |
| reuse-key | caught | 101 |
| lose-ack | caught | 101 |
| concurrent-writers | caught | 101 |
| kill-mid-write | caught | 101 |

Result: 5/5, status `pass`, 318.5 s. `core/events/src/local.rs` restored byte-identically.
