# SEALED prediction — #154's guard, written BEFORE the slot opens

D Agent, 2026-08-20, branch `issue-154-doc-attachment` off `origin/main` @ `9f3ee1e`.
**Written before any cargo ran** (slot queue is J #118 → F #152 → me). Sealed so the guard cannot
be graded against a number it has already seen.

## What the guard is

`#![warn(clippy::missing_errors_doc)]` at module scope in `core/execution/src/dispatch.rs`.
The gate runs clippy with `-D warnings` (`ci/gate.ps1:100`), so a warn-level lint is a red stage.

Adopted from L's review suggestion, narrowed by me to the module. It is the ONLY mechanical
detector proposed for this defect class, and a guard nobody can make fail is decoration — hence
this file.

## Positive control (runs first, and it is the one that can quietly fail)

**P0 — the unsabotaged branch gates GREEN.**
If the attribute fires on existing code, the "cost" of this guard is larger than the fix and the
scope claim in the module comment is false. Blast radius was enumerated by reading, not assumed:
`dispatch.rs` has exactly one public `Result`-returning fn (`dispatch_plan`), and this change gives
it its `# Errors` back. **Predicted: GREEN, 27 stages, zero red.**

### AMENDMENT 1 — P0 does not prove the attribute is in effect (L, before any run)

Recorded as an amendment rather than a silent edit: **nothing has been executed yet**, so no number
has been seen and the seal is still honest. Raised by L on reading `b5e98b0`.

**P0 green is consistent with two different worlds:** the lint is active and finds nothing, or the
attribute is misplaced/ignored and does nothing at all. A positive control that passes identically
whether or not the instrument is connected is not evidence the instrument is connected — the same
objection this factory applies to any absence-shaped guard. **S1 is the only step that
discriminates**, so the order matters and P0 must never be reported as "the guard works".

**Discriminator if S1 comes back GREEN — corrected, because L's proposed one does not discriminate.**
L suggested swapping `#![warn(...)]` for a temporary `#![deny(...)]`. That distinguishes nothing
here: the gate already runs clippy with `-D warnings`, so warn and deny are the same red when the
attribute is in effect, and both are equally silent when it is not. The variable being tested is
*placement/scope*, and the swap does not vary it.

The clean A/B is **attribute versus command line**: re-run clippy with
`-W clippy::missing_errors_doc` passed as a flag, which takes effect unconditionally.

| command-line `-W` | module attribute | conclusion |
|---|---|---|
| RED | GREEN | the lint catches this shape; **the module attribute is not in effect** — placement or scope |
| GREEN | GREEN | **the lint does not catch this shape**; the guard is wrong in kind, not in placement |

Either outcome kills the guard as written, which is the point of sealing it.

## Sabotage S1 — the defect this guard exists for

Delete `dispatch_plan`'s `# Errors` section (lines 38-39), leaving the rest of its doc intact. This
reproduces the observable half of #154: a public `Result`-returning fn without documented errors.

**Predicted: the clippy stage goes RED, naming `clippy::missing_errors_doc`, at
`core/execution/src/dispatch.rs`, on `dispatch_plan`.**

**Falsifier — and it kills the guard, not the fix:** if the gate stays GREEN under S1, then the
module-scoped inner attribute is not in effect (module-level lint levels not honoured as I expect,
or the lint does not fire for this shape). In that case the guard is decoration and must be either
removed from #154 outright or escalated to crate level as its own scoped decision — it must NOT be
kept as a comforting no-op. I commit to reporting that outcome as a failed prediction rather than
quietly rewording the guard.

## What this guard explicitly does NOT catch — stated now so no one reads it as broader later

1. **The mirror shape.** `parallel_limit` carrying a `# Errors` for an error it cannot return —
   the other half of #154 — is not linted by anything. Fixed by hand here; unguarded afterwards.
2. **The actual mechanism of the defect.** #154 was caused by INSERTING an item between a doc block
   and its function. The lint sees a consequence, not the cause, and only when the consequence
   happens to be a missing `# Errors` on a `Result` fn. An insertion that detaches a doc from a
   non-`Result` fn produces no warning at all.
3. **Any other module.** One file is guarded. The class is repo-wide.

Point 2 is the honest limit: this is not a doc-attachment guard. It is a lint that would have
caught THIS instance. Naming that gap is worth more than a guard that is trusted past its reach —
a one-module detector read as "we cover that now" is worse than an openly named gap.

## Grading

At the slot: run P0 first, then S1, then restore and re-run P0. Record exit codes read from the
producing command itself (never through a pipe or a `;`-chain), with the sha captured AT LAUNCH.
Results go to `.factory/d-agent-154-results/` and into the PR body whether they confirm or refute.
