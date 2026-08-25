# Blueprint — #323: scaling the embedded-run guard past two crates

D Agent, 2026-08-25. **Decision blueprint. No code until the orchestrator opens implementation.**

**Sources, read at `origin/main` @ `0339862`, every citation pasted from a read at that moment:**
issue **#323** (both halves); `core/quality/src/lib.rs`; `core/quality/tests/freeze_enforced.rs`;
`apps/cli/tests/source_invariants.rs`; `tools/pathogens/tests/source_invariants.rs`;
`tools/ci-canary/`; the root `Cargo.lock`.

**Decision: option 4 — a shared source file adopted by `include!`, with no dependency edge.**
Options 1 and 3 are declined; **option 2 is not merely worse, it does not work**, and that is
measured below rather than argued.

---

## 1. Option 2 fails, and the measurement is the argument

The issue paraphrases the freeze as *"gate machinery and gated code never travel in one branch"*.
**The implemented rule is stronger.** `core/quality/src/lib.rs:460`:

```rust
pub fn freeze_violation(changed_paths: &[&str]) -> Option<(String, String)> {
    const GATE_MACHINERY: [&str; 3] = ["core/quality/", "tools/pathogens/", "docs/gates/"];
    let is_gate = |path: &str| GATE_MACHINERY.iter().any(|prefix| path.starts_with(prefix));
    let gate_side = changed_paths.iter().find(|path| is_gate(path))?;
    let code_side = changed_paths.iter().find(|path| !is_gate(path))?;
    Some(((*gate_side).to_owned(), (*code_side).to_owned()))
}
```

**Any path inside plus any path outside is a violation.** Not "gated code" — *anything*. A lockfile
counts.

Option 2 proposes landing the dev-dep in a non-gate PR first, then adopting in a gate-only PR. The
second PR must add a `[dev-dependencies]` line to **`tools/pathogens/Cargo.toml`** — and that file
is itself gate machinery, under the `tools/pathogens/` prefix.

**Does that alone move the lockfile?** Measured against the checked-in `Cargo.lock`: it records
dependency edges, path dependencies included.

```
[[package]]
name = "graphhelm-development-benchmark"
dependencies = [ "graphhelm-protocols", "graphhelm-runtime", "hex", "serde", … ]
```

So PR-2 touches `tools/pathogens/Cargo.toml` (inside) and `Cargo.lock` (outside) — **a violation
again.** Option 2 defers the helper crate's creation and leaves the adoption exactly where it was.

## 2. Option 4 works, verified against the ENFORCEMENT and not only the rule

`include!` is a compile-time file inclusion. **No `Cargo.toml`, therefore no `Cargo.lock`.** The
gate-side PR touches one file.

**Precedent in this repository, not invented here:**

```
tools/ci-canary/build.rs:9      include!("hashing.rs");
tools/ci-canary/src/lib.rs:23   include!("../hashing.rs");
```

One source, two compilation units, zero dependency edges.

**Confirmed as the orchestrator asked: the freeze prefix is `tools/pathogens/`, not `tools/`.** The
constant has exactly one authoritative definition (`core/quality/src/lib.rs:461`); `ci/gate.ps1`
carries no gate-machinery list of its own — grepped directly, because it is one of the known
parse-partial files. So a shared file under a non-gate `tools/` path is outside the machinery, and
`tools/acceptance-map`, `tools/ci-canary`, `tools/development-benchmark` and `tools/schema-parity`
already establish that `tools/` is not wholly gate.

**And the enforcement is real, which changes what "two PRs" has to mean.**
`core/quality/tests/freeze_enforced.rs` runs the check on the branch's diff against `main`'s merge
base, inside the workspace-test stage. Its header is worth quoting, because it is the same trap this
decision is walking around:

> *This test is the enforcement. It runs inside the workspace-test stage the gate already has, so
> wiring it needs no edit to `ci/gate.ps1` — which matters, because `ci/` is NOT gate machinery by
> the rule's own list, and adding the stage there would have made the enforcing commit violate the
> very rule it enforces.*

Since the check reads **the branch's** changed-path set, the split must be two branches, each with a
clean set:

| PR | touches | `gate_side` | `code_side` | verdict |
|---|---|---|---|---|
| **1** (non-gate) | the shared file under `tools/<name>/`, plus adoption in `apps/cli`, `core/events`, `core/graph` | none | present | clean |
| **2** (gate-only) | `tools/pathogens/tests/source_invariants.rs` **only** | present | none | clean |

Both halves return `None`. **This is the only one of the four options where that is true.**

> **SUPERSEDED by §11.1 and §12.** This table assumed the shared file was OUTSIDE the freeze. Review
> established that it must be inside — a predicate is not an input to a gate, it is the gate — which
> makes the split three PRs, not two. §12 has the measured table. This one is kept as the record of
> what was true before that, not as the plan.

## 3. The premise the issue rests on is already false: the twins have diverged

The issue says the two crates hold a **byte-identical** predicate. **They do not**, and each file
says it of the other.

| | `apps/cli` | `tools/pathogens` |
|---|---|---|
| shape | one function, `offending_literal` | **three**: `has_run_in_literal`, `is_exempt`, `offends` |
| exemptions | comments | comments **and `html:`** |

The `html:` exemption exists in one copy only, and it is principled rather than convenient: those
literals are the rendered-surface fixtures of the geometry pathogens, and `suite_digest` is a sha256
over the serialized suite, so tidying their whitespace would void every recorded geometry
certification with the corpus semantically unchanged.

**The comment claiming identity is what made the divergence silent.** Both files carry the sentence
*"a duplicated ORACLE diverges in silence"* — written directly above a divergence it did not
prevent. A reader who wants to know whether the twins agree reads the comment and stops.

