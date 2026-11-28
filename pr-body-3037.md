Session: Ronda Keel + GraphHelm (zuvemo e vou-por-ai) [local_143fe79a] (keel-author subagent) · Head: 99ac8a29

## Summary

`keel check` now counts a declared skill as loaded only when a `skill.loaded` record made by the lane the identity line's `[ref]` names exists in this task. Before, it took the union of every record of the execution, any lane, any day.

- Match: the ref equals the record's actor, session id or agent id (the three the recorder writes; no one of them is proven to equal the ListAgents ref by construction, so all three are accepted).
- Bound for "this task" (sequence in the execution, no clock): the lane's newest `task.claimed` record; with none, the first record of the session of the lane's newest `skill.loaded` record. Chosen because it needs no issue number, exists with zero task records, is deterministic, and a rebase does not move it (a merge-base time would).
- A ref that is missing, or that no event of the execution carries: one warning `keel.card.identity_ref_unknown` at `/card/identity`, and no per-skill warning (the skills cannot be attributed).
- No binding, or events but no `skill.loaded`: still `skillsObserver: unobserved`, no finding (#3013). Prose and "not used" lines still declare nothing (#3017).

Limits (also in `docs/keel/KEEL_CHECK.md`): the recorder is idempotent per session and skill, so a session that loads one skill for two tasks leaves one record and the second task can warn; skills loaded before the claim warn. Both are warnings, not blocks.

Closes #3037

## Card

Paths:
- core/policy/src/keel.rs
- core/policy/tests/keel.rs
- apps/cli/src/commands/keel_skills.rs
- apps/cli/src/commands/keel.rs
- apps/cli/tests/keel_check.rs
- docs/keel/KEEL_CHECK.md

New public items: `SkillRecord`, `SkillHistory`, `identity_ref` (core/policy); `check_skills_loaded` takes the history and the ref instead of a name set.

## Skills usadas

keel:keel, keel:keel-exec

Advisor: consulted (before the plan was fixed)

## Tests run

- `cargo +1.97.1 test --locked -p graphhelm-policy --test keel`: 30 passed.
- `cargo +1.97.1 test --locked -p graphhelm-cli --test cli keel_check`: 44 passed.
- New CLI test `a_declared_skill_must_be_loaded_by_the_named_lane_during_this_task` on the parent commit (test file copied onto origin/main): FAILED (`left: [] right: ["keel.card.skill_not_loaded"]`, the other-lane case). It covers other lane, older task, same lane during task, no claim, ref = session id, unknown ref.
- New policy tests do not compile on the parent (the function signature changed), so they are red there by construction.
- Done-when items that pass on the parent by nature: same lane during this task, unobserved with no binding or no recorder, prose lines. Their existing tests stay green unchanged in assertions; only the stub Runtime gained the `actor` and evidence `session` fields the real route sends.
- `cargo +1.97.1 fmt --check -p graphhelm-policy -p graphhelm-cli`: ok.
- `cargo +1.97.1 clippy --locked -p graphhelm-policy -p graphhelm-cli --all-targets -- -D warnings`: ok.
- Source guards `-p graphhelm-process --test no_console_window --test core_uses_only_fs`: 2 and 10 passed.
- `python tools/diff-hygiene/diff_hygiene.py --card .graphhelm/keel-card.json`: passed @ 99ac8a29.

Break-it-once: the parent-commit run above (old union logic) turns the new test red; not repeated separately.

Not run: the full reached plan (nightly on main). PR 3034 touches the same three files; hunks are local.

## Security notes

Reads only the events and evidence the check already read, plus the actor id and evidence lines of `skill.loaded` records. No new network surface, no secrets, no new reply text with URLs or paths.

## Dependency notes

None.

## Rollback

Revert the commit; `keel check` returns to the execution-wide union.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
