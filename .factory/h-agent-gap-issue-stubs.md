# Gap-map tracking-issue stubs — H Agent, 2026-08-19

Source: `.factory/h-agent-mvp-gap-analysis.md` @ base `0fb0e66`. Marker issues only:
each stub records a promise/reality delta so the gap map survives as issues. None of
these is a design or a milestone proposal. Citations: MASTER_PRD.md (PRD),
docs/product/ROADMAP_AND_ACCEPTANCE.md (RA), both read at `0fb0e66`.

---

## A. Phase-1 product gaps (12)

### 1. Local Studio does not exist — every operator surface is CLI/HTTP/MCP

RA §3.2 lists "local Studio" in Phase-1 scope; PRD §7.3 specifies it (visual graph,
live agents, node editing). Today: zero Studio code — workspace members are core/*,
adapters/*, apps/cli, tools/* only (Cargo.toml @ 0fb0e66). Operationally ABSENT means
acceptance steps 2, 6, 9 (RA §3.4) cannot begin: there is no surface on which a user
connects, sees Graph v1, or reads a Draft's risks visually. Marker issue; not a design.

### 2. No SSH/Docker bootstrap — the Runtime has no install story

RA §3.2 "SSH/Docker bootstrap"; PRD §8 topology puts the Runtime on the user's VPS.
Today: no bootstrap/installer tooling anywhere in the tree. Operationally: acceptance
step 1 ("user installs Runtime on a clean VPS") has no supported path; every current
run starts from a developer checkout. Marker issue; not a design.

### 3. Dynamic harness / Graph Architect does not exist — nothing synthesizes a graph from a prompt

PRD §10 (harness inputs/outputs) and §1's core loop (prompt → task profile → harness →
graph); RA acceptance steps 5–6. Today: graphs enter the system as authored DSL files
(core/schema loads, examples/), and executions drive those graphs (core/execution,
core/runtime); no component reads a goal and emits a graph. Operationally: the
product's first move — "user requests a feature; system generates Graph v1" — is
unstartable. This is the largest single gap in the map. Marker issue; not a design.

### 4. Context Compiler and runtime capsules do not exist

RA §3.2 "Context Compiler"; PRD §4.5 (low-consumption context) and the Context Capsule
concept. Today: "capsule" appears only in schema/graph tests as a wire/schema type
(e.g. apps/cli/tests/schema_cli.rs @ 0fb0e66); nothing assembles a capsule for an agent
at execution time. Operationally: agents cannot receive scoped context; acceptance
steps 16–17 (context-efficiency figures, reuse) have no substrate. Marker issue.

### 5. Native Codex/Claude adapters absent — only the BYOK route exists

RA §3.2 lists "Codex/Claude native adapters where officially supported" AND
"BYOK/OpenRouter adapter" as separate items. Today: adapters/model-gateway ships the
BYOK route (byok.rs references anthropic + openrouter). No subscription-native adapter
exists. Operationally: users with Claude/Codex subscriptions have no first-party route;
BYOK is the only path, which PRD invariants deliberately never auto-fallback into.
Marker issue; provider-support reality should be re-checked at implementation time.

### 6. Project Agent Registry does not exist

RA §3.2 "Project Agent Registry"; PRD §7.1 (registry of agents as reusable, versioned
capabilities). Today: actors exist only as event attribution (PersistedActor,
core/protocols); there is no catalog an execution can discover agents from.
Operationally: the harness gap (#3 above) has nothing to discover even once it exists;
agent definitions live nowhere. Marker issue; not a design.

### 7. Initial Knowledge Graph does not exist

RA §3.2 "initial Knowledge Graph"; PRD §7.1/§4.8 (truth with provenance). Today: zero
code hits in core/ and apps/. Operationally: no cross-execution knowledge accumulates;
every claim's provenance lives only in per-execution event streams. Marker issue.

### 8. Living Docs do not exist

RA §3.2 "Living Docs"; acceptance step 14 ("docs and claims are updated"). Today: docs
are static files maintained by hand; no engine updates documentation from execution
evidence. Operationally: step 14 is unreachable; documentation drift is manual debt.
Marker issue; not a design.

### 9. Basic Dreams do not exist

RA §3.2 "basic Dreams"; PRD §7.1 (consolidation without rewriting evidence — the
Event-Store invariant "historical evidence is never rewritten by a projection or
Dreams" already guards a component that does not exist). Today: zero grep hits.
Operationally: no consolidation/learning pass over past executions. Marker issue.

### 10. No deploy capability — forced-deploy sovereignty flow has no object

RA acceptance steps 8–10 (user removes review, forces deploy to a test environment,
Draft shows risks); PRD §4.6. Today: drafts and waivers exist (core/governor), but
there is no deploy capability to force — the sovereignty machinery has real levers
(partially: #93/#94 track the override side) and no deploy target. Operationally:
steps 8–10 cannot be exercised end-to-end even once overrides land. Marker issue.

### 11. ReuseDecision events and context-efficiency observability do not exist

RA acceptance steps 16–17 name ReuseDecision events, per-node token counts, and a
context-efficiency figure (§9.2) as VISIBLE artifacts. Today: no ReuseDecision in the
event vocabulary (core/protocols); no token accounting surface. Operationally: the
MVP's closing evidence — "measurably lower compiled-context cost through reuse" —
cannot be measured. Depends on #4 but is a distinct observable surface. Marker issue.

### 12. Export-without-credentials surface unverified/absent

RA §3.2 "export/replay"; PRD §4.10 (export without credentials, re-execute with
equivalent routes). Today: replay + events backup/restore/verify exist
(apps/cli/src/commands/{replay.rs,events/}); a credential-free EXPORT manifest as a
product surface is not in evidence (marked UNVERIFIED in the gap analysis — first task
of this issue is to establish whether backup already satisfies the promise, then close
or scope). Marker issue.

---

## B. M09 future-work orphans (4) — cite docs/milestones/arming-the-alarm.md "What remains for M10" (line 292)

### 13. Owner's founding performance numbers (48% / 15→36ms / 3x) have no in-repo reproduction

The close doc requires reproduction with provenance BEFORE M10's incremental-
verified-prefix design builds on them; A's M10 proposal reached the same conclusion
independently. #87 tracks the D1 fix itself, NOT the reproduction. Operationally:
a perf milestone stands on numbers the repo cannot re-derive. Marker issue.

### 14. Flake #3's oracle upgrade (per-round receipt-sequence identity) is untracked

Named in the close doc as separate from the #74 discriminator half that already
shipped. Today: no open issue covers it (title-scan of the open set @ analysis time).
Operationally: concurrent_sweeps' guard still asserts at a coarser grain than the
mechanism it protects against. Marker issue.

### 15. wake_mis_burns is populated but surfaced to no operator

Close doc: "the mis-burn-to-attention wiring (`wake_mis_burns` is now populated and
not yet surfaced to an operator)". #95 is attention-adjacent (edge-aware gating) but
does not cover mis-burn surfacing. Operationally: a mis-burned lease is recorded and
invisible — the exact silence class M09 existed to kill, one hop from its own subject.
Marker issue.

### 16. Storm attribution results live only in .factory, not in a merged record

The close doc demands any future storm comparison be a fresh paired baseline in one
session/disk state. That measurement HAPPENED (2026-08-19: paired 53d212d vs ef51193
same-disk 0/10-0/10; 4/10→0/10 same-commit across disk states; tail bound p99 ~1.8x,
composition bound "sick volume yes, mild pressure no") but its record lives in
.factory/ working files (h-agent-base-measurements.md, a-agent-storm-execution-record.md,
M's ledger). Operationally: the milestone-grade evidence is one `git clean`/archival
sweep away from being folklore. Marker issue: land it in a merged doc.

---

*Discipline notes: no milestone proposals, no sequencing, no designs. Stub 12 may
close on investigation (its ABSENT is an UNVERIFIED). Stub 5's "native" claim should
be re-validated against current provider offerings before posting.*
