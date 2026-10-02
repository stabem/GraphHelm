# Node deliveries and project document edits

A lifecycle outcome is not a description of the work. Agents can record deliveries progressively, without waiting for a node to finish. Reports are sealed evidence attached to their source node; the Studio displays what changed, why, and the reported document/journey/business-rule relationships.

## Record a delivery

After a meaningful change, write a UTF-8 JSON report:

```json
{
  "version": 1,
  "summary": "Defined payment retry behavior",
  "reason": "The purchase journey did not explain recovery from a failed payment.",
  "documents": [
    {
      "path": "docs/prd/checkout.md",
      "title": "Payment retry rule",
      "kind": "business_rule",
      "action": "updated",
      "journeyIds": ["checkout"],
      "ruleIds": ["PAY-02"]
    }
  ]
}
```

```powershell
graphhelm execution delivery --events <events-directory> --execution <run-id> --node <node-id> --project-directory <main-project-directory> --delivery <report.json> --actor-id <agent-id> --keyring <keyring-directory> --key-id <key-id>
```

The sealing key remains in `GRAPHHELM_EVENTS_KEY`, never in the report or command arguments. `--actor-id` attributes the report to that agent; omitting it records a CLI owner action. The command derives the opaque `projectId` from the canonical directory. Do not reuse a report's project identity to claim another checkout.

Reports are **reported provenance**: recording one does not verify that a file changed, certify a business rule, or change the node's lifecycle state. Accepted kinds are `file` and `business_rule`; actions are `created`, `updated`, and `reviewed`. Reports are limited to 16 KiB and 32 documents. Supply explicit journey/rule references when applicable, rather than inferring them from filenames. Generic `execution signal` can carry the same reserved `node_delivery` envelope, but it must satisfy the same typed validation and sealing requirements.

## Follow the mission to its proof

Studio keeps the mission objective visible above the people roster. **Open assigned steps**
shows an actor's explicitly assigned nodes in the operational graph. Other nodes and their
verified dependencies remain available; an actor's messages or session hierarchy never create
an assignment or dependency. **Open direct chat** remains a separate action. With no assigned
node, the view says so instead of constructing a workflow from prose.

Opening a node shows its reported outputs and their source records. A delivery may include
an optional `work` object to explain the current reported stage, skill use and checks. For example,
append this object to the version 1 delivery above, using the actual revision and resource digests:

```json
{
  "work": {
    "version": 1,
    "sessionId": "session-writer-1",
    "stage": "checking persistence",
    "revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "skills": [
      {
        "id": "keel",
        "version": "1.3.0",
        "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "status": "reported"
      }
    ],
    "checks": [
      {
        "id": "persist-customer",
        "command": "cargo test customer_persistence",
        "observer": "local test process",
        "outcome": "unobserved",
        "attemptId": "attempt-1"
      }
    ]
  }
}
```

These values illustrate the shape; they are not an actual check or installed-skill receipt.
`stage` is a bounded description, not a fixed workflow category. The report accepts at most
16 skills and 32 checks within the existing 16 KiB total limit. Skill status is `requested` or
`reported`; neither establishes host loading, invocation observation or evaluation. The reported
session ID is distinct from the event's recorded actor and may differ from it.

Each check names a command, observer, outcome and attempt. Outcomes are `passed`, `failed`,
`skipped` or `unobserved`. A passed or failed report also requires an `evidence` binding with
`evidenceId`, `contentHash` (`sha256:` plus 64 lowercase hexadecimal characters) and nonnegative
`size`. A later attempt can name `previousAttemptId`; earlier reports stay in the append-only
history. An evidence reference is a reported binding, not proof that its bytes were independently
opened or that its observer is trusted. Skipped and unobserved checks do not count as passes.
All checks in one work report concern its declared Git `revision` (40 or 64 lowercase hexadecimal
characters). A report of a different revision remains a separate historical record.

### JPD candidate results

`work.journeyVerification`, when supplied, is the complete artifact defined by the existing
[journey verification result schema](../../extensions/builtin/graphhelm-jpd/schemas/journey-verification-result.schema.json).
The Runtime validates its shape offline, its code revision against the work report, and its graph
binding against the execution's graph when recording the signal. Older deliveries without work
or JPD data remain valid and display the absence explicitly.

A graph binding requires an active published Graph Version in the execution projection. An
ordinary `execution start` declares the graph shape; that declaration alone is not a sealed
publication. Such a run can record ordinary work but refuses a graph-bound JPD artifact until
the existing governed publication path establishes the active version.

