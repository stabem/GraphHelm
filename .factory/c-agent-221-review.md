# Cross-review of the #221 blueprint — criteria frozen before either comment was opened

**Method.** Criteria and five predictions were derived from **#221's body**, the **Task 005 section of
the plan** (`f0640cd`, lines 253+), and `origin/main` — **then sealed**, and only then were
`5392769204` and its follow-up `5392787280` opened. The frozen file is
`.factory/c-agent-221-frozen.md` on `claude/c-agent-e82f40`.

## Disclosures — what I was given before reading, so it is not read as my finding

**Wave rules, identical for all seven:** ED-18; skills data-only; Governor-only; append-only manifest
(#216); bytes-not-digests (#217).

**Two coordinator decisions I was told this document already incorporates — received, not the
author's findings:** **T9 = whole-response refusal**, and **injected clock (#218)**.

- **T9 is present and correctly attributed** by the author as *"decided by the orchestrator, not left
  to the implementer"*. **Credit for that decision is not his and he does not claim it.**
- **The injected clock is NOT in this document.** Measured: no occurrence of clock / `Instant` /
  `SystemTime` / injected anywhere in either comment. **This is very likely correct rather than
  missing** — nothing in this task's surface is time-dependent — but I was told the document
  incorporates it, and it does not, so I say so rather than list both as disclosed.

**The coordinate arrived as a question, not an answer:** *where can a refusal be translated into
something that reads as success?* — with an explicit "I am not telling you whether the blueprint
treats it."

## Predictions, scored — four of five falsified

| # | sealed before reading | outcome |
|---|---|---|
| 1 | `Nothing now` reachable from an error path is untreated | **partly holds** — see F1, and the residual is narrower than I sealed |
| 2 | the fallback's indistinguishability is unaddressed | **narrowly holds** — see F3 |
| 3 | guards assert on the validation outcome, not the output bytes | **falsified** — T5 asserts the `Result` slot text *byte-identical across repeated calls* |
| 4 | the exclusive serializer is asserted but not guarded | **falsified** — T10 is exactly that guard |
| 5 | a refusal presented *as one of the two options* is unconsidered | **falsified, and by something better than a test** |

**Prediction 5 is where the design beat my criterion.** I expected to find the "refusal dressed as
Option B" case unhandled, and looked for a test. **There is no test because the architecture
forecloses it:** the `Result` slot is enum-derived from `result.status` and no path lets caller text
reach it, so a `Failure` renders the failure arm *regardless of what the options say*. **A structural
impossibility beats a guard, and this one is stated as the central invariant rather than discovered
per-case.**

**And T8 is the same move at its strongest:** *"attempt to construct the plan type with any field
holding rendered text → does not compile"*, with the sabotage named as a future edit adding a `String`
field "for convenience". **A compile-time proof is not a stricter test; it is a different kind of
claim.**

---

## F1 — the invariant guarantees rendering faithful to `status`; nothing here makes `status` faithful to what happened, and the threat table implies otherwise

The central invariant is exact and I would not weaken it:

> *No sequence of valid inputs produces a `Result` slot whose rendered phrase contradicts
> `result.status`.*

**That closes laundering inside this component completely.** What it cannot close — and cannot be
expected to — is an upstream stage that **swallows a failure and reports `status: Success`**. That is
a *valid input*, and this validator renders it faithfully. The laundering then happens upstream and
**passes through untouched**.

**The finding is not the gap — it is that §4's table reads as covering it.** The row says a
compromised or buggy upstream stage "cannot have that input reach rendered bytes — caught at steps
2/3". **Steps 2 and 3 catch cardinality mismatch and secrets. Neither inspects whether `status` is
true.** A reader of that table concludes the upstream threat is closed here; it is closed only for the
two shapes named.

**Cost to fix: one sentence**, and it converts an implied coverage into a declared boundary — *a
truthful `status` is the caller's obligation and outside this validator's reach; what this task
guarantees is that a stated failure cannot be rendered as success.*

**Why it matters at this specific spot:** `Nothing now` is where the gap is least visible to the
owner. A swallowed upstream failure arriving as `owner_action_required: false` renders the calmest
sentence the system can produce. **The owner's reading of "Nothing now" is identical whether nothing
needed doing or nothing could be determined** — and this task is the last place before those bytes
reach a human.

## F2 — T10 is the right guard and needs its own positive control, or it is the empty-set case in the one test that protects the door

T10 (*only this module emits owner bytes*, by static grep across the workspace) is the guard I would
have asked for and did not expect to find. It closes the door rather than the function, which is the
distinction most of these designs miss.

**But a grep is a derived key.** Its predicted GREEN is *"zero call sites construct owner-facing bytes
outside `owner_output.rs`"* — **and zero is what a pattern that matches nothing returns.** A future
emitter that constructs bytes by a route the pattern does not describe yields the same zero as
compliance. **The test cannot tell "no violators" from "the pattern found nothing".**

**Fix, one assertion:** the same grep must find **the known emitter** — `owner_output.rs`'s own
serializer — before the zero is trusted. That is the positive control that makes the zero a fact about
the workspace instead of a fact about the pattern. **Without it, the guard whose whole job is to catch
a bypass is the one that passes when it is blind.**

## F3 — the discarded plan leaves no owner-visible trace, and the blueprint does not say where the refusal is recorded

T7's predicted GREEN is that a malicious plan is discarded and *"the response still renders via the
built-in plan"* — the correct behaviour, and the sabotage it names (*a discard path that fails the
whole response*) is the right one to catch.

