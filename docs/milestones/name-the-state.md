# Milestone 10 — Name the State You Were True Of

**The user sentence, end to end:** *"When a surface answers me, I can tell which world I am in —
and when I read the record later, I can tell what it was true of."*

M09 made silence a property of having been reached. M10 is about the sentence after the answer:
an operator who is told "the call failed" still could not tell whether their hold survived; a
`resume` that could not run still consumed what it was refusing to release; a timeout reported
itself as corruption. Eighteen merges later those answers name their world. The same milestone
found the identical defect in its own records, nine times, and the second half of this document
is that account.

---

## What landed

**Eighteen merge commits, closing seventeen issues** — the counts differ because one merge
(`e60a27d`) closed two (#143 and #146), and three of the merges closed no issue at all (the
gate's docs, the `#[ignore]` fixture, and the recovery of commits stranded by an earlier
squash). Both numbers are derived from the table below rather than from memory. Each row's
evidence lives in its PR and its `.factory/` run record.

| # | What it was | Landed as |
|---|---|---|
| #82 | MCP `start`/`resume` had no `project`, so every pure-MCP resume collided with the server's own cwd | `1b55a13` (PR #91) |
| #83 | A refused resume still committed `ExecutionResumed` and dropped the operator's hold | `6193f5c` (PR #99) |
| #97 + #98 | The gate discarded a failing tool's own output, and its suite list had drifted from the filesystem | `dff6e9d` (PR #100) |
| #80 | A queued node dispatched without its edges, credited to a node field nothing reads | `0fb0e66` (PR #103) |
| #81 | A restore step that ran out of time reported itself as a corrupt archive | `c357a5e` (PR #121) |
| #104 | The grounding test read the gate's TEXT, so improving the gate broke the test | `e0849e8` (PR #122) |
| #123 | `pause` re-held a permanently-`Queued` node every round, forever | `21fd7dc` (PR #125) |
| #87 | The verified prefix re-proved the whole journal on every request | `6d389eb` (PR #137) |
| #88 | `wake-wait`'s timeout answered from a pre-block snapshot the store already contradicted | `d2ef897` (PR #139) |
| #140 | A server panic and a slow server produced identical client evidence | `0433e7c` (PR #141) |
| #143 + #146 | Readers serialized again after #87; the upgrade path never re-classified layout | `e60a27d` (PR #145) |
| #89 | `mode` dropped the "Graph" qualifier on every operator-visible surface | `d864b93` (PR #144) |
| #96 | `GHCLI016` answered two opposite hold-states with one value | `cf2d417` (PR #151) |
| #101 | Three copies of `is_terminal`, two of the parallelism policy | `9f3ee1e` (PR #136) |
| #118 | The 15-round wake belt passed with its recorder deleted | `cbb699e` (PR #155) |
| — | The gate's docs, the `#[ignore]` trap, and the stranded M09 commits | `07b1243`, `6dd47c4`, `5c31a5b` |

**The shape most of these share.** A surface answered correctly and told the operator nothing they
could act on. "The call failed" is true of a setup refusal and of a mid-drive failure, and those
two need opposite next moves. A timeout is true of a busy machine and of a broken archive. The
milestone's product work is one sentence repeated: **make the answer name which world it is in.**

---

## The methodological thesis — one defect, nine sets of clothes

The factory found the same defect in itself that the product had. It is stated separately from the
defect classes below because those cover what the SOFTWARE got wrong; this covers what the PROCESS
got wrong, and reading either as the other's excuse loses both.

> **A record that was true when written, and is read later by someone who cannot see what moved.**

Not a documentation problem. Every instance passed review, verified for whoever checked it, and
misled the next reader anyway — correctness was established against a state that then changed, and
the record carried no way to notice. **A more careful version of each would have been just as
wrong, just as verifiable, and just as stale.** "Be diligent" teaches nothing here; everyone
involved was.

**The seven that cost something:**

1. **Cite against a named base.** Forty `file:line` citations, written from one worktree and read
   against another, pointed about 47 lines off — right behaviour, wrong code. The trap inside the
   trap: the offset is not uniform, so correcting by a constant produces a second wrong set that
   looks internally consistent and survives review.