**Recorded fairly: the claim was true when written.** The three-way split in `tools/pathogens` came
afterwards, and nobody updated the sibling. This is not a false statement authored in bad faith —
it is a statement that stopped being true and had no mechanism attached to notice. Which is the
argument for consolidation rather than for a better comment.

**This dictates the helper's shape, and it is the most useful thing the investigation produced.** A
shared helper cannot be one copy of the whole predicate, because the two crates legitimately differ.
It must split:

- **DETECTION — shared.** `has_run_in_literal`: three-or-more spaces inside an odd-index
  `split('"')` segment. Identical in both today, and the part whose false-positive bug reached two
  crates by copying.
- **EXEMPTION — per crate, by ROLE.** `apps/cli` exempts comments. `tools/pathogens` exempts
  comments and `html:`. A third crate will have its own.
- **COMPOSITION — per crate.** `offends = !is_exempt && has_run_in_literal`.

**`tools/pathogens` already discovered this shape.** Its three functions are exactly that separation,
with its own doc saying the exemption is *"by ROLE, not by file"*. The consolidation adopts the shape
the crate that needed it already found, rather than imposing one.

## 4. The population is defined by ROLE, which is the review's other half

The reviewer's finding on the shipped `apps/cli` guard: its population is `read_dir` over
`apps/cli/src`, which includes `#[cfg(test)]` inline modules — so programmer-facing assert messages
sit inside a guard whose declared reason is *"the operator reads this verbatim"*, which is false for
a `mod tests` message.

**Measured at `0339862`:** `apps/cli/src` holds **56** `.rs` files, **9** of which contain
`#[cfg(test)]` — the review named six and said "at least", so the count is higher than stated.
**Lines the predicate would catch inside those test regions: zero**, taking everything after the
first `#[cfg(test)]` marker as an upper bound on the region.

**So the over-coverage is REAL and LATENT.** It changes urgency, not design: there is no false
positive today, and there is a guard whose stated justification does not apply to part of what it
scans — which is the guard someone argues with the first time it fires on a test assert. The
population belongs defined by role:

- **exempt** data literals (the `html:` exemption, already role-based);
- **exclude** `#[cfg(test)]` inline modules — programmer-facing, not operator-facing;
- **cover** operator messages, which is the declared reason.

## 5. Why not 1 and not 3

**Option 1** (helper for non-gate crates, gate crates keep a copy with a twin-pointer) keeps a
divergence mechanism alive by design, and §3 shows what that mechanism has already produced. It also
institutionalises two populations of the same predicate, which is the thing the house rule about a
third crate exists to prevent.

**Option 3** (per-crate copies plus a census) is the status quo with bookkeeping. The census would
have to notice that two copies differ — and a census is a document, so §3 is the measurement of how
well a document holds this invariant.

## 6. What this blueprint does NOT establish

- **Nothing run.** Every claim is a read at `0339862`. No `cargo` has been executed for #323, and
  the machine is under a containment window at the time of writing.
- **`include!` across crate boundaries is not free of cost, and I have not paid it yet.** The
  included file is not a crate: it cannot carry its own `#[test]` items as a library, so the
  predicate's own tests must be included alongside it and will then run once per adopting crate.
  Whether that is acceptable or annoying is a judgement I have not tested against a real build.
- **The relative path in an `include!` is fragile in a way a dependency is not.** `ci-canary`'s
  precedent is intra-crate (`"hashing.rs"`, `"../hashing.rs"`); mine would be cross-crate via
  `env!("CARGO_MANIFEST_DIR")`. **That is a step beyond the precedent, and it is the part most
  likely to be wrong.** The first implementation red should be a build that proves the include
  resolves from two different crates.
- **I have not measured `core/events` and `core/graph`.** The issue says they were swept clean; I
  did not verify the sweep, nor count what their guards would scan.
- **The `#[cfg(test)]` exclusion has no design yet.** Excluding an inline module needs region
  detection, and the naive version — everything after the first marker — is the upper bound I used
  for MEASUREMENT, not a rule I would ship. A brace-matched region or an attribute-aware scan is a
  decision this blueprint defers.
- **One defect noticed and not fixed here, and it is a contradiction rather than a tidiness issue.**
  In `tools/pathogens`, lines 49–74 are one unbroken `///` block attached to `has_run_in_literal` —
  two doc comments merged with no gap between them. The halves disagree about the same function:

  ```
  51: /// **`html:` lines are exempt, and the exemption is principled rather than convenient.**
  62: /// Detection only -- no exemptions. Split from the role check so the exemption below can be
  ```

  The function implements the second. A reader checking whether detection carries exemptions finds
  both answers above one signature, and the first one they meet is the wrong one. In scope for the
  consolidation, named here so it is not rediscovered as a surprise — and it is a small instance of
  §3's shape: a comment that stopped describing its subject, with nothing attached to notice.

---

## 7. Populations measured, turning two sealed limits into data

Read-only, at `0339862`, walking each `src/` and applying the DETECTION half of the predicate
(three-or-more spaces in an odd-index `split('"')` segment, `//` lines skipped).

| crate | `.rs` files | with `#[cfg(test)]` | offenders | in a test region |
|---|---|---|---|---|
| `core/events` | 15 | 5 | **0** | 0 |
| `core/graph` | 11 | 1 | **0** | 0 |
| `apps/cli` | 56 | 9 | **0** | 0 |
| `tools/pathogens` | 4 | **0** | **2** | 0 |

**Three things follow, and one of them changes the design.**

**(a) The `html:` exemption is load-bearing, not defensive.** Both of `tools/pathogens`'s
offenders are `html:` fixture lines in `lib.rs` (301 and 336). Without the exemption that guard
is RED on its own crate. So the exemption is not a precaution against a hypothetical — it is the
only reason the guard passes today, which is the strongest possible case for keeping exemptions
per-crate rather than folding them into a shared predicate.

