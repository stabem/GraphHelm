# Spec-debt queue: triggers as commands

The queue itself lives in issue #35. This file holds only its **triggers**, restated so that a Task 0
can answer them by running a command instead of by judging a milestone.

## Why this file exists

A trigger written as a milestone name fires on the neighbour of its subject.

Entry 4 governs a signal called `missing_context`, and its trigger read *"harness Governor milestone
entry"*. On 2026-09-07 the Governor crate exists — `core/governor/src/apply.rs`, `externalize.rs`,
`inflight.rs` — so the trigger fires. The subject does not exist anywhere in `core/**` or `apps/**`.

A lane running Task 0 reads "fires", merges normative text about a signal that is nothing in this
tree, and the queue has manufactured second-degree spec-debt — which is what the two-consumer rule
exists to prevent.

**So a trigger names its SUBJECT, and names it as a command.**

## The form of the command, and why this section exists

The first version of this file wrote every trigger as `git grep -lE "A\|B"`. **Under `-E`, `\|` is a
literal pipe**, so each command searched for the string `A|B`, which exists nowhere, and returned
zero whether the subject existed or not. All five rows read "no", and would have read "no" with all
five subjects present. Found by the J lane on #994 before the file landed.

The cell that proves the form is live, and that belongs beside any table of greps:

```
git grep -l  "MAX_PROJECTION_NODES"                     <sha> -- '*.rs'  -> 3
git grep -lE "MAX_PROJECTION_NODES\|ZZZ_DOES_NOT_EXIST" <sha> -- '*.rs'  -> 0   <- the broken form
git grep -lE "MAX_PROJECTION_NODES|ZZZ_DOES_NOT_EXIST"  <sha> -- '*.rs'  -> 3   <- alternation
```

**The `-- '*.rs'` is load-bearing, and this file is why.** The first version of the cell ran without a
pathspec, and the numbers it quoted were measured before the file existed. Once committed, the file
contains the literal string `MAX_PROJECTION_NODES\|ZZZ_DOES_NOT_EXIST` — so the "broken" command
matches its own documentation and returns **1**, not 0, and the other two return **10**, not 3. The
proof of a false negative had become a false positive by being written down.

A document that quotes a measurement of the tree becomes part of the tree it measures. Scope the
proof to the population the rows actually search — the same `'*.rs'` every row uses — and the cell
measures the regex form again instead of measuring itself.

The first version also carried a control — `git grep -l "MAX_PROJECTION_NODES"`, without `-E`. It
was alive, and it caught nothing, because **it did not share the form of the thing it controlled**.
A control tests the instrument only when it is the same instrument.

**So every row below rides its control in the SAME invocation, in the SAME form.** The subject and
`MAX_PROJECTION_NODES` are alternatives of one pattern: if the output does not contain
`MAX_PROJECTION_NODES`, the command is broken and the row says nothing about its subject.

## The triggers

Run against `origin/main`. **Fired** means the output contains the subject; every output must contain
the control or the row is void.

```sh
git grep -hoE "<subject>|MAX_PROJECTION_NODES" origin/main -- '*.rs' | sort -u
```

| # | entry | `<subject>` | measured on `95a7ad9d` |
|---|---|---|---|
| 1 | Calibrated-claims doctrine | `ExtensionRegistry\|extension_registry\|ToolCache\|tool_cache` | control only → **not fired** |
| 2 | Freshness-scoped refresh floors | same subject as 1 | control only → **not fired** |
| 3 | Compile-time budget feasibility | `graphhelm_simulation` | subject + control → **FIRED** |
| 4 | Bounded expansion request | `missing_context\|MissingContext` | control only → **not fired** |
| 5 | Gate-surviving findings become claims | `KnowledgeClaim\|knowledge_claim` | control only → **not fired** |

*(The backslashes in this table are Markdown escaping for the cell separator. In the command the
pipe is bare — that is the whole point of the section above.)*

**Entry 3 fires, and the first version of this file said it did not.** Two independent defects hid
it: the literal-pipe form returned zero for everything, and the trigger named `GraphSimulator` /
`graph_simulator`, which the tree does not use. The crate is `graphhelm-simulation`; the symbol a
consumer sees is `graphhelm_simulation`, and `core/simulation/src/engine.rs` carries `pub fn
simulate`. **A trigger must name the symbol the tree uses, not the one the entry's prose imagined.**

Entry 4 reads **not fired** here and **fired** under its old milestone wording. That difference is
what this file was written for.

## Running Task 0

**PowerShell**, because that is the shell this repository requires and the one a lane will be holding:

```powershell
git fetch origin main
if ($LASTEXITCODE -ne 0) { Write-Host 'Task 0 aborted: the fetch failed'; exit 1 }
$sha = (git rev-parse origin/main).Trim()
if ($LASTEXITCODE -ne 0) { Write-Host 'Task 0 aborted: the sha did not resolve'; exit 1 }
Write-Host "measuring against $sha"
# per row:
#   git grep -hoE "<subject>|MAX_PROJECTION_NODES" $sha -- '*.rs' | Sort-Object -Unique
# every output must contain MAX_PROJECTION_NODES, or the row is void
```

POSIX shell, for a lane on bash:

```sh
git fetch origin main || { echo "Task 0 aborted: the fetch failed"; exit 1; }
sha=$(git rev-parse origin/main) || exit 1
echo "measuring against $sha"
# per row: git grep -hoE "<subject>|MAX_PROJECTION_NODES" "$sha" -- '*.rs' | sort -u
```

*(The first version gave only the POSIX block. `|| { ... }` is not PowerShell syntax and
`sha=$(...)` is not PowerShell assignment, so a Windows lane could not run the recipe at all — and a
recipe that cannot be run is the same failure as a trigger that cannot be measured, one level up.
The `$LASTEXITCODE` check is on its own line rather than chained, per the repository's rule that
nothing which can stop a pipeline may sit between a native command and the read of its exit code.)*

**The fetch is checked, and the sha is resolved ONCE and passed to every grep.** A transient fetch
failure leaves `origin/main` pointing at whatever it held before — the greps then run happily against
a stale tree and the lane records a trigger reading for a commit it never saw. Nothing goes red: a
`git grep` against an old ref is a successful command with a wrong answer. Raised on #994; it is the
same shape as the literal-pipe defect above, one layer out — an instrument that answers confidently
about the wrong subject.

Resolving the sha once also removes the drift between rows: five greps against a moving `origin/main`
can measure five different trees if a merge lands mid-run.

Record the sha you measured against — it is printed above so it can be pasted rather than
remembered. A row that flips to fired is the entry becoming due; record it on #35 with that sha, and
merge the entry's frozen text only then.

**A fired trigger is not permission on its own.** Read the entry's text and check that its second
consumer is real — the two-consumer rule is the queue's admission gate, and a fired trigger is one
consumer. Entry 3 fires today; whether the simulator is the *second* consumer of #10's graded cost
unit is a question for #35, not for this file.

## What this file is not

It is not the queue. The entries' frozen text, their economy rationale and their metric predictions
stay on #35, where they can be argued. This file holds the part that must not need arguing.
