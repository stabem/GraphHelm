# RFC-0072: Dreams Shadow Journey

Status: proposed for implementation by issue #72

## Promise

GraphHelm can run one bounded Dreams analysis from an explicit input file. The run is scoped and
immutable, validates its input deterministically, asks an independent critic for a verdict, and
records an advisory proposal or discard. A code finding becomes a normal substantiated task
request. The journey never edits code, publishes an operational Graph Version, changes policy,
calls a model, spends BYOK, installs a plugin, or widens permissions.

## Scope and authority

This RFC implements D-011 and D-012 and the Shadow Workspace rules in
`docs/context/CONTEXT_KNOWLEDGE_DREAMS.md`. It defines only the first deterministic slice. The
Event Store remains append only, and every result is advisory metadata.

## Input and immutable shadow

The caller supplies one bounded UTF-8 JSON document. Its canonical bytes are hashed with SHA-256
before validation. The run records the repository scope, trigger, input digest, and a bounded
category. The canonical bytes are retained only in the caller's process; the event carries the
digest and safe metadata, never the input text or a filesystem path. A shadow is immutable: the
critic and validator receive the same canonical digest and cannot replace the input during a run.

Supported triggers are `manual`, `idle`, and `incident`. Supported categories are `memory_expiry`,
`conflict_report`, `retrieval_optimization`, and `code_finding`.

## Deterministic validation and independent critic

Validation rejects empty or oversized input, unknown trigger/category, missing evidence, and
scope mismatch. The validator emits a stable reason code. The critic is an explicit caller supplied
identity distinct from the planner identity. A proposal is admissible only when validation passes
and the critic accepts. Critic rejection produces a discard. No model or council is implicit.

## Advisory outcomes and task mapping

An accepted result is `advisory_proposal`; a rejected result is `discarded`. A `code_finding`
proposal additionally contains a bounded normal Task Request with the finding, evidence digest,
scope, and source Dream run identity. The task request is a request for ordinary Graph Engine
processing; it is not code, graph, or permission authority.

## Replay events

One append-only event records the run outcome and one optional task request. The event includes the
run identity, scope, trigger, category, input and evidence digests, planner/validator identity, critic identity and
verdict. Replay folds it into a keyed projection. Replaying the same event twice is deterministic;
the event does not mutate or erase prior events.

## Adoption and discard

Adoption means only that the advisory result is recorded as accepted by the deterministic
workflow. It does not publish memory, alter documents, change agents or skills, or apply a Graph
Draft. Discard records the refusal reason and preserves the input digest and provenance.

## Security and limits

The input is bounded before parsing and the canonical document is bounded before hashing. Dynamic
prose is not copied into events. Secrets are rejected by the existing bounded detector. Scope,
actor and critic identity are explicit. Missing evidence, an invalid critic relationship, and a
write-critical project state fail closed. Retries use a caller idempotency key and preserve event
lineage.

