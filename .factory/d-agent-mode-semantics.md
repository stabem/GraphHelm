# ExecutionMode — what it promises, what it does, and where they part

**Status: RATIFIED 2026-08-19 — OPTION B.** Drafted by D Agent from existing evidence only; no
semantics invented here. This table is the contract #79's close cites and the surface fix enforces.

- **B — ADOPTED NOW.** Restore the scope *in the words, at the surfaces that dropped it*: a doc
  comment on `--mode`, `--help` text, a schema `description`, and MCP tool descriptions, each
  saying in substance — *"manual (graph): only the owner changes the graph — governs mutation
  autonomy per D-022; does NOT hold dispatch, use `pause`."*
- **C — DEFERRED, with its trigger named.** The vocabulary change (`--mutation-policy
  auto|supervised|owner-only`) is more honest to behaviour but costs a schema version, a wire
  change and a migration. **Trigger: batch it with the next wire change that forces a schema
  version bump** — do not force one for this alone.
- **A — REJECTED**, on engineering grounds rather than deference: folding dispatch-gating into
  `mode` conflates two axes the design correctly separates (mutation autonomy vs dispatch
  control), and changes the meaning of every existing execution. **Reversible and owner-visible:
  A remains reopenable by the owner.**

**The absent-mode row was checked, not assumed — and it is sound.** `schemas/event-envelope.schema.json:340`
lists `mode` in `executionStarted`'s `required`, with `additionalProperties: false`. So a
well-formed `execution_started` **cannot** carry no mode; `projection.mode == None` means only
*"no ExecutionStarted has been folded yet"* — an unrooted stream. And that case is handled
**fail-closed**: `decide_mutation` maps `Some(Manual) | None` to `Rejected(ManualMode)`
(`inflight.rs:116-118`), the strictest verdict. **No fail-open path found.** Recorded as checked
rather than left as an open worry.

**Base:** `0f4e7fe`. Every citation below was read at that commit.

---

## 1. The evidence, in full

