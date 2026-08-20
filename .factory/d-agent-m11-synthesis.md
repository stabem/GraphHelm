# M11 synthesis — working backwards from the sealed bar

D Agent, 2026-08-20. Input to the M11 proposal; **nothing here is a decision**, and #153's bar is
sealed — this argues about what the bar will *encounter* and in what order, never about moving it.

Sources, all public and read for this document rather than recalled: **#153** (anchor and kill bar),
its two comments — the pre-code findings (`issuecomment-5353059144`) and **L's addendum**
(`issuecomment-5353119498`) — **#152** (parallelization phase 1), **#154** (the doc defect from
#101), and the open-issue inventory as of `origin/main` @ `9f3ee1e`. Superseded predecessor:
`d-agent-m11-engine-synthesis.md`, which said in its own closing that it had not read the M11 frame.
It now has one, and §5 records what that changed.

**No measurement was taken for this document.** The slot is J's (#118) with a measurement in flight;
zero cargo was run. Every claim is a code read at a named base or a quote from a public artefact.

---

## 1. The frame decides the shape of the milestone

#153 is not one deliverable among several — it is an acceptance test, so it *orders* everything
else. The right question for each candidate lane stops being "is this valuable?" and becomes
**"does the gate-graph run without it?"** That reordering is most of this document's content, and it
demotes work that is genuinely good (see §5).

The milestone's honest verdict is defined in advance, which is the part worth protecting: if the
gate-graph cannot run by M11's end, the blocking verb gap must be **nameable by issue number**, and
that named gap *is* the result. A milestone that can fail loudly is worth more than one that
succeeds vaguely.

## 2. Critical path — what the gate-graph cannot run without

Working backwards from "27 stages execute as nodes with a verdict someone merges on":

| Piece | State today | Issue |
|---|---|---|
| **Process-executor node driver** — run a command, capture exit + streams as events, drain child pipes, map exit to outcome | **does not exist** — the new capability of the milestone | needs one |
| **Resource edges / leases** — compile-lock (exclusive), DB-lease (counted), port-lease | **do not exist** — dependency edges only | needs one |
| **A completion verb with evidence** — external work can be COMPLETED, not only waived | absent; lived as dogfood F3 | #132, #94 |
| **Per-node state on the status surface** + gated-vs-ready vocabulary | counts only; vocabulary over-promises | #133, #134 |
| **Per-node evidence contract** (what each node must record) | in flight | #152 |
| **Parallelism that actually parallelizes** | policy unified, mechanism is not — §3 | #154 documents it; the design call is open |

The first two are the milestone's real engineering. Everything else on this list exists in some
form and needs connecting — which is the shape the predecessor synthesis identified and which
survives contact with the frame.

## 3. The parallelism collision, and why it is a design decision rather than a bug

`parallel_limit` returns one number that means two different things: the async runtime driver spawns
the plan into a `JoinSet` and runs nodes concurrently; the sync CLI driver walks the plan in a
blocking `for` loop, so the same number only widens how many nodes **one sequential pass** covers.

Two consequences, both already public and both binding on how M11 measures:

- **L's rule (adopted):** a measurement against this bar **must declare which driver it touched**.
  A CLI-path measurement cannot certify the wall-clock item — it would produce correct numbers
  against the wrong driver.
- **The collision:** item 2 is reachable only through the async path, while D-039 parity is a
  precondition of the same bar. Either "identical" means serial on both (item 2 fails at ~100%), or
  the sync driver gains concurrency it does not have. **This is resolvable on paper, and it should
  be resolved before the process-executor driver is written** — the driver's author should not
  inherit an unresolved contradiction as an implementation detail.

The reason this was invisible until now belongs in the M11 design reviews: #101 unified the
*policy* and, in doing so, removed the *signal* that two copies had carried by accident. One shared
function reads as unification. The policy is unified; the parallelism is not. #154 writes that
sentence into the surviving symbol.

## 4. The sequencing claim nobody has made yet: item 2 is downstream of phase 3, and it is a bet on the compile/test split

#153 already names #152's phases 2-3 as creating the parallelism the resource edges exploit, and it
already names **compile-lock as an exclusive resource edge**. Put those two together and a
consequence falls out that changes *when* the milestone may measure:

1. If a large fraction of the gate's wall-clock is compilation, and compilation is serialized by an
   exclusive compile-lock (correctly — concurrent `cargo` invocations contend on one target dir
   anyway), then **the gate-graph's achievable speedup is bounded by the non-compile fraction.**
