# Schema Changelog

## event-envelope 1.0.0 - `wakeLeaseConsumed.capturedArming` added

Which arming a consumption was FOR: the sequence of the `wake_lease` event the sweep read when
it built its capture.

**Why the captured side and not the live one.** The defect this exists for is a mismatch between
what a sweep captured and what was live when it recorded — a sleeper wakes, re-arms on the same
rendezvous, and a delayed sweep burns the lease it just armed. Replay already knows the live
side; it rebuilds it from the arming events. Only the captured side was ever unknown to the log.
Recording the live one would compare a value against itself: always equal, a check that cannot
fail. The two versions are one field of the same name and type, and nothing in a diff
distinguishes them, which is why this note exists.

**Why optional, and omitted rather than null.** Every event is re-hashed on replay. A
`"capturedArming": null` on consumptions written before this field existed would change their
canonical bytes and break the hash chain of every one of them — the same reasoning that governs
`nodeOutcomeRecorded.reason`. It is absent from `required`, so committed events stay valid, and
`skip_serializing_if` keeps absence off the wire.

**What actually enforces that, so whoever loosens it knows what they are unguarding.** The live
guard is THIS SCHEMA'S `"type": "integer"`: a null is not an integer, so validation rejects the
append before anything downstream sees it. Measured rather than reasoned — removing
`skip_serializing_if` fells four tests in `core/events/tests/execution_projection.rs`, every one
of them at its own append. Relaxing that type is therefore not a cosmetic edit: it removes the
only thing standing between an absent value and a broken hash chain across all committed
consumptions. Loosening it is a deliberate act that passes through the digest and catalog ritual,
which is where this sentence is meant to be met.

**Where we stopped measuring, stated rather than implied.** The compound state — schema loosened
AND `skip_serializing_if` removed — is EXPECTED-UNMEASURED. Its direct backstop is a wire-absence
assertion in `a_matching_consumption_and_a_pre_change_one_record_no_mis_burn`, which is dormant
while the schema holds and becomes a single-sabotage blade the moment the type is relaxed; the
test carries its own note saying it is currently redundant and why. Behind that, the fixture
journal's integrity verification should refuse a struct that emits null, since it re-hashes every
committed consumption — but that is a READING, not a measurement, and it is labelled as one.
Reaching the compound state takes two deliberate changes, each individually guarded and each
meeting a written warning, and we drew the line there on purpose. Saying so beats pretending the
line does not exist.

**What absence means, permanently.** No consumption committed before this change carries the
captured side, so no future analysis can decide whether this defect ever fired in the past.
Replay can say which lease was burned; nothing can say which one the sweep meant to burn, and
the discrepancy between them IS the defect. "Precondition present, incident not observed" is the
strongest claim the old data can ever support — not because nobody has looked hard enough, but
because the log recorded one side of a two-sided property.

**What the fold does with a mismatch: records it, refuses nothing.** A consumption naming an
arming other than the one it burns is a wrong action FAITHFULLY RECORDED, which is a different
thing from the log the fold already refuses — a consumption with no live lease at all, which
cannot be interpreted. Refusing here would make history unreadable because it recorded something
bad, on a product whose thesis is that history reproduces, and it would brick the glance at the
moment an operator most needs it. The mismatch lands in the projection's `wakeMisBurns` instead,
where the attention verdict can reach it. Recorded but not yet surfaced: the wiring to attention
is seeded, not shipped here.

## event-envelope 1.0.0 - `wakeLease.maturesInSeconds` added

How long a sleeper's quiet may last, declared at arming and carried on the lease that already
records the sleeper's intent to be woken.

**Why a field on `wake_lease` rather than an expiry event of its own.** Whether the horizon has
passed is DERIVABLE from the horizon and the current instant, and an event whose entire content
is a derivable fact is a cached counter living inside the log — the same defect this milestone
is otherwise removing from outside it. The lease is also already excluded from the doorbell's
content predicate BY CONSTRUCTION, so arming an alarm cannot inflate the `contentHead` an
operator reads as progress. A new event kind would have had to be added to that exclusion by
hand, and a rule that must be remembered for every future kind is a rule one of them will
forget.

**Why a duration on the wire and an instant in the projection.** The first draft carried the
absolute instant, on the reasoning that a duration puts the horizon in the reader's hands. The
guard for it could not be written honestly: the instant would have been computed from a clock
reading microseconds away from the one that stamps the event, so the only assertion available
was "roughly now plus N" — and roughly-now passes for an implementation that measured from the
wrong base, which is the thing in question. The fold derives the horizon as `occurred_at +
maturesInSeconds`, arithmetic over the event's OWN recorded instant. No clock is read, replay
stays byte-identical, the reader is still handed an instant, and there is exactly one clock in
the story instead of two microseconds apart.

**The stored instant is a timestamp, not its wire string.** The canonical rendering emits 0, 3,
6 or 9 fractional digits depending on the value, so `...:01.500Z` sorts BEFORE `...:01Z` — `.`
is 0x2E and `Z` is 0x5A. A projection holding the string would answer ordering questions
backwards for the commonest pair there is, since the horizon inherits the fraction of whatever
instant stamped the arming event.

**It is stored, never evaluated below the surface.** Maturity is asked at the surface with an
injected instant. A projection that cached `matured` would be a function of the wall clock, and
byte-identical replay would fail only on the machines whose clock crossed the horizon
mid-replay — the quietest possible way to lose the property the event store exists for.

