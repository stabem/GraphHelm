> **PROVENANCE: this document carried the seeds for PR 2, which closed #74.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# Seeds from PR 2 (#74)

Draft text, not yet placed. `docs/milestones/m09-seeds.md` is the natural home, but F is
actively patching seed 9 in that file — placing these without coordinating would collide. B or
the orchestrator to direct where they land.

---

## Seed: surface the mis-burn to attention

`wake_mis_burns` is populated and **recorded but not yet surfaced**. The fold records every
consumption that burned an arming other than the one it named; nothing yet tells an operator.

The field, so whoever wires it connects a pipe that exists rather than inventing one — on
`ExecutionProjection`, keyed by session, last-wins:

    wake_mis_burns: BTreeMap<String, WakeMisBurn>
        at_sequence     — the consumption that did it
        captured_arming — the arming the sweep captured
        live_arming     — the arming that was live, and got burned

Both armings are kept deliberately: "something was wrong here" is not actionable, and the pair
is the diagnosis. Recovering it from raw events is exactly the derivation the single attention
predicate must not perform.

**Acceptance shape, and the distinction is the point:** a guard must prove the operator SEES it,
not that the field EMITS it. Those are different, and this milestone has spent itself proving
they are different — a value that is populated and never read is the shape of the defect this
seed exists to close. So the guard asserts visibility on the attention surface, not presence in
the projection.

Why it was not shipped with the fix: the fix's entire property is that every claim in it is
measured, and operator-surface work needs its own oracle design — what the operator sees, when,
and what proves they saw it. That would have entered unmeasured or doubled the change. Post-fix
the mis-burn state should also be unreachable through the main path, so the recording is
defence-in-depth whose surfacing is real but not urgent.

---

## Seed: a CLI that can PRINT a schema digest

`graphhelm schema catalog` can COMPARE digests and refuse on mismatch. It cannot PRINT one.

So every schema ritual that changes a schema needs the new digest and has no way to get it. In
this PR that cost a throwaway test in `core/schema-evolution/tests/` which called
`schema_digest` and printed the result — written, run once, deleted. The next ritual re-invents
it, and a scratch file in a reviewed crate is exactly the kind of thing that should not keep
reappearing.

Proposal: `schema digest <file>`, or a `--print` flag on `schema catalog`. One canonical
implementation of the thing nobody should hand-roll — the canonicalisation is a recursive key
sort plus an exact serialisation, and a 37 KB schema is not where anyone should discover a
number-formatting difference.

Not done here on scope grounds, and the grounds are this PR's own: a new subcommand arrives with
its own surface to guard, and everything in this change is measured. Adding an unmeasured
surface to a change whose property is measurement would cost more than the convenience is worth.

---

## Seed: the rendezvous-EQUAL burn's remaining slice, and what can never be known about it

The recorder-side discriminator closes this defect going forward. Two things remain open, and
one of them is permanently unanswerable:

1. **The fold-side check is forward-only.** `capturedArming` is absent on every consumption
   committed before this change, and the fold is permissive for absence — correctly, since
   inventing a mismatch from an absent field would make all committed history look defective.
2. **No analysis can ever decide whether this defect fired in the past.** Replay can say which
   lease was burned; nothing can say which one the sweep MEANT to burn, because only the live
   side was ever written down. The discrepancy between them is the defect.

So "precondition present, incident not observed" is the strongest claim the old data can ever
support. That is not a gap a better sweep could close, and a future reader must not search the
archives, fail to find the incident, and conclude it never happened — absence read as evidence,
on a defect whose entire shape is absence read as calm.

The precondition IS present in real history: the archived pair store shows session `agente-a`
arming rendezvous `factory-a-1` at sequences 26, 30, 33 and 39 — four arms, one rendezvous, one
session.
