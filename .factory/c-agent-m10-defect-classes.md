# M10 close doc — defect classes

Draft section. Proposed by B; drafted by C, who holds first-hand evidence for two of the three
classes and marks the rest as reported-not-verified.

**Why a defect-class section exists at all.** #81 taught the lesson at the level of tests: registering
flaky *sites* one at a time never converges, because the defect belongs to the shape and the victim
rotates. The same is true of defects. Three shapes recurred across M09/M10 lanes in different
subsystems, found by different people who were not looking for them. Naming them turns
"we fixed a bug" into "we know what to look for next time", which is the only version that
compounds.

**Citation discipline.** Every claim below either cites its issue and evidence file, or is marked
**NOT MEASURED**. Nothing is asserted from memory.

---

## Class 1 — Flattening

**A boundary maps distinct causes onto one legal, well-formed value, and the consumer needs exactly
the distinction that was destroyed.**

Not a swallowed error: every surface stays calm and the value is in range. The loss is the
*distinction*. The common tell is **one value, two causes, opposite responses.**

**But the value COUNT is not the class.** #81's census (1.1) shows the same defect living as *three
values, none carrying the dimension the consumer needs* — timing, in that case. **A flattening can
hide behind apparent variety**, and the variety is what makes it survive: several distinct-looking
errors read as a considered taxonomy rather than a lost distinction. The invariant is the loss of
**the distinction the consumer needs**, whatever the arity. (The shared flattening memory already
says this dimensionally; the close doc must not re-narrow it to a value count.)

### Sightings