**(b) The `cfg(test)` question is an `apps/cli` question.** `tools/pathogens/src` has **zero**
`#[cfg(test)]` files, so the over-coverage the review found cannot occur there. It is 9 files in
`apps/cli` and 5 in `core/events`. That does not change the decision — population by ROLE is
still right — but it narrows where the exclusion has to be careful.

**(c) THE FLOOR CANNOT BE SHARED, and this is the design change.** `apps/cli`'s guard asserts
`found.len() >= 40`. `core/graph` has **11** source files and `tools/pathogens` has **4**. A
third crate copying the guard wholesale fails instantly — which is the good case, because it is
loud. **The bad case is the next step: someone lowers the number to fit.** A floor tuned until it
stops complaining is the vacuous pass the floor exists to prevent, arrived at by the door marked
"obviously this crate is smaller".

`apps/cli` already carries the better control beside the count — a NAMED FILE that must appear in
the walk (`wake_wait`). That one cannot be tuned quietly: lowering a threshold is a plausible
edit, deleting a named assertion is a visible one.

**So the split has a third part:** detection is shared, exemptions are per-crate by role, and **the
floor is per-crate and should be a named file rather than a count** — or a count *plus* a named
file, as `apps/cli` has it. The shared helper must not carry a number that only fits the crate it
was written in.

## 8. What §7 still does not establish

- **Zero offenders today is not zero risk.** `core/events` and `core/graph` are clean, which is
  what the sweep in #322 claims; this measures the same thing independently and agrees. It says
  nothing about what a future literal in either crate would need.
- **I did not look for data-literal roles in `core/events` or `core/graph`.** With zero offenders
  there is nothing to classify, so whether either would eventually need its own `html:`-style
  exemption is unknown rather than answered no.
- **The `cfg(test)` counts use the same upper bound as before** — first marker to end of file. For
  counting FILES that is exact; for attributing a line to a test region it is still an upper bound,
  and it reported zero either way.

---

## 9. `tests/` measured — and the design question answers itself in a third way

Requirement arrived from two lanes independently: the three guards scan only `CARGO_MANIFEST_DIR/src`,
so `tests/` is unguarded, and that is where the eight corrections of #329 and the thirteen of #332
lived. The question put to this lane: **`tests/` in the same population with exemptions, or a separate
population with its own named floor?**

**Measured at `0339862`, detection half only:**

| crate | `tests/` files | offenders |
|---|---|---|
| `apps/cli` | 32 | **19** |
| `tools/pathogens` | 7 | **13** |
| `core/execution` | 6 | **6** |
| `core/events` | 15 | **4** |
| `core/graph` | 4 | **1** |

**43 in `tests/` against 2 in `src/`.** The class does not merely recur there — it lives there.

### The answer is neither option, because the reason is wrong before the population is

Reading what the 43 actually ARE settles it. Almost all are the guarded defect itself, in test
assertion messages:

```
amend_budget.rs:192   "the next read forgot the bound the operator declared: the verdict flaps between
api_http.rs:3026      "a node whose silence could not be judged must survive even when another reason
attention.rs:363      "and no basis to assert CALM either -- the question is unanswerable, which is its ow
```

Trailing runs, exactly the continuation defect.

**And the house has already decided these count: #329 and #332 SWEPT them.** Twenty-one corrections,
in `tests/`, that nobody argued were out of class.

But the shipped guard's declared reason is *"nothing renders them except a human… the operator reads
this verbatim"* — and the review is right that **this is false for a `mod tests` message**. A
programmer reads it, not an operator.

**So the guard's stated reason is narrower than the class the house actually maintains.** Two
repairs are available and they point opposite ways:

- **narrow the population** to match the reason — exclude `cfg(test)` and `tests/`, and accept that
  21 corrections the house made were out of scope;
- **restate the reason** to match the sweeps — and the population follows from it.

**The second is right, and the evidence is the sweeps themselves.** Nobody swept `tests/` and then
argued the corrections did not belong. The class was never "operator strings"; that was a narrower
label placed on it by the first crate to need a guard, whose own messages happened to be operator
messages.

**The class, restated:** *strings a human reads VERBATIM when something has already gone wrong.* That
covers operator messages, test failure messages, and gate refusal findings — every string whose
reader is a person diagnosing, and whose collapsed whitespace costs them a second failure to
understand. It excludes data literals, where formatting is the subject rather than the presentation.

Under that reason the review's finding still holds and lands differently: the `apps/cli` guard's
justification was mis-stated, not its population mis-drawn. Its 9 `cfg(test)` files are IN class.

### Which makes the exemption the whole design, and there are now three species

Data literals were one species (`html:`) when this blueprint was written. There are three:

1. **`html:` fixtures** — `tools/pathogens/src/lib.rs`. Feed `suite_digest`; tidying voids recorded
   certifications.
2. **Canonical JSON under test** — `apps/cli/tests/schema_cli.rs:1298`, `"{
  "a": {
    "x": 3…`.
   The FORMATTING is the thing under test. Sweeping it leaves the test green and empty: the canonical
   digest no longer proves what it was written to prove, and nothing says so.
3. **The guard's own predicate fixtures** — and this is the one that makes the population question
   urgent rather than tidy. `apps/cli/tests/source_invariants.rs:142` is
   `offending_literal(r#"    "a real defect with          collapsed indent","#)`. Extending the
   population to `tests/` **makes the guard flag itself**: 5 offenders in its own file in `apps/cli`,
   6 in `tools/pathogens`, 2 in `core/execution`. **Thirteen of the 43 are the guards' own fixtures.**

Species 3 cannot be exempted by FILE — "skip `source_invariants.rs`" would blind the guard to a
genuine defect in the guard's own messages, of which that file has several. It is exempt by ROLE:
**a literal passed as an ARGUMENT to the predicate under test is data**, the same way an `html:` line
is. That is a narrower rule than a filename and it is the only one that keeps the file scanned.