**The narrow gap: `OWNER_OUTPUT_SCHEMA_INVALID` fires on the plan, and the blueprint does not say
where that code goes.** As written, **a stylist that attacked and was rejected produces byte-identical
owner output to no stylist at all.** For a single call that is arguably right — the owner does not
need to know a component misbehaved. **Across calls it is the difference between an attack nobody sees
and an attack somebody counts.**

**One clause settles it:** name the durable surface the plan-level refusal lands on, or state
deliberately that it is per-call and not recorded. **Either is fine; the absence of the sentence is
what leaves it unknowable.**

---

## What passed, including two items above the bar this review could set

- **The invariant is calibrated, and the calibration is stated:** *deliberately narrower than "the
  response is never wrong" (unfalsifiable) and broader than any single test*. **Most invariants fail
  on one side or the other and few say which.**
- **T8's compile-time proof** and **T5's byte-identity across repeated calls** — content and
  determinism in one assertion.
- **T4 refuses the fabricated second option** with a real defer/stop/rollback path carrying its own
  consequence, which is the honest form of "only one safe action exists".
- **§10 declares the dependency status without softening it:** #217 is open, the types do not exist,
  nothing can be type-checked or sealed as fixtures, *"report the design, not the patch, until the
  dependency clears"*.
- **§9 leaves three questions open and marks the fourth decided, attributed to whoever decided it.**
  **An open question left open is worth more than one closed by the author's preference.**
- **The follow-up's reasoning for the whole-response refusal is the correct shape** — *a partial
  response that silently dropped a section lies about its own completeness; "looks complete" is the
  worse failure, not the safer one* — and it names the section and the reason code **without
  repeating the caught content**.

## Verdict

**The design is sound and I would not change its approach.** The central invariant is the right one,
and the two structural closures (enum-derived `Result` slot; a plan type that cannot carry a value)
are stronger than the tests that would otherwise be needed to police them.

**F2 is the one I would want before implementation** — one assertion, and it is on the guard that
protects every other guard. **F1 and F3 are one sentence each**, and both convert an implied coverage
into a stated boundary. **None blocks.**

## What this review is not

**I executed nothing** — no cargo, no gate, no fixtures; nothing in this task exists to run yet. Every
statement above is derived from the issue, the plan section, the two comments, and `origin/main`.
**I did not read any other blueprint in this wave**, and I am the author of #220's, so this pass is
independent of the others by construction rather than by discipline.
