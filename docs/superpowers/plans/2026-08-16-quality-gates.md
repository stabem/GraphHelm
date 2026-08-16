# Milestone 06 — Quality Gates: the usefulness story (skeleton, pre-plan)

> Status: DESIGN SKELETON from the 2026-08-16 divergent pass (five frames, 30 candidates).
> Not a task plan yet — the full plan is written when 05g merges. This document fixes the
> load-bearing decisions so they survive until then.

**The problem:** Milestone 05 proved CORRECTNESS end to end (TDD, sabotage-per-guard, the
self-verifying acceptance map). Nothing yet proves USEFULNESS: that a delivered feature
works as a user journey, that a screen is right and beautiful, that a UX is efficient, that
green code is not dead code. Two layers share one answer:
- **(a) the dev factory** (the A/B pair shipping PRs) needs usefulness gates on its own output;
- **(b) the product**: `Gate` and `Evaluator` node types exist and refuse execution since 05d —
  this milestone is where they finally RUN, and the factory is their first production user.

**The five load-bearing decisions (from the divergent pass):**

1. **The thymus rule — a gate must earn the right to gate.** Before any usefulness gate
   passes real work, it must REJECT a bred suite of "pathogen PRs": deliberately
   useless-but-green deliverables (dead feature, unreachable UI, tautological journey,
   blank-screen minimalism). Sabotage-per-guard elevated to the product level: a gate that
   cannot catch a synthetic fake is itself declared fake. The pathogen suite is versioned,
   grows with every real escape, and runs in CI against the gate — the gate has a gate.

2. **The blind judge — information asymmetry beats Goodhart.** Usefulness is scored by an
   Evaluator that receives ONLY the user story and the running system — never the code,
   never the tests, never the rubric the builder could read. It attempts the journey cold
   and files a structured verdict: steps-over-par, stall points, could-I-do-the-thing.
   Verdicts are never bare pass/fail — always **refusal-with-findings** (severity, cited
   evidence, remediation path), the audit-report grammar.

3. **The journey is a replayable artifact.** A demonstrated journey (HTTP transcript /
   event trace / DOM snapshot sequence) is recorded into the event store and the acceptance
   map grows a third binding: clause → tests → **demonstration**. The gate replays the
   demonstration against the new build deterministically; seeds/traversal orders are
   sampled at review time so the builder cannot precompute the exact evaluation path.

4. **Beauty is geometry, never prose.** Visual quality is evaluated on computed layout
   projections only — alignment grids, spacing rhythm, contrast pairs, density budgets,
   tap-target sizes — with every builder-authored string stripped before any judge sees the
   page (severs the prompt-injection channel; makes keyword-gaming worthless). The zero-JS
   monitor gets the first **typed layout grammar** checked server-side in Rust; a
   spec-derived **content manifest** (these N elements MUST be visible and reachable)
   closes the blank-screen loophole; the **view→clause peptide** (each view presents a
   machine-readable digest of the goal it serves) kills both orphan UI and invisible
   features. Screenshot diffing is explicitly refused as a foundation.

5. **Dogfood with separation of powers.** The factory's PR pipeline becomes a GraphHelm
   graph whose Gate/Evaluator nodes do the gating — the refused node types earn execution
   by carrying production load from day one. Gate definitions are event-sourced and
   **freeze before implementation starts**; a gate-definition change in the same PR as the
   code it gates is an automatic hard fail (anti-collusion, temporal separation).

**Metrics that are allowed to exist** (each a vector component, never a single scalar):
steps-over-par per journey; stall count and stall locations; content-manifest coverage;
layout-grammar violations by class; contrast/density budget deltas; pathogen-catch rate of
each gate (the gate's own fitness). **Refused metrics:** any single "quality score"; any
LLM-judge reading builder-authored text; adoption/pain telemetry before real users exist
(recorded as the follow-on era, with the trigger named: first external operator).

**Sequencing:** after 05g (the doorbell is small, ready, and the blind judge's long
evaluation loops want the wake primitive). Before Living Docs: the factory dogfooding
Gate/Evaluator is the strongest argument — the product's core promise (governed agent
work) finally exercises itself.

**Deferred to the plan proper:** node contract shapes for Gate/Evaluator execution; the
pathogen suite's first ten specimens; the layout grammar's first ruleset over the monitor
page; where demonstrations live in the store (evidence vs artifact); the judge's model
route and budget.
