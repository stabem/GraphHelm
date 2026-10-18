# Keel watchdog routing fixture (#1045)

This draft journey replays the five user steps of governed routing through the real CLI:

1. `execution start` a held task on `examples/graphs/software-feature.yaml`.
2. `keel check --route-proposal` on a diff with a path outside the card: exit 2, finding
   `keel.scope.path_outside_card`, a `keel.blocked` route proposal.
3. `execution status` shows the implement draft pending; after its exact approval the
   `route-correction` node is ready and `route-review` is absent.
4. The corrected head is checked with a card that covers it: exit 0, `keel.passed`. Its drafts
   bind the published version by the safe hash `graph replay` shows (`expectedSafeHash`).
5. After the review draft's approval, `route-review` is ready.

Everything runs on a temporary local event store and keyring under this fixture, removed on exit.
The keyring key is a fixture constant; the shared Runtime at 127.0.0.1:8793 is never touched and
`GRAPHHELM_EVENTS_KEY` from the caller is not passed through. Child processes are hidden on
Windows and bounded by deadlines.

Set `GRAPHHELM_CLI` to a compatible CLI, or place `graphhelm` on `PATH`, then run from the
repository root:

```powershell
python .graphhelm/fixtures/keel-watchdog-routing/scenario.py
```

The journey stays draft until the owner approves it; the direct scenario is its observer until then.
