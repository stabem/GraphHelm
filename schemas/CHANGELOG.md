# Schema Changelog

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
