# #217 slice 1 — sabotage matrix

The five guards passed the moment they were written, because I authored both sides of every
comparison. **That is "green having never been red", and it is worth nothing until each cell has
been seen to fail on its own assertion.** Subject committed at `0463b21` before any mutation, because
the matrix resets with `git checkout` and an uncommitted subject is one the reset eats.

| id | mutation | guard reddened | others |
|----|----------|----------------|--------|
| S1 | **rename** one refusal code in the Rust match only | `the_refusal_vocabulary_is_one_set_on_both_sides` | all green |
| S2 | rename one kind in the schema only | `the_kind_vocabulary_is_one_set_on_both_sides` | all green |
| S3 | drop `producer` from `artifactBinding.required` | `a_binding_cannot_be_partially_specified` | all green |
| S4 | `additionalProperties: true` on the envelope | `the_envelope_shape_matches_the_schema` | all green |
| S5 | unparseable version defaults to the current major | `an_unknown_major_is_refused_and_an_unparseable_version_is_not_defaulted` | all green |

**Five distinct single-guard failure sets.** No cell is satisfiable by a neighbour, which is the
decomposition the blueprint promised and the reason each of these is a separate test rather than one
"the contract is valid" assertion.

## S1 is the row that proves D1 was load-bearing

S1 is deliberately a **rename**, not a removal. The refusal vocabulary keeps exactly ten members on
both sides; only one member's spelling moves. **A count comparison stays green through it.** That is
the divergence the design review named, and the reason the guard asserts set equality and prints
both differences instead of a length.

## One row was harness-broke before it was data

S4's first attempt reported `ANCHOR FAILED` — the mutation never applied, so the run measured the
unmutated subject. Under the rule this repository already carries, a cell whose mutation did not
apply is **harness-broke, not a pass**, and it was re-run rather than read.

The cause was the fifth instance today of the same trap: the schema file is stored **CRLF**, the
multi-line anchor was written with `\n`, and the match silently returned zero. The repair is the one
that keeps working — anchor on a **single unique line** located by index, with the file's own line
terminator preserved. Anchors spanning newlines keep failing on this repository and a generic
delimiter is not an anchor, it is luck.

Restored after the matrix: **5 passed, 0 failed.**

# Slice 2 — the five binding cells, proven one at a time

Subject committed at `3da9c91` before any mutation. Each check in `verify_binding` was disabled in
turn and the suite run.

| check disabled | cell reddened | others |
|---|---|---|
| `scope` | `a_scope_mismatch_is_refused_under_its_own_code` | 15 green |
| `schema_id` / `schema_version` | `a_schema_mismatch_is_refused_under_its_own_code` | 15 green |
| `producer` | `a_producer_mismatch_is_refused_under_its_own_code` | 15 green |
| `digest` | `a_digest_mismatch_is_refused_under_its_own_code` | 15 green |
| `snapshots` | `a_snapshot_mismatch_is_refused_under_its_own_code` | 15 green |

**Five checks, five cells, five distinct single-test failures.** This is the decomposition the design
review asked for and the reason `verify_binding` returns five distinct refusal codes rather than one:
with a single code, one guard asserting "the binding was rejected" would stay green while four of the
five checks were gone.

The fifth was run separately rather than skipped. Its condition is compound
(`schema_id || schema_version`), so the loop's anchor did not match it, and a cell left unproven
because the harness could not reach it is exactly the thing this file exists to prevent.

**The control belongs in this table too, in spirit:** `an_identical_binding_verifies`. Without it,
every one of the five above is satisfied by a verifier that refuses everything.

## What is deliberately NOT claimed here

`canonical_output_is_identical_across_input_key_order` is **not** in the table because **no mutation
of `canonical_json` can redden it.** This workspace builds `serde_json` without `preserve_order`, so
its object map is a `BTreeMap` and parsing sorts keys before that function is reached — measured
directly, with two differently-ordered documents serialising identically. The property holds today
because of the dependency, not because of this code. The cell is a regression net against a future
`preserve_order`, and calling it evidence of implemented behaviour would be the un-reddenable-branch
defect with the sign flipped.

# Slice 3 — the compatible-minor pair, and an honest limit in my own decomposition

Subject committed at `fb3fb89` first.

| id | mutation | cells reddened |
|----|----------|----------------|
| S8 | drop the capture map (permit unknown fields but do not keep them) | **both** — preservation AND non-authority |
| S9 | let the captured map into `digest_input` | **non-authority alone** |

**S9 isolates cleanly. S8 does not, and the reason is in my own test.**

The non-authority cell opens with a precondition — *"the extra field really was captured, or this
measures nothing"*. S8 destroys capture, so that precondition fires and the cell goes red **as an
ARRANGEMENT, not as a MEASUREMENT**. Its property claim (a preserved field may not move the digest)
is untouched by S8; only its setup is.

**So the pair is not fully independent, and saying so is the point.** A precondition widens a
guard's failure surface beyond the property the guard is named after — measured on this repository
before, in a cell whose doc claimed it stayed green under a mutation that in fact broke its
precondition. **Claiming "two mutations, two isolated cells" here would repeat that error.** What is
true: **non-authority has a mutation that reddens it alone; preservation does not, because every
mutation that breaks preservation also breaks the other cell's arrangement.**

That is a weaker result than a clean 2x2 and it is the accurate one.

## Slice 3b — the cure was DISTINGUISHABILITY, not independence

The consuming lane's reviewer returned the better half of my own concession. What the pair was
missing is not independence — it is a **third state**. Marked so, the precondition reports that the
arrangement never assembled, instead of looking like a property failure:

```
S8 (capture dropped)          preservation FAILED
                              non-authority -> "HARNESS-BROKE: the arrangement did not assemble"
S9 (capture into the digest)  preservation green
                              non-authority -> "an unknown field is preserved but carries no
                                                authority: it may not move the digest"
```

**Two mutations, two readable signatures.** The table is legible again, not because the cells became
independent but because *"I could not measure"* stopped being printed the same way as *"I measured
and it passed"*. Same root as an exit code that cannot separate **ran and failed** from **never
started**.

**The limitation is NOT repaired and stays written:** there is still **no mutation that reddens
preservation alone**. Every mutation that breaks preservation also breaks the other cell's
arrangement. What this buys is diagnosis, not independence, and recording it as anything better
would be the optimism this file exists to refuse.
