> **PROVENANCE: this document became a comment on the PR that closed #83.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

## Measured pre-fix behaviour: the retry is answered with a fabricated success

Recording this against the issue because it upgrades the operator story and, until now, it existed
only inside a sabotage dump. It is **measured, not argued** — observed by running the guards against
the pre-fix ordering (the sabotage that restores commit-before-setup IS the shipped behaviour this
issue reports).

The issue already describes two response classes an operator can meet:

1. `GHCLI016_DRIVER_FAILURE` — "the call failed", while the store has recorded the resume anyway;
2. `GHCLI005_EXECUTION_STATE: resume refused: not_paused` — the retry, refused for a hold the first
   call silently consumed.

**There is a third, and it is the worst of them.** A client that meets (1) retries the identical
request — same `Idempotency-Key`, same body — because that is precisely what an idempotency key
exists to permit. Pre-fix, the refused call had already committed, and the committed event carries
that key. The retry therefore never executes: the idempotency layer replays the committed half-state
as a **success the first call never reported**.

Raw reply to the same-key retry, pre-fix:

```json
{"command":"execution.resume",
 "data":{"executionId":"exec-83-same-key","status":"running","headSequence":21,
         "attention":"needs_you","attentionReasons":[{"kind":"blocked_node","node":"deploy"}],
         "acceptedMutations":0,"mode":"supervised", ...},
 "diagnostics":[],
 "ok":true}
```

`ok: true`. `status: "running"`. No diagnostics. HTTP 200.

So the operator is told the call **failed**, retries exactly as the protocol invites them to, and is
told it **worked** — about a resume that never ran, on an execution whose hold was consumed by the
call that reported failure. Three answers to one request, all from the same defect:

| attempt | answer | what the operator concludes |
|---|---|---|
| first call | 500 `GHCLI016` | it failed, my hold stands |
| retry, **different** key | 409 `GHCLI005` not_paused | my hold never existed |
| retry, **same** key | **200 `ok:true`, running** | it worked after all |

The third is the most dangerous because it is the only one that reads as *fine*. A failure and a
refusal both prompt a human to look; a fabricated success ends the investigation. And the same-key
retry is the path a well-behaved client takes by default, so this is the branch an automated caller
is most likely to land on.

**Guarded.** `the_same_idempotency_key_after_a_failed_setup_still_executes` asserts the same key
executes fresh and commits nothing. It is a distinct blade from the different-key retry guard: that
one asks whether the hold survived, this one asks whether the key was spent. Both fall under the
pre-fix ordering, at their own assertions.
