# M06 fixture journey — a byte-archive of an event store

This directory holds a recorded demonstration, byte for byte, checksummed in `SHA256SUMS`:
the committed store, the seed frozen at recording, and the projection digest the current build
must reproduce on replay.

`events/` is a **byte-archive of a store, not a directly openable store**. A live local
repository also has the directories `blobs/`, `.tmp/` and `active/`, and all three were empty
when this journey was archived (a fixture journey seals no evidence). Git tracks files, not empty
directories, so they are absent here — and the store refuses a repository whose `format.json` is
present but whose layout is incomplete (`core/events/src/local.rs:2078-2082`), reporting
`GHE005_INTEGRITY_FAILURE`. Nothing is missing from the history.

The grounding test has restored this shape before replaying since #59, so the demonstration
binding has always been checked. A reader opening the directory by hand has not:

```sh
cp -r docs/acceptance/demos/m06-fixture-journey /tmp/journey
mkdir -p /tmp/journey/events/.tmp /tmp/journey/events/active /tmp/journey/events/blobs
graphhelm execution status --events /tmp/journey/events --pretty
```

Expected: `executionId` `demo-journey`, `headSequence` 10.

The workspace test `every_committed_acceptance_store_opens_and_replays_its_recorded_identity`
(`tools/acceptance-map/tests/committed_stores.rs`) covers this archive too, so every committed
store answers the same question in one place. Restoring empty directories adds no content and can
hide no loss: a real evidence blob is a tracked FILE and survives checkout.
