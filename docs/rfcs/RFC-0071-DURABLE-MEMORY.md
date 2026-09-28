# RFC-0071: Durable opt-in memory

## Status

Proposed for issue #71. This RFC records the typed contract before implementation.

## Contract

Durable memory is an explicit project opt-in. A caller supplies a `MemoryCandidate` containing the
complete observation and its `DevelopmentScope`. The governor screens the candidate before it
constructs a record, asks a provider, logs content, seals evidence, or appends an event. Screening
checks opt-in, exact scope, captured origin, credential-shaped content, and a bounded expiry.

An admitted candidate becomes a `MemoryPublication` with a stable record id, exact scope, content
digest, expiry, and one sealed Evidence reference. The plaintext observation is stored only in
Evidence. The journal carries metadata and the Evidence reference. The candidate is published only
after an independent validator confirms the same scope and digest; producer self-validation is
refused. Sealing and the publication event are one `PreparedAppend`, so a failure leaves no record.

Replay reconstructs record lifecycle metadata from the journal. Retrieval accepts only records whose
semantic state is validated, publication state is published, expiry is after the supplied clock,
scope equals the request, and Evidence is available and digest-valid. Missing, stale, expired,
withdrawn, unpublished, or invalid Evidence produces no context item. Retrieval defaults to an empty
result when durable memory is disabled.

The contract is deterministic and provider-free. It adds no model, council, paid fallback, or global
capture switch. Existing publication and supersession events remain separate semantic and
publication axes. Frozen release schemas are unchanged; the new typed contract is represented by
existing event and Evidence envelopes and validated before replay.

## Keel card

- Paths: `core/governor/src/memory.rs`, `core/events/src/memory.rs`, `core/events/src/projection.rs`,
  `core/events/src/repository.rs`, `core/protocols/src/event.rs`, `core/protocols/src/persistence.rs`,
  `core/runtime/src/context.rs`, `core/runtime/src/retrieval.rs`, `core/runtime/src/ports.rs`, and
  the issue-71 tests.
- Promise: an opt-in candidate is screened, independently validated, sealed, journaled atomically,
  replayed after restart, and retrieved only when scope, digest, lifecycle, expiry, and Evidence all
  validate.
- Proof: focused governor/event/runtime tests, `cargo +1.97.1 fmt --all -- --check`, touched-crate
  clippy, workspace authored-string guard, and the relevant workspace tests from the committed head.

## Threats and recovery

Plaintext leakage is prevented by screening before every boundary and sealing before append.
Cross-scope retrieval, replay tampering, provider recapture, self-validation, stale dependencies,
and missing Evidence fail closed. An atomic append failure is retryable with the same idempotency key;
replay is the recovery path after restart. Rollback is a source revert; no external service or
dependency is introduced.