### The answer to the question as asked

**One population, extended to `src/` and `tests/`, with exemptions by role** — not a separate
population.

A separate population would need its own floor, its own adoption, and its own exemption list, and the
two lists would be nearly identical: the data-literal roles do not care which directory they sit in.
**Two populations is two things to keep in step, which is the divergence this whole issue exists to
end.** The three-part split already carries what a wider population needs: shared detection, per-crate
exemptions by role, per-crate named floor.

**The floor is the one thing that must grow a second entry.** A crate now has two roots, and a walk
that silently lost one of them would pass. Each crate names one landmark file **per root** — a `src/`
landmark and a `tests/` landmark — for the reason §7 gives: lowering a threshold is a plausible edit,
deleting a named assertion is a visible one.

### What §9 does not establish

- **I classified the 43 by reading them, not by a rule a machine applied.** The counts are measured;
  "almost all are the defect itself" is my reading of the lines, and a reader who disagrees about a
  particular line has the file and the line number.
- **I did not check whether the #329 and #332 corrections are the same 21 lines** the sweep touched.
  The two lanes reported those counts; I measured today's remaining offenders, which is a different
  question and a different moment.
- **The role rule for species 3 is stated, not implemented.** "An argument to the predicate under
  test" is easy to say and needs a real definition — probably that the line calls the predicate by
  name. Whether that is sound against a fixture built in a `let` binding first is unmeasured, and it
  is the first thing the implementation should redden.

---

## 10. Implementation order, with the two reds that come before any predicate

Written out so the sequence is a decision on the record rather than something the implementer
reconstructs.

**PR-1, non-gate.** Creates the shared detection file under a non-gate `tools/` path and adopts it in
`apps/cli`, `core/events`, `core/graph`. Touches nothing under `core/quality/`, `tools/pathogens/` or
`docs/gates/`, so `freeze_violation` finds no gate side.

**PR-2, gate-only.** `tools/pathogens/tests/source_invariants.rs` and nothing else. No `Cargo.toml`,
therefore no `Cargo.lock`, therefore no code side.

### The first red is not about the predicate

**RED 1 — the `include!` resolves from two different crates.** This blueprint's chosen option rests on
a cross-crate `include!` via `CARGO_MANIFEST_DIR`, and the precedent it leans on (`tools/ci-canary`)
is INTRA-crate. That is a step beyond the precedent and the likeliest thing here to be wrong. It must
be a build that fails before the file exists and passes after, from two crates — **not** a predicate
test that happens to compile.

**RED 2 — a fixture built in a `let` before the call is still exempt.** §9's role rule for the guards'
own fixtures says *"a literal passed as an argument to the predicate under test is data"*. The obvious
implementation matches a line that calls the predicate by name. That is defeated by:

```rust
let fixture = r#"    "a real defect with          collapsed indent","#;
assert!(offending_literal(fixture));
```

The literal is on a line that names nothing. Whether the rule survives this is unmeasured, and it is
the difference between an exemption by role and an exemption by line shape. If it does not survive,
the rule needs restating before any crate adopts it — a role exemption that only works when the code
is written one way is a line-shape exemption wearing a better name.

### Doc repairs that ship with the consolidation, not after

Both are comment-claims already known false, and both are in the files the consolidation rewrites:

1. **The declared reason.** *"nothing renders them except a human… the operator reads this verbatim"*
   is the narrower label §9 replaces. It must be rewritten to the restated class in the same change
   that widens the population, or the guard ships with a justification its own scope contradicts —
   which is the condition the review found, preserved rather than fixed.
2. **The contradictory block** at `tools/pathogens/tests/source_invariants.rs:49-74`, where one
   unbroken doc block promises the `html:` exemption and then says "detection only, no exemptions"
   about the same function.

3. **The twin-pointer sentences.** Both files claim the other holds a byte-identical predicate. §3
   measured that false. They die with the copies they point at — but only if the consolidation
   actually removes them, so they are listed here rather than assumed.

---

## 11. Review delta — four amendments, layered rather than rewritten

Review found four. The first is blocking and I had missed it in a way worth naming: **§2 verified
that the two migration PRs pass the freeze, and never asked what the freeze permits AFTERWARDS.** I
checked the migration and called it the design. The steady state is the design.

### 11.1 BLOCKING — the shared file must be inside the freeze, because a predicate IS what the gate is

As proposed, the shared detection file sits under a non-gate `tools/` path. That is the mechanism and
it is also the hole:

> A later PR edits the shared detection file **and** gated code. Every path in it is non-gate, so
> `freeze_violation` returns `None` and the branch is clean — while that PR changed what the
> `tools/pathogens` guard DETECTS in the same breath as the code that guard judges.

The judge and the judged travelled together, and the check waved them through, because the judge had
been moved somewhere the check does not look. **A predicate is not an input to a gate; it is the
gate.**

**Direction taken: the shared prefix enters `GATE_MACHINERY`.** Adoption by `include!` does not edit
the shared file, so this does not recreate the blocker option 2 died of — **that blocker was a
dependency EDGE in the root lockfile, not a path prefix**, and the two are unrelated.

**The chicken-and-egg this creates, measured rather than assumed** — as the freeze rule itself was.

Adding the prefix means editing `core/quality/src/lib.rs`, which is gate. Creating the file under a
prefix that is not yet frozen, in that same PR, could go either way depending on which list is in
force at evaluation time.

**It is the branch's own list.** `core/quality/tests/freeze_enforced.rs:20` imports
`graphhelm_quality::freeze_violation`, and `GATE_MACHINERY` is a `const` **inside that function**
(`core/quality/src/lib.rs:461`). The test binary is compiled from the branch under test, so a branch
that edits the list is judged by the list it edited.

So one PR can do both:

