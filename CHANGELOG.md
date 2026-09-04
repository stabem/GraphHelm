# Specification Changelog

## A registered gate runs the gate it was registered as, #668 - 2026-09-03

- **Certification said yes and the consumer path could only say no.** `quality certify` stamped
  `GateCertified` for `gate-retry-lineage` and `gate-journey-contract`, and a node using either
  was refused as uncertified: the drive carried ONE suite digest (geometry's) and the driver
  compared every gate's receipt against it, while the executor evaluated every dispatched gate
  with geometry's evaluator. Two production sites, one consequence - the registry could admit a
  gate that no running graph could reach.
- **The runtime now asks a registry, per gate id.** `GateRegistryPort` (`core/runtime/src/ports.rs`)
  answers both questions from one object: the digest of THAT gate's own suite, and that gate's own
  evaluation of the node's evidence. The binary supplies it (`RegisteredGates` in
  `apps/cli/src/commands/quality.rs`), so `core` still names no pathogen suite and no gate's
  evaluator.
- **A gate node carries the evidence its gate judges.** `GateCheckWork` no longer names geometry's
  three fields; it carries `gateId` plus the contract's remaining keys, and the registered
  evaluator parses what it needs. Geometry's `deny_unknown_fields` strictness moved WITH it, into
  the geometry evaluator.
