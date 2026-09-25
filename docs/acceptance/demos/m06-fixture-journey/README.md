# M06 fixture journey — a byte-archive of an event store

This directory holds a recorded demonstration, byte for byte, checksummed in `SHA256SUMS`: the
committed store, the seed frozen at recording, and the projection digest the current build must
reproduce on replay.

`events/` is a **byte-archive of a store**. A live local repository also has `blobs/`, `.tmp/` and
`active/`, and all three were empty when this journey was archived — a fixture journey seals no
evidence — so git, which tracks files rather than directories, carried none of them.

Since #76 the store recreates `.tmp/` and `active/` on open: they are transient workspace holding
nothing the journal does not already carry. **`blobs/` is not recovered, on purpose.** A real
evidence blob is a tracked FILE, so a missing `blobs/` normally means evidence is gone, and the
store refuses rather than paper over it. This archive is the honest edge of that rule: its
`blobs/` is empty because there is nothing to seal, and an empty directory cannot be committed —
so opening it by hand needs that one directory back first:

```sh
mkdir -p docs/acceptance/demos/m06-fixture-journey/events/blobs
graphhelm execution status --events docs/acceptance/demos/m06-fixture-journey/events --pretty
```

Expected: `executionId` `demo-journey`, `headSequence` 10.

The directories created this way are untracked and empty, and no committed byte changes —
`SHA256SUMS` still verifies after an open.

Two tests cover this archive, and neither needs the command above: the grounding test has restored
the shape before replaying since #59, and
`every_committed_acceptance_store_opens_and_replays_its_recorded_identity`
(`tools/acceptance-map/tests/committed_stores.rs`) opens and replays every committed store in one
place. The command exists for the reader who opens the directory by hand — the case that was never
covered.
