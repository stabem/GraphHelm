# M11 proposal — the four blueprints read side by side, and where they disagree

Author: B, 2026-08-20. Design-only, zero cargo. Sources read as ARTIFACTS at the paths below,
not from memory. Base for code citations: main `d0b3f04`.

| document | author | pin |
|---|---|---|
| `.factory/b-agent-107-blueprint.md` (Graph Architect) | B | 246 lines |
| `.factory/b-agent-159-blueprint.md` (customs pipeline) | B | 557 lines |
| `.factory/b-agent-process-executor-blueprint.md` | B | 293 lines |
| `.factory/d-agent-m11-synthesis.md` | D | 147 lines |

**AUTHOR-BIAS DISCLOSURE, stated first because it changes how to read this document.** Three
of the four are mine, so I am the worst-placed reader for three quarters of the corpus: I
remember what I MEANT, which is exactly the copy that ages fastest. Two compensations
applied — (1) every claim below comes from re-reading the artifact, never from recall; (2)
**agreement between two of my own documents is NOT independent evidence** and is never cited
as corroboration here. Where my three agree and D's disagrees, D's is the outside reading and
gets the benefit of the doubt until measured.

**Measured silence, which is the reason contradictions survived to now:** `#107` mentions
customs or `#159` **zero** times. The process-executor blueprint mentions `#107` or the
architect **zero** times. `#159` mentions the architect three times, all in passing. Four
documents about one milestone, written independently, and only one cross-reference among the
three pairs that matter. **Nobody had read them side by side, which is the whole reason this
consolidation exists.**

---

## PART 1 — CONTRADICTIONS (named, never harmonized in silence)

### X1 — The architect's first synthesized graph would be born with the defect M11 exists to fix. **[STRONGEST]**

- **#107** emits a graph document from a prompt; slice 1's example is a 2-node graph
  (`build_check` tool node → `summarize` agent node). Nothing in #107 emits a `completion`
  block of any kind — customs did not exist when it was written.
- **#159** decides (§2b'', ratified) that a node with **no** `completion.customs` block keeps
  today's behaviour — **which is the 0/4 measured failure**: the node parks and no verb can
  complete it. #159's G2 part 1 adds a load-time WARNING for exactly this shape.

**The collision:** every graph the architect synthesizes is born undeclared, therefore born
unable to complete honestly, therefore born emitting #159's own warning. The product's FIRST
MOVE (PRD §25 step 5, "see a customized graph get compiled") would produce artifacts carrying
the defect the milestone's other half is spending itself to close.

**Cost if unnamed:** #107's slice-1 acceptance ("executes to quiescence, status shows
completion with evidence") would be demonstrated on a graph that cannot be completed by any
operator verb — a green demo of a defective artifact, which is worse than a red one.

**Decision owed (not taken here):** either the architect emits customs declarations for
waiting-capable nodes (a scope addition to #107's slice — it currently has ten named
non-goals and this is not among them), or #107's slice deliberately synthesizes only
non-waiting graphs and SAYS so, or the slices are sequenced so the architect lands after
customs and inherits it. All three are defensible; picking silently is not.

### X2 — Two documents treat the program allowlist as opposite kinds of thing. **[STRONG]**

- **#107** builds its `CapabilityCatalog` **from** "the tool broker's program allowlist from
  the serve wiring" — the allowlist is a FACT TO READ, an input the architect consumes.
- **process-executor §2.4** says the allowlist is "a SECURITY SURFACE, not a config detail:
  it is the list of programs an execution may spawn", that it defaults to `["git", "cargo"]`,
  and that widening it silently to make a demo pass is the failure mode to avoid.

**The collision:** if the architect can only synthesize work whose programs are already
allowed, it can synthesize `git` and `cargo` work and nothing else. #107's own slice-1 example
("check that the repo builds") is cargo — **it works by luck, not by design**. The moment a
user's goal needs another program, the architect either emits a graph the runtime refuses, or
something widens the allowlist — and #2.4 says that widening must be a deliberate, reasoned
declaration, which a synthesis loop cannot make on a user's behalf.

**Cost if unnamed:** the first user goal outside git/cargo produces either an unrunnable graph
(architect looks broken) or a silently widened security surface (the exact anti-pattern).