- **Evidence a gate cannot READ refuses the node; it does not verdict it.** The registry answers
  either a verdict or `Unreadable`, and `Unreadable` becomes the same `Unassemblable` refusal an
  unparseable contract always produced: the node is never dispatched and NOTHING is appended. The
  distinction is the append-only store's, not a taste in error shapes -- a `GateVerdict` is
  permanent evidence that a delivered surface was examined and refused, so emitting one for a
  misspelled `gate.check` block would leave a High-severity claim about a surface nothing looked
  at, and fixing the typo could not retract it. A gate that PARSES the node's evidence and rejects
  it for being the wrong kind (the journey-contract gate's `GHJPD000_WRONG_EVIDENCE_KIND`) is a
  verdict: it looked. (Found by L reviewing the first version of this change, which returned
  findings for both.)
- **Fail-closed did not move.** No registry, no entry for the named gate, or a gate this build can
  no longer certify all refuse to dispatch exactly as an absent digest did. A gate id the registry
  does not hold produces no verdict at all rather than another gate's opinion under its name.
- **`core/runtime` no longer depends on `core/quality`.** Geometry became one registered evaluator
  among three, wired by the binary; the dependency survives only as a dev-dependency of the gate
  cells that choose it.

## Local Studio MVP and the execution index, #105 - 2026-08-27

- **The Studio stopped being a specification with no code behind it.** `apps/studio` is a
  local-first operator surface: it lists the runs a store holds, answers which one needs the
  operator and why, shows the sixteen node states and the append-only timeline, and performs
  three mutations - pause, approve a node, resume. It is NOT the Studio `docs/ux/STUDIO_SPEC.md`
  specifies: no graph canvas, no DSL editor, no chat, no collaboration, no cloud.
  `docs/ux/STUDIO_MVP.md` records the line between the two.
- **D-040 is unchanged.** The monitor stays local, read-only, and simple; it gained no
  operational action. The Studio is a separate application, and it may use only public Runtime
  contracts - it imports no crate and reads no event file.
- **The board draws real edges, and only ones it can prove.** `POST /v1/graph/topology` reads a
  graph file on the Runtime host and returns its entrypoints, nodes and edges together with the
  `semanticHash` that says WHICH graph it is. The Studio compares that hash against the one the
  run itself recorded in `execution_started` and draws on a match and only on a match: mismatched
  or unverified yields an EMPTY edge list by construction, so a component that forgets to branch
  cannot render the wrong graph. Sabotaging that single expression turns two guards red. It grants
  no reach the API lacked - `start` and `resume` already load a file by path - and it returns node
  identity only, never objectives or agent blocks. Reachable from all three surfaces.
- **`GET /v1/executions` exists because scraping `/monitor` was the alternative.** A presentation
  surface for a human browser was the only way to discover which executions a store held. The new
  index is authenticated like every other `/v1` route, ordered by execution id, bounded at 100
  rows, and paged by an EXCLUSIVE cursor; an over-large limit is refused rather than clamped,
  because a caller that asked for 500 and silently received 100 cannot tell a clamp from a short
  store. Each row is a key subset of the same `render` value `execution status` replies with, so
  the index cannot disagree with the detail view - checked field by field on a live store, not
  asserted in prose. Reachable from all three surfaces: `graphhelm execution list`, the route, and
  the MCP tool `list`.
- **Seven WebMCP site tools over the SAME client the buttons use.** The adapter builds no request,
  chooses no actor, and interprets no diagnostic; it calls `RuntimeClient`. `cancel` is
  deliberately absent - it is the destructive verb, and the journey does not need it.
- **No write reports success it has not verified.** Every mutation reads the head, mutates with
  `If-Match`, then reads the status and the appended events back, and answers `succeeded`,
  `refused`, or `unknown`. The third value is the point: reporting an unverifiable write as
  succeeded makes the one state an operator must act on look exactly like the one they can ignore.
- **A browser confirmation is consent, not authorship.** A mutation an agent chose is recorded as
  actor type `agent`, id `studio-webmcp-adapter`, even though the person approved it in the
  browser. Recording it as the owner would destroy the only distinction the audit log exists for.
- Declared gap: the Studio's own suite is not part of `ci/gate.ps1`, which covers the Rust
  workspace, schemas, and PostgreSQL. Running it is an explicit step, named in
  `apps/studio/README.md`, rather than a risky pipeline change made to show a green check.

## Name the state you were true of, M10 — 2026-08-20

- The milestone's product sentence, repeated eighteen times: **make the answer name which world it is in.** `GHCLI016` answered a setup refusal and a mid-drive failure with one value, so "the call failed" could not tell an operator whether their hold survived (#96); a restore step that ran out of time reported itself as a corrupt archive, with elapsed scattered across four variants and three codes, none of which named timing (#81); `wake-wait`'s timeout answered from a snapshot the store already contradicted (#88). Each fix is the same shape — the answer now names its world.
- The operator's verbs stopped disagreeing with themselves. A refused `resume` no longer commits `ExecutionResumed` before the drive's setup can fail, so a 500 and an intact hold stop being two facts where the operator reads one (#83). A `resume` names what it held and the drive releases each node when its edges allow, ending the round-forever loop where `pause` re-held on bare state and `resume` force-started (#123); the release is recorded under the OWNER's actor, because releasing work the owner paused is the owner's act (D-019), which partially reverses an earlier deliberate decision and says so at the site. MCP `start`/`resume` gained a deployer-level `--project` default, so an operator confined to that surface can resume without knowing a workspace path (#82).
- Dispatch waits for its edges (#80), and the flagship story that used to credit `deploy.userOverrideAllowed` was rewritten around the recorded path that exists — that field has no execution-lane consumer anywhere. Attention was deliberately NOT made edge-aware (deferred, with in-code signposts at the two sites whose change would make the deferred population non-empty).
- The gate stopped discarding its own evidence: a failing tool's stdout was swallowed at every call site, and the per-suite pass ran off a hardcoded list that had drifted from the filesystem (#97, #98). The reported exit-code defect was investigated and REFUTED — it is what piping the run through `tail` does to an exit status — and saying so is the finding; the lane's real yield was the swallowed-output defect nobody had filed.
- Instruments gained edges they lacked. `ServerGuard` now drains the spawned server's stderr, so a server panic and a slow server stop producing identical client evidence (#140) — an instrument boundary, not a fix: it does not say which storm hypothesis is true, it makes one visible for the first time. The wake belt's headline moved from an aggregate a dead recorder satisfies to a per-arming identity assertion read from the raw journal (#118).
- Performance work landed and immediately taught its own lesson: the verified prefix stopped re-proving history on every request (#87), then readers serialized again until clean opens shared the lock (#143, #146). **A performance change that had been measured, and measured well, still broke a property no measurement was watching** — the rule that measurement never substitutes for the gate arrives with its receipt attached.
- One terminality predicate and one parallelism policy, not five (#101), with the dedup re-verified AFTER the merge because eight commits had landed in between: "there is exactly one" is a claim about a tree, and the tree moved.
- The storm regression the owner saw was attributed by interleaved comparison within one session and fixed (#123). Its MECHANISM stays OPEN with four candidates named side by side, including dead-server, which the instrument was structurally unable to see for the whole milestone; the rate is recorded INDETERMINATE, declared rather than restored; and a storm rate is a property of commit × session, which voids any standalone characterization that does not name its session.
- The milestone found the same defect in its own records nine times — a record that was true when written, read later by someone who cannot see what moved — and recorded the two cases that cost nothing beside the seven that cost something. `docs/milestones/name-the-state.md` carries the account, the defect classes, the traps, the deferrals with their four distinct reasons, and what is NOT claimed.

## Arming the alarm, M09 — 2026-08-19

- Seed 3 from `m09-seeds.md` closed on `main` (#70, `efd85d0`): silence is keyed on whether a
  node has been REACHED, not on `state == Running` — a node retried into `Queued` after a
  declared bound now counts. `Invalidated` (a completed node returned to the queue with no
  failure anywhere in its history) was the case a failure-keyed rule would have left mute
  forever; found by reading the outcome vocabulary, not by the failing run alone.
- Seed 4 ("the doorbell cannot ring for silence"): arming declares how long quiet may last; a
  bound nobody could live to see is refused rather than answered with a date (a trillion-second
  horizon overflowed into a year-33715 date before this fix); `wake-wait` reads its own lease's
  deadline instead of a caller-supplied `--timeout`, closing the two-numbers-one-question gap on
  the sleep surface; the MCP half of `wake_wait` now matches the CLI half (one definition, not
  two); shortening a re-armed horizon is accepted and named, never silently late; reads take a
  shared lock instead of the same exclusive lock as writes (measured: eight concurrent reads
  cost the same wall time as eight sequential ones, before the change).
- Three flakes named at milestone open; two fixed and landed on `main`, one still open at
  close. `concurrent_sweeps_never_double_consume_a_lease`'s window (a validate/`next_sequence`
  gap) closed by pinning the consume's sequence from the read that judged the lease, not a
  second store read — the seam survives the fix and stays sabotage-testable, unlike the
  alternative design considered and rejected. The belt test sharing this guard's name was
  separately measured hollow: green with 14 of 15 consumptions missing, by construction — its
  own oracle upgrade is tracked as #74. `a_sleeper_wakes_on_a_peer_append_with_zero_requests_
  in_the_window` fixed by a condition-wait replacing a timing-dependent immediate read (#73);
  base rate 9/10 isolated, 3/3 in-suite before the fix. `the_storm_holds_under_eight_
  concurrent_agents` stays OPEN: a same-disk paired re-baseline (ten runs at the pre-M09
  commit, ten at the current tip, three minutes apart, identical free disk on every row) found
  ZERO failures at both — the same code produced 4/10 failures and 0/10 failures on the SAME
  machine at two different disk states, which is this lane's central finding, not a caveat:
  code is EXONERATED, but any before/after comparison that is not a fresh paired baseline in
  the same session and disk state reproduces today's confound while looking clean (conditions
  recorded with the numbers so a later pairing is checkable: free disk ~19G, fsync 1.5-2.0ms/op,
  commit `ef51193`, 2026-08-19, n=1521 store opens). An instrumented headroom measurement, on
  today's disk, found ≈8x headroom on the storm's 8-request convoy at the median (29.1-32.6ms
  per store-open x 2.40-2.56 opens per request x 8 requests = 0.56-0.64s against the 5s budget)
  but only ≈1.8x at the p99 (the SAME 8-deep convoy at p99 per-open latency totals 2.81s) and
  CROSSES the budget by 4% at the observed maximum — a non-firing median with a
  tail this close to the budget is a live finding, not a clearance. The composition bound
  narrows it further: reaching the budget needs a burst-average around 250ms, which requires
  broad degradation (roughly 20% of opens slowed to ~1.1s) — a few slow outliers cannot get
  there. Verdict: CONSISTENT WITH A GENUINELY SICK VOLUME, INCONSISTENT WITH MILD PRESSURE.
  Quantified for M10: each store open costs ~30ms of structural work serializing on an
  exclusive lock regardless of handler threading, at ~2.5 opens per request — removing ONE open
  saves ~30ms per request and ~244ms off the 8-deep convoy (an 8x amplification, because the
  convoy is where an open's cost is spent). Fewer opens, not more threads. No fix lands with
  M09; all instrumentation used to measure this was reverted before commit.
- A consumption now names the arming it burns (#74, merged with one Postgres-adapter stage RED
  under an explicit owner decision to land on the evidence rather than re-roll — see the
  milestone record): the rendezvous-equal-burn discriminator closes the #55 family's remaining
  silent-loss window going forward. The fold-side check stays forward-only by design (inventing
  a mismatch from a field absent in pre-fix history would make all committed history look
  defective); whether the defect ever fired in already-committed history is permanently
  unanswerable, because only the live side of that comparison was ever written down.
- The second judge story (M08's own coverage gap: seven of fourteen MCP tools never touched
  across nine M08 runs) ran paid and FAILED (`passed: false`, 8 findings, 2 critical). The
  coverage goal was MET — all seven tools fired, verified against the audit middleware's own
  route-registration order, which records a refused request too. The redesign's own forcing
  mechanism (`MAX_IDENTICAL_OUTCOMES`) fired correctly in the real run and the judge triaged the
  resulting incident correctly; the release did not ship, on real MCP-surface defects the
  story's own design surfaced rather than a story-design flaw — a `resume` that refuses on a
  workspace/staging collision with no MCP parameter able to satisfy it (#82), and that SAME
  refused `resume` still committing state and dropping the operator's pause hold before
  reporting failure (#83). Archived as `docs/acceptance/m09-judge-run-2026-08-19/`. Method
  lesson: rehearse on the EXACT surface the real actor uses — the free rehearsal supplied a
  `project` parameter on every `resume` call that the real MCP tool schema never exposes at
  all, so the rehearsal proved the state machine's mechanics thoroughly and could not have
  caught #82/#83 by construction — a gap in which LAYER was rehearsed, not in how carefully.
- A second method lesson, from the storm lane's own falsifier discipline grading its own
  author: a pre-registered no-referral prediction held 10 out of 10, but it was derived from a
  headroom model wrong by roughly an order of magnitude (40-100x predicted, ≈8x measured at the
  median) — right only because both values landed on the same side of the trigger, a
  near-boundary result would have flipped it. Recorded as RIGHT-FOR-WRONG-REASON, not a
  successful forecast, on the predictor's own principle: a number that is right for a reason its
  author does not have is not a measurement. The weight-bearing findings of that lane are the
  measurements themselves, not the prediction that happened to survive them.
- Three more fixes landed on `main` this milestone, main-based rather than part of the M09
  branch itself: cited evidence must open, not just hash (#75/#77) — two committed acceptance
  stores had answered `GHE005_INTEGRITY_FAILURE` on open for their entire committed life while
  every checksum stayed green (`SHA256SUMS` hashes files; two EMPTY DIRECTORIES have no file to
  hash), closed by a test that opens and replays every committed store by directory rather than
  by binding. Store-layout recovery (#76/#84) — `.tmp/`/`active/` are recoverable on open
  (transient workspace, nothing the journal does not already carry), `blobs/` stays strict (a
  blob is a tracked file; its absence means evidence is actually gone). A CLI that can print a
  schema digest (#78/#85) — a ritual that previously needed a throwaway test and a manual run to
  get the number a schema change requires now has one canonical command for it.
- PENDING, not a close blocker: a status-code question on `resume`'s failure surface (D) and
  the prediction ledger's final scoring pass (M) remain open rows, owners named, carried into
  M10 rather than resolved here.

## Ask Once and Sleep, M08 — 2026-08-18 (backfilled 2026-08-19, during M09's close — this
section was omitted at M08's own ship time; written now, dated then)

- `node_silence_seconds` is the single place elapsed time is computed; the monitor's private
  `last_event_per_node` and its three staleness thresholds are deleted, not deprecated. Silence
  is judged from an INSTANT the surface injects, never a duration a surface computes.
- §8 clause seven: the product now promises no surface recalculates the attention verdict — an
  owner decision, taken after the measurement that would otherwise have made recalculation the
  honest description of the state.
- `wake_wait`, the thirteenth MCP tool: bounded in the schema, content-free by construction,
  refusing any rendezvous the calling session does not hold.
- Task 3 ("the serve cannot serve while it drives") was going to be a fix; measurement refuted
  the premise instead — with a drive parked in a 90-second model call, `/health` answered in
  0.00s, `GET status` in 0.03s, and `POST wake-lease` was ACCEPTED in 0.08s with a live lease.
  Four of five causes proposed for the M07 alarm failure died to measurement; the fifth stands
  recorded as an unadopted hypothesis. The task became a correction of the record and was
  deleted rather than reworded, in both the milestone record and this milestone's own plan.
- One defect shape — an assertion reading one level above what it actually measures — appeared
  eleven times in one milestone, including once inside the very guard written to close a
  previous instance of it. The rule it produced, binding since: assert at the finest grain the
  question has.
- Closed after nine blind-judge runs, on the rule that findings close when NAMED and WITHDRAWN
  — never when the judge simply approves, which the M07 record established he does not. The
  finding carrying F1's number survived and changed KIND, from a claim that the mechanism fails
  to an argument about a default; it opened M09 rather than closing here.

## The one-glance answer, M07 — 2026-08-17

- Scope was the blind judge's four M06 findings and nothing else; the closing rule was that the same judge, on the same story, had to stop making them. Three real subscription runs were needed (`docs/acceptance/m07-run-2026-08-17/` — transcripts, not stores: the committed `journal.jsonl` has no `format.json` and no `blobs/`, so it cannot be opened or replayed, and the evidence its batches reference was never committed): 8 findings with one critical, then 7 with two criticals, then 7 with none — the last crediting a fix in its own words ("wake_status does correctly separate contentHead (12) from head (14)").
- F1: the sleep question is decided ONCE. `graphhelm_execution::attention` is the single pure predicate; `render` and the monitor both call it, and the monitor's private copy (the third in the codebase) is gone. `attentionRequired` is derived from its reasons, never declared beside them. The monitor states the verdict in words.
- F2: all sixteen lifecycle states are emitted as zero-filled buckets, reversing a documented guarantee — the assert that pinned the omission was inverted with its reasoning rewritten, not deleted.
- F3: `NodeOutcomeRecorded` carries an optional `reason` from a closed vocabulary (the fourteen route classes plus empty reply, malformed judgment, judge/gate refusals, four tool dispositions, fixture-scripted). Closed by design: the class rides the event, the text seals to Evidence (D-036). The gateway-error arm — which knew the most and sealed the least — now seals too. `reason` is OMITTED when absent because replay re-serializes each envelope and recomputes its hash: an always-emitted null breaks the chain of every pre-M07 event, proven by sabotage (a committed store stopped opening).
- F4: the fold keeps the last consumption per session, recorded from the consumption event; `wake_status` returns live/cursor/head/contentHead/lastConsumed. `contentHead` exists because the doorbell rings on content only, and publishing the raw head alone made a lost ring look plausible to the judge.
- The closing rule caught two defects both agents had shipped: the wedge arm was dead code in production (a real execution never emits `simulation_started`, so its status is null throughout and the arm demanded `Some(Running)`), and `status` itself was null on every read of a live run. A started execution with no recorded status now reports `running` on every surface.
- Honest limits recorded in `docs/milestones/one-glance.md`, including the four M08 seeds the judge raised: no liveness/time data in the glance, retry flapping invisible, no blocking wait on the MCP surface, and `wake_last_consumed` growing without bound.

## Quality gates, M06 — 2026-08-17

- The verdict vocabulary (kinds 29/30): `GateVerdict` is refusal-with-findings by construction (the envelope schema refuses a bare fail on the wire; the judge parser refuses it in-process); `GateCertified` is the thymus receipt as replayable state — `gate_certifications[gate_id] = suite_digest` in the fold, compared against the CURRENT suite digest so growing the pathogen suite voids old immunity by comparison.
- The thymus: ten bred pathogens (one per uselessness mode, paired per specimen with the plausible gate each fools); `certify()` refuses on any pass naming who was fooled; the correctness battery itself fails all ten (correctness alone certifies nothing) and `reject_everything` certifies (necessary, not sufficient).
- The deterministic evaluators (`core/quality`, pure): spec-derived content manifest + layout grammar over stripped HTML (style bodies survive — geometry declaration, not prose), praise-stuffing sentinel pinning identical findings; the COMPOSED evaluator certified, the layout grammar alone REFUSED — geometry never gates by itself, as a test.
- Gate nodes execute certified-or-not-at-all: `NodeWorkKind::GateCheck` (no model port, panicking-port-proven), one refusal arm died, fifteen node types byte-identical to 05d; failing verdicts are `TerminalFailure` with findings sealed beside the verdict event.
- The blind judge: no new work kind — cognitive transport with blindness as input discipline (type diet + assembler signature + source fence, each test-pinned); refusal-with-findings with stepsOverPar/stallPoints; fence/prose-tolerant parsing learned from the live run without loosening the contract.
- Demonstrations as the acceptance map's third binding: recorded journeys replayed against the current build, seed frozen at recording from injected entropy; the artifact verifier gained tracked-vs-named (the 05f journal lesson as machinery).
- The dogfood run (2026-08-17, committed with checksums): uncertified refusal live → `graphhelm quality certify` (GHCLI018 debut, closed registry) → certified geometry pass → the blind judge on the owner subscription probed the live system via MCP and REFUSED the one-glance story with four findings — the recorded M07 backlog seed. The gate-freeze rule ships as a pure check (gate machinery and gated code never move in one PR).
- `gate_http` is the twelfth gate suite (22 stages). Hotfix #55/#56 (wake sweep double-consume) landed mid-milestone from the pair loop's own dogfooding.

## Wake doorbell, 05g — 2026-08-16

- The wake primitive: a session sleeps at zero cost and is woken by another actor's append — one content-free byte, no payload, no polling anywhere (D-036/D-037; the 05f-era divergent pass's attacker traps are binding refused scope). `WakeLease`/`WakeLeaseConsumed` kinds with fold-pinned invariants: one live lease per session (arming replaces — anti fork-bomb), consumption burns, consuming unarmed corrupts replay, and a replay never rings (the fold speaks no transport, source-scanned).
- The serve-side ring fires only AFTER the trigger append is durable — the test's sleeper snapshots the store at the instant the byte arrives (the first sabotage exposed a blind detector; it was hardened before the guard was trusted). Two-phase consumption records the TRUE reason (rung / stale_rendezvous); only non-wake appends ring; a burned lease never rings twice; ring failure never fails the route.
- The sleeper-only surface: MCP tools `wake_arm`/`wake_status` (twelve exactly — no ring tool exists, the thirteenth-tool sabotage failed the closed list; the session can only arm ITSELF, its identity injected inside the dispatch, never an argument) over `POST/GET /v1/executions/{id}/wake-lease`, one path never two. The rendezvous derives from an OPAQUE id under a fixed local prefix — a hostile lease points nowhere.
- `graphhelm wake-wait`: the sidecar blocks for free and exits by code (0 rung / 3 timeout-as-routine / 2 GHCLI017); hostile ring bytes die in its sink — content never crosses, sentinel-proven.
- §5 measured, not promised: two real sessions through a counting TCP proxy — ZERO connections from the sleeper in the arm→ring window; degradation pinned (dead serve → routine timeout → a plain read still true: slow, never wrong).
- `wake_http` joined the gate (eleventh CLI suite, red-proven). Honest limits recorded: the Unix arm compile-shaped, CLI-direct appends ring nothing (dead-man covers), spurious wakes possible across a crash (content-free, so slow never wrong), the driver does not sleep on leases yet, and the 05f gitignore'd-evidence lesson with its named hardening candidate.

## Monitor and Milestone 05 close, 05f — 2026-08-16

- The read-only monitor (D-040): `GET /monitor[/{id}]` on serve, server-side-rendered ZERO-JavaScript HTML over the same `ExecutionProjection` the status command folds — the medium enforces the refusal (no script for a button to hook into), GET-only structurally (405 to every mutating verb, CSP on every 200), cookie bootstrap through the one shared constant-time verifier (token never in a Location or a page byte), meta-refresh with a `since` cursor making the delta strip stateless.
- Silence as signal and remediation as text: per-node staleness clocks from event gaps (per-kind bounds when the stream carries a graph, honest "unknown" when it does not), blast radius via pure reachability, and the EXACT approve command beside each triaged node — rendered from the clap definition itself and parse-round-trip-tested so page and CLI cannot drift.
- The negative proof: every monitor route hammered with every verb, store bytes fingerprinted bit-identical; `execution status --html` writes the same renderer frozen (byte-equal minus exactly the refresh line) as the incident artifact.
- The Milestone 05 close: `m05-clauses.toml` binds each §8 clause to named provers with assert fingerprints; `acceptance_map_is_grounded` (gate-listed) verifies fn existence, gate membership, fingerprints, D-citations, the generated map's bytes, and the committed run evidence's checksums in both directions. The real-work run happened once on the owner-subscription route (native_runtime, claude CLI) — completed, replayed byte-identically, identical state via CLI/HTTP/MCP — and is never re-run, only re-hashed.
- `monitor_http` joined the gate (tenth CLI suite, red-proven). Honest limits recorded: staleness vs event granularity, graph-publication absence on CLI/serve streams, the cookie as a named second door, the 2s refresh as the whole update contract, remediation without If-Match, the native adapter's cwd sensitivity, 401-as-needs_capacity inherited from 05b, the double-duty keyring biting once, and the single-store monitor index.

## Chat surface, 05e — 2026-08-16

- `graphhelm mcp` added: a stateless stdio MCP server whose tools map 1:1 onto Public Runtime API requests (D-039). Hand-rolled minimal JSON-RPC 2.0 per ADR-026 — the declined rmcp footprint measured and recorded (328→342 packages, 14 crates) — with a bounded 1 MiB line reader, protocol revision pinned to 2025-06-18 (the handshake model the targeted hosts speak; the meta-versioned 2026-07-28 spec noted as a revisit candidate), and a conformance suite pinning id echo, notification silence, and oversized-line resync.
- The ten tools: start, status, events, signal, approve, pause, resume, cancel, routes, probe — a closed list with closed schemas, guard-tested (no credential tool is representable; omission is the enforcement). Every mutation carries the optional `ifMatch` head pin; idempotency keys follow the logical act (`mcp-{nonce}-{s|n}{id}`, type-marked, digested past 32 chars) with retry-reuse and 409 divergence behaviorally proven; a notification-form tools/call never executes.
- Config fail-closed before any protocol byte: loopback-only URL under the post-#36 userinfo rule, token via file or env never argv (sentinel-scanned across a real transport attempt), GHCLI015_MCP_INVALID as reserved.
- The serve layer grew `GET /v1/gateway/routes|probe` calling the same command-layer functions as the CLI (parity test-pinned, explicit 401 asserts, server manifest preferred with query override) so the MCP tools never become a second path.
- Parity and choreography as tests: the 05a story via MCP equals direct HTTP with an empty exception list; two chat sessions coordinate through events alone and resolve an If-Match race with one re-read retry.
- Packaging thin and deletable: Claude Code plugin (.mcp.json with --token-file, skills operate-execution/observe-agents with `tool:`-marked choreography) and the Codex snippet, all validated in the suite; both READMEs carry §7's deletability sentence.
- `mcp_stdio` joined the gate as the ninth CLI suite, red-proven.
- Honest limits recorded: eight skills deferred each with its dependency named, pull-only notifications, tools-only MCP surface, the narrow secret-prefix heuristic, the aging protocol pin, probe's inherited spawn surface, verbatim gateway-read queries, and the shared stdout.

## Runtime, 05d — 2026-08-16

- `core/runtime` added: the `AsyncNodeExecutor` seam, dependency-inverting `ModelPort`/`ToolPort` (adapter crates implement them in `apps/cli`'s wiring — the arrow is pinned from both sides by source invariants), deterministic prompt assembly from the node contract, and the closed cognitive/tool classification with typed refusals the driver never dispatches.
- Evidence-before-append generalized to node work: seal first, then one atomic append naming exactly the sealed references; a sealing failure appends nothing (sabotage-proven). The 05c `ReuseDecision` obligation has its first producer — the ledger entry rides the same `PreparedAppend` as its outcome. Signal envelopes seal beside the operator copy on both the CLI (mandatory keyring) and the configured HTTP path.
- The M04 ledger closed: attempt-fair deterministic dispatch, edge-aware readiness for the decidable subset (literal-false conditions ungate; Failure edges release on Failed and only Failed, both deltas property-pinned), and resume cross-checking the supplied file's hash against the recorded graph before any recovery append — with the empirical discovery that the CLI path's record is the `execution_started` hash, not the M03 publication field.
- `drive_to_quiescence_async`: the 04f sequencing event-for-event, concurrent work up to the budget bound, every write serialized through the single writer inside `spawn_blocking`, and §12 immediate stop composed from the 04e pieces — aborted futures, real children killed and proven dead, `Interrupted → Blocked`, resume gated on triage.
- The API drives async while the CLI stays byte-identical (`execute_prepared` split; the 05a parity test unchanged through the swap). Serve gains grouped runtime flags, `pause` `{"mode":"immediate"}`, and the milestone's §8 acceptance sentence is one named green test: agent (sealed reply) + tool (sealed record and streams) to completion over HTTP with byte-identical double replay. `runtime_http` joined the gate, red-proven.
- Honest limits recorded: the viability gate's mixed-graph quiescence-without-completion case, the double-duty serve keyring, `ReuseDecision` silent on the serve path pending a host accessor, HTTP calls not abortable mid-flight, prompt assembly without a Context Compiler, one route, empty `artifact_refs`, and throughput recorded in its own unit (≈0.17 full stories/second vs 05a's ≈3 raw requests/second — not comparable, both kept as baselines).

## Runtime, 05c — 2026-08-16

- `core/tool-broker` (pure) and `adapters/tool-host` (impure) added: the closed repository/shell/tests call vocabulary with per-action effects and capabilities, the effect→tier rule with `SecretUse` structurally refused, lexical path/program rules, the deny-by-default capability lease, and the pure `authorize` pipeline in pinned order — decisions in the pure crate, enforcement in the host, purity pinned by a source-invariant scan over all six sources.
- Tier 1 executions run inside an ephemeral detached `git worktree` provisioned with hooks disabled and removed under retry/backoff + prune; the process primitive is argv-only with `env_clear()` plus a six-name allowlist, redirected homes, a synthetic git identity, and an `extra_env` deny-list covering every host-defined name. Provision runs git under the SAME scrubbed config posture as execution — config consistency proved to be the correctness condition when a user-level `autocrlf` smudge made `git apply --index` refuse everything.
- The register's hard constraint is one named green test: `credentials_are_demonstrably_absent_from_the_tier_1_workspace` — sentinels in the parent environment and a protected keyring, real invokes including a write, streams/workspace-scan/separation/record asserts, sabotage-proven in both directions.
- The 05c amendment landed: `FreshnessClass` in `core/protocols`, a clean-tree-gated snapshot-keyed `ReadCache` (Tier 0 + `SnapshotClosed` only; erasure invalidation deletes bytes), and `ReuseDecision` as the 26th event kind through the full D-037 ritual — identity-only payload (`node_id`, `key_digest`, closed enums), an explicit ledger-not-state fold arm, and no producer until the 05d executor.
- `graphhelm tool invoke` added (GHCLI012–014, mandatory `--capture-out`, digest-only record in the envelope) and `tool_cli` joined the gate as a named stage, proven able to go red.
- Honest limits recorded: Tier 1 is worktree+scrub not a container; the Policy Engine step is fixed rules; redaction is caps + structural env emptiness; leases have no lifecycle; records are not yet Evidence; `apply_patch`/`commit` never compose across calls (the 05d `ToolPort` reconciliation names the composition decision); the read cache persists under staging by design.

## Runtime, 05b — 2026-08-14

- `core/gateway` (pure) and `adapters/model-gateway` (impure) added: the Universal Model Gateway's route manifest, error taxonomy, capacity policy, credential broker, and BYOK/native-runtime adapters, with purity enforced by a source-invariant test pinning `core/gateway`'s dependency table to exactly `graphhelm-protocols`/`serde`/`serde_json` and forbidding it from ever naming the adapter crate.
- `RouteManifest::from_json` makes billing mode and authentication a single structural fact tied to transport (§20): a `direct_api` route requires `api_key`/`per_token`/`baseUrl`/`model`/`credentialRef` and a provider in `{anthropic, openai}`; a `native_runtime` route requires `account_subscription`/`subscription_quota`/`runtime`/`command` and structurally forbids `credentialRef`/`baseUrl`, so the manifest cannot route a broker secret into a runtime that owns its own auth. Cleartext `http://` is refused except to loopback.
- The fourteen-kind `GatewayError` taxonomy (§17) maps to `NodeOutcome` through one exhaustive match with no wildcard arm: quota/rate/auth failures park the node as `NeedsCapacity` (§12, no automatic paid fallback), provider/timeout/crash/malformed trouble retries, a request the gateway can never satisfy is terminal, cancellation passes through.
- The credential broker invents no new cryptography — it wraps the existing `EvidenceProtector<SealedKeyProvider>`, persisting sealed parts atomically, enforcing `usable_by` route scoping and durable revocation at `lease()`, and never caching plaintext.
- ADR-025 pins `ureq =3.4.0` for the BYOK adapters' outbound HTTPS, rustls-only, confirmed to share `sqlx`'s existing `rustls`/`ring` versions rather than adding a second TLS stack. Anthropic and OpenAI adapters map fixed status tables onto the taxonomy; native-runtime adapters spawn Claude Code/Codex under `env_clear()` plus a fixed allowlist and a stdin-only prompt — the isolation proved load-bearing when a sabotage run leaked a live `SENTRY_AUTH_TOKEN` into a fake child before the guard was restored.
- `graphhelm gateway routes|probe|credential set|remove` added, with a quota-free probe (§18) and `gateway_cli` as a new named gate stage.
- Honest limits recorded: JSON manifests where the spec shows YAML, router scoring and the broker's access audit deferred, three of the five route types deferred, session management and gateway-native tool calls out of scope, host-CLI JSON shapes are fixtures pending 05e's live re-verification, and the quota-detection marker list is a documented heuristic rather than a wire contract.

## Runtime, 05a — 2026-08-14

- `graphhelm serve` added: the Public Runtime API as the multi-agent concurrency contract. Loopback-only fail-closed bind, sibling-path bearer token with constant-time comparison, the CLI's four-key envelope on every route.
- Endpoints one-to-one with the seven execution commands plus status (now carrying `headSequence`) and a paged events tail that refuses rather than truncates. A fresh mutation's own reply now carries `headSequence` too (a serve-layer enrichment; CLI mutation output is unchanged), closing the extra-GET gap on `If-Match`-chained writes.
- Every mutation attributed from headers (`owner`/`agent`; `system` reserved for the driver's hops) and idempotent under full retry: decision-event keys derive from the caller's command key plus a fixed suffix plus a content digest, and a pre-flight classifies them Absent/Complete/Partial/**Divergent** against the stream history it already has in hand. **Fixed in final review**: the pre-flight was content-blind — key presence alone, no check that a committed key's event actually carried the same request — so reusing an `Idempotency-Key` across two different bodies got silently absorbed as a completed retry of the first, its real effect never applied. The digest closes that: a byte-identical retry still re-derives the same key (Complete, unchanged), a divergent reuse now derives a same-prefix-different-digest key and is refused 409 before the store is touched. The header is capped at 64 characters to keep the longer derived key within `OpaqueId`'s limit. `If-Match` gives optimistic concurrency with the current head in every 409.
- The eight-agent storm holds across consecutive runs — no 500s, coherent replay, byte-identical double replay, full attribution — and its sabotage was caught synchronously by the fold rejecting the incoherent history inside the causing request.
- CLI-API parity pinned with an empty exception list; `api_http` is a named gate stage proven able to fail. axum `=0.8.9` recorded as ADR-024.
- Honest limits recorded: no documentation surface yet (Living Docs arrives into this same API), ~3 req/s on the current-thread runtime as the baseline 05d must beat, unknown executions read as empty, mTLS deferred.

## Graph Engine and Governor, 04f — Milestone 04 complete — 2026-08-13

- The driver ships in `apps/cli`: drive-to-quiescence over every pure piece, with `Queued` nodes unioned into the dispatch candidates so retries redispatch, every `next_state` from `apply_transition`, every append through the production store.
- JSON-only `execution start|status|signal|approve|pause|resume|cancel` with redaction-safe codes. Evidence is written before any event that references it; an unrecordable signal preserves its envelope and says so; approval is the triage act and never auto-drives; resume redispatches only what pause held.
- Two semantic corrections landed first: `Started` no longer touches run-length accounting, making `MAX_IDENTICAL_OUTCOMES` fire for retry loops, and resume refuses untriaged interruptions and only them.
- The operator story runs end to end through the binary and the final stream replays byte-identically; `execution_cli` is a gate stage, proven able to fail.
- The acceptance map in the milestone document ties all eight §8 criteria to named tests, with the gaps in the same table: file-based signal evidence, undesigned signal-to-draft translation, unbudgeted ghost births, the resume file-trust seam, and the intentional simulate/executor divergence.

## Graph Engine and Governor, 04e — 2026-08-13

- `NodeOutcome::{Paused, Interrupted}` and `SimulationStatus::Cancelled` added, appended so no existing wire name moves; `execution_paused` and `execution_resumed` grow the closed event set from 23 to 25, with the envelope schema corrected in place under D-037 and both catalog digests recomputed. Cancel is a final status per §13, not a new event kind.
- Three transition arms close long-named gaps: `Blocked` gains its owner resume path (open since 04a), graceful pause holds `Ready`/`Queued` work, and an interrupted running node can only become `Blocked` — a crash is not an outcome the executor reported, and anything but blocking would authorize a retry nobody judged safe.
- Pause and resume fold with coherent-history guards; an incoherent pause or resume is corrupt.
- Pure `recovery_plan` and `resume_preconditions` added; §11.4's undecidable items are named, not approximated. The checkpoint is the `ProjectionGeneration` 04b already ships — no second checkpoint type exists.
- The composed lifecycle test drives every pure piece since 04a through pause, crash, recovery, owner approval, resume and completion, and the full history replays byte-identically, including split through `apply_page`. Three findings for 04f are recorded in the milestone document: resume does not demand triage of blocked nodes, the honest crash-recovery order is pause-recover-approve, and `MAX_IDENTICAL_OUTCOMES` is structurally unreachable for retry loops, a design defect 04f must resolve.

## Graph Engine and Governor, 04d — 2026-08-13

- Three governance event kinds added — `signal_recorded`, `ghost_node_proposed`, `mutation_accepted` — growing the closed set from 20 to 23; the `1.0.0` envelope schema corrected in place under D-037 with both copies byte-identical and both catalog digests recomputed. `signal_recorded` carries no free-form content per D-036: typed fields plus a digest binding the record to the raw envelope bytes destined for encrypted Evidence.
- `SignalSeverity` and `SignalSourceKind` moved to `graphhelm-protocols` as wire vocabularies, gaining the `Serialize` their new role requires; `core/execution` re-exports both.
- The projection folds `signals_recorded` and `accepted_mutations` by counting history, and folds a ghost's birth: a proposal for a node that already has any state is corrupt, and an acceptance recorded under a mode the execution was not in is corrupt — decision 5.5 enforced at replay.
- Pure governance decisions added in `core/governor`: `admit_signal` blocks at the signal budget rather than dropping evidence; `decide_mutation` maps D-022's modes with Manual and no-mode rejecting, blocks at `MAX_ACCEPTED_MUTATIONS` in every mode, and rejects an unrecognized kind at every mode and counter value; `override_with_waiver` reuses the M03 waiver verbatim, node-scoped, refusing an empty risk acknowledgement. Id and clock are injected, never read.
- Bounded concurrency added as `dispatch_plan`: a deterministic prefix of the ready set, `ZeroParallelism` surfaced loudly. The function that will read `max_parallel_model_calls` now exists; wiring the field to it is the 04f driver's work.
- Nothing appends these events or drives intake yet; the decisions await the 04f driver. Ghost approval reuses the existing `Approved -> Ready` path unchanged.

## Graph Engine and Governor, 04c — 2026-08-13

- `ready_set` added: which nodes may be dispatched now. Only `Ready` nodes are dispatchable, and a test pins that every dispatchable state accepts a `Started` outcome, so the scheduler cannot propose work the state machine rejects. Dependencies are fail-closed — every incoming edge gates, whatever its type — and a predecessor releases its dependent only when `Succeeded`, `Waived` or `Skipped`. Exceeding `MAX_READY_SET` blocks rather than truncating, per decision 5.7.
- `NodeState::Ghost` is excluded from the ready set by construction and never releases a dependent, making "consumes no tokens" structural rather than conventional. Covered by a property test sampling randomised assignments of the other nodes' states.
- `classify_progress` added: retry exhaustion and repeated identical outcomes, read against the counters the projection derives. It reads the run length through `identical_outcomes_for`, so a run belonging to a different outcome cannot block a node on its first failure.
- Five of the seven `OBSERVABILITY_AND_RECOVERY.md` §15 no-progress conditions are deliberately not detected; they need signal intake or real tool calls, and none is approximated.
- `FixtureExecutor` added in `core/simulation`: the first and only milestone-04 `NodeExecutor`, consulting a fixture table and nothing else. Its answer does not depend on the attempt number. It has no callers yet — `simulate()` still drives its own transitions, so the seam is defined but simulation is not yet a consumer of it; the divergences between the two are tabulated in the milestone document and tracked as outstanding work.
- `MAX_PROJECTION_NODES` separated from decision 5.7's domain bounds as a resource guard, published, and pinned above `MAX_READY_SET` by a module-scope `const` verified to fail `cargo build`. It is not compared against `MAX_SIGNALS_PER_EXECUTION`, which counts a different dimension.
- 04a's purity invariant narrowed to what it protects — adapters, clocks and randomness, not sibling core crates — with the exact dependency set pinned by a second test.

## Graph Engine and Governor, 04a/04b — 2026-08-13

- `core/execution` added: a pure crate with no I/O, no clock, no randomness, and no adapter dependency, enforced by a source invariant test rather than documented alone.
- `NodeState::Ghost` added to the shared vocabulary; its only legal exit is approval to `Ready`, proven by property test across every outcome and counter value.
- Bounds fixed as counters, never durations: `MAX_NODE_ATTEMPTS` 8, `MAX_IDENTICAL_OUTCOMES` 3, `MAX_ACCEPTED_MUTATIONS` 64, `MAX_READY_SET` 1024, `MAX_SIGNALS_PER_EXECUTION` 10,000.
- The closed Graph Signal typed subset added; an unrecognized kind is recorded as evidence but can never propose a mutation.
- `apply_transition` added: total and property-tested for totality, determinism, and ghost-safety. The `NodeExecutor` trait added as the Milestone 05 seam, with no implementor yet.
- `NodeOutcome` and `ExecutionMode` added to `graphhelm-protocols` as closed vocabularies. The four execution event kinds — `execution_started`, `execution_mode_changed`, `node_outcome_recorded`, `execution_completed` — added to the closed event set, growing it from 16 to 20; the `1.0.0` envelope schema corrected in place under D-037, both schema copies kept byte-identical, and both catalog digests recomputed.
- `ExecutionProjection` extended with execution ID, mode, and per-node attempt and identical-outcome counters, all derived by folding history rather than read from a payload. A generation predating these fields loads with them empty; one missing a pre-existing field still fails.
- Replay proven identical across runs, and a discarded generation rebuilt through `apply_page` proven to land on the same state as a direct replay.
- `GHPROJ001_WATERMARK_MISMATCH`, shipped in Milestone 03 with no observed test, now has one: it drives the guard in `ProjectionRebuilder::rebuild` through a test-double repository, since the production PostgreSQL adapter cannot structurally reach it.
- Scheduling, in-flight governance, pause/resume/cancel, and the operator CLI remain out of scope; they are Milestones 04c through 04f.

## Production Event and Evidence Store — 2026-08-12

- Decisions D-035, D-036, and D-037 accepted with ADR-021 through ADR-023: authoring and persistence are distinct representations, the Governor externalizes free-form content as encrypted Evidence, and required content that is unavailable blocks execution.
- Safe persistence projection design and a focused Event/Evidence Store threat model added.
- Single pre-release schema baseline `1.0.0` rebuilt with 15 contracts, adding `PersistedGraphVersion`, event envelope, Evidence record, artifact reference, repository scope, and sensitivity; the intermediate release `1.1.0` removed.
- Eleven typed content positions registered, including the `context_path`, `permission_path`, and `isolation_path` authoring scopes.
- Local JSONL repository and PostgreSQL adapter implemented against one wire contract, with forced row-level security, transaction-local scope, authenticated stream heads and integrity checkpoints, and least-privilege runtime roles.
- Encrypted Evidence, sealed local key provider, authenticated revocation journal, legal holds, and auditable cryptographic erasure implemented.
- Disposable projection generations, fail-closed executable materialization, encrypted streaming backup, and verified restore implemented.
- Operator commands `events verify`, `events rebuild`, `events backup`, and `events restore` added, with bounded JSON configuration and out-of-band key material.
- No legacy compatibility layer: superseded event formats, importers, dual readers, and fallback branches removed before the first public release. Existing developer repositories and databases must be deleted and recreated.
- Milestone documentation and operational procedures published; the specification version remains 0.1.1.

## 0.1.1-spec — 2026-08-08

- Product name selected: GraphHelm.
- Naming decision and brand architecture documented.
- Initial Codex prompt for the Foundation Graph Kernel.
- Manual override example fixed with explicit deploy target.

## 0.1.0-spec — 2026-08-08

- Full PRD for Programação 5.0.
- Decisions on topology, autonomy, harness, agents, context, Dreams, models, isolation, UI, open source, and licensing.
- Studio functional specification.
- Harness Compiler and Graph Governor.
- Complete Graph Engineer guide.
- Graph DSL v1 and JSON Schemas.
- Event Store, Knowledge Graph, Living Documentation, and Context Capsules.
- Universal Model Gateway with BYOK, official subscriptions, and local models.
- Threat model and isolation tiers.
- Observability, checkpoints, replay, and recovery.
- MIT license and open governance.
- Graph and manifest examples.