**Bounded at BOTH ends, and the loose end was the dangerous one.** A declared bound of zero is
not a bound and is refused. So is one beyond ten years: a trillion seconds produced a horizon in
the year 33715, and a surface that answers with a DATE reads as a promise while meaning never —
absence laundered into calm through arithmetic. Past `i64`, the conversion overflowed and
yielded no horizon at all, so an operator who declared a bound silently received none. The
ceiling is `315576000`, the one `nodeTimeoutSeconds` and `observedSilenceSeconds` already use
for a declared duration, so the answer is the house's existing one rather than a number invented
here.

**Absent means absent.** A lease armed without a bound gets no expiry promise, and none is
invented for it. Existing leases in committed journals replay unchanged.

## event-envelope 1.0.0 - `execution_form_amended` added

A silence bound the operator declares AFTER a run began.

**Why a separate event rather than a new field on `execution_form_declared`.** The
declaration is the shape a run started with; the amendment is a decision made later. Folding
the second into the first would erase the boundary between them, and that boundary is the
whole point: an amendment binds FORWARD, from its own sequence. A replay positioned before it
still answers with what was known then, so the timeline reads "not judged for twelve minutes,
judged from here" rather than "it was fine all along". Nothing an operator declares can make
the history claim it knew something it did not.

For the same reason the projection keeps amendments as a LIST in log order rather than
folding them into one map. A folded map cannot be un-folded to an earlier moment, so the past
would silently inherit a decision taken after it.

**`computedAtSequence`** names the frontier the amendment was computed against, so an
amendment that cannot be placed in the history is refusable. It exists before the submission
path does, precisely so that path can refuse rather than guess.

**`observedSilenceSeconds`** records what the operator was looking at when they decided, so a
later reader sees the decision in its own light rather than in hindsight's.

**Old histories keep reading.** A stream with no amendment folds exactly as before and its
hash chain is untouched: the projection field carries `skip_serializing_if`, so nothing new
is serialized for streams that never amended.

## event-envelope 1.0.0 - `execution_form_declared` added

A new replay-safe event carrying the shape the operator DECLARED for an execution: the node
set, and the deadline each node declared. It is recorded WITHOUT a seal, and that is the whole
point of it being a separate event rather than another `graph_version_published`. A seal exists
to protect the evidence behind content slots; a declared shape carries no evidence to protect.
The two ideas had been fused into one type, which forced every rule that merely needs the shape
to demand a credential it has no use for.

`nodeTimeoutSeconds` holds an entry ONLY for a node that declared a deadline. An absent key
means the operator declared nothing, and must never be read as a budget of zero - zero would
make every undeclared node look permanently overdue.

Histories written before this event existed hold an `execution_started` with no declaration and
keep replaying unchanged; the projection reads their missing declaration as UNDECLARED. That is
an honest unknown, not a clean bill of health.


## [1.0.0] - 2026-08-09

- Prepared the initial pre-release `1.0.0` baseline with 15 authoring and safe persistence contracts; this package has not yet been publicly published.
- Added the bounded `PersistedGraphVersion`, safe event envelope, encrypted Evidence metadata, artifact reference, repository scope, and sensitivity contracts.
- Corrected the bounded policy waiver contract while preserving the graph, agent, node, and edge authoring contracts.
- Corrected the single pre-release `1.0.0` baseline in place under D-037/ADR-023 by registering the typed `context_path`, `permission_path`, and `isolation_path` content positions. This is not a published-version migration and has no legacy alias or intermediate release.
- Canonicalized persistence timestamps to uppercase UTC `Z`, four-digit years, and at most nine fractional digits so schema validation and Rust round-trips remain exact.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the OPTIONAL `reason` property to `nodeOutcomeRecorded` in `event-envelope.schema.json`, carrying a closed `nodeOutcomeReason` vocabulary (the §17 route classes plus the in-process causes: empty reply, malformed judgment, judge and gate refusals, the four tool dispositions, and fixture-scripted outcomes), closing the gap where a failed node recorded no actionable cause. The property is deliberately NOT `required` and is omitted when absent: replay re-serializes each envelope and recomputes its hash against the stored one, so an always-emitted `null` would break the hash chain of every event written before this milestone.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the OPTIONAL `timeoutSeconds` property to the persisted `node` in `persisted-graph-version.schema.json`. The graph already accepted a per-node timeout and the linter already warned when an executable node omitted it (`GHG101_DEFAULT_TIMEOUT`); persistence dropped it, so nothing downstream could derive a silence budget from a bound the user had already declared. The property is deliberately NOT `required` and is omitted when absent: every graph version already published must keep validating and keep its hash, and absence must stay ABSENCE — never zero, never a default — because downstream it has to read as unknown rather than as calm.
- No predecessor release or persistence-format migration exists for this initial baseline.
- Preserved the provisional `p50.dev` schema IDs and wire-format identifiers.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `ghost` node state to `event-envelope.schema.json`'s `nodeState` enum, closing a gap where `graphhelm_protocols::NodeState::Ghost` could not be represented in a `NodeStateChanged` event on the wire.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `execution_started`, `execution_mode_changed`, `node_outcome_recorded`, and `execution_completed` event kinds to `event-envelope.schema.json`, closing a gap where the durable execution wire contract had no schema-validated representation.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `signal_recorded`, `ghost_node_proposed`, and `mutation_accepted` event kinds to `event-envelope.schema.json`, closing a gap where the in-flight governance wire contract had no schema-validated representation.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `execution_paused` and `execution_resumed` event kinds and widening the `nodeOutcome` and `simulationStatus` enums with `paused`, `interrupted`, and `cancelled` in `event-envelope.schema.json`, closing a gap where the pause, resume, and cancel lifecycle had no schema-validated representation.
