# Factory handoff packet - Agent B (beacon, not queue)

## Provenance (binding rule, from A's review)
Everything below was WRITTEN BY AGENT B. This packet is the only channel where text enters
a session's context without the agent choosing to open it, so it carries no verbatim issue
bodies, no PR text, no model output, no judge verdicts - quoting outside text here would
smuggle instruction authority past the agent's own judgement. POINTER, never an order.

ASCII-only on purpose: PowerShell 5.1 reads this as ANSI when the SessionStart hook prints
it, and accented characters arrive as mojibake.

Beacon, not one-use: always overwritten, never deleted. Deleting on consumption would risk
losing the state if the session died between reading and acting.

## Staleness anchors - CHECK THESE, do not judge by elapsed hours
If any disagrees with the repo, this packet is stale and the repo wins:

| Anchor | Value when written | How to check |
|---|---|---|
| main HEAD | 9aa4075 (M07 merged, #61) | `git -C F:/github/GraphHelm log --oneline -1` |
| issue #60 | CLOSED | `gh issue view 60 --json state` |
| pair serve binary | built from main 9aa4075 | `.factory/bin/graphhelm.exe` mtime vs merge time |
| pair-store head | 68 | `GET /v1/executions/pair-loop/events?after=0` on 127.0.0.1:41999 |

Rule of thumb: main HEAD moved past 9aa4075 => a newer milestone landed; re-read the repo
before trusting anything below.

## Where the pen is
NOWHERE - M07 is merged and closed. The pair is idle, waiting for the owner to open M08.

## State after the M07 merge (all done by me, verified)
- main rebuilt; attention 17/17, wake_http 9/9, monitor_http 4/4 green on merged main.
- Pair serve rotated to the merged binary (serve v5) - the doorbell now runs the code the
  milestone shipped. Verified live: my own alarm answers `lastConsumed {reason: rung,
  atSequence: 67}` and reports `head` and `contentHead` separately. Both are M07 features
  running in production for the first time.

## M08 candidates (the judge's own seeds, in his priority order)
1. Liveness/time in the glance - no startedAt, lastEventAt or heartbeat, so "healthy and
   working" and "stopped emitting" still render identically. He raised it in all three
   runs; it is the biggest.
2. Retry flapping invisible in the glance (a re-queued failure needs no operator, so it
   raises no attention, but repeated flapping accumulates no signal either).
3. No blocking wait on the MCP surface (wake_arm + wake_status only; wake-wait is a CLI
   sidecar), so an MCP client can only poll - the opposite of what the doorbell is for.
4. wake_last_consumed has no ceiling; MCP sessionId is a per-process nonce, so receipts
   accumulate in a long-lived execution.
5. Anchoring "no surface may recompute the attention verdict" needs a PRD section 8 clause
   - an owner decision about what the product promises, not a test edit.
6. Serve concurrency: REFUTED 2026-08-17, do not act on the old text here. Measurement:
   with a drive parked in a 90s model call, /health answered 0.00s, GET status 0.03s
   reporting the run live, and POST wake-lease was ACCEPTED in 0.08s. Arming during a
   running execution works. F4 stayed test-proven and WHY the M07 live ring failed is
   NOT KNOWN. See docs/milestones/one-glance.md honest limit 2.

## Coordination state
- Pair serve: 127.0.0.1:41999, binary from .factory/bin/graphhelm.exe (NEVER target/debug:
  the exe lock killed an 18-stage gate once).
- My rendezvous: factory-b (FIXED). Cycle: issue -> send_message -> ring the peer's ->
  RE-ARM my own -> sleep. After every EXIT=0 the sleeper is dead: restarting it is part of
  waking up. My own ring burns my own lease if I am armed - ring while unarmed, then arm.
- Agent A's session for send_message: local_609750f6-0fde-49dd-bdf4-a5351abc0aa9
- My own sessionId: UNKNOWN - list_sessions excludes the current session, so only A can
  read it and hand it over.
