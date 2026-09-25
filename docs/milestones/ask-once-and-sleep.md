# Milestone 08 — Ask Once and Sleep

**The user sentence, end to end:** *"I ask once whether I can go back to sleep, the answer
contains time, and if I have to wait, I can actually wait."*

M07 made the glance HONEST — it stopped saying green over a wedged execution. M08 makes it
COMPLETE (it says *when*) and makes waiting REAL (an MCP client can block instead of poll).
The scope was again the blind judge's seeds, in his order of insistence.

## What landed

| Part | Shape |
|---|---|
| One subtraction, one verdict | `node_silence_seconds` is the single place elapsed time is computed; the monitor's private `last_event_per_node` and its three staleness thresholds are deleted, not deprecated. |
| Time never enters the reply | Silence is judged from an INSTANT the surface injects, never a duration a surface computed. |
| §8 clause seven | The product now PROMISES that no surface recalculates the attention verdict. Owner decision, taken in the only order that could earn it. |
| Blocking wait as MCP primitive | `wake_wait`, the thirteenth tool: bounded in the schema, content-free by construction, and refusing any rendezvous the session does not hold. |
| The freeze rule bites | Merged separately (#64) as machinery, because a branch carrying both the judge and the judged is the thing the rule forbids. |

## The measurement that deleted a task

Task 3 was going to fix "the serve cannot serve while it drives". **Measurement refuted the
premise**: with a drive parked in a 90-second model call, `/health` answered in 0.00s,
`GET status` reported the execution running in 0.03s, and `POST wake-lease` was ACCEPTED in
0.08s with a live lease. Five causes were proposed across two agents for the M07 alarm
failure and **four died to measurement**; the fifth is recorded as a hypothesis, not adopted.

The task became a correction of the record instead of a fix, and the binding decision whose
premise had died was **deleted rather than reworded** — in both the milestone record and this
milestone's own plan, where it had survived in present tense.

## The defect family this milestone spent itself on

One shape appeared **five times in one day**, in five different places, found by five
different means:

1. A one-truth guard passed because its fixture drove the story to quiescence: no node in
   flight, so both surfaces answered "nothing" and the test called that agreement.
2. The §8 clause those guards support inherited the same weakness.
3. The CLI/API parity compared only the FINAL view of a story that ends completed — measured:
   `false [] []` on both sides.
4. An instrument's doc comment said it scanned `src/` while its code walked every `.rs`.
5. A tool-table invariant asserted a substring while its message claimed a mapping.

**The rule that came out of it, now binding:** every guard must state, in one line, *which
state of the store makes the question exist* — and where sabotage does not apply (prose), the
obligation becomes a **coverage sweep**, never an exemption.

**Its twin, paid for twice in one day by two authors:** prose and check diverge, and review
reads the prose. The cure is to move the claim inside an assertion.

## Honest limits

1. **The counter is not here.** Interaction cost as a gated axis needs to count against the
   CORRECTED surface, and the corrected surface is this milestone. Counting on a branch cut
   from `main` would have measured the product M08 exists to fix — the disease, recorded as
   the baseline for the cure. The paired specimens (`expensive-but-correct` and its mirror
   `dumped-but-unanswered`) ARE merged, so the counter cannot arrive alone and teach the
   mirror. The enforcing stage is a declared follow-up, not a dropped scope.
2. **Why the M07 live alarm failed is still unknown.** Four hypotheses died to measurement.
   Any future fix here must be justified by a reproduction, never by a record.
3. **The freeze check reads the LOCAL `main` ref.** In this repository — merges land on
   GitHub, nobody checks out `main`, two worktrees share the object store — that ref lags
   routinely, and a stale base makes the check accuse a clean branch by name. Found while
   integrating `main` into this branch. A confident false accusation corrodes a rule faster
   than a silent false negative, because it teaches people to ignore the alarm.
4. **`wake_last_consumed` still only grows.** Unchanged from M07, and still deliberate.

## The gate, and the first RED explained rather than shrugged off

The first full-gate run of this branch was **RED** in two stages (`workspace tests`,
`cli: wake_http`); the second was **GREEN — every stage passed**. A green after a red is
worth nothing unless the red is explained, so it was reproduced rather than filed as a flake:

`git merge origin/main` rewrites the mtime of every file it touches, which makes the built
binary **older than the sources it should embody**. The freshness instrument merged in #64
then refuses — correctly — and that refusal lives inside the `workspace tests` stage.
Reproduced deliberately by pushing one pathogen source's mtime an hour ahead:

```
with a binary newer than every source, the instrument must measure, not refuse:
Err(Stale { path: target/debug/graphhelm.exe, newer_source: tools/pathogens/src/lib.rs })
```

Cure: `cargo build -p graphhelm-cli`. **Operational rule, now paid for four times in one
day:** any command that rewrites source mtimes — `merge`, `checkout`, `fmt` — invalidates the
instrument until a rebuild. That is the guard working, not an incident.

`cli: wake_http` passed in isolation under the gate's own flags (`--locked`) both before and
after, and passed in the green run. Its first-run failure is **NOT explained**, and is
recorded as unexplained rather than attributed to the mtime cause that explains the other
stage. One measured cause does not license a second guess.


## The finding that outlived the milestone: revert-and-redo is a sieve

A schema edit went wrong (a JSON printer produced a 3315-line diff that would have hidden the
real change inside formatting churn), so the whole `schemas/` directory was reverted and the
work redone by hand. The redo restored three of the four legs of the D-037 ritual and lost the
fourth. The reviewer measured it: zero occurrences of the new event in the CHANGELOG.

**The three that came back are exactly the three a test covers.** The schema copies, their
byte-identity, and the two catalog digests all have guards, and the guards refused to go green
until the work was there. The CHANGELOG has no guard, so its return depended on a human
remembering, at the end of a long day, something no failure would ever mention.

This is not carelessness and it is not haste. **It is selection.** A revert-and-redo cycle is a
sieve: whatever a test protects is compelled back, whatever it does not protect survives only
on memory. Run the cycle a few times and the documentation disappears BY CONSTRUCTION while
the code stays — and nobody involved ever made a decision to drop it.

It is also the exact twin of the defect this milestone chased all day. That one was **prose
asserting what the check does not sustain**. This one is **prose disappearing because no check
sustains it**. Same root, opposite sign.

The practical rule, which costs nothing: after any revert-and-redo, list what the tests do NOT
cover and check those by hand. That list is short, and it is precisely the set that a green
build will never mention.


## The rule the eleven instances were always pointing at

Eleven times in one milestone, the same defect. Not eleven careless moments — one shape:

**An assertion that looks ONE LEVEL ABOVE what it means to measure.**

| What it asked | What it should have asked |
|---|---|
| the headline verdict | the node the question was about |
| the reply of the write | a later read, through another door |
| a hand-kept list of crates | a scan of the directory |
| "do the surfaces agree?" | "does the surface contain the thing?" |
| "was this guard green?" | "could this guard ever have been red?" |

Every one of them is the AGGREGATE standing in for the ITEM. And every one was
self-consistent: the headline was right about the headline, the write was right about the
write, the list was right about the list. Nothing lies. The reading is simply taken one floor
up from where the truth lives, and at that height two different worlds look identical.

**The rule, which costs nothing to apply:**

> **Assert at the finest grain the question has.** If the question is about a node, do not ask
> the headline. If it is about what a reader will see, do not ask the writer.

The eleventh instance is the proof that this is a shape and not a habit: it happened INSIDE
the guard written to close the tenth. The guard asserted that the page did not say "NOT
KNOWN" — a headline — while the question was about one node, and the headline said something
else for an unrelated reason. A guard born empty, written by someone who had spent the day
finding empty guards.

That is why the cure is a rule about GRAIN rather than a rule about care. Care does not
survive a long day. Grain is checkable while writing the assertion.


## How it closed, and what was NOT withdrawn

Nine blind-judge runs. The rule was always: a milestone closes when the NAMED findings are
WITHDRAWN — never when the judge approves, because he does not.

**Withdrawn, each with bytes rather than prose** (the amendment this milestone made to its own
closing rule, after its author tried to violate it):

| Finding | How it died |
|---|---|
| the permanent `unknown` that named this milestone | the answer resolves, and it survives a later read |
| the remedy explains and offers nothing | step 1: the remedy travels as data |
| the remedy is nameable and not callable | step 3: three surfaces, one value |
| the remedy does not stick | one budget function; the judge's own probes confirm |
| calm purchased by amendment is indistinguishable from untroubled calm | the fourth verdict |

**NOT withdrawn, and named rather than buried:** the finding that carried F1's number
survived. It dropped from CRITICAL to HIGH — and, more importantly, it CHANGED KIND. It stopped
being a claim that the mechanism fails and became an argument about a DEFAULT:

> *why must an operator declare a per-node ceiling before the screen can speak? That is exactly
> the work the screen exists to avoid.*

That is not a defect and it is not withdrawn. A defect dies to evidence; a design argument
does not, because it is not making a factual claim that measurement can refute. The honest
statement is therefore narrow: **the defect this milestone was named for is gone, and a
different KIND of objection now occupies its number.** It opens M09 rather than closing here.

Saying it this way costs something and that is the point. The comfortable sentence — "the
judge backed down" — would be the prose this milestone spent nine runs deleting.

## What this milestone actually produced

The features are real: time on the glance, a three-valued verdict, a clock immune to being
watched, a blocking wait in chat, the operator's declared timeout carried from YAML to
verdict, an amendment that binds forward, and a socket the remedy plugs into.

But the durable output may be the two rules, both paid for in defects rather than reasoned
into existence:

* **Assert at the finest grain the question has.** Eleven instances of one shape, including one
  inside the guard written to close the tenth.
* **Revert-and-redo is a sieve.** It selects against exactly the work no test protects, and
  nobody involved ever decides to drop it.

Both are checkable while writing, which is the only property that survives a long day.

## Closing rule

The blind judge re-judges the same story against the corrected surface. The milestone closes
when the NAMED findings are withdrawn — not when the judge approves, which the M07 record
established he never does. New findings become M09 seeds.
