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

## Finding: kill-mid-write is not pinned

With the unterminated-tail check in replay turned into a pass-through, every append contract test
and `local_atomicity` stay green. The torn-write test in `append_effect_contract.rs` therefore does
not reach that check (or an earlier guard refuses first). That is a gap in the contract tests, not
in the tool. It needs its own card: a test whose only possible refusal is that check.

## After ADR-046: a torn tail is recovered, not refused (#41)

ADR-046 changes what kill-mid-write means. On open, the bytes after the journal's last newline
are now trimmed, under the exclusive lock and only after every earlier line verifies. They are
quarantined first in the sibling `<events>.quarantine/`, with a manifest. The contract cells
assert that recovery: the interrupted operation's retry appends exactly one event; a torn first
append trims to an empty journal; a corrupt terminated line, and a tail longer than one batch,
still refuse. The planted kill-mid-write defect turns the unterminated-tail check into a
pass-through. The torn bytes then reach the line parser, open refuses instead of recovering, and
the recovery cells go red.

Measured on `main` at `21348962` plus #41's change (lane gh-claude-17, 2026-10-18, `--jobs 2`):

| Defect | Verdict | Exit |
|---|---|---|
| drop-dedup | caught | 101 |
| reuse-key | caught | 101 |
| lose-ack | caught | 101 |
| concurrent-writers | caught | 101 |
| kill-mid-write | caught | 101 |

Result: 5/5, status `pass`, 318.5 s. `core/events/src/local.rs` restored byte-identically.
