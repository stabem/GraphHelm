---
name: keel
description: "Work under the Keel development model: load a contract card instead of the tree, spend a declared write surface, prove a promise against a named defect, and read the watchdog's verdict as a routing state. Use by default for code work and context retrieval in projects that adopted GraphHelm; no explicit user invocation is needed."
---

# Keel, in brief

This digest is what loads by default; it carries every binding rule. Use only as much Keel as the
change needs: a docs or one-line change needs nothing beyond the task record.

1. Stay inside the card and search on purpose: open what it names first, then search for the
   specific symbol or caller. Change only the paths the task needs; a file the task does not reach
   is out of scope, even when it has the same defect or looks untidy.
2. Add nothing unrequested: no new helper, type, module, flag, dependency, file or test the task
   does not need. Extend an existing body or reuse a proven symbol before adding a type; a new
   interface needs a real caller (producer and consumer may land together).
3. Keep existing behaviour working. Before editing shared code, find its other callers and the
   behaviour they rely on; a fix that breaks a neighbour is not a fix.
4. A new test must name the defect and fail on the parent (without your change). A test that is
   green before the fix proves nothing; do not add it. A new or changed test states its cost (run
   time, what it needs) and passes the `test-audit` skill's four-question gate.
5. Prove the promise with the smallest check that observes it. A proxy (another OS, a mock, an
   emulator, a cross-compile, a test compiled out on this host) is not an observation.
   Report passed, failed, skipped and unobserved separately; a later green never erases an earlier red.
6. When you cannot observe the promise on this host, change nothing, say what is missing, and end
   with `OBSERVER_MISSING: <what is missing>`. Never write "verified" for what you did not observe.
7. Stop when the promise is proven. Do not refactor, reformat or sweep beyond it.
8. Removing a test needs the `test-audit` skill's deletion record naming the observer that still
   covers its obligation. No record, no removal.

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
