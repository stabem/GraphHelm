# #161 design — addendum: N's three findings applied

Author: J, after N's review (`.factory/n-agent-161-review-criteria.md`). All three
findings accepted; none required a redesign, and F1 turned into a stronger claim
than the one it corrected.

---

## F1 — the enumeration I owed (and it changes the claim)

N is right and it is my own rule turned on me: I named the CLASS of danger
("last-state answering a per-sequence question") and never enumerated WHO is
exposed. "No history map needed" was an architecture claim with no coverage
argument behind it. Enumerate-by-spend-site says list the CONSUMERS, so:

**Method, with its limits declared.** `trace_path(replay, inbound, depth=1)` via
codebase-memory. Index root is the MAIN CHECKOUT, whose branch is
`issue-m09-arming-the-alarm` — **PRE-M11**, so it does not contain A's customs
family. The M11 consumers are therefore enumerated by direct read of A's branch
instead, and named as such. 38 callers returned; tests and my own parked draft
excluded, leaving the production set below.

| Consumer | Question it asks | At-N? |
|---|---|---|
| `execution/driver.rs::reread`, `core/runtime/driver.rs::{reread_async, record_outcome_with_evidence}` | current node states, to dispatch | HEAD |
| `execution/mod.rs::{load_projection, replay_projection}` | generic load | HEAD |
| `execution/{start,resume}.rs::execute_prepared` | current state before acting | HEAD |
| `execution/status.rs::{execute, write_snapshot}` | operator status now | HEAD |
| `execution/wake.rs::{arm, status, replayed_horizon}` | current lease / receipt | HEAD |
| `commands/replay.rs::run` | rebuild to head | HEAD |
| `serve/monitor.rs::monitor_page` | current page | HEAD |
| `serve/wake.rs::{sweep, record_consumptions_inner}` | is this lease still live **as of my own read** | HEAD-AT-READ |
| `wake_wait.rs::read_own_lease` | my lease, pre-block | HEAD |
| `tools/acceptance-map::replay_demonstration_store` | rebuild to head | HEAD |
| **the fold itself** (`apply_projection_event`, mid-walk) | **registry at the event being folded** | **AT-N** |
| M11, read from A's branch: the `CompletionCleared` arm | (today) none — releases unconditionally | — |

**Result — a better claim than the one N corrected.** The at-N question has
exactly ONE consumer in the entire tree today: the fold, mid-walk, where the
accumulator IS the answer. Every other consumer asks a HEAD question, and for a
head question `clearance_registry` is correct. So "no history map" is no longer
an architectural assertion; it is a coverage statement over an enumerated set of
38 call sites.

**And the enumeration names the real exposure, which the class-level claim hid.**
The danger is not a consumer that exists — it is the NEXT one. A future surface
wanting "could X sign at N?" reaches for `clearance_registry`, gets head, and is
wrong with no signal. That is precisely the gap F2-of-my-own-design could not
close with a comment, which is why N's mechanism proposal below is adopted rather
than argued with.

**Re-run condition, named so this does not rot:** this enumeration is valid for
the pre-M11 tree plus A's branch as read today. When the customs lanes land —
especially E's `customs_scans` — the inbound set changes and this table must be
re-derived, not assumed. A stale enumeration is exactly the instrument-aimed-at-
the-wrong-tree defect.

## F2 — R3's missing positive control (accepted, two lines)

N is right: R3 as written ("a bundle hash that does not match the manifest is
refused `HashMismatch`") is passed by an implementation that never hashes
anything and refuses every `MachineReplay`. R1 has R2 as its control; R3 had
none, so it was an absence guard with no presence member — the family rule I
have been applying to everyone else's cells all week.

**Added — R3b, the positive control:** a `MachineReplay` clearance whose bundle
hash MATCHES the node's declared manifest CLEARS, and the downstream unlocks.
R3 and R3b are a pair and are reported as a pair; neither is cited alone.

## F3 — R5 measures non-determinism, not correctness (accepted, and I take the
stronger of the two options N offered)

N: "identical across folds" is satisfied by any deterministic implementation,
**including a deterministically wrong one** — sabotage sE leaves R5 green. True,
and it means R5 was carrying a name it had not earned.

I take BOTH halves rather than choosing:

1. **Stated plainly in the criteria**: R5 measures NON-DETERMINISM ONLY. It is
   not a correctness guard, it cannot fail for a wrong-but-stable fold, and it
   must never be cited as evidence that verdicts are right.
2. **A deterministic-but-wrong sabotage that DOES fell it** — N's suggestion,
   adopted: **sF, unordered accumulation.** Collapse register/revoke into a
   set-union that ignores order (e.g. apply all registrations first, then all
   revocations, or key the registry by identity without honoring walk order).
   That is perfectly deterministic and produces the WRONG membership-at-N. It
   leaves R5 green and fells R1-sharp and the revocation pair — which is the
   point: sF is what proves R5's greenness is not evidence, by exhibiting the
   world R5 cannot see.

## The eyes question — answered with a mechanism, and the mechanism wins

I asked for a second pair of eyes on "may the command layer's pre-flight read use
head-state?" N agrees the reasoning is sound (at append time head IS N) and then
makes the better point: **a comment is not a mechanism.** Their proposal — the
computed value CARRIES the sequence it was computed at, so a consumer holding a
head that is not N cannot silently accept it — converts a discipline the reviewer
must remember into a refusal the type performs.

**Adopted.** The membership answer is not a bare `bool`; it is a value carrying
the sequence it was decided at, and a consumer comparing it against a different
sequence gets a compile-time or explicit-refusal failure rather than a quiet
wrong answer. This is the same move as the append-only canary from #88, one rung
better: the canary told the next reader; this one tells the compiler.

Recorded as a CHOICE with its reason, per N's framing — had I kept the comment,
the design would have to say that was chosen and why. It was not; the mechanism
is cheaper than the vigilance it replaces.

## Scoreboard note

N refuted one of their own predictions in my favour (sD already names both
drivers by path, with the decoration clause). Recorded here because a review
whose predictions never lose is a review written afterwards — the same standard
I hold my own sealed cells to.

## A property obtained free from a mistake (2026-08-20)

Adding the two clearance-identity kinds to `event-envelope.schema.json`, my first
attempt rewrote the file with a JSON pretty-printer and produced **1748 insertions**
for what is a ~30-line addition — the packaging defect that buries a real change under
a reformat. I redid it as a minimal textual insertion: **40 insertions**.

**The catalog pin came out IDENTICAL from both** (`sha256:17c379d4ae54b5fbb405cc0664
8933c035ed27f0403856698acbe8315d5d51fb`). That is a demonstration, not an argument:
the canonical digest is **independent of the file's formatting**, exactly as the
canonicalisation was designed to be (recursive key sort into a BTreeMap, compact
serialisation — so whitespace and key order in the stored file cannot move the pin).

Nobody had exercised that property. It means a reformat of a schema file — by a linter,
an editor, a different serialiser — does NOT invalidate its catalog pin, and a pin
mismatch therefore always indicates a CONTENT change rather than a cosmetic one. Worth
knowing before someone treats a formatting diff as a schema break, or worse, treats a
pin match as proof that a file was untouched.

**Method note, since the pin was computed without the tool:** `graphhelm schema digest`
is the right instrument and needs a build. Lacking a slot, I reproduced the CURRENT
recorded pin (`sha256:2ff2036d…`) with my own canonicalisation first, byte-for-byte,
and only then computed the new one. A positive control on the instrument before
measuring the unknown — the same discipline this file spends its length demanding of
other people's guards.
