# #209 rebase onto `44936b7` — expectations written BEFORE the rebase

Right zeros and wrong zeros look identical. Only a list written first separates them, so this is the
list, and every row is checked in the RESULT, not in the diff.

| symbol | main `44936b7` | this branch, pre-rebase | required AFTER | why |
|---|---|---|---|---|
| `clearance_identity_registered` | 2 | 2 | **2** | landed with #193; mine and main's are byte-identical |
| `clearance_identity_revoked` | 2 | 2 | **2** | same |
| `completion_claimed` | 0 | 2 | **2** | lane 1's, copied here as scaffolding; my guards append it |
| `completion_cleared` | 0 | 2 | **2** | same |
| `sweep_performed` | 0 | 0 | **0** | already absent: the merge I took was lane 1's REMOVAL commit |
| `overdue_exception` | 0 | 0 | **0** | same |
| `proof_kinds` | 0 | 0 | **0** | lane 1's, never landed, and not carried here |
| `ClearanceOutcome` (type) | 0 | present | **present** | #193 deliberately removed it; this lane restores it with its writer |
| `clearances` (field) | 0 | present | **present** | same |

**The crossing to watch:** main gained my two kinds while #193 simultaneously *removed* the two types
this lane needs. So the same rebase must **keep** what main added and **re-add** what main dropped —
opposite directions in the same files, which is exactly where a blanket `--ours`/`--theirs` deletes
one side silently.

**Both directions get checked:** nothing of main's lost, nothing of this lane's lost. A merge that
verifies only its own side detects half the possible losses.
