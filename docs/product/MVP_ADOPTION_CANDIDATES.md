# MVP adoption candidates — four capabilities a stranger needs

Status: **PROPOSAL for the owner**, not adopted scope. Nothing here edits
`ROADMAP_AND_ACCEPTANCE.md` §3.2: changing what the product promises is an owner-level
decision, not a milestone-time edit (the M07 record's honest limit #8 established that
rule when the acceptance map correctly refused a clause added at the end of a milestone).

Origin: a comparison against [`akitaonrails/ai-memory`](https://github.com/akitaonrails/ai-memory),
a shipped Rust tool for cross-agent memory, requested 2026-08-17. Its README and docs were
read; its source was not. Claims about it below are therefore about its *declared* design.

## Why this document exists

GraphHelm has shipped seven milestones of engine: an event-sourced store with a hash chain,
deterministic replay, a governor, a tool broker with credential-free Tier 1 workspaces, an
MCP surface, quality gates that must prove they can reject, and a blind judge whose verdict
scoped the milestone after it. What it has never had is **a stranger using it**.

`ai-memory` inverts that ratio: narrower engine, but installable in one command and useful
on the first run, including with no model provider at all. That contrast names four
capabilities. **Three of the four are not new scope** — they are sharper statements of
Phase 1 items already promised or already half-built. Saying so is the point: the cheapest
way to finish an MVP is to discover that part of it already exists.

## The four candidates

### 1. Installable by a stranger (Phase 1 §3.2 "SSH/Docker bootstrap")

**What ai-memory does:** one binary; `install-mcp` and `install-hooks` write the host's
config for it; loopback-only by default; reverse-proxy templates for the exposed case.

**What we already have:** `graphhelm serve` binds loopback and refuses anything else;
a bearer token is minted per store; `examples/chat-surface/` ships a Claude Code plugin
manifest and an MCP config; the CLI is a single binary.

**The actual gap:** every one of those steps is currently performed *by us, by hand, on
this machine*. There is no `graphhelm init` that provisions a store, mints a token, writes
the MCP config for the detected harness and prints the next command. The Phase 1 acceptance
scenario opens with "user installs Runtime on a clean VPS" — untested, never rehearsed.

**Proposed acceptance (verifiable, not aspirational):** on a clean machine with only the
binary, a documented command sequence reaches a running serve, a registered MCP surface and
one completed fixture execution, with a recorded transcript committed as run evidence —
the same one-real-run pattern the acceptance map already enforces for paid runs.

**Cost:** small. This is packaging and a first-run path over surfaces that already exist.

### 2. Retrieval that scales (Phase 1 §3.2 "Context Compiler")

**What ai-memory does:** SQLite FTS5 lexical search, entity extraction, reciprocal-rank
fusion over entity matches and graph neighbours, optional vector similarity, and an
authority adjustment that favours canonical pages over episodic ones — all before
truncation, and all functional with no model provider.

**What we already have:** nothing in this shape. Context capsules are Phase 1 scope and
unbuilt. The pair's own memory is a handful of markdown files loaded whole.

**Why it matters more than it looks:** §9.2 — context efficiency — is the product's north
star, and its v1 estimator is defined as *a free by-product of the compiler's ranking*
(`eligible_candidate_tokens` and `tokens_saved` per node). No ranking, no manifest, no
metric. Acceptance steps 16 and 17 are unreachable until this exists, which means the
product cannot demonstrate its own central claim.

**What the comparison actually buys us:** not a dependency — a *validated recipe*. The
layered design (lexical → entity RRF → graph RRF → optional vectors → authority) is a
concrete starting shape for capsule ranking, proven in a shipped tool, that we would
otherwise invent from scratch. Their authority rule also matches a rule we adopted
independently for the pair's own memory, which is evidence the shape generalizes.

**Cost:** large, and unavoidable — it is already Phase 1 scope. This candidate does not add
work; it de-risks work we owe.

### 3. Multi-harness continuity (Phase 1 §3.2 "Codex/Claude native adapters")

**What ai-memory does:** a portable ledger across harnesses plus one-use handoff packets
delivered at session start, so "quit Claude Code, continue in Codex" is a supported flow.

**What we already have — more than it looks:** the MCP server is harness-agnostic by
construction (ADR-026 keeps the protocol minimal and pull-only), and 05e shipped both a
Claude Code plugin and a Codex configuration. The event store is *already* the portable
ledger: it is the durable, replayable record every surface folds.

**The actual gap:** nothing consumes that ledger as a *resume briefing*. A fresh session in
a different harness can read events but is handed no summary of where the work stands. The
pair hit this exact wall from the other side and solved it with a handoff packet injected
at SessionStart — a factory-local fix that is not a product feature.

**Proposed acceptance:** an execution started under one harness is picked up under another
with a briefing derived *from the store* (never from prose someone typed twice), and the
two surfaces report identical state — the parity guard that already exists for CLI/API/MCP,
extended across harnesses.

**Cost:** small-to-medium, and mostly a projection plus a surface. The hard part (durable
portable state) is done.

### 4. Zero-provider mode as a stated guarantee (partially shipped, never promised)

**What ai-memory does:** declares that full retrieval works with no LLM configured, with
rule-based consolidation as the fallback. Robustness as a product property.

**What we already have:** the fixture/simulation path. An entire execution can run to
completion with no model provider — that is how 05a shipped before any gateway existed, and
the async driver still runs the fixture executor as a first-class path with parity proven
against the real one.

**The actual gap:** it is an implementation detail, not a promise. Nothing in the product
docs says "GraphHelm is useful before you authenticate a route", no acceptance clause pins
it, and no test asserts that a *fresh install with zero credentials* reaches a useful state.
A capability nobody is told about, and that no test defends, decays into an accident.

**Proposed acceptance:** a named product mode with a gate stage — from a clean store with
no manifest, no keyring and no credentials, a documented flow produces a completed
execution, a monitor page and an export. Most of this passes today; the value is in the
promise and the guard, not in new machinery.

### 5. Interaction cost as a gated axis — "six calls to answer *can I sleep*"

**The defect that provoked this candidate:** the M07 judge, having read a surface that was
*correct*, still complained that answering the operator's one question cost six MCP calls.
Nothing in the delivery was wrong. Everything in it was expensive.

**Why every gate we own missed it.** The pathogen suite catches deliverables that are green
by correctness measures and useless by construction; this one is green *and useful*, merely
costly. The geometry evaluators score whether a surface is broken, never how many round
trips it takes to read. The correctness suites assert answers, not the price of asking. Our
immune system has an antibody for uselessness and none for friction.

**The uncomfortable part:** we designed the signal and then did not gate on it. The blind
judge's verdict contract already carries `stepsOverPar` and `stallPoints` — M06 put them
there on purpose. But they are only ever produced by an announced, paid, real-model run,
which means the cheapest thing to measure is currently the most expensive thing to observe,
and nothing fails a build when it regresses.

**Proposed remedy, in three parts, all deterministic and provider-free:**

1. **Par declared with the question.** Every operator question in a user story states its
   budget: "can I sleep" is *one call*. A budget nobody wrote down is a budget nobody can
   miss.
2. **A call-counting gate stage.** Drive the MCP/API surface over the fixture path — no
   model, no cost, no flake — and count the calls needed to answer each declared question.
   Over budget fails the build. This is the same trick the doorbell's zero-polling proof
   already uses (a counting proxy in front of the serve, asserting a measured number rather
   than a claimed one), pointed at interaction cost instead of network traffic.
3. **A new pathogen class: correct-but-expensive.** The thymus currently breeds ten
   useless-but-green specimens. Add the specimen that is *right in six calls where one
   would do*, and every gate must reject it to stay certified. Growing the suite changes
   its digest and voids existing certifications by construction — the M06 machinery doing
   exactly what it was built for, aimed at a pathogen we did not know existed when we built
   it.

**The design rule this hardens into:** one operator question, one surface, one call. When a
question provokes predictable follow-ups ("is it wedged?" → "why?" → "did my alarm fire?"),
the answer embeds them instead of scattering them across endpoints. M07's own findings are
this rule violated three times: no time data in `status` forced a second call, causes living
only in the event tail forced a third, the alarm's state living apart from the execution's
forced a fourth.

**Cost:** small, and it pays immediately. It converts the most expensive kind of feedback we
have — a paid judge run — into a cheap deterministic check, which is the same trade the
acceptance map already made for evidence.

## Recommended shape (owner decides)

1. **Fold 1, 3, 4 and 5 into Phase 1 as refinements**, with the acceptance sentences above.
   They are cheap, they are mostly already true, and each one converts something we do by
   hand into something a stranger can do. Candidate 5 is the cheapest of all and the only
   one that closes a hole in the gates themselves.
2. **Leave 2 where it is** — the Context Compiler is already Phase 1 scope — but adopt the
   layered-retrieval recipe as its documented starting design instead of inventing one.
3. **Do not adopt the tool itself.** Running its daemon would put a second SQLite store of
   record beside our event store: two sources of truth, which is the failure mode this
   architecture exists to refuse, and which the M07 verdict just demonstrated in miniature
   (three honest copies of one predicate, all agreeing until the day they did not).

## The uncomfortable observation, kept in the record

Seven milestones deep, this project's feedback loop is a judge it built for itself. That
loop is rigorous — it caught two defects that survived cross-review in both directions —
but a judge cannot report the friction a first-time user feels in the first ten minutes.
Its own verdicts have started to gesture at it ("answering *can I sleep* cost six MCP
calls"). Every candidate above is, in the end, one bet: **that the next unit of quality
comes from someone outside the loop using the thing, and that the fastest path there is to
finish the parts of Phase 1 that make being outside the loop possible.**
