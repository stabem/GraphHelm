---
name: keel
description: "Work under the Keel construction pattern: load the project's KEEL.md and the file for your step, start from a contract card instead of the tree, stay inside its paths, prove the promise end to end first, and apply every Keel value. Use by default for code work and context retrieval in projects that adopted GraphHelm; no explicit user invocation is needed."
---

# Keel, in brief

**When the project has a `KEEL.md` at its root, read it and the file for your step under `keel/`
(`KEEL_PLAN.md`, `KEEL_EXEC.md`, `KEEL_REVIEW.md`, `KEEL_MERGE.md`) first: they are the pattern,
and they win over this skill where they differ.** For a project without them, the execute step's
rules follow verbatim; the standalone `keel` plugin carries every step. Use only as much Keel as
the change needs: a docs or one-line change needs nothing beyond the task record.

<!-- keel embed start: keel/KEEL_EXEC.md (generated; edit the source, then run tools/keel-plugin/sync_keel_plugin.py) -->
# KEEL_EXEC

Step 2, from an accepted card to a proven change. Read `KEEL.md` and the card first.

## Write

- Read the symbol, not the file: search for the name, then open the window around it. Open a
  whole file only when the task is the file.
- Commit structure and behaviour apart: first the moves, renames and extractions with the tests
  unchanged, then the behaviour change.
- Stay inside the card's paths. A path outside it blocks; if the card was wrong, change the card
  first and say so in the pull request.
- Reuse what the plan found. Add only what the promise needs: no speculative option, layer or
  abstraction, and no private function that only forwards to another.
- Apply every value in `KEEL.md` as you write: no duplicate, no dead code, inward dependencies,
  searchable names, no panic or swallowed error, a deadline on every external call, atomic writes,
  idempotent writes, no new slow shape, no complexity increase the card did not justify.
- Keep existing behaviour working. Before you edit shared code, find its other callers and what
  they rely on. A fix that breaks a neighbour is not a fix.
- A change that removes, replaces or redirects a production caller deletes the displaced path
  (its declarations, exclusive helpers, types, styles, dependencies and tests) in the same change
  and writes `Retirement:` in the pull request: each candidate removed, retained (name its live
  consumer or contract) or unobserved (name the gap). A zero-hit search or green tests are leads,
  not deletion proof.
- Stop when the promise is proven. Do not refactor, reformat or sweep beyond it, even when a
  neighbouring file has the same defect: name it in the pull request instead.

## Prove

1. **Choose the cheapest observer that proves the promise**: integration or end to end, or a unit
   test under step 2.
   Prefer the observer closest to the user-visible promise when it is affordable:
   - a screen with an approved journey flow: `graphhelm journey replay` with the browser observer
     installed (`graphhelm setup --install-observer playwright`); a project that keeps its own Playwright
     tests: `tools/playwright-observer/playwright_observe.py --journey <contractId>`;
   - the CLI and the Runtime: the real binary against a real Runtime on a scratch store, asserting
     the reply and what was recorded;
   - hooks: the real hook process in a scratch folder, asserting what it recorded.
   Run it red on the parent and green on the change.
2. **A standalone unit test is a regression test.** Add one only when someone finds a bug: the
   smallest test that reproduces it, red before the fix and green after. The exceptions are data
   formats and digests, security (paths, links, keys, permissions) and pure algorithms, where an
   end-to-end run cannot reach every case. Every new test, whenever it was written, passes the
   break-it-once sabotage (step 5) and the `test-audit` skill's four questions.
3. **The artifact.** Every end-to-end proof ends in a receipt recorded in GraphHelm, the one the
   Studio Proof tab shows: what ran, on which commit, the result, and the command that repeats it.
4. **The failure list.** A change in the last row of the `KEEL_PLAN.md` table writes down every way
   it can fail and proves each one.
5. **Break it once.** Before you call it done, break the change on purpose in one place the
   promise depends on, and show that a proof turns red. Restore it and say what you broke.
6. **Run the whole reached plan.** `tools/reached-tests/run_reached.py` on main plus the change,
   every step, never a hand-picked subset. Cite each step and its result in the pull request, and
   leave the JSON report as the receipt the reviewer reuses (`KEEL_REVIEW.md` step 2).
7. **Report what you saw, not what you hoped.** Passed, failed, skipped and unobserved are four
   different lines. A proxy (another OS, a mock, an emulator, a test compiled out on this host) is
   not an observation. Mock only the clock, randomness and I/O at a named boundary.
8. **No observer, no claim.** When the promise cannot be observed on this host, change nothing
   more, say what is missing and end with `OBSERVER_MISSING: <what is missing>`. Never write
   "verified" for what you did not observe.

## Hand off

Open the pull request with the card, the receipts, the broken-on-purpose result, the reached-plan
steps, every exception you wrote with its reason, and what context the card was missing.

When a rule fires, record its cause (the card, the tool, the task or the agent) before anyone
tightens anything.
<!-- keel embed end: keel/KEEL_EXEC.md -->

Read run state through the GraphHelm MCP tools (`status`, `briefing`, `tasks`, `events`,
`evidence`); use the CLI for builds, the slot, `keel check` and other work on the machine, and
say so when the MCP server is not connected.

