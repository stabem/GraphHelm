> **PROVENANCE: this document became a comment on #74.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

## The fixed-rendezvous convention, cited rather than asserted

The issue body says the re-arm-on-the-same-rendezvous shape is "this factory's own daily shape
rather than a contrived one". That was a claim about our conventions. It is now a citation.

From the archived production store at
`.factory/archive/pair-events-corrupted-2026-08-16/journal.jsonl`, session `agente-a`:

    seq 26  wake_lease  session=agente-a  rendezvous=factory-a-1
    seq 30  wake_lease  session=agente-a  rendezvous=factory-a-1
    seq 33  wake_lease  session=agente-a  rendezvous=factory-a-1
    seq 39  wake_lease  session=agente-a  rendezvous=factory-a-1

Four arms, one rendezvous id, one session. Each of those re-arms is a moment where a capture
held from the previous arm would have matched on **both** fields the recorder compares, leaving
the filter nothing to distinguish them by.

**Grain, stated so this is not read as more than it is: the precondition is PRESENT, the
incident is NOT OBSERVED.** The arm and consume pairs in that history are ordered, so the window
never opened there. This raises the defect's plausibility; it does not change its status. The
red remains typed, unrun, and able to kill this issue if it comes back green.

## Two related facts from the same sweep

**No committed journal contains an rdv-equal incident.** Every git-tracked journal carrying wake
events was checked. Only one has a same-session re-arm —
`docs/acceptance/m08-rejudge8-2026-08-18/journal.jsonl`, session `a42799d1f049fc41`, arming
`judge8-glance-1` then `judge8-glance-2` — and those rendezvous ids **differ**, with the arm and
consume strictly interleaved. Not this shape.

So a fold that refuses on arming identity would reject nothing that replays today. The two
corrupted archives *would* be rejected, but both already fail replay on a consume-without-lease
(`seq 39 arm → seq 42 consume → seq 43 consume` in the first), so a new refusal is a no-op for
them.

**The protocol gap is visible in the same data.** Every `wake_lease_consumed` payload in those
journals carries `executionId`, `sessionId` and `reason` — and no rendezvous, and no arming
identity. That is why no replay can detect this class today, and it is what the second half of
the fix is for.
