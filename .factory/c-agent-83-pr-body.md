> **PROVENANCE: this document became the body of the PR that closed #83.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

Closes #83

`execute_prepared` committed the resume decision — the `ExecutionResumed` append and the paused-node
redispatches — and only then did `drive()` run its own setup. That setup can refuse: `build_sealer`
alone rejects a missing or malformed `GRAPHHELM_EVENTS_KEY`. When it did, the operator got a 500
`GHCLI016_DRIVER_FAILURE` over a store that had already recorded the resume and released their held
nodes. "The call failed" and "your hold still holds" are one fact to an operator, and that ordering
made them two, silently, at exactly the moment someone is leaning on the hold.

## The fix

The fallible half of the setup moves into `prepare_drive`, which runs **before** the commit, so the
decision is the last thing that can fail rather than the first thing that commits.

Hoisting is safe because **every hoisted step is read-only**, and that was verified rather than
assumed: `build_sealer` validates an environment variable; `ServeToolPort::build` validates a
workspace config and constructs a host; the `direct_api` arm opens the broker with `open` (which
reads — not `open_or_create`) and leases a credential through `CredentialBroker::lease`, which takes
`&self`, acquires no lock and persists nothing. A decision refused after that ran leaves nothing to
release, so no drop guard or lease-lifetime story is owed. An earlier draft of this design claimed
the lease WAS a side effect and built machinery for it; the code says otherwise and the autopsy
lives in `prepare_drive`'s doc comment so nobody re-derives the scare.

The hoist lands in the shared route shape, so `start` (`routes.rs:277`) is repaired by the same
change — and carries its own guard rather than being fixed silently.

## What this does NOT make atomic

The `Started` redispatches are a **per-node append loop** (`resume.rs:230-243`), so a crash mid-loop
still leaves `ExecutionResumed` with some nodes `Paused`. Pre-existing and out of scope. The claim
here is only that **a setup failure cannot commit the decision** — stated as a named non-goal rather
than covered by a vaguer adjective.

The response still answers `GHCLI016` for both a setup refusal and a genuine mid-drive failure,
which are opposite hold-states for an operator. Split out as **#96** rather than folded in: bundling
an operator-visible contract change with an internal ordering change would make the ordering
unrevertable on its own, and would mint an error code with no observed red behind it.

## Commits

| SHA | what |
|---|---|
| `2f5083f` | the fix, four guards, design + sealed predictions |
| `4aa2e9c` | S4 same-key guard; orphaned doc comments clippy caught |
| `f7726b4` | gate runs this suite in isolation; the allowlist trap documented |

## Guards

Five, all at the named-test grain:

- **`a_resume_whose_drive_setup_fails_leaves_the_operator_hold_intact`** — the primary. Its journal
  assertion is **watermark-anchored and landmark-proving**: *in the journal that provably contains
  this test's own pause at sequence W, nothing after W is `execution_resumed`*. Watermark rather
  than last-event because the `Started` redispatches append AFTER `ExecutionResumed`, so a
  last-event formulation is **green before the fix**. Landmark because absence is only evidence when
  the same read proves it is looking at the right stream.
- **`a_second_resume_after_a_failed_one_is_still_accepted`** — the operator-visible half, and the
  judge's recorded symptom from the issue's observation 1.
- **`the_same_idempotency_key_after_a_failed_setup_still_executes`** — a different blade: that one
  asks whether the hold survived, this asks whether the key was spent.
- **`a_resume_whose_drive_setup_succeeds_still_commits_the_decision`** — the positive control. See
  row PC below for why it is the most important test here.
- **`a_start_whose_drive_setup_fails_commits_no_execution`** — `start`'s thin guard, with a **paired
  control**: a refused start legitimately leaves the stream empty, so the same read must go from
  empty to non-empty across an accepted retry. A wrong stream id cannot do that.

**Arrangement is `docs/acceptance/m09-judge-run-2026-08-19/release.yaml`** — deliberately not a fresh
fixture. It is the graph the M09 paid run started, paused and resumed over this same API: the one
that produced the issue's observation 1. The guards reproduce the defect on the artefact that
discovered it. (Its three nodes are all `type: tool`; the node NAMED `deploy` is a Tool, not a
`NodeType::Deploy` — only an all-classifiable graph reaches the async drive path where the defect
lives.)

