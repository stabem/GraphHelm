# Keel: a development paradigm for code written by agents

**Why the classic paradigms were not enough, what was kept from each, and what is new.**

Status: position paper accompanying the Keel specification
([`docs/keel/KEEL_SPEC.md`](../keel/KEEL_SPEC.md))
and tracker #1212. Written 2026-09-23. Every number below comes from a cited source or from a
measurement made in this repository; a sentence with neither is a design decision and says so.

---

## Abstract

Software paradigms were written for humans who read code slowly, remember it across days, and
pay nothing per line read. An agent reads code by the token, forgets it at the end of the
context window, and pays for every line twice: once to read it, once to write it back. Applied
unchanged, the classic paradigms — SOLID, Clean Code, Clean Architecture, Extreme Programming,
the Pragmatic Programmer, design by contract — push an agent toward more files, more indirection,
more tests that assert nothing, and more prose that costs on every turn. Keel keeps the part of
each paradigm that survives measurement, discards the part that does not, and adds the one thing
none of them has: a deterministic watchdog that attributes drift to a seat, repairs it, and
changes what that seat may write next. The unit of work is a promise with a named proof; the unit
of cost is the whole billed session; the enforcer is code over a versioned rules file, never prose.

## 1. The problem, in numbers

An agent's spend is dominated by reading, not writing: about three quarters of tokens in a coding
session go to reads ([SWE-Pruner, arXiv 2601.16746](https://arxiv.org/html/2601.16746)). What it
reads is decided by what is always loaded and by how the tree is shaped. Always-loaded context
files raised inference cost by more than 20% with no gain in success, and repository overviews in
particular did not help ([arXiv 2602.11988](https://arxiv.org/abs/2602.11988)); selective,
path-scoped context cut cache-creation tokens on every task at unchanged correctness ([arXiv
2607.27250](https://arxiv.org/html/2607.27250)). Structure costs too: cross-file indirection cost
about 3× after controlling for size, with 2.2× more round trips, and single-implementation
interfaces are zero-information tokens ([arXiv 2604.07502](https://arxiv.org/html/2604.07502)).

Writing is where the waste hides. Stripping tokens from files on disk looks like a saving and is
not: the most aggressive stripping cut input 17% and raised billed session cost 67%, because the
bill is the whole session — reads, cache creation, cache reads, output, turns — not the delivered
bytes ([PointFive](https://www.pointfive.co/press/pointfive-research-token-reduction-not-cost-reduction)).
Rule compliance decays as the agent generates: an odds ratio of 0.944 per function for a trivial
annotation rule ([arXiv 2605.10039](https://arxiv.org/abs/2605.10039)), so a rule stated once in a
prompt is a rule that is off by the twentieth function. Passive instructions ("be concise") saved
about 6% where scheduled compaction saved 22.7% ([arXiv 2601.07190](https://arxiv.org/abs/2601.07190); [Augment guide](https://www.augmentcode.com/guides/ai-agent-loop-token-cost-context-constraints)).

Tests are the third leak. Line coverage saturates at 100% for LLM-written suites while mutation
score varies between 84% and 96% and correlates with fault detection at r = 0.11 ([arXiv
2609.24341](https://arxiv.org/abs/2609.24341)). LLM-written oracles capture what the code does, not
what it should do ([Autonoma](https://getautonoma.com/blog/ai-generated-tests-pass-but-dont-assert)).
Agents add a mock in 36% of test commits and choose a mock over a fake 95% of the time ([Hora &
Robbes, MSR 2026](https://arxiv.org/html/2410.10628)). Projects at 91% functional correctness
still carried between 1,305 and 3,193 design findings ([arXiv 2604.06373](https://arxiv.org/abs/2604.06373)),
and 13–53% of merged agent pull requests introduced new issues.

None of this is a defect of any one paradigm. It is what happens when advice written for a reader
with unlimited free reads is followed by a reader who pays per token and forgets per session.

## 2. Method

Three inputs, kept separate so their weight can be judged:

1. **The owner's order** (2026-09-22): a paradigm for AI guidance that spends fewer tokens
   reading and writing, keeps code testable without useless tests, and carries a watchdog node
   that detects drift, repairs it, and penalizes the offender.
2. **A divergent ideation run** under six isolated cognitive frames (hostile competitor, 3 a.m.
   on-call engineer, regulator, remove-the-load-bearing-assumption, biology, game design): 36
   ideas, 18 flagged as traps with a one-line reason, clustered into six angles, three deepened.
   The frames were isolated so they could not anchor each other.
3. **A web research sweep** across eight angles (context engineering, spec-driven development,
   token-efficient code, meaningful tests, drift guards, agent harnesses, critiques of the
   classics for the LLM era, new paradigms 2024–2026): 105 agents, every finding re-verified by
   opening its source and one independent source, 93 kept, 3 refuted; synthesized into 37 kept
   principles, 16 discarded with the measurement that discards them, and 12 gaps no source covers.

The synthesis was then reduced to six laws by one test: a principle stays only if a deterministic
mechanism can enforce it. Advice that only a reader can enforce is not a law; it is a comment.

## 3. What was kept, and from where

| School | Kept | Discarded, and the measurement |
|---|---|---|
| **Design by contract** | The contract card: scope as a file list, exported symbols, criteria naming their instrument, one refusal per failure mode. Pre- and post-conditions paired, with a violating input shipped. | Postcondition-only or prompt-only contracts: honoured 23–41% in prompt alone; invalid inputs admitted 76–82% without a violating example ([ContractEval](https://arxiv.org/html/2510.12047)). |
| **SOLID** | Single responsibility (one file, one reason to change). Dependency inversion **at the boundary only**. | "Always program to an interface", interface segregation as a default: single-implementation interfaces are zero-information tokens; indirection costs 3× ([arXiv 2604.07502](https://arxiv.org/html/2604.07502)). |
| **Clean Architecture** | The dependency rule as a tag/layer matrix with a mandatory reason per forbidden edge ([Nx module boundaries](https://nx.dev/docs/features/enforce-module-boundaries)). Vertical slices: a feature is a directory, slices talk through contracts. | The full layer stack per feature: six files opened to change one business decision ([Miller, "the codebase is the prompt"](https://jeremydmiller.com/2026/06/04/the-codebase-is-the-prompt-wolverine-vertical-slices-and-ai-assisted-development/)). The file hop is the cost, not the line count. |
| **Clean Code** | Greppable identifiers, explicit types on signatures, files sized to one tool read, error messages that interpolate what they received ([Akita](https://akitaonrails.com/en/2026/04/20/clean-code-for-ai-agents/)). Comments that are contracts. | "Extract until each function does one thing" as a hard cap on hotspots: extraction spread complexity across more files and the agent opened more of them, tokens flat ([SonarSource minimal pairs](https://arxiv.org/abs/2605.20049)). "Delete comments": accurate comments raised model comprehension from 84% to 96%, wrong ones cut it to 61%. |
| **Extreme Programming** | Test-first **intent**, as an attachable red-on-parent artefact the watchdog verifies; short cycles; tests immutable to the implementing agent ([Beck](https://newsletter.kentbeck.com/p/augmented-coding-beyond-the-vibes)). | TDD as a self-discipline enforced by prompt: "the genie doesn't want to do TDD"; prompt-level guidance moves the intercept, not the decay slope. Coverage as the gate (r = 0.11). Mock-every-collaborator isolation. |
| **Pragmatic Programmer** | The tracer bullet: one thin promise end to end before widening. DRY of knowledge, not of text. "Document the why" as decision records with an executable check. | DRY by Manager/Service/Util extraction across features: generic names break the grep navigation agents live by, and cross-slice services regrow the shared ball of mud. A narrative overview in always-loaded context. |
| **Journey-Proven Development** (this repository) | The unit of work is an observable promise with a named proof instrument; `OBSERVER_MISSING` is a first-class result; retries stay linked to their first failure. | "Functional tests green means the code is good": 2.11 issues per task that passed its tests ([arXiv 2508.14727](https://arxiv.org/abs/2508.14727)). Delivery proof is necessary, not sufficient, for design health. |

## 4. The six laws

Each law names the mechanism that enforces it. The mechanism is the law; the prose is its label.

**Law 1 — Nothing exists without a promise and a consumer.** Work starts from a contract card
bounded in paths, symbols and bytes; a card over a bound is refused before anyone reads it. A new
module, type or public function is a door that needs a key: a consumer that already exists in
the graph. Ownership of every source file by exactly one promise, with orphan code refused the
way orphan tests already are ([Spec Growth Engine](https://arxiv.org/pdf/2606.27045)).

**Law 2 — The write surface is declared, spent, and refused by name.** A node spends from a
budget vector {new module, new type, new public function, new dependency, new test}; zero is a
budget. A deterministic classifier reads the diff and charges each declaration it recognises;
every overrun is a finding by rule id, never a score, because a score lets a large win on one kind
buy sprawl on another. A per-file added-line cap stands in for the parser the first slice does
not have, and says so.

**Law 3 — A test is born against a named defect.** A proof instrument exists to kill an executable
defect for one criterion in the card. Admission, in order: not vacuous, an independent oracle,
passes, kills at least one mutant on the changed lines that the pre-existing suite left alive
([Meta TestGen-LLM](https://arxiv.org/html/2402.09171); [ACH concern
mutants](https://engineering.fb.com/2025/09/30/security/llms-are-the-key-to-mutation-testing-and-better-compliance/)).
Mocks only at contract-named boundaries. Tests and the rules file are immutable to the
implementing agent.

**Law 4 — Penalty is a routing state, not a message.** Drift and proof fold over a seat's recent
records into a rung: `full → contract_only → patch_only → propose_only`. Each rung is a smaller
write *shape*, not a smaller number; an allowance cannot reopen a closed rung. Credit comes only
from a passing named proof; a revert refunds nothing; credit is never banked ahead of drift. The
repair's own cost is charged to the offender; a reviewer who vouched for a head later debited is
debited at a smaller tariff.

**Law 5 — The enforcer is itself receipted and pinned.** The rules file is versioned and
content-addressed; its version travels with every verdict. A refusal is an append-only event
carrying rule id, config version, diff hash and actor. A penalty that does not replay is void.
The watchdog earns the right to gate by rejecting a bred suite of useless-but-green specimens,
the way every registered gate in this repository already does.

**Law 6 — Verification is proportional to what the change can reach.** Added on 2026-09-23 by
owner order: Keel also optimises time. The unit of cost is the whole delivery, so a check the change
cannot affect is cost with no evidence bought. Verification runs what the change can reach: Markdown
no code reads needs no build, a Rust edit runs the tests of the crates that depend on it, and only
the schemas, the protocols or the dependency graph run the whole workspace. Since 2026-09-24 there is
no separate gate: the author and one reviewer run the reached tests and name them in the pull
request. The two failure directions are not symmetric: running too little ships a defect, so every
doubt still widens to the whole workspace; running too much only wastes time, so it is corrected, not
tolerated. A check found running without being reachable is drift in the method, recorded with the
rule that caused it and fixed with a control that still runs the check when the change does reach it.

## 5. Why Law 4 is new

The research sweep found the neighbours and their edges. Architectural fitness functions and
[archfit](https://asdecided.com/articles/how-to-prevent-architectural-drift-from-ai-coding-agents)
emit a repair task; [AgentLint](https://github.com/mauhpr/agentlint) keeps a per-session fire
ledger with an escalating severity ladder; [AHE](https://arxiv.org/abs/2604.25850) reverts a
config edit whose predicted effect fails. None attributes a violation to a seat, folds a score
across sessions, or feeds that score into what the seat may do next — its write surface, its
budget, its model tier. That absence was the single largest gap the synthesis named.

GraphHelm already holds the primitives the neighbours lack: an identity line per lane, an
append-only Event Store the fold can replay, a budget amendment surface, route eligibility, and a
gate registry whose entries are certified against pathogens. Law 4 is what those primitives were
missing a reason for.

## 6. Why the name

Three of six isolated ideation frames arrived at the same word independently. A keel is laid
before any plank and is the part of the hull that resists drift; it does not steer, the helm
does. That is the division of labour: Keel bounds the writing, GraphHelm's graph steers the work.

## 7. What Keel does not claim

- The first classifier is a line grammar, not a parser: it does not see a function grow inside a
  body, resolve re-exports or macros, or read a manifest beyond its section headings. Each limit
  is sealed in the rules file, and the pathogen suite that certifies the classifier as a gate is a
  later slice.
- No source measures write-token economy directly (first-write survival to merge, patch
  precision); Keel will measure it from the Event Store before claiming it.
- "The card is answerable without opening bodies" is a hypothesis no source tested; the contract
  index slice pairs it with a body-read ratio.
- Compliance decay under re-checking every K units is unmeasured for real paradigm rules, and the
  cost of re-injection on the cache prefix is unknown.
- Multi-lane drift attribution — two seats drifting one module, a drift that appears only in the
  squash of two clean pull requests — is unsolved everywhere, including here.

## 7a. Revisions after an independent review (2026-09-23)

An independent reviewer (the Codex orchestrator lane) read the specification, the skill, the policy,
the classifier and this paper before anything merged. Six of its points change the model, and they
are adopted here rather than argued:

| As first proposed | Why it was wrong | As adopted |
|---|---|---|
| Optimise read and write tokens | Fewer tokens can mean more repairs, calls and cost | Quality first, then lowest total cost per proven delivery |
| "Read only the card, never browse" | An incomplete card hides a dependency | Start from the card, search on purpose, record the missing context |
| Fixed quotas refuse by count | Invites huge functions, fewer tests, artificial splits | Counts are signals for the reviewer; only objective contracts block |
| Every test must kill a mutant | Contradicts the repository's meaningful-test policy (#1198), which refuses mutation exercises as a ritual | Smallest adequate observer; mutation only when it adds evidence |
| Penalise the agent's seat | The cause may be the card, the tool or the task | Record the cause first; measure by task, model, context and tool |
| Consumer must exist before the producer | Blocks features that create both | A consumer may arrive in the same change when both are proven there |

Two further points are accepted as stated. **Size is not risk**: `patch_only` is not a safe shape,
because one line can remove a permission check; risk is read from what a change touches. **The
sources motivate hypotheses; they do not prove Keel**: the context-file study supports avoiding
unneeded context, not forbidding investigation, and TestGen-LLM treats mutation as future work with
a real execution cost.

**Status therefore: Keel ships as guidance and measurement.** The classifier reports; it blocks
only objective contracts. The ladder of Law 4 is specified and implemented as a pure function, but
it drives nothing until the comparison below shows it helps.

**The comparison.** Three arms on the same frozen tasks, budget, model access and acceptance
checks: **A** the current process and gates; **B** Keel's card, targeted context and signals;
**C** B plus blocks and penalties. A answers whether the organisation helps; C against B answers
whether punishment adds anything. Outcomes are judged by independent acceptance tests, journeys
that must keep working, and blind review — never by "passed the Keel gate". Measured: accepted
deliveries and escaped regressions; total cost of all attempts divided by accepted deliveries,
including review, repair and watchdog; human and wall time separately from money; wrong blocks,
abandoned work and hand-offs; and the cost of a second change to the same code, so a saving that
turns into expensive maintenance is caught. A 12-task pilot finds the experiment's own defects; it
proves nothing. A larger comparison is then sized from the observed variance, on fresh tasks with
criteria fixed before running. If C does not beat B, the penalties are removed and the rest stays.

## 8. Traps refused

Cells-not-files as the unit of code (a build-system project, not a policy); tests synthesised from
a property DSL (a research project); AST patches as the authoring surface (fights how models write
— parse the text diff at the gate instead); read-set declaration enforced only when every read is
a metered tool; telomeric write budgets with no recovery path; one-strike trust resets; a warrant
per function (multiplies card bytes); refusing every stdout match as non-proof (for a CLI,
structured stdout is the contract — refuse log-message matches, not structured output).

## 9. Delivery

The slices below are the plan as first written; section 7a governs which of them may enforce anything.

Slice 1 ships the specification, the rules file (`policies/keel.yaml`) with its schema, fixtures
and entry skill in the development-contracts package that `graphhelm setup` installs, and
`core/policy::keel` (`classify_write`, `ladder`, `check_card`) with tests at both sides of every
bound and one diff per language. Slice 2 registers `gate-keel` with its pathogen suite and adds
mutant-kill admission for tests. Slice 3 folds the ladder from the Event Store per actor and
wires it to budget and route eligibility. Slice 4 generates the contract index from the AST and
serves it through the context compiler instead of files.

## References

The sources above are the subset cited inline; the full verified set (93 findings across eight
angles) and the discarded set are recorded on #1212. arXiv identifiers are given as the
research agents opened them; a reader should confirm the identifier before citing onward.
