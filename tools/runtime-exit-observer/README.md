# Runtime exit observer

This manual tool observes one Windows process that is already running. It does not start,
stop, restart, or signal the process. It never contacts the GraphHelm Runtime or reads its
event store.

```powershell
python runtime_exit_observe.py --pid 1234 --creation-filetime 133700000000000000 `
  --timeout-seconds 30 --output C:/private/receipts/process-1234.jsonl
```

Obtain the expected process creation FILETIME from an independent trusted source. Correlate
PID and creation time with the Runtime instance and executable separately; this tool does not
verify an instance label. The output must be a new private path. It is opened with create-new
semantics and is never overwritten. Each fixed-schema JSONL record is flushed and synced.

An `attached` record means the requested PID and creation time matched on the retained native
process handle. `exited` contains the unsigned OS exit code, including 259 when the handle has
signaled. `timeout` means the handle did not signal within the monotonic wait budget. Attach
failure, identity mismatch, unsupported hosts, and invalid input produce fixed diagnostics and
do not claim an exit cause. If the observer or host stops, a receipt with only `attached` is
incomplete. Filesystem and OS calls are not covered by a hard wall-clock guarantee.
If flushing or syncing a terminal record fails, or closing the receipt fails after one, the observer
attempts one append of a `receipt_unavailable` record when the output remains writable.
Readers must treat that final marker as a failed observation, even if an earlier `exited`
record is present. If appending or syncing the marker also fails, the process exit code
and fixed diagnostic are required to classify the observation; the JSONL file alone may
be incomplete.

The observation budget starts just before the `attached` record is written. Its remaining
time is checked before the first process wait and after later waits. If attachment syncing
uses the budget, the observer records `timeout` without starting a wait. This bounds when
a wait may begin; it does not bound filesystem calls, OS calls, or total command wall time.

The observer requests only `PROCESS_QUERY_LIMITED_INFORMATION` and `SYNCHRONIZE`; it does not
request process termination rights. The process handle is closed on every attached path.