## Sabotage ledger — six confirmed, one dropped

Each confirmed row is red **at its own assertion, named by panic site**.

| # | WHICH edit | result |
|---|---|---|
| 1 | resume ordering reverted to commit-then-setup | **CONFIRMED** — `:427:5`, `:457:5` |
| PC | `prepare_drive` always refuses | **CONFIRMED** — control falls `:550:5`; **both hold-guards stay GREEN** |
| 7 | start ordering reverted alone | **CONFIRMED** — `:633:5` only; resume guards green |
| iii | S4 same-key retry | **CONFIRMED** — `:722` |
| ii | non-sealer setup failure (valid key + synthetic later refusal) | **CONFIRMED** — `:439`, `:469`, `:722` |
| i | projection-vs-journal | **MEASURED — the sealed claim was OVERSTATED** |
| iv | S5 triage preserved | **DROPPED — reason named** |

**Row PC is the one that changed the shape of this PR.** With setup always refusing, **both**
hold-guards still pass — because nothing commits is exactly what they assert. A "fix" that merely
made resume refuse more would have satisfied the entire defect-guard family. Only the positive
control falls. Generalised: **every absence-shaped guard family needs one member asserting the
presence the family exists to protect.**

**Row iii exposed a third response class.** Under the pre-fix ordering the same-key retry answers
**200 `ok:true`, `status: "running"`** — a replayed success the first call never reported. The
operator is told it failed, retries exactly as an idempotency key exists to permit, and is told it
worked. Measured, raw reply quoted in
[a comment on #83](https://github.com/stabem/GraphHelm/issues/83#issuecomment-5348037413). It is the
most dangerous of the three because it is the only one that reads as *fine*: a failure and a refusal
both prompt a human to look; a fabricated success ends the investigation.

**Row i is recorded against its own seal.** The prediction was pre-registered as likely to weaken,
and it did: under sabotage 1 the reply carries `status: "running"`, so a projection-based assertion
would also have caught it. The journal assertion is therefore **not** uniquely load-bearing for that
sabotage. Its real and narrower value — pinning WHICH event at WHICH sequence
(`execution_resumed` at 20, watermark 19), and immunity to a projection agreeing for the wrong
reason — is what the corrected row claims.

**Row iv is dropped, not absorbed.** S5 needs a node still `Running` when the execution stopped so
`recovery_plan` is non-empty, which requires the hanging-executor runtime arrangement — a different
test shape and more machinery than this slot warranted. **S5's protection therefore rests on a
structural argument** (triage lives inside `execute_prepared`; the hoist moves code ABOVE that call,
so it cannot move triage) **and that is an ARGUMENT, not a measurement.** Labelled as such. Seeded
in #96 to ride a future runtime-test slice rather than justify one.

Row ii's two other casualties (`:562`, `:680`) are the success-path controls, which a
make-everything-fail sabotage necessarily takes. **Named as expected casualties, not counted as
confirmations** — five reds are three confirmations plus two anticipated.

## Gate

```
[gate] GREEN - every stage passed.
```

23 stages. `grep -cE "test result: FAILED|panicked at|error\["` over the full 1088-line log returns
**0**. The non-C collation matrix — where #19's flake class lives — passed.

**Content-verified, because the exit code carries no information.** This run exited 0; so did an
earlier RED run whose verdict line read
`[gate] RED - failed stages: rustfmt, clippy (deny warnings)`. Filed as **#97**, with the symmetric
finding that the status says nothing in *either* direction.

**Env stated so no one reads across a boundary:** `CARGO_TARGET_DIR=D:/graphhelm-target-c83`,
**warm** — not a cold figure.

That earlier RED was mine and worth keeping in the record: clippy's three "empty line after doc
comment" errors were **orphaned doc comments** dragged in when this file's HTTP helpers were mirrored
from `api_http.rs` by line range — two described functions never extracted, one was truncated
mid-sentence. **Five green test runs never noticed.** A green run audits code; only clippy and the
diff read audit prose.

Also filed from this lane: **#98** — `gate.ps1`'s per-suite loop is an allowlist, so a new test file
is under-gated **by default and silently** (it still runs inside `workspace tests`, but misses the
isolated pass that catches cross-test interference). `f7726b4` adds this suite by hand and documents
the trap until #98's structural fix lands.
