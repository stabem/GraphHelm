## What and why

The second half of #222: budget refusal, citation verification with its own vocabulary, cold and amortized index cost kept apart, delta provenance, and the section authority the compiler had been asserting in a comment.

Closes #222
Closes #269

## Eight red-first cycles

Every guard names, in the source above the test, **the production change that would make it fail**, written before the test. Each red was observed landing on its own assertion; a compile error was treated as an error and fixed until the failure was an assertion.

| # | guard | the red that was observed |
|---|---|---|
| 1 | required context over budget refuses instead of trimming | the trimming stub returned a SUCCESS |
| 2 | citations report two lists, not one count | — (written green, with the discriminating case) |
| 3 | the refusal vocabulary is one set on both sides | **task-001's own guard**: `only in the Rust type: ["citation_unresolved", "required_citation_missing"]` |
| 4 | the compiler's section set is the schema's section set | `only in the schema: ["projectKernel", "task", "node", "evidence", "dependencyOutputs", "agentExperience"]` |
| 5 | both citation failures emit both codes | `it emitted []` — the codes were allocated and nothing emitted them |
| 6 | every citation case produces the codes it declares | four sabotage arms, table below |
| 7 | an amortized share is derived, not measured | `It names Some("index") as its observer` |
| 8 | a base rewritten under a delta is refused as such | the identity-only stub accepted it |

## What each half is protecting against

**Budget.** Optional context is droppable and its drops are counted; required context is not droppable at any budget. An implementation that trims required items to fit **succeeds**: it returns a capsule, under budget, with every number healthy, and without the evidence the caller was required to see. The refusal carries the allocated code *and* an expansion request naming a budget that would fit — refusing without saying how much more is needed leaves the operator with no move.

**Citations, and why two codes.** A required item nobody cited and a citation the capsule cannot resolve are different causes with **opposite remedies**: cite the evidence, versus stop citing something that does not exist. Amendment 4 records the allocation and the reasoning; the names are deliberately asymmetric, because a symmetric pair reads better and is usually a sign the names were derived from each other rather than from the two failures.

Both codes are **emitted**, not only allocated. Allocation without an emitter is the shape L named on #95: the set-equality guard is green, schema and type agree, and no run can ever produce the refusal. Having closed exactly that for the budget code in cycle 1, this change was reopening it twice.

**Index cost.** Cold is what this run paid to make an index usable, and somebody watched it happen. An amortized share is arithmetic over a cost paid earlier by a run that is not this one. Summed, the result is neither: too large to be what this run paid, too small to be what the index cost. So they are two receipt lines, an amortized share is `derived` and never `measured`, and a zero denominator is refused at construction — a share over zero runs is not small, it is undefined.

**Delta provenance.** A delta read alone looks exactly like a small complete capsule. Three refusals rather than one "base mismatch": a different capsule is a routing mistake, a different version may still be recomputable, and the third — same identity, different bytes — means the base was **rewritten underneath the delta**, which no identity check can see. Identity is checked before the digest, because a digest mismatch is also what a completely different capsule produces: check it first and every routing mistake is reported as a rewrite.

