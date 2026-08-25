## What and why

The `apps/cli/tests/` slice of #314's collapsed-indentation class, swept **by role**, plus one precondition on the negative fixture main repaired in #368 -- all that survives of L's #324 follow-up, for reasons set out below.

Part of #314

## The sweep: 13 corrected, 6 deliberately not

Thirteen runs of whitespace inside assertion messages across six files. A continued literal keeps the next line's indentation inside it, so a human reading a failure sees `the verdict flaps between          wake-up`.

**Six occurrences were left alone, and the reasoning is the whole point of a role-aware pass:**

| left alone | why |
|---|---|
| `schema_cli.rs:1298` | a JSON document written to disk whose **formatting is the test** |
| `source_invariants.rs:80` | the predicate's own `"   "` threshold, not prose |
| `source_invariants.rs` ×4 fixtures | their runs of spaces **are** the thing under test |

**The `schema_cli.rs` case is worth spelling out, because its damage would have been silent.** That test writes two files — one compact, one formatted — and asserts their digests match, proving the digest is invariant under formatting. Collapsing the spaces would **not** have made it fail: the digest is canonical, so it ignores whitespace either way. The test would have stayed green while the "formatted" file stopped being formatted, and the property it exists to prove would have quietly ceased to be exercised.

That is the false-positive class at its worst. #314 warns about a sweep breaking a corpus loudly; this one would have gutted a test without a single red.

The replacement only ever touches the odd-index segments of the quote split — literal bodies — which is the same distinction the guard's predicate makes, and the reason that predicate needed fixing once already.

## A false defect caught before it was reported

One line appeared to carry mojibake (`â€”` where an em dash belongs). Decoded from the file's bytes it reads correctly, and the corrupt byte sequence is absent: the terminal was rendering UTF-8 as cp1252. **The view was mangling, not the file.** Had I trusted the window I would have reported a defect that does not exist and "fixed" a file that was already right.

## L's follow-up, and what the rebase took back

An earlier revision of this PR claimed three preconditions plus a corrected doc-comment fixture. **Most of that claim is withdrawn**, because main got there first or got there better, and the honest resolution was to take main's file and keep only what is genuinely missing.

| claimed before | status now |
|---|---|
| two preconditions on the code-case fixtures | **dropped.** Main's `fixture.contains("   ")` is simpler and equivalent in effect: lose the run and it fails; move the run *inside* a literal and `offends` turns true, so `!offends` fails. The pair is tight in both directions without a stronger predicate. Mine was longer, not better. |
| correcting the comment fixture that passed for two reasons | **dropped.** D found the same defect independently, by sabotaging the exemption and watching nothing go red, and his fixture landed first (#368). |
| a precondition on that corrected fixture | **kept — nothing on main does it.** |

**What survives is one assertion, and it guards a repair rather than a fixture this branch wrote.** D fixed the fixture; nothing asserted it *stays* fixed. Tidy the run out of its inner literal and `has_run_in_literal` answers false again, the exemption stops being reached, and `!offends(commented)` passes for exactly the reason the repair removed — silently, with the explanatory comment still sitting above it saying that cannot happen.

**It composes `has_run_in_literal` rather than re-splitting on quotes**, and that is not only hygiene against a fourth copy. A precondition that re-implements the predicate it validates inherits that predicate's blind spots by construction: mine would have carried the escaped-quote bug the shared version was cured of, where in `let s = "a \" b   c";` a naive split flips the parity and reads a run *inside* a literal as being outside it. A second opinion assembled from the first opinion's parts is not a second opinion.

The assertion below it now uses the binding instead of repeating the literal, so the precondition and the assertion cannot come to guard different strings.

**Sabotage-verified, from a committed tree.** With the run tidied out of the comment fixture's inner literal, `git diff --numstat` printed `1 1` — proof the subject actually changed — and the precondition fired with its own message rather than the negative assertion. A sabotage that never lands reports the same green as a guard that survived one, and this lane nearly recorded that green as evidence once.

**The 13-message sweep is untouched by all of this** and still needed: all thirteen occurrences were measured file by file as still present on main before the merge.

## Declared gap: `tests/` has no guard

`apps/cli/tests/source_invariants.rs` walks **`src/` only**, so this sweep leaves `tests/` at zero occurrences and **nothing stops the class returning there**. #314's own decisive datum is that a sweep alone did not hold — its author re-added the defect hours after reporting it.

Extending the walk to `tests/` is the durable fix, and it needs design rather than a one-line change: the guard's own fixtures and the `schema_cli.rs` JSON would both be flagged, so it requires a **role-based** exemption (never per-file, which would leave a whole file unguarded) plus a guard on the exemption itself — remove the exemption and the guard must fail at exactly the data lines, proving the exemption carries weight rather than being decoration.

Not done here because it was not the assignment and it is a scope call, not a detail. Raised rather than left for someone to notice.

## Validation evidence

Base is `origin/main` at `051e3ba`; the branch is the merge result `e95fa5f`.

```
cargo check --workspace --all-targets --locked   exit 0
cargo fmt -p graphhelm-cli -- --check            exit 0
cargo clippy -p graphhelm-cli -D warnings        0 errors

source_invariants  3     amend_budget      6     attention_tag_domain          5
mcp_stdio         19     schema_cli       29     development_contract_schemas 27
                                                        total 89 passed, 0 failed
```

`source_invariants` reads 3 rather than the 4 an earlier revision reported: that count is main's, not a loss — this branch no longer adds a test of its own there, only an assertion inside an existing one.

`schema_cli` passing at 29 includes the digest-invariance case whose fixture this PR deliberately did not touch.

The sweep fingerprints the tree before and after and reports contamination itself; this run reported it unmoved at `e9912a31bc702427`.

**One red was met and not fixed, on purpose.** At the intermediate base `8f5b353`, `mcp_stdio` failed with 18 passed, 1 failed: the tool list had grown to 17 while the test still named sixteen. It is not this branch's — the only line this branch changes in that file is an assertion message — and it was already cured upstream, where the array and the test both read eighteen. The cure was to merge the newer main, not to write a second correction that would collide with the first.

**The change set is the merge-base diff, not `origin/main..HEAD`.** Main moved again during verification, and against the moved main this branch appears to delete 185 lines across three `core/*` `source_invariants.rs` files it never opened. The exonerating control is direct: `git log 051e3ba..HEAD -- <each file>` returns empty for all three. The real change set is seven test files under `apps/cli` plus this document — one crate, so M06 holds.

## Note for the twin

`tools/pathogens/tests/source_invariants.rs` holds a copy of the same machinery. **This branch must not touch it** — that crate is gate machinery and `apps/cli` is not, so M06 forbids one branch touching both. The precondition kept here belongs there too, and the twin-copy comment in both files already says that a duplicated oracle diverges in silence. Whether its fixtures carry the same gap is unmeasured from here and should be measured, not assumed, by whoever takes that crate.

## Rollback

Revert. The thirteen messages get their embedded runs back, and D's corrected comment fixture returns to being unguarded — repaired, but with nothing asserting it stays repaired.
