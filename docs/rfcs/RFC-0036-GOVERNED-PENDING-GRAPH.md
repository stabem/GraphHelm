# RFC: Durable pending graphs and governed approval recovery

Status: Proposed implementation clarification for issue #36.

## Evidence and affected contracts

The graph-engine Governor design, sections 5.1 and 5.5, publishes a successor
Graph Version only when a mutation is accepted. Section 5.4 keeps signals from
mutating the active graph. Section 5.2 nevertheless describes a Ghost as present
in the persisted graph. The word "graph" does not distinguish pending topology
from the accepted executable version.

The existing safe persisted version is not an inverse representation of the
authoring graph. Reconstructing it from a caller-supplied graph during approval
would introduce a second, untrusted source of identity. Pending node IDs alone
also cannot recover the full typed proposal after restart.

## Alternatives

1. Publish a successor active version when a proposal arrives. This changes the
   accepted-only publication contract and gives an unaccepted proposal an
   operational version.
2. Persist only pending IDs and accept authoring content from the approval
   caller. This loses recoverable proposal content and makes approval depend on
   new caller input rather than the proposal the owner reviewed.
3. Keep a durable pending view beside the immutable active version and recover
   authoring content from sealed evidence. This is the recommended interpretation.

## Recommended interpretation

The active graph contains accepted operational topology. Only Governor acceptance
publishes its successor. The pending view contains Governor-created Ghost nodes
and proposed edges reconstructed from durable proposal events and the exact
sealed typed draft. Ghosts never enter the scheduler's ready set.

New governed executions retain a sealed complete `GraphVersionRecord`. A sibling
typed event binds its evidence reference and digest to the execution and published
version. The snapshot binding and publication share one append transaction. Every
accepted successor carries a new binding in the same transaction; retaining only
the initial snapshot would break a second approval after restart.

Recovery checks scope, digest, size and schema. `GraphVersion::from_record`
verifies authoring canonicalization and hash. The Governor projects the recovered
record with the persisted predecessor and requires equality with the entire
active safe persisted version. Authoring and safe persisted hashes are distinct
identities and must not be compared as though they were interchangeable.

Approval recovers the exact sealed proposal rather than accepting replacement
graph content. Publication, approval, assignment and executable reconciliation
must have a single durable outcome and a defined retry path. Assignment identifies
the responsible agent; the actor that records an event remains separately visible.

## Failure and compatibility behavior

An older execution without a recoverable snapshot reports content unavailable.
It is not silently migrated. Missing or erased required evidence blocks recovery;
the snapshot must not restore content whose evidence has been erased. This also
applies to optional content that reconstruction would otherwise copy again.
Errors must not disclose sealed plaintext or credentials.

A mismatched execution, digest, active version, actor or proposal fails closed.
Ghost visibility is not execution, and successful dispatch is not validated task
completion. Studio must show pending, assigned, processing and result evidence
without promoting an incomplete check to success.

## Required observations

- Restart restores the pending proposal's exact recoverable topology.
- A Ghost cannot dispatch before acceptance.
- Approval publishes the reviewed proposal and its successor snapshot together.
- A second proposal can be approved after restart using that successor snapshot.
- Scope mismatch, unavailable evidence and a stale approval leave the active
  version unchanged.
- Replay preserves assignment and distinguishes the responsible agent from the
  event-recording actor.
- The actual browser journey separately observes pending, assignment, progress,
  result and reload. Offline tests do not substitute for that observation.

This RFC records the ambiguity and recommendation. It does not claim that the
implementation or browser journey has already met these observations.