| # | Source | What it says about mode |
|---|---|---|
| 1 | `docs/DECISION_REGISTER.md:28` — **D-022** | "Modes \| Autopilot, Supervised, and **Manual Graph**, switchable during execution." |
| 2 | `docs/superpowers/specs/2026-08-13-graph-engine-governor-design.md:105-110` §5.5 | "A **mutation** is evaluated against the mode in force at the instant the Governor **accepts** it." Binds at acceptance, not proposal. |
| 3 | `core/protocols/src/simulation.rs:65-79` — the type's own docs | "How much autonomy the owner has granted this execution, per D-022." Autopilot: "may accept its own **mutations**". Supervised: "**expansions** await owner confirmation". Manual: "only the owner **changes the graph**". |
| 4 | `core/governor/src/inflight.rs:102-120` — `decide_mutation` | **The only production branch on mode in the entire tree.** Returns a `MutationDecision`. |
| 5 | `apps/cli/src/args.rs:159-160` | `--mode` has **no doc comment at all** — `--help` prints nothing about what the modes mean. |
| 6 | `apps/cli/src/args.rs:176-177` | The *only* CLI text mentioning mode, on `execution signal`: "reports the **governance verdict** for the mode in force." |
| 7 | `schemas/event-envelope.schema.json:306` | `"executionMode": {"enum": ["autopilot","supervised","manual"]}` — bare enum, no descriptions. |
| 8 | `apps/cli/src/commands/execution/start.rs:252-259, :211` | Parses and validates mode, records it in `ExecutionStarted`, **never branches on it.** |
| 9 | `core/runtime/src/driver.rs` | Zero references to mode. Auto-approves every `Draft` node each pass (`:446-468`) and dispatches via `ready_set`, identically in every mode. |
| 10 | Observed (M09 storm; #79's own run) | `--mode supervised` dispatched all nodes unattended; `mode: manual` over HTTP ran a 3-node graph to completion with no operator action. |

## 2. The table

| Mode | **Promises** (sources 1-4, 6) | **Does today** (sources 4, 8-10) | Delta |
|---|---|---|---|
| **Autopilot** | The Governor may accept its own graph mutations without asking. | `decide_mutation` → `Accept`. Dispatch unaffected. | **none** |
| **Supervised** | The Governor proposes; expansions await owner confirmation. | `decide_mutation` → `RequiresApproval`. Dispatch unaffected. | **none** |
| **Manual** | Only the owner changes the graph. | `decide_mutation` → `Rejected(ManualMode)`. Dispatch unaffected. | **none against the written promise** |
| *(absent)* | Schema requires it; an absent mode must be an error, never a default (source 3). | Reads identically to Manual for mutations (`Some(Manual) \| None`). Reachable only on a stream with no `ExecutionStarted`. | none |

**The single most important row is the one the table cannot show, because it is the same for all
three: DISPATCH IS MODE-INVARIANT.** Whether a node is approved, queued and run is decided by
`ready_set` and `dispatch_plan` alone. No mode changes it, in either the sync or the async path.

## 3. Where the gap actually is

**The code matches every written decision.** #79 reads the driver's mode-blindness as a defect; on
the evidence it is correct behaviour — the driver performs no graph mutations, so it has nothing
to honour, and the nodes it auto-approves are already in the published spec (`spec.nodes.keys()`),
not ghost expansions.

**The gap is between the word and the promise, not between the promise and the code.**

> **D-022 says "Manual GRAPH". Every operator-visible surface dropped the qualifier.**
> The CLI flag is `--mode manual`, the enum variant is `Manual`, the schema value is `"manual"`,
> the event payload is `mode: manual`. The word that carried the scope survives only in the
> decision register, which no operator reads.

So an operator sending `{"mode":"manual"}` has exactly two cues: the bare word, and `--help`, which
says nothing (source 5). "Manual" in an execution-control API conventionally means *nothing runs
without me*. They get *nothing changes the graph without me*, and then the log records `mode:
manual` beside three nodes that ran unattended.

**That is this factory's log-that-lies class — but the lie is in the vocabulary, not the driver.**

## 4. Options, with costs. NOT a recommendation.

| | Option | Cost |
|---|---|---|
| **A** | Extend modes to gate dispatch as well as mutations. | Expands D-022. Changes the meaning of every existing execution: the storm and every `supervised` fixture would change behaviour. Largest blast radius. |
| **B** | Keep mutations-only; close the gap in the surfaces — `--help` text, schema `description`s, API docs — and, if operators genuinely need it, add an explicit start-held mechanism rather than overloading mode. | Small, additive, no wire change. Leaves the word "manual" still doing misleading work unless the help text is read. |
| **C** | Restore the scope in the vocabulary itself (e.g. `--mutation-policy auto\|supervised\|owner-only`). | Most honest to actual behaviour. Wire-vocabulary change: schema version, event payload, CLI flag, migration. |

**One observation that bears on the choice, offered as evidence rather than preference:** the
decision register *already* scoped this correctly. D-022 is not ambiguous — it says "Manual
Graph". Every place the qualifier was dropped is a surface, not a decision. That makes this a
**drift from a decision that was right**, which is a different repair from *changing* a decision.

## 5. What the guards enforce once ratified

Only after ratification, and only what the table says:

- If **B**: the guard asserts the *documented* contract — mode changes `decide_mutation`'s verdict
  and nothing else; a sabotage that makes dispatch mode-sensitive must fail it. Plus doc/schema
  additions carrying the scope.
- If **A**: the guard is #79's original red — `mode: manual` must hold nodes undispatched — and it
  encodes a *new* product promise, which is why it needs ratification first.
- If **C**: guards follow B's shape, plus a vocabulary migration with its own compatibility tests.

**No guard gets written against inferred semantics.** That is the whole reason this document
exists.
