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
- agent reuse rate.

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

### 11.3 Resume

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
- backup failed.

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
- project export/import.

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
- metrics are local by default.
