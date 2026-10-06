# Keel and JPD records: one thread per change

A skill's output is only connected to the graph when it is recorded there. Inside a GraphHelm
execution, each Keel and JPD skill ends by recording what it produced as a signal on the node it
works for (`tool:signal`, or `cli:execution signal` offline). The signals form one thread through
`replyTo`, so the Studio and any reviewer can see which step was skipped.

| Step | Skill | Signal `type` | `replyTo` |
|---|---|---|---|
| Journey | `journey-verifier` (from the `journey-contract` draft) | `jpd.journey` | none (thread root) |
| Obligation | `journey-verifier` (one per promise) | `jpd.obligation` | the journey signal id |
| Card | `keel` | `keel.card` | the obligation it serves (or none for a change with no journey) |
| Proof | `journey-verifier`, `keel` | `keel.proof` | the card signal id |

Rules:

- `source` is `{"type": "node", "id": "<the node this work belongs to>"}`.
- `evidence` holds one observation per entry. For a card: the promise, each scope path and the
  proof command. For a proof: the command and what it printed, or `OBSERVER_MISSING: <what>`.
- A proof that failed is recorded too, with `severity: "high"`; a later green proof replies to the
  same card and never replaces the red one.
- Outside an execution (no MCP and no `GRAPHHELM_EXECUTION_ID`), the card is written to
  `.graphhelm/keel-card.json` instead, which the plugin's hooks read
  ([`KEEL_CHECK.md`](KEEL_CHECK.md)).

No new node types and no new tools: these are ordinary graph signals (`schemas/graph-signal.schema.json`),
so the Runtime's existing evidence and threading rules apply unchanged.

## Screen captures and walked transitions (ADR-044)

Two more signal kinds prove a journey visually. They are not part of the `replyTo` thread above;
the fold groups them by contract and step.

| Signal `type` | Description `protocol` | Producer | Carries |
|---|---|---|---|
| `jpd.screen_captured` | `graphhelm-screen-capture-v1` | `graphhelm journey capture` | `contractId`, `stepId`, `revision`, `dirty`, `viewport`, `observer`, optional `pr` and `phase`; exactly one image attachment |
| `jpd.transition_walked` | `graphhelm-transition-walked-v1` | `graphhelm journey walked` | `contractId`, `fromStepId`, `toStepId`, `revision`, `observer`, `fromCaptureId`, `toCaptureId` |

The description is a JSON document with the schema
`extensions/builtin/graphhelm-jpd/schemas/screen-capture.schema.json` or
`transition-walked.schema.json`. A dirty capture is shown but never counts as fresh proof. Records
whose ids are path-like (`/`, `\`, `:`, `..`) are ignored by the fold.

