# Convergence refutation — does "one mechanism" survive the code nobody read?

Author: M Agent, 2026-08-19. Worktree `m-agent-76c232`, base `53d212d`. Read-only: zero cargo,
zero writes outside this worktree. Every line reference read by me at `53d212d` today.

## The question

Three independent findings were reported as converging on one mechanism each:

- **CONV-1.** A's per-request store open (M10 proposal) = D's storm convoy driver. Recorded on
  the board as "One mechanism, two symptoms."
- **CONV-2.** B's #55 middle window (issue-55 memo) = C's flake-3 rival consume (wake-flakes
  study). Two independent derivations of one window.

Convergence is either strong evidence or a shared blind spot. The orchestrator's sharpening
picks the discriminator: everyone read the same funnel, so start from **what nobody read** —
the code paths that touch the store and appear in NO report — and see whether the one-mechanism
story survives contact with them.

## Method

I enumerated every production (non-test) caller of the single store-open helper
`commands::event_store` (`apps/cli/src/commands/mod.rs:281-285`, wrapping
`LocalEventRepository::open`), then marked each as covered or absent across A's, B's, C's, D's
and L's reports.

## The inventory

**Covered somewhere in the report set:**

| Path | Site | Covered by |
|---|---|---|
| serve API request path | `serve/routes.rs` | A, D |
| serve wake sweep (3 phases) | `serve/wake.rs` (3 opens) | C, D |
| async driver fan-out | `core/runtime/src/driver.rs:220-221`, `:274-275`, `:308-309`, fed `serve/routes.rs:819` → `:872` | L only — and only after D's retraction had already been built on top of it |

**Absent from every report:**

| Path | Site | Why it matters |
|---|---|---|
| serve monitor page | `serve/monitor.rs:539-540` (index), `:585-587` (page) | Two more per-HTTP-request opens on the same serve process, both inside `spawn_blocking` |
| CLI execution commands | `execution/{start,resume,status,pause,cancel,approve,amend,signal,wake}.rs` — 12 `event_store` sites | Out-of-process openers contending for the same directory |
| wake sidecar | `wake_wait.rs:109` (`read_own_lease`) | Named as a process in the flake-2 reports; its store open is counted in nobody's open model |
| store maintenance | `commands/events/{backup,rebuild,restore,verify}.rs` | Out-of-process **writers**; `verify` is queue item 4's "doctor" |
| simulate / replay / quality | `simulate.rs`, `replay.rs`, `quality.rs` | One open each, out-of-process |

**Checked and found NOT to be a store toucher:** the MCP surface. `commands/mcp/*` contains no
`event_store` call; it reaches the store only by calling the same command functions
(e.g. `mcp/tools.rs:330` → `wake_wait::wait`). So the MCP stdio surface adds no independent
opener — one candidate refutation angle, closed.

**Naming drift worth flagging:** there is no `doctor` command in `apps/cli/src` (grep: zero
hits). Queue item 4's "doctor: 2 journals fail integrity" must refer to `events verify` or the
integrity module (`core/events/src/integrity.rs`). K owns that lane; naming it here so the lane
is not chased under a command name that does not exist.

---

## CONV-2 — SURVIVES. One finding, one sharpening.

**Verified independently, not taken on trust.** C's review gate G8 asserts `record_consumptions`
is the only production writer of `WakeLeaseConsumed`. I re-ran that check rather than accept it:
the two construction sites are `serve/wake.rs:214` and `:308`, and `:308` sits inside
`#[cfg(test)] mod tests`, which opens at `:240-241`. So there is exactly one production writer,
and no unread rival consumer exists anywhere in the tree. **G8 holds.** CONV-2's shared premise
is not a shared blind spot.

**The sharpening, and it widens a parked seed.** B's and C's window-3 analyses both model the
rival as another *consumer*. The window does not require that. `execution/wake.rs:100`
(`arm`, the CLI `wake arm` path) is a production **appender** on the same stream, running
**out-of-process**. Between G1's validation replay (`wake.rs:171`) and G2's sequence pin
(`wake.rs:190`), a rival *arm* moves the head exactly as a rival consume does: the pin lands
post-rival, the CAS has nothing to refuse, and the consume burns against a replay that predates
the arm.

That is the shape of C's parked seed C-R3 (`a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it`)
reached by a different actor than C's fixture uses. C's fixture re-arms via the sleeper; the CLI
`wake arm` reaches the same state from another process, across the file lock. This is **not** a
refutation of CONV-2 — it is a widening of the seed's blast radius, and it matters because C-R3
is currently parked outside the crate as second-PR work on the grounds that it is a defect PR 1
does not fix. A second, out-of-process route into the same defect is an argument about
scheduling, which is the owner's call, not mine.

---

## CONV-1 — DENTED. Not one mechanism with two symptoms.

### Finding 1: "per-request open" is one pattern with at least four instances, and the storm model counts two

D's open arithmetic (his frozen D-P5: "opens per mutation request ≈ 4 — three request-path, one
sweep") is scoped to the API request path plus the sweep. On the same serve process, in the same
`spawn_blocking` pool, two more production openers exist that no report counts: the monitor's two
handlers, and the driver's three sites (L's finding). Calling A's and D's subjects "one
mechanism" overstates the unity — they are two instances of a repository-wide *pattern*, and the
pattern's other instances were never enumerated before the arithmetic was written on top of two
of them.

