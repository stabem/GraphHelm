> **PROVENANCE: this document became the body of the PR that closed #96.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

Closes #96

`GHCLI016_DRIVER_FAILURE` answered two opposite hold-states with one value. #83 fixed the *ordering*
so a refused setup commits nothing — but the *response* still said the same thing whether nothing had
committed or the decision had committed and the work had then failed. From the operator's chair those
are opposite facts, and one code for both made them one.

| class | code | the operator's position |
|---|---|---|
| **(a)** setup refusal | **`GHCLI019_DRIVER_SETUP`** (new) | nothing committed, **hold intact** — fix the environment and retry |
| **(b)** mid-drive failure | `GHCLI016_DRIVER_FAILURE` | decision committed, work attempted and failed — **hold gone**, execution attended |

## The change is a constructor swap, not new logic

#83's hoist already made the distinction structural: every site raising (a) runs **before**
`execute_prepared`, and the single remaining `driver_failure` caller on this path sits in the
drive-result handler **after** it. The response was destroying a distinction the code already had.

`GHCLI016`'s doc now scopes its narrowed meaning to the start/resume path explicitly.

## Sealed red, observed in natural form

Reverting the four-site swap — nothing else — with casualties predicted and sealed **before the run**
(`.factory/c-agent-96-slot-seal.md`, sha256 `9ec8c327…`, written with zero cargo processes running):

```
a_resume_whose_drive_setup_fails_leaves_the_operator_hold_intact ... FAILED  :424
a_start_whose_drive_setup_fails_commits_no_execution            ... FAILED  :639
the_same_idempotency_key_after_a_failed_setup_still_executes    ... FAILED  :731
a_resume_whose_drive_setup_succeeds_still_commits_the_decision  ... ok
a_second_resume_after_a_failed_one_is_still_accepted            ... ok
```

**Three casualties, exactly the three sealed. Both sealed survivors held** — and that half is the
discriminating evidence: a blanket 5/5 red would have **refuted** the seal, because the claim is that
this change alters only which code a *failure* carries. The positive control and the hold-survival
guard are untouched, so the change is as isolated as this commit says.

**Recorded against the seal: I predicted `:425 / :640 / :732` and the panics landed at
`:424 / :639 / :731`** — off by one, all three, one structural reason. I derived the lines by grepping
for the assertion string, which sits *inside* the macro; a panic reports the macro's opening line.
Right about which tests, how many, and the split; wrong about the thing I stated most precisely.

## Guard-header framing, verified strictly-stronger

The three casualties are exactly the tests asserting the **code**; the two survivors assert
**behaviour** (a committed decision; a retry that is not a state refusal). Had a behavioural guard
fallen, the header's claim — that the guards having to change *is* the evidence — would be false,
because the change would have altered behaviour rather than only the code a failure carries.

## Not split here, named rather than left as an oversight

The immediate-`pause` budget path answers the same code for a **third** question — *did my pause take
effect?* — whose remedy is an **unknown**, not a failure, since the pause may still record after the
budget elapses. That is **#130**, cited at the site. It is **not contradicted** by this change, which
scopes `GHCLI016`'s narrowed meaning to start/resume; the site stays a pre-existing approximation
rather than becoming false. I argued otherwise in #130's own opening and corrected it there after the
reviewer read this constant's doc instead of my summary of it.

Allocating `019` required grepping the whole tree, which surfaced **`GHCLI009` reaching the wire
twice with different meanings**. Not fixed here — renaming a wire-visible code is its own blast
radius — and filed as **#131**: the registry's absence is the defect, the collision its sighting.

## Half-measured, and labelled rather than dressed up

**(a) is guarded** by three tests. **(b) rests on a structural argument**: after the swap,
`routes.rs`'s drive-result handler is the only remaining `driver_failure` caller on this path, and it
is post-commit by construction. **That is an argument, not a measurement** — verifiable by reading the
diff, which the reviewer did at read-grain rather than accepting the label. The guard that would make
it a measurement — *(a) and (b) answering different codes in one run* — needs a real-executor story
(`RefusingSealer` never fires on a fixture story, by its own doc), and is seeded in #96's follow-up
alongside S5-measured.

## Slot record

- **Slot-start clean, per the law this slot produced:** `cargo clean -p` × **20** workspace-own
  packages, 21111 files / 8.7 GB. Recorded beside the results.
- **Base:** merged `origin/main` (**not** rebased — the approval is pinned to `6e9c0fe`, a merge
  preserves it). Merge commit; deltas beyond the reviewed diff (the merge, and a CRLF→LF refresh of
  the test file with content byte-identical) confirmed by the reviewer.
- **Gate exit read from the producing command, never through a pipe** — the pattern `ci/gate.ps1`'s
  own docs now carry.