**1.1 — timing laundered into verdicts-about-the-data (#81).** The PostgreSQL backup/restore path
bounds many steps on fixed timeouts, and **not one of the elapsed paths named timing.**

**Corrected to the measured grain, and the correction makes it a stronger sighting.** My first draft
said "maps every elapsed step to `InvalidRestore`" — the ISSUE'S original claim, cited from an
evidence file that predates B's census. Their #81 lane measured it and it was **understated**:
elapsed scattered across **four variants and three codes** — `InvalidBackup` and `Unavailable`
(GHB001), `InvalidRestore` (GHB002), `LimitExceeded` (GHE006). Verified by me from the merged fix
commit `87d8ebd`, whose removals show all four on elapsed paths; site-by-site table in
`.factory/b-agent-81-analysis.md` §1 and PR #121.

**Why the scatter is worse than the uniformity I originally described:** a consistent one-value
flattening is grep-findable — one string, one census, one fix. **A scatter means no single grep
finds the policy.** Every site looks locally reasonable, and the defect is only visible when someone
asks "which of these means *the machine was slow*?" — which is exactly why registering sites one at
a time never converges, this section's own opening thesis. **As first written, 1.1 undersold the
very argument it was cited to support.**

Operator responses remain opposite: *retry on a quieter machine* vs *never trust this backup*.
Fixed in #121 — a step that runs out of time now says so, and the operation has one budget.

**1.2 — `GHCLI016_DRIVER_FAILURE` (#96).** After #83's fix, one code covers a setup failure (nothing
committed, the operator's hold intact) and a mid-drive failure (the resume genuinely happened, then
the work failed). Opposite hold-states. Evidence: issue #96; PR #99.

**Note against ourselves: #83's fix CREATED this divergence.** Before it, both classes had the resume
committed, so "did my hold survive" answered NO either way — ambiguous about cause, not about
consequence. Recorded because the tempting write-up omits it.

**1.3 — the reader-level case (#83's fixture selection).** `manual-override-deploy.yaml` is unusable
for the async drive path because a node **is** `NodeType::Deploy`; the M09 release graph has a node
**named** `deploy` whose **type** is `tool`, and it is the correct fixture. The word "deploy" fused
two node types **in the reader**. Worse, the obvious check misleads too: `grep type:` on that YAML
returns edge types mixed in with node types. Evidence:
`.factory/c-agent-83-sealed-predictions.md` (Amendment 2); PR #99's test header.

**Keep 1.3 in the list precisely because it is not in the code.** The same failure mode operates on
whoever is reading, which is why "audit the values" does not catch all of it — and being freshly
bitten by a real `Deploy` node primes you to reject the one correct fixture in the tree.

### Repair

Widen the value so the cause survives the boundary — a distinct variant, an error kind, a caller
tag. **After fixing one, ask what the caller now sees for each class you just separated**: separating
causes internally while the response still collapses them moves the defect up one altitude rather
than curing it. That is exactly how 1.2 came to exist.

---

## Class 2 — Fabricated success

**A failure path answers with a success the operation never achieved.**

Related to flattening but distinct, and the distinction is worth keeping: flattening makes an answer
**ambiguous**; a fabricated success makes it **positively wrong and reassuring**. Ambiguity prompts a
second look. Reassurance ends the investigation.

### Sighting

**2.1 — the same-key retry (#83).** Pre-fix, a refused resume still committed, and the committed
event carried the caller's `Idempotency-Key`. A client meeting the 500 retries the identical request
— which is precisely what an idempotency key exists to permit — and the retry never executes: the
idempotency layer replays the committed half-state. Measured reply:

```json
{"command":"execution.resume","data":{"status":"running", ...},"diagnostics":[],"ok":true}
```

`ok: true`, HTTP 200, no diagnostics. One defect, three answers:

| attempt | answer | what the operator concludes |
|---|---|---|
| first call | 500 `GHCLI016` | it failed, my hold stands |
| retry, different key | 409 `GHCLI005` not_paused | my hold never existed |
| retry, **same key** | **200 `ok:true`, running** | it worked after all |

Evidence: issues/83#issuecomment-5348037413 (raw reply); guard
`the_same_idempotency_key_after_a_failed_setup_still_executes`; PR #99 ledger row iii.

**Why it ranks worst of the three:** it is the only one that reads as *fine*. A failure and a refusal
both send a human looking; a fabricated success closes the ticket. And it is the branch a
**well-behaved automated caller reaches by default**, because retrying with the same key is correct
client behaviour.

### Repair

Any path that can answer success must be the path that actually did the work. Where a replay layer
sits in front of an operation, its stored outcome must be written by the operation's **completion**,
never by a partial commit the caller was told had failed.

---

## Class 3 — The instrument's self-report

**A tool's report of what it did and what it actually did are separate facts.**

Distinct from the first two: those are defects the instrument *reports on*, this is a defect *in the
reporting* — which is why it would degrade every verdict the instrument issues, including verdicts
about the other classes, if it were as widespread as this section first claimed.

**Read the retraction below before using this class.** It was drafted around a defect that does not
exist, and the surviving material is one sighting, not three. The class is kept because that one
sighting is real and because the retraction is the most useful thing in the section — but it is
currently the *thinnest* of the three classes, not the worst-placed, and nothing here should be
cited as evidence that our instruments generally misreport themselves. One of them has a coverage
gap. That is the claim.

### RETRACTED: "the gate's exit code lies" (#97) — it does not

**This section previously opened with a defect that does not exist, asserted by me, and the claim
propagated into #97, PR #99's body, and a reviewer's accepted delta before F disproved it.** It is
retracted here in full rather than corrected in place, because the retracted version is the more
instructive artefact.

F's investigation (PR #100) invoked the gate **directly**: RED exits 1, GREEN exits 0, every time,
across the reported pattern, the full structure, and both shells. The gate's exit code is correct.

**Read at first hand, not through a summary** — which matters here, because adopting a corrected
number from someone else's arithmetic is how the original claim survived as long as it did. PR #100
reproduces the footgun on demand rather than only refuting the claim:

```
$ powershell.exe -File repro.ps1 | tail -5
[gate] FAILED: rustfmt (exit 1)
[gate] RED - failed stages: rustfmt
$ echo "BASH $? AFTER THE PIPE: $?"
BASH $? AFTER THE PIPE: 0
```

Same script, same real failure, same RED banner, `$? = 0`. F's conclusion is the one worth carrying:
**no change to `gate.ps1` can fix it**, because in that invocation shape the exit code the caller
observes is never the script's. The fix is documentation at both invocation sites — the script's doc
comment and `AGENTS.md` — which is the correct repair for a defect that lives in how a tool is
called rather than in the tool.

**F also corrected an independent inaccuracy in the same docstring**, found while working there: it
claimed the gate stops at the first failure. It does not — every stage runs regardless, by design,
so one invocation reports every failure rather than the first. Worth noting here because this
section leaned on gate behaviour it had not read.

**What I actually observed, re-derived from scratch rather than patched:**

| run | invocation | exit read | whose code |
|---|---|---|---|
| RED | `… -File ci/gate.ps1 2>&1 \| tail -60` | 0 | **`tail`'s** |
| GREEN | `… -File ci/gate.ps1 2>&1` | 0 | the gate's — **and correct** |

Verified by me in one line: `false \| tail -1` returns **0**; `false` returns **1**. A bash pipeline
reports the LAST command's status.

**So the two runs were two different instruments and I compared them as one.** That is the error this
document warns others about — an instrument change demands its own ledger row — committed by the
author of the warning, inside the section that states it. The "symmetric evidence" that the status
was *constant* was the strongest-sounding part of the claim and was pure artefact: one piped run,
one unpiped run, presented as a controlled pair.

**The tell was in my own words and I overrode it.** My provenance line to the orchestrator read: *"my
first run piped through `tail`, which is why I had no stage-by-stage output — my error, not the
script's."* I identified the pipe, attributed the missing log to it, and did not carry the same
attribution to the exit code sitting beside it. A peer's more confident framing ("the gate's exit
code lies") then hardened it into a filed issue. **A correct observation with the wrong conclusion
attached is more durable than a wrong observation**, because the evidence keeps checking out.

### What survives, re-derived

**3.1 — the exit status you read is your PLUMBING's, not the tool's.** This is a reader-side defect,
not an instrument defect, and it belongs beside 1.3 (the reader-level flattening) rather than with
the gate's own bugs. Verified instance: the `tail` pipe above. A second candidate — a `pwsh`
invocation whose "command not found" was recorded with `[exited with code 0]` — is **NOT FULLY
DERIVED**: bash returns 127 for a missing binary, so an exit 0 implies another wrapper in that
command line, and I have not seen it. The output file is byte-verified; **the mechanism is not**.
Whoever holds that invocation should attach the command line, or the sighting should be dropped.

**3.2 — the log carries no per-test evidence.** `grep -cE "^test [a-z_]+ \.\.\. ok"` over the full
1088-line green log returns **0**: it records which binaries *started*, never whether any test
passed. **This survives as a fact about log verbosity and NOTHING MORE.** Its former weight — "if the
exit code is the only pass/fail signal, a wrong exit code has no second source" — **is deleted, not
softened**, because its premise was the retracted claim. A verbose log is still worth having; it is
no longer a defence against anything.

**3.3 — coverage shrinks silently (#98). Untouched by the retraction and the strongest remaining
sighting.** The per-suite loop is a hardcoded allowlist of twelve names, so a new test file gets the
workspace pass but never the isolated pass that catches cross-test interference. Nothing announces
the omission. Evidence: commit `f7726b4`, which adds one suite by hand and documents the trap at the
site. **This is a genuine instrument-self-report defect: the gate's coverage and the gate's report of
its coverage are two facts.**

**The repair has already been demonstrated, not merely proposed — B's #81 ledger (PR #121).** Its
S1 and S3 rows are **measured nulls**: sabotages whose casualty is *nothing*, recorded as nulls and
seeded rather than quietly dropped (S1's wrapper mappings unit-unreachable, five of six review-only;
S3's budget threading likewise). That is what silent coverage looks like when it is made loud —
**the suite maps its own perimeter instead of implying it covers everything.** A dropped null row
reads as "not tested and nobody noticed"; a recorded one reads as "tested to here, and here is the
edge". The same move as this document's own NOT-MEASURED marks, applied to a sabotage ledger.

### What the class is, after the retraction

Class 3 does not currently hold "the gate lies about its own result" — that was one claim and it was
mine and it was wrong. What it holds is narrower and still real: **#98's silent coverage**, plus the
reader-side lesson that a status read through plumbing is the plumbing's. The class survives with one
sighting instead of three, which is the honest size.

### The exit-status lesson, re-derived (NOT a defect class)

The earlier draft of this subsection counted "four observations across three mechanisms" of
instruments whose exit codes lie. **That derivation is void: its lead mechanism was the retracted
claim, and a count built on a dead premise does not get patched down to three — it gets re-derived.**
Re-derived, what remains is not an instrument-defect family at all. It is one reader-side rule and
one genuine tool limitation:

**The rule — an exit status read through plumbing belongs to the plumbing.** `false | tail -1`
returns 0. Every wrapper between you and the tool — a pipe, a shell, a task harness — is entitled to
answer in its own name, and none announces that it has. Read the tool's own output for the tool's own
verdict.

**Two sightings, one mechanism, both now derived:**

1. My gate run: `… -File ci/gate.ps1 2>&1 | tail -60`, RED banner, `$? = 0` — `tail`'s.
2. The M09 close gate: `cd F:/github/GraphHelm && pwsh -NoProfile -File ci/gate.ps1 2>&1 | tail -40`
   — `pwsh` was absent, bash's 127 was eaten by `tail` exiting 0, and the task file recorded
   `[exited with code 0]` beneath `pwsh: command not found`.

**How sighting 2 came to be derived is the point.** It was first offered as a grouping and I refused
it, marked **NOT DERIVED**, on one discriminator: **bash returns 127 for a missing binary, not 0**,
so an observed 0 required a wrapper nobody had shown me. The refusal forced a transcript search that
recovered the invocation — and the pipe was there. **The grouping was correct all along and was
supported by nothing**, which from the outside is indistinguishable from a laundered one.

Provenance, at the grain that matters: the invocation was recovered from the orchestrator's session
transcript and **verified by me in a tool-call `"command"` field — not in prose quoting it.** That
distinction is not pedantry: the same transcript contains the claim's own text, and counting that as
evidence would be the claim citing itself. One real invocation, one occurrence.

**The limitation — an exit code names THAT something failed, never WHICH thing.** An isolation
harness exited 1 while its tests had never run: the argument vector reached cargo as a single token
and was parsed as a toolchain name. The status was *true and useless*, and under a pre-declared
reading its zero-failures became "deterministic bug". This is real, is not a lie by any tool, and is
why a harness needs three outcomes — **PASS / FAIL / HARNESS-BROKE** — rather than an exit code and
an inference. Evidence: `.factory/c-agent-wake-flakes-study.md`.

**Neither belongs in class 3.** The first is a reader-side defect, kin to 1.3. The second is a
limitation every exit code has by design. Filing either as "the instrument misreports itself" is how
the retracted claim got written in the first place: a real observation, a plausible class, and no
one asking whether the tool had actually been measured directly.

**Note on the count.** The board named three sightings; enumerated by mechanism there are four
observations across three mechanisms, because A is one defect observed twice. Stated rather than
rounded, since the section's own thesis is that collapsing distinct things into one number destroys
the distinction the reader needs.

### Repair

- The status and the verdict must be the same fact.
- A failing stage must be visible **in the log body**, not only in the status — so the two can
  disagree loudly instead of silently.
- Coverage must be **derived** (from `tests/*.rs`) with exclusions explicit, so an omission cannot be
  silent.
- For any harness: three outcomes, never two — **PASS / FAIL / HARNESS-BROKE** — and refuse to
  compute a rate over iterations that measured nothing.

---

## Cross-cutting

**A zero is the one result a completely dead instrument reproduces perfectly.** Every other outcome
is at least evidence that something happened. So run-verification only ever has to be argued for the
zeros, which turns "audit every rate" into a finite job. Applied to M09's evidence corpus: of the
zero-shaped claims found, most already carried receipts, one was marked NOT run-verified, and the
rest were owner-asks. Evidence: `.factory/c-agent-zeros-enumeration.md`.

**The finest available grain, applied to receipts as well as assertions.** A count (`1 passed`)
proves *a* test ran; the named line (`test <name> ... ok`) proves *that* test ran. The count grain
cannot separate a real zero from a mis-targeted one — a wrong filter, a rename, a silently filtered
test all produce a healthy count for the wrong test.

**Two prose failures in one lane, both surviving every green run** (PR #99): backticks in a patch
executed by the shell, silently deleting three identifiers from doc comments; and orphaned doc
comments dragged in by a line-range extraction, describing functions that were never copied. Clippy
caught the second; nothing caught the first but a diff read. **A green run audits code. Only the
diff read audits prose.**
