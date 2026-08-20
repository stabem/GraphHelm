# D — defect I introduced in #101, now on main: `parallel_limit` was inserted INSIDE `dispatch_plan`'s doc block

Base: `origin/main` @ `9f3ee1e` (the squash of #136). Parent for comparison: `cf2d417`.
Docs only — **no behaviour change, no runtime effect.** Found while drafting L's requested clause.

## What is on main

`core/execution/src/dispatch.rs`, lines 19-44: the doc block that reads *"Selects which ready nodes
to dispatch now"* — including its `# Errors` section naming `ZeroParallelism` — now sits above
`pub fn parallel_limit`, and `pub fn dispatch_plan` at line 46 has **no doc comment at all**.

Verified against the parent, so this is not a pre-existing condition: at `cf2d417` that same block
was lines 19-27 and was immediately followed by `pub fn dispatch_plan` at line 28. **#101 inserted
`parallel_limit` between a doc block and the function it documented.**

Three consequences, all mine:

1. **`dispatch_plan` lost its documentation** — including the recorded decision I *quoted in the
   PR body* as the reason not to fold the conversion into it. The authority I cited for my own
   design choice is the text I detached.
2. **`parallel_limit` carries a `# Errors` section for an error it cannot return.** It returns
   `usize`. `ZeroParallelism` is `dispatch_plan`'s.
3. **My intended summary line lost its head.** Line 28 reads *"`GraphBudgets::max_parallel_model_calls`
   as the `max_parallel` [`dispatch_plan`] wants."* — the tail of "Converts ... into the ...", now
   stranded inside another function's `# Errors` list.

## Why nothing caught it — the gate is not broken, it prices a different unit

The gate runs `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
(`ci/gate.ps1`, the clippy stage — line 129 on `origin/main`; see the base note below). That is the **default** lint set:

- `clippy::missing_errors_doc` — would flag a `Result`-returning public fn with no `# Errors`, which
  is exactly what `dispatch_plan` became — is in **pedantic**, not enabled.
- `missing_docs` — rustc, **allow by default**, and no crate-level attribute turns it on
  (`core/execution/src/lib.rs` has no lint attrs; no `[lints]` table in the manifests).

So 27 green stages, a reviewer who re-derived the identity by hand, and a merge all passed over it,
correctly. Doc *attachment* is not a unit anything here measures. Nothing to blame; worth naming.

## The fix — two things, one closing criterion

Both are "the surviving symbol carries the right words", so they belong in one change.

**(a) Give each function its own doc back.** Restore `cf2d417`'s block verbatim to `dispatch_plan`;
give `parallel_limit` a correct summary line and no `# Errors`.

**(b) L's clause, requested in review of #136 and adopted whole.** In `parallel_limit`'s doc:

    /// A LIMIT, NOT A MECHANISM — and the two drivers do not agree on the mechanism. This
    /// answers "how many may be in flight", never "does anything actually run at the same time".
    /// `core/runtime/src/driver.rs` spawns the plan into a `JoinSet` and runs nodes concurrently.
    /// `apps/cli/src/commands/execution/driver.rs` walks the plan in a `for` loop and blocks on
    /// each executor call, so the same limit only widens how many nodes one SEQUENTIAL pass may
    /// cover. Written here because #101's dedup removed the signal that used to carry it: two
    /// identical copies accidentally marked "two drivers, check both", and one shared function
    /// reads as unification. The policy is unified. The parallelism is not.

L's general lesson, which is the reason the clause belongs at this site rather than in a note:
**deduplication is not free — it removes the duplicate's signalling value.** When the copies go, the
reason they were plural has to be written into the survivor, or the next reader infers a unification
that does not exist. That is the exact inverse of the argument that won #103, and both hold at once:
one implementation for the POLICY, one sentence for the MECHANISM that is still double.

## Guard question, answered honestly

No test is proposed. A doc-attachment guard would be a source-scraping test, fragile by name — the
same objection L raised, correctly, against a `grep`-for-`fn is_terminal` test. The durable options
are lint-level (`missing_docs` at crate level, or `clippy::pedantic`), which are workspace-wide
policy decisions far larger than this defect and should not ride in on it. **Recorded as a named
structural gap: doc attachment is unmeasured here, and this change does not close it.**

## Addendum: no `cargo doc` stage — but rustdoc DOES run, and the gap is enforcement

Read from `ci/gate.ps1` while checking what the fix must survive. The stages are: `rustfmt`,
`clippy (deny warnings)`, `workspace tests`, the CLI suites, `schema catalog` / `baseline
compatibility` / `conformance`, `locked metadata`, `whitespace`, and two PostgreSQL matrices.

**CORRECTED after L's refinement — the first version of this addendum said rustdoc never runs, and
that was wrong in the way that changes the fix.** Two facts, each verified separately:

- **No `cargo doc` stage, and no `RUSTDOCFLAGS` anywhere.** `grep -cE "cargo doc|rustdoc|RUSTDOCFLAGS"
  on `ci/gate.ps1` returns **0**.
- **But rustdoc executes on every run anyway.** `cargo test` invokes it to extract and compile
  doctests: the reference green log (`265809a:.factory/d-agent-80-results/17-gate-fixed-instrument.txt`)
  contains **57 `Doc-tests <crate>` lines**. Counted from the artefact, not taken from the report
  that cited it.

So the gap is **not absence, it is enforcement** — rustdoc runs without `-D warnings`, so any
rustdoc lint it does emit lands in the log and fails nothing. That changes the cheap fix from "add
a stage" (cost: a full workspace doc build) to "deny warnings on the step that already runs"
(`RUSTDOCFLAGS=-D warnings` in the test stage's environment; no new stage, no extra machine time).

**STILL UNVERIFIED, and stated as such rather than assumed in either direction:** whether doctest
extraction emits `broken_intra_doc_links` on this toolchain. Verified that rustdoc *executes*; not
that this particular lint fires during extraction. Decidable in one line by someone holding a slot:
break a link deliberately and see whether the log gains a line. Until then, "a broken intra-doc link
cannot fail this gate" is established; "it passes silently" is not.

The trap worth naming before someone walks into it: **the obvious response ("add a `cargo doc`
stage") does not fix this defect class.** A doc block attached to the wrong item is still perfectly
valid documentation — rustdoc renders it happily, on the wrong function. That is exactly what
happened here.

| candidate | catches broken links | catches detached docs |
|---|---|---|
| `cargo doc -D warnings`, or `RUSTDOCFLAGS` on the existing test stage | **yes** (pending the unverified cell above) | **no** |
| `clippy::missing_errors_doc`, module-scoped (this change) | no | only the `Result`-without-`# Errors` half, in this file |
| crate-level `missing_docs` | no | **only if the orphaned item is `pub`** — both items here are, so it would have caught `dispatch_plan`; a doc detached from a private item is silent |

Recorded as an observation, not a scope request. If any of it becomes work, it is its own issue, and
this table plus the two precisions are its material.

## Citation correction: one shared tree, reached twice by `cd` — third version, and the first one measured

I published the clippy invocation as `ci/gate.ps1:100`. A reviewer confirmed it independently at
`99-100`. **Both wrong for `origin/main`, where the same command is line 129.**

**Four trees, each read with an explicit base (`git -C` / `git show <ref>:`), no `cd`, no inference:**

| tree | HEAD | clippy line |
|---|---|---|
| shared main checkout `F:/github/GraphHelm` | `1897651` (branch `issue-m09-arming-the-alarm`) | **100** |
| reviewer's worktree | `c357a5e` | **129** |
| my worktree | `f73e03a` | **129** |
| `origin/main` | — | **129** |

**Exactly one tree in the system says 100, and both of us read that one** — by `cd` into the shared
checkout, from two worktrees that would each have answered 129. Not a shared base by inheritance,
and not two different wrong trees: **one shared tree, reached twice by the same reflex.**

### This entry was corrected three times, and that is the finding

| version | claim | basis |
|---|---|---|
| 1 (mine) | the confirmation inherited my error by sharing a base | **inferred** — heredity assumed |
| 2 (reviewer's, adopted by me) | two *different* deviated trees | **inferred** — divergence assumed, my worktree never measured |
| 3 (this one) | one shared tree, reached twice by `cd` | **measured** — all four trees, explicit bases |

Each correction was issued with more confidence than the one before, because each was fixing a real
error. **The rule that was missing: a correction needs the same evidence bar as the original claim,
and "correcting someone who was wrong" is exactly the moment nobody applies that bar.** Version 2
was caught by an internal contradiction in my own message — I wrote that my read came from
`cd F:/github/GraphHelm` and, two lines later, that we had read *different* trees. Both cannot hold.

The substance was never affected: all four trees run
`cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings`, so the
default-lint-set conclusions stand throughout. Only the coordinate died — three times.

**Rules taken, operational rather than attitudinal:**

1. **Read with an explicit base** — `git show <sha>:<path>` or `git -C <worktree>` — **never `cd`
   into a shared checkout.** It is the shortest-path gesture, so it cannot be fixed by care; it has
   to be fixed by using a different command.
2. **Cite by CONTENT.** The command is identical across all four trees and does not rot; the line
   number rotted within two hours and took three attempts to bury.
3. **A correction is a claim.** Measure it before publishing it, especially when it corrects someone.

## Not established

- Not compiled, not gated. The slot is J's with a measurement in flight; zero cargo was run for
  this note. The change is doc-comment text only, and it must still pass the full gate before merge.
- **Survey of the rest of #101's diff: DONE, and the defect is isolated to `dispatch.rs`.** The
  mistake was mechanical, so a second instance was plausible; it is now ruled out by reading rather
  than assumed away.
  - `core/execution/src/transition.rs` — doc sits correctly on `is_terminal`. No insertion happened
    there; the function already existed and #101 only widened its visibility.
  - `apps/cli/.../driver.rs` and `apps/cli/.../mod.rs` — both deletions removed each copy's doc
    comment *together with* its function, so nothing was orphaned onto a following item. `mod.rs`
    gained a correct doc for the re-export.
  - `core/runtime/src/driver.rs` — the change is four lines inside a function body; no doc touched.
  Only `dispatch.rs` had a NEW item inserted into an existing file at a doc boundary, which is why
  it is the only site that could have had this failure.