2. **Comments carry later state than bodies.** Two dispositions in this milestone's own issue index
   were drawn from bodies a comment had already superseded — and the audit that made this rule for
   one task skipped it for the next, in the same document, two sections apart.
3. **A watch condition reported without its current state is just an old worry.** #80's condition
   fired twice and was adjudicated twice; the original wording alone would have described a live
   risk that had already been answered.
4. **The board is the one surface a stranger cannot see.** #118's real state lived only on the
   coordination board while the issue still showed its original framing. Every stranger the
   acceptance evidence is written for reads the issue.
5. **An imperative does not carry a timestamp.** An instruction stays imperative long after the
   premise that justified it has moved, and the text cannot show the difference.
6. **A shared record moves WHILE you write in it.** A reinforcement was appended to a shared memory
   file that already carried the same reinforcement, twice, written in parallel by other sessions.
   The read that preceded the write was true when it was made; two more writes landed in the gap.
   The rule is not "read before writing" — **the read-write interval is itself a moving base.**
7. **Two readings of ONE tree that belonged to neither reader.** A `ci/gate.ps1` line citation was
   confirmed independently by a second reader, and the confirmation held — because both readers had
   left their own worktrees by a bare `cd` and landed in the shared main checkout, a live tree
   belonging to a third lane. Enumerated afterwards: that checkout is the ONLY tree in which the
   line reads `:100`; both readers' own worktrees, and `origin/main`, all read `:129`. **No base
   inheritance, no base divergence — the default gesture picked the shortest tree, twice.** Each
   reader had the right answer in the tree they had just left.
   The pattern that proves the rule, in the same author's numbers: **the five citations re-derived
   against a named base survived; the one read by `cd` broke.**

   *(This entry was corrected TWICE before publication, and that is the strongest item in the
   catalogue. v1: the second reader inherited the first's base — inferred. v2: the two readers were
   in two different deviated trees — also inferred, and offered by the reviewer who had just
   disproved v1. v3, above: enumeration of all four trees. **Each correction arrived with MORE
   confidence than the last, because each was correcting a real error** — and only the enumerated
   version survived. The detail that should embarrass all of us equally: the evidence that killed v2
   was in the same paragraph as v2 — its author had written "my `:100` came from `cd
   F:/projects/GraphHelm`" two lines above concluding that the trees differed.
   One last coat of precision, from the same author: v3 was REPLICATED by two hands, and
   replication is not independence. The hands differ; the method does not — same `git`, same
   object store, same question. **For "which line of this blob holds this string", the instrument
   is DEFINITIONAL: there is no second instrument with the power to disagree.** So the convergence
   buys exclusion of human slip — a wrong finger, a wrong worktree, a mis-transcription, which is
   exactly what v1 and v2 lacked — and buys nothing about instrument bias. Calling half a
   verification a verification and calling replication independence are the same error: mistaking
   ONE MORE reading for ANOTHER reading.)*

**The two that cost nothing, and why they are the argument.** A gate result was cited as
`gate @ cf2d417+2 (191ecbf)` — never "@ main" — because its author asked for the base to be named
BEFORE the measurement existed. ("@ main" would have been false in both directions at once:
`191ecbf` is not an ancestor of `origin/main`, so the gate ran on commits main does not contain,
and main contains commits the gate never ran.) And #154: the author of #101, writing a
review-requested doc clause, found that their own change had put one function inside another's doc
block — leaving one symbol with no documentation and the other carrying an `# Errors` section for
an error it cannot return. **The author was the corrector, and the correction arrived before any
known consequence.**

Both clean cases share one property, and it is the practical rule: **the author asked what their
own record now SAYS, rather than what they meant it to say.**

**What all nine share:** each record was verified against a state it did not NAME. The remedy has
one shape everywhere — bind the claim to the state it was true of: the base commit, the layer, the
date, the surface, the premise.

**And the frame closes on itself.** The paragraph above first read "cost: zero readers misled",
which is bare absence — nobody looked for readers, the wrong doc was public for about half an hour,
and the reviewer who caught it was asking a different question. The correction came from #101's own
author, arguing against the stronger claim. **An instance of the thesis had to be corrected BY the
thesis.** The rule caught its own celebration of itself.

---

## Defect classes

Drafted by C, who holds first-hand evidence for two of the three and marks the rest
reported-not-verified. Full section with every citation:
`.factory/c-agent-m10-defect-classes.md`.

**Why the section exists.** #81 taught it at the level of tests: registering flaky SITES one at a
time never converges, because the defect belongs to the shape and the victim rotates. The same is
true of defects.

**Class 1 — Flattening.** A boundary maps distinct causes onto one legal, well-formed value, and
the consumer needs exactly the distinction that was destroyed. The tell is one value, two causes,
opposite responses (#96: setup failure vs mid-drive failure, hold intact vs hold consumed). But
**the value COUNT is not the class**: #81's census found elapsed scattered across four variants and
three codes, none of which named timing. **A scatter is worse than a uniform flattening**, because
no single grep finds the policy and every site looks locally reasonable.

**Class 2 — Fabricated success.** A run reports completion for work that did not happen. The wake
belt (#118) is the milestone's sharpest instance: fifteen rounds asserting `ok == true` and
`consumed <= armed`, both of which a component that writes NOTHING satisfies perfectly.

**Class 3 — The instrument's self-report.** What a tool says about its own run is evidence about
the tool, not about the work. This class survived a retraction: "the gate's exit code lies" (#97)
was investigated and **refuted** — the observed behaviour is what piping through `tail` does to an
exit status, not a script defect. What survives the retraction is stronger than what was claimed.

**Cross-cutting, and worth quoting exactly:** *a zero is the one result a completely dead
instrument reproduces perfectly.* Every other outcome is at least evidence that something happened,
which turns "audit every rate" into a finite job: only the zeros need run-verification. And **a
green run audits code; only the diff read audits prose.**

---

## Traps

**`#[ignore]` means two different things, and a gate stage runs the other one.** #141 added a
deliberately-failing helper marked `#[ignore]` in its ordinary sense — keep this out of the default
suite. `ci/gate.ps1` carries a stage whose entire selector is the opposite reading: run the ignored
ones. Fixed by #149 (`6dd47c4`), and the fix's shape is the lesson — the fixture now arms only via
an environment variable the guard's own subprocess sets, and the OUTER guard always arms it and
asserts the subprocess failed, so a renamed variable fails LOUDLY instead of silently disarming the
sabotage. **A guard that degrades to silence when its plumbing breaks is not a guard.** General
form, from the merge's own words: **a repo with an `--ignored` matrix has two default sets.**

Note where it landed: #141 was itself an instrument fix. **A change that makes a blind spot visible
immediately produced a red in a matrix nobody was watching — that is what instrument work costs,
not a misfire.**

**The panic-path guard is Windows-only (#150, open).** The sabotage child spawns `cmd`, so on any
other platform the spawn fails, the outer guard reds, and it presents AS A DRAIN FAILURE — in the
file this project attributes reds from. Nothing fires today because the gate is PowerShell-based.
**A latent red that misdirects is worse than a latent red that stops you.**

**The board is invisible to strangers.** Durable state that matters to an issue's next reader
belongs ON the issue; if the board is the only source, that is a gap to close, not a citation to
keep.

**A silenced error reads as an answer.** `git show REF:path` under Git Bash on Windows rewrites the
path unless `MSYS_NO_PATHCONV=1` is set; with stderr discarded it fails producing nothing, and
nothing is indistinguishable from "that file is not in that ref". Four files were nearly reported
as absent from `main` on that basis. The shape is general and this milestone met it twice — once
here, once in the gate's swallowed stdout (#97/#98): **a tool that is prevented from complaining
does not become silent, it becomes agreeable.** Any check whose NEGATIVE result is load-bearing
must keep its error channel, and `2>/dev/null` on a verification command is the cheapest way to
manufacture a confident wrong answer.

---

## Incidents

**Main went red after #87, and the rule was born and paid inside one cycle.** The verified-prefix
work regressed reader concurrency; readers serialized again and `read_concurrency` failed on main.
Found on a clean base, fixed within hours (#143, #146 → `e60a27d`), and the miss recorded by the
person who made it rather than left to be inferred. The honest claim: **a performance change that
had been measured, and measured well, still broke a property no measurement was watching.** The
rule it bought — **measurement never substitutes for the gate** — arrives with its receipt
attached.

**A PR merged on a stale-worded approval (#104 / PR #122).** A process-boundary incident, recorded
here so it is not reconstructed later from a green lane entry. The lane's code outcome and its
process outcome are separate facts.

**Two merge-gate misses.** Recorded in the orchestrator's own words in `6dd47c4`'s squash body:
full-gate evidence is now a merge precondition.

**And the reporting channel, reinforced by recurrence.** Reports go to the orchestrator, who
consolidates; they do not go directly to the owner. The rule existed and was re-broken, which is
the diagnosis: **a rule without a mechanism is a choice made every turn, and a choice made every
turn is how rules die.** Same shape as the guard rule above.

---

## The dimension a study earned is not the dimension it may spend

The sharpest methodological finding, and it is why the storm's mechanism is still open. The storm
study asked "can the pipe BLOCK?", got a correct NO, and spent that no as "the pipe is fine". **The
READ question was never asked.** That is how a candidate stayed invisible for a whole milestone
behind an instrument that looked thorough.

Any claim of the form "we checked X, so Y is fine" must name the dimension the evidence actually
covers.

---

## What is NOT claimed

**The storm's mechanism is OPEN. It is not convoy.** Four candidates stand side by side: the
convoy/latency chain, the environment hypothesis, H3, and **dead-server** — until #141,
`ServerGuard` never drained the spawned server's stderr, so a server PANIC under load produced
exactly the same client evidence as a slow server. Read-phase attribution separates read from
connect, not slow from dead. **The instrument was structurally unable to see that candidate for the
whole milestone.** The discriminator is #140: the first post-#140 storm red either carries the
server's dying words, or convoy survives an elimination.

**The MOVED verdict is untouched by that.** Attribution and mechanism are different claims: the
verdict is a rate comparison, interleaved within one session, and #103 causes more failures
whichever proximate cause fires. Letting an open mechanism erode a measured attribution would trade
a measured claim for an unmeasured one.

**The rate itself is INDETERMINATE — declared, not restored.** #125's approval rests on the
MECHANISM: slope back to 2 events/round, storm counters at or below the pre-#103 baseline, three
non-inheriting instruments. It was merged with one stage of 28 red, **classified and not
attributed**: pre-#103 main itself fails 4/10 quiet on that machine and session, the dedicated
stage passed 31/31 in the same run, and one uninstrumented observation separates nothing.

**The storm's rate is a property of COMMIT × SESSION.** This voids any standalone rate
characterization that does not name its session, including earlier baselines — flagged rather than
repeated.

**The residual ceiling on #145's agreement figure is INDICATIVE, NOT MEASURED**, with its
confounder named.

**#120: the storm attribution's own record lives in working files, not in a merged record.** The
headline landing's supporting evidence was unmerged until this close merged it.

**Three red-forms appear in this milestone, and naming which is which is the point:** CLASSIFIED
(cause identified, belongs elsewhere — #81), ATTRIBUTED (cause traced to this change, then fixed —
#80's watch condition), and merged-without-either (none here, and that is worth a line).

**#80's watch condition remains OPEN, and here are its terms.** It fired twice and was adjudicated
twice — once CLASSIFIED (the cause belonged to another lane), once ATTRIBUTED and then fixed. It
carries forward to post-#125 gates, and it is executable rather than a feeling:

- **The trigger:** any gate red on the `api_http` storm at or after `21fd7dc`.
- **The obligation, on the FIRST failure:** attribute it then, with the run captured
  (`--nocapture`), while the failing output still exists. Not on the second occurrence, not after a
  re-roll.
- **The prohibition:** it is never archived as a known flake BY NAME. "That one is flaky" is the
  move this condition exists to prevent, because it converts an unexplained failure into a
  permanent excuse.
- **The discriminator it now has:** since #141, a server panic writes its dying words. So the first
  red either carries them — dead server — or it does not, which is an elimination the convoy
  hypothesis has to survive.
- **How it closes:** a named cause, or a clean post-#125 run recorded WITH its session, because a
  rate is a property of commit × session and a green from another session closes nothing.

A condition that fires, is adjudicated, and stays open is working. One that quietly disappears
between milestones was never a condition, only a worry with a deadline — and one without these
terms hands the next person a worry rather than a procedure.

---

## What #118 sealed, in its authors' terms

**The belt's headline moved to an identity assertion.** The old guard asserted `ok == true` per
round and `consumed <= armed` overall — and the M09 close doc measured it GREEN with the recorder
deleted. A component that writes nothing satisfies both. It could not see a sweep that consumes
nothing, a key-smear that swallows fourteen of fifteen consumptions, or a compensating
redistribution where round N consumes twice and round M never (15 ≤ 15 holds; two failures cancel
inside a satisfied aggregate).

**Sabotage sA dies in round 0, at the named blade** — the recorder-dead sabotage that passes the
old belt was this commit's acceptance bar, and it does not survive.

**The limit of sD, in J's terms:** the belt gained an ORACLE over a dead recorder, **not power over
a race that does not fire.** What proves #55 is C's seam, not this instrument. The aggregate
inequality and per-round legality stay — insufficient, not wrong.

**Why it reads the log rather than the projection:** the receipt maps are last-per-session, so an
arming-scoped question must walk the raw journal. And the arming is read back from the store rather
than derived by arithmetic — **a guard whose expected value can be derived without doing the work
is not a guard.**

---

## Method notes for anyone verifying this document

Each was paid for during this milestone; each is the thesis applied to an instrument rather than to
a document.

1. **A graph citation carries CHECKOUT + HEAD + TREE STATE.** The code-knowledge index tracks a
   live checkout, not a commit; at the time of writing it served a branch checkout with dirty files,
   not `main`. `index_status` at query time is necessary and not sufficient. **To cite code on main,
   use `git show <sha>:<path>`.**
2. **The graph is blind to `ci/gate.ps1`** — reported unparsed over its whole length, which is
   exactly where four of this milestone's lanes live. Four readers found this independently, which
   is what separates a real blind zone from an artifact of one query.
3. **The acceptance journals are not indexed at all**, by design. A reader verifying those claims
   through the graph gets silence, and silence reads as absence.
4. **A read runs with its base EXPLICIT — `git show <sha>:<path>` or `git -C <worktree>`, never a
   bare `cd` into a shared checkout.** A grep that informs a report names the SHA it ran against.
   **Ask for existence with an EXIT CODE, never by hashing output: `git cat-file -e <ref>:<path>`.**
   Hashing `git show` output puts "the command died" and "the file is absent" on the same wire —
   an empty stdout hashes to the sha256 of the empty string, which looks like a successful read of
   nothing. The exit code cannot collide that way. (The symptom patch, one level below: on Git Bash
   for Windows `git show REF:path` needs `MSYS_NO_PATHCONV=1`, or the path is rewritten and the
   command fails — silently, if stderr was discarded. Four files were nearly reported as absent
   from `main` that way. The patch fixes the platform; the exit code removes the class.)
   This is the operational form of instance 7: the shared checkout is a live tree belonging to
   whichever lane is working in it, so `cd` silently selects a base — and the two readers who did
   it both had the correct line in the worktree they had just left.
   **And the third level, which is the one that actually bites: a CORRECTION needs the same
   evidence bar as the original claim.** Correcting someone who was demonstrably wrong is precisely
   when nobody applies that bar — the error is real, the correction feels earned, and inference
   gets promoted to finding. Two of this entry's three versions were corrections, both inferred,
   both wrong. For `ci/gate.ps1` specifically, prefer citing by CONTENT — the command or
   the stage name — because a command does not rot when lines move.
5. **Closure links must be a UNION** of `git log --grep='Closes #'` and the PR-level link field.
   Three shapes appeared, differing in which half misleads: a commit keyword with no PR link (#87);
   a second `Closes` line the PR link drops while LOOKING complete (#146); and a state that is
   correctly closed with the commit saying only `Refs`, so a log audit concludes "never closed by a
   PR" **with evidence in hand** (#101). The third is the meanest: nothing is broken, and the
   auditor who chose the more durable source is the one who is wrong.

The shared shape: **each instrument answers confidently within a boundary it does not announce.**
None of them lies. All of them mislead a reader who does not know where their edge is.

---

## Found and deliberately not fixed

Ten issues opened by this milestone's work and left open ON PURPOSE. The four reasons are not
interchangeable, and a list that flattens them loses the only information a reader needs.

**Not reachable yet.** #95 — attention is not edge-aware when a gating predecessor can go unheard.
Its population is still empty, established by walking emitters rather than reading the transition
table, and its triggers are signposted IN CODE at the two sites whose change would make it
non-empty. **Not to be confused with #123**, whose predecessor is `Blocked` and therefore has a
voice; both issues now say so.

**Blocked on another issue.** #93 (`userOverrideAllowed` has no execution-lane consumer) is blocked
by #94 (`Waived`/`Skipped` are table-legal but surface-unproducible). Deleting the field before the
replacement ships would leave operators with no route past a blocked node at all.

**Not a bug — a shape.** #96's family and #101's duplications were not live defects; they are the
shape that breeds one. #101's own words: patching one driver had already left the other dispatching
gated nodes.

**Routed to the owner, because it is a promise rather than a defect.** #124 — an ordinary `pause`
does not stop work in flight, on either driver. Whether `pause` means "stop accepting new work" or
"stop working" is a product commitment, and an agent choosing silently would be writing the
contract by implementation. Read it beside #92 (`approve` without `resume` makes an execution
quieter without making it move): together they are the operator's model being wrong in BOTH
directions.

**Resolved after this milestone.** Ordinary pause now declines to dispatch more nodes on both the synchronous CLI driver and the asynchronous Runtime driver. Work already in flight drains. Immediate pause remains the separate interrupting operation. The drivers recheck pause state before planning and before each dispatch; the synchronous path still documents a narrow concurrent-append window between its final read and append.

**The deferral lesson, which is the part that generalises.** #80 contained a deferral that was
CORRECT and whose trigger named the WRONG DIMENSION: it watched for what would make the defect
reachable, never for what would make it EXPENSIVE. So its own alarm never fired, and what caught
the regression was the watch condition on #103's PR plus a reviewer refusing to re-roll a red.
**Signposts work when the trigger names the right dimension. An heir must name both — what makes
the defect reachable, and what makes it expensive.**

The full index of #79–#155, with per-issue disposition, closer, and evidence pointer, is
`.factory/k-agent-m10-issue-index.md`.

---

## The CHANGELOG precondition, declared

This repository writes CHANGELOG entries at milestone close, in one batch, never per fix PR — the
file's history shows only milestone merges touching it. That made one thing a precondition for this
close: **M09's own close was tasked with backfilling the M08 section, which the file had skipped
entirely.**

**Checked, with the result:** the backfill LANDED. `CHANGELOG.md` on `main` carries
`## Ask Once and Sleep, M08 — 2026-08-18 (backfilled 2026-08-19, during M09's close…)`, with the
date-of-writing label in the header rather than a backdated one, and the M09 section above it. So
M10's section simply follows M09; this close inherits no gap and no ordering question.

Stated because a precondition that is silently satisfied is indistinguishable from one nobody
checked — and this document's whole thesis is that the difference has to be visible in the record.

---

## What M10 hands to M11

Not a backlog — a consumer. M10's dogfood measured the surface **informing 3/3 and acting 0/3**:
it said what was happening every time and completed external work no time.

**#153 is the answer shaped as an acceptance test.** The gate's stages run AS a GraphHelm execution
graph, with dependency and resource edges, and the run IS the milestone's acceptance evidence. Its
kill bar was sealed at the OPENING, before any M11 code exists — verdict agreement against
`ci/gate.ps1`, wall-clock re-derivable only before the first measurement, provenance answerable
from the journal alone, red replayable — **and the bar names its own residual**: agreement over a
small N bounds the disagreement rate loosely, so it buys a first certificate rather than
equivalence.

Its preconditions are exactly the issues this milestone produced, and its failure branch is already
a result: if it does not work, the outcome is a NAMED verb gap. **A milestone that cannot fail
without producing a finding is the shape M10 spent itself learning to build.**
