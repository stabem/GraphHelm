---
name: graphhelm-guide
description: Use when the user asks how GraphHelm works, how journeys, Keel and the five task steps (plan, implement, prove, review, merge) fit together, or where to start with its plugin, CLI and Runtime.
---

# GraphHelm guide

Read the bundled [GraphHelm guide](../../README.md) and explain only the parts relevant to the user's question. Distinguish shipped behavior from product plans and host discovery from Runtime activation. When a claim depends on the current repository or a live execution, inspect that source before stating it as current.

## Journeys are the proof

GraphHelm develops journey-first (design: `docs/specs/2026-10-07-journey-first-keel-design.md`). A user-visible promise is proven by replaying the journey it touches at the head and seeing its screens, not by a unit test or a status code. Tests are kept for invariants a user cannot see (schemas, digests, concurrency, security). An unavailable observer stays `OBSERVER_MISSING`.

Journeys are flows in `.graphhelm/journeys/<id>.journey.yaml` (schema `graphhelm.journey-flow/1`: screens, edges, paths). `graphhelm journey compile` generates the frozen contracts `<contractId>.json`; never edit those by hand. A flow stays `draft` until the owner approves it in the Studio's Journey tab or with `graphhelm journey approve <id>`; agents do not approve. `graphhelm journey replay <id>` walks an approved flow headless with no model call and records sealed captures and walked transitions; `graphhelm journeys` and the Studio's Journey tab show each step as fresh, stale or unknown at the head. `graphhelm keel check` warns, without blocking, when a change touches a screen with no fresh capture. Step-by-step with real output: `docs/guides/journeys.md`.

## One task, five steps

Every task runs the same steps after its issue, in any agent host, each a skill that names the record it emits:

1. `task-plan`: paths, promise, proof kind (`journey`, `tests`, `both`, `none`), review count and skills, as one `keel.plan` record.
2. `implement`: the change inside the card under `keel`, with cited context; `test-audit` gates any test.
3. `journey-prove`: replay the touched journeys at the head, keep the first failure of every retry.
4. `blind-review`: one assigned reviewer runs the reached tests and `keel check` on the pinned head and posts the verdict.
5. `merge`: the approving reviewer merges the pinned head, reads back what landed, and releases the workspace.

`journey-map` creates journeys for a project that has none; `journey-contract` adds one for a new behavior. `keel plan` and the `task.*` records are still being built; each skill says how to do its step by hand until they land. Skills advise; only GraphHelm's deterministic controls validate and publish operational graph changes.

Installing this plugin exposes guidance. It does not install the CLI, start the Runtime, rewrite host customization, activate extension packages, or certify an execution. The `graphhelm-setup` skill covers installing and starting.

## Prefer the GraphHelm MCP tools

When the `graphhelm` MCP server is connected, read live state through its tools rather than shelling out: `mcp__graphhelm__briefing`, `status`, `events`, `evidence`, `resume` and `memory_status`. If neither is connected, use the `graphhelm` CLI with `--json`, and say which source you used. `graphhelm init` registers a project-aware MCP command; `graphhelm setup --resolve home/.claude.json=register-mcp` can install one user-scope entry that resolves the active project by identity. Explicit `--url` and `--token-file` remain available for a deliberately pinned Runtime.