| path | gate under the branch's own list? |
|---|---|
| `core/quality/src/lib.rs` (adds the prefix) | yes, always |
| `tools/<shared>/detect.rs` (created) | **yes — under the list this same diff installs** |

`code_side` is `None`. Clean. **The egg and the chicken are in the same basket and the basket is
gate.**

**Stated as a limit rather than glossed:** this is measured by READING the mechanism — which function
is called, where the constant lives, what compiles it — not by running a branch through it. Running
it needs the branch to exist. **RED 0 for the implementation is therefore this PR shape passing
`freeze_enforced`**, before RED 1's include! build. If it fails, the sequencing splits and §11.1 is
wrong about the order, not about the direction.

**And an observation this exposed, out of scope and recorded so it is not lost:** the freeze's own
definition is editable by the branch it judges. Removing a prefix and touching that path in one PR
still self-catches (the `core/quality` edit is gate, the freed path is not). Removing a prefix and
touching nothing else is clean, and a later PR then moves freely. That is a two-PR bypass of the
same shape as any two-PR split, and it is not this issue's to close.

### 11.2 The declared population is PREVENTIVE and has never contained a defect

Named plainly because the blueprint implied the opposite by defending the population so carefully.

Across the four crates measured: **zero true positives in live `src/`** — the two in
`tools/pathogens/src` are the `html:` fixtures the exemption exists for. Every known instance of the
class lived in TEST text: the eight of #329, the thirteen of #332, the proof case of #322.

**So the population the guards currently declare is the COMPLEMENT of where the defect has ever
been.** That does not make it worthless — a guard that has never fired is not thereby wrong — but it
must not be described as though it has been holding a line. It has been preventive, and the
extension to `tests/` is what would make it retrospective.

If the `tests/` extension lands in a later phase rather than with the consolidation, **that phase
gets a trigger under §11.4** rather than a place in a queue.

### 11.3 The floor is a count PER CRATE and a named landmark — both, not either

§7 argued the named landmark is the better control and left the count as optional. Review measured
that both existing floors already tolerate silent shrinkage: `apps/cli` asserts `>= 40` against 56
real files, so **sixteen can vanish quietly**; `tools/pathogens` asserts `>= 3` against 4.

And a landmark alone is **mute about a file ADDED** — the exact property whose loss made the guard
abandon `include_str!` lists in the first place.

**Decided: both.** A per-crate count, tightened to the real number rather than left slack, and a
named landmark per root. The shared helper still carries **no number at all** — the numbers live in
each adopting crate, which is what §7's argument was actually about.

### 11.4 Every deferral carries a trigger independent of the error

The `cfg(test)` exclusion is deferred in this blueprint. A deferral whose wake-up condition is
"when someone hits the defect" is not deferred, it is abandoned — the note's own delivery condition
is the failure it exists to prevent.

**Trigger for the `cfg(test)` exclusion:** the THIRD crate to adopt the shared helper, **or** the
first `cfg(test)` offender to appear in any census — whichever comes first. Both are events someone
is obliged to notice while doing something else, which is the property that separates a live
deferral from a dead one.

---

## 12. RED 0 and RED 1, run — and the sequence is three PRs, not two

Everything in §11 was reasoned from reading the mechanism. This section is what happened when the
reading was put on a machine. Base for every cell: `origin/main` at `2760565`, worktree
`D:/gh-d-red0`, serial slot held. **Nothing here is merged; the probe branches are measurement, not
work.**

Three controls on every cell, because a cargo failure and a cargo non-compile are both exit 101:
the mutation LANDED (grep a marker before running), it BUILT (`^error\[E` count at zero), and WHICH
cell moved (per-test lines, never the exit code).

### RED 0 — one PR can add the prefix and create the file under it

| cell | diff | `error[E` | verdict |
|---|---|---|---|
| **control** | `core/quality/src/lib.rs` + `tools/elsewhere-not-listed/detect.rs` | 0 | **FAILED** at `freeze_enforced.rs:107`, naming the pair |
| **subject** | `core/quality/src/lib.rs` + `tools/source-invariants/detect.rs` | 0 | **ok** — 1 passed |

The two diffs are identical but for one path. The control is the positive control: it proves the
test reads THIS branch's changed set and will speak — without it, the subject's green is
indistinguishable from a test that never ran.

**§11.1 confirmed: a branch that edits `GATE_MACHINERY` is judged by the list it edited.** The
reading was right, and now it is a run rather than a reading.

### RED 1 — `include!` resolves cross-crate, from two crates, and it costs two E0753s

Shape adopted: `include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/source-invariants/detect.rs"))`.

| cell | `apps/cli` | `core/events` |
|---|---|---|
| shared file **absent** | `error: couldn't read …detect.rs` | `error: couldn't read …detect.rs` |
| shared file **present** | ok, **3 passed** | ok, **1 passed** |

`apps/cli` runs its three existing tests against the SHARED predicate — its local copy was deleted,
not renamed, so a failure to resolve could not hide behind a leftover definition. `core/events` is a
crate that had no such guard at all, which is what makes it the second crate rather than a second
copy of the first.

**Two things this cost, both found by failing rather than by planning:**

1. **The include cannot go at the top of the adopting file.** Placed above the leading `//!` block it
   is `E0753: expected outer doc comment`. It goes after the module docs.
2. **The included file cannot carry `//!` comments at all** — it is spliced into the MIDDLE of the
   including file, so an inner doc comment is `E0753` in every adopting crate at once. The shared
   file documents itself with `//`. This is a real cost of `include!` over a crate dependency: **the
   shared file cannot have a module header**, and the reason is now written in the file so the next
   person does not restore it.

### The sequence is THREE PRs — §2's table is superseded

§2 planned two PRs on the assumption that the shared file was non-gate. §11.1 moved it inside the
freeze, and that changes the split. **Measured, not derived:** a branch carrying the prefix, the
shared file, AND the adoption in `apps/cli` and `core/events` **FAILS** — `Some(("core/quality/src/lib.rs",
"apps/cli/tests/source_invariants.rs"))`. Adoption cannot ride with creation.