2. Item 2 (≤60%) is therefore not a property of the graph engine at all. It is a **bet on the
   gate's compile/test time split** — and nobody has measured that split.
3. #153 permits re-deriving item 2 *before* the first measured run if stage discovery shows less
   inherent parallelism than expected. **Measuring before phase 3 (compile-once / test-parallel)
   would exercise exactly that clause against a gate that has not been parallelized yet** — and the
   bar would be lowered for a true observation about a temporary state.

**Recommendation, cheap and available before any M11 code:** #152's run-manifest records per-stage
wall time. **Read the first manifests and publish the compile/test split before item 2's number is
either defended or re-derived.** That converts the bar's most contestable clause from a negotiation
into an arithmetic question, and it costs nothing beyond running the gate you were running anyway.

This is the same failure shape as the un-set-budget decoy already adopted into the bar's procedure:
a true observation, sourced from a temporary condition, consumed as if it described the structure.
Both are the flattening family — two causes collapsing into one legal number.

## 5. What the frame changed in my earlier reading — recorded because it was wrong, not because it improved

The predecessor (`d-agent-m11-engine-synthesis.md`) argued lane order bottom-up from three lanes I
worked. Against #153:

- **#114 / #129 (deploy) leave the critical path.** The gate-graph needs a process-executor driver;
  a deploy node is not on the route. My sequencing argument (#93 → #94 → #129 → #114) remains correct
  *within* that lane and was wrong as an M11 priority claim. It stays a real operator gap, correctly
  ordered, and it is not this milestone's spine.
- **#87 c2 changes character, not position.** Still independent of everything. But the contention
  exposure carried forward from the c1 review — the cache's critical section is O(suffix bytes)
  because the file read happens under the mutex — becomes the **most likely first hit** under
  #153's load, since the gate-graph is the first workload with genuine concurrency and resource
  contention. An independent lane whose risk profile the frame raised.
- **#93 (dispatch-time override) is not on the bar's path, and probably is on the operability
  path.** Item 4's replay is offline journal reading, which needs no override. But re-running one
  failed stage after fixing it is the ordinary operation of a CI graph, and nothing today can force
  a node to dispatch. Flagged as a question for the proposal rather than asserted as a requirement —
  the distinction between "the bar needs it" and "an operator will want it on day two" should not
  be blurred.

## 6. Packaging call on #154 (mine to make, per the routing)

**Open M11 with it, rather than riding the M10 close.** The restore half is M10 debris — my own
defect from #101 — so attribution would argue for the close. The consumer argues louder: the clause
says `parallel_limit` is **the LIMIT, not the MECHANISM**, and every M11 measurement depends on that
distinction being true and *present* where it will be read. L's rule ("declare which driver you
measured") is unenforceable if the symbol everyone reads says nothing about mechanism. Package where
the consumer is; state the trade rather than hide it.

Small, doc-only, and it still runs the full gate before merge — "it is only a comment" is not a
reason to skip measurement.

## 7. Risk register

| Risk | Why it bites | Already covered by |
|---|---|---|
| Measurement runs on the CLI path and certifies nothing | correct numbers, wrong driver | L's addendum — declare the driver |
| Un-set budget makes the graph serial and the bar looks unachievable | authoring omission read as structure | the two binding guards in the bar's procedure |
| Item 2 re-derived against a pre-phase-3 gate | true observation, temporary condition | **§4 — nothing yet; this is the open one** |
| Concurrency unverifiable from the journal | `in_flight_nodes` is driver-local, never an event | timestamps (`occurred_at`) — **emitters still UNVERIFIED** |
| Doc-attachment-class defects | no lint measures it | **nothing; named structural gap (#154)** |

## 8. What this synthesis does not establish

- **Nothing measured.** No cargo, no timing, no execution — the slot is J's.
- **The compile/test split is unknown to me.** §4's recommendation is an argument about *when* to
  measure, not a claim about what the number will be. If the split turns out to favour test time,
  item 2 may well be reachable before phase 3 and §4 costs one reading of a manifest.
- **I did not verify the event emitters** for item 3 — envelope only. That gap is stated in #153's
  comment thread and is unchanged here.
- **No estimates.** Nothing in this document prices any lane.
- **The process-executor driver's placement on the async path is an assumption**, marked as one in
  the public comment and unchanged: if it lands elsewhere, §3's collision belongs to whoever chooses.
