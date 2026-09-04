# Observability, operations, and recovery

## 1. Objective

Give the user complete visibility into system behavior and enable pause, resume, replay, diagnosis, and recovery without repeating valid work or hiding failures.

## 2. Observability pillars

### 2.1 Events

Append-only operational source. Every relevant transition generates a typed event.

### 2.2 Metrics

Local series and aggregations of performance, cost, quality, capacity, and security.

### 2.3 Logs

Structured logs for diagnosis, redacted by sensitivity.

### 2.4 Traces

End-to-end trace: prompt → harness → graph → nodes → model/tool → artifacts → docs.

### 2.5 Artifacts

Persistent outputs that allow inspection and reproduction.

## 3. Correlation

Required IDs:

- workspace_id;
- project_id;
- execution_id;
- graph_version;
- node_id;
- attempt_id;
- agent_runtime_id;
- model_call_id;
- tool_call_id;
- trace_id;
- artifact_id.

## 4. Harness metrics

- time_to_profile;
- time_to_first_graph;
- graph_nodes_initial;
- graph_nodes_final;
- mutation_count;
- unnecessary_node_estimate;
- policy_gates_added;
- graph_lint_errors;
- graph_simulation_failures;
- cost/time estimate error;
- capability gaps;
- agent reuse rate;
- telemetry overhead (storage bytes, compute seconds, measurement tokens per subsystem), with aggregation joins off the execution critical path.

## 5. Execution metrics

- queue time;
- runtime duration;
- node duration;
- concurrency;
- retry count;
- pause/wait time;
- success/failure by category;
- invalidated outputs;
- checkpoint duration;
- resume success;
- sandbox cold start;
- artifact throughput.

## 6. Model metrics

- route availability;
- latency;
- input/output tokens when available;
- monetary cost when available;
- subscription throttles;
- waiting capacity time;
- schema compliance;
- tool success;
- cancellation latency;
- model switch count;
- quality by task class.

## 7. Context metrics

- tokens allocated/consumed;
- retrieval count;
- cache hit;
- expansion requests;
- expansion accepted/denied;
- context redundancy;
- stale item rate;
- relevant evidence coverage;
- tokens saved vs full-context baseline;
- contradiction count presented.

## 8. Quality metrics

- gate pass/fail;
- finding severity;
- reviewer disagreement;
- false positives confirmed later;
- regressions after completion;
- evidence coverage;
- unsupported claim count;
- user acceptance/rework;
- manual override rate;
- completed_with_waivers rate.

## 9. Dreams metrics

- cycles;
- duration;
- proposals;
- commit/discard;
- rollback;
- docs consolidated;
- claims updated;
- memories expired;
- agents merged;
- context savings;
- generated task precision.

## 10. Initial SLOs

Reference SLOs, to be calibrated per hardware/provider:

- Runtime API local/VPN availability: 99.5% monthly;
- event propagation to Studio: p95 < 500 ms on a healthy network;
- graph state write: p95 < 250 ms, excluding model/tool;
- durable checkpoint metadata: p95 < 2 s;
- resume of a checkpointable node: success > 99%;
- no secret in persisted logs: 100% expected, treated as an incident;
- graph mutation atomicity: 100%;
- event idempotency under retry: 100%;
- Studio canvas interaction: 60 fps target with 1,000 nodes on reference hardware;
- Runtime recovery after reboot: < 2 min to rebuild the scheduler, not counting container/model cold start.

## 11. Checkpoints

### 11.1 Types

- node pre-call;
- model turn;
- tool call boundary;
- artifact produced;
- sandbox filesystem snapshot;
- graph mutation safe point;
- external effect receipt;
- human decision.

### 11.2 Content

- graph version;
- node state;
- attempt;
- input refs;
- context capsule ref;
- artifacts;
- tool/model session refs;
- leases;
- sandbox snapshot ref;
- pending timers;
- compensation state.

Secrets do not enter the checkpoint; only references.

### 11.3 Integrity checkpoints

The Event/Evidence Store keeps a second, cryptographic kind of checkpoint. It anchors a stream prefix with an authentication tag produced by the `KeyProvider`, and its canonical bytes include the key version and the provider revocation epoch. Reads bind the physical tail to the provider-authenticated stream head and, when a checkpoint is present, bind the bounded suffix to it. A database-only attacker cannot rehash a suffix and replace the head without a fresh provider tag.

Verify a range with the operator CLI:

