**Posting note.** A first attempt to publish this was cut off by a machine reboot and left no
comment; verified before re-posting (`#209` had zero comments, with `#197`'s three as the positive
control that the instrument sees them). **The head is unchanged at `deff9dd`, the sha this review
was written against** — if it moves later, this verdict is about `deff9dd` and nothing else.

# Review — frozen-then-diff, criteria sealed before the body or diff were opened

Criteria and five predictions were derived from **#201** and from **`origin/main` `d0b3f04`**, plus PR
*metadata only* (base, head, file list, counts). Head reviewed: **`deff9dd`**.

**Contamination declared.** The routing message told me, before I read anything, that the body claims
seven reds at seven distinct panic sites, a mutation matrix with three distinct failure sets, and a
declaration of what is not citable. **Knowing what an author claims biases a reviewer toward
confirming it**, so the frozen file sealed what would make each claim fall — and one of the five
predictions is scored below **against me**.

## Predictions, scored

| # | sealed | outcome |
|---|---|---|
| 1 | the matrix's failure sets do not cover all seven guards | **held** — see F1 |
| 2 | the size-2 set is `{registered_after, survives_revocation}` | **falsified, and my derivation was the wrong one** — see below |
| 3 | the seven panic sites are cited as a count, not as seven distinct sites | **falsified** — seven line numbers are given |
| 4 | `nonce.rs` is in the diff with no justification in the body | **held** — F3 |
| 5 | the gate-run manifests are unexplained | **held** — F3 |

**Prediction 2 is where I was wrong, and the correction is worth more than the prediction.** I derived
that only two of the seven separate *per-sequence* from *final-registry* semantics —
`registered_after` and `survives_revocation`, pointing in opposite directions — and I wrote off
`interleaved_registrations_and_clearances_replay_identically` as testing determinism rather than the
thesis. **T1's observed set includes it.** Interleaving is exactly the arrangement where the two
semantics diverge, so that guard is a third discriminator, not scenery. **The matrix is a better
instrument than my reading of the guard list.**

## What holds

**F1 — the matrix leaves two of the seven in no failure set at all.**

Union of the observed sets: T1 `{registered_after, survives_revocation, interleaved}`, T2
`{revoked_cannot_clear, survives_revocation}`, T3 `{fingerprint_mismatch}` — **five of seven**.

Not covered: `a_clearance_by_an_identity_never_registered_is_refused` and
`a_refused_clearance_is_recorded_and_the_log_still_replays`.

The first is fine and declared: **#201 itself calls it "the control, not the evidence."** The second is
not declared anywhere I found, and **no mutation in this matrix can turn it red** — so this matrix
does not distinguish it from decoration. That is not a claim it is decoration: it asserts that a
refusal is journal data rather than `Corrupt`, and **the mutation that would test it (make a refusal
`Corrupt`) is a dimension the matrix does not enter.** Naming that boundary costs a sentence and stops
the matrix from being read as covering all seven.

**F2 — the seven panic sites are published without a base, and against this PR's own head they are
not assertions.**

Resolved against `deff9dd`, `core/events/tests/execution_projection.rs`:

```
:1769  let mut batch = parked_batch();
:1830  fn a_clearance_survives_the_later_revocation_of_its_signer() {
:1937  ///
```

A `let`, a function signature, a doc comment. **This is expected** — the reds were observed before the
fold landed and the file has moved since. **The defect is that the numbers travel with no ref**, so a
reader checking them today finds nothing and cannot tell an obsolete coordinate from a wrong one.

**And this branch already contains a commit that says so**: `c4542b0`, *"docs(201): the numbers above
describe a base that exists nowhere."* **The author found this; the PR body did not get the
correction.** The body is the highest-traffic carrier — it is what whoever merges reads. One clause
fixes it: name the commit the reds were observed at (`60f6468` looks like the candidate, *"the seven
finally fail where they should"*).

**F3 — roughly a third of the diff is carried, not written, and the body does not say so.**

Measured against `d0b3f04..deff9dd`:

| | lines |
|---|---|
| `.factory/gate-runs/*.json` (two run manifests) | **1,997** |
| `schemas/` incl. duplicated `releases/1.0.0/` copies | ~858 |
| `tools/ci-canary/src/nonce.rs` | 10 |

Of `+6453`, about **2,000 lines are gate-run records**, and one of them
(`c449d9b128dc-20260820T144732Z.json`) is the manifest **#199 cites as belonging to lane
`issue-160`**. The nonce's own comment says *"the value itself carries no meaning beyond `this file
changed this run`"* — **it is churn, and it will conflict with every other branch that ran the gate.**

The body's only nod to scope is a subordinate clause in Verification (*"after merging lane 1's
`b81f546`"*). **#201 lists that schema work as a BLOCKER landing separately; here it is inside.** Each
of these may be the right call — **but a reader cannot tell a decision from a drift unless the
decision is written**, and at 31 files a reader will not reconstruct it.

## What passed, and two items are better than the standard asks for

- **The declared limit is kept.** The body says durations are not citable and the maximum claim is
  *"this suite, this dir", not a gate* — and no duration and no gate claim appears anywhere else in
  it. It also names *which* oracle died: overlapping builds destroy the **clock** oracle and leave the
  **result** oracle intact. **Declaring a limit and then honouring it three sections later is rarer
  than declaring one.**
- **T2 refuted the seal and the refutation is published, with its cause named as a false claim in the
  author's own doc comment.** A sealed prediction that survives contact unchanged tells you little;
  **one that is refuted, and whose refutation is published rather than quietly re-sealed, is the only
  evidence the seal was real.** The generalisation drawn from it is correct and reusable: **a
  precondition widens a guard's failure surface beyond the property it is named after** — so claiming
  a guard is blind to a mutation means reading every assertion in it, not the headline one.
- **A production branch no guard could turn red, found by sabotage**: inverting the `MachineReplay`
  arm changed **zero** tests. That is the exact mirror of a guard that has never been red, and it got
  a dedicated cell written red-first — **explicitly not delegated to a test that exercises the arm
  incidentally**, because incidental coverage disappears the day that test changes for its own
  reasons. Measured one mutation at a time: zero before, exactly one after.
- **Seven distinct panic sites rather than a count**, with the right reason stated — distinctness is
  the control that the seven were not sharing one failure upstream at `append_atomic`.

## What I verified, and what that is not

`origin/main` `d0b3f04` measured directly: the claim/clearance machinery is **absent** there —
`open_claims`, `CompletionClaimed`, `CompletionCleared` all zero, by two methods, with `pub` → 92 in
the same file as the positive control. **So #201's coordinates (`projection.rs:1153`, `:1195`) cite a
tree that is not `main`**, which is worth a word in the issue: against `main` this PR does not add
validation to existing machinery, it brings the machinery.

**I executed nothing** — no tests, no gate, no mutation run. The matrix is re-runnable because
`.factory/bin/j-201-mutate.py` is committed, **which is the right call and is what makes the table
checkable rather than reported**; I did not run it. The author's verification claims are not
reproduced here and I do not contest them.

## Verdict

**Nothing here asks for the approach to change**, and the sabotage work is above the bar #201 set.

**F2 is one clause** and matters most, because it is a correction the author already made that did not
reach the body. **F1 is one sentence** naming the dimension the matrix does not enter. **F3 is a scope
paragraph** — or dropping the nonce and the foreign manifest, if they are drift rather than decision.
None of the three blocks.
