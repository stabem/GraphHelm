# Dogfood log — M10 close driven as a GraphHelm execution

H Agent, operator. Started 2026-08-19 (UTC 2026-08-20 02:36). Graph:
`.factory/m10-close.yaml` → execution `exec-m10-close`, mode **manual**, store
`.factory/m10-close-events/`, serve live on `127.0.0.1:41888` (fixture-less 05a server,
binary `target/debug/graphhelm.exe` mtime 22:04 2026-08-19 — A's post-revert clean
build; `.factory/bin/` from the gate-rules memory NO LONGER EXISTS, note below).

Graph shape: 3 entry lanes (#87 A, #88 J, #89 E, each `agent`) → `close_doc` (agent)
→ `evidence_commit` + `changelog` (tool) → `owner_report` (agent, terminal).
Location call: `.factory/` not `examples/` — examples/ is shipped product content
(read-only for this lane; adding an example is a PR decision), and this graph is
operational tracking state, which is exactly what `.factory/` is for.

## What worked, named (dogfooding cuts both ways)

- `graph validate` + `graph lint` caught nothing wrong and the lint warnings
  (GHG101 default timeout, per node) were actionable and correct — added explicit
  `timeoutSeconds`, warnings gone. Authoring loop felt finished.
- `execution start` in manual mode did exactly what D-020 promises: nothing
  auto-ran, and `attention: needs_you` with `waiting_input_node` per lane is the
  RIGHT first answer for a work-tracking graph.
- Serve minted its bearer token TO DISK (`m10-close-events.token`) — my background
  pipe swallowed serve's stdout and the token file made that a non-event. Design
  that assumes the operator loses stdout is correct design.
- `execution status` over HTTP == CLI byte-for-byte (one store, one truth held).

## Frictions (each = gap-map candidate, cross-checked against #105–#120)

**All four posted as issues 2026-08-20: F3 → #132, F1 → #133, F2 → #134,
F4 → #135.** Advance-attempts below feed #132's evidence stream.

### F1 — status has NO per-node state map
`execution.status` returns `nodeStateCounts` (aggregates) + `nodeLastEventAt` +
`attentionReasons`, but NOTHING answers "what state is node X in". Payload keys
verified: no `nodes` field. The attention list names the 3 `waiting_input` lanes,
but the other 4 nodes' states are only deducible by subtracting counts — or by
reading the raw events tail. **This is the order's "a status you could not see",
lived, on the first status call of the first real use.** Cross-check: not covered
by #105–#120 (it is an operator-surface gap below Studio); adjacent to D-040's
monitor. CANDIDATE NEW ISSUE: "execution status names states in aggregate but
cannot name a node's state."

### F2 — 4 downstream nodes report `ready` while their dependencies are unmet
`close_doc` (needs 3 evidence edges), `evidence_commit`, `changelog`,
`owner_report` all count as `ready` at start. #103/#80 fixed DISPATCH gating
(a queued node waits for its edges), but the STATE VOCABULARY still tells the
operator "ready" for nodes that cannot possibly run yet. An operator triaging by
counts reads 4 runnable nodes where 0 exist. Cross-check: sibling of the #89
family (a surface over-promising against execution reality) — lived instance,
CANDIDATE NEW ISSUE rather than an upgrade of an existing one.

### F3 — no verb feeds a `waiting_input` node
The M10-close's real advancing event is "A's PR merged, here is the SHA". The verb
inventory: `signal` admits a governance envelope (not node input), `approve` acts
on Ghost/Blocked only, `resume` re-dispatches held nodes, `amend-budget` is
silence-budget. NOTHING says "here is lane_87's input/completion evidence".
For fixture-less agent nodes the operator has NO way to complete a lane except
(presumably) cancel or waive — and #94 says Waived is surface-unproducible too.
**This is the largest lived gap: the product can REPRESENT external work but
cannot RECEIVE its completion.** Cross-check: #94 (waive unproducible) is the
nearest existing issue and this UPGRADES it with a lived instance; the missing
"complete-with-evidence" verb itself is a CANDIDATE NEW ISSUE (distinct from
waiving: this work SUCCEEDED, waiving would misrecord it).

### F4 — `signal` requires a sealed keyring for a plain-text envelope
`execution signal` refuses without `--keyring`/`--key-id`/`GRAPHHELM_EVENTS_KEY`
even when the store was created keyless (this one was; the pair store precedent
already proved keyless stores are legitimate). An operator wanting to record
"lane landed" as a signal must first stand up sealing infrastructure the rest of
their store never used. Help text admits the tension ("encrypted externalization
for signals is still unbuilt" yet the seal is mandatory). CANDIDATE ISSUE:
signal's keyring requirement should match the store's own sealing posture.

### F5 — CORRECTED: the `.factory/bin` rule was right and I nearly re-earned it
First write of this entry called the missing `.factory/bin/` "memory decay" and ran
serve from `target/debug/graphhelm.exe`. Re-reading the memory's WHY caught it: the
rule exists because a serve running from target/debug HOLDS THE EXE LOCK on Windows
and killed an 18-stage gate on 2026-08-17 — and five lanes were running cargo while
my serve held exactly that lock. Cure applied: serve stopped, `.factory/bin/`
RECREATED, binary copied, serve restarted from the copy (health 200, same token,
execution state intact). **Lock window to disclose: ~02:36–02:56 UTC — any lane
whose graphhelm.exe link failed in that window, that was me.** Lesson: the rule
includes recreating the dir when it vanishes; "the path is gone" never voids a
rule whose reason is the lock, not the path. (Codebase-memory-index deviation note
still stands: surgical greps while lanes run cargo, per no-index rule.)

## Advance attempt #1 — lane_87's real completion vs the verb inventory (2026-08-20, #132 evidence)

Real-world fact first: issue #87 CLOSED via PR #137, merged to main as `6d389eb`
(verified via gh before any verb). Then every plausible verb, in order, verbatim:

1. **approve** → refused, `GHCLI005_EXECUTION_STATE: "node is waiting_input, not
   ghost or blocked"`. Honest, well-named refusal; confirms approve is not the path.
2. **signal (CLI)** → refused AT THE ARGUMENT PARSER: `--keyring`/`--key-id`
   mandatory. And no CLI verb mints a keyring (checked events/gateway/quality
   families) — the precondition names infrastructure the CLI cannot create.
   #135's second lived instance.
3. **signal (HTTP, same keyless store)** → NO KEYRING WALL. Walked the ceremony
   (Idempotency-Key → X-GraphHelm-Actor → X-GraphHelm-Actor-Type must be
   owner|agent → body must carry "signal" → body must carry "evidenceOut" →
   envelope schema), then **ACCEPTED**: `ok:true`, appended at headSequence 19,
   evidence written to `.factory/evidence-lane87-signal.json` (outside the store,
   per the GHE007 rule). Governance verdict: **`decision: "rejected",
   rejectionReason: "signal_not_actionable", mayProposeMutation: false`**.
   Post-signal status: `waiting_input: 3`, attention unchanged, `signalsRecorded: 1`.
   **The product recorded that the work finished and changed nothing.**
   ALSO: CLI demands a keyring where HTTP accepts keyless — the same verb, two
   surfaces, different preconditions — a lived D-039 violation instance (feeds
   #135 and the D-039 record).
4. **resume** → refused, `GHCLI005: "resume refused: not_paused"`.
5. **cancel** → DELIBERATELY NOT ATTEMPTED: it completes the execution as
   Cancelled — recording a false fact about work that SUCCEEDED. Same wrong-fact
   class as waive (#132's first line). The refusal to misuse it is part of the
   evidence.

Conclusion, one line: **#132 confirmed end-to-end — the operator can inform the
product (signal admits, records, seals evidence) but no verb lets the graph act on
the truth; the model of the work and the work itself have permanently diverged
after the FIRST real event.** The signal path also produced the ceremony map an
implementer of the complete-verb will want (headers, envelope, evidenceOut).

## Advance attempt #2 — lane_88 (2026-08-20, #132 evidence, N=2)

Fact: #88 CLOSED via PR #139, merged `d2ef897`. Same verb set, same store, same
running serve. **Identical wall, zero shape change:** approve → `node is
waiting_input, not ghost or blocked`; resume → `resume refused: not_paused`;
HTTP signal (fresh Idempotency-Key `h-dogfood-sig-2`, evidenceOut
`.factory/evidence-lane88-signal.json`) → accepted, appended **headSequence 20**,
verdict again `rejected / signal_not_actionable / mayProposeMutation: false`.
Post-state: `waiting_input: 3`, `signalsRecorded: 2`. CLI signal parser wall not
re-run (argument-parser behavior, deterministic; N=1 stands for it).
Method note: this serve runs the 2026-08-19 22:04 binary, which PREDATES
`d2ef897` — the merge's own new capability (wake_wait receipt consultation)
cannot alter this wall from inside my server, and it is not a completion verb
anyway. Rebuilding the serve to current main needs a cargo slot (not taken;
SLOT.lock law).

## Advance attempt #3 — lane_89 (2026-08-20, #132 evidence, N=3, entry lanes COMPLETE)

Fact: #89 CLOSED via PR #144, merged `d864b93`. Identical wall, third time:
approve → same refusal; resume → `not_paused`; HTTP signal (key
`h-dogfood-sig-3`, evidenceOut `.factory/evidence-lane89-signal.json`) →
accepted, **headSequence 21**, verdict `rejected / signal_not_actionable`.
Post-state: `waiting_input: 3`, `signalsRecorded: 3`.

**The evidence set is now complete and symmetric: every entry lane of the graph
finished in the real world (#87→#137@6d389eb, #88→#139@d2ef897,
#89→#144@d864b93); all three completions are truthfully recorded in the store
(headSequence 19/20/21, sealed, actor-attributed); and the graph consumed none
of them.** The tracking graph is now 100% wrong about its own entry row while
holding 100% of the information needed to be right — which is #132 stated as a
measurement instead of a claim.

Context for the record: #141 (ServerGuard drain) landed 0433e7c; #143 is the
fleet's critical path (gate red on #137's regression, A fixing forward). Final
act pending: when the close doc lands, the close_doc advance attempt runs — the
graph that tracked the close, closed (or provably not closable).

## Operator state (live)

- Execution `running`, attention `needs_you`, 3× `waiting_input` (lanes), 4× `ready`.
- Next real event: any of #87/#88/#89 merging. On each: attempt to advance the
  graph with the real verbs and record what actually works (expected per F3: no
  honest verb exists; the attempt itself is the evidence).
- Serve stays up (background task boa9s8thu); token on disk; store is
  `.factory/m10-close-events/` — nobody else write to it, one writer per store.