```bash
graphhelm events verify --config OPERATOR_CONFIG \
  --workspace WORKSPACE --project PROJECT --stream STREAM \
  --start 1 --max-events 100000
```

`--repository PATH` verifies only that a local repository declares the supported format. Range verification is a PostgreSQL capability and is refused against a local repository rather than silently skipped. A verification failure is `GHE005_INTEGRITY_FAILURE` and is an incident: stop writes to the affected stream and restore from an authenticated backup.

### 11.4 Resume

Before resuming:

- validate graph version;
- validate dependencies;
- validate source snapshot;
- renew leases;
- revalidate route health;
- reopen/recreate sandbox;
- invalidate unsafe session;
- emit event.

## 12. Pause semantics

### Graceful pause

Waits for the current call to finish, persists output, and stops before the next step.

### Immediate stop

Cancels model/tool, kills the process/sandbox if necessary, and returns to the last safe checkpoint.

### Branch pause

Pauses only affected descendants. Independent branches continue.

### Global pause

Prevents new nodes; running nodes obey the chosen policy.

## 13. Cancel semantics

Cancel does not erase history. External effects already performed require compensation. The final status records partial effects.

## 14. Retry

Categories:

- transient provider;
- rate limit;
- tool timeout;
- malformed output;
- sandbox crash;
- deterministic failure;
- policy denied;
- invalid input.

Only configured categories retry. Exhausted subscription quota enters wait, not aggressive retry.

## 15. No-progress detection

Detect:

- semantically identical outputs;
- retries with no change in context/strategy;
- recurring remediation loop;
- graph mutation alternating states;
- agent delegation chain;
- repeated tool failure;
- budget consumption without evidence gain.

Actions:

- stop branch;
- change strategy;
- add critic;
- request human decision;
- mark blocked;
- preserve diagnostics.

## 16. Failure categories

- user_input;
- context_missing;
- schema_invalid;
- model_auth;
- model_quota;
- model_provider;
- model_output;
- tool_permission;
- tool_runtime;
- sandbox;
- policy;
- graph;
- storage;
- network;
- external_effect;
- internal_bug;
- cancelled.

Each failure includes retriable, severity, evidence, and recommended actions.

## 17. Recovery scenarios

### 17.1 Studio closes

No effect on the Runtime. On reopening, Studio fetches the snapshot and streams from the last sequence.

### 17.2 Runtime restarts

- recovery lock;
- load nonterminal executions;
- check expired leases;
- mark running nodes as recovering;
- reconcile sandboxes;
- resume or roll back to the checkpoint;
- emit recovery report.

### 17.3 Worker dies

Worker lease expires. The scheduler reassigns the attempt according to idempotency and effect state.

### 17.4 Database unavailable

Stop new mutations/side effects; nodes may finish the current call, but cannot confirm success without a durable event. Limited local buffering does not substitute for persistence for critical effects.

### 17.5 Artifact store unavailable

The node does not complete a large output until persistence occurs. Small payloads may remain pending within a safe limit.

### 17.6 Model quota

`waiting_for_model_capacity`; no automatic fallback.

### 17.7 Sandbox cleanup fails

Quarantine; no reuse; alert; separate privileged cleanup job.

### 17.8 Graph draft stale

Studio receives the diff, rebases, and needs to reconfirm operational changes.

### 17.9 Event/Evidence Store corrupted

Integrity verification, a migration-ledger mismatch, or a rejected authenticated head means the repository is no longer trustworthy. Stop writes, keep the damaged database for forensics, and restore into a fresh distinct target following 19.4. A local repository crash that left unreachable Evidence blobs is not corruption: reopening the repository deletes orphan blobs and staging files and republishes the active marker from replayed events.

### 17.10 Key provider unavailable

`GHK001_KEY_UNAVAILABLE` means the sealed keyring, the root key material, or the revocation journal cannot be authenticated. Canonical replay continues because it never requires plaintext; Evidence reads and executable materialization fail closed. Restore the keyring from its own separately encrypted backup. An obsolete keyring whose monotonic revocation state was not preserved can resurrect content that was cryptographically erased, so keyring recovery is a compliance-relevant action and must be recorded.

## 18. Replay

Replay reconstructs:

- Graph Version over time;
- node states;
- model/tool call metadata;
- artifacts;
- user interventions;
- waivers;
- knowledge/doc updates.

Modes:

- visual timeline;
- deterministic simulation;
- re-execution with the same refs;
- re-execution with substituted models;
- branch-only replay;
- failure reproduction.

Re-execution creates a new execution and does not alter the original.

