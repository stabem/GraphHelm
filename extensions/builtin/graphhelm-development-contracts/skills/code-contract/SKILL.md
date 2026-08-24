---
name: code-contract
description: "Turn a change request into a development contract with a stated scope, a refusal vocabulary, and evidence obligations, before any code is written. Use when the files in scope, the acceptance criteria, or what would count as proof are still implicit."
---

# Code contract

## What this skill is, and what it is not

This skill produces a **document**. It asks the Runtime for facts through public read surfaces and
proposes an artifact for a person to accept or reject. It enforces nothing, publishes nothing, and
decides nothing.

That boundary is not modesty, it is the security model. Every enforcing decision in this system is
made by the Governor against a schema. A skill that also enforced would be a second authority whose
rules live in prose, and prose cannot be validated, versioned, or refused.

## Applicability

Use this before implementing a change whose scope or success condition is still being argued about.
Skip it when a current contract already covers the same scope and the same refusal cases.

## Reads

Ask for state through the public read surfaces only:

- `tool:status` for the current execution's state
- `tool:events` for what has already happened

If a fact you need is not available through those, **say so in the proposal** rather than inferring
it. An inferred fact and an observed one look identical once written down, and the reader has no way
to separate them afterwards.

## Produces

A proposed contract carrying:

1. **Scope as a file list**, not a description. "The auth module" is a description; a list of paths
   is a scope, and only the second can be checked against a diff.
2. **Acceptance criteria that name their instrument.** A criterion nobody can run is a wish. Write
   the command whose output decides it.
3. **A refusal case per failure mode**, each naming the operator's next move. Two failures with the
   same remedy are one refusal; two failures with opposite remedies must never share a code, because
   a code is an instruction and folding them tells the operator to undo correct work.
4. **What the contract will NOT establish**, stated rather than implied. The gap a reader can see is
   the gap that gets closed.

## Refuses

Stop and ask rather than guessing when:

- the scope cannot be expressed as a file list;
- an acceptance criterion has no instrument that could decide it;
- the change would need authority this package does not hold — this bundle can read the Runtime and
  propose artifacts, and cannot mutate, publish, or reach the network.

## Hands off to

`context-retrieval` when the contract needs evidence assembled, and `memory-curator` when finishing
the work produces a lesson worth keeping.
