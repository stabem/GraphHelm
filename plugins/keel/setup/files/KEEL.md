# KEEL

Keel is how code is built here. It is a pattern, not a review style: every agent that plans,
writes, reviews or merges code follows it, and every rule the machine can measure without doubt
blocks the merge.

Load this file, then the file for your step:

| Step | File | Who loads it |
|---|---|---|
| Plan | [`keel/KEEL_PLAN.md`](keel/KEEL_PLAN.md) | the planner, before any code |
| Execute | [`keel/KEEL_EXEC.md`](keel/KEEL_EXEC.md) | the implementer, while writing and proving |
| Review | [`keel/KEEL_REVIEW.md`](keel/KEEL_REVIEW.md) | the single blind reviewer |
| Merge | [`keel/KEEL_MERGE.md`](keel/KEEL_MERGE.md) | the approving reviewer, at the merge |

An orchestrator that starts a subagent for a step passes it this file and that step's file, and
nothing of its own reasoning. The reasons behind the rules live in
[`docs/keel/KEEL_SPEC.md`](docs/keel/KEEL_SPEC.md); no rule is written in two places.

## What Keel optimises

1. **Read cost.** An agent should answer a task by reading one card and the few files it names.
   A file too big to read whole and a change spread over too many small files both waste it.
2. **Correct behaviour for the user.** A change is done when an observer proves the promise a
   user can see, and leaves a receipt anyone can replay.
3. **Code that stays cheap to change.** No duplicate, no dead code, no hidden coupling, nothing
   slower than the data needs.

## The values

Each value has a rule id and a mechanism. A value without a working mechanism is reviewed by hand
and listed as `to build` until its check exists.

