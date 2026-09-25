---
name: keel
description: "Work under the Keel development model: load a contract card instead of the tree, spend a declared write surface, prove a promise against a named defect, and read the watchdog's verdict as a routing state. Use before writing code in any project that adopted GraphHelm."
---

# Keel

## What this skill is, and what it is not

This skill tells an agent how to WORK so that the deterministic half of Keel — the classifier in
`core/policy` driven by `policies/keel.yaml` — has nothing to refuse. It enforces nothing. Every
refusal below is made by code against the rules file whose version travels with the verdict; a
skill that also enforced would be a second authority whose rules live in prose.

A keel is laid before any plank and is the part of the hull that resists drift. Everything here is
one of two things: laid before (the card, the budget, the named defect) or resists drift (the
classifier, the ladder, the receipt).

**Today Keel is guidance and measurement, not punishment** (rules file `1.1.0`). Its goal is
quality first, then the lowest total cost per proven delivery. Count findings are signals for the
reviewer; only objective contracts refuse (an unparseable diff, a card scope that is not a list of
paths). The rung ladder is computed and reported but applied only when `ladder.enabled` is true, and
it ships off. Use only as much of Keel as the change needs: a docs or one-line change needs nothing
beyond the task record.

## The five moves, in order

### 1. Start from the card, then search on purpose

Ask `code-contract` for the current card, or write one: scope as a **list of paths** (no globs, no
`..`), the **exported symbols** the change will add, acceptance criteria each naming the **command**
that decides it, and one refusal per failure mode. The card is bounded by `keel.yaml` `card`:
paths, symbols and bytes. A glob or `..` in the scope is refused (`keel.card.scope_not_a_path`); a
card over a size bound is a signal to split (`keel.card.scope_too_wide`, `keel.card.too_many_symbols`,
`keel.card.too_large`): the cure is two promises, never a wider card.

Open what the card names first. When you need more, search for the specific symbol or caller
rather than browsing, and record in the PR body what the card was missing: that record is how cards
get better. An incomplete card that hid a dependency is the defect to fix, not a reason to guess.

For a repository where a source index would save repeated reads, create one in a private directory
outside the repository with `graphhelm keel index --repo <repo> --out <outside-dir>/index.json`.
Before using it, run `graphhelm keel verify --repo <repo> --index <outside-dir>/index.json`, then
`graphhelm keel query --repo <repo> --index <outside-dir>/index.json --term <symbol>` for the
specific card question. A changed source makes verification and query refuse the old index; rebuild
it rather than treating a stale hit as evidence. The index reports observed code facts, not the
user's intended behavior. Its first version extracts Rust public declarations only; unsupported
languages and omitted paths remain coverage gaps. A zero-hit or partial result never proves
absence. Inspect exact source when coverage or freshness cannot support the claim, and record that
fallback with the card. Skip the index for a small task whose named files are already enough.

### 2. Spend the surface you declared

Every node spends from `keel.yaml` `surface`: new modules, new types, new public functions, new
dependencies, new tests. Zero is a budget (a dependency is never free). If the promise needs more,
declare an **allowance** at planning time — it is capped by `maxAllowance` and paid from the
graph's budget, not argued at write time.

Before the diff leaves your hands, count what it brings into existence the way the classifier
does: a new source file is a module; `pub fn`, `export function`, a top-level `def` are public
functions; `pub struct|enum|trait|type`, `export interface|type|class|enum`, a top-level `class`
are types; `#[test]`, `it(`, `test(`, `def test_` are tests; a line under a dependency section of
`Cargo.toml`, `package.json`, `requirements*.txt`, `pyproject.toml` is a dependency. Private items,
nested defs and anything inside a test file are free. A file that gains more than
`body.maxFileLinesDelta` lines in one diff is reported as `keel.body.oversized_change`. These counts are signals, never a quota: do not inflate one function or drop a test to stay under a number.

Prefer the move that spends nothing: satisfy the promise inside an existing contract, cite an
existing proven symbol instead of writing a twin, extend a body instead of adding a type. The
cheapest correct diff is the one the classifier cannot charge.

### 3. Prove against a named defect

A test exists to kill a defect for one criterion in the card. Name both in the test: the criterion
id and the defect it would catch. A test that cannot fail on its own defect, that asserts a value
the test itself constructed, or that matches a log message instead of a structured output is not
proof and is refused by the meaningful-test policy this package already carries. Reuse an existing
instrument when it already observes the contract; a second test over the same promise is a
`newTest` charge with nothing bought.

Report passed, failed, skipped and unobserved separately. A skipped instrument is not a pass; an
unobserved promise is unresolved.

### 4. Read the verdict, and record the cause

The watchdog returns a `Classification`: the charges with the line each came from, the totals
against the budget, the findings by rule id (each marked blocking or not), and the policy version.
A blocking finding names an objective rule, and the repair is the smallest diff that removes it. A
signal is for the reviewer: answer it in the PR body (why the surface was needed) or remove the
charge. When a rule fires, record whether the cause was the card, the tool, the task or the agent.

When `ladder.enabled` is true (it ships off), drift and proof fold into a rung per seat: `full` → `contract_only` (no new module, type or
dependency) → `patch_only` (existing bodies only) → `propose_only` (write a plan for another seat).
Credit comes only from a passing named proof; a revert refunds nothing. A seat in `patch_only` is
not being punished for its past — it is being routed to the moves that cannot drift.

### 5. Spend time only where the change reaches

Keel optimises time as well as tokens. Run the tests the change can reach and name each command and
its result in the pull request; a reviewer runs them again. Do not run a full build for a change no
code reads.
If you see a check run that your change could not have affected — a full build for a Markdown edit,
a crate's tests for a file no crate compiles — record it with the rule that caused it and propose the
narrower path with a control that still runs the check when the change does reach it. Never narrow
by guessing: every doubt runs everything.

## Reads

`tool:status` for the node's mode and budget, `tool:events` for prior verdicts on the same
promise. A fact not available there goes into the card as a gap.

## Produces

A card (through `code-contract`), a diff that spends within its surface, a test that names its
criterion and defect, and a completion claim whose evidence is the instrument's output. Nothing
else: no rule edits, no budget edits, no verdicts.

## Refuses

Stop and say so rather than guessing when the promise cannot be stated as a card within the
bounds, when the smallest correct diff needs a surface the rung does not allow (ask for an
allowance or a split), or when no instrument can decide a criterion (`OBSERVER_MISSING`).

## Hands off to

`code-contract` for the card, `context-retrieval` for the cited symbols, `memory-curator` when a
refusal taught something worth keeping, and the JPD `journey-contract` skill when the promise is a
user journey rather than a code contract.
