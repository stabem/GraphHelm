# Lane Stop reconciles a merged task

Status: draft. Issue: #766. Actor: the lane agent. Entry point: its registered Stop hook.

## Preconditions

- The lane has a bound Runtime execution and an open `task.pr_opened` record with a PR number, head SHA, lane, and repository.
- GitHub can answer the PR state and merge commit. The Runtime accepts the lane actor's token.
- Proof uses a disposable local Runtime and event store. It never addresses `127.0.0.1:8793`.

## Flow and observable obligations

1. **Stop:** The lane invokes its registered Stop hook. The hook reads the bound execution's open PR records. Observation: only records for this lane and repository are candidates.
2. **Confirm merge:** For each candidate, the helper asks GitHub for the PR state and merge commit. Observation: an open or unverifiable PR produces no `task.merged` event.
3. **Record merge:** For a confirmed merged PR, the helper sends `task.merged` to the bound Runtime with the PR and merge evidence. Observation: the disposable event store contains one durable record with the correct actor, task, PR, and merge commit; the task projection no longer treats the merged slice as open.
4. **Repeat Stop:** The hook runs again against the same store and GitHub result. Observation: the event count stays at one and the helper reports no new merge.

## Failure and recovery

If GitHub or the Runtime cannot be reached or rejects the write, Stop must leave the task open rather than claim a merge. A later Stop may retry from the persisted open record. A timeout is a failed attempt, not evidence of success. The record's idempotency key prevents a retry from duplicating a committed merge.

## Proof boundary

`plugins/graphhelm/hooks/test_lane_stop_hook.py::test_reconcile_records_github_merge_once` observes the Runtime write and repeat behavior in a temporary store with a stub GitHub merge response. It does not prove live GitHub availability, a production Stop invocation after this PR merges, or a Studio board render. Those remain unobserved until their respective observers run. No browser journey is claimed.
