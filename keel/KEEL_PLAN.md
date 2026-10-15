# KEEL_PLAN

Step 1, after the issue and before any code. The planner writes no code. It produces the card
and records it as one `keel.plan` record. Read [`KEEL.md`](../KEEL.md) first.

## How much plan the change needs

| The change touches | The card has |
|---|---|
| Docs, comments, inert config values, one line, tests only | a summary line; a test removal also carries its deletion record |
| Bounded code | the paths, the promise, the proof |
| A screen inside a journey's `scopePaths` | the paths, the promise, and `journeys:` naming every journey touched, replayed green at the head |
| A new public surface (module, type, public function, dependency, route, command, tool, schema field) | the full card, and the surface is named |
| Persisted data, permissions, compatibility, security, effects outside the process, Runtime-affecting config, concurrency, destructive operations | the full card, a journey, and the failure list `KEEL_EXEC.md` asks for isolated tests |

Risk is read from what the change touches, not from its size: one line can remove a permission
check. A display-only config label is a summary-line change; a Runtime timeout or permission
setting needs behavioural proof even if it parses.

When in doubt, take the larger row.

## The card

1. **Promise.** What a user can see change, in one or two sentences. Not what the code does.
2. **Proof.** The end-to-end observer that decides it: the journey id, the CLI command against a
   real Runtime, or the hook run. Name what it asserts. An isolated test is planned only for the
   cases `KEEL_EXEC.md` allows, and its list of failure modes is written here first.
3. **Paths.** Every file the change may touch, as a list of paths. No globs. A change that needs
   more than about a dozen paths is two promises.
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
