---
name: context-retrieval
description: "Assemble the evidence a task actually needs, cite it by stable item identity, and declare what was not retrieved. Use when an answer depends on repository or execution facts that have not been gathered, or when a previous answer cited nothing."
---

# Context retrieval

## What this skill is, and what it is not

This skill **assembles and cites**. It requests context through public read surfaces and proposes a
compiled result. It does not decide what is true, does not write to the repository, and does not
publish anything.

## Applicability

Use it when a conclusion depends on facts nobody has gathered yet, or when a result asserts something
about the system with no citation behind it. Skip it when the needed evidence is already cited and
still current.

## Reads

- `tool:status` for the current execution's state
- `tool:routes` for what surfaces are reachable

## Produces

A proposed context capsule and a result that cites it, under three rules:

1. **Every relied-on claim cites a stable item identity.** Not a position, not a line number, not
   "as discussed above". An identity derived from content survives an insertion; a position does
   not — and a citation recorded against a position does not dangle when the content shifts, it
   retargets, resolving cleanly to the wrong evidence.
2. **Required evidence is never dropped to fit a budget.** If it does not fit, refuse and say how
   much room would be needed. Trimming required evidence returns an answer that is under budget,
   internally consistent, and missing the thing the reader was supposed to see.
3. **What was not retrieved is written down.** A retrieval that silently covered less than asked
   reads exactly like one that covered everything.

## Refuses

- when a citation names an item the capsule does not contain — that is a citation to nothing, and a
  report counting citations rather than resolving them will read it as provenance;
- when required context does not fit the budget, with the budget that would fit;
- when the only way to answer would need authority this package does not hold.

## A note on cost

Report what was measured as measured, what was computed as computed, and what could not be observed
as **unavailable** — never as zero. A measured zero and an unobserved value are different findings,
and a total that silently includes the second reads as complete when it is not.

## Hands off to

`code-contract` when the evidence changes what the contract should say.
