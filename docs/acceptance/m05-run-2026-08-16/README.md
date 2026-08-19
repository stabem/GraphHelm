# M05 acceptance run — a byte-archive of an event store

This directory holds the evidence of one real run, byte for byte, checksummed in `SHA256SUMS`.

`events/` is a **byte-archive of a store, not a directly openable store**. A live local
repository also has the directories `.tmp/` and `active/`, and they were empty when this run was
archived. Git tracks files, not empty directories, so they are absent here — and the store
refuses a repository whose `format.json` is present but whose layout is incomplete
(`core/events/src/local.rs:2078-2082`), reporting `GHE005_INTEGRITY_FAILURE`. Nothing is missing
from the history: every event, hash and evidence blob is committed and verified.

To open it, restore the empty shape first — into a COPY, so the archived bytes stay the record:

```sh
cp -r docs/acceptance/m05-run-2026-08-16 /tmp/m05
mkdir -p /tmp/m05/events/.tmp /tmp/m05/events/active /tmp/m05/events/blobs
graphhelm execution status --events /tmp/m05/events --pretty
```

Expected: `executionId` `exec-m05-acceptance`, `headSequence` 12.

The workspace test `every_committed_acceptance_store_opens_and_replays_its_recorded_identity`
(`tools/acceptance-map/tests/committed_stores.rs`) does exactly this on every run, so this
instruction cannot rust silently. Restoring empty directories adds no content and can hide no
loss: a real evidence blob is a tracked FILE and survives checkout.
