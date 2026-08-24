# #162 — dead-letter node and the deterministic `sweep(as_of)`: design, phase 1

K Agent, 2026-08-20. Slot-free phase: design and red fixtures. **No code written, no cargo run.**
Depends on lane 1 (#160, A implementing); nothing here may land before it.

Sources read directly, not in summary: issue #162; `.factory/b-agent-159-blueprint.md` §2f/§4/§7;
`.factory/d-agent-159-review.md`. Where I quote a decision it is cited to one of those.

---

## 1. The decision that is mine: LIVENESS

The blueprint delivers "no state the sweep cannot SEE" and declares the other half owed: **nothing
forces the sweep to be called.** Three callers were seeded — operator/cron, piggyback on mutation
routes, a serve tick. The review carried it as a declared dependency rather than a closed property.

**The decision splits the property in two, because the two halves have different liveness needs and
merging them is what made this look like one hard question.**

### 1a. DETECTION is read-time and needs no caller

An overdue episode is a function of `(stage-entry deadline, as_of)`. Nothing about noticing it
requires an event to be written. The glance already computes attention from an instant the SURFACE
INJECTS — that is M08's settled rule, `node_silence_seconds` its precedent, and §8 clause seven
promises no surface recomputes the verdict privately.

So: **`attention` derives overdue episodes at read time, from the same stage-entry deadlines the
sweep uses, with `as_of` injected by the caller of the read.** Any glance — CLI `status`, the HTTP
API, the monitor, MCP — surfaces an overdue quarantine the instant someone looks, with no sweep
having run, on any journal, forever.

**What this buys, stated precisely:** an item cannot silently park FROM THE OPERATOR'S POINT OF
VIEW. The word "silently" was carrying the whole property, and read-time derivation is a mechanism
rather than a discipline. It costs no new liveness because reads already happen for other reasons.

**What it does NOT buy:** nothing is recorded, and nothing acts. That is the other half.

### 1b. ACTION and RECORD need a caller — and the caller is the serve tick

`sweep(as_of)` mutates: it journals `sweep_performed`, mints one `OverdueException` per episode,
and an `on_overdue: auto_return` edge performs a state change. None of that can be read-time — a
read that mutates is the defect this project refuses everywhere.

**Chosen: a serve-side periodic tick, with the operator verb always available.**

**Why not piggyback on mutation routes, despite the precedent.** The wake sweep does exactly this
(`apps/cli/src/commands/serve/wake.rs:86`, invoked after a mutation route's durable append), and it
is the right shape THERE: a ring is only interesting when something appended, so the trigger and
the subject share a cause. Here they do not. **An overdue quarantine matters most in the world
where nothing is happening — and piggyback fires only when something is.** The mechanism would be
absent in exactly the state it exists to detect: an execution parked overnight with no mutations
gets no sweep, and the property is void precisely when it is load-bearing. That is the same shape
as a guard that degrades to silence when its plumbing breaks.

**Why not operator/cron as the primary.** It works and it is honest, but it is discipline: the
property then holds only where someone remembered to configure it, and #152's own argument is that
discipline is not mechanism. Keep it as the explicit verb (an operator must be able to force a
sweep at a named `as_of`, and replay needs that verb to exist anyway), never as the thing the
property rests on.

**Why the tick survives the objections to itself.**
- *"It puts wall-clock into the journal."* It does not: `as_of` remains an ARGUMENT. The tick
  supplies a value the way an operator would, and the journal records what it was given. Replay
  reads the argument; it never re-derives it. The determinism property is untouched, which is the
  whole reason `as_of` was made an argument in the first place.
- *"It only works while serve runs."* True, and it is the honest boundary: the property is scoped
  to a running deployment, which is the same scope every other serve-side mechanism has (the ring,
  the monitor, the API). A stopped serve parks nothing new because a stopped serve runs nothing.
- *"It is a background actor writing events."* It is, and it must therefore be an actor a reader
  can see: `sweep_performed` records the caller, so a journal shows WHICH sweeps were automatic and
  which an operator forced.

**The residue, declared rather than dressed up:** between ticks, an episode is overdue-and-visible
(1a) but not yet overdue-and-recorded (1b). The tick interval bounds that window and does not
remove it. If a future decision needs the record to be closer to the deadline than the interval,
that is a knob, not a redesign.

### 1c. DETECTION and RECORD diverge BY CONSTRUCTION, and that is correct

Named by A while building the fold, and written here before someone reads it as a bug. The sweep's
`exception_marked` is idempotence OF THE SWEEP — one exception per episode, forever. §1a is a
DERIVATION, not a record. So there is a legitimate, permanent class of state in which **the glance
says "overdue" and no `OverdueException` exists**, because nobody has swept yet.

**That is not an inconsistency to reconcile. It is the difference between "true now" and "was
recorded".** A journal answers the second question; a derivation answers the first, and a system
that can only answer the second cannot tell an operator anything until a background actor has run.

**The trigger, stated so the next reader does not have to rediscover it:** if anyone proposes
"aligning" the two — making the glance report only what the sweep has recorded, or making every
read write — **what dies is the read-time half, and with it the only part of the anti-parking
property that needs no caller.** The divergence is the feature; the alignment is the regression.

---

## 2. Episode grain, in one paragraph so the fixtures can be read

An **episode** is a stage entry: the envelope sequence at which the node entered its current
customs-holding state (`Waiting`, `Waiting`-reopened, `Claimed`, `Claimed`-reentered,
`Dead-lettered`). The identity is the SEQUENCE, never the node id or a name — names and ids repeat
across re-parks, sequences cannot (the `armed_at_sequence` precedent, and #118's per-arming identity
is the same rule one lane over). One `OverdueException` per episode; a redrive starts a NEW episode
and is therefore eligible again; the same episode never raises twice, no matter how many sweeps run.

---

## 3. The sealed cells as red fixtures

Each cell gets its own test, its own named sabotage, and — the part that makes them guards rather
than decoration — a statement of WHICH assertion the sabotage falls at. A sabotage that fells the
test "somewhere" is not evidence.

### R1 — a claimed-never-cleared claim past its instant is swept

- **Fixture:** node enters `Claimed` at sequence S with a clearance deadline; no clearing event;
  `sweep(as_of > deadline)`.
- **Asserts:** exactly one `OverdueException`, naming episode S.
- **Named sabotage:** exempt `Claimed` from the overdue computation (the state most likely to be
  treated as "already being handled").
- **Falls at:** the exception-count assertion, which reads zero. Not at the state check, not at the
  sweep's own bookkeeping.

### R2 — the DLQ-returned wait is swept

- **Fixture:** `dlq_routed` → `dlq_returned`, which REOPENS the wait with a fresh `wait_within`
  deadline; let the fresh deadline lapse; sweep.
- **Asserts:** an exception for the REOPENED episode, whose sequence is the return's, not the
  original wait's.
- **Named sabotage:** carry the original wait's deadline through the return (the plausible
  implementation: "the wait is the same wait").
- **Falls at:** the episode-sequence assertion — the exception exists either way, so a test that
  only counted exceptions would pass under sabotage. **This is why the cell asserts the sequence,
  not the count.**

### R3 — episode grain: redrive then lapse raises a NEW exception

- **Fixture:** episode A lapses and is swept; `dlq_redrive` (the fold REBASES the clearance clock
  from the redrive's own envelope — no new claim is minted); the redriven episode lapses; sweep.
- **Asserts:** two exceptions, distinct episodes; and re-running `sweep(same as_of)` emits NOTHING
  for either.
- **Named sabotage:** key the "already raised" marker by node id instead of episode sequence.
- **Falls at:** the second exception's absence — the count reads one where two are due.

**Cross-lane dependency, recorded here rather than left in a message.** R3 assumes the
`DeadLettered` entry REMAINS in scan history when a redrive appends the new `Claimed` entry — the
old entry is history, not something the redrive mutates. That half belongs to #163 (E's lane), and
E's independent reading of the blueprint landed on the same shape without seeing this file, which
is what makes it evidence rather than agreement. If #163 ever collapses history instead of
appending, **R3 needs a fixture that does not depend on the old entry being present**, and this
line is the trigger that says so.

### R4 — idempotence at one `as_of`

- **Fixture:** `sweep(T)` twice over the same journal.
- **Asserts:** the second emits no `OverdueException`, and `sweep_performed` is recorded for both
  (a sweep that ran and found nothing is not the same fact as a sweep that never ran — the
  PASS/FAIL/HARNESS-BROKE distinction, one layer down).
- **Named sabotage:** make the second sweep re-emit.
- **Falls at:** the emptiness assertion on the second run.

### R5 — replay reproduces the identical exception set

- **Fixture:** a journal containing sweeps, replayed.
- **Asserts:** the same exceptions, same episodes, same order.
- **Named sabotage:** derive `as_of` from the clock at replay instead of reading the journaled
  argument.
- **Falls at:** the exception-set comparison, and it is the cell that protects the entire
  `as_of`-as-argument decision. If this one is green under sabotage, the argument is decorative.

### R6 — the un-claimed wait (the 0/4 guard the review asked to seal)

- **Fixture:** a wait that was never claimed at all, past its deadline.
- **Asserts:** it is swept, same as a claimed one.
- **Why it exists:** without it, an implementation that only ever looks at claims is green on every
  other cell. The review named this as the guard whose ABSENCE would let a claims-only
  implementation pass — the cheapest possible false green.

---

### R7 — a node with NO declared budget is never overdue (inherited constraint, B's reading)

- **Fixture:** a graph version published BEFORE customs existed — no `waitWithinSeconds`, no
  `clearanceWithinSeconds` — with a node parked in a customs-holding state; `sweep(as_of)` at any
  instant, however far past.
- **Asserts:** ZERO `OverdueException`, and the read-path reports the episode as unbounded rather
  than overdue.
- **Named sabotage:** default the absent budget to `0` (or to any number) when the field is
  missing.
- **Falls at:** the zero-exception assertion, which reads "every parked node in the journal".
- **Why this cell exists, and it is not mine:** the ruling on #160 carries the `timeout_seconds`
  precedent's absence semantics — **absence is ABSENCE, never zero, never a default.** B found it
  by reading the precedent rather than the plan. The consequence lands in MY lane: with a defaulted
  budget, re-reading any pre-customs version makes every stage instantly overdue, and **the
  read-time detection path (§1a) is what would report it** — the surface that exists to be trusted
  becomes the one crying wolf on every old journal. A guard whose population is "every execution
  that predates this feature" is worth a cell of its own.

## 4. What I am NOT deciding here, named rather than omitted

- `dlq_within_seconds` semantics beyond the blueprint's §4 note — conditional on #153's
  finish-predicate grammar, which is another lane's.
- Auto-materialisation of `dead_letter` nodes: v1 is DECLARED-only, per the blueprint's §8.
- Countersign key custody: rides the keyring, MARKED there.
- The tick's interval value. It is a deployment knob, and picking a number here would freeze a
  policy the residue in §1b says is adjustable.

## 5. Order of work

1. This design + the six fixtures, reviewed BEFORE any implementation (phase 1, no slot).
2. Lane 1 (#160) lands.
3. Reds observed at a slot — each fixture red at its NAMED assertion, transcript captured.
4. Implementation, greens, sabotage runs reported as WHICH edit + raw output.

---

## 6. PREREQUISITE: the five event kinds do not exist to the schema yet

Relayed by the orchestrator from A's measurement (11 commits of enum + fold + guards with nothing
entering a journal). **Re-derived here against a named base rather than taken on report** —
`origin/main` at `d0b3f04`, this branch at `cdcbac4`.

### What I measured

`grep -i "dlq\|sweep\|overdue" schemas/event-envelope.schema.json` returns **nothing**, on both
bases. None of `dlq_routed`, `dlq_redrive`, `dlq_returned`, `sweep_performed`,
`overdue_exception` is declared. The schema's union currently carries 32 kinds, ending at
`gate_certified`.

### The mechanism, read rather than assumed

`validate_envelope` (`core/events/src/integrity.rs:293`) serialises the envelope, compiles
`repository_schema_set()`, and returns `EventRepositoryError::Invalid` when
`schemas.validate_event(&value)` is non-empty — **before** any of the kind-specific checks below
it. An undeclared kind therefore fails with a bare `Invalid`: no diagnostic, no mention of the
schema, no mention of the kind. That silence is the whole cost.

### FOUR sites per kind, not three

The relay named three. Measured on `origin/main`, adding a kind touches **four** places:

| # | site | shape |
|---|---|---|
| 1 | `$defs/<payloadName>` | the payload object, `camelCase` fields, `additionalProperties: false` |
| 2 | branch in `$defs/eventKind`'s `oneOf` (line 686 →) | `{"required":["type","data"],"properties":{"type":{"const":"snake_case"},"data":{"$ref":"#/$defs/<payloadName>"}}}` |
| 3 | line in the top-level `oneOf` (line ~981 →) | `{"properties":{"kind":{"properties":{"type":{"const":"snake_case"}}},"scope":{"$ref":"#/$defs/scopeWithExecution"}}}` — all five of mine are execution-scoped, matching `is_project_level` (`core/protocols/src/event.rs:189`), which lists only the five integrity/evidence kinds |
| 4 | **`schemas/catalog.json`** | the `event-envelope` entry pins `sha256:2ff2036d…`; editing the schema invalidates it, and the gate's `schema catalog` stage recomputes it. `schema baseline compatibility` then diffs against `schemas/releases/1.0.0/catalog.json` — additive union branches should pass, but this is a GATE STAGE that can go red on a docs-shaped edit, and it is not visible from the Rust side at all |

### The consequence the relay did not carry, and it is R5's

`validate_envelope` is not only an APPEND check. It also runs on the read paths:
`core/events/src/local.rs:1518` and `core/events/src/projection.rs:740` and `:1148`. So an
undeclared kind does not merely fail to be written — **a journal that somehow contained one would
also fail to REPLAY.** R5 asserts replay reproduces the identical exception set; under a missing
schema entry R5 dies in setup, and its named sabotage (`as_of` derived from the replay clock)
never gets to run. A cell that dies in setup is not a red at its named assertion — it is a
harness break wearing a red's clothes.

### Consequence for the order of work

Schema entries for all five are **step 0 of phase 2**, before any fixture that appends. §5's order
is amended accordingly. Writing fixtures first would produce six cells failing identically in
setup, which is exactly the shape that makes a sabotage unfalsifiable.

### The serde trap, carried forward but NOT yet applicable

`#[serde(rename_all = "camelCase")]` on an enum renames VARIANTS, not the fields inside a
variant-struct — that needs `rename_all_fields`. My payloads are not written yet, so there is
nothing of mine to correct; the note exists so phase 2 checks the emitted JSON against site 1's
`camelCase` field names rather than against the Rust source. A's judgement stands and is the
cheap direction: fix the TYPE while nothing has published the event, because the schema is a
published contract the moment one journal carries it.

---

## 7. Step 0 made reviewable: the five payload shapes, BEFORE they are written

§6 established that schema entries come first. This section is what gets reviewed before any of it
is typed, for the reason A paid for at the type level: **while nothing has published these events,
their shape is free to change; the moment one journal carries one, it is a published contract.**
Writing the payloads straight into the schema would seal five shapes with no reader — the same
defect as sealing a cell with no fixture (§3's own rule, one layer down).

### 7a. A COLLISION WITH THE M09 PRECEDENT, found by reading it rather than remembering it

`WakeLease::matures_in_seconds` carries a **DURATION** on the wire, and the fold computes the
instant from the event's OWN `occurred_at`. Its doc states the reason: *carrying the instant
instead would mean computing it from a second clock reading microseconds from the one that stamped
the event — two clocks answering "when did this happen".*

**My `sweep_performed` carries `as_of` as an INSTANT on the wire, which looks like the exact thing
that precedent forbids.** It is not, and the difference has to be written down here or a reviewer
will read it as a violation — or, worse, someone will later "align" it and destroy the property:

- `matures_in_seconds` is a **BUDGET**: how long from THIS event. It is anchored to its own event
  by definition, so deriving the instant from `occurred_at` is not a convenience, it is the correct
  reading of what was declared.
- `as_of` is a **QUESTION**: the instant the sweep was asked to evaluate at. It is NOT necessarily
  the instant the sweep ran. An operator may legitimately sweep as of last midnight, and replay
  must reproduce that answer. Deriving `as_of` from `occurred_at` would silently rewrite the
  question to "now", which is the determinism property destroyed in one line.

**The precedent's actual rule survives intact in both cases: NO READER INVENTS AN INSTANT FROM A
CLOCK OF ITS OWN.** The lease reads the log's own stamp; the sweep reads the argument the caller
supplied and the journal recorded. **Neither consults a clock at read time.** That is the property.

**The rule was never "durations on the wire" — that was the SHAPE the rule took where the value was
a budget.** The distinction between a rule and the shape a rule takes in one instance is what dies
when someone aligns two sites for looking different, and this paragraph exists to stop that review.

**And `dlq_returned` is the positive control for this whole argument.** If all five payloads carried
instants, the reasoning above would read as a rationalisation built around the conclusion. One of
them obeys the precedent — `waitWithinSeconds` is a DURATION, because the reopened wait's budget is
anchored to its own event — which shows the criterion was applied rather than constructed backwards.
R2's named sabotage (carry the original wait's deadline through the return) is precisely what an
instant on the wire would invite.

**Why "cheap now, impossible later" is literal here and not a figure of speech.** A published wire
tag is referenced by events already committed, whose hash chains re-verify on every replay.
Renaming one does not edit the past — **it forks it**: old journals verify against the old name and
new ones against the new, and no migration reconciles them without rewriting history that the chain
exists to make unrewritable.

### 7b. The five payloads

All five are execution-scoped (site 3 of §6 is `scopeWithExecution` for each; `is_project_level`
lists only the five integrity/evidence kinds and none of these belong there).

| kind | fields | why each field, where it is not obvious |
|---|---|---|
| `dlq_routed` | `executionId`, `nodeId`, `episodeSequence`, `reason` | `episodeSequence` is the stage entry being CLOSED by the routing, captured, not re-derived — the `WakeLeaseConsumed` precedent (record the side the log does not already know) |
| `dlq_redrive` | `executionId`, `nodeId`, `dlqEpisodeSequence` | names the dead-letter episode being redriven; the NEW episode's identity is this event's own sequence, so it is never carried in the payload (deriving it would let a caller assert an identity the log contradicts) |
| `dlq_returned` | `executionId`, `nodeId`, `dlqEpisodeSequence`, `waitWithinSeconds` (optional) | the DURATION precedent applies here and is the right shape: the reopened wait's budget is anchored to THIS event, so the fold computes the deadline from `occurred_at`. R2's sabotage — carrying the original wait's deadline through — is exactly what an instant on the wire would invite |
| `sweep_performed` | `executionId`, `asOf`, `caller` | `asOf` is an INSTANT for the reason in 7a. `caller` distinguishes an automatic tick from an operator-forced sweep, because §1b promised a background actor a reader can SEE |
| `overdue_exception` | `executionId`, `nodeId`, `episodeSequence`, `stage`, `deadline` | `deadline` is the instant the episode was measured against — recorded so a later reader can check the verdict without re-deriving it from a graph version that may since have been superseded |

**`waitWithinSeconds` is OPTIONAL and absent means ABSENT** — never zero, never a default. That is
R7's guarantee reaching the wire, and it uses the same `skip_serializing_if` shape as
`matures_in_seconds`. A node returned without a declared budget gets no deadline, forever.

### 7b-i. `Option` has TWO schema spellings and they mean different things

N derived the model while closing the schema end to end; verified here against both halves rather
than adopted, because the half that does NOT apply to me is the one that tells me whether the rule
was derived or assumed:

| Rust | schema | count in `event.rs` |
|---|---|---|
| `Option<T>` **with** `skip_serializing_if` | key ABSENT from `required`, type NOT nullable | 8 — incl. `maturesInSeconds`, `capturedArming` |
| `Option<T>` **without** it | key **IN** `required`, type **nullable** via `oneOf [T, null]` | 2 — `previousState`, `previousMode` |

**The two spellings are not stylistic, they make different claims**, and that is why this matters at
birth rather than at review:

- **absent key** = the field was never declared. Nobody said anything.
- **present null** = the field WAS declared and its value is nothing. `previousState: null` is the
  first transition asserting *there was no previous state* — a positive statement about history.

**`waitWithinSeconds` takes the first spelling, and taking the second would silently break R7.**
`waitWithinSeconds: null` would say "a budget was declared, and it is nothing", which is a claim
about the return that nobody made, and it is one careless `serde` line away. R7's sabotage is
defaulting the absent budget to zero; **this is the same defect wearing schema clothes**, and it
would be introduced by the shape rather than by the fold — invisible to a fixture that only
exercises the fold.

The other four payloads have no optional fields, so they take neither spelling and the question does
not arise for them.

### 7c. What is deliberately NOT in these payloads

- **No count, no total, no "swept N episodes" summary on `sweep_performed`.** The exceptions ARE
  the record; a count beside them is a second source that can disagree with the first, and the
  disagreement would be undecidable from the log alone.
- **No wall-clock beside `asOf`.** The envelope already stamps `occurredAt`. A second instant in
  the payload would be two clocks answering the same question, which is the precedent's actual
  target.
- **No node NAME anywhere.** Identity is id and sequence; names repeat across re-parks (§2).

### 7d. Order, now that the shapes exist

1. This section reviewed. Shapes are cheap to change until one journal carries one.
2. Rust variants + payload structs, `rename_all_fields` checked against the EMITTED json.
3. The four schema sites per kind, including `schemas/catalog.json`'s sha256 pin.
4. `cargo check -p` at the cheapest grain — not a slot — to confirm the types compile.
5. Fixtures R1-R7, which need #160's `Option<PersistedTimestamp>` on the node and therefore wait
   for it to land on main. **Measured 2026-08-20 14:05Z: #160 is NOT on `origin/main` (d0b3f04);
   `git grep PersistedCustoms origin/main` is empty.** Writing the fixtures against a type that
   exists only on another agent's branch is writing against a moving target, and building against
   that branch through the shared target dir is the contamination the slot law forbids.

---

## 8. Answers to N's review — four changes, three of them holes I did not see

N reviewed against `bbd8551` and re-checked against `b2b91fe` by content. Three findings survive
literally and one got stronger. All four are accepted; none is argued down.

### 8a. N1 — `as_of` IN THE FUTURE, and it is the worst of the four

**The hole.** §7a defends `as_of` as an INSTANT because it is a QUESTION rather than a budget. It
never says WHICH instants are askable. N derived the consequence I missed: a sweep with
`as_of > occurred_at` mints exceptions for episodes that are not overdue yet — and because §2
guarantees **one exception per episode, forever**, the honest later sweep emits NOTHING for them.
A single future-dated sweep permanently silences the detection it was supposed to perform.

**DECISION: `as_of > occurred_at` is REFUSED at the command layer. Future is not askable.**

Why refusal rather than "legal, with the meaning stated": a question about the future has no honest
answer here. The sweep does not predict; it compares deadlines to an instant. Asking which episodes
are overdue as of tomorrow asks the log to assert something it cannot know, and the answer would be
indistinguishable from a real one. **Past `as_of` is a different case and stays legal** — asking
what was overdue at midnight is answerable from the log alone, which is exactly the property that
`as_of`-as-argument exists to give.

**Where the refusal lives, and the part that needs saying:** the command layer refuses, so no such
event is ever appended. The fold does NOT get a second, private copy of the rule — a fold that
silently reinterprets a journaled event is the defect this project refuses everywhere, and a
journal that somehow contains one should be visible rather than quietly re-judged.

**R8, new sealed cell.** Fixture: `sweep(as_of)` where `as_of` is after the appending instant.
Asserts: refused, nothing appended, and — the assertion that matters — a subsequent honest
`sweep(as_of = now)` still emits the exception for that episode, proving nothing was consumed by
the refused call. **Named sabotage:** remove the command-layer refusal. **Falls at:** the second
sweep's exception being absent, NOT at the first call's error — a test that asserted only the
refusal would pass against an implementation that refuses AND still marks.

### 8b. N2 — the five `$defs` keys, named, and the convention measured

N is right that `<payloadName>` was a placeholder, and a sha256-pinned artifact cannot ship with
one. Measured across all 32 existing kinds:

**23 of 32 are the straight camelCase of the wire tag. NINE ARE NOT**, and every one of the nine
drops a leading domain word: `graph_validation_failed` to `validationFailed`,
`policy_waiver_created` to `waiverCreated`, `integrity_checkpoint_created` to `checkpointCreated`,
`evidence_legal_hold_changed` to `legalHoldChanged`, and five more. **It is not a rule** —
`graph_imported` KEEPS its prefix as `graphImported` while `graph_validation_failed` drops it. The
nine are historical, so the key is a CHOICE at birth and never a derivation.

**The five, chosen and stated rather than derived:**

| wire tag | `$defs` key |
|---|---|
| `dlq_routed` | `dlqRouted` |
| `dlq_redrive` | `dlqRedrive` |
| `dlq_returned` | `dlqReturned` |
| `sweep_performed` | `sweepPerformed` |
| `overdue_exception` | `overdueException` |

All five are the straight camelCase, joining the 23-case majority. `dlq` is not a droppable domain
prefix — it is the subject, not a namespace, so the nine-case pattern does not apply.

### 8c. N's question 5 — the LINK, which is better than the count I refused

§7c refused a count on `sweep_performed`, on the grounds that a count beside the exceptions is a
second source that can disagree with the first. N accepted that and named what is actually missing:
**nothing says WHICH exceptions a given sweep minted.** For events not yet written a field could be
added later; for events already written it could not — so the link has to be derivable from ORDER.

**DECISION: `sweep_performed` and every `overdue_exception` it mints are ONE APPEND BATCH.**
Adjacency IS the link, and replay reads it with no field at all.

This resolves both items at once, which is why N said one sentence would do it: the batch makes the
link derivable AND makes partial loss impossible by construction — there is no state where the
sweep is recorded and its exceptions are not, or the reverse. A count would have been a weaker
version of a guarantee the batch gives for free.

### 8d. The populations — §1a promised more than §7b delivers

N read §1 against §7b and found the seam. §1a promises that an item cannot silently park FROM THE
OPERATOR'S POINT OF VIEW, with detection a function of `(deadline, as_of)`. §7b says a node returned
without a declared budget **never gets a deadline**. A node with no deadline has nothing to compare
`as_of` against — **so it is invisible to read-time detection AND to the sweep.** The promise was
written without the exception, and the exception arrives two sections later.

**The promise is SCOPED rather than the behaviour changed.** R7 exists precisely to guarantee that
an undeclared budget never becomes an overdue verdict, and inventing a default so that §1a's
sentence comes out true is that cell's named sabotage. So:

> §1a's guarantee holds for episodes **with a declared budget**. An episode whose node declared no
> budget for its stage is deliberately NOT watched: nobody said how long it may wait, and the system
> does not answer a question nobody asked.

**And the residue that scoping leaves, named rather than hidden:** a graph published before customs
existed parks its nodes outside the anti-parking property entirely. That is correct — the
alternative is every pre-customs execution reading as overdue on every glance — but it means the
property's population is a function of what the AUTHOR declared, and an operator reading "nothing is
overdue" is reading a statement about the WATCHED SET, not about the execution. Whether the glance
should SAY that — surfacing unwatched-parked episodes as a separate, non-overdue category — is a
question for the read surface, and it is now **issue #198** rather than only a line here.

N flagged why the line was not enough, and the reasoning is this project's own: **a residue recorded
in a design doc that the next phase supersedes disappears**, and a DECLARED gap with no consumer is
indistinguishable from a HIDDEN one — only with a clearer conscience. In this document it has a
reader for as long as the document is read; as an issue it has a CONSUMER, the next person who
touches the glance.

### 8e. What N confirmed, kept short because agreement is cheap

The `WakeLeaseConsumed` criterion is APPLIED and not merely cited, and the proof N gives is the
PAIR rather than the prose: `dlq_routed` carries the episode it closes, and `dlq_redrive`
deliberately does not carry the episode it opens, because that identity IS the event's own sequence.
The `as_of` divergence is constructed, derived independently by N from `projection.rs:1035-1038`.
`dlq_returned` is the same criterion rather than a convenient exception. And none of the other four
payloads has a legitimate optional field — `deadline` in particular cannot be absent, since an
episode with no deadline never becomes overdue, so an `overdue_exception` always had one.

---

## 9. The `CustomsStage` collision, and the mapping VERIFIED rather than asserted

Measured 2026-08-24T08:44Z against `origin/main` = `97710e7`. Two enums, same name, same
`rename_all = "snake_case"`, different crates — so nothing fails to compile and the overlap is only
visible on the wire.

| | where | variants |
|---|---|---|
| **main** | `core/events/src/projection.rs:146` | `Parked, Claimed, Cleared, Rejected, Refused, Overdue` |
| **this lane** | `core/protocols/src/event.rs:768` | `Waiting, Claimed, DeadLettered` |

**One enum survives: main's.** The authority is not deference — it is the interface main published:

> *"The enum is OPEN TO GROWTH by design: lane 1 mints only the stages lane 1 produces, **and the
> DLQ lane adds its own**."*

Accepting that is using a door whose owner wrote the name on it.

### 9a. The mapping, variant by variant, with each side's own words

`Waiting` → `Parked`. Main: *"The node parked and a wait was minted (this entry's `at_sequence` IS
that wait's identity)."* This lane's §2: the episode is *"the envelope sequence at which the node
entered its current customs-holding state (`Waiting`, …)"*. **Same referent, and the two even agree
that the entry sequence IS the identity.** The rename is a rename.

`Claimed` → `Claimed`. Main: *"Testimony recorded against the open wait. Releases nothing."* Same
referent. **The name collision here is the dangerous one and it resolves benignly** — but only
because the two meant the same thing, which had to be read to know.

`DeadLettered` → **no counterpart.** This is the variant the DLQ lane adds, exactly as invited.

### 9b. The third overlap check, and it found something — just not where it was expected

Asked to confirm no other semantic overlap before deleting this lane's enum: mine has three
variants, main's has six, and only one pair had been mapped. Checking the remaining four against
`DeadLettered`:

- `Cleared` — *"the only stage that releases a dependent"*. Not dead-letter.
- `Rejected` — *"clearance withheld with a reason; the claim is spent, **the node stays parked**"*.
  Explicitly still parked, so not dead-lettered.
- `Refused` — *"the command layer would not accept the claim; state unchanged"*. Not a state change
  at all.
- `Overdue` — *"A stage deadline lapsed and a sweep said so."*

**`Overdue` does not collide with `DeadLettered`. It collides with this lane's EVENT.**

That sentence describes precisely what `overdue_exception` records. So the same fact has two
candidate homes: an event in the journal, and a timeline entry in the fold. **That is not resolved
here, and it is not a blocker for the rename — it is a question the implementation must answer
before both exist:**

1. If the fold appends `CustomsScan { stage: Overdue }` when the sweep mints an
   `overdue_exception`, the timeline entry is DERIVED from the event and must never be writable
   independently — otherwise two sources can disagree about whether an episode lapsed, and nothing
   in the log adjudicates.
2. If it does not, `Overdue` is a variant nothing emits — which is the exact thing main's own doc
   says not to do (*"declaring variants here that nothing emits would put a legal-but-never-produced
   value in front of every reader"*).

**Either answer is fine. Having neither written down is not.**

### 9c. A consequence of adopting main's enum that the rename hides

`overdue_exception.stage` answers *which stage was the node in when its deadline lapsed*. Under this
lane's three-variant enum every value was answerable. Under main's six, `stage: Overdue` becomes
**expressible and meaningless** — "the stage that lapsed was overdue".

So adopting the shared enum requires one of:
- a documented restriction naming which variants are legal in this payload, enforced at the command
  layer and stated in the schema (`enum` subset in `$defs`), so the wire cannot carry the nonsense;
- or accepting a legal-but-never-produced value in a payload, which is what main's doc warns
  against.

**Recommendation, not decision: the schema restricts.** `$defs/customsStage` for this payload lists
the legal subset rather than referencing the full enum, and the restriction is the guard — a wire
contract that cannot express the nonsense beats a comment asking readers not to.

### 9d. What this changes about the reviewed shape

The `overdue_exception` payload reviewed in §7/§8 carried `stage: CustomsStage` meaning this lane's
enum. **That review is pinned to the old shape and does not transfer.** The type changes, the legal
value set changes, and §9c may add a schema-level restriction that did not exist when the shape was
read. A delta-review of the payload is owed before implementation.

### 9e. The two decisions, recorded here because a decision that lives only in a channel disappears

Both taken by the orchestrator on 2026-08-24 in answer to §9b and §9c.

**DECISION 1 — the `Overdue` timeline entry is DERIVED from `overdue_exception` in the fold, and
nothing else writes it.** This is option 1 of §9b, and the reason is the architecture already in
place rather than a preference: the journal is the source and the fold derives. A timeline entry
writable independently of the event would be a second source able to disagree about whether an
episode lapsed, with nothing in the log to adjudicate.

And main's own wording closes the circle rather than merely permitting it: `Overdue` is documented
as *"a stage deadline lapsed and **a sweep said so**"*. **This lane's event IS the "sweep said so".**
The derivation is what the sentence was already pointing at.

**DECISION 2 — the `overdue_exception` payload restricts to the legal subset in the schema, and
never references the whole enum.** §9c's recommendation, adopted as decision. The wire cannot carry
`stage: Overdue`, so the nonsense is not merely discouraged, it is unrepresentable.

The principle it belongs to, named by the orchestrator across three instances landing the same day:
**illegal by construction beats forbidden by policy.** The other two are #221 (the field where an
injection would fit does not exist) and the two-element decision array. A comment asking readers not
to write something survives exactly as long as everyone reads the comment.

### 9f. What the single pass now owes, in one list

Sequence is fixed: nothing here happens until #160 lands, because `core/events/src/projection.rs` is
in another lane's conflict map and a third hand in that file is the cost being avoided. Then, in ONE
pass rather than two:

1. rebase onto the landed main;
2. delete this lane's `CustomsStage`; adopt main's; add `DeadLettered` to it, per the published
   invitation;
3. carry the §9a mapping into the PR body **as quotations from both sides**, because "it is the
   same" is a claim about meaning that no digest covers;
4. implement the derivation arm of DECISION 1 — the fold appends the `Overdue` timeline entry from
   the event, and no other writer exists;
5. restrict the payload in the schema per DECISION 2, and let the restriction be the guard;
6. write R1–R8 against the final type, never against the intermediate one;
7. hand N a delta-review of the whole payload shape — the earlier review is pinned to a shape that
   had a different type, a different legal value set, and no schema restriction.

**The one thing this list must not do is start at 6.** Fixtures written against the intermediate
type would be rewritten by step 2, and a fixture rewritten mid-lane is where a guard quietly stops
asserting what it was built to assert.