| Id | Value | The rule | Mechanism | Status |
|---|---|---|---|---|
| `keel.cohesion.split` | Cohesion | A file whose code falls into groups that never call each other and share no data holds two responsibilities, and is split. There is no line limit. | symbol graph per file | to build |
| `keel.cohesion.merge` | Cohesion | Files that almost always change in the same commit are one responsibility spread out, and are merged. | git co-change history | to build |
| `keel.deep` | Deep modules | A module has a small interface and does its work inside. Do not split a body into many tiny functions or layers: every hop is a file or a symbol the model has to open. | review | review |
| `keel.purpose` | Read cost | Every source file starts with one line saying what it is for, so an agent can decide relevance without reading the file. | first-line check | to build |
| `keel.comments` | Read cost | A comment says what the code cannot: why, a constraint, a trap. No comment that restates the code, no commented-out code, no history in comments (issue numbers, dates, "added for"): git holds history. | lint for commented-out code and history markers | to build |
| `keel.slice` | Cohesion | A feature's code, types and tests live together in a folder named after the feature, so one search finds all of it. | review | review |
| `keel.dup` | No duplication | Before creating a function, search for one that does the job. A second copy of the same logic keeps the same name and links to the first; the third copy is extracted. | clone count over the diff | to build |
| `keel.dead` | No dead code | A function, type or export that nothing reaches is deleted, not kept "for later". | reachability from entry points | to build |
| `keel.deps` | Dependency direction | Dependencies point inward: core never imports a host, an app or the Studio. | dependency tag matrix | to build |
| `keel.names` | Searchable names | A name says what the thing does. No `Manager`, `Util`, `Helper`, `Service`, `Common` or `Misc`. | `graphhelm keel check` (added lines) | blocks |
| `keel.names.one` | Searchable names | One concept has one name in every layer (no field renamed at a boundary), and no table, route, event or key name is assembled at run time. | lint on built identifiers and renaming maps | to build |
| `keel.names.no_bare_flag` | Searchable names | A call never passes a bare `true`, `false` or `None` whose meaning lives in the callee: use an enum, a named argument or two named functions. | `graphhelm keel check` (added lines) | blocks |
| `keel.near_dup` | No duplication | Two functions that look alike but behave differently are merged or renamed so the difference is in the name. | similarity detector | to build |
| `keel.explicit` | No hidden magic | Control flow and checks are visible where they apply: no decorator, injection, inheritance or framework hook that hides what runs, and a permission check sits on the code it protects. | review, plus a deny-list of named features | to build |
| `keel.types.boundary` | Explicit types | A module boundary has explicit types: no `any`, untyped map or stringly-typed payload in an exported signature. | lint on exported signatures | to build |
| `keel.deps.fewer` | Fewer dependencies | A small amount of own code beats a new dependency. A new dependency is named in the card and justified. | lockfile diff | to build |
| `keel.rel.no_panic` | Reliability | Production code does not panic, `unwrap` or throw an uncaught error: every failure that can happen becomes a result with a code and a reason. `expect` is allowed on an invariant the code proves, with a message naming that invariant. | `graphhelm keel check` (added lines) | blocks |
| `keel.rel.deadline` | Uptime | Every call that leaves the process (network, disk wait, child process, lock) has a deadline. | `graphhelm keel check` (added lines) | blocks |
| `keel.rel.no_swallow` | Reliability | No error is ignored in silence: it is handled, or it travels up with its reason. | `graphhelm keel check` (added lines) | blocks |
| `keel.rel.no_impossible` | Reliability | No handling for a case that cannot happen. Guard what can fail, and let the type system rule out the rest. | review | review |
| `keel.errors.actionable` | Actionable errors | Every error says what went wrong and how to fix it, with a valid example where one helps, and names things by readable names, not raw ids. | lint for bare-code messages | to build |
| `keel.rel.atomic` | Reliability | A file or a state record is written atomically: a crash in the middle leaves the old version, never half of the new one. | shared atomic writer, raw write lint | to build |
| `keel.idem` | Idempotency | Repeating a write gives the same final state. New interfaces require a request key; an interface that cannot change carries a run-twice end-to-end test. | route and command inventory | to build |
| `keel.complexity` | Less complex | No change makes a function more complex (more branches, loops and early exits) unless the card says why. There is no maximum, only no regression. | per-function complexity diff | to build |
| `keel.bigo.declared` | Big O | The plan states the cost of the main path in terms of the data that grows. | card field | to build |
| `keel.bigo.scale` | Big O | A path over growing data (events, tasks, PRs, files) has an end-to-end scale check at N and 10N; time that grows far faster than 10x blocks. | scale harness | to build |
| `keel.bigo.patterns` | Big O | Known slow shapes are refused: a search or sort inside a loop over the same data, re-reading a whole store per request, an unbounded cache. | pattern lint | to build |
| `keel.commit.kind` | Small reviewable steps | A commit is either structural (moves, renames, extracts, with tests unchanged) or behavioural, never both. Structure goes first. | commit label, tests unchanged across a structural commit | to build |
| `keel.tool.quiet` | Read cost | A check prints nothing when it passes and everything that matters when it fails, and never hangs. | bytes printed by a green run | to build |
| `keel.cause` | No programming by coincidence | A fix names the root cause, with the evidence, before the change. Trying edits until the tests go green is refused. | a `Cause:` line in the pull request | to build |
| `keel.scope` | Write surface | A change touches only the paths its card lists. | `graphhelm keel check --card` | blocks |
| `keel.test.e2e_first` | Proof | Prefer the observer closest to the user-visible promise when it is affordable; see `KEEL_EXEC.md`. | proof kind in the plan record | to build |
| `keel.flow.self_report` | Proof | A delivery-flow step that should have run on its own and did not (workaround, stale record, uncaught check, failed tool) gets its own issue at once, after a search for a duplicate; see `KEEL_EXEC.md`. | review | review |

## Talking to GraphHelm

- **State goes through the GraphHelm MCP tools.** Read the run with `status`, `briefing`, `tasks`,
  `events` and `evidence`, never by scripting the CLI or HTTP for the same answer. The reply is
  already structured, and the server already knows the Runtime and the actor.
- **Task records** (`claimed`, `pr_opened`, `review_verdict`, `merged`) are written with
  `tools/task-record/task_record.py --lane <you>` until the MCP can sign a record as the lane
  (#1407). The Runtime refuses a record signed by anyone but the lane.
- **The CLI is for work on the machine:** builds and the slot (`graphhelm workspace slot`,
  `tools/reached-tests/run_reached.py`), `graphhelm keel check`, setup and anything that runs for
  minutes or touches local files.
- When the MCP server is not connected, use the CLI with `--json` and say so.

## Force and exceptions

- A rule with a working mechanism blocks the merge. A rule marked `to build` is checked by the
  reviewer against this table until its mechanism lands.
- An exception is written in the code, next to what it excuses: the rule id, the reason, and an
  expiry date or issue. The blind reviewer accepts or refuses it, and the list can only shrink.
  Each mechanism prints the exceptions it honoured once it lands (#1404 is the first).
- There is no penalty beyond the block. An agent that breaks a rule fixes the change.
- A rule that is wrong is fixed in its own pull request, never worked around in a feature.

## In other projects

Planned, not built yet: `graphhelm setup` will install this file and the `keel/` folder in a
project, and a project will switch a single rule off in its Keel configuration, with a reason,
while the check reports every rule it skipped. The values are the same everywhere. Until then,
a project without these files follows the `keel` skill's digest.