Replay never requires evidence plaintext. Erased or unavailable evidence replays as a typed unavailability; only executable materialization fails, and only when the missing content slot is required.

### 18.1 Projection rebuild

Projections are disposable and are always rebuildable from the canonical journal. A rebuild creates a fresh generation, consumes bounded verified pages, transactionally advances a hash-bound watermark, rechecks the source head, and activates only a complete generation. A failed rebuild or a failed swap leaves the previous active generation unchanged, so a rebuild is safe to run against a live stream.

```bash
graphhelm events rebuild --config OPERATOR_CONFIG \
  --workspace WORKSPACE --project PROJECT --stream STREAM \
  --generation N --page-size 1000
```

The generation must be new. `GHE005_INTEGRITY_FAILURE` from a rebuild means the stored cursor, hash, or projection version cannot safely resume; rebuild into the next generation instead of repairing the old one. Rebuild is a PostgreSQL operation; the local repository stores no projection generations.

## 19. Export and backup

### 19.1 Export

- project metadata;
- graphs;
- agents/skills;
- policies;
- docs;
- claims/evidence;
- artifacts optional;
- events optional;
- execution manifests;
- no secrets.

### 19.2 Backup

- encrypted database dump;
- artifact snapshot;
- documents/repositories;
- vault backup separately encrypted;
- config and certificates;
- restore test.

### 19.3 Disaster recovery

The runbook defines RPO/RTO according to deployment. Single-node default: daily full backup plus frequent incremental events/artifacts. Enterprise production may use streaming replicas/object versioning.

### 19.4 Local Event Store backup and restore

Stop writers or otherwise hold the repository stable, then write a new archive path:

```bash
graphhelm events backup --repository LOCAL_REPOSITORY --output LOCAL_ARCHIVE
```

The local archive contains the journal and regular-file blobs. Backup refuses non-regular blob entries. The archive does not contain repository layout files, Runtime configuration, or bearer tokens. `--output` must not exist. Copy configuration and other deployment state separately.

Restore into a path that does not exist or is empty:

```bash
graphhelm events restore --repository EMPTY_REPOSITORY --archive LOCAL_ARCHIVE
```

The restore command accepts local archive version `1.0.0`, creates the repository layout through the local adapter, and then writes the journal and blobs. It refuses to merge into a non-empty repository.

**A FAILED local restore leaves a partial repository at the target, and that target cannot be retried into.** The layout is created and `journal.jsonl` is replaced before the blob loop runs, so an archive with a valid version and journal but an invalid blob name or value -- or a blob write that fails -- returns an error with the destination already carrying a supported, non-empty repository. The next attempt is then refused by the same non-empty check that protects a real repository, and nothing distinguishes the two states from outside.

So: discard a failed target and restore into a FRESH path. Do not delete files inside it to make it look empty, and do not restart writers against it -- it holds a journal with no blobs behind it, which reads as a repository and is not one.

The local CLI does not currently provide a post-restore integrity observer. `graphhelm events verify --repository EMPTY_REPOSITORY` recognizes a complete local repository layout, but it always reports `"verified": false`; it does not verify the journal hash chain, blob references or digests, or application-level replay. Those properties therefore remain unverified after a local restore, and operators must not treat that command as a readiness gate before restarting writers.

### 19.5 PostgreSQL Event/Evidence Store backup and restore

Both commands read one bounded JSON operator configuration from `--config` or `GRAPHHELM_EVENTS_CONFIG`. It must be a regular file, not a symbolic link, and not group- or world-accessible on Unix. It declares the administrative DSN, an absolute passfile, the sealed keyring directory and key ID, absolute `pg_dump` and `pg_restore` paths with pinned SHA-256 digests and versions, and a process timeout. The 32-byte root key is never in that file; it is supplied as 64 lowercase hexadecimal characters in `GRAPHHELM_EVENTS_KEY`, so a leaked configuration alone cannot unwrap Evidence.

Back up PostgreSQL:

```bash
graphhelm events backup --config OPERATOR_CONFIG --output ARCHIVE
```

The dump is streamed through ordered 1 MiB authenticated-encryption chunks with an authenticated manifest binding source identity, pinned tool versions and digests, the migration, schema, and privilege contracts, provider metadata, counts, and totals. Publication is atomic and no-replace: an existing `--output` path is never overwritten, so archives are written under new names and rotated by the operator.

Restore PostgreSQL:

```bash
graphhelm events restore --config OPERATOR_CONFIG --archive ARCHIVE
```

