# M05 acceptance run — a byte-archive of an event store

This directory holds the evidence of one real run, byte for byte, checksummed in `SHA256SUMS`.

`events/` is a **byte-archive of a store**. A live local repository also has the directories
`.tmp/` and `active/`, and they were empty when this run was archived; git tracks files, not empty
directories, so they are absent here. That absence used to make the archive unopenable
(`GHE005_INTEGRITY_FAILURE`). Since #76 the store treats those two as recoverable and recreates
them on open, because they are transient workspace: they hold nothing the journal does not
already carry. So this archive opens directly:

```sh
graphhelm execution status --events docs/acceptance/m05-run-2026-08-16/events --pretty
```

Expected: `executionId` `exec-m05-acceptance`, `headSequence` 12.

Opening creates the two empty directories in your working tree. They are untracked and empty, and
no committed byte changes — `SHA256SUMS` still verifies after an open, which the test below
asserts in the same run.

`blobs/` is different and is present here: a real evidence blob is a tracked FILE and survives
checkout, so a MISSING `blobs/` means evidence is genuinely gone. The store keeps refusing that
case, deliberately.

The workspace test `every_committed_acceptance_store_opens_and_replays_its_recorded_identity`
(`tools/acceptance-map/tests/committed_stores.rs`) opens and replays this archive on every run, so
the identity above cannot rust silently.