This does not change the storm number: the storm test drives the API, not the monitor. It
changes what the number licenses. "Serve opens the store per request and that is the bottleneck"
is a claim about the serve process; it was measured against a fraction of that process's openers.

### Finding 2 (the headline): both A and D refuted the doc's MECHANISM and inherited its CONCLUSION as unsupported — but the conclusion has a second support neither examined

The `ServeState.events` doc comment (`serve/mod.rs:106-117`) is the one D flagged as stale. Read
it in full, because the part that matters is not the part that is wrong. Verbatim:

> "`ServeState` deliberately does *not* cache an open repository handle: Milestone 05a Task 1
> confirmed empirically that `LocalEventRepository::open` holds an OS-level exclusive lock for
> the handle's entire lifetime, so a handle cached here would hold that lock for the server's
> whole run and lock out every concurrent CLI process against the same events directory —
> defeating the multi-agent premise this API exists for. The per-request open/use/drop cycle
> *is* the concurrency model".

The **stated mechanism is dead**: `open_inner` releases its locks before returning
(`core/events/src/local.rs:390-397`), so a cached handle would not hold a lock for the server's
run. A found this; D found it; both are right.

What both then did is treat the doc's *conclusion* — do not cache — as resting on that dead
mechanism, and therefore as free to discard. It does not rest only on that. The doc names the
thing being protected: **"every concurrent CLI process against the same events directory"**, and
it calls that "the multi-agent premise this API exists for". That premise is real and checkable
in the tree, not rhetorical: `event_store` is (the doc's own words) "exactly the call every CLI
command makes", and my inventory above counts 12 `execution/*` sites, the sidecar, `simulate`,
`replay`, `quality`, and four `events/*` maintenance commands — several of which **write**. The
doc even records the empirical incident that proved cross-process reality: a hand-run
`graphhelm execution status` against a directory `serve` had already touched
(`serve/mod.rs:1113-1119`).

Now apply that to A's proposed M10 fix. A's D1 is an **incremental verified-prefix held in
memory per handle**, plus a long-lived serve handle. Locking is not the hazard — `with_lock`
takes and releases per operation either way. **Cache coherence is.** A verified prefix cached
across requests is invalidated by any out-of-process append, and nothing notifies serve that one
happened. Today's per-request open re-reads and re-verifies the whole journal from disk on every
request; that is precisely the cost A wants to remove, and it is *also* the mechanism that makes
the multi-process premise safe today.

So the sentence "the per-request open is pure cost" is false as stated. It is cost **and**
cache-coherence. A fix that removes it must supply coherence some other way (a cheap staleness
check on each operation, or an explicit narrowing of the multi-process premise — an owner
decision, since the doc calls it the reason the API exists).

**This is scoring rule 6 in the wild, on a second retraction.** Refuting the doc's mechanism
retired the doc's protection; the replacement (cached handle, cached prefix) imported an
unexamined premise — single-writer process — and the reviewers were looking at the corpse. Same
shape L found behind D's runtime-thread retraction, independently arrived at, on a different
retraction. Two instances make it a pattern in this milestone, not an isolated slip.

### Finding 3: D's cached-handle analysis is correct and incomplete in the same direction

D's T3 correction says a shared handle makes `operation_gate` load-bearing and deletes the win
`53d212d` bought. That is an **in-process** consequence, and it is right. The cross-process
consequence — a long-lived handle's cached state versus out-of-process writers — is not in his
study either. Both A and D examined the fix inside one process. The doc they both partially
refuted is the only artifact in the tree that examines it across processes.

---

## Verdicts

| Claim | Verdict | Basis |
|---|---|---|
| CONV-2 (B ≡ C, window 3) | **SURVIVES** | G8 single-writer re-verified by me; no unread rival consumer exists |
| CONV-2 blast radius | **WIDER than modelled** | CLI `wake arm` (`execution/wake.rs:100`) reaches C-R3's state out-of-process |
| CONV-1 (A ≡ D, per-request open) | **DENTED, not refuted** | Same pattern, but ≥4 instances exist and the arithmetic covers 2 |
| CONV-1's shared premise | **UNEXAMINED where it matters** | Both refuted the doc's mechanism; neither examined the multi-process conclusion the doc protects |

**What is NOT claimed here.** I have measured nothing. Finding 2 is an argument from code and
from a doc comment, and its consequence for M10 is a design question, not a defect in anything
that ships today. Nothing in this report changes a flake diagnosis: CONV-2 survives intact, and
CONV-1's dent is about what the storm numbers will license, not about whether they are real.

## Known-false statements with NO owner (feeds ledger section E)

- **None found.** The one candidate — the `ServeState.events` doc comment — is *partly* false
  (its stated lock mechanism) and its falseness already has an owner: D's storm study names it,
  and the cached-handle fix direction is where it must be corrected. Added here as a note rather
  than a ledger entry: whoever flips that comment must not delete the multi-process paragraph
  along with the dead lock claim. The dead mechanism and the live premise are in the same
  sentence, and the live half is the reason the sentence exists.
