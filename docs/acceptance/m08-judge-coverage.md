# What the blind judge never touched — measured, not judged

The M09 plan asked *"what did the first judge stop catching by being inside the loop?"*. The
reviewer refused the question as close to unfalsifiable: answering it requires knowing what he
SHOULD have caught, and if we knew that we would not need him. He proposed a replacement that
needs no omniscience — **what did he never touch?** — and stated a hypothesis before the
measurement so it could be falsified. This file is that measurement.

Source: `read-audit.jsonl` in `m08-rejudge{5..9}-2026-08-18` — 60 recorded requests, the runs
where the recorder existed. The audit records SERVED bytes, so it has no opinion.

## Result: he exercised exactly half the tool surface, and the same half every time

| touched | never touched in any of the five runs |
|---|---|
| `start`, `status`, `events`, `wake_arm`, `wake_status`, `amend_budget`, `routes` | `signal`, `approve`, `pause`, `resume`, `cancel`, `probe`, `wake_wait` |

Distinct routes per run: **5, 5, 6, 6, 6** — out of 14 registered routes, against a union across
all five runs of **7**. The union barely exceeds the individual runs, which is the finding: the
runs did not explore different parts of the surface, they re-walked the same part.

## The cause is the story, not the loop

`graph.yaml` was compared across **all seven** M08 judge runs, not only the five the recorder
covers. Every one is **byte-identical except the execution identifier** — same two edges, same
`judge` node, same user story. The world was the same from the first run, before the recorder
existed. (Measured twice, independently, by both authors; the first normalisation here was too
narrow to collapse `exec-m08-judge` and `exec-m08-judge6bb`, so the files were diffed directly
rather than trusted through a digest.) He did not run a fixed
PROTOCOL five times; he ran a fixed WORLD five times, and a world that never blocks on a human
never produces an `approve`, and one that never pauses never produces a `resume`.

**The reviewer's hypothesis is confirmed and was stated first** — and he declined the credit,
on the ground that he knew the story, so predicting what it cannot produce was cheap. The under-coverage is explained
by the fixture without needing to invoke the loop at all. Occam applies to our own suspicion of
ourselves: before hiring a second judge to escape our influence, give the first one a **second
story**. It is far cheaper and immediately testable, and if a new story leaves the same half of
the surface untouched, the loop hypothesis becomes the best remaining explanation and can be
paid for then.

## Limit of this measurement, declared

`wake_wait` is the one tool whose work is not an API request (`apps/cli/src/commands/mcp/tools.rs:416`)
— it blocks locally on the rendezvous. The recorder cannot see it, so for that one tool "never
touched" is **unprovable here**, not proven. The other six untouched tools are HTTP routes and
their absence is a real absence.

Nothing above is a verdict on the judge. It is an inventory of what the surface offered and what
his world could reach.

## The action this licenses, with the condition that makes it worth paying for

A second story — and **it must be written to produce the moments the first cannot**, not merely
be a different graph. Concretely it needs a node that stops and waits for human approval, an
execution that is paused and resumed, and one that is cancelled. A second story that is just
another graph would re-walk the same seven routes and turn this measurement into ornament.

**The prediction to settle:** if the new story leaves the SAME half of the surface untouched,
the fixture explanation is exhausted and the loop hypothesis becomes the best remaining one —
at which point a second judge has evidence behind it rather than suspicion.
