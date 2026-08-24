> **PROVENANCE: this document tracked the #96 lane to its merge.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# #96 — typed, with three findings and one honest gap

**Branch `issue-96-driver-failure-split` off main `6193f5c`. Typed-unbuilt — no cargo has run.**

## What is typed

`GHCLI016_DRIVER_FAILURE` split in two on the start/resume path:

| class | code | site | operator's position |
|---|---|---|---|
| **(a)** setup refusal | **`GHCLI019_DRIVER_SETUP`** (new) | the four sites inside `prepare_drive` | nothing committed, **hold intact** — fix the environment and retry |
| **(b)** mid-drive failure | `GHCLI016_DRIVER_FAILURE` (unchanged) | the drive-result handler | decision committed, work attempted and failed — **hold gone**, execution attended |

**The change is a constructor swap, not new logic, and that is the point.** #83's hoist already made
the distinction structural: every site raising (a) runs before `execute_prepared`, every site raising
(b) runs after it. The response was destroying a distinction the code already had.

Three `#83` guards now assert the new code, and the file header says why: *that they had to change is
the point, not an accident.*

## FINDING 1 — there are THREE classes, not two

My own issue named two. Splitting them surfaced a third: `routes.rs:599`, the immediate-`pause`
path, answers `GHCLI016` when it runs out of budget waiting for `execution_paused`. That is not a
drive failure at all — the operator's question is *"did my pause take effect?"*, a different remedy
from both (a) and (b). It wears the code because that was the nearest stable one when the path was
written.

**Deliberately NOT split here**, and documented at the site so the next reader finds a decision
rather than an oversight: it belongs to a different command and a different operator story, and
widening this diff to cover it would bundle two contracts. **Reviewer call whether it rides here or
gets its own issue** — my lean is its own issue, on the #83/#96 precedent.

## FINDING 2 — the error-code numbering has a live collision, and no registry

Enumerating codes to pick a free number turned up **two different codes sharing 009**:

- `apps/cli/src/commands/gateway/mod.rs:40` — `GHCLI009_GATEWAY_INVALID`
- `apps/cli/src/commands/serve/mod.rs:70` — `GHCLI009_SERVE_AUDIT_FAILED`

Both reach the wire. Consumers matching the FULL string are unaffected, so this is not a live
operator defect — but **there is no registry**, which is why it happened silently and why I had to
grep the whole tree to allocate 019 with any confidence. Same shape as #98: the numbering's intent
and the numbering's reality are two facts that nothing reconciles. **Not fixed here** (renaming a
wire-visible code is a contract change with its own blast radius); flagged for its own issue.

## FINDING 3 — the (b)-side guard is NOT cheaply arrangeable, and I am not faking it

The natural class-(b) arrangement is a sealing failure: run without a keyring, `build_sealer`
returns `RefusingSealer`, setup SUCCEEDS, and the drive fails at seal time with
`DriverError::Sealing`. **It does not work**, and the reason is in `ports.rs`'s own doc:

> a fixture story never calls `seal` (fixtures produce no sealable material)

My whole arrangement is fixture-based (`runtime: None` + an all-tool graph is what reaches the async
drive path at all), so `RefusingSealer` never fires. The other `DriverError` variants need a corrupt
history (`Replay`), an unwritable store mid-drive (`Repository`, racy), or an illegal transition the
fixture executor will not produce. A real class-(b) failure needs the **real-executor arrangement** —
manifest, route, broker, staging — which is the same machinery that blocked S5 in #83.

**So #96's property is HALF-MEASURED and I am labelling it that way:**

- **(a) is guarded** — three tests assert `GHCLI019` on the setup path, and they fail if the split
  regresses.
- **(b) rests on a structural argument**: after the swap, `routes.rs:987` is the only remaining
  `driver_failure` on the start/resume path, and it sits in the drive-result handler after the
  commit. **That is an argument, not a measurement** — the same disposal as #83's S5, and it should
  be verified the same way: by the reviewer reading the diff, which shows the constructor at each
  site.

The guard that would make it a measurement — *(a) and (b) answer different codes in one run* — is
seeded for whichever slice builds the hanging-executor/real-story arrangement, alongside S5-measured
(already seeded in this issue).

## Open for the reviewer

1. **`GHCLI019_DRIVER_SETUP` as the name and number.** 019 is the next free integer; the name mirrors
   `DRIVER_FAILURE`. Both are wire-visible forever, so this is the one irreversible choice here.
2. **Does the third class ride or get its own issue?** My lean: its own issue.
3. **The half-measured disposal** — acceptable as argument-plus-diff-read, or does #96 wait for the
   real-story arrangement so (b) can be measured in the same run as (a)?
