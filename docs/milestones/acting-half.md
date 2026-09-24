# The acting half

Status: M11 acting half SHIPPED 2026-09-11 (#159) — a parked node can be claimed, cleared, and
finished from the CLI and the HTTP API, and every door renders the same scan history. The
same two verbs are reachable as MCP tools and each names its route, but NO chat journey was
hand-run for this record - the acceptance record says so, and this line must not say more.
Design: `docs/specs/2026-09-11-customs-acting-surface-design.md`. The
acceptance record, with one hand-run journal and every cell bound to its test:
`docs/acceptance/m11-acting-2026-09-11.md`.

Milestone 11 was designed as two stages of completion — a claim with evidence, then a
clearance that countersigns it — over a fold that already kept every node's scan history and
released nothing on a claim alone. What was missing, measured on #132 as informing 4/4 and
acting 0/4, was a verb: no surface appended `completion_claimed` or `completion_cleared`, and no
surface rendered the timeline the fold kept.

What shipped:

- `execution claim` and `execution clear` on the CLI; `POST /v1/executions/{id}/claim` and
  `/clear` on HTTP; `claim` and `clear` as MCP tools. One verb module in `core/events`
  (`customs.rs`) decides against the replayed fold and appends at the sequence it read; a
  refusal is a `completion_refused` event with a registry code; a clearance that clears drives
  the graph to its finish in the same reply.
- `graphhelm_execution::CustomsView` — the scan history, open wait, open claim and clearance
  verdicts per node as one typed value, embedded by the one `render()` both doors share as
  `data.customs`.
- `examples/graphs/customs-acting.yaml` — a two-node graph whose first node declares
  `proofKinds: [test_report]`, used by every surface cell and by the hand run.
- Countersign refused at the door (#529 / D-047), naming the issue and appending nothing.

Declared gaps (spec §5; no new issues, owner order 2026-09-05): deadlines are `null` on every
stream `execution start` creates, because `current_graph` is set only by the governor's
`GraphVersionPublished` — so `clearance_expired` has no producer on that path; countersign,
rejection, DLQ redrive/return and the identity registry verbs are deferred; the Studio does not
yet read `data.customs`.

What stays open: #153 (gate-as-graph consumes the completion verb and needs its own
exceptions story); #529 (a signature on the wire, which is what unlocks countersign); and a
`PersistedGraphVersion` on the start path through the D-036 externalizer, which is what gives
those streams deadlines and the sweep something to raise.