| PR | touches | gate side | code side | verdict |
|---|---|---|---|---|
| **A** | `core/quality/src/lib.rs` (adds the prefix) + `tools/source-invariants/detect.rs` (creates it) | both | none | **measured clean** |
| **B** | adoption in `apps/cli`, `core/events`, `core/graph` | none | present | clean |
| **C** | adoption in `tools/pathogens/tests/source_invariants.rs` | present | none | clean |

**§2's two-row table is superseded by this one.** It is left in place rather than rewritten, because
it records what was true before the shared file entered the freeze — but it must not be read as the
plan.

### What §12 does not establish, and one instrument I broke

### The ordering rule the freeze forces: ACCOMMODATE FIRST, CHANGE THE JUDGE LATER

This table is a special case of something that governs every day, not just this issue, and it is
worth writing beside the table rather than left for each lane to rediscover.

**The freeze turns any change to what the predicate DETECTS into a two-PR sequence, and the naive
order leaves `main` RED in between.** The gate-only PR tightens the predicate; the crates it now
flags are non-gate and cannot be corrected in the same branch — that is the rule working, not a
bug in it. So the correction cannot travel with its own cause, and between the two merges every
lane sees a red tree for a defect none of them introduced.

**The remedy is to reverse the order, and it costs nothing:**

> Land the CRATE-side PR first, while it is still a **no-op under the predicate as it stands today**.
> Then the gate-only PR lands into a tree that already tolerates the tightening.

Same two PRs, same total work, **zero red window**. The crate-side change has to be legal under both
the old predicate and the new one, which it is whenever the change is a correction rather than a
rewrite — corrected text passes the loose predicate too.

**And there is a cheap escape hatch that this issue used.** Before assuming a sequence is needed,
DIFFERENCE the two predicates over the corpus. The escaped-quote repair was differenced over all 348
`.rs` files and changed no line's verdict, so nothing needed accommodating and the tightening could
travel alone. **The measurement is what turns a two-PR dance into one PR** — and skipping it means
paying for accommodation you may not owe.

