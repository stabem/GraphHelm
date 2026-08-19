# Competitive / inspiration survey — the road to "best harness system on the internet"

Author: B Agent, 2026-08-19. Read-only web research via three parallel research agents
(WebFetch), zero cargo, zero repo writes. Feeds the MVP definition at M09 close.

PROVENANCE OF EVERY CLAIM: each external claim below cites (repo, file/page, section) as
captured by the research agent that read it. The agents read model-condensed fetches, not raw
bytes — section names are as the fetch reported them. Claims I could not ground are marked
INFERENCE (mine or the agent's, attributed). Star counts reported by fetch summarizers were
inconsistent and are treated as UNVERIFIED throughout. GraphHelm-side citations are to
MASTER_PRD.md sections and the repo at efd85d0 / branch d10916b.

Surveyed: deepseek-ai/deepseek-harness (deep), composiohq/composio (deep),
manaflow-ai/subrouter, akitaonrails/ai-memory (deep), obra/superpowers,
Yeachan-Heo/oh-my-claudecode, plus skims (obsidian-skills = 404, likely obra/knowledge-graph;
marketingskills = coreyhaines31/marketingskills, not obra; OpenMontage = calesthio/OpenMontage).

---

## 0. The headline comparison: deepseek-harness vs GraphHelm

**deepseek-harness is not an eval harness — it is an agent runtime** ("everything is a
plugin" on a vendored plugin framework; agent loop itself is a plugin) (deepseek-harness
README intro; docs/architecture.md "Cordis Framework"). The name collision is noise; the
substance overlap is real and narrow:

**Where they independently arrived at our thesis** — their core invariant is
"model-visible means logged": an append-only SessionEvent log is the source of truth and
model history is DERIVED from it by projection (`deriveMessages()`)
(docs/architecture.md "Session Log as Truth Source"). That is GraphHelm's event-store +
fold/replay thesis (PRD 4.8 truth-with-provenance, 4.10 reproducibility) restated by a
frontier lab. Validation, not competition.

**Where GraphHelm asserts what they do not:**
- Their replay is TRANSCRIPT replay (raw chunks preserved); nothing addresses execution
  replay, tool-side nondeterminism, or environment capture (agent INFERENCE from
  docs/architecture.md — no stronger claim found). GraphHelm's replay re-hashes every event,
  refuses corrupted history, and the fold is the single source of projections — execution
  history that PROVES itself, not a transcript that describes itself.
- They have NO versioning/compatibility story — "developer preview", breaking changes
  promised, on-disk format disclaimed (README "Developer preview"; AGENTS.md pre-release
  stance). GraphHelm has the schema digest ritual (byte-identical copies, canonical digests,
  catalog refusal on mismatch, CHANGELOG discipline) live since M0x and exercised twice today.
- Their guard standard is 100% per-file COVERAGE as a CI gate (AGENTS.md commands).
  GraphHelm's standard is sabotage-observed guards with panic-site-named reds — coverage
  measures that lines ran; our standard measures that failure is SEEN AT the assertion
  (vacuous-red rule, 2026-08-19). Ours is strictly stronger and is a marketing-grade
  differentiator: "guards that have been watched fail" vs "lines that have been executed".

**Where they are ahead, honestly:**
- Capability seams (Service Definition / Provider / Consumer): swapping one provider (e.g.
  remote sandbox) moves Bash/PTY/LSP without forking consumers (docs/architecture.md
  "Capability Seams"). Cleaner than anything we have specced for the runtime's swap points.
- Engineering-discipline artifacts: "Agent Notes" required in-PR for non-trivial changes
  (AGENTS.md); keyless CI lanes + real-API lane that self-skips without keys; snapshot tests
  with keyless replay fixtures (docs/development.md "CI gates"; AGENTS.md testing).
- Typed event evolution: new model-visible input REQUIRES extending SessionEventMap and
  rendering to the log; branded IDs, assertNever on closed unions (AGENTS.md conventions).
  Same instinct as our fold's no-catch-all match (M09-A #70), theirs at the type layer.

---

## 1. ADOPT — take the idea, build it our way

| # | Idea | Source (cited) | Lands on |
|---|------|----------------|----------|
| A1 | **Bounded authority multiplier for memory**: rules/decisions tiers BIAS ranking, never hard-filter; "retrieved text remains untrusted historical evidence and never gains instruction authority from its namespace, tier, tags, pin, or rank" (verbatim) | ai-memory ARCHITECTURE.md retrieval; README recall | Our MEMORY.md REGRA/STATUS split + M07 handoff-provenance rule — same instinct, independently arrived at; theirs adds the enforcement wording and the bias-not-filter mechanics. Adopt the sentence into our memory/capsule design as a stated invariant of context capsules (PRD 10.2 "context capsules"). |
| A2 | **TTL-beats-pin + lint on conflicts**: `expires_at` expiry hard-deletes after sweep; pins exempt from decay but "a TTL beats a pin; memory_lint warns about pinned+expiring combos"; feedback floors salience but "never deletes — lowers confidence and flags for review" | ai-memory README staleness section | Our STATUS-perecível convention is manual today. Adopt: explicit expiry semantics + a lint that names contradictory metadata. Maps to memory design when memory becomes a product surface (PRD 7.1 framework scope). |
| A3 | **Pre-written rationalization rebuttals in process rules**: excuse-rebuttal table in the TDD skill; "This is different because..." named as itself a red flag; "Violating the letter of the rules is violating the spirit" | superpowers skills/test-driven-development/SKILL.md (read directly) | Our gate texts and review rulings already do this ad hoc (e.g. "no by-construction exemption"). Adopt the FORM: each binding rule ships with its own rebuttal table. Lands in quality gates docs / the eventual public methodology doc. |
| A4 | **Agent Notes required in-PR** for non-trivial changes — design rationale lives in-repo, same PR | deepseek-harness AGENTS.md; docs/development.md `.agents/notes/` | We do this via long commit bodies + .factory design files, informally. Adopt as a named convention: the design file rides the change. Cheap; mostly codifying existing practice. |
| A5 | **Session-handoff hygiene**: new automatic handoffs EXPIRE older ones from the same cwd; handoff commit atomic with the end watermark | ai-memory README cross-session | Our handoff beacon is overwrite-only by rule but nothing expires a stale beacon from a dead lane. Adopt the expiry idea for multi-agent beacons. Maps to factory protocol docs. |

## 2. ADAPT — the mechanism is right, the shape must be ours

| # | Idea | Source (cited) | Adaptation + landing |
|---|------|----------------|----------------------|
| B1 | **Meta-tools runtime discovery**: sessions expose `SEARCH_TOOLS`/`MULTI_EXECUTE` instead of preloading schemas; "the agent discovers app tools at runtime through search" | composio docs/sessions-vs-direct-execution "Meta Tools" | Directly serves PRD 4.5 low-consumption-context: don't preload the capability catalog into every agent's context — give nodes a search-then-bind tool surface. Adapt into the tool-broker design (05c lineage): the harness manifest's context_plan decides preload vs discover per node. INFERENCE (mine): their weakness — correctness depends on the model driving meta-tools well — is why ours must make discovery results part of the evidence trail. |
| B2 | **Date-stamped tool versioning with mandatory pinning**: `20251027_00` versions; manual execution REQUIRES explicit version; env/per-call pinning; `dangerouslySkipVersionCheck` marked never-production | composio docs/migration-guide/toolkit-versioning | We version schemas by digest; tools in the broker should carry the same discipline: a node's evidence names the TOOL VERSION it ran against, or replay claims less than it should. Adapt digest-pinning (not dates) into tool-broker contracts. Maps to PRD 10.3 "registered capabilities" + reproducibility 4.10. |
| B3 | **Two-layer auth: auth-config (HOW) vs connected-account (WHO)**, per-user keying, "credentials never pass through your app or the model" | composio docs/authentication "Core Architecture", "OAuth Flow" | Same separation our PRD 4.7 capability-by-scope-and-time implies but never names. Adapt the two-layer vocabulary into the capability/permission design; the "never through the model" property is a hard invariant worth stating in PRD §security. |
| C1 | **Headroom-aware sticky routing**: conversations pin to one account; new ones go to most rate-limit headroom; tie-breaks = protect low-headroom, prefer quota-resets-soonest | subrouter README routing section | Not for account arbitrage (AVOID that half, ToS-gray — agent INFERENCE) but the SIGNAL is right for model_routes under budgets (PRD 10.2 outputs, budgets input): route new nodes to the model/provider with headroom, keep an execution sticky to its model for cache/consistency. Lands in harness-manifest model_routes design. |
| C2 | **Verify→fix as first-class pipeline stages** with executor/critic role separation | oh-my-claudecode README modes (`team-plan → prd → exec → verify → fix`) | Our factory already lives this (review/sabotage/verdict), but the PRODUCT graph should name verify and fix as node TYPES in the operational graph (PRD 11.1), not leave them as conventions. Validation + a naming adaptation. |
| C3 | **Pipeline manifests + stage-director skills + budget caps + decision audit trails + approval gates** | OpenMontage (calesthio) repo skim | Independent convergence on the harness_manifest shape (PRD §10) in a video-production domain. Worth one read at MVP-definition time as a parallel-evolution check; no mechanism to copy sight-unseen. |
| C4 | **Capability seams (Definition/Provider/Consumer)** | deepseek-harness docs/architecture.md "Capability Seams" | Adapt the three-role vocabulary for the runtime's swap points (sandbox, store, model adapter). Our ports exist (05d); the discipline of "swap a provider without forking consumers" is a spec sentence worth writing into runtime docs. |

## 3. AVOID — named traps, each with the reason

| # | Trap | Source (cited) | Why avoided |
|---|------|----------------|-------------|
| V1 | **Policy hooks that live in one surface**: composio's MCP path bypasses SDK tool-call modifiers — "tool-call modifiers don't run" over MCP | composio docs/sessions-via-mcp "Trade-offs" | This is the flattening hazard: local policy/redaction silently not applying on one of two equivalent surfaces. GraphHelm's precedent already forbids it ("no surface may recompute the attention verdict"); generalize: POLICY LIVES BELOW EVERY SURFACE, at the store/fold layer, never in a client SDK. PRD §8/§10.3 material. |
| V2 | **Unsandboxed custom tools with ambient auth**: custom tools "run in-process alongside remote tools", "not supported in the sandbox", with `ctx.proxyExecute()` inheriting session auth | composio custom-tools page "Execution Location", "Sandboxing" | Violates PRD 4.7 (an agent does not inherit another's access). Our tool-broker must never give in-process code the session's ambient credentials. |
| V3 | **Coverage percentage as the quality gate** | deepseek-harness AGENTS.md (100% per-file coverage CI gate) | Coverage proves execution, not meaning — the vacuous-green/vacuous-red family. Our sabotage-observed standard supersedes it; adopting a coverage gate would ADD a number that reads as more than it measures. |
| V4 | **Kitchen-sink orchestration surface**: ~10 modes, 19 agents, keywords + commands + CLI; "saves 30-50%" with no cited methodology | oh-my-claudecode README (agent INFERENCE on the number) | PRD 4.2 smallest-sufficient-graph is the exact counter-thesis. Also a cite-or-mark violation as marketing: never publish an efficiency number without the base, N, and scope. |
| V5 | **Prompt-level process enforcement as the only layer** | superpowers README how-it-works (agent INFERENCE: injected text + hooks, nothing mechanical prevents skipping RED) | Adopt their rebuttal-table FORM (A3) but our enforcement stays mechanical where it matters: fold refusal, gate scripts, digest rituals, sabotage evidence. Prose rules bend under pressure; stores do not. |
| V6 | **Subscription-pool arbitrage** | subrouter README (agent INFERENCE: ToS-gray) | Not our product's business; only the routing signal (C1) is worth keeping. |
| V7 | **No self-hosting story / hosted-backend dependency** for credentials, schemas, versioning, execution | composio (agent INFERENCE from fetched docs; no self-host page found) | PRD 4.9 real-open-source: no mandatory proprietary backend. Anti-pattern for us by charter. |

## 4. ALREADY-HAVE — where GraphHelm is ahead, with the receipts

| # | Property | Their nearest analog | Our receipt |
|---|----------|----------------------|-------------|
| H1 | Append-only log as single source of truth, projections derived | deepseek-harness "model-visible means logged" (docs/architecture.md) | Event store + fold since M0x; theirs validates the thesis; ours adds integrity hashing + replay refusal (GHE005) — history that proves itself. |
| H2 | Schema/protocol evolution as a ritual with instruments | composio date-versioning; deepseek typed SessionEventMap | Digest catalog + byte-identical copies + GHC002 refusal + CHANGELOG discipline, exercised twice on 2026-08-19 (capturedArming ritual). |
| H3 | Guard standard above coverage | deepseek 100% coverage gate | Sabotage-observed, panic-site-named reds; vacuous-red rule; dormant-blade doctrine. Nothing surveyed has an equivalent. |
| H4 | Memory with authority tiers + provenance firewall | ai-memory `_rules/` multiplier + "never gains instruction authority" | REGRA/STATUS precedence + M07 handoff-provenance (packet carries only the agent's own writing, pointers never orders). Theirs is richer mechanically (A1/A2 adopt the deltas); ours is already enforced culturally and in the packet format. |
| H5 | Verify/fix as real stages with independent critics | oh-my-claudecode team pipeline | Factory pipeline + PRD 4.4 critical-independence + cross-review protocol with observed evidence classes. |
| H6 | Wake/attention: absence never laundered into calm | (nothing surveyed has ANY analog — none of the six model silence, missed wakes, or operator attention) | M07-M09 line: attention verdict, silence budgets, doorbell with consume receipts, wake_mis_burns. This is GraphHelm's uncontested ground and should lead the MVP story. |

## 5. What this feeds into the MVP definition (synthesis, my judgment)

1. **The moat is evidence, not orchestration.** Everything surveyed orchestrates; nothing
   surveyed can PROVE what happened (transcript ≠ execution history; coverage ≠ observed
   guards; marketing numbers ≠ cited measurements). The MVP should sell the store + fold +
   attention + evidence chain as the product, with orchestration as the commodity shell.
2. **Low-context tooling is the convergent direction** (composio meta-tools, deepseek
   capability seams): the tool-broker with search-then-bind + digest-pinned tool contracts
   (B1+B2) is the highest-leverage adapt.
3. **The memory design has a ready-made upgrade path** (A1+A2) that is two mechanics away
   from what we already practice — cheap win, big differentiation when paired with H4.
4. **Say what we refuse**: V1-V7 written into the PRD as named anti-patterns would make the
   docs read like the survey's own lesson — a system that states its refusals with reasons
   is one none of the six surveyed systems resemble.

Seeds/blueprints touched: tool-broker (B1, B2, V2), harness-manifest model_routes (C1),
operational-graph node types (C2), PRD §8/§10.3 policy clause (V1), memory product surface
(A1, A2, A5), quality-gates doc (A3, A4, V3, V5), MVP narrative (§5, H6).
