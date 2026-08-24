# #209 MERGE into `6cfae28` — expectations written BEFORE the merge

Supersedes `j-209-rebase-expectations.md`, which was written for a REBASE onto `44936b7`. Two
things changed and both invalidate that list's `main` column:

1. **The base moved.** Lane 1 (#244, `8932641`) and the fmt/clippy hotfix (#245, `6cfae28`) landed.
   The symbols that list recorded as `0` on main are no longer `0`.
2. **The operation changed.** Factory rule (H): integrate by **MERGE, not rebase** — shas cited in
   issues and PRs die in a rebase, and this whole wave cited blueprints by sha.

The old list is not deleted. It was true when written, against the base it names.

## Measured on both refs BEFORE the merge

| symbol | `origin/main` @6cfae28 | branch @2ae37e2 | required in the RESULT |
|---|---|---|---|
| `EventKind` variant set | 38 | 38, **byte-identical** | the SAME 38 |
| `ClearanceOutcome` | **0** | 16 | **present** — branch-only, main's absence must not win |
| `pub clearances` | **0** | 1 | **present** — same |
| `proof_kinds` | **7** | 6 | **every main site survives** — main is AHEAD here |
| `clearance_registry` | 13 | 16 | **the UNION**, not either count |
| `SweepPerformed` | 0 | 0 | 0 |
| `OverdueException` | 0 | 0 | 0 |

## The crossing, restated for the new base

It did not go away when the base moved — it INVERTED in one row. `ClearanceOutcome` and
`clearances` exist only on the branch, so a resolution favouring main deletes them. `proof_kinds`
has a site on main the branch does not have, so a resolution favouring the branch deletes that.
**The two directions are in the same files.** This is the exact shape where a blanket `--ours` or
`--theirs` produces a clean, conflict-free, silently wrong tree.

`--theirs` on a two-owner file is not a policy choice, it is a deletion. The question is never
"whose file is this" but **"what do I have here that the other side never had"** — and the control
belongs on the RESULT, not on the diff.

## How each row is checked

By SET, never by count, wherever a set is available: a rename preserves a count, and every row
above whose evidence is a number is the weaker row. `EventKind` is checked by `diff` against the
pre-merge enum text, which is the one row here that has a real set oracle.

## What this list will NOT prove

That the merged tree COMPILES or that its tests pass. Symbol presence is a loss detector, not a
correctness oracle: every row can read correct while the fold is broken. The gate is the separate
instrument, and `--workspace --all-targets` on the merge RESULT is the one that counts (ED-18).
