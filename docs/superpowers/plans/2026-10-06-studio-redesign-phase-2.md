# Studio Redesign Phase 2: Actor Alias and Owner Refusal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Owners can name a bot (`actor_alias`) and refuse an agent's question (`owner_refusal`); the Runtime enforces both as owner-only sealed records and the Studio shows the alias and lets the owner refuse from a question card.

**Architecture:** One new pure validator in the CLI crate (`apps/cli/src/commands/execution/owner_records.rs`) called from the shared signal core `execute_authenticated_inner` in `signal.rs`, right after the existing `documents::validate_owner_signal` call. Every transport (CLI `execution signal`, HTTP `/v1/executions/{id}/signal`, MCP `signal`) reaches that core, so D-039 parity holds by construction. The Studio reads sealed `actor_alias` envelopes the same way `useNativePersonaLinks` reads `native_persona_linked`, and sends both kinds through `RuntimeClient.signal` with a widened `kind` union.

**Tech Stack:** Rust 1.97.1 (`graphhelm-cli`), TypeScript/React/Vitest (`apps/studio`).

**Spec:** `docs/specs/2026-10-05-studio-live-team-and-proven-journeys-design.md` §4.1, §4.3, §5.2, §5.3, §9 phase 2.

## Global Constraints

- Both kinds are owner signals: actor type `Owner`, envelope `source.type == "user"`, sealed keyring present. Anything else is `GHCLI003_SIGNAL_INVALID` before any evidence write or append (same contract as `documents::validate_owner_signal`).
- `actor_alias`: `to` = the actor id; `description` = JSON `{"protocol":"graphhelm-actor-alias-v1","displayName":string,"personaThreadId"?:string}`, unknown fields refused. Refused when `to` is `codex` and when `to` is not the id of an `Agent` actor that has an event in this run's stream. Newest alias per actor wins (read side).
- `owner_refusal`: `replyTo` required = the `signal_id` of a `signal_recorded` event by an `Agent` actor in this run; `description` = JSON `{"protocol":"graphhelm-owner-refusal-v1","reason"?:string}`.
- Bounds: `displayName` trimmed 1..=80 chars, no control chars, not `secret_shaped`; `personaThreadId` 1..=128 chars; `reason` ≤ 2048 chars, not `secret_shaped`; description ≤ 8192 bytes.
- Studio copy: button "Name this bot" (only on bots with no persona and not the shared `codex`), button "Refuse" on question cards. Accessible names "Principal conversation", "Next step", "Refresh request status" unchanged.
- Docs in English. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. An agent credential sending `actor_alias` for itself or another agent → refused (an agent cannot rename another agent, spec §8). Pinned by unit test in Task 1.
2. An alias whose `displayName` is blank or whitespace → refused, never renders an empty bot name. Pinned in Task 1 (Rust) and Task 2 (read side skips it).
3. A refusal whose `replyTo` names a nonexistent or owner-sent signal → refused, so a refusal cannot be a dangling record. Pinned in Task 1.
4. Two aliases for the same actor → the newer (higher sequence) wins. Pinned in Task 2.
5. A Refuse click while the Runtime refuses the signal → the question stays open and the error shows. Pinned in Task 2.

---

### Task 1: Runtime validation of `actor_alias` and `owner_refusal`

**Files:**
- Create: `apps/cli/src/commands/execution/owner_records.rs` (validator + unit tests)
- Modify: `apps/cli/src/commands/execution/mod.rs` (add `mod owner_records;`)
- Modify: `apps/cli/src/commands/execution/signal.rs:~305` (call the validator after `documents::validate_owner_signal`, reading the stream history via `store.read_replay_stream(&scope, &stream)`)
- Test: `apps/cli/tests/owner_records_cli.rs` (CLI integration through `graphhelm execution signal`)

**Interfaces:**
- Produces: `pub(super) fn validate_owner_record(envelope: &serde_json::Value, actor: &PersistedActor, sealed: bool, history: &[EventEnvelope]) -> Result<(), Failure>`; returns `Ok(())` for any other kind.

- [ ] Step 1: Unit tests in `owner_records.rs` (`#[cfg(test)]`), building `history` from hand-made `EventEnvelope`s or by calling the validator with a minimal history helper: (a) agent actor + `actor_alias` → Err; (b) owner + `to:"codex"` → Err; (c) owner + `to` absent from history → Err; (d) owner + `to` of a present agent → Ok; (e) unsealed → Err; (f) blank displayName / unknown field → Err; (g) `owner_refusal` without `replyTo` → Err; (h) agent-sent refusal → Err; (i) `replyTo` naming an unknown signal id → Err; (j) valid refusal → Ok.
- [ ] Step 2: `cargo +1.97.1 test -p graphhelm-cli owner_records` fails (module missing).
- [ ] Step 3: Implement with `#[serde(deny_unknown_fields, rename_all = "camelCase")]` structs; reuse `super::signal_invalid` and `graphhelm_runtime::context::secret_shaped`.
- [ ] Step 4: Wire into `signal.rs` before the `node_delivery` block so nothing is written on refusal.
- [ ] Step 5: Integration test `owner_records_cli.rs` following `documents_cli.rs`: start a run, record an agent-free run → alias for absent id refused with `GHCLI003_SIGNAL_INVALID` and no `--evidence-out` file; `codex` alias refused; refusal without `replyTo` refused; an owner `operator_note` signal exists then a refusal whose `replyTo` names that owner signal is refused.
- [ ] Step 6: `cargo +1.97.1 fmt --all -- --check`, `cargo +1.97.1 clippy -p graphhelm-cli --all-targets --locked -- -D warnings`, `cargo +1.97.1 test -p graphhelm-cli`. Commit `feat(runtime): owner-only actor_alias and owner_refusal signal kinds (#311)`.

### Task 2: Studio alias, Name this bot, Refuse, skill note

**Files:**
- Modify: `apps/studio/src/runtime/client.ts:~1785` (kind union adds `"actor_alias" | "owner_refusal"`, both owner-only like `native_persona_linked`)
- Modify: `apps/studio/src/components/panel.tsx` (new `useActorAliases(events, executionId, openEvidence): Record<string,string>` beside `useNativePersonaLinks`; newest sequence wins; validates protocol, `envelope.to`, displayName)
- Modify: `apps/studio/src/App.tsx` (replace `NO_ALIASES` with the hook; `nameBot(actorId, name)` and `refuse(item, reason)` callbacks; pass to `TeamCanvas` and `QuestionCards`)
- Modify: `apps/studio/src/components/team-canvas.tsx` (optional `onNameBot?: (bot: Bot) => void`; "Name this bot" button when `bot.actorId !== null && bot.role === null && !bot.native && !bot.shared`)
- Modify: `apps/studio/src/components/beacon.tsx` (`onRefuse` prop; "Refuse" button)
- Modify: `examples/chat-surface/claude-code-plugin/skills/leave-records/SKILL.md` (a refusal is final for that question)
- Test: `team.test.ts` (alias renames bot), `beacon.test.tsx` (Refuse calls onRefuse), `needs-you.test.ts` (an owner `owner_refusal` with replyTo closes the question and the lit count drops), `client.test.ts` (kinds sent, non-owner rejected), `App.test.tsx` only if wiring needs it.

- [ ] Step 1: failing tests; Step 2: implement; Step 3: `npx tsc -b` and `npx vitest run` in `apps/studio`; Step 4: commit `feat(studio): name this bot and refuse a question (#311)`.
