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