(The rule is L's; the writing is mine.)

- **PR-B and PR-C are derived, not measured.** Only PR-A and the combined-branch violation were run.
  B and C follow from the same function on path sets I have not put through it.
- **The `"/../../"` in the include string assumes the adopting crate sits exactly two levels below
  the repo root** — true of every current workspace member (`core/*`, `apps/cli`, `adapters/*`,
  `tools/*`). **CORRECTED (L): this fails LOUD, not silently, and the first row of RED 1's own table
  above is the proof.** A path that does not resolve is `error: couldn't read …detect.rs` — a hard
  compile error, in the adopting crate, at the moment of adoption, shown to the person adopting.
  That is the best failure mode available, and the residual is one sentence rather than a depth
  helper: a wrong depth would resolve silently only if a *different* `tools/source-invariants/detect.rs`
  existed under some other ancestor, which the frozen prefix makes a non-event.

  **The mis-statement is worth keeping visible, because sealing is itself a claim.** I wrote
  "silently wrong" without checking it against a measurement I had already taken and printed two
  paragraphs earlier. An OVERSTATED residual is not a safe error: it reads as needing a remedy, and
  the remedy it invites here — a depth-aware include helper — is machinery bought against a failure
  that already announces itself.
- **A freeze verdict is a property of a MOMENT.** Mid-measurement `origin/main` advanced from
  `2760565` to `fbc1be7`. I checked that the new tip descends from my base and does not touch
  `GATE_MACHINERY`, and re-ran RED 0's subject against the moved ref — still green. Recorded because
  the next person will re-run these and get a different base.
- **I broke my own instrument and the enforcement caught me.** I read the change set with
  `git diff --name-only origin/main HEAD` and got **15 paths**, eleven of them another lane's
  `core/extension-host` work. The refusal message said **"over 4 changed path(s)"**. The refusal was
  right: a two-dot diff against a MOVING ref reports everything that differs between two tips,
  including what main has and the branch lacks. **The change set is the diff from the MERGE BASE**,
  which is what `changed_paths()` uses and what I did not. Had the discrepancy not been printed in
  the failure text, I would have reported a contaminated path list as a measurement.

---

## 13. The family is EIGHT, the hand-named lists are already stale, and the role rule fails 4/4

Two inputs arrived from the L lane. Both are re-measured here rather than relayed, at `24bb6c5`.

### 13.1 Eight guards, not three — and five of them are the shape this issue exists to replace

| guard | walks `src/` | names files by hand |
|---|---|---|
| `apps/cli` | yes | — |
| `tools/pathogens` | yes | — |
| `core/execution` | yes (`read_dir`, plus three named landmarks) | — |
| `adapters/postgres-event-store` | — | **yes** |
| `core/gateway` | — | **yes** |
| `core/governor` | — | **yes** |
| `core/runtime` | — | **yes** |
| `core/tool-broker` | — | **yes** |

Confirmed. Every earlier section of this blueprint that says "three guards" or reasons about four
crates is **underpriced**, and the five hand-named ones are literally the `include_str!`-list form
that `apps/cli` abandoned and that §7 quotes as the weaker half of the trade.

**And L's conclusion survives the correction: none of the eight reaches `tests/`.** Measured
independently — the only two `tests` hits are in `core/governor`, and they are the string `0 tests`
in a message, not a path.

### 13.2 The hand-named lists are ALREADY STALE — twenty files, measured

This is the part that changes the issue from tidying to repair. Matching named basenames against the
`.rs` files actually present (no basename collisions in any of the five trees, checked):

| crate | named | present | **unscanned** |
|---|---|---|---|
| `core/governor` | 1 | 8 | **7** — incl. `lib.rs` |
| `adapters/postgres-event-store` | 5 | 11 | **6** — incl. `lib.rs` |
| `core/runtime` | 7 | 13 | **6** |
| `core/tool-broker` | 6 | 7 | **1** |
| `core/gateway` | 5 | 5 | 0 |

**Twenty files carry a `source_invariants` guard in their crate and are not scanned by it.**

**The dated instance, because a rate would hide it.** `core/tool-broker/src/mcp_capability.rs` was
added by `6b0b058` (#307). The guard `core/tool-broker/tests/source_invariants.rs` has not been
touched since `9d15bf4` (#47) — the crate's founding milestone. A file was ADDED and the guard did
not grow, and nothing anywhere went red. That is not a prediction about what a hand-list might do;
it is the failure in the archive with two commit hashes on it.

**This revises §11.2 and I would rather say so than let the two coexist.** §11.2 said the declared
population is preventive and has never contained a defect. That is still true of the DEFECT CLASS —
zero collapsed-indent true positives in live `src/`. It is **not** true of COVERAGE: five of the
eight guards have a live hole today, and the hole is exactly "a file was added". §11.2's claim was
about the defect; it should not be read as a claim about the guards' reach.

### 13.3 Species 3 enumerated — and the role rule cannot be implemented by line shape

Four addresses, all four read and confirmed:

| address | literal |
|---|---|
| `apps/cli/tests/schema_cli.rs:1298` | the canonical-JSON sample (`"{\n  \"a\": {…`) |
| `core/execution/tests/source_invariants.rs:196` | a bare code sample on a continuation line |
| `core/gateway/tests/source_invariants.rs:165` | `let sample = "// mentions rand and std::fs…` |
| `core/tool-broker/tests/source_invariants.rs:169` | `let sample = "// mentions rand and std::fs…` |

**The decisive measurement.** §9 exempted species 3 by ROLE — *"a literal passed as an ARGUMENT to
the predicate under test is data"* — and §10 sealed the worry that the obvious implementation
matches the line that NAMES the predicate, defeated by a fixture bound in a `let` first. That worry
is no longer a worry:

> **The predicate name appears on the same line as the literal in ZERO of the four.**

Two are `let sample = …` — precisely the shape RED 2 was written to catch. The other two are bare
literals on continuation lines. **A line-shape implementation of the role exemption would fail on
100% of the real instances**, which means it is not a weaker version of the rule, it is not the rule
at all.

**Direction, marked as a proposal and not a measurement:** the exemption becomes an explicit
per-site marker — a comment on the literal or its enclosing block declaring it a detection fixture —
rather than anything inferred from line shape. That fits the discipline the rest of this blueprint
already argues for: **removing a named marker is a visible edit, and a fixture that must announce
itself cannot be created by accident.** RED 2 is now written FIRST and is expected to be red, because
we already know what it will say.

### 13.4 The count that did not reconcile

The input named **five** data literals and gave **four** addresses. I verified the four and swept the
repository for the sample string myself: three copies (`core/execution`, `core/gateway`,
`core/tool-broker`) plus the canonical-JSON one. **I did not find a fifth and I am not going to
supply one.** If a fifth exists, it is outside the sample-string sweep and needs its own address.

### 13.5 The floor-lowering rule (adopted)

§11.3 tightens each per-crate count to the real number. That has a cost: **every legitimate file
removal now becomes an edit to the floor**, which is the plausible-looking edit the floor exists to
resist. The rule that goes on the line beside the count:

> *Lower this only in the same commit as the removal that caused it, and name the removed file.*

Landmark and count then fail on different work, which is the whole reason for keeping both.

### 13.6 Two housekeeping notes

- **The §2 pointer L asked for is already in.** It landed in `ed41951`/`4c8dec1` — §2's table carries
  a `SUPERSEDED by §11.1 and §12` blockquote, and §12 holds the measured three-PR table. Recording
  the coordinate rather than doing it twice.
- **None of this moves RED 0.** The shape measured in §12 is unchanged. What grew is the adoption
  population for PR-B, which now has eight guards in view rather than three — and five of them want
  the walk more than the other three did.

---

## 14. The sequence is no longer derived, and PR-A is open

`freeze_violation` is a pure function over `&[&str]`. §12 left PR-B and PR-C as derived because I was
thinking in branches; they never needed branches. Their path sets were passed to the function
directly, against the four-entry list PR-A installs.

| set | paths | verdict |
|---|---|---|
| **B1** | `apps/cli`, `core/execution` guards — the two NON-gate walkers | `None` |
| **B2** | the five hand-named crates | `None` |
| **B1+B2** | both, as one PR | `None` |
| **C** | `tools/pathogens` guard alone | `None` |
| *positive control* | shared file + adoption travelling together | **`Some`** |

The control is in the same test for the reason §12 gives about RED 0: a wrong call site would make
every clean verdict above uninterpretable. It was temporary and is not in PR-A — it asserts about
future PRs and goes stale the moment they land.

**`tools/pathogens` is the third walker and is gate machinery**, so B1 is two crates, not three. §12's
row saying "adoption in `apps/cli`, `core/events`, `core/graph`" predates the census in §13 and names
crates that have no guard; the real adopter set is the eight that exist.

### PR-A: https://github.com/stabem/GraphHelm/pull/350

Three gate paths. `graphhelm-quality` fully green, `freeze_enforced` included. Both new predicate
tests were made to fail first — wrong but legal mutations, zero build errors, one cell each and the
right one both times.

**And one trap the branch walked into and out of, recorded because it will recur.** `cargo fmt --all`
reformats the WORKSPACE. It rewrote `core/events/tests/held_file_contention.rs` — a file this branch
has nothing to do with — and `git add -A` committed it. On a gate-only branch that is not untidy, it
is a **freeze violation**: `core/events/` is not gate machinery, so the branch acquires a code side
and is refused. Caught only because `git status --porcelain` was printed before the push.

The underlying unformatted file is pre-existing on `main` from `e433b7b` (#340) and is now
https://github.com/stabem/GraphHelm/issues/352 — not fixed here, because PR-A cannot carry a
`core/events/` path without violating the rule it is extending.

---

## 15. The count that would not reconcile was a defect in the predicate

§13.4 recorded a number that did not add up — five literals claimed, four addresses given — and left
it open rather than reconciling it by guesswork. L reconciled it: **five HITS over four ADDRESSES**,
one line hit twice.

**The reason it hit twice is a flaw in the predicate itself, which makes this the most valuable
thing the discrepancy could have been.** `split('"')` treats every `"` as a literal boundary,
including an ESCAPED one, so a JSON fixture full of `\"` is chopped into many segments and several
of them read as literal bodies.

### Measured before writing anything, and it fails in BOTH directions

| line | truth | predicate |
|---|---|---|
| `let s = "he said \" and then          waited";` | run IS inside a literal | **missed** — the run lands on an even index |
| `let s = "x\"";          let t = 1;` | run is in CODE | **reported** — the run lands on an odd index |

A single escaped quote shifts every following segment's parity by one. Refusal and operator messages
quote things, so this is the common case and not a corner.

**The mirror is the part worth naming as method.** The false negative was the direction I went
looking for; the false positive is its reflection and I only found it because I wrote the parity out
rather than testing the case I suspected. A mis-tokenizer never breaks one way.

### The repair, and why it landed with the consolidation

Drop `\"` before splitting — one call. Differenced over every `.rs` file in the repository:
**348 files, 51 of them containing an escaped quote across 169 lines, and NOT ONE line changes
verdict.**

The zero carries both controls: the differ reports non-zero on synthetic cases, so it can speak; and
the subject exists in quantity, so it had something to speak about. A correctness repair that creates
no corpus work rather than a widening that hides one.

It landed in PR-A rather than after because the file is new and nothing has adopted it — blast radius
zero today. Shipping the new shared authority with a known blind spot would have handed the same
defect to all eight guards and invalidated the census in §13.

### Two consequences for the rest of this issue

- **A hit count from this predicate is not a site count**, and neither converts to the other without
  re-deriving. That applies to any census reusing these numbers.
- **§13.5's B2 price survives, and here is why rather than by assumption.** It was measured with the
  BROKEN predicate. The whole-repository difference between broken and fixed is zero lines, and those
  348 files include all 44 in the five hand-named crates. So the twenty-unscanned zero and the
  twenty-four-scanned zero both stand.

### What this section does not establish

- **The fix is measured neutral TODAY.** It is a semantic change to a gate predicate, so a line
  written tomorrow containing an escaped quote may be judged differently than it would have been —
  which is the point, and is not the same as no effect.
- **I did not re-derive L's five-over-four myself.** I measured four addresses, verified them, and
  swept the sample string; the hit/address reconciliation is L's and I am relaying it as theirs.

---

## 16. What actually shipped, and where this document is wrong

Written at the close so the record does not preserve superseded claims as though they were the
outcome. #323 closed on 2026-08-25; five pull requests landed.

| PR | what it did |
|---|---|
| **#350** `023a44b` | the shared predicate created, its path frozen, a compile carrier added |
| **#355** `a909758` | the predicate mis-read escapes in BOTH directions — repaired by consuming escape pairs |
| **#361** `923281c` | the frozen prefix bound to the file's real location |
| **#368** `51b9456` | `apps/cli` adopts, local copy deleted, floor at the real count |
| **#369** `8f5b353` | `tools/pathogens` adopts, a fixture that could not fail replaced |

Verified on `main` rather than reported: **one copy of the predicate**, zero local copies in either
crate, floors at 57 and 4.

### The three places this document is wrong, named rather than edited away

1. **§13.1 counted eight guards and treated them as one family.** They are not. Only `apps/cli` and
   `tools/pathogens` ever held this predicate; the other six share the FILENAME
   `tests/source_invariants.rs` and assert unrelated things — dependency purity, clocks, RLS, key
   vocabularies. I grouped by filename and read it as a guard family. **The test names said so the
   whole time** and I had already printed them.
2. **§13.2's twenty unscanned files are real but belong elsewhere.** They are a coverage hole in
   those OTHER guards, not in this one. Rehoused as #363 **before** the scope was corrected —
   otherwise the finding would have died with the correction that displaced it.
3. **§12's "B2" does not exist**, and §13.5's B2 price was measured in the wrong unit. The question
   those five guards ask is dependencies and clocks, not runs of spaces; a count of collapsed-indent
   offenders is not their cost.

### Two things this document got right for the wrong reason, which is worth as much

- **§12's zero-verdict-change differential** was offered as evidence the escape repair was correct.
  It was not: the substring version and the pair version disagree on constructible input and agree on
  every line this repository holds. **A corpus difference measures what the corpus CONTAINS, never
  what the predicate DOES.** The differential licenses the ORDER (no red window), never the mechanism.
- **§11.1's seal** said the raw-string limit was pre-existing and "nothing here makes that better".
  Measured later: in one narrow shape the new predicate is strictly WORSE than the one it replaced.
  Narrow, zero live instances — but narrow and measured is not neutral.

### What the freeze rule did to its own author, twice

§11.1 argued the shared predicate had to sit INSIDE `GATE_MACHINERY` because a predicate is not an
input to a gate, it IS the gate. Having put it there:

- a review clause could no longer travel with the `apps/cli` adoption, and `freeze_violation` named
  the offending pair when asked;
- the combined creation-plus-adoption branch was refused, which is what settled the sequence into
  three pull requests rather than two.

**The mechanism worked against the person who hardened it, on the same day, twice.** That is the only
real evidence a rule is not decorative.

