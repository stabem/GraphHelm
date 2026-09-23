# Manual adoption checkpoint restore

## Goal

Issue #1203 addresses the gap between a verified local backup and restore eligibility. The current
restore planner accepts `original` or a backup ID that appears in an adoption journal. A backup
created directly by the operator is verified as bytes, but has no provenance that permits it to
drive a restore.

## Architecture

Manual checkpoints use the existing private backup store, authority locks, retained-handle guards,
journal, and recovery engine. They add a detached restore intent with no adoption source list, so
manual restore does not invent package compensation or ownership claims.

## Tech stack

Rust workspace crates `graphhelm-host-adoption` and `graphhelm-cli`, JSON manifests and schemas,
and deterministic offline tests. No model, provider, network, or runtime dependency is introduced.

## Spec

## Bounded semantics

Every newly created backup records private provenance in its private, hash-bound manifest: the retained
project and home root records, plus the retained state-root record. The manifest digest includes
that provenance, so changing a root, state directory, or provenance changes the checkpoint ID.
This hash detects corruption and binds the snapshot to its recorded roots; filesystem custody still
comes from the existing private owner-only backup directory. A legacy manifest remains verifiable
for integrity. It is restore-eligible when an existing adoption journal explicitly links its ID;
an unlinked legacy manifest is refused with `backup_unverified`.

Manual restore reopens the roots from the verified checkpoint provenance and requires their current
identities, paths, and the state-root identity to match. It then creates the same sealed
`RestorePlan` used by the existing offline flow. The manual plan keeps active adoption journal
digests as staleness evidence but does not claim or compensate their packages or shared-user
ownership. Present supported surfaces are explicit reviewed replacements to the checkpoint bytes
and access metadata; this differs from ownership-aware original rollback. Absent checkpoint
surfaces are retained and shown as `retain_unowned`, so the result is not an exact file-tree claim.
A plan binds current bytes and access facts, so any edit after preview makes apply return
`plan_stale` before publication. Apply uses the existing authority locks, retained-handle guards,
journal, recovery, and exact digest acceptance.

## Implementation steps

- [x] Add hash-bound checkpoint provenance and access metadata to backup manifests. Keep legacy
   verification separate from restore eligibility.
- [x] Add a manual restore preparation path that validates provenance, roots, state binding, snapshot
   bytes, access metadata, and the active journal binding without writing.
- [x] Route manual plans through the existing apply and recovery engine with an empty adoption source
   list, preserving the existing linked restore path.
- [x] Add focused adapter regressions for direct manual restore bytes and permissions, metadata-only
   changes, unbound and foreign snapshots, corrupt data, preview drift, repeated same-edit cycles,
   no-op history markers, and interruption/recovery; retain the existing linked cases.
- [x] Run focused tests, formatting, clippy for affected crates, and report full-gate execution to the
   orchestrator for its required final validation.

## Constraints and review focus

- Preserve original rollback, journal-derived package and shared-user ownership, and later edits.
- Refuse malformed, unbound, corrupt, stale, foreign-root, busy, and ownership-conflicting input
  before publication.
- Treat absent checkpoint surfaces as retained unowned state and report that partial recovery.
- Verify real bytes and access metadata, including Windows ACL changes and repeated restore.

## Recorded results

Focused host adoption tests: 12 backup and 19 restore cases passed. The detached manual
interruption recovery test passed. Affected-crate Clippy and formatting checks passed. The CLI
backup and restore integration cases passed. The Windows ACL probe passed both metadata-only and
byte-plus-ACL restore cases with `restored` receipts. Follow-up validation adds journal-bound
same-edit cycles, idempotent retries, no-op history markers, and post-preview history drift refusal.
The authoritative full gate and independent review remain orchestrator work.