Studio presents the candidate's proposed result, obligations, missing observers, bindings and
retry history. It keeps `proven`, `accepted_with_waiver` and `unresolved` as proposed artifact
statuses; it does not mint an authoritative certificate. Schema validity does not authenticate
evaluator receipts, recompute evidence digests or establish freshness. A generic JPD journey
still needs its registered deterministic validator and adequate observers. A passed node gate,
reported check, host task completion, or a skill's text cannot supply those missing capabilities.

Reported output, observed test results, JPD acceptance and integrated delivery are separate facts.
The panel does not infer review or merge from this report. Review and main-branch integration
remain unobserved until an independent external receipt establishes them.

### Responsibility boundaries

| Piece | Responsibility |
|---|---|
| Agent or host adapter | Produce the scoped report, skill provenance, attempt identities and evidence bindings; state what is unobserved. |
| CLI and Runtime signal admission | Bound and validate the sealed record and its node/graph/revision bindings; preserve attributed events without changing acceptance. |
| Event Store | Preserve every report and earlier failure in order; replay does not rewrite history. |
| Governor and deterministic validators | Decide authorized operational transitions and acceptance through their existing contracts. |
| Studio | Navigate the mission, explicit assignments, outputs and candidate proof; show source, age, missing data and historical attempts faithfully. |
| Independent reviewer | Run the reached checks on the reviewed head and integrate that head under the repository delivery process. |

Installing a companion package only makes its skill resources available to the host. It does not
populate this report automatically. A real producer must record the facts it can observe through
the existing delivery command or sealed signal API; absent instrumentation remains visible.

## Read and edit from Studio

Select the node, then a blue document link under **Deliveries**. The adjacent editor reads the **main project directory**, not an ephemeral agent workspace or the run's Git output ref. A report can describe a file that is not present in that directory; opening it then refuses rather than substituting a different version.

The Runtime must have an explicit `--project` matching the delivery and a sealed keyring. Documents are addressed through the run's evidence ID and document index. The client cannot supply an arbitrary filesystem root or path. Existing text is limited to 128 KiB; unsafe paths, links, protected directories, binary content and recognized secret content are refused.

Saving requires a reason and the revision returned by the read. A stale revision preserves the draft. CLI equivalents are `execution document-read` and `execution document-save`, both using `--project-directory` for the filesystem location without changing execution stream scope; the latter accepts an `--edit` JSON file containing `document: { evidenceId, index }`, `content`, `expectedSha256`, `reason`, and `idempotencyKey`.

MCP exposes `document_read` with `executionId` and `document: { evidenceId, index }`, and `document_save` with those fields plus `content`, `expectedSha256`, and `reason`. Saving preserves the MCP session's actor identity and requires an owner-configured session; an agent session is not elevated. The MCP adapter derives the identical body/header idempotency key from the RPC ID, so retry the same RPC ID within that session for the same edit. These file commands accept neither `ifMatch` nor an explicit idempotency key; file revisions govern concurrency. Runtime refusals and notification receipts pass through unchanged.

Public HTTP equivalents are authenticated `POST /v1/executions/{id}/documents/read` and `/documents/save`. Read takes `{ evidenceId, index }`. Save takes the edit JSON and the ordinary owner/idempotency headers. Document concurrency uses `expectedSha256`; these routes do not accept stream `If-Match` as a substitute.

## Change notices and recovery

Saving records a sealed intent, publishes the file, records a saved receipt, then appends an owner-change notice to runs associated with the project through their delivery records. Runs without a recorded project association are not guessed. The association census refuses above 256 streams or 128 delivery evidence references per stream before changing the file.

The editor distinguishes a saved file from pending notices. **Retry run notices** uses the same request and identity. Once a saved receipt exists, retrying can finish historical notices even if the file subsequently changed or disappeared; it never restores the earlier bytes. If publication occurred but no saved receipt could be recorded and the file subsequently changed, the system cannot prove that earlier publication and refuses to overwrite the newer file.

Ordinary cognitive nodes on the real Runtime executor receive recent notices in their actual, sealed task input before their next model call. Notice input keeps the newest records within 64 KiB and reports omitted older records. Existing model calls are not interrupted; blind judges, tools and gates keep their original input. External agents can observe the run's event/wake stream and read the sealed notice. A recorded notice is not proof of acknowledgment or replanning. Saving never resumes a paused/completed run or publishes an operational graph mutation.

Runtime writers use a project lock and atomic replacement. An expected hash detects changes observed before publication, but an unrelated editor that ignores this lock can still race the final comparison and replacement. Windows owner/group/DACL and protection are copied before temporary plaintext is written; privileged SACL/audit metadata is not part of that guarantee.
