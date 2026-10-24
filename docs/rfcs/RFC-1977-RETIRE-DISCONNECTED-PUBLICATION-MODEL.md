# RFC-1977: Retire the disconnected publication model

Status: Accepted by the owner on 2026-10-24

Date: 2026-10-24

## Decision

Retire the experimental public Rust exports `PublicationStep` and
`publication_steps` from `graphhelm_governor`. They described a fixed list of
publication labels, but they were not the publisher and had no production
caller. The list-only crash test that consumed them is retired with the
exports.

This is an intentional source-compatibility break for unobserved external Rust
consumers. Consumers that imported these experimental symbols must remove those
imports or move to the real publication API. No compatibility shim is added.

## Scope and retained obligations

The real publisher remains `prepare_durable_memory_publication`. Its HTTP caller,
sealed evidence, publication event, schema and event replay remain unchanged.
The durable-memory HTTP publication/restart/context observer remains the
user-visible proof. Event append/replay/idempotency and storage atomicity tests
also remain in place. These observers cover actual storage and replay behavior;
the removed fixed list did not execute a write or a crash boundary.

There is no persisted-data, wire-schema, event-kind, or replay migration. The
retirement changes only Rust source exports and their exclusive model-only test.

## Cost and complexity

The real publication path and its cost are unchanged. This retirement adds no
runtime branch, dependency, storage work, or algorithmic complexity; it only
removes a disconnected public list and its test.

## Alternatives considered

1. Keep the exports and rename the test. Rejected: the model is disconnected
   from the real publisher and would continue to imply crash coverage it cannot
   provide.
2. Replace the list with a new synthetic crash-step API. Rejected: that would
   add another production seam solely for a test and still would not observe the
   storage implementation.
3. Add a real crash-injection harness. Out of scope for this deletion-only
   cleanup: it needs a separately designed storage fault-injection contract and
   observer. The retained HTTP observer remains, but this RFC makes no
   every-crash-checkpoint guarantee.

## Test-deletion record

Test: `core/governor/tests/memory.rs::no_crash_point_leaves_a_record_published_without_its_evidence`.

Detects: only drift in the two-item fixed list and its own prefix assertion. It
does not append evidence, publish a record, restart a process, or exercise a
crash checkpoint in the event store.

Production callers: none. The only caller was this test; the retired symbols
were not used by the real publisher or HTTP route.

Covering observers retained: `apps/cli/tests/api_http.rs::durable_memory_publishes_replays_and_enters_production_context`
publishes through the real HTTP endpoint, verifies sealed evidence and the
publication event, restarts, replays, and retrieves production context.
The governor memory tests for publication transition, event append/replay and
idempotency remain. Event-store atomicity and replay tests remain in their owning
event tests. These are the real observers for the retained obligation.

History: the test was introduced as a blueprint/list model check; its fixed
sequence was never connected to the publisher implementation.

Unlocks: `PublicationStep`, `publication_steps`, their governor re-exports and
their test imports.

Risk: external Rust users may import the retired experimental exports; that
consumer population is unobserved and must adapt. The retained tests do not
prove every possible crash checkpoint. In particular, a happy restart does not
prove every crash boundary, so the implementation-crash-observation gap remains
explicit rather than being erased.

Validation: run the retained focused memory and HTTP observers, then the
computed reached plan at the exact head. No fault-injection exercise is
required for this source-only retirement; the retained real observers are the
evidence, and their crash-boundary limits remain explicit above.

## Security, dependencies and rollback

No secrets, permissions, validation rules, network behavior, or persistence
formats change. No dependency or workspace-version change is introduced.
Rollback is a source revert if an identified in-repository consumer needs the
experimental exports; external consumers must still be considered explicitly.
