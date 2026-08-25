# The membership check: four tests, four reds, each alone

`ci/membership.sh` (or `.factory/bin/`) gates every vocabulary merge. It passed the moment it was
written, because I wrote both the check and the thing it checked. **That is "green having never been
red" and it is worth nothing** until each test has been seen to fail on its own case.

Three of the four had never been red when the instrument was first used on a real merge. This is
the matrix that fixed that.

## The tests, and why each exists

| test | question it answers |
|---|---|
| `comm -23` LOST | did anything main had go missing in the merge? |
| `comm -13` GAINED | is what appeared exactly what the PR declared? |
| `uniq -d` DUPS | is any member present twice? |
| PREFIX | is main's list an **unbroken head** of the result, **in order**? |
| extraction control | did the extractor read anything at all? |

The four are not redundant. **Each has a case only it catches**, which is the property that stops one
of them being removed later by someone who believes another covers it.

## The matrix

| id | mutation | red | the others |
|----|----------|-----|------------|
| V1 | none — the real merge `591dac7` → `0754cda` | **PASS** (the control: it must be able to say yes) | — |
| V2 | real loss: base `0754cda`, result `591dac7` (20 → 18) | **LOST**, naming both codes | PREFIX also fires — a loss is also a prefix break |
| V3 | **reorder**: swap main's first two members, nothing else | **PREFIX alone** | LOST `none`, GAINED as expected — **the set tests PASS** |
| V4 | **duplicate**: one member repeated at the end | **DUPS alone** | LOST passes, PREFIX passes |
| V5 | nonexistent ref | **extraction control**, refuses and says why | — |

## V3 is the row that justifies the prefix test

```
SET  (comm -23 / -13):  lost=none   gained=the two expected   -> WOULD PASS
PREFIX               :  FAILS
```

**A pure reorder passes both halves of a set-based check.** Sets are unordered; append-only is a
claim about ORDER. A set test guarding an order claim prices the wrong unit, and is silent in
exactly the dimension the claim lives in. The blindness was named by D on #267 — *"a reorder is
invisible here and a renumbering downstream"* — put into numbers by B on #273, and verified
independently here. **I did not find it myself, and the record should say so.**

## V4 has a real historical fixture rather than a synthetic one

The duplicate synthesised for V4 landed on `code_rule_waiver_invalid` — **the same member a real
merge resolution duplicated**, when both sides of a conflict hunk were pasted together. Valid JSON,
valid schema, a 19-member list silently becoming 20. A duplicate in a closed vocabulary is not
untidiness: **it makes the equality guard's subject ambiguous**, so the guard downstream stops
meaning what it says.

## What this instrument deliberately does NOT do

- **It does not use the Rust ↔ schema equality guard as a bridge.** That guard goes green in the
  exact failure mode this check exists to catch: when a merge drops main's codes, *both halves lose
  them together* and the sets stay equal. A guard cannot be the bridge of the verification that
  exists because it is blind.
- **It does not assert a count.** The invariant is the empty `comm`, never a number. Two lanes
  landing between measurements make any fixed target wrong, and a right number reached by a wrong
  composition leaves no trace.
- **It is scoped to the `DevelopmentRefusalCode` block, not the file.** That module carries three
  vocabularies; an unscoped `=> "..."` sweep returns 34 arms and would let a `DevelopmentKind` name
  satisfy a refusal-code check. (Author's finding about their own instrument, on #273.)
- **It names the base and expires.** A check that does not name its base is worse than none, because
  it looks like coverage.

# The manifest subject — the same four tests, a different extraction

The instrument has TWO subjects and the wrong one returns a true PASS about something the PR never
touched. **Choose by measuring what changed**, not by what the last task used:

```
git diff --name-only <main>..<head> -- <the files of my subject>     # empty -> wrong instrument
```

| subject | extraction |
|---|---|
| refusal vocabulary | the `DevelopmentRefusalCode` block in `core/protocols/src/development.rs`, and `$defs/refusalCode/enum` in the package's envelope schema |
| manifest | `spec/contracts/contributions` in the package's `extension.json`, keyed on `id`, plus `path` and case-folded `path` |

Scoping matters on the Rust side: that module carries three vocabularies and an unscoped
`=> "..."` sweep returns 34 arms, which would let a `DevelopmentKind` name satisfy a refusal-code
check. (Failure mode found by B on their own instrument, #273.)

The manifest extraction was reddened on all four tests before first use: R1 loss, R2 reorder
(**PREFIX alone** — both `comm` directions pass a pure reorder), R3 duplicate (**DUPS alone**), R4
empty `contributions` (extractor **refuses** rather than comparing nothing).

## What a PASS from this instrument does NOT say

Membership and order. **Not** that each declared file exists, and **not** that any `sha256` matches
its file — a PASS here is compatible with a stale digest, which belongs to the inventory guard that
re-hashes each declared path independently. Saying so on every report is deliberate: a green check
under a heading wider than the check is how evidence that is correct produces a conclusion that is
false.

## A fifth form, and why it is recorded rather than left in the code

**Stale base**: editing against 37 entries while main carries 43 drops six, and `validate` stays
green. Raised by N on #295. Measured against this instrument on that PR's real data rather than
assumed either way — building the case as 37 of main's entries plus the 15 new ones:

```
LOST                                    names all six
PREFIX as implemented here              ALSO FAILS - it diffs main's FULL list against the head,
                                        so a hole inside the head breaks it
"surviving entries appear in order"     OK - BLIND, with six missing
```

The concern was right about a **formulation** and not about the one implemented, and the pair covers
it twice. **The finding is the name.** "Prefix test" denotes two different properties; the safe one
is a non-obvious detail of how the comparison is written, and a later simplification into the weak
reading would keep the name, keep the green, and drop the property.

That is why it is written here and in the script header rather than left to be inferred from the
code. **A property that depends on a non-obvious implementation detail is a property waiting to be
refactored out.**
