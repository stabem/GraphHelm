# Schema Changelog

## [1.0.0] - 2026-08-09

- Prepared the initial pre-release `1.0.0` baseline with 15 authoring and safe persistence contracts; this package has not yet been publicly published.
- Added the bounded `PersistedGraphVersion`, safe event envelope, encrypted Evidence metadata, artifact reference, repository scope, and sensitivity contracts.
- Corrected the bounded policy waiver contract while preserving the graph, agent, node, and edge authoring contracts.
- Corrected the single pre-release `1.0.0` baseline in place under D-037/ADR-023 by registering the typed `context_path`, `permission_path`, and `isolation_path` content positions. This is not a published-version migration and has no legacy alias or intermediate release.
- Canonicalized persistence timestamps to uppercase UTC `Z`, four-digit years, and at most nine fractional digits so schema validation and Rust round-trips remain exact.
- No predecessor release or persistence-format migration exists for this initial baseline.
- Preserved the provisional `p50.dev` schema IDs and wire-format identifiers.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `ghost` node state to `event-envelope.schema.json`'s `nodeState` enum, closing a gap where `graphhelm_protocols::NodeState::Ghost` could not be represented in a `NodeStateChanged` event on the wire.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `execution_started`, `execution_mode_changed`, `node_outcome_recorded`, and `execution_completed` event kinds to `event-envelope.schema.json`, closing a gap where the durable execution wire contract had no schema-validated representation.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `signal_recorded`, `ghost_node_proposed`, and `mutation_accepted` event kinds to `event-envelope.schema.json`, closing a gap where the in-flight governance wire contract had no schema-validated representation.
- Corrected the single pre-release `1.0.0` baseline in place under D-037 by adding the `execution_paused` and `execution_resumed` event kinds and widening the `nodeOutcome` and `simulationStatus` enums with `paused`, `interrupted`, and `cancelled` in `event-envelope.schema.json`, closing a gap where the pause, resume, and cancel lifecycle had no schema-validated representation.
