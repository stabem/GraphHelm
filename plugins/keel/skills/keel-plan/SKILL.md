---
name: keel-plan
description: "Use when planning a code change under Keel, before writing code: the contract card, the paths in scope, the promise and how it will be proven."
---

How to use this skill:

- First run: if the project has no `KEEL.md` at its root, install Keel into it with
  `python <this plugin>/setup/keel_setup.py <project root>` (the plugin folder is two levels above
  this skill's folder). It copies `KEEL.md` and the `keel/` step files into the project, offline,
  never overwrites an existing one, and adds a marked pointer line to `CLAUDE.md` and `AGENTS.md`.
- Read the project's own `KEEL.md` at its root when one exists, and the `keel/` step files it names.
  Before that step has run, read https://github.com/stabem/GraphHelm/blob/main/KEEL.md and the step
  files under https://github.com/stabem/GraphHelm/blob/main/keel/. The text below is the bundled copy.
- In the text below, `keel/KEEL_PLAN.md`, `keel/KEEL_EXEC.md`, `keel/KEEL_REVIEW.md` and
  `keel/KEEL_MERGE.md` are this plugin's `keel-plan`, `keel-exec`, `keel-review` and `keel-merge`
  skills, and `KEEL.md` is its `keel` skill. `docs/keel/KEEL_SPEC.md` exists only in the GraphHelm
  repository.
- If the `graphhelm` CLI is on PATH, run `graphhelm keel check --diff <base>..<head> --card <card>`
  for the rules marked as measured. If it is not, the reviewer checks those rules by reading and says
  so in the review.
- Nothing here needs a GraphHelm Runtime, MCP server, task records or journeys. Where the text names
  them, they apply only inside GraphHelm: elsewhere, prove the promise with the project's own test
  runner.

<!-- keel source: keel/KEEL_PLAN.md (generated; edit the source, then run tools/keel-plugin/sync_keel_plugin.py) -->

# KEEL_PLAN

Step 1, after the issue and before any code. The planner writes no code. It produces the card
and records it as one `keel.plan` record. Read [`KEEL.md`](https://github.com/stabem/GraphHelm/blob/main/KEEL.md) first.

## How much plan the change needs

| The change touches | The card has |
|---|---|
| Docs, comments, inert config values, one line, tests only | a summary line; a test removal also carries its deletion record |
| Bounded code | the paths, the promise, the proof |
| A screen inside a journey's `scopePaths` | the paths, the promise, and `journeys:` naming every journey touched, replayed green at the head |
| A new public surface (module, type, public function, dependency, route, command, tool, schema field) | the full card, and the surface is named |
| Persisted data, permissions, compatibility, security, effects outside the process, Runtime-affecting config, concurrency, destructive operations | the full card, a journey, and the failure list `KEEL_EXEC.md` asks for |

Risk is read from what the change touches, not from its size: one line can remove a permission
check. A display-only config label is a summary-line change; Runtime-affecting configuration (a
timeout, a permission setting) needs behavioural proof: parsing or shape validation alone is not
proof.

When in doubt, take the larger row.

## The card

1. **Promise.** What a user can see change, in one or two sentences. Not what the code does.
2. **Proof.** The cheapest observer that decides it: the journey id, the CLI command against a
   real Runtime, or the hook run. Name what it asserts, and write the
   failure list here first when `KEEL_EXEC.md` asks for one.
3. **Paths.** Every file the change may touch, as a list of paths. No globs.
4. **What already exists.** The searches you ran for code that already does part of the job, and
   what they found. A new function, type or module is planned only when nothing found fits, and
   the card names who will call it.
5. **Cost.** The cost of the main path in terms of the data that grows (events, tasks, PRs,
   files), for example "O(new events per poll), not O(all events)". A path over growing data
   names its scale check.
6. **Risks.** For each that applies: persisted data or digests, security and permissions,
   compatibility with a running Runtime or an older client, idempotency of every write, external
   calls and their deadlines.
7. **Measured gain.** A change that promises a gain (speed, lines, tokens, requests) measures the
   current number before the plan is accepted, and states the condition it measured under. An
   estimate is written as an estimate.
8. **Complexity.** If a function must become more complex, the card says which one and why.

## Refuse the plan when

- the promise describes code instead of a user-visible result;
- there is no observer, and no `OBSERVER_MISSING` with what is missing;
- a new surface has no named caller;
- the gain is claimed without a measured baseline.