**Decision owed:** does the architect REFUSE goals outside the catalog (honest, and consistent
with #107's refusal-is-an-answer design), or does it emit graphs whose programs the operator
must then authorize (a new approval surface nobody has designed)? #107's refusal arm already
exists and is the cheap answer, but it must be stated, because today the two documents point
opposite ways and neither knows it.

### X3 — "Evidence" carries FOUR distinct meanings across the corpus, and two of them are already in one code path. **[STRONG — vocabulary]**

| sense | where | what it means |
|---|---|---|
| E1 | `#159` `completion.customs.requires_evidence` | kinds a completion CLAIM must present |
| E2 | process-executor / `WorkOutcome::sealables` | stdout/stderr bytes sealed to the Evidence store (D-036) |
| E3 | existing `completion.requires[].evidence { type, min }` — read by `build_completion_control` at `d0b3f04` | a completion-CONTRACT predicate over the node's output |
| E4 | `#107` §3 and PRD §10.2 | "evidence requirements" as a harness output, and the acceptance-run artifacts |

**E1 and E3 now live in the SAME YAML block** — `completion.requires[].evidence` and
`completion.customs.requires_evidence` — one nested inside the other, spelled almost
identically, meaning different things. This is the `completion` collision (#170) repeating
one level down and inside the very block that produced it, before a line of customs has been
written.

**Cost if unnamed:** an author writing a graph cannot tell which evidence they are declaring;
a reviewer cannot tell which one a diff changes. And #159's own open question ("`requires`
and `requires_evidence` are close relatives, a future milestone may unify them") is asked
without knowing that `requires[].evidence` already exists between them.

**Decision owed:** rename before either ships, or state the distinction at both sites (the
#170 minimum). Renaming is cheapest now and impossible after the schema ritual.

### X4 — #107's slice-1 demonstration runs over three defects its own corpus has since named. **[MEDIUM, and it is a sequencing fact]**

#107 was written first; its slice-1 example runs a shell command through the tool path and
claims "status shows completion with evidence". The process-executor blueprint, written later,
found on that exact path: the record does not name the binary (#177), a non-zero exit is a
`RetryableFailure` so a failing stage retries (#178), stream capture keeps the HEAD and drops
the TAIL (#177), and cancel is a no-op (#180).

**Not a contradiction of intent — a contradiction of timing:** #107's acceptance is stated
against a path whose evidence quality is now known to be worse than it looked. If the
architect's demo node fails, it retries; if it produces long output, the interesting end is
discarded.

**Decision owed:** #107's acceptance either cites the #177/#178 fixes as preconditions, or
declares that its demo tolerates them and says why (a 2-node happy-path demo may genuinely not
care — but that must be written, not assumed).

### X5 — D's marked assumption about the process driver is SUPERSEDED, and the resolution is better than either document assumed. **[RESOLUTION, not a clash]**

- **D's synthesis §8** marks as an assumption: *"the process-executor driver's placement on
  the async path is an assumption — if it lands elsewhere, §3's collision belongs to whoever
  chooses"*, and §3 raises that item 2 (wall-clock ≤60%) is reachable only via the async path
  while D-039 parity demands identical CLI/HTTP behaviour.
- **process-executor §0/§1** measured that there is no new driver to place: the process
  primitive exists (`adapters/tool-host/src/process.rs`), its module doc forbids a second
  spawn path, and gate stages are already expressible as Tool-kind work reached from **both**
  drivers via `dispatch_candidates`.

**Resolution:** the process work adds no driver and no concurrency, so it neither inherits nor
worsens D's parallelism collision. D's assumption is retired — correctly marked, correctly
superseded by measurement. **D's collision itself survives untouched** and still belongs to
#153: it is about `parallel_limit`'s mechanism, not about process execution.

---

## PART 2 — WHERE THE FOUR AGREE (and the agreement is load-bearing)

Stated briefly, and with the bias caveat: three of these documents are mine, so agreement
among them is one author, not three witnesses.

1. **One entry road.** #107 (document → load → lint → publish) and #159 (spec declarations
   through the governor) both refuse to build a second path around the existing trust
   boundary; the process-executor blueprint refuses a second SPAWN path for the same reason.
   Three applications of D-039, arrived at separately.
2. **Refusal is an answer.** #107's `ArchitectRefusal` after K repairs, #159's
   `CompletionRefused` registry, process-executor's declared-gap refusal
   (`SignatureUnverifiable`, #161's custody split): all three prefer a named refusal at the
   site over a best-effort success.
3. **The envelope is the only clock.** #159's stage-entry deadline rule and the
   process-executor's replay-determinism section land on the same law from different
   directions, both citing M09's `matures_in_seconds`.
4. **D's frame orders the milestone.** All three of mine consume #153's bar rather than
   arguing with it — which is what D's synthesis §1 said the frame would do.

---

## PART 3 — THE M11 SHAPE THE FOUR IMPLY TOGETHER

Once X1–X4 are decided, the corpus describes one milestone with three tracks and one
acceptance:

- **Track A (acting):** #159's lanes #160–#163 — the event family and fold, clearance,
  DLQ/sweep, status scan-history. Closes the measured 0/4.
- **Track B (executing):** #177/#178/#180 — the record names what ran, a failing stage is a
  verdict, a cancel actually stops. Closes #153's items 3 and 4, which are otherwise
  unanswerable.
- **Track C (compiling):** #107's slice — prompt → graph → executed. **Depends on X1's
  decision**, and on nothing else in Tracks A/B except the honesty of the path it demos.
- **Acceptance:** #153's gate-as-graph, which consumes Track B for its stages and (per the
  process-executor's §6, now the seventh addendum on #153) **Track A for its terminal verdict
  node** — the human sign-off is outside-fact work and therefore customs.

**The sequencing consequence nobody wrote down before this reading:** Track C is the only one
that can produce a defective artifact by succeeding (X1). A and B fail loudly when incomplete;
C ships a graph that looks synthesized and cannot finish. That asymmetry is an argument for
deciding X1 before C starts, not for delaying C.

---

## PART 4 — WHAT THIS DOCUMENT DOES NOT DECIDE

Every X above names its decision as owed, not taken — the packaging call, the slice ordering,
and any renaming (X3) belong to the orchestrator and the owner. Nothing here re-opens a sealed
bar: #153's kill bar and #159's acceptance cells are consumed as given. And H's `#110`
registry blueprint was NOT part of this corpus — it is adjacent (the architect's future
Matcher consumes its projection, seam recorded in `#159` §5b) and should be read against
Track C when Track C's scope is decided.
