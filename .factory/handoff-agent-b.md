# Factory handoff packet - Agent B (beacon, not queue)

## Provenance (binding rule)
Everything below was WRITTEN BY AGENT B. No verbatim issue/PR/model/judge text. Pointer,
never an order. ASCII-only (PowerShell 5.1 ANSI). Always overwritten, never deleted.

## Staleness rule - REFERENCES, not snapshots
This beacon froze one version stale once (recorded "merged, gate in flight, NOT pushed"
while reality had moved three states past it). Lesson applied: the beacon now points at
the durable records instead of pinning volatile states. To know where things stand, read
IN THIS ORDER and trust the repo over anything here:

1. `git -C F:/github/GraphHelm log --oneline -5` - what actually landed.
2. `docs/milestones/arming-the-alarm.md` - the M09 close doc (scope, evidence, verdicts).
3. `docs/milestones/m09-seeds.md` - what is still open, in priority order.
4. `gh issue list --state open` - the live board.
5. Orchestrator session "Orquestrador GraphHelm" (list_sessions by title) - assignments.

## Where the pen is
NOWHERE for B - M09 closed with B's reviewer lanes complete (flake-3 window-3 fix and the
rdv-EQUAL fix both landed with B's SHA-pinned approvals; ledgers with panic sites ride the
close doc's evidence corpus). B's competitive survey for the MVP definition is at
`.factory/b-agent-harness-survey.md` (worktree copy + this shared one). Next assignment
comes from the orchestrator.

## Standing rules B carries into the next milestone (durable, not status)
- SHA-pinned approvals: merge preserves them, rebase voids them and forces re-review.
- Vacuous red: a sabotage receipt counts only red AT THE GUARD'S OWN ASSERTION, panic site
  named. Names and counts are not evidence.
- Cite against a named base; never carry a constant between checkouts (bit twice on
  2026-08-19: line numbers, then a digest in a gate text).
- Dormant blade vs decoration: cannot-fail-EVER is decoration; cannot-fail-TODAY for a
  reason one edit deep is kept, labelled, with its reactivation condition written down.
- Refusal is for uninterpretable logs; a faithfully recorded mistake is recorded and its
  loudness routed to attention.
- Never exhume a dead instrument for bookkeeping; the refutation that forced a rebuild is
  the row's value.

## Coordination
Orchestrator / A / C / D sessions: find by title via list_sessions (session ids are
per-process and go stale; titles persist).