**Section authority (J's finding, #269).** `DECLARED_SECTION_ORDER` and the capsule schema agreed with nothing linking them, and the constant's doc claimed an authority nothing verified. Measuring changed the remedy: the schema names its sections in two places, `sections.required` (an array) and `sections.properties` (an object), and JSON Schema reads **both as sets**. There is no ordered authority in that document to defer to — and binding emitted bytes to a file's key order would turn a semantically empty reorder into a different digest for every capsule ever compiled, hence every cache key. The schema owns the set, the constant owns the order, and a third defect neither of us had is now guarded: the two declarations can diverge **from each other**, one direction making a section silently optional and the other making the document unsatisfiable.

## The citation cases, sabotaged

The walker was born green. The cases are runtime data, so no rebuild sits between arms:

| arm | perturbation | result |
|---|---|---|
| 0 | untouched — the control the others must differ from | `2 passed` |
| 1 | the scope-bleed case declares one of its two codes | red, naming both sides |
| 2 | the cited item is moved **into** the capsule under test | red — the verifier produced `[]` |
| 3 | the case directory is absent | red, both tests, on the sweep |

Arm 2 is the one that matters: the two capsules hold an item with the same section and the same text, so moving the citation across makes the verifier accept. That is the proof the case's red comes from cross-capsule identity and not from the text differing — L's gate 3 names exactly this shape as the one a naive check passes.

## Scope

Amendments **4** (two citation refusal codes) and **5** (the citation-case schema and its walker) belong to this PR, and both are in #222's body. Amendment 5 took a detour worth recording: publishing it was refused by this session's permission policy after amendment 4 had gone through, the refusal was not worked around, and the orchestrator published the text from the committed file at `.factory/b-agent-222-scope-amendment-5.md`.

The manifest is append-only: **10 → 14** contributions, 52 insertions and 0 deletions, every new entry declaring its own file with a digest recomputed from disk.

## What this does NOT establish, stated rather than implied

- **No wire refusal code was allocated for the three delta-base causes.** They are typed refusals inside the crate. A wire code would need a further scope amendment, and this session cannot publish one. Deferred deliberately, not overlooked.
- **The header invariant and the section-set guard were sabotage-verified; the delta positive case was not.** It is the positive control for three negatives, and its own red twin does not exist.
- **Nothing in this surface has a production caller — measured, not assumed.** Raised by J while auditing the chain behind one of my own arguments, and it is wider than the field they started on. Every public entry point added by #222 across both PRs has **zero** call sites outside its defining module and the tests:

  ```
  cache_key(  ContextCacheKeyInputs  compile_capsule(  fit_within_budget(
  verify_citations(  verify_delta_base(  item_id(  IndexCost::
  → 0 sites each, in core/ apps/ adapters/, excluding /tests/ and the two defining files
  ```

  Positive control on that zero, since a filter that erases everything reports the same number as a codebase that never mentions these: without the exclusions the same command finds **16** sites for `compile_capsule(` and **7** for `verify_citations(`. The instrument sees them; the zero is about the population meant.

  This dates the guarantees rather than removing them. There is no live cache to poison and no capsule being compiled today, so every failure these guards block is **prospective** — which is the argument for writing the constraints now, while they cost a doc line, instead of after someone wires the type and the coupling has to be re-derived by whoever inherits it.

## Security review

No new authority, no new surface. Types, pure derivations, one schema, three fixtures, and tests. No network, no filesystem writes outside the package, no credentials, no process spawning, no dependency added.

The two refusal codes are append-only inside a closed vocabulary the schema owns and a set-equality guard enforces on both sides — and they travel in the development envelope, which is where `#216`'s jurisdiction rule (merged as `ed7e57b`) says they belong.

Citation spoofing is now covered in two places rather than one: the unit guards and a package fixture whose cited ID is a **real** ID of a **real** item in another capsule.

## Rollback

Revert the commits. Everything added is additive and nothing else calls it yet; removing it removes the guards with it, and the state returned to is the one where required evidence can be trimmed to fit a budget, an amortized cost can sign as measured, and a delta can be applied to a base that was rewritten underneath it.

## Validation evidence

**ED-18, on the merge result rather than on the branch alone.** `origin/main` at `ed7e57b` is merged into this branch as `3b042cd`; every number below was taken on the head commit that carries it, and each names its own commit rather than borrowing another's.

Three tasks in this wave declare `core/runtime/src/lib.rs` and `Cargo.toml`, and `core/protocols/src/development.rs` moved on main while this branch was open — main added the vocabulary jurisdiction doc block (`ed7e57b`), this branch appended two enum variants. Different regions, merged without conflict, and the combination exists in full only here.

```
cargo check --workspace --all-targets --locked                            -> exit 0
cargo clippy -p graphhelm-runtime   --all-targets --locked -- -D warnings -> exit 0
cargo clippy -p graphhelm-protocols --all-targets --locked -- -D warnings -> exit 0
cargo clippy -p graphhelm-cli       --all-targets --locked -- -D warnings -> exit 0
```

Clippy is here because of J's lint amendment to ED-18, merged during this lane: `cargo check` does not run clippy, so a merge can land a red stage the check cannot see. It caught two reds across this lane's merge results, and the record is worth keeping even though both are now closed.

**The first was mine, and this PR fixes it.** `origin/main` was clippy-red for `graphhelm-runtime` from PR #253's hand-written `Vec<(&'static str, fn(&mut ContextCacheKeyInputs))>`. Named type alias, gone.

**The second was inherited and this PR deliberately did not fix it** — one cause, `core/runtime/src/retrieval.rs:161`, in a file that arrived with a merge and was byte-identical to main. Collapsing another lane's `if` would have removed the signal their own gate should show them, so it was reported rather than patched. Its owner closed it on main in `f4c9501`, which is why the four lines above are clean.

**Tests, all seven binaries this change can affect, on the same commit:**

```
graphhelm-runtime   --test context_compiler              20 passed
graphhelm-runtime   --test context_accounting            13 passed
graphhelm-cli       --test development_contract_schemas  27 passed
graphhelm-cli       --test development_package_inventory  1 passed
graphhelm-cli       --test context_citation_fixtures      2 passed
graphhelm-protocols --lib                                 1 passed
graphhelm-schema    --lib                                51 passed
                                                  total 115 passed, 0 failed
```

**What was NOT run, stated rather than implied:** the full `graphhelm-cli` suite. Its integration tests stand up servers and exceeded a ten-minute budget in this session's contention, so the three binaries this change touches were run instead. `--all-targets` type-checked everything and clippy linted all three crates; execution coverage outside those seven binaries is not claimed.

**Instrumentation notes, because two readings in this session are not what they look like.** One run was killed by a ten-minute tool timeout with no verdict read, and one sweep was killed by me to free the target-dir lock; both windows are closed in `check-activity.log` as *timeout/killed without a verdict*, never as a pass. And one earlier reading of the citation walker is recorded as **undefined rather than green**: the fixtures were being written while that run was in flight, so what its case count described depends on when `read_dir` happened. The green in the table above was taken after the tree stopped moving.