Restore accepts only a fresh, distinct target and never modifies the source. It authenticates the whole archive before trusting any metadata, rejects an observable provider-epoch rollback, and after loading verifies the migration, schema, RLS, policy, trigger, function, and grant contracts, every bounded event chain and checkpoint, references and Evidence state, retention authorities, holds, receipts, tombstones and cleanup, and an independent projection rebuild. Only then does it return a verification receipt. Select the restored database only after that receipt.

A failed restore leaves the partial target disabled behind an authenticated marker and requires manual recovery; nothing is renamed or dropped automatically by database name. Back up the sealed keyring separately and at least as often as the database: an archive whose key material is lost is unreadable, and an archive restored against an obsolete keyring is rejected.

Restore tests are mandatory and non-destructive by construction, because restore always targets a new database. Run one on the current archive on the cadence the deployment runbook sets.

## 20. Retention

Configurable by data class:

- raw prompts;
- model outputs;
- tool logs;
- artifacts;
- traces;
- metrics;
- events;
- docs;
- claims;
- secrets access audit.

Event Store "immutable" means not rewriting within the retention period. Legal/owner expiration may create a tombstone/cryptographic erasure per compliance design, preserving minimal metadata and a deletion audit.

### 20.1 Erasure and legal-hold audit

Erasure is a prepared, revoked, and finalized saga, and every stage is auditable from the journal alone. The audit trail for one operation is the requested event carrying scope, operation, Evidence and key-handle identity, policy identity and version, authority, reason code and prior state; the completed event adding the ciphertext digest, provider receipt, and provider epoch; the minimal tombstone; and the physical cleanup receipt. Event and Evidence history is never rewritten, so an erased item remains visible as erased.

Legal holds are authenticated append-only place and release records, and every eligibility decision reconstructs and verifies hold history before deciding. A hold committed before prepare blocks erasure with `GHEV002_LEGAL_HOLD`; a hold placed afterwards cannot resurrect pending or erased content. `GHEV003_RETENTION_INELIGIBLE` means policy, age, scope, or state does not permit the action, not that the request failed.

Audit checks an operator can perform:

- every finalized operation has a matching tombstone and completion event;
- every cleanup receipt maps bijectively to its `EvidenceCiphertextDeleted` event and reproduces its canonical request digest;
- the provider revocation epoch never regresses between backups;
- no Evidence sits in `erasure_pending` past the reconciliation window;
- every active hold has an authenticated place record and no unmatched release.

Restore reruns all of these before returning its receipt, so a restored database that passes verification has an intact erasure audit trail.

## 21. Alerts

- runtime offline;
- auth required;
- quota exhausted;
- execution blocked;
- production effect failed;
- secret scan finding;
- sandbox quarantined;
- storage pressure;
- graph no-progress;
- Dreams regression/rollback;
- stale critical doc;
- plugin vulnerability;
- backup failed;
- restore verification failed;
- event chain or checkpoint verification failed;
- key provider unavailable;
- erasure operation pending past reconciliation;
- projection rebuild failed.

Channels are plugins; local notifications by default.

## 22. Health dashboard

- API;
- database;
- artifact store;
- worker pool;
- sandbox host;
- model routes;
- credential broker;
- scheduler;
- Dreams;
- disk/memory/CPU;
- pending migrations;
- quarantines.

## 23. OpenTelemetry

The reference implementation uses OpenTelemetry semantics and local export. An external collector is optional. Sensitive attributes are filtered before the exporter.

## 24. Mandatory runbooks

- install failure;
- update rollback;
- runtime recovery;
- database restore;
- vault recovery;
- model reconnect;
- sandbox quarantine;
- secret leakage;
- compromised plugin;
- graph stuck;
- production deploy failure;
- Dreams rollback;
- project export/import;
- event chain integrity failure;
- projection rebuild;
- evidence erasure and legal-hold audit.

## 25. Acceptance criteria

- restart does not lose completed outputs;
- duplicate event does not duplicate the effect;
- branch pause preserves other branches;
- immediate stop returns to a known checkpoint;
- replay shows correct Graph Versions;
- quota wait does not consume BYOK;
- secret does not appear in logs/export;
- quarantine prevents reuse;
- backup/restore is testable;
- metrics are local by default;
- replay succeeds without evidence plaintext;
- erased required content blocks execution instead of degrading it;
- a projection can be rebuilt from events without touching canonical history;
- restore verifies the whole database before the target is selectable.