Record the card and the proof where the hooks and the Studio can see them: inside a GraphHelm
execution, as `keel.card` and `keel.proof` signals threaded by `replyTo`; outside one, the card at
`.graphhelm/keel-card.json`. Shape: `docs/keel/RECORDS.md` in the GraphHelm repository.

When lanes coordinate through an execution, the step that writes an identity line also records its
`task.*` signal (`task.claimed`, `task.pr_opened`, `task.review_assigned`, `task.review_verdict`,
`task.merged`), signed as your own lane; one command per step:
`python tools/task-record/task_record.py --lane <you> <step> --issue <N> ...` (GraphHelm checkout). The document shape is
`schemas/task-event.schema.json` in this package; the steps are in `docs/process/DELIVERY.md`,
"Task records". The Runtime refuses a task record signed by another actor.

When the change touches a screen of a journey (a path listed in a step's `screen.scopePaths` in
`.graphhelm/journeys/<contractId>.json`), name the journey in the card (`journeys: [contractId]`,
or a `Journeys:` line in the PR body). With `--events`, `--execution`, `--keyring` and `--key-id`,
`graphhelm keel check` then warns `keel.journey.no_fresh_capture` for each touched screen with no
capture at the head. The warning is advisory and never blocks. Answer it by capturing: a `before`
capture at the base and an `after` capture at the head, linked in the PR body (`docs/process/DELIVERY.md`
in the GraphHelm repository, "Before/after captures for screens"); the Playwright observer's
`--journey <contractId>` mode records the head captures in one run. A screen left uncaptured is
reported as unobserved, not as passed.

The full rules (the card and its bounds, surface counting, the verdict and its causes, the index,
where to spend time): `REFERENCE.md` beside this file. Read it only when this digest does not
answer your question.

## Reuse current evidence before retrieving again

Start with the task's existing capsule, source citations and proof records. Ask for more context
only for a named gap: a missing caller, stale source, incomplete coverage or a new question. Do
not repeat a tool call just to reread an answer already present in the current context. A reference
only replaces content when the receiving agent can resolve it.

Keep reuse scoped to the current task and authorized source generation. A commit id or a TTL alone
does not prove freshness: dirty files, exclusions and permissions can change without a commit.
GraphHelm's Runtime can reuse bounded search results from the same immutable search/reader pair
within one drive; it still reads the selected source and compiles each node's capsule with its own
id and budget. Live searches and unavailable sources keep their existing fallback behavior.

This is the default context policy for every Keel task, not an owner opt-in. Apply it before
retrieval and carry it in delegated briefs; do not ask the owner to request reuse again. Record
any structural retrieval gap and its named source paths in the task card yourself. This does not
expand the task's source permissions or authorize another agent's actor identity.

Use the contained Codebase Memory code-index methods by default when a valid task bundle already
exists or repeated structural retrieval would avoid more work than a focused source search.
For a single known symbol or a cheaper targeted read, use the existing source tools. Never build
an index merely because a session or subagent started. If the provider or a verified executable
pin is unavailable, use a focused source fallback and record the missing capability; do not
install or start a memory service to hide it.

The CLI methods are `graphhelm --json tool code-index build` and
`graphhelm --json tool code-index query`. Both take `--project`, `--staging`, `--bundle`,
`--executable`, `--sha256`, and your own `--actor`; query also takes bounded `--path` arguments.
Resolve the installed CLI/provider pin once per task and retain the parameters in the card.
Use a private bundle outside the source project and a disposable staging directory for each call.
Prefer an already validated result present in context to another tool call. If a tool lookup is
needed, query the same task bundle with the same request and your own stable actor; the CLI
validates freshness before returning `reused: true` and `providerInvocations: 0`.

Pass the question, source binding, resolvable evidence path/citations, coverage, bundle location,
and this policy to subagents. They must preserve their own actor and permissions; cross-actor
cache hits are not promised. Do not pass raw source bodies unnecessarily or treat an evidence
path alone as proof that a receiver can access it. If freshness or coverage cannot be established,
read the missing source. Rebuild refused stale bundles into a new private location only when the
same bounded retrieval gap still justifies it; never sweep or overwrite another session's bundle.

Build once into the card's private immutable bundle, then
reuse that bundle for repeated queries only after its source, store, executable, request, and broker
evidence still validate. A query opens one contained provider call and closes its process tree; it
does not retain a background indexing watcher or an MCP daemon, modify the global Runtime/MCP configuration, or
silently assemble Runtime context ports. A timeout bounds the provider process execution after
preparation; it does not make unbounded filesystem or snapshot preparation safe. Keep partial coverage as partial; refuse stale, timed-out, cancelled, truncated, or otherwise
incomplete process evidence.

Do not launch a per-agent memory server or watcher to retain an answer. An optional external
provider must run through the contained broker, with native process-tree closure evidence on the
host OS. A frontend exiting alone does not prove its daemon closed. Preserve unrelated sessions.
Report actual tool calls and bytes separately from token estimates; reuse is not measured token
savings. Installed skill copies need the normal update before this guidance takes effect there.
