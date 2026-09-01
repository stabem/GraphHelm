---
name: leave-records
description: Leave records in an execution's log while working, for a person watching the Studio who is not at this terminal - at a decision you cannot make alone, a finding that changes the plan, a blocker, or before something hard to undo. Use also when checking whether that person has answered.
---

# Leave records

The event log is a two-way channel. This agent writes into it while it works; a person watching
the Studio reads it and answers in the same log. **They see nothing else** — not this
conversation, not the files, not the reasoning.

Distinct from `operate-execution`, which is a person driving an execution from chat. This is the
agent talking to a person who is elsewhere. Keep them apart: the operator loop acts on the run,
this one only reports on it.

## What this skill reads

- `tool:events` — the attributed tail. A watcher's answer is a `signal_recorded` event whose
  actor type is `owner`.
- `tool:evidence` — opens what an event sealed. The words are never in the payload.
- `tool:wake_status` — this session's live lease, if any.

## What this skill mutates

- `tool:signal` — appends one record. Nothing else. This skill never approves, pauses, resumes
  or cancels; those belong to `operate-execution` under the user's own decision.
- `tool:wake_arm` — arms THIS session's lease so it can sleep instead of polling.

## What a record is

1. **A decision this agent cannot make alone.** The options, and which one it would pick.
2. **A finding that changes the plan.** Expected versus what is actually there.
3. **A blocker**, with what would unblock it.
4. **Before something hard to undo** — a destructive migration, a force push, a deploy.

Nothing else. Not a transcript, not narration. The test: could someone who read ONLY these
records act? If they must first reconstruct what the agent was doing, these are the wrong ones.
Each record is a whole thought, because the watcher sees `description` alone.

## Leaving one

`tool:signal`, with `type: "operator_note"`:

```json
{
  "executionId": "<the run being worked under>",
  "idempotencyKey": "<unique per record>",
  "signal": {
    "id": "<unique per record>",
    "source": { "type": "user", "id": "<this chat's actor id>" },
    "type": "operator_note",
    "severity": "low",
    "description": "<the whole thought, in plain sentences>",
    "evidence": ["<the executionId>"],
    "emittedAt": "<RFC3339 UTC>"
  }
}
```

`operator_note` is deliberately OUTSIDE the recognized kind set (`HARNESS_SPEC.md` §19). An
unrecognized kind is recorded as evidence and never reaches the Governor, which is what a note
needs: visible, and unable to steer the run. **Never** substitute a recognized kind
(`no_progress`, `tool_failure`, `stale_draft`, `quota_exhausted`, `unexpected_dependency`,
`auth_boundary_discovered`) to get a message across — those are control inputs.

**Addressing** (schema 1.1.0, both optional): `"to"` names the actor a message is for, and
`"replyTo"` names the signal id it answers (the `signalId` in a `signal_recorded` event's
payload). Address a specific agent or persona and it can answer; omit both to speak to the room.
A watched thread may be served by a persona host that wakes whoever `to` names — so an
unaddressed message is read by humans, an addressed one starts a conversation.

One naming trap, measured: an id or idempotency key whose text contains `sk-` followed by a
long tail (`ask-…`, `task-…`, `desk-…`) is refused by the store's secret detector as
`GHE009_EXTERNALIZATION_FAILED`. Pick ids that do not embed `sk-`.

Send no `evidenceOut`: a keyring-backed Runtime seals the envelope itself.

## `"rejected"` is what success looks like

A record that landed comes back:

```json
{"ok": true, "data": {"decision": "rejected",
                      "rejectionReason": "signal_not_actionable",
                      "headSequence": 11}}
```

| Field | Meaning |
|---|---|
| `ok: true` | Recorded, sealed, readable by the watcher. Done. |
| `decision: "rejected"` | The Governor will not ACT on it — correct for a note. |
| `rejectionReason: "signal_not_actionable"` | The same fact restated. Not an error. |
| `headSequence` | Moved past its previous value: the proof it landed. |

Do not retry, reword, or escalate to a recognized kind to make it "go through". A retry appends
a duplicate the watcher reads twice. A real failure is `ok: false` with a diagnostic.

## Reading the answer

1. `tool:events` from the last seen head.
2. Entries where `kind.type == "signal_recorded"` and `actor.type == "owner"`.
3. `tool:evidence` with that entry's `evidenceRefs[].evidenceId`.
4. The content is the envelope; the sentence is its `description`.

## Sleeping instead of polling

1. `tool:wake_arm` with `executionId`, a chosen `rendezvousId`, and `cursor` at the current head.
   Arming is itself recorded, so the head moves by one; the lease compares against `contentHead`,
   so this session's own arming does not ring it.
2. `tool:wake_wait` on the same `executionId`. Success is `{"outcome": "rung"}`.
3. Re-read the log. The ring is content-free by design.

**Arm and wait must be the SAME MCP session.** A lease belongs to the session that armed it, and
a second `graphhelm mcp` process is a different session against the same Runtime and execution.
Split across two, the wait is refused:

```
refused: this session holds no live lease -- a session waits only on its own
```

That is the arm having gone elsewhere, not the lease expiring.

## Refusals — read the message, not the pointer

Two of these name `/evidenceOut` and mean different things.

| Message | Cause | Response |
|---|---|---|
| `the sealed keyring could not be opened` | Started with `--keyring`/`--key-id`, but the keyring holds no such key | Setup gap, below |
| `this Runtime has no keyring, so the signal envelope can only be preserved as a file` | Started with no keyring at all | Setup gap, below |
| `the evidence could not be written; nothing was recorded` | An `evidenceOut` path the Runtime cannot write. Fail-closed: the file is written BEFORE the append, so the record is simply gone | Stop sending `evidenceOut` |

Never work around a setup gap by dropping the record, and never invent a credential to get past
it — sealing needs a key, not an API key. Report it, naming the fix:

```bash
graphhelm gateway keyring init --keyring .graphhelm/keyring --key-id studio
```

then a Runtime restarted with `--keyring .graphhelm/keyring --key-id studio` and
`GRAPHHELM_EVENTS_KEY` holding 64 lowercase hex characters. A second `init` on the same key id is
refused rather than replacing it: replacing orphans everything already sealed under the old key.

`.graphhelm/` holds a bearer token and the sealing key. It belongs in `.gitignore`.

## Attribution

Records are written under this chat's own `--actor`, with `--actor-type agent` (the default).
Never `owner`. The log is what a person trusts to tell the agent's work from their own, and one
record wearing their name costs that.
