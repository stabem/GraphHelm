# M11 engine lanes — one-page synthesis

D, 2026-08-20. Covers the three candidate lanes I hold pieces of: **#87 commit 2**, **#101**, and
**#114 + #129**. Optional input to the M11 proposal; nothing here is a decision.

## The thesis: M11's engine work is mostly CONNECTING HALVES, not building capability

All three lanes have the same shape, and it is worth naming because it changes how they should be
estimated. In each, **the capability already exists in one layer and does not reach the layer that
would use it.**

| Lane | The half that exists | The half that is missing |
|---|---|---|
| **#87 c2** | a verified-prefix cache that makes warm reads 100–1100× faster **per handle** | `serve` holds no long-lived handle, so requests never get a warm one |
| **#101** *(closing)* | one terminality rule, one parallelism policy | *(was: three and two copies agreeing by hand)* |
| **#114** | `NodeType::Deploy` + linted `targetRef` + compensation rule + policy hook + owner-override structure | **execution** — `work_kind` refuses it by name |
| **#129** | a complete owner override (`waived_requirements`, `acknowledged_risks`, actor, reason) | any path from that override to the **deploy deny**, which is a *lint error* in another subsystem |

**Estimating these as "new features" will overprice them and mis-order them.** The work is
plumbing between existing parts, and the risk is concentrated at the seams rather than in the
parts — which is exactly where this milestone's defects have actually lived (#80's two drivers,
#123's two drivers again, #129's two subsystems).

## Sequencing, including one hard constraint

**#114 is downstream of #93 and #94.** Steps 8–10's *"forces deploy"* is precisely the capability
those two record as missing: no dispatch-time override exists at all (#93), and D-019's waive/skip
lever cannot be pulled from any surface (#94). Scoping #114 ahead of them would build a deploy that
sovereignty cannot reach.

**#129 is the blocker inside #114, not a parallel item.** Even with execution built, an operator
following the acceptance steps hits a load-time lint error with nothing to reach for. #129 also
carries the second half — nothing in the tree distinguishes a test environment from production;
`targetRef` is a free string and the only distinction is which policy key an author typed.

**#87 c2 is independent of all of the above** and can run in any order.

**#101 needs nothing** — PR #136 is open, reviewer L, awaiting a slot for its certifying gate.

## What each lane needs from M11, concretely

**#87 c2 — a decision, not a design.** Requirements are already sealed
(`.factory/a-agent-issue87-evidence.md`): CONV-1 substitution, ABAB, per-row fields, `with_caller`,
the named trigger revisit. **One input I owe forward from the c1 review:** the cache's critical
section is **O(suffix bytes)**, because the file read happens under the mutex. Under per-operation
opens that is invisible. Under a long-lived shared handle — which is exactly what c2 introduces — a
handle that has fallen behind pays a long read with the process-wide lock held, and every other
operation waits. It is filed against A's existing trigger comment, which names the *staleness*
exposure; this is a *contention* exposure on the same line. **c2 should not be scoped without it.**

**#101 — nothing.** Mechanical, no behaviour change claimed, and the PR body flags that its green
came from a build predating the full-members clean rule, so the certifying gate is the evidence it
merges on.

**#114 — an adapter and two decisions.** Smallest honest object is the existing `NodeType::Deploy`
plus one `NodeWorkKind::Deployment` plus one adapter port mirroring `Tool`'s shape. No protocol
change, no schema vocabulary, no new lint. The decisions are #129's two exits.

**#129 — two product calls, both owner-reopenable.** Whether a complete override may clear a hard
deny (which changes what "lint error" *means* product-wide), and whether `targetRef` gains a checked
shape or environment classification stays the author's responsibility.

## The pattern worth carrying into M11's design reviews

Three defects this milestone shared one cause: **a rule that existed in more than one place, or in
a place that could not reach its consumer.** #80's ungated union existed twice. #123's release
half had to be added twice. #101's terminality existed three times. #129's override and deny exist
in subsystems that never meet.

**The cheap check that would have caught all four: for any rule, enumerate by where its value is
SPENT, not by where it is defined.** Searching for callers of the thing that consumes a rule found
the second driver in #80; searching for lookalikes of the expression did not. That is a review
question M11 can ask on paper, before any code exists.

## What this synthesis does NOT establish

- **I did not read the M11 proposal or PRD.** This is bottom-up from three lanes I worked, not a
  reconciliation with whatever M11 already says it wants.
- **No estimates.** "Plumbing rather than capability" is a shape claim; I have not priced any of
  the three, and #87 c2 in particular touches 38 call sites by A's count.
- **#87 c2's requirements are A's work, not mine.** I reviewed c1's arithmetic and CONV-1; the
  sealed requirements are theirs and should be read from their file rather than from this summary.
- **No demand evidence for #114.** It remains a marker issue; nobody has needed a deploy node yet,
  because none can execute.
