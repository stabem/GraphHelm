# Studio Redesign Phase 1 (Team, Beacon, Question Cards, Handover) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Studio's Overview / Free canvas run view with a four-column live-team layout (Projects · Chat · Team canvas · Right panel), a needs-you beacon with question cards, and a "While you were away" handover, using only data the Runtime already serves.

**Architecture:** Four pure derivations (`runtime/team.ts`, `runtime/needs-you.ts`, `runtime/handover.ts`, `runtime/threads.ts`) run over the existing event tail and opened envelopes, each with fixture tests. Five presentational components (`team-canvas.tsx`, `beacon.tsx`, `chat-column.tsx`, `handover-card.tsx`, `right-panel.tsx`) render them. `App.tsx` wires them in place of `Board` (run view), `WorkOverview`, the attention block, the topstrip and the JEV box; `MainChat` keeps its native-request gating unchanged and is mounted inside the Chat column.

**Tech Stack:** React 19, TypeScript 7 (`strict`, `noUnusedLocals`), Vite 8, Vitest 4 (jsdom, `pool: "threads"`, `maxWorkers: 1`), Testing Library, lucide-react icons. No new dependency.

**Spec:** `docs/specs/2026-10-05-studio-live-team-and-proven-journeys-design.md` (Phase 1 = §4.1–§4.6, §9, §10 item 1, and the phase-1 doc lines of §8).

## Global Constraints

- Studio only: every change is under `apps/studio/` or `docs/`. No Runtime, schema, CLI or MCP change.
- No new polling. Everything derives from the existing 4 s incremental events poll (`eventList`), `useEnvelopes`, `usePersonas`, `useNativePersonaLinks`, `claudeTaskRead`, `status`, and `MainChat`'s native request ledger.
- Accessible names that must survive: `"Principal conversation"` (complementary landmark), `"Next step"`, `"Refresh request status"` — `docs/acceptance/studio-main-chat-journey-2026-10-03.json` must pass unchanged.
- `MainChat` gating is kept unchanged: pending requests block team sends; the composer is locked until the ledger is read.
- Bot state is derived from records only: `working` = a record in the last 15 minutes; `quiet` label reads `"No new record for N min"`, never "stuck"; `waiting_for_you` = open question to `studio-operator` or an assigned node in `waiting_input`; `done` = its last task completed and nothing newer.
- A bot is a persona (`persona_created` / `native_persona_linked`), an actor with an owner alias, or an actor with a record in the last 2 hours; everything else folds into "N other recorders".
- The shared `codex` actor is never mapped to a thread; it renders as `"Codex (shared)"` with the note that its records cannot be attributed to one chat.
- Tasks come only from `agent_task_created` / `agent_task_completed` records (via `readClaudeTasks`) and graph nodes assigned to the bot — never from note text.
- Beacon `unknown` ("Can't tell: the Runtime is not answering") must never render as dark.
- Team lines animate only while the pair exchanged a record in the last 60 seconds; `prefers-reduced-motion` turns all motion off.
- Handover `fromSeq` is stored per project and run in `localStorage`; it advances only on **Got it** or after the live view has been visible for 10 seconds; missing storage means no card. The card shows when the gap is ≥ 15 minutes **and** ≥ 20 events.
- Phase 2 is out: no **Refuse** button, no "Name this bot"; the aliases map passed to `teamModel` is always the empty constant `NO_ALIASES`.
- Bot-to-bot tabs are labelled "recorded messages" (NATIVE_CHAT_BRIDGE rule).
- Jev **Use** fills the composer and never sends.
- Every class a component renders needs a rule in `styles.css` (`styles.test.ts` "has a rule for every class the components render"); every `var(--x)` read must be defined.
- Docs in English only.

## Review Focus

1. **Stale connection with open items** — the beacon must read "Can't tell…" (unknown), not a lit count and never dark, while the Runtime is not answering. Pinned in Task 3 (`needsYou` stale test) and Task 7 (Beacon render test).
2. **Sealed envelopes not opened yet** — a bot whose notes are still sealed must still appear (counted from the log) with `"No note yet"`, and the Chat column must show "Opening N sealed records…" instead of dropping them. Pinned in Task 2 (`doingNow` sealed test) and Task 8 (opening count test).
3. **A question answered by another agent, not the owner** — must stay open (ledger rule). Pinned in Task 3 (`needsYou` keeps a question another agent replied to).
4. **localStorage unavailable / corrupted** (private window, quota, junk JSON) — no handover card, positions fall back to the default row, nothing throws. Pinned in Task 4 (`readLastSeen` throws → null; junk → null) and Task 6 (junk positions → default).
5. **Run switch while a handover or answer is armed** — the card and the "Answering X" chip must belong to the run on screen; switching runs must not show run A's handover over run B. Pinned in Task 11 (App test: switching runs hides the handover card).

---

## File Structure

| File | Responsibility |
|---|---|
| Modify `apps/studio/src/graph/ledger.ts` | `EnvelopeRecord` gains optional `recommendations`; new `recommendations_of()` reads them from a sealed graph-signal envelope. |
| Modify `apps/studio/src/components/panel.tsx` | `useEnvelopes` stores `recommendations` beside `to`/`replyTo`/`text`. |
| Create `apps/studio/src/runtime/team.ts` | `teamModel()`, `teamLinks()`, bot state rules. |
| Create `apps/studio/src/runtime/needs-you.ts` | `needsYou()` → beacon state + items. |
| Create `apps/studio/src/runtime/handover.ts` | `buildHandover()`, `shouldShowHandover()`, last-seen storage. |
| Create `apps/studio/src/runtime/threads.ts` | `chatThreads()`, `unreadCounts()`, `parseMention()`, `namesOf()`, `describeActivity()`, `sealedNotesPending()`. |
| Create `apps/studio/src/components/graph-file-row.tsx` | The graph-file / fixture-file row, extracted from `board.tsx` so the Team canvas and the draft board share it. |
| Create `apps/studio/src/components/team-canvas.tsx` | Bots, tasks, live lines, pan/drag, persisted positions, unassigned steps, other recorders. |
| Create `apps/studio/src/components/beacon.tsx` | `Beacon` and `QuestionCards`. |
| Create `apps/studio/src/components/handover-card.tsx` | "While you were away" card. |
| Create `apps/studio/src/components/right-panel.tsx` | Journeys (empty state), Before / after (empty state), What just happened. |
| Create `apps/studio/src/components/chat-column.tsx` | Thread tabs, messages, Jev card, record composer, native composer slot. |
| Modify `apps/studio/src/components/main-chat.tsx` | Drop the JEV box and the Request status box heading/details; add `recipientId`, `seed`, `refreshNonce`, `onRequestsChange`. |
| Modify `apps/studio/src/components/board.tsx` | Drop Overview toggle, `WorkOverview`, regions, ink tools, strokes, notes, cast and talk bubbles; use `GraphFileRow`. Board remains the draft sheet only. |
| Delete `apps/studio/src/components/work-overview.tsx`, `work-overview.test.tsx` | Replaced by Team tab + handover (§9). `isEntryNode` / `isFirstEntryNode` move to `graph/model.ts`. |
| Modify `apps/studio/src/App.tsx` | New derivations, new layout, top bar + beacon, removals, resume selects the Team tab by state. |
| Modify `apps/studio/src/styles.css`, `apps/studio/src/styles.test.ts` | Rules for new classes, four-column layout, breakpoints 1100 / 768 px, reduced motion; remove overview rules and their guard. |
| Modify `docs/ux/STUDIO_SPEC.md`, `docs/ux/STUDIO_MVP.md`, `docs/studio/NATIVE_CHAT_BRIDGE.md` | §8 phase-1 lines. |

All commands below run from `apps/studio` (`cd apps/studio` once per shell).

---

### Task 1: Envelope recommendations

**Files:**
- Modify: `apps/studio/src/graph/ledger.ts` (type `EnvelopeRecord` at ~line 96; add function after `address_of`)
- Modify: `apps/studio/src/components/panel.tsx` (`useEnvelopes`, ~line 869–925)
- Test: `apps/studio/src/graph/ledger.test.ts`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `export type EnvelopeRecord = Record<number, { to: string | null; replyTo: string | null; text: string; recommendations?: string[] }>;`
  - `export function recommendations_of(text: string, mediaType: string): string[]` — at most 4 strings, each trimmed, 1–120 chars, duplicates dropped; `[]` for non-JSON, malformed JSON or a missing/ill-typed field.
  - `useEnvelopes(...)` return type becomes `EnvelopeRecord` (import it from `../graph/ledger`).

- [ ] **Step 1: Write the failing test** — append to `apps/studio/src/graph/ledger.test.ts` (add `recommendations_of` to the existing import from `./ledger`):

```ts
describe("recommendations_of", () => {
  it("reads the graph-signal recommendations as choice labels", () => {
    const sealed = JSON.stringify({ to: "studio-operator", description: "Merge now?", recommendations: ["Wait", "Merge", " Merge ", ""] });
    expect(recommendations_of(sealed, "application/json")).toEqual(["Wait", "Merge"]);
  });
  it("caps the list at four and drops labels longer than 120 characters", () => {
    const sealed = JSON.stringify({ recommendations: ["a", "b", "x".repeat(121), "c", "d", "e"] });
    expect(recommendations_of(sealed, "application/json")).toEqual(["a", "b", "c", "d"]);
  });
  it("yields nothing for plain text, broken JSON or a wrong type", () => {
    expect(recommendations_of("Merge?", "text/plain")).toEqual([]);
    expect(recommendations_of("{broken", "application/json")).toEqual([]);
    expect(recommendations_of(JSON.stringify({ recommendations: "Merge" }), "application/json")).toEqual([]);
    expect(recommendations_of(JSON.stringify({ recommendations: [1, null] }), "application/json")).toEqual([]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/graph/ledger.test.ts`
Expected: FAIL — `recommendations_of is not a function` / not exported.

- [ ] **Step 3: Implement** — in `ledger.ts`, replace the `EnvelopeRecord` line and add the function after `address_of`:

```ts
/** The envelope a signal's sealed evidence carried, keyed by the signal event's sequence.
 * `recommendations` is the graph-signal envelope's own field (schemas/graph-signal.schema.json);
 * absent when the envelope carried none or was not JSON. */
export type EnvelopeRecord = Record<number, { to: string | null; replyTo: string | null; text: string; recommendations?: string[] }>;

const MAX_RECOMMENDATIONS = 4;
const MAX_RECOMMENDATION_LENGTH = 120;

/** The choices a question offers, read from the sealed envelope's `recommendations`. Anything
 * that is not a short, non-empty string is dropped rather than rendered as a button. */
export function recommendations_of(text: string, mediaType: string): string[] {
  if (!mediaType.includes("json")) return [];
  try {
    const parsed: unknown = JSON.parse(text);
    if (parsed === null || typeof parsed !== "object") return [];
    const list = (parsed as { recommendations?: unknown }).recommendations;
    if (!Array.isArray(list)) return [];
    const out: string[] = [];
    for (const entry of list) {
      if (typeof entry !== "string") continue;
      const label = entry.trim();
      if (label.length === 0 || label.length > MAX_RECOMMENDATION_LENGTH || out.includes(label)) continue;
      out.push(label);
      if (out.length === MAX_RECOMMENDATIONS) break;
    }
    return out;
  } catch {
    return [];
  }
}
```

In `panel.tsx` `useEnvelopes`: import `recommendations_of` and `type EnvelopeRecord` from `../graph/ledger`; change the return type and both `map` types to `EnvelopeRecord`; build `opened` as:

```ts
const recommendations = recommendations_of(content.content, content.mediaType);
const opened = {
  ...address_of(content.content, content.mediaType),
  text: readable_content(content.content, content.mediaType),
  ...(recommendations.length > 0 ? { recommendations } : {}),
};
```

and extend the unchanged-check to `&& (before.recommendations ?? []).join("\u0000") === (opened.recommendations ?? []).join("\u0000")`.

- [ ] **Step 4: Run tests and typecheck**

Run: `npx vitest run src/graph/ledger.test.ts src/components/panel.test.ts && npx tsc -b`
Expected: PASS, no type errors.

- [ ] **Step 5: Commit**

```bash
git add src/graph/ledger.ts src/graph/ledger.test.ts src/components/panel.tsx
git commit -m "feat(studio): read question recommendations from sealed envelopes (#301)"
```

---

### Task 2: Team model (`team.ts`)

**Files:**
- Create: `apps/studio/src/runtime/team.ts`
- Test: `apps/studio/src/runtime/team.test.ts`

**Interfaces:**
- Consumes: `EnvelopeRecord` (Task 1); `GraphModel`, `GraphNode` from `../graph/model`; `ClaudeTaskReadModel` from `./team-tasks`; `NativeChatSummary`, `RuntimeEvent` from `./types`; `hueOf` from `../components/format`.
- Produces:

```ts
export const SHARED_CODEX_ACTOR = "codex";
export const BOT_RECENT_MS: number;   // 2 h
export const WORKING_MS: number;      // 15 min
export const LIVE_LINK_MS: number;    // 60 s
export type BotState = "working" | "waiting_for_you" | "quiet" | "done";
export interface BotTask { id: string; title: string; done: boolean; source: "task_record" | "graph_node"; nodeId: string | null; sequence: number; doneSequence: number | null }
export interface Bot { key: string; actorId: string | null; name: string; hue: number; role: string | null; doingNow: string; lastRecordAt: string | null; lastSequence: number; state: BotState; quietMinutes: number | null; shared: boolean; native: boolean; tasks: BotTask[] }
export interface OtherRecorder { actorId: string; count: number; lastRecordAt: string | null }
export interface TeamModel { bots: Bot[]; otherRecorders: OtherRecorder[] }
export interface TeamLink { a: string; b: string; count: number; lastAt: string | null; live: boolean }
export interface TeamInput { events: RuntimeEvent[]; envelopes: EnvelopeRecord; personas: Record<string, string>; nativeLinks: Record<string, { chat: NativeChatSummary; charter: string; nodeId: string }>; aliases: Record<string, string>; model: GraphModel | null; claudeTasks: ClaudeTaskReadModel | null; waitingAskers: ReadonlySet<string>; now: number }
export function teamModel(input: TeamInput): TeamModel;
export function botKeyOf(bots: Bot[], id: string | null): string | null;
export function teamLinks(events: RuntimeEvent[], envelopes: EnvelopeRecord, bots: Bot[], now: number): TeamLink[];
```

- [ ] **Step 1: Write the failing test** — create `apps/studio/src/runtime/team.test.ts`. The fixture follows the `lojakit-levas-2-a-5` journal shape (a coordinator persona, three kit agents, the shared `codex` actor, a crowd of short-lived merge actors from hours ago) with ids replaced:

```ts
import { describe, expect, it } from "vitest";

import type { GraphModel } from "../graph/model";
import type { EnvelopeRecord } from "../graph/ledger";
import type { ClaudeTaskReadModel } from "./team-tasks";
import type { RuntimeEvent } from "./types";
import { teamLinks, teamModel, type TeamInput } from "./team";

const NOW = Date.parse("2026-10-05T12:00:00Z");
const minutesAgo = (m: number) => new Date(NOW - m * 60_000).toISOString();

function record(sequence: number, actorId: string, at: string, kind = "operator_note"): RuntimeEvent {
  return {
    sequence, kind: "signal_recorded", payload: { kind, signalId: `sig-${sequence}` }, occurredAt: at,
    actorId, actorType: actorId === "studio-operator" ? "owner" : "agent",
    idempotencyKey: null, eventId: `e-${sequence}`, evidenceRefs: [`ev-${sequence}`],
  };
}

function input(overrides: Partial<TeamInput> = {}): TeamInput {
  return {
    events: [], envelopes: {}, personas: {}, nativeLinks: {}, aliases: {}, model: null,
    claudeTasks: null, waitingAskers: new Set(), now: NOW, ...overrides,
  };
}

// 40 merge actors that recorded five hours ago, then the live crew.
const oldCrowd = Array.from({ length: 40 }, (_, i) => record(i + 1, `merge-${i}`, minutesAgo(300)));
const live: RuntimeEvent[] = [
  record(100, "coordinator", minutesAgo(3)),
  record(101, "kit-1", minutesAgo(2)),
  record(102, "kit-2", minutesAgo(50)),
  record(103, "kit-3", minutesAgo(5)),
  record(104, "codex", minutesAgo(4)),
];
const envelopes: EnvelopeRecord = {
  100: { to: null, replyTo: null, text: "Splitting Leva 5 into three slices\nmore detail" },
  101: { to: "coordinator", replyTo: null, text: "Cart page done" },
  103: { to: "studio-operator", replyTo: null, text: "Merge now or wait?" },
};

describe("teamModel", () => {
  it("makes bots of personas and recent actors and folds the rest into other recorders", () => {
    const team = teamModel(input({ events: [...oldCrowd, ...live], envelopes, personas: { coordinator: "Coordinator of Leva 5\nKeeps the plan" } }));
    expect(team.bots.map((bot) => bot.key)).toEqual(["coordinator", "codex", "kit-1", "kit-2", "kit-3"]);
    expect(team.otherRecorders).toHaveLength(40);
    expect(team.otherRecorders[0]).toEqual({ actorId: "merge-0", count: 1, lastRecordAt: minutesAgo(300) });
  });

  it("names the shared codex actor and never maps it to a thread", () => {
    const codex = teamModel(input({ events: live })).bots.find((bot) => bot.key === "codex")!;
    expect(codex.name).toBe("Codex (shared)");
    expect(codex.shared).toBe(true);
    expect(codex.native).toBe(false);
  });

  it("says what each bot is doing from the first line of its newest opened note", () => {
    const team = teamModel(input({ events: live, envelopes }));
    expect(team.bots.find((bot) => bot.key === "coordinator")!.doingNow).toBe("Splitting Leva 5 into three slices");
    // kit-2's note is still sealed: it is a bot (counted from the log) with no words yet.
    expect(team.bots.find((bot) => bot.key === "kit-2")!.doingNow).toBe("No note yet");
  });

  it("derives state from records only", () => {
    const team = teamModel(input({ events: live, envelopes, waitingAskers: new Set(["kit-3"]) }));
    const by = (key: string) => team.bots.find((bot) => bot.key === key)!;
    expect(by("kit-1").state).toBe("working");
    expect(by("kit-2").state).toBe("quiet");
    expect(by("kit-2").quietMinutes).toBe(50);
    expect(by("kit-3").state).toBe("waiting_for_you");
  });

  it("marks a bot done when its last task completed and nothing newer was recorded", () => {
    const claudeTasks: ClaudeTaskReadModel = {
      executionId: "run", rejected: 0,
      tasks: [{
        executionId: "run", nativeTaskId: "t1", taskSubject: "Build cart", createdByTeammateName: null,
        completedByTeammateName: null, sourceId: "kit-1", parentSessionId: "p", createdSequence: 90,
        createdAt: minutesAgo(30), createdEvidenceId: "ev-90", completedSequence: 101, completedAt: minutesAgo(2),
        completedEvidenceId: "ev-101",
      }],
    };
    const kit1 = teamModel(input({ events: live, claudeTasks })).bots.find((bot) => bot.key === "kit-1")!;
    expect(kit1.state).toBe("done");
    expect(kit1.tasks).toEqual([{ id: "t1", title: "Build cart", done: true, source: "task_record", nodeId: null, sequence: 90, doneSequence: 101 }]);
  });

  it("takes tasks from assigned graph nodes and never from note text", () => {
    const model = { nodes: [{ id: "cart", declaredName: "Cart page", state: "waiting_input", assignedActor: { type: "agent", id: "kit-1" }, touches: 1, lastEventAt: null, history: [{ sequence: 60 }] }] } as unknown as GraphModel;
    const kit1 = teamModel(input({ events: live, envelopes, model })).bots.find((bot) => bot.key === "kit-1")!;
    expect(kit1.tasks.map((task) => task.title)).toEqual(["Cart page"]);
    expect(kit1.state).toBe("waiting_for_you");
    expect(teamModel(input({ events: live, envelopes })).bots.find((bot) => bot.key === "coordinator")!.tasks).toEqual([]);
  });

  it("puts coordinator-like bots first and keeps a stable colour per key", () => {
    const a = teamModel(input({ events: live })).bots;
    const b = teamModel(input({ events: [...live].reverse() })).bots;
    expect(a[0].key).toBe("coordinator");
    expect(a.map((bot) => [bot.key, bot.hue])).toEqual(b.map((bot) => [bot.key, bot.hue]));
  });

  it("uses a native persona's chat title as its name", () => {
    const thread = "0d9a6f5e-1111-4222-8333-444455556666";
    const team = teamModel(input({ nativeLinks: { [thread]: { chat: { id: thread, title: "loja kit 2", projectDirectory: "C:/p", updatedAt: 0 }, charter: "Builds checkout", nodeId: "start" } } }));
    expect(team.bots[0]).toMatchObject({ key: thread, name: "loja kit 2", native: true, actorId: null, role: "Builds checkout", state: "quiet", quietMinutes: null });
  });
});

describe("teamLinks", () => {
  it("joins two bots that addressed each other and animates only the last minute", () => {
    const events = [record(1, "kit-1", minutesAgo(10)), record(2, "kit-2", new Date(NOW - 30_000).toISOString())];
    const env: EnvelopeRecord = { 1: { to: "coordinator", replyTo: null, text: "a" }, 2: { to: "kit-1", replyTo: null, text: "b" } };
    const bots = teamModel(input({ events: [...events, record(3, "coordinator", minutesAgo(1))], envelopes: env })).bots;
    expect(teamLinks(events, env, bots, NOW)).toEqual([
      { a: "coordinator", b: "kit-1", count: 1, lastAt: minutesAgo(10), live: false },
      { a: "kit-1", b: "kit-2", count: 1, lastAt: new Date(NOW - 30_000).toISOString(), live: true },
    ]);
  });

  it("never draws a line to the operator", () => {
    const events = [record(1, "kit-1", minutesAgo(1))];
    const env: EnvelopeRecord = { 1: { to: "studio-operator", replyTo: null, text: "q" } };
    const bots = teamModel(input({ events, envelopes: env })).bots;
    expect(teamLinks(events, env, bots, NOW)).toEqual([]);
  });
});
```

The expected order is coordinator first (name matches the coordinator rule), then by display name: "Codex (shared)" sorts before "kit-1".

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/runtime/team.test.ts`
Expected: FAIL — `Cannot find module './team'`.

- [ ] **Step 3: Implement** — create `apps/studio/src/runtime/team.ts`:

```ts
/**
 * The live team, derived from records only (spec §4.1).
 *
 * A bot is a persona, an aliased actor, or an actor with a record in the last two hours; every
 * other recorder is folded into one line so a long run does not read as "98 agents". State and
 * tasks come from records, never from note text, and silence is reported as an age, never as
 * "stuck".
 */
import { hueOf } from "../components/format";
import type { EnvelopeRecord } from "../graph/ledger";
import type { GraphModel } from "../graph/model";
import type { ClaudeTaskReadModel } from "./team-tasks";
import type { NativeChatSummary, RuntimeEvent } from "./types";

export const SHARED_CODEX_ACTOR = "codex";
export const BOT_RECENT_MS = 2 * 60 * 60 * 1000;
export const WORKING_MS = 15 * 60 * 1000;
export const LIVE_LINK_MS = 60 * 1000;
const OPERATOR = "studio-operator";
const SETTLED = new Set(["succeeded", "waived", "skipped"]);
const COORDINATOR = /coordinat|orchestrat|\blead\b/i;

export type BotState = "working" | "waiting_for_you" | "quiet" | "done";
export interface BotTask { id: string; title: string; done: boolean; source: "task_record" | "graph_node"; nodeId: string | null; sequence: number; doneSequence: number | null }
export interface Bot { key: string; actorId: string | null; name: string; hue: number; role: string | null; doingNow: string; lastRecordAt: string | null; lastSequence: number; state: BotState; quietMinutes: number | null; shared: boolean; native: boolean; tasks: BotTask[] }
export interface OtherRecorder { actorId: string; count: number; lastRecordAt: string | null }
export interface TeamModel { bots: Bot[]; otherRecorders: OtherRecorder[] }
export interface TeamLink { a: string; b: string; count: number; lastAt: string | null; live: boolean }
export interface TeamInput {
  events: RuntimeEvent[];
  envelopes: EnvelopeRecord;
  personas: Record<string, string>;
  nativeLinks: Record<string, { chat: NativeChatSummary; charter: string; nodeId: string }>;
  aliases: Record<string, string>;
  model: GraphModel | null;
  claudeTasks: ClaudeTaskReadModel | null;
  waitingAskers: ReadonlySet<string>;
  now: number;
}

function signalKind(event: RuntimeEvent): string | null {
  const payload = event.payload;
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) return null;
  const kind = (payload as Record<string, unknown>).kind;
  return typeof kind === "string" ? kind : null;
}

function timeOf(at: string | null): number | null {
  if (at === null) return null;
  const value = Date.parse(at);
  return Number.isNaN(value) ? null : value;
}

function firstLine(text: string, max = 120): string {
  const line = text.split(/\r?\n/).find((candidate) => candidate.trim().length > 0)?.trim() ?? "";
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}

interface Seed { actorId: string | null; native: boolean; charter: string | null; title: string | null }
interface Tally { count: number; lastAt: string | null; lastSequence: number }

export function teamModel(input: TeamInput): TeamModel {
  const tallies = new Map<string, Tally>();
  const notes = new Map<string, string>();
  for (const event of input.events) {
    if (event.kind !== "signal_recorded" || event.actorType !== "agent" || event.actorId === null) continue;
    const tally = tallies.get(event.actorId) ?? { count: 0, lastAt: null, lastSequence: 0 };
    tally.count += 1;
    if (event.sequence >= tally.lastSequence) {
      tally.lastSequence = event.sequence;
      tally.lastAt = event.occurredAt;
    }
    tallies.set(event.actorId, tally);
    if (signalKind(event) === "operator_note") {
      const text = input.envelopes[event.sequence]?.text?.trim();
      if (text) notes.set(event.actorId, firstLine(text));
    }
  }

  const seeds = new Map<string, Seed>();
  for (const [id, charter] of Object.entries(input.personas)) {
    if (id !== OPERATOR) seeds.set(id, { actorId: id, native: false, charter: charter || null, title: null });
  }
  for (const [thread, link] of Object.entries(input.nativeLinks)) {
    if (!seeds.has(thread)) seeds.set(thread, { actorId: null, native: true, charter: link.charter || null, title: link.chat.title });
  }
  for (const id of Object.keys(input.aliases)) {
    if (!seeds.has(id) && id !== SHARED_CODEX_ACTOR && id !== OPERATOR) seeds.set(id, { actorId: id, native: false, charter: null, title: null });
  }
  const otherRecorders: OtherRecorder[] = [];
  for (const [id, tally] of tallies) {
    if (seeds.has(id)) continue;
    const at = timeOf(tally.lastAt);
    if (at !== null && input.now - at <= BOT_RECENT_MS) {
      seeds.set(id, { actorId: id, native: false, charter: null, title: null });
    } else {
      otherRecorders.push({ actorId: id, count: tally.count, lastRecordAt: tally.lastAt });
    }
  }

  const nodes = input.model?.nodes ?? [];
  const bots: Bot[] = [...seeds.entries()].map(([key, seed]) => {
    const tally = seed.actorId === null ? undefined : tallies.get(seed.actorId);
    const tasks: BotTask[] = [];
    for (const task of input.claudeTasks?.tasks ?? []) {
      if (seed.actorId === null || task.sourceId !== seed.actorId) continue;
      tasks.push({ id: task.nativeTaskId, title: task.taskSubject, done: task.completedSequence !== null, source: "task_record",
        nodeId: null, sequence: task.createdSequence ?? task.completedSequence ?? 0, doneSequence: task.completedSequence });
    }
    const nativeNode = seed.native ? input.nativeLinks[key]?.nodeId ?? null : null;
    for (const node of nodes) {
      const mine = (seed.actorId !== null && node.assignedActor?.id === seed.actorId) || node.id === nativeNode;
      if (!mine) continue;
      const last = node.history.at(-1)?.sequence ?? 0;
      const done = SETTLED.has(node.state);
      tasks.push({ id: `node:${node.id}`, title: node.declaredName ?? node.id, done, source: "graph_node", nodeId: node.id, sequence: last, doneSequence: done ? last : null });
    }
    tasks.sort((a, b) => a.sequence - b.sequence);

    const lastSequence = tally?.lastSequence ?? 0;
    const lastAt = timeOf(tally?.lastAt ?? null);
    const quietMinutes = lastAt === null ? null : Math.floor((input.now - lastAt) / 60_000);
    const waitingNode = tasks.some((task) => task.nodeId !== null && nodes.find((node) => node.id === task.nodeId)?.state === "waiting_input");
    const newest = tasks.at(-1);
    let state: BotState;
    if ((seed.actorId !== null && input.waitingAskers.has(seed.actorId)) || waitingNode) state = "waiting_for_you";
    else if (newest?.done === true && newest.doneSequence !== null && lastSequence <= newest.doneSequence) state = "done";
    else if (lastAt !== null && input.now - lastAt < WORKING_MS) state = "working";
    else state = "quiet";

    const shared = seed.actorId === SHARED_CODEX_ACTOR;
    const name = input.aliases[key] ?? seed.title ?? (shared ? "Codex (shared)" : key);
    return {
      key, actorId: seed.actorId, name, hue: hueOf(key), role: seed.charter === null ? null : firstLine(seed.charter) || null,
      doingNow: seed.actorId !== null ? notes.get(seed.actorId) ?? "No note yet" : "No note yet",
      lastRecordAt: tally?.lastAt ?? null, lastSequence, state, quietMinutes, shared, native: seed.native, tasks,
    };
  });

  bots.sort((a, b) => {
    const lead = Number(COORDINATOR.test(`${b.name} ${b.role ?? ""}`)) - Number(COORDINATOR.test(`${a.name} ${a.role ?? ""}`));
    return lead !== 0 ? lead : a.name.localeCompare(b.name);
  });
  otherRecorders.sort((a, b) => a.actorId.localeCompare(b.actorId, undefined, { numeric: true }));
  return { bots, otherRecorders };
}

/** The bot an actor id or thread id belongs to, or null when it belongs to none. */
export function botKeyOf(bots: Bot[], id: string | null): string | null {
  if (id === null) return null;
  return bots.find((bot) => bot.key === id || bot.actorId === id)?.key ?? null;
}

/** One line per pair of bots that addressed each other (the `talks` pair rule in App.tsx). */
export function teamLinks(events: RuntimeEvent[], envelopes: EnvelopeRecord, bots: Bot[], now: number): TeamLink[] {
  const links = new Map<string, TeamLink>();
  for (const event of events) {
    if (event.kind !== "signal_recorded" || (event.actorType !== "agent" && event.actorType !== "owner")) continue;
    const to = envelopes[event.sequence]?.to ?? null;
    if (event.actorId === OPERATOR || to === OPERATOR) continue;
    const from = botKeyOf(bots, event.actorId);
    const target = botKeyOf(bots, to);
    if (from === null || target === null || from === target) continue;
    const [a, b] = [from, target].sort();
    const link = links.get(`${a}\u0000${b}`) ?? { a, b, count: 0, lastAt: null, live: false };
    link.count += 1;
    if (link.lastAt === null || (timeOf(event.occurredAt) ?? 0) >= (timeOf(link.lastAt) ?? 0)) link.lastAt = event.occurredAt;
    links.set(`${a}\u0000${b}`, link);
  }
  return [...links.values()]
    .map((link) => {
      const at = timeOf(link.lastAt);
      return { ...link, live: at !== null && now - at <= LIVE_LINK_MS };
    })
    .sort((x, y) => x.a.localeCompare(y.a) || x.b.localeCompare(y.b));
}
```

- [ ] **Step 4: Run tests and typecheck**

Run: `npx vitest run src/runtime/team.test.ts && npx tsc -b`
Expected: PASS (10 tests), no type errors.

- [ ] **Step 5: Commit**

```bash
git add src/runtime/team.ts src/runtime/team.test.ts
git commit -m "feat(studio): derive the live team from records (#301)"
```

---

### Task 3: Needs-you derivation (`needs-you.ts`)

**Files:**
- Create: `apps/studio/src/runtime/needs-you.ts`
- Test: `apps/studio/src/runtime/needs-you.test.ts`

**Interfaces:**
- Consumes: `openQuestions`, `EnvelopeRecord` from `../graph/ledger`; `ExecutionStatus`, `NativeChatRequest`, `NativeChatRequestState`, `RuntimeEvent` from `./types`.
- Produces:

```ts
export interface QuestionItem { kind: "question"; key: string; asker: string; text: string; signalId: string | null; recommendations: string[]; at: string | null; sequence: number }
export interface NativeRequestItem { kind: "native_request"; key: string; requestId: string; threadId: string; nodeId: string; title: string; state: NativeChatRequestState; detail: string | null }
export interface StepItem { kind: "waiting_step" | "blocked_step"; key: string; nodeId: string; name: string; reason: string }
export interface DraftItem { kind: "draft"; key: string; draftId: string }
export type NeedsYouItem = QuestionItem | NativeRequestItem | StepItem | DraftItem;
export type BeaconState = { kind: "lit"; count: number } | { kind: "dark" } | { kind: "unknown"; reason: string };
export const RUNTIME_SILENT = "Can't tell: the Runtime is not answering";
export interface NeedsYouInput { status: ExecutionStatus | null; stale: boolean; events: RuntimeEvent[]; envelopes: EnvelopeRecord; nativeRequests: NativeChatRequest[] | null; pendingDraftIds: string[]; nodeNames: Record<string, string>; operatorId: string }
export function needsYou(input: NeedsYouInput): { state: BeaconState; items: NeedsYouItem[] };
```

- [ ] **Step 1: Write the failing test** — create `apps/studio/src/runtime/needs-you.test.ts`:

```ts
import { describe, expect, it } from "vitest";

import type { EnvelopeRecord } from "../graph/ledger";
import type { ExecutionStatus, NativeChatRequest, RuntimeEvent } from "./types";
import { needsYou, RUNTIME_SILENT, type NeedsYouInput } from "./needs-you";

const STATUS: ExecutionStatus = {
  executionId: "run", mode: "supervised", status: "running", attention: "can_sleep", attentionReasons: [],
  nodeStateCounts: {}, untriagedInterruptions: [], silenceUnevaluated: [], startedAt: null, lastEventAt: null,
  nodeLastEventAt: {}, headSequence: 10,
};

function signal(sequence: number, actorId: string, signalId: string): RuntimeEvent {
  return { sequence, kind: "signal_recorded", payload: { kind: "operator_note", signalId }, occurredAt: "2026-10-05T11:00:00Z",
    actorId, actorType: actorId === "studio-operator" ? "owner" : "agent", idempotencyKey: null, eventId: `e${sequence}`, evidenceRefs: [`ev${sequence}`] };
}

const base = (overrides: Partial<NeedsYouInput> = {}): NeedsYouInput => ({
  status: STATUS, stale: false, events: [], envelopes: {}, nativeRequests: [], pendingDraftIds: [], nodeNames: {},
  operatorId: "studio-operator", ...overrides,
});

const question = [signal(5, "kit-3", "sig-q")];
const asked: EnvelopeRecord = { 5: { to: "studio-operator", replyTo: null, text: "Merge now?", recommendations: ["Wait", "Merge"] } };

describe("needsYou", () => {
  it("is dark when the Runtime answered and nothing is open", () => {
    expect(needsYou(base())).toEqual({ state: { kind: "dark" }, items: [] });
  });

  it("is lit with one card per open question, carrying its choices", () => {
    const result = needsYou(base({ events: question, envelopes: asked }));
    expect(result.state).toEqual({ kind: "lit", count: 1 });
    expect(result.items).toEqual([{ kind: "question", key: "question:sig-q", asker: "kit-3", text: "Merge now?", signalId: "sig-q",
      recommendations: ["Wait", "Merge"], at: "2026-10-05T11:00:00Z", sequence: 5 }]);
  });

  it("closes a question only when the owner replies to it", () => {
    const ownerReply = [...question, signal(6, "studio-operator", "sig-a")];
    expect(needsYou(base({ events: ownerReply, envelopes: { ...asked, 6: { to: "kit-3", replyTo: "sig-q", text: "Wait" } } })).items).toEqual([]);
    const agentReply = [...question, signal(6, "kit-1", "sig-b")];
    expect(needsYou(base({ events: agentReply, envelopes: { ...asked, 6: { to: "kit-3", replyTo: "sig-q", text: "merge it" } } })).items).toHaveLength(1);
  });

  it("is unknown, never dark, when the connection is stale or the status is missing", () => {
    expect(needsYou(base({ stale: true })).state).toEqual({ kind: "unknown", reason: RUNTIME_SILENT });
    expect(needsYou(base({ stale: true, events: question, envelopes: asked })).state).toEqual({ kind: "unknown", reason: RUNTIME_SILENT });
    expect(needsYou(base({ status: null })).state.kind).toBe("unknown");
  });

  it("lists native requests the ledger cannot confirm and leaves confirmed ones out", () => {
    const request = (state: NativeChatRequest["state"]): NativeChatRequest => ({ requestId: `r-${state}`, nodeId: "start", threadId: "t-1", title: "loja kit 2", sourceDirectory: "C:/p", state });
    const items = needsYou(base({ nativeRequests: [request("unobserved"), request("blocked"), request("completed"), request("requested")] })).items;
    expect(items.map((item) => item.key)).toEqual(["native:r-unobserved", "native:r-blocked"]);
  });

  it("lists waiting and blocked steps by name, and pending drafts", () => {
    const status = { ...STATUS, attentionReasons: [{ kind: "waiting_input_node", node: "start" }, { kind: "blocked_node", node: "cart" }, { kind: "untriaged_interruption", node: "pay" }] };
    const items = needsYou(base({ status, nodeNames: { cart: "Cart page" }, pendingDraftIds: ["d-1"] })).items;
    expect(items).toEqual([
      { kind: "draft", key: "draft:d-1", draftId: "d-1" },
      { kind: "waiting_step", key: "waiting_input_node:start", nodeId: "start", name: "start", reason: "waiting_input_node" },
      { kind: "blocked_step", key: "blocked_node:cart", nodeId: "cart", name: "Cart page", reason: "blocked_node" },
      { kind: "blocked_step", key: "untriaged_interruption:pay", nodeId: "pay", name: "pay", reason: "untriaged_interruption" },
    ]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/runtime/needs-you.test.ts`
Expected: FAIL — `Cannot find module './needs-you'`.

- [ ] **Step 3: Implement** — create `apps/studio/src/runtime/needs-you.ts`:

```ts
/**
 * What needs the owner, in one list (spec §4.3). Dark is a claim that nothing blocks, so a stale
 * connection or a missing status is `unknown` - never dark, and never a count the page cannot back.
 */
import { openQuestions, type EnvelopeRecord } from "../graph/ledger";
import type { ExecutionStatus, NativeChatRequest, NativeChatRequestState, RuntimeEvent } from "./types";

export interface QuestionItem { kind: "question"; key: string; asker: string; text: string; signalId: string | null; recommendations: string[]; at: string | null; sequence: number }
export interface NativeRequestItem { kind: "native_request"; key: string; requestId: string; threadId: string; nodeId: string; title: string; state: NativeChatRequestState; detail: string | null }
export interface StepItem { kind: "waiting_step" | "blocked_step"; key: string; nodeId: string; name: string; reason: string }
export interface DraftItem { kind: "draft"; key: string; draftId: string }
export type NeedsYouItem = QuestionItem | NativeRequestItem | StepItem | DraftItem;
export type BeaconState = { kind: "lit"; count: number } | { kind: "dark" } | { kind: "unknown"; reason: string };
export const RUNTIME_SILENT = "Can't tell: the Runtime is not answering";

export interface NeedsYouInput {
  status: ExecutionStatus | null;
  stale: boolean;
  events: RuntimeEvent[];
  envelopes: EnvelopeRecord;
  nativeRequests: NativeChatRequest[] | null;
  pendingDraftIds: string[];
  nodeNames: Record<string, string>;
  operatorId: string;
}

function sequenceOfSignal(events: RuntimeEvent[], signalId: string | null): number {
  if (signalId === null) return -1;
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const payload = events[index].payload as { signalId?: unknown } | null;
    if (payload !== null && typeof payload === "object" && payload.signalId === signalId) return events[index].sequence;
  }
  return -1;
}

export function needsYou(input: NeedsYouInput): { state: BeaconState; items: NeedsYouItem[] } {
  const items: NeedsYouItem[] = [];
  for (const debt of openQuestions(input.events, input.envelopes, input.operatorId)) {
    const sequence = sequenceOfSignal(input.events, debt.signalId);
    items.push({ kind: "question", key: `question:${debt.signalId ?? `seq-${sequence}`}`, asker: debt.asker, text: debt.text,
      signalId: debt.signalId, recommendations: input.envelopes[sequence]?.recommendations ?? [], at: debt.at, sequence });
  }
  for (const request of input.nativeRequests ?? []) {
    if (request.state !== "unobserved" && request.state !== "blocked") continue;
    items.push({ kind: "native_request", key: `native:${request.requestId}`, requestId: request.requestId, threadId: request.threadId,
      nodeId: request.nodeId, title: request.title, state: request.state, detail: request.detail ?? null });
  }
  for (const draftId of input.pendingDraftIds) items.push({ kind: "draft", key: `draft:${draftId}`, draftId });
  for (const reason of input.status?.attentionReasons ?? []) {
    const node = typeof reason.node === "string" ? reason.node : null;
    const kind = typeof reason.kind === "string" ? reason.kind : "";
    if (node === null) continue;
    const step = kind === "waiting_input_node" ? "waiting_step" : kind === "blocked_node" || kind === "untriaged_interruption" ? "blocked_step" : null;
    if (step === null) continue;
    items.push({ kind: step, key: `${kind}:${node}`, nodeId: node, name: input.nodeNames[node] ?? node, reason: kind });
  }
  const state: BeaconState = input.status === null || input.stale
    ? { kind: "unknown", reason: RUNTIME_SILENT }
    : items.length > 0 ? { kind: "lit", count: items.length } : { kind: "dark" };
  return { state, items };
}
```

- [ ] **Step 4: Run tests and typecheck**

Run: `npx vitest run src/runtime/needs-you.test.ts && npx tsc -b`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src/runtime/needs-you.ts src/runtime/needs-you.test.ts
git commit -m "feat(studio): one needs-you list behind the beacon (#301)"
```

---

### Task 4: Handover derivation (`handover.ts`)

**Files:**
- Create: `apps/studio/src/runtime/handover.ts`
- Test: `apps/studio/src/runtime/handover.test.ts`

**Interfaces:**
- Consumes: `Bot` (Task 2), `QuestionItem`, `NeedsYouItem` (Task 3), `GraphModel`, `ClaudeTaskReadModel`, `RuntimeEvent`.
- Produces:

```ts
export const HANDOVER_MIN_GAP_MS: number;   // 15 min
export const HANDOVER_MIN_EVENTS = 20;
export const QUIET_IN_GAP_MS: number;      // 30 min
export interface HandoverLine { text: string; sequences: number[] }
export interface Handover { fromSeq: number; toSeq: number; eventCount: number; gapMinutes: number; shipped: HandoverLine[]; needsYou: HandoverLine[]; quiet: HandoverLine[]; untouched: HandoverLine[] }
export interface HandoverInput { events: RuntimeEvent[]; bots: Bot[]; model: GraphModel | null; claudeTasks: ClaudeTaskReadModel | null; openItems: NeedsYouItem[]; fromSeq: number; toSeq: number }
export function shouldShowHandover(events: RuntimeEvent[], fromSeq: number | null, toSeq: number): boolean;
export function buildHandover(input: HandoverInput): Handover;
export function lastSeenKey(project: string, executionId: string): string;
export function readLastSeen(project: string, executionId: string): number | null;
export function writeLastSeen(project: string, executionId: string, sequence: number): void;
```

- [ ] **Step 1: Write the failing test** — create `apps/studio/src/runtime/handover.test.ts`:

```ts
import { afterEach, describe, expect, it, vi } from "vitest";

import type { GraphModel } from "../graph/model";
import type { Bot } from "./team";
import type { RuntimeEvent } from "./types";
import { buildHandover, lastSeenKey, readLastSeen, shouldShowHandover, writeLastSeen } from "./handover";

const T0 = Date.parse("2026-10-05T08:00:00Z");
const at = (minutes: number) => new Date(T0 + minutes * 60_000).toISOString();

function ev(sequence: number, minutes: number, kind: string, actorId: string, payload: Record<string, unknown> = {}): RuntimeEvent {
  return { sequence, kind, payload, occurredAt: at(minutes), actorId, actorType: actorId === "system-cli" ? "system" : "agent",
    idempotencyKey: null, eventId: `e${sequence}`, evidenceRefs: [] };
}

function bot(key: string): Bot {
  return { key, actorId: key, name: key.replace("-", " "), hue: 0, role: null, doingNow: "", lastRecordAt: null, lastSequence: 0,
    state: "working", quietMinutes: null, shared: false, native: false, tasks: [] };
}

// seq 1..10 before the owner left (minute 0..9), seq 11..40 while away (minute 20..140).
const before = Array.from({ length: 10 }, (_, i) => ev(i + 1, i, "signal_recorded", "kit-1", { kind: "operator_note" }));
const away: RuntimeEvent[] = [
  ev(11, 20, "node_outcome_recorded", "system-cli", { nodeId: "cart", outcome: "succeeded", nextState: "succeeded" }),
  ev(12, 25, "signal_recorded", "kit-2", { kind: "agent_task_completed" }),
  ev(13, 30, "signal_recorded", "kit-3", { kind: "operator_note", signalId: "sig-q" }),
  ...Array.from({ length: 27 }, (_, i) => ev(14 + i, 40 + i * 4, "signal_recorded", "kit-1", { kind: "operator_note" })),
];
const events = [ev(0, 0, "execution_form_declared", "system-cli", { nodeIds: ["cart", "pay"] }), ev(0.5, 1, "node_ready", "system-cli", { nodeId: "pay" }), ...before, ...away];

describe("shouldShowHandover", () => {
  it("needs a stored position, 15 minutes and 20 events", () => {
    expect(shouldShowHandover(events, 10, 40)).toBe(true);
    expect(shouldShowHandover(events, null, 40)).toBe(false);
    expect(shouldShowHandover(events, 30, 40)).toBe(false);          // 10 events
    expect(shouldShowHandover(before, 1, 10)).toBe(false);           // 9 minutes
  });
});

describe("buildHandover", () => {
  const model = { nodes: [{ id: "cart", declaredName: "Cart page", state: "succeeded" }, { id: "pay", declaredName: "Payment", state: "ready" }] } as unknown as GraphModel;
  const handover = buildHandover({
    events, bots: [bot("kit-1"), bot("kit-2"), bot("kit-3")], model, claudeTasks: null, fromSeq: 10, toSeq: 40,
    openItems: [{ kind: "question", key: "question:sig-q", asker: "kit-3", text: "Merge now?\nmore", signalId: "sig-q", recommendations: [], at: at(30), sequence: 13 }],
  });

  it("lists what shipped, each line citing its sequences", () => {
    expect(handover.shipped).toEqual([
      { text: "Cart page succeeded", sequences: [11] },
      { text: "kit 2 finished a task", sequences: [12] },
    ]);
  });
  it("lists the questions opened while away", () => {
    expect(handover.needsYou).toEqual([{ text: "kit 3 asked: Merge now?", sequences: [13] }]);
  });
  it("lists bots with no record for 30 minutes or more inside the gap", () => {
    expect(handover.quiet.map((line) => line.text)).toEqual(["kit 2: no new record for 119 min", "kit 3: no new record for 114 min"]);
  });
  it("lists graph steps that existed before and got no record in the gap", () => {
    expect(handover.untouched).toEqual([{ text: "Payment got no record", sequences: [0.5] }]);
  });
  it("carries the gap's size", () => {
    expect(handover).toMatchObject({ fromSeq: 10, toSeq: 40, eventCount: 30, gapMinutes: 135 });
  });
});

describe("last seen storage", () => {
  afterEach(() => { localStorage.clear(); vi.restoreAllMocks(); });
  it("round-trips per project and run", () => {
    writeLastSeen("ml-saas", "run-a", 42);
    expect(readLastSeen("ml-saas", "run-a")).toBe(42);
    expect(readLastSeen("ml-saas", "run-b")).toBeNull();
  });
  it("treats junk and unavailable storage as no position", () => {
    localStorage.setItem(lastSeenKey("ml-saas", "run-a"), "not a number");
    expect(readLastSeen("ml-saas", "run-a")).toBeNull();
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("blocked"); });
    expect(readLastSeen("ml-saas", "run-a")).toBeNull();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("blocked"); });
    expect(() => writeLastSeen("ml-saas", "run-a", 1)).not.toThrow();
  });
});
```

Note: the quiet expectations are computed against the gap end (seq 40 at minute 40+26·4 = 144). kit-2's last record is minute 25 → 119 min; kit-3's minute 30 → 114 min. `gapMinutes` = 144 − 9 = 135. The fixture uses sequence `0.5` only so the `node_ready` event sorts before `before`; that is legal for a test because sequences are plain numbers in the type.

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/runtime/handover.test.ts`
Expected: FAIL — `Cannot find module './handover'`.

- [ ] **Step 3: Implement** — create `apps/studio/src/runtime/handover.ts`:

```ts
/**
 * "While you were away" (spec §4.5): what shipped, what needs you, what went quiet and what
 * nobody touched between the last sequence the owner saw and now. Every line cites the event
 * sequences it came from. Missing or unreadable storage means no card - never a wrong one.
 */
import type { GraphModel } from "../graph/model";
import type { NeedsYouItem } from "./needs-you";
import type { Bot } from "./team";
import type { ClaudeTaskReadModel } from "./team-tasks";
import type { RuntimeEvent } from "./types";

export const HANDOVER_MIN_GAP_MS = 15 * 60 * 1000;
export const HANDOVER_MIN_EVENTS = 20;
export const QUIET_IN_GAP_MS = 30 * 60 * 1000;
const SETTLED = new Set(["succeeded", "waived", "skipped"]);

export interface HandoverLine { text: string; sequences: number[] }
export interface Handover { fromSeq: number; toSeq: number; eventCount: number; gapMinutes: number; shipped: HandoverLine[]; needsYou: HandoverLine[]; quiet: HandoverLine[]; untouched: HandoverLine[] }
export interface HandoverInput { events: RuntimeEvent[]; bots: Bot[]; model: GraphModel | null; claudeTasks: ClaudeTaskReadModel | null; openItems: NeedsYouItem[]; fromSeq: number; toSeq: number }

function timeOf(at: string | null): number | null {
  if (at === null) return null;
  const value = Date.parse(at);
  return Number.isNaN(value) ? null : value;
}

function payloadOf(event: RuntimeEvent): Record<string, unknown> {
  return event.payload !== null && typeof event.payload === "object" && !Array.isArray(event.payload) ? event.payload as Record<string, unknown> : {};
}

function firstLine(text: string): string {
  return text.split(/\r?\n/).find((line) => line.trim().length > 0)?.trim().slice(0, 140) ?? "";
}

function gapBounds(events: RuntimeEvent[], fromSeq: number, toSeq: number): { inGap: RuntimeEvent[]; fromTime: number | null; toTime: number | null } {
  const inGap = events.filter((event) => event.sequence > fromSeq && event.sequence <= toSeq);
  const seen = events.filter((event) => event.sequence <= fromSeq).at(-1);
  const fromTime = timeOf(seen?.occurredAt ?? inGap[0]?.occurredAt ?? null);
  const toTime = timeOf(inGap.at(-1)?.occurredAt ?? null);
  return { inGap, fromTime, toTime };
}

export function shouldShowHandover(events: RuntimeEvent[], fromSeq: number | null, toSeq: number): boolean {
  if (fromSeq === null || toSeq <= fromSeq) return false;
  const { inGap, fromTime, toTime } = gapBounds(events, fromSeq, toSeq);
  return inGap.length >= HANDOVER_MIN_EVENTS && fromTime !== null && toTime !== null && toTime - fromTime >= HANDOVER_MIN_GAP_MS;
}

export function buildHandover(input: HandoverInput): Handover {
  const { inGap, fromTime, toTime } = gapBounds(input.events, input.fromSeq, input.toSeq);
  const botName = (id: string | null) => input.bots.find((bot) => bot.key === id || bot.actorId === id)?.name ?? id ?? "Someone";
  const nodeName = (id: unknown) => {
    if (typeof id !== "string") return "A step";
    return input.model?.nodes.find((node) => node.id === id)?.declaredName ?? id;
  };

  const shipped: HandoverLine[] = [];
  for (const event of inGap) {
    const payload = payloadOf(event);
    if (event.kind === "node_outcome_recorded" && payload.outcome === "succeeded") shipped.push({ text: `${nodeName(payload.nodeId)} succeeded`, sequences: [event.sequence] });
    else if (event.kind === "completion_cleared") shipped.push({ text: `${nodeName(payload.nodeId)} was cleared as complete`, sequences: [event.sequence] });
    else if (event.kind === "signal_recorded" && payload.kind === "agent_task_completed") {
      const task = input.claudeTasks?.tasks.find((candidate) => candidate.completedSequence === event.sequence);
      shipped.push({ text: task ? `${botName(event.actorId)} finished “${task.taskSubject}”` : `${botName(event.actorId)} finished a task`, sequences: [event.sequence] });
    }
  }

  const needsYou: HandoverLine[] = input.openItems.flatMap((item) =>
    item.kind === "question" && item.sequence > input.fromSeq && item.sequence <= input.toSeq
      ? [{ text: `${botName(item.asker)} asked: ${firstLine(item.text)}`, sequences: [item.sequence] }]
      : []);

  const quiet: HandoverLine[] = [];
  if (toTime !== null) {
    for (const bot of input.bots) {
      if (bot.actorId === null) continue;
      const last = input.events.filter((event) => event.actorId === bot.actorId && event.sequence <= input.toSeq).at(-1);
      const lastTime = timeOf(last?.occurredAt ?? null);
      if (last === undefined || lastTime === null || toTime - lastTime < QUIET_IN_GAP_MS) continue;
      quiet.push({ text: `${bot.name}: no new record for ${Math.floor((toTime - lastTime) / 60_000)} min`, sequences: [last.sequence] });
    }
  }

  const touchedInGap = new Set(inGap.flatMap((event) => { const id = payloadOf(event).nodeId; return typeof id === "string" ? [id] : []; }));
  const untouched: HandoverLine[] = [];
  const firstNamed = new Map<string, number>();
  for (const event of input.events) {
    if (event.sequence > input.fromSeq) break;
    const id = payloadOf(event).nodeId;
    if (typeof id === "string" && !firstNamed.has(id)) firstNamed.set(id, event.sequence);
  }
  for (const [id, sequence] of firstNamed) {
    const node = input.model?.nodes.find((candidate) => candidate.id === id);
    if (touchedInGap.has(id) || (node !== undefined && SETTLED.has(node.state))) continue;
    untouched.push({ text: `${nodeName(id)} got no record`, sequences: [sequence] });
  }
  const recordedInGap = new Set(inGap.map((event) => event.actorId));
  for (const task of input.claudeTasks?.tasks ?? []) {
    if (task.createdSequence === null || task.createdSequence > input.fromSeq) continue;
    if (task.completedSequence !== null && task.completedSequence <= input.toSeq) continue;
    if (recordedInGap.has(task.sourceId)) continue;
    untouched.push({ text: `${task.taskSubject} (${botName(task.sourceId)}) got no record`, sequences: [task.createdSequence] });
  }

  return {
    fromSeq: input.fromSeq, toSeq: input.toSeq, eventCount: inGap.length,
    gapMinutes: fromTime === null || toTime === null ? 0 : Math.floor((toTime - fromTime) / 60_000),
    shipped, needsYou, quiet, untouched,
  };
}

export function lastSeenKey(project: string, executionId: string): string {
  return `graphhelm.handover.last-seen:${project}:${executionId}`;
}

export function readLastSeen(project: string, executionId: string): number | null {
  try {
    const raw = globalThis.localStorage.getItem(lastSeenKey(project, executionId));
    if (raw === null || !/^\d{1,15}$/.test(raw)) return null;
    return Number(raw);
  } catch {
    return null;
  }
}

export function writeLastSeen(project: string, executionId: string, sequence: number): void {
  try {
    globalThis.localStorage.setItem(lastSeenKey(project, executionId), String(Math.max(0, Math.floor(sequence))));
  } catch {
    // A convenience: without storage the next visit shows no card, which is correct.
  }
}
```

The `node_ready` fixture event uses sequence `0.5`; `readLastSeen` only ever stores integers, and the derivation compares numbers, so the fixture is valid.

- [ ] **Step 4: Run tests and typecheck**

Run: `npx vitest run src/runtime/handover.test.ts && npx tsc -b`
Expected: PASS (8 tests). If a quiet minute count differs by one, recompute from the fixture comment above; do not loosen the assertion to a regex.

- [ ] **Step 5: Commit**

```bash
git add src/runtime/handover.ts src/runtime/handover.test.ts
git commit -m "feat(studio): build the while-you-were-away handover (#301)"
```

---

### Task 5: Chat threads and activity lines (`threads.ts`)

**Files:**
- Create: `apps/studio/src/runtime/threads.ts`
- Test: `apps/studio/src/runtime/threads.test.ts`

**Interfaces:**
- Consumes: `WorkMessage` from `./work-conversation`; `Bot`, `botKeyOf` (Task 2); `EnvelopeRecord`; `RuntimeEvent`; `ago` from `../components/format`.
- Produces:

```ts
export type ThreadKind = "everyone" | "pair" | "direct";
export interface ChatThread { key: string; kind: ThreadKind; label: string; participants: string[]; messages: WorkMessage[] }
export const EVERYONE = "everyone";
export function namesOf(bots: Bot[]): Record<string, string>;
export function chatThreads(messages: WorkMessage[], bots: Bot[], operatorId: string): ChatThread[];
export function unreadCounts(threads: ChatThread[], lastOpened: Record<string, number>): Record<string, number>;
export function parseMention(text: string, bots: Bot[]): { to: string | null; text: string };
export function sealedNotesPending(events: RuntimeEvent[], envelopes: EnvelopeRecord): number;
export interface ActivityItem { sequence: number; actorId: string | null; occurredAt: string | null; text: string | null }
export interface ActivityLine { sequence: number; text: string; at: string | null }
export function describeActivity(item: ActivityItem, names: Record<string, string>, envelopes: EnvelopeRecord, operatorId: string): ActivityLine;
```

- [ ] **Step 1: Write the failing test** — create `apps/studio/src/runtime/threads.test.ts`:

```ts
import { describe, expect, it } from "vitest";

import type { Bot } from "./team";
import type { RuntimeEvent } from "./types";
import type { WorkMessage } from "./work-conversation";
import { chatThreads, describeActivity, namesOf, parseMention, sealedNotesPending, unreadCounts } from "./threads";

const bot = (key: string, name: string): Bot => ({ key, actorId: key, name, hue: 0, role: null, doingNow: "", lastRecordAt: null,
  lastSequence: 0, state: "working", quietMinutes: null, shared: false, native: false, tasks: [] });
const BOTS = [bot("coordinator", "Coordinator"), bot("kit-1", "loja kit 1"), bot("kit-2", "loja kit 2")];
const msg = (sequence: number, sender: string, to: string | null): WorkMessage => ({ id: `event-${sequence}`, sequence, sender, to,
  replyTo: null, text: `m${sequence}`, at: null, provenance: "stored", acknowledged: false });

describe("chatThreads", () => {
  const threads = chatThreads([msg(1, "coordinator", null), msg(2, "kit-1", "kit-2"), msg(3, "kit-2", "kit-1"),
    msg(4, "studio-operator", "kit-1"), msg(5, "kit-1", "studio-operator")], BOTS, "studio-operator");

  it("splits Everyone, bot pairs and direct lines with the owner", () => {
    expect(threads.map((thread) => [thread.key, thread.kind, thread.label, thread.messages.map((m) => m.sequence)])).toEqual([
      ["everyone", "everyone", "Everyone", [1]],
      ["pair:kit-1+kit-2", "pair", "loja kit 1 ↔ loja kit 2", [2, 3]],
      ["direct:kit-1", "direct", "loja kit 1", [4, 5]],
    ]);
  });

  it("always offers Everyone, even with no messages", () => {
    expect(chatThreads([], BOTS, "studio-operator").map((thread) => thread.key)).toEqual(["everyone"]);
  });

  it("counts unread messages since a tab was last opened", () => {
    expect(unreadCounts(threads, { everyone: 1, "pair:kit-1+kit-2": 2 })).toEqual({ everyone: 0, "pair:kit-1+kit-2": 1, "direct:kit-1": 2 });
  });
});

describe("parseMention", () => {
  it("targets one bot by its display name, longest name first", () => {
    expect(parseMention("@loja kit 2 please rebase", BOTS)).toEqual({ to: "kit-2", text: "please rebase" });
    expect(parseMention("@Coordinator  plan?", BOTS)).toEqual({ to: "coordinator", text: "plan?" });
  });
  it("leaves an unknown mention as plain text to everyone", () => {
    expect(parseMention("@nobody hi", BOTS)).toEqual({ to: null, text: "@nobody hi" });
  });
});

describe("sealedNotesPending", () => {
  it("counts operator notes whose envelope is not opened yet", () => {
    const note = (sequence: number): RuntimeEvent => ({ sequence, kind: "signal_recorded", payload: { kind: "operator_note" }, occurredAt: null,
      actorId: "kit-1", actorType: "agent", idempotencyKey: null, eventId: null, evidenceRefs: ["ev"] });
    expect(sealedNotesPending([note(1), note(2)], { 1: { to: null, replyTo: null, text: "x" } })).toBe(1);
  });
});

describe("describeActivity", () => {
  it("reads as bot, verb, object", () => {
    const names = namesOf(BOTS);
    const envelopes = { 7: { to: "studio-operator", replyTo: null, text: "Merge now?" }, 8: { to: "kit-2", replyTo: null, text: "Take the cart" } };
    expect(describeActivity({ sequence: 7, actorId: "kit-1", occurredAt: null, text: "Merge now?" }, names, envelopes, "studio-operator").text)
      .toBe("loja kit 1 asked you “Merge now?”");
    expect(describeActivity({ sequence: 8, actorId: "coordinator", occurredAt: null, text: "Take the cart" }, names, envelopes, "studio-operator").text)
      .toBe("Coordinator told loja kit 2 “Take the cart”");
    expect(describeActivity({ sequence: 9, actorId: "kit-9", occurredAt: null, text: null }, names, {}, "studio-operator").text)
      .toBe("kit-9 recorded a sealed note");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/runtime/threads.test.ts`
Expected: FAIL — `Cannot find module './threads'`.

- [ ] **Step 3: Implement** — create `apps/studio/src/runtime/threads.ts`:

```ts
/**
 * Chat threads for the Chat column (spec §4.4). Pair threads are RECORDED messages: what two
 * agents recorded to each other through the Runtime, not native chats talking directly.
 */
import type { EnvelopeRecord } from "../graph/ledger";
import { botKeyOf, type Bot } from "./team";
import type { RuntimeEvent } from "./types";
import type { WorkMessage } from "./work-conversation";

export type ThreadKind = "everyone" | "pair" | "direct";
export interface ChatThread { key: string; kind: ThreadKind; label: string; participants: string[]; messages: WorkMessage[] }
export const EVERYONE = "everyone";

export function namesOf(bots: Bot[]): Record<string, string> {
  const names: Record<string, string> = {};
  for (const bot of bots) {
    names[bot.key] = bot.name;
    if (bot.actorId !== null) names[bot.actorId] = bot.name;
  }
  return names;
}

export function chatThreads(messages: WorkMessage[], bots: Bot[], operatorId: string): ChatThread[] {
  const names = namesOf(bots);
  const label = (id: string) => names[id] ?? id;
  const everyone: ChatThread = { key: EVERYONE, kind: "everyone", label: "Everyone", participants: [], messages: [] };
  const pairs = new Map<string, ChatThread>();
  const directs = new Map<string, ChatThread>();
  for (const message of messages) {
    if (message.to === null) { everyone.messages.push(message); continue; }
    const fromOwner = message.sender === operatorId;
    const toOwner = message.to === operatorId;
    if (fromOwner || toOwner) {
      const other = fromOwner ? message.to : message.sender;
      const key = botKeyOf(bots, other) ?? other;
      const thread = directs.get(key) ?? { key: `direct:${key}`, kind: "direct" as const, label: label(key), participants: [key], messages: [] };
      thread.messages.push(message);
      directs.set(key, thread);
      continue;
    }
    const a = botKeyOf(bots, message.sender) ?? message.sender;
    const b = botKeyOf(bots, message.to) ?? message.to;
    const [first, second] = [a, b].sort();
    const key = `pair:${first}+${second}`;
    const thread = pairs.get(key) ?? { key, kind: "pair" as const, label: `${label(first)} ↔ ${label(second)}`, participants: [first, second], messages: [] };
    thread.messages.push(message);
    pairs.set(key, thread);
  }
  return [everyone, ...pairs.values(), ...directs.values()];
}

export function unreadCounts(threads: ChatThread[], lastOpened: Record<string, number>): Record<string, number> {
  return Object.fromEntries(threads.map((thread) => [thread.key, thread.messages.filter((message) => message.sequence > (lastOpened[thread.key] ?? 0)).length]));
}

export function parseMention(text: string, bots: Bot[]): { to: string | null; text: string } {
  const lowered = text.toLowerCase();
  for (const bot of [...bots].sort((a, b) => b.name.length - a.name.length)) {
    const prefix = `@${bot.name.toLowerCase()} `;
    if (lowered.startsWith(prefix)) return { to: bot.actorId ?? bot.key, text: text.slice(prefix.length).trim() };
  }
  return { to: null, text };
}

export function sealedNotesPending(events: RuntimeEvent[], envelopes: EnvelopeRecord): number {
  return events.filter((event) => event.kind === "signal_recorded" && event.evidenceRefs.length > 0
    && (event.payload as { kind?: unknown } | null)?.kind === "operator_note" && envelopes[event.sequence] === undefined).length;
}

export interface ActivityItem { sequence: number; actorId: string | null; occurredAt: string | null; text: string | null }
export interface ActivityLine { sequence: number; text: string; at: string | null }

export function describeActivity(item: ActivityItem, names: Record<string, string>, envelopes: EnvelopeRecord, operatorId: string): ActivityLine {
  const who = item.actorId === operatorId ? "You" : names[item.actorId ?? ""] ?? item.actorId ?? "Someone";
  const words = item.text?.split(/\r?\n/)[0]?.trim().slice(0, 80) ?? "";
  const to = envelopes[item.sequence]?.to ?? null;
  const text = words === ""
    ? `${who} recorded a sealed note`
    : to === operatorId ? `${who} asked you “${words}”`
    : to !== null ? `${who} told ${names[to] ?? to} “${words}”`
    : `${who} said “${words}”`;
  return { sequence: item.sequence, text, at: item.occurredAt };
}
```

- [ ] **Step 4: Run tests and typecheck**

Run: `npx vitest run src/runtime/threads.test.ts && npx tsc -b`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add src/runtime/threads.ts src/runtime/threads.test.ts
git commit -m "feat(studio): chat threads, mentions and activity lines (#301)"
```

---

### Task 6: Team canvas (`team-canvas.tsx`) and the shared graph-file row

**Files:**
- Create: `apps/studio/src/components/graph-file-row.tsx`
- Create: `apps/studio/src/components/team-canvas.tsx`
- Modify: `apps/studio/src/components/board.tsx:1276-1312` (replace the inline `.graph-file` row with `<GraphFileRow …/>`)
- Modify: `apps/studio/src/styles.css` (append Team canvas rules)
- Test: `apps/studio/src/components/team-canvas.test.tsx`

**Interfaces:**
- Consumes: `Bot`, `BotTask`, `OtherRecorder`, `TeamLink` (Task 2); `GraphNode` from `../graph/model`; `ago` from `./format`.
- Produces:

```ts
// graph-file-row.tsx
export interface GraphFileRowProps { graphFile: string; onGraphFileChange: (value: string) => void; onDrawConnections: () => void; busy: boolean; demonstration?: boolean; fixtureFile?: string; onFixtureFileChange?: (value: string) => void; inputRef?: React.Ref<HTMLInputElement> }
export function GraphFileRow(props: GraphFileRowProps): JSX.Element;
// team-canvas.tsx
export interface TeamCanvasProps { storageKey: string; bots: Bot[]; otherRecorders: OtherRecorder[]; links: TeamLink[]; unassignedSteps: GraphNode[]; selectedBot: string | null; onSelectBot: (key: string) => void; onOpenBotDetails: (key: string) => void; onOpenNode: (nodeId: string) => void; onOpenTask: (bot: Bot, task: BotTask) => void; graphFileRow: React.ReactNode; graphFileOpen: boolean; onGraphFileOpenChange: (open: boolean) => void }
export function botStateLabel(bot: Bot): string;
export function defaultBotPosition(index: number): { x: number; y: number };
export function TeamCanvas(props: TeamCanvasProps): JSX.Element;
```

- [ ] **Step 1: Write the failing test** — create `apps/studio/src/components/team-canvas.test.tsx`:

```tsx
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import type { Bot } from "../runtime/team";
import { botStateLabel, defaultBotPosition, TeamCanvas, type TeamCanvasProps } from "./team-canvas";

const userEvent = fastUserEvent();
const bot = (key: string, state: Bot["state"], extra: Partial<Bot> = {}): Bot => ({ key, actorId: key, name: key, hue: 120, role: null,
  doingNow: `${key} is busy`, lastRecordAt: null, lastSequence: 1, state, quietMinutes: state === "quiet" ? 50 : 1, shared: false, native: false, tasks: [], ...extra });

function props(overrides: Partial<TeamCanvasProps> = {}): TeamCanvasProps {
  return {
    storageKey: "graphhelm.team-positions:p:run", otherRecorders: [], links: [], unassignedSteps: [], selectedBot: null,
    bots: [bot("coordinator", "working"), bot("kit-1", "waiting_for_you", { tasks: [{ id: "t1", title: "Build cart", done: false, source: "task_record", nodeId: null, sequence: 3, doneSequence: null }] }), bot("kit-2", "quiet")],
    onSelectBot: vi.fn(), onOpenBotDetails: vi.fn(), onOpenNode: vi.fn(), onOpenTask: vi.fn(), graphFileRow: null,
    graphFileOpen: false, onGraphFileOpenChange: vi.fn(), ...overrides,
  };
}

afterEach(() => { cleanup(); localStorage.clear(); });

describe("TeamCanvas", () => {
  it("shows every bot with its state in words, never 'stuck'", () => {
    render(<TeamCanvas {...props()} />);
    expect(screen.getByRole("button", { name: "coordinator, Working" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "kit-1, Waiting for you" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "kit-2, No new record for 50 min" })).toBeInTheDocument();
    expect(screen.getByLabelText("Team")).not.toHaveTextContent(/stuck/i);
    expect(botStateLabel(bot("x", "quiet", { quietMinutes: null }))).toBe("No record yet");
  });

  it("opens a bot's thread on click and its task on the task button", async () => {
    const p = props();
    render(<TeamCanvas {...p} />);
    await userEvent.click(screen.getByRole("button", { name: "kit-1, Waiting for you" }));
    expect(p.onSelectBot).toHaveBeenCalledWith("kit-1");
    await userEvent.click(screen.getByRole("button", { name: "Build cart" }));
    expect(p.onOpenTask).toHaveBeenCalledWith(p.bots[1], p.bots[1].tasks[0]);
  });

  it("draws a live line only for a pair that exchanged a record in the last minute", () => {
    const { container } = render(<TeamCanvas {...props({ links: [
      { a: "coordinator", b: "kit-1", count: 3, lastAt: null, live: true },
      { a: "kit-1", b: "kit-2", count: 1, lastAt: null, live: false },
    ] })} />);
    expect(container.querySelectorAll(".team-link")).toHaveLength(2);
    expect(container.querySelectorAll(".team-link-live")).toHaveLength(1);
    expect(within(screen.getByRole("list", { name: "Who talks to whom" })).getAllByRole("listitem")).toHaveLength(2);
  });

  it("folds other recorders into one expandable line", () => {
    render(<TeamCanvas {...props({ otherRecorders: [{ actorId: "merge-1", count: 2, lastRecordAt: null }, { actorId: "merge-2", count: 1, lastRecordAt: null }] })} />);
    expect(screen.getByText("2 other recorders")).toBeInTheDocument();
  });

  it("keeps a dragged position per run and falls back to the row on junk storage", () => {
    const p = props();
    const { unmount } = render(<TeamCanvas {...p} />);
    const grip = screen.getByRole("button", { name: "Move kit-2" });
    fireEvent.pointerDown(grip, { clientX: 0, clientY: 0, pointerId: 1 });
    fireEvent.pointerMove(screen.getByLabelText("Team sheet"), { clientX: 30, clientY: 40, pointerId: 1 });
    fireEvent.pointerUp(screen.getByLabelText("Team sheet"), { clientX: 30, clientY: 40, pointerId: 1 });
    const stored = JSON.parse(localStorage.getItem(p.storageKey)!) as Record<string, { x: number; y: number }>;
    expect(stored["kit-2"]).toEqual({ x: defaultBotPosition(2).x + 30, y: defaultBotPosition(2).y + 40 });
    unmount();
    localStorage.setItem(p.storageKey, "{not json");
    render(<TeamCanvas {...p} />);
    expect(screen.getByTestId("team-bot-kit-2")).toHaveStyle({ left: `${defaultBotPosition(2).x}px` });
  });

  it("lists graph steps no bot owns so the node panel stays reachable", async () => {
    const p = props({ unassignedSteps: [{ id: "start", declaredName: "Start", state: "waiting_input" } as never] });
    render(<TeamCanvas {...p} />);
    await userEvent.click(screen.getByRole("button", { name: "Start · waiting input" }));
    expect(p.onOpenNode).toHaveBeenCalledWith("start");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/team-canvas.test.tsx`
Expected: FAIL — `Cannot find module './team-canvas'`.

- [ ] **Step 3: Implement `graph-file-row.tsx`** — move the JSX of `board.tsx` lines 1276–1312 verbatim into a component:

```tsx
import type { Ref } from "react";
import { FileCode2, Waypoints } from "lucide-react";

export interface GraphFileRowProps {
  graphFile: string;
  onGraphFileChange: (value: string) => void;
  onDrawConnections: () => void;
  busy: boolean;
  demonstration?: boolean;
  fixtureFile?: string;
  onFixtureFileChange?: (value: string) => void;
  inputRef?: Ref<HTMLInputElement>;
}

/** The graph file on the Runtime's host: one field for both connections and resume. */
export function GraphFileRow({ graphFile, onGraphFileChange, onDrawConnections, busy, demonstration = false, fixtureFile = "", onFixtureFileChange, inputRef }: GraphFileRowProps) {
  return (
    <div className="graph-file">
      <span className="wrap">
        <FileCode2 aria-hidden="true" />
        <label>
          <span className="sr-only">Graph file path on the Runtime host</span>
          <input ref={inputRef} value={graphFile} onChange={(event) => onGraphFileChange(event.target.value)} placeholder="Graph file on the Runtime host…" />
        </label>
      </span>
      <button type="button" onClick={onDrawConnections} disabled={busy || graphFile.trim().length === 0} title="Read this file's shape and check it against the hash this run recorded">
        <Waypoints aria-hidden="true" />
        connect
      </button>
      {demonstration && onFixtureFileChange && (
        <span className="wrap">
          <FileCode2 aria-hidden="true" />
          <label>
            <span className="sr-only">Fixture file path on the Runtime host, sent with resume</span>
            <input value={fixtureFile} onChange={(event) => onFixtureFileChange(event.target.value)} placeholder="Fixture file for resume (optional)…"
              title="A demonstration run's outcomes come from a fixture file. Name one here and resume sends it; leave it empty and the resumed node waits for input." />
          </label>
        </span>
      )}
    </div>
  );
}
```

In `board.tsx`, replace that block with `<GraphFileRow graphFile={graphFile} onGraphFileChange={onGraphFileChange} onDrawConnections={onDrawConnections} busy={busy} demonstration={demonstration} fixtureFile={fixtureFile} onFixtureFileChange={onFixtureFileChange} inputRef={fileRef} />` and drop `Waypoints` / `FileCode2` from its lucide import if now unused.

- [ ] **Step 4: Implement `team-canvas.tsx`**

```tsx
/**
 * The Team tab (spec §4.2): bots in a row, their tasks below, a line between two bots that
 * addressed each other. Motion means new records - a line animates only while its pair spoke in
 * the last minute. Positions are a per-viewer convenience in localStorage, never shared state.
 */
import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { FileCode2, RotateCcw } from "lucide-react";

import type { GraphNode } from "../graph/model";
import type { Bot, BotTask, OtherRecorder, TeamLink } from "../runtime/team";
import { ago, readable } from "./format";

export interface TeamCanvasProps {
  storageKey: string;
  bots: Bot[];
  otherRecorders: OtherRecorder[];
  links: TeamLink[];
  unassignedSteps: GraphNode[];
  selectedBot: string | null;
  onSelectBot: (key: string) => void;
  onOpenBotDetails: (key: string) => void;
  onOpenNode: (nodeId: string) => void;
  onOpenTask: (bot: Bot, task: BotTask) => void;
  graphFileRow: ReactNode;
  graphFileOpen: boolean;
  onGraphFileOpenChange: (open: boolean) => void;
}

const BOT_W = 220;
const BOT_GAP = 48;
const BOT_FACE_H = 120;
type Point = { x: number; y: number };

export function defaultBotPosition(index: number): Point {
  return { x: 24 + index * (BOT_W + BOT_GAP), y: 32 };
}

export function botStateLabel(bot: Bot): string {
  switch (bot.state) {
    case "working": return "Working";
    case "waiting_for_you": return "Waiting for you";
    case "done": return "Done";
    case "quiet": return bot.quietMinutes === null ? "No record yet" : `No new record for ${bot.quietMinutes} min`;
  }
}

function readPositions(key: string): Record<string, Point> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(key) ?? "{}");
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: Record<string, Point> = {};
    for (const [id, value] of Object.entries(parsed as Record<string, unknown>)) {
      const point = value as { x?: unknown; y?: unknown } | null;
      if (point && Number.isFinite(point.x) && Number.isFinite(point.y)) out[id] = { x: point.x as number, y: point.y as number };
    }
    return out;
  } catch {
    return {};
  }
}

function writePositions(key: string, positions: Record<string, Point>): void {
  try { localStorage.setItem(key, JSON.stringify(positions)); } catch { /* per-viewer convenience only */ }
}

export function TeamCanvas(props: TeamCanvasProps) {
  const { bots, links } = props;
  const [positions, setPositions] = useState<Record<string, Point>>(() => readPositions(props.storageKey));
  const [view, setView] = useState({ x: 0, y: 0, zoom: 1 });
  const drag = useRef<{ kind: "bot" | "pan"; id: string; start: Point; origin: Point } | null>(null);
  useEffect(() => { setPositions(readPositions(props.storageKey)); }, [props.storageKey]);

  const placeOf = (key: string, index: number): Point => positions[key] ?? defaultBotPosition(index);
  const centre = (key: string): Point | null => {
    const index = bots.findIndex((bot) => bot.key === key);
    if (index < 0) return null;
    const at = placeOf(key, index);
    return { x: at.x + BOT_W / 2, y: at.y + BOT_FACE_H / 2 };
  };
  const nameOf = (key: string) => bots.find((bot) => bot.key === key)?.name ?? key;

  const onSheetDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    drag.current = { kind: "pan", id: "", start: { x: event.clientX, y: event.clientY }, origin: { x: view.x, y: view.y } };
  };
  const onMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const held = drag.current;
    if (held === null) return;
    const dx = event.clientX - held.start.x;
    const dy = event.clientY - held.start.y;
    if (held.kind === "pan") setView((current) => ({ ...current, x: held.origin.x + dx, y: held.origin.y + dy }));
    else setPositions((current) => ({ ...current, [held.id]: { x: held.origin.x + dx / view.zoom, y: held.origin.y + dy / view.zoom } }));
  };
  const onUp = () => {
    if (drag.current?.kind === "bot") setPositions((current) => { writePositions(props.storageKey, current); return current; });
    drag.current = null;
  };

  const stepsX = defaultBotPosition(bots.length).x;
  return (
    <section className="team-canvas" aria-label="Team">
      <div className="team-toolbar">
        <button type="button" onClick={() => { setPositions({}); writePositions(props.storageKey, {}); setView({ x: 0, y: 0, zoom: 1 }); }}>
          <RotateCcw aria-hidden="true" /> Reset layout
        </button>
        <button type="button" aria-expanded={props.graphFileOpen} onClick={() => props.onGraphFileOpenChange(!props.graphFileOpen)}>
          <FileCode2 aria-hidden="true" /> Graph file
        </button>
        {props.graphFileOpen && props.graphFileRow}
      </div>
      <div className="team-sheet" aria-label="Team sheet" onPointerDown={onSheetDown} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={onUp}
        onWheel={(event) => {
          if (!event.ctrlKey) return;
          setView((current) => ({ ...current, zoom: Math.min(2, Math.max(0.4, current.zoom * (1 - event.deltaY * 0.0015))) }));
        }}>
        <div className="team-world" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})` }}>
          <svg className="team-links" aria-hidden="true" width={stepsX + BOT_W} height={640}>
            {links.map((link) => {
              const a = centre(link.a);
              const b = centre(link.b);
              if (a === null || b === null) return null;
              return <line key={`${link.a}+${link.b}`} className={`team-link ${link.live ? "team-link-live" : ""}`} x1={a.x} y1={a.y} x2={b.x} y2={b.y} />;
            })}
          </svg>
          <ul className="sr-only" aria-label="Who talks to whom">
            {links.map((link) => <li key={`${link.a}+${link.b}`}>{nameOf(link.a)} and {nameOf(link.b)}: {link.count} recorded messages{link.live ? ", just now" : ""}</li>)}
          </ul>
          {bots.map((bot, index) => {
            const at = placeOf(bot.key, index);
            return (
              <article key={bot.key} data-testid={`team-bot-${bot.key}`} className={`team-bot team-bot-${bot.state} ${props.selectedBot === bot.key ? "team-bot-selected" : ""}`}
                style={{ left: `${at.x}px`, top: `${at.y}px`, "--bot-hue": String(bot.hue) } as React.CSSProperties}>
                <button type="button" className="team-bot-grip" aria-label={`Move ${bot.name}`}
                  onPointerDown={(event) => { event.stopPropagation(); drag.current = { kind: "bot", id: bot.key, start: { x: event.clientX, y: event.clientY }, origin: at }; }}>⋮⋮</button>
                <button type="button" className="team-bot-face" aria-label={`${bot.name}, ${botStateLabel(bot)}`} onClick={() => props.onSelectBot(bot.key)}>
                  <span className="team-bot-avatar" aria-hidden="true">{bot.name.slice(0, 1).toUpperCase()}</span>
                  <span className="team-bot-name">{bot.name}</span>
                  <span className="team-bot-state">{botStateLabel(bot)}</span>
                </button>
                {bot.role && <p className="team-bot-role">{bot.role}</p>}
                <p className="team-bot-doing">{bot.doingNow}</p>
                {bot.shared && <p className="team-bot-note">Shared actor: its records cannot be attributed to one chat.</p>}
                <p className="team-bot-when">{bot.lastRecordAt === null ? "no record" : `last record ${ago(bot.lastRecordAt)}`}</p>
                <button type="button" className="team-bot-details" onClick={() => props.onOpenBotDetails(bot.key)}>Details</button>
                {bot.tasks.length > 0 && (
                  <ol className="team-tasks" aria-label={`${bot.name} tasks`}>
                    {bot.tasks.map((task) => (
                      <li key={task.id} className={task.done ? "team-task-done" : ""}>
                        <button type="button" onClick={() => props.onOpenTask(bot, task)}>{task.title}</button>
                      </li>
                    ))}
                  </ol>
                )}
              </article>
            );
          })}
          {props.unassignedSteps.length > 0 && (
            <article className="team-steps" style={{ left: `${stepsX}px`, top: "32px" }} aria-label="Steps without a bot">
              <h3>Steps without a bot</h3>
              <ol>
                {props.unassignedSteps.map((node) => (
                  <li key={node.id}><button type="button" onClick={() => props.onOpenNode(node.id)}>{`${node.declaredName ?? node.id} · ${readable(node.state)}`}</button></li>
                ))}
              </ol>
            </article>
          )}
        </div>
      </div>
      {props.otherRecorders.length > 0 && (
        <details className="team-others">
          <summary>{props.otherRecorders.length} other recorders</summary>
          <ul>{props.otherRecorders.map((other) => <li key={other.actorId}>{other.actorId} · {other.count} records · {ago(other.lastRecordAt)}</li>)}</ul>
        </details>
      )}
    </section>
  );
}
```

Add `import type { CSSProperties } from "react";` and use `as CSSProperties` instead of `React.CSSProperties` if the file has no `React` namespace import. Confirm `readable("waiting_input")` returns `"waiting input"` (`components/format.ts`); the unassigned-step test name depends on it.

- [ ] **Step 5: Add styles** — append to `apps/studio/src/styles.css` (tokens `--surface`, `--ink`, `--muted`, `--line`, `--amber` must already exist; run `grep -n "^\s*--" src/styles.css | head -40` and substitute the real token names before saving):

```css
/* Team canvas (spec §4.2). */
.team-canvas { position: relative; display: flex; flex-direction: column; height: 100%; min-height: 0; }
.team-toolbar { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--line); }
.team-toolbar button { display: inline-flex; gap: 6px; align-items: center; }
.team-sheet { position: relative; flex: 1; overflow: hidden; touch-action: none; cursor: grab; }
.team-world { position: absolute; inset: 0; transform-origin: 0 0; }
.team-links { position: absolute; left: 0; top: 0; pointer-events: none; overflow: visible; }
.team-link { stroke: var(--line); stroke-width: 1.5; }
.team-link-live { stroke: var(--ink); stroke-width: 2.5; stroke-dasharray: 6 6; animation: team-link-flow 1.2s linear infinite; }
@keyframes team-link-flow { to { stroke-dashoffset: -12; } }
.team-bot { position: absolute; width: 220px; padding: 12px; border: 1px solid var(--line); border-radius: 14px; background: var(--surface); }
.team-bot-selected { outline: 2px solid hsl(var(--bot-hue) 70% 60%); }
.team-bot-working .team-bot-avatar { animation: team-pulse 2.4s ease-in-out infinite; }
@keyframes team-pulse { 50% { box-shadow: 0 0 0 6px hsl(var(--bot-hue) 70% 60% / 0.25); } }
.team-bot-waiting_for_you { border-color: var(--amber); box-shadow: 0 0 0 2px var(--amber); }
.team-bot-quiet { opacity: 0.6; }
.team-bot-done .team-bot-state { color: var(--muted); }
.team-bot-grip { position: absolute; right: 8px; top: 8px; cursor: grab; background: none; border: 0; color: var(--muted); }
.team-bot-face { display: grid; grid-template-columns: 36px 1fr; gap: 2px 10px; align-items: center; width: 100%; text-align: left; background: none; border: 0; padding: 0; color: inherit; }
.team-bot-avatar { grid-row: span 2; display: grid; place-items: center; width: 36px; height: 36px; border-radius: 50%; background: hsl(var(--bot-hue) 60% 45%); color: #fff; font-weight: 600; }
.team-bot-name { font-weight: 600; }
.team-bot-state, .team-bot-when, .team-bot-role, .team-bot-note { font-size: 12px; color: var(--muted); margin: 0; }
.team-bot-doing { margin: 8px 0 4px; font-size: 13px; }
.team-bot-details { font-size: 12px; }
.team-tasks { list-style: none; margin: 10px 0 0; padding: 0; display: grid; gap: 6px; }
.team-tasks button { width: 100%; text-align: left; padding: 6px 8px; border: 1px solid var(--line); border-radius: 8px; background: none; color: inherit; }
.team-task-done button { text-decoration: line-through; color: var(--muted); }
.team-steps { position: absolute; width: 220px; padding: 12px; border: 1px dashed var(--line); border-radius: 14px; }
.team-steps h3 { margin: 0 0 8px; font-size: 13px; }
.team-steps ol { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
.team-others { padding: 8px 12px; border-top: 1px solid var(--line); font-size: 12px; }
```

- [ ] **Step 6: Run tests and typecheck**

Run: `npx vitest run src/components/team-canvas.test.tsx src/components/board.test.tsx src/styles.test.ts && npx tsc -b`
Expected: PASS. `styles.test.ts` "has a rule for every class the components render" must stay green.

- [ ] **Step 7: Commit**

```bash
git add src/components/graph-file-row.tsx src/components/team-canvas.tsx src/components/team-canvas.test.tsx src/components/board.tsx src/styles.css
git commit -m "feat(studio): team canvas with live lines and persisted layout (#301)"
```

---

### Task 7: Beacon and question cards (`beacon.tsx`)

**Files:**
- Create: `apps/studio/src/components/beacon.tsx`
- Modify: `apps/studio/src/styles.css`
- Test: `apps/studio/src/components/beacon.test.tsx`

**Interfaces:**
- Consumes: `BeaconState`, `NeedsYouItem`, `QuestionItem`, `NativeRequestItem`, `StepItem`, `DraftItem`, `RUNTIME_SILENT` (Task 3).
- Produces:

```ts
export function beaconLabel(state: BeaconState): string;
export function Beacon(props: { state: BeaconState; onOpen: () => void }): JSX.Element;
export interface QuestionCardsProps { items: NeedsYouItem[]; names: Record<string, string>; busy: boolean; onChoose: (item: QuestionItem, choice: string) => void; onAnswer: (item: QuestionItem) => void; onCheck: (item: NativeRequestItem) => void; stepActions: (item: StepItem | DraftItem) => React.ReactNode }
export const NEEDS_YOU_ID = "needs-you";
export function QuestionCards(props: QuestionCardsProps): JSX.Element | null;
```

- [ ] **Step 1: Write the failing test** — create `apps/studio/src/components/beacon.test.tsx`:

```tsx
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import { RUNTIME_SILENT, type NeedsYouItem, type QuestionItem } from "../runtime/needs-you";
import { Beacon, beaconLabel, QuestionCards } from "./beacon";

const userEvent = fastUserEvent();
afterEach(cleanup);

const question: QuestionItem = { kind: "question", key: "question:sig-q", asker: "kit-3", text: "Merge now?", signalId: "sig-q",
  recommendations: ["Wait", "Merge"], at: null, sequence: 5 };

describe("Beacon", () => {
  it("names lit, dark and unknown in words", () => {
    expect(beaconLabel({ kind: "lit", count: 1 })).toBe("1 decision needs you");
    expect(beaconLabel({ kind: "lit", count: 3 })).toBe("3 decisions need you");
    expect(beaconLabel({ kind: "dark" })).toBe("Nothing needs you");
    expect(beaconLabel({ kind: "unknown", reason: RUNTIME_SILENT })).toBe(RUNTIME_SILENT);
  });

  it("never renders unknown as dark", () => {
    const { container } = render(<Beacon state={{ kind: "unknown", reason: RUNTIME_SILENT }} onOpen={vi.fn()} />);
    expect(container.querySelector(".beacon-dark")).toBeNull();
    expect(screen.getByRole("button", { name: RUNTIME_SILENT })).toHaveClass("beacon-unknown");
  });

  it("opens the cards on click", async () => {
    const onOpen = vi.fn();
    render(<Beacon state={{ kind: "lit", count: 1 }} onOpen={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: "1 decision needs you" }));
    expect(onOpen).toHaveBeenCalled();
  });
});

describe("QuestionCards", () => {
  const handlers = () => ({ onChoose: vi.fn(), onAnswer: vi.fn(), onCheck: vi.fn(), stepActions: vi.fn(() => <button type="button">approve cart</button>) });

  it("turns a question's recommendations into one row of choice buttons plus Answer, with no Refuse in phase 1", async () => {
    const h = handlers();
    render(<QuestionCards items={[question]} names={{ "kit-3": "loja kit 3" }} busy={false} {...h} />);
    const card = screen.getByRole("article", { name: "loja kit 3 asks" });
    expect(within(card).getAllByRole("button").map((button) => button.textContent)).toEqual(["Wait", "Merge", "Answer"]);
    await userEvent.click(within(card).getByRole("button", { name: "Merge" }));
    expect(h.onChoose).toHaveBeenCalledWith(question, "Merge");
    await userEvent.click(within(card).getByRole("button", { name: "Answer" }));
    expect(h.onAnswer).toHaveBeenCalledWith(question);
    expect(within(card).queryByRole("button", { name: /refuse/i })).toBeNull();
  });

  it("says an unconfirmed native request in plain words and keeps transport ids in details", async () => {
    const h = handlers();
    const item: NeedsYouItem = { kind: "native_request", key: "native:r1", requestId: "r1", threadId: "t-1", nodeId: "start", title: "loja kit 2", state: "unobserved", detail: "Send outcome is unobserved." };
    render(<QuestionCards items={[item]} names={{}} busy={false} {...h} />);
    expect(screen.getByText("Your message to loja kit 2 was not confirmed. It may not have arrived.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Check it" }));
    expect(h.onCheck).toHaveBeenCalledWith(item);
    expect(screen.getByText("r1").closest("details")).not.toBeNull();
  });

  it("delegates step and draft actions to the page", () => {
    const h = handlers();
    render(<QuestionCards items={[{ kind: "blocked_step", key: "blocked_node:cart", nodeId: "cart", name: "Cart page", reason: "blocked_node" }]} names={{}} busy={false} {...h} />);
    expect(screen.getByText("Cart page is blocked.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "approve cart" })).toBeInTheDocument();
  });

  it("renders nothing when nothing needs you", () => {
    const { container } = render(<QuestionCards items={[]} names={{}} busy={false} {...handlers()} />);
    expect(container).toBeEmptyDOMElement();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/beacon.test.tsx`
Expected: FAIL — `Cannot find module './beacon'`.

- [ ] **Step 3: Implement** — create `apps/studio/src/components/beacon.tsx`:

```tsx
/**
 * The needs-you beacon and its question cards (spec §4.3). Dark means nothing blocks; unknown is
 * its own colour and its own words. Phase 1 offers no Refuse (owner_refusal is phase 2).
 */
import type { ReactNode } from "react";

import type { BeaconState, DraftItem, NativeRequestItem, NeedsYouItem, QuestionItem, StepItem } from "../runtime/needs-you";

export const NEEDS_YOU_ID = "needs-you";

export function beaconLabel(state: BeaconState): string {
  if (state.kind === "lit") return state.count === 1 ? "1 decision needs you" : `${state.count} decisions need you`;
  if (state.kind === "dark") return "Nothing needs you";
  return state.reason;
}

export function Beacon({ state, onOpen }: { state: BeaconState; onOpen: () => void }) {
  return (
    <button type="button" className={`beacon beacon-${state.kind}`} onClick={onOpen} aria-live="polite">
      <span className="beacon-dot" aria-hidden="true" />
      {beaconLabel(state)}
    </button>
  );
}

export interface QuestionCardsProps {
  items: NeedsYouItem[];
  names: Record<string, string>;
  busy: boolean;
  onChoose: (item: QuestionItem, choice: string) => void;
  onAnswer: (item: QuestionItem) => void;
  onCheck: (item: NativeRequestItem) => void;
  stepActions: (item: StepItem | DraftItem) => ReactNode;
}

function stepSentence(item: StepItem | DraftItem): string {
  if (item.kind === "draft") return "A proposal is waiting for your review.";
  if (item.kind === "waiting_step") return `${item.name} is waiting for your input.`;
  return item.reason === "untriaged_interruption" ? `${item.name} was interrupted and awaits triage.` : `${item.name} is blocked.`;
}

export function QuestionCards({ items, names, busy, onChoose, onAnswer, onCheck, stepActions }: QuestionCardsProps) {
  if (items.length === 0) return null;
  return (
    <section className="question-cards" id={NEEDS_YOU_ID} aria-label="Needs you" tabIndex={-1}>
      {items.map((item) => {
        if (item.kind === "question") {
          const title = `${names[item.asker] ?? item.asker} asks`;
          return (
            <article key={item.key} className="question-card" aria-label={title}>
              <h3>{title}</h3>
              <p className="question-text">{item.text.length > 400 ? `${item.text.slice(0, 400)}…` : item.text}</p>
              <div className="question-actions">
                {item.recommendations.map((choice) => (
                  <button key={choice} type="button" disabled={busy} onClick={() => onChoose(item, choice)}>{choice}</button>
                ))}
                <button type="button" className="question-answer" disabled={busy} onClick={() => onAnswer(item)}>Answer</button>
              </div>
            </article>
          );
        }
        if (item.kind === "native_request") {
          return (
            <article key={item.key} className="question-card" aria-label={`Message to ${item.title}`}>
              <p className="question-text">{item.state === "blocked"
                ? `Your message to ${item.title} was refused. Check the original chat before sending another.`
                : `Your message to ${item.title} was not confirmed. It may not have arrived.`}</p>
              <div className="question-actions"><button type="button" disabled={busy} onClick={() => onCheck(item)}>Check it</button></div>
              <details className="question-details">
                <summary>Details</summary>
                <dl>
                  <dt>Request ID</dt><dd><code>{item.requestId}</code></dd>
                  <dt>Thread ID</dt><dd><code>{item.threadId}</code></dd>
                  <dt>Node ID</dt><dd><code>{item.nodeId}</code></dd>
                  <dt>State</dt><dd>{item.state}</dd>
                  {item.detail && <><dt>Diagnostic</dt><dd>{item.detail}</dd></>}
                </dl>
              </details>
            </article>
          );
        }
        return (
          <article key={item.key} className="question-card" aria-label={stepSentence(item)}>
            <p className="question-text">{stepSentence(item)}</p>
            <div className="question-actions">{stepActions(item)}</div>
          </article>
        );
      })}
    </section>
  );
}
```

- [ ] **Step 4: Add styles** — append to `styles.css` (substitute real token names as in Task 6):

```css
/* Beacon and question cards (spec §4.3). */
.beacon { display: inline-flex; align-items: center; gap: 8px; padding: 6px 14px; border-radius: 999px; border: 1px solid var(--line); background: none; color: inherit; font-weight: 600; }
.beacon-dot { width: 10px; height: 10px; border-radius: 50%; background: var(--muted); }
.beacon-lit { border-color: var(--amber); }
.beacon-lit .beacon-dot { background: var(--amber); animation: team-pulse 2.4s ease-in-out infinite; }
.beacon-dark .beacon-dot { background: transparent; border: 1px solid var(--muted); }
.beacon-unknown { border-style: dashed; color: var(--muted); }
.beacon-unknown .beacon-dot { background: repeating-linear-gradient(45deg, var(--muted) 0 2px, transparent 2px 4px); }
.question-cards { display: grid; gap: 8px; padding: 8px; }
.question-card { padding: 10px 12px; border: 1px solid var(--amber); border-radius: 12px; background: var(--surface); }
.question-card h3 { margin: 0 0 4px; font-size: 13px; }
.question-text { margin: 0 0 8px; white-space: pre-wrap; }
.question-actions { display: flex; flex-wrap: wrap; gap: 6px; }
.question-answer { font-weight: 600; }
.question-details { margin-top: 6px; font-size: 12px; color: var(--muted); }
```

- [ ] **Step 5: Run tests and typecheck**

Run: `npx vitest run src/components/beacon.test.tsx src/styles.test.ts && npx tsc -b`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/beacon.tsx src/components/beacon.test.tsx src/styles.css
git commit -m "feat(studio): needs-you beacon and one-button question cards (#301)"
```

---

### Task 8: Handover card and right panel

**Files:**
- Create: `apps/studio/src/components/handover-card.tsx`
- Create: `apps/studio/src/components/right-panel.tsx`
- Modify: `apps/studio/src/styles.css`
- Test: `apps/studio/src/components/handover-card.test.tsx`, `apps/studio/src/components/right-panel.test.tsx`

**Interfaces:**
- Consumes: `Handover`, `HandoverLine` (Task 4); `ActivityLine` (Task 5); `ago` from `./format`.
- Produces:

```ts
export function HandoverCard(props: { handover: Handover; onOpen: (sequences: number[]) => void; onDismiss: () => void }): JSX.Element;
export function RightPanel(props: { activity: ActivityLine[]; onOpenActivity: (sequence: number) => void }): JSX.Element;
```

- [ ] **Step 1: Write the failing tests**

`apps/studio/src/components/handover-card.test.tsx`:

```tsx
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import type { Handover } from "../runtime/handover";
import { HandoverCard } from "./handover-card";

const userEvent = fastUserEvent();
afterEach(cleanup);

const HANDOVER: Handover = {
  fromSeq: 10, toSeq: 40, eventCount: 30, gapMinutes: 135,
  shipped: [{ text: "Cart page succeeded", sequences: [11] }],
  needsYou: [{ text: "kit 3 asked: Merge now?", sequences: [13] }],
  quiet: [], untouched: [{ text: "Payment got no record", sequences: [2] }],
};

describe("HandoverCard", () => {
  it("summarises the gap in four groups and says Nothing for an empty one", () => {
    render(<HandoverCard handover={HANDOVER} onOpen={vi.fn()} onDismiss={vi.fn()} />);
    const card = screen.getByRole("dialog", { name: "While you were away" });
    expect(card).toHaveTextContent("30 records over 2 h 15 min");
    expect(within(screen.getByRole("region", { name: "Went quiet" })).getByText("Nothing.")).toBeInTheDocument();
  });

  it("opens the records a line cites and advances only on Got it", async () => {
    const onOpen = vi.fn();
    const onDismiss = vi.fn();
    render(<HandoverCard handover={HANDOVER} onOpen={onOpen} onDismiss={onDismiss} />);
    await userEvent.click(screen.getByRole("button", { name: "Cart page succeeded" }));
    expect(onOpen).toHaveBeenCalledWith([11]);
    expect(onDismiss).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(onDismiss).toHaveBeenCalled();
  });
});
```

`apps/studio/src/components/right-panel.test.tsx`:

```tsx
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import { RightPanel } from "./right-panel";

const userEvent = fastUserEvent();
afterEach(cleanup);

describe("RightPanel", () => {
  it("shows honest empty states for journeys and before/after until phase 4 data exists", () => {
    render(<RightPanel activity={[]} onOpenActivity={vi.fn()} />);
    expect(screen.getByRole("region", { name: "Journeys" })).toHaveTextContent("No journeys mapped yet");
    expect(screen.getByRole("region", { name: "Before and after" })).toHaveTextContent("No before/after screenshots yet");
    expect(screen.getByRole("region", { name: "What just happened" })).toHaveTextContent("Nothing recorded yet");
  });

  it("lists what just happened as bot verb object, each opening its record", async () => {
    const onOpen = vi.fn();
    render(<RightPanel activity={[{ sequence: 7, text: "loja kit 1 asked you “Merge now?”", at: null }]} onOpenActivity={onOpen} />);
    await userEvent.click(screen.getByRole("button", { name: /loja kit 1 asked you/ }));
    expect(onOpen).toHaveBeenCalledWith(7);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/components/handover-card.test.tsx src/components/right-panel.test.tsx`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement `handover-card.tsx`**

```tsx
import type { Handover, HandoverLine } from "../runtime/handover";

function span(minutes: number): string {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return hours === 0 ? `${rest} min` : rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

function Group({ title, lines, onOpen }: { title: string; lines: HandoverLine[]; onOpen: (sequences: number[]) => void }) {
  return (
    <section className="handover-group" aria-label={title}>
      <h3>{title}</h3>
      {lines.length === 0 ? <p className="handover-empty">Nothing.</p> : (
        <ul>{lines.map((line) => <li key={`${line.text}:${line.sequences.join(",")}`}><button type="button" onClick={() => onOpen(line.sequences)}>{line.text}</button></li>)}</ul>
      )}
    </section>
  );
}

/** "While you were away" (spec §4.5). It overlays the canvas; only Got it (or ten visible
 * seconds of the live view, handled by the page) moves the last-seen position. */
export function HandoverCard({ handover, onOpen, onDismiss }: { handover: Handover; onOpen: (sequences: number[]) => void; onDismiss: () => void }) {
  return (
    <section className="handover-card" role="dialog" aria-modal="false" aria-label="While you were away">
      <h2>While you were away</h2>
      <p className="handover-span">{handover.eventCount} records over {span(handover.gapMinutes)}</p>
      <Group title="Shipped" lines={handover.shipped} onOpen={onOpen} />
      <Group title="Needs you" lines={handover.needsYou} onOpen={onOpen} />
      <Group title="Went quiet" lines={handover.quiet} onOpen={onOpen} />
      <Group title="Nobody touched" lines={handover.untouched} onOpen={onOpen} />
      <button type="button" className="handover-dismiss" onClick={onDismiss}>Got it</button>
    </section>
  );
}
```

Note: the group title "Needs you" is a region named "Needs you" inside the dialog; the question-cards section is also named "Needs you". App tests must scope with `within(dialog)` when both are on screen.

- [ ] **Step 4: Implement `right-panel.tsx`**

```tsx
import type { ActivityLine } from "../runtime/threads";
import { ago } from "./format";

/** Right panel (spec §4.6). Journeys and Before / after get their data in phases 4-5; until then
 * they say so instead of drawing an empty chart. */
export function RightPanel({ activity, onOpenActivity }: { activity: ActivityLine[]; onOpenActivity: (sequence: number) => void }) {
  return (
    <aside className="right-panel" aria-label="Run side panel">
      <section className="right-section" aria-label="Journeys">
        <h2>Journeys</h2>
        <p className="right-empty">No journeys mapped yet.</p>
      </section>
      <section className="right-section" aria-label="Before and after">
        <h2>Before / after</h2>
        <p className="right-empty">No before/after screenshots yet.</p>
      </section>
      <section className="right-section" aria-label="What just happened">
        <h2>What just happened</h2>
        {activity.length === 0 ? <p className="right-empty">Nothing recorded yet.</p> : (
          <ol className="right-activity">
            {activity.map((line) => (
              <li key={line.sequence}><button type="button" onClick={() => onOpenActivity(line.sequence)}>{line.text}<span className="right-when"> · {ago(line.at)}</span></button></li>
            ))}
          </ol>
        )}
      </section>
    </aside>
  );
}
```

- [ ] **Step 5: Add styles** (substitute real tokens):

```css
/* Handover card (spec §4.5) and right panel (§4.6). */
.handover-card { position: absolute; right: 16px; top: 56px; z-index: 20; width: min(420px, calc(100% - 32px)); max-height: calc(100% - 72px); overflow: auto; padding: 16px; border: 1px solid var(--line); border-radius: 16px; background: var(--surface); box-shadow: 0 12px 32px rgb(0 0 0 / 0.35); }
.handover-card h2 { margin: 0; font-size: 16px; }
.handover-span { margin: 4px 0 12px; color: var(--muted); font-size: 12px; }
.handover-group h3 { margin: 12px 0 4px; font-size: 13px; }
.handover-group ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
.handover-group button { width: 100%; text-align: left; background: none; border: 0; padding: 4px 0; color: inherit; text-decoration: underline dotted; }
.handover-empty { margin: 0; color: var(--muted); font-size: 12px; }
.handover-dismiss { margin-top: 12px; font-weight: 600; }
.right-panel { display: flex; flex-direction: column; gap: 16px; padding: 12px; overflow: auto; border-left: 1px solid var(--line); }
.right-section h2 { margin: 0 0 6px; font-size: 13px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); }
.right-empty { margin: 0; font-size: 13px; color: var(--muted); }
.right-activity { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
.right-activity button { width: 100%; text-align: left; background: none; border: 0; padding: 0; color: inherit; font-size: 13px; }
.right-when { color: var(--muted); }
```

- [ ] **Step 6: Run tests and typecheck**

Run: `npx vitest run src/components/handover-card.test.tsx src/components/right-panel.test.tsx src/styles.test.ts && npx tsc -b`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/components/handover-card.tsx src/components/handover-card.test.tsx src/components/right-panel.tsx src/components/right-panel.test.tsx src/styles.css
git commit -m "feat(studio): handover card and right panel (#301)"
```

---

### Task 9: Chat column and MainChat slimming

**Files:**
- Create: `apps/studio/src/components/chat-column.tsx`
- Modify: `apps/studio/src/components/main-chat.tsx`
- Modify: `apps/studio/src/components/main-chat.test.tsx`
- Modify: `apps/studio/src/App.tsx:2582-2590` (the `<MainChat …>` call only — drop the three reply props)
- Modify: `apps/studio/src/styles.css`
- Test: `apps/studio/src/components/chat-column.test.tsx`

**Interfaces:**
- Consumes: `ChatThread`, `EVERYONE`, `parseMention` (Task 5); `Bot` (Task 2); `ReplySuggestion` from `../runtime/types`; `ago`, `hueOf` from `./format`.
- Produces:

```ts
// main-chat.tsx — MainChatProps after this task:
export interface MainChatProps {
  client: MainChatClient | null; executionId: string; personas: MainChatPersona[]; refreshSequence?: number; onConnect?: () => void;
  /** Thread id the Chat column selected; MainChat switches its recipient to it when linked. */
  recipientId?: string | null;
  /** Fills the instruction when it is empty (Jev "Use"); never sends. */
  seed?: { text: string; nonce: number } | null;
  /** Bumped by a question card's "Check it" to re-read the request ledger. */
  refreshNonce?: number;
  /** Every ledger merge, so the page's needs-you list sees unconfirmed requests. */
  onRequestsChange?: (requests: NativeChatRequest[]) => void;
}
// chat-column.tsx
export interface ChatColumnProps {
  threads: ChatThread[]; selected: string; onSelect: (key: string) => void; unread: Record<string, number>;
  bots: Bot[]; names: Record<string, string>; openingCount: number; cards: React.ReactNode;
  jev: { suggestions: ReplySuggestion[]; loading: boolean; issue: string | null };
  nativeKeys: ReadonlySet<string>; principal: React.ReactNode;
  onSend: (text: string, to: string | null, replyTo: string | null) => void; sending: boolean; sendError: string;
  answering: { asker: string; signalId: string | null } | null; onClearAnswer: () => void;
  composerFocus: number; highlight: number | null; onUseSuggestion: (text: string) => void;
}
export function composerMode(thread: ChatThread | undefined, nativeKeys: ReadonlySet<string>): "record" | "native" | "record+principal" | "none";
export function ChatColumn(props: ChatColumnProps): JSX.Element;
```

- [ ] **Step 1: Write the failing test** — create `apps/studio/src/components/chat-column.test.tsx`:

```tsx
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import type { Bot } from "../runtime/team";
import { chatThreads } from "../runtime/threads";
import type { WorkMessage } from "../runtime/work-conversation";
import { ChatColumn, composerMode, type ChatColumnProps } from "./chat-column";

const userEvent = fastUserEvent();
afterEach(cleanup);

const bot = (key: string, name: string, native = false): Bot => ({ key, actorId: native ? null : key, name, hue: 10, role: null, doingNow: "",
  lastRecordAt: null, lastSequence: 0, state: "working", quietMinutes: null, shared: false, native, tasks: [] });
const BOTS = [bot("coordinator", "Coordinator"), bot("kit-1", "loja kit 1"), bot("kit-2", "loja kit 2")];
const msg = (sequence: number, sender: string, to: string | null, text: string): WorkMessage => ({ id: `event-${sequence}`, sequence, sender, to,
  replyTo: null, text, at: null, provenance: "stored", acknowledged: false });
const THREADS = chatThreads([msg(1, "coordinator", null, "Plan ready"), msg(2, "kit-1", "kit-2", "Take the cart")], BOTS, "studio-operator");

function props(overrides: Partial<ChatColumnProps> = {}): ChatColumnProps {
  return {
    threads: THREADS, selected: "everyone", onSelect: vi.fn(), unread: { everyone: 0, "pair:kit-1+kit-2": 1 }, bots: BOTS,
    names: { coordinator: "Coordinator", "kit-1": "loja kit 1", "kit-2": "loja kit 2" }, openingCount: 0, cards: null,
    jev: { suggestions: [], loading: false, issue: null }, nativeKeys: new Set(), principal: <aside aria-label="Principal conversation">main</aside>,
    onSend: vi.fn(), sending: false, sendError: "", answering: null, onClearAnswer: vi.fn(), composerFocus: 0, highlight: null,
    onUseSuggestion: vi.fn(), ...overrides,
  };
}

describe("composerMode", () => {
  it("routes each tab to the right composer", () => {
    expect(composerMode(THREADS[0], new Set())).toBe("record+principal");
    expect(composerMode(THREADS[0], new Set(["t-1"]))).toBe("native");
    expect(composerMode(THREADS[1], new Set())).toBe("none");
    expect(composerMode({ key: "direct:t-1", kind: "direct", label: "x", participants: ["t-1"], messages: [] }, new Set(["t-1"]))).toBe("native");
    expect(composerMode({ key: "direct:kit-1", kind: "direct", label: "x", participants: ["kit-1"], messages: [] }, new Set())).toBe("record");
  });
});

describe("ChatColumn", () => {
  it("shows thread tabs with unread counts and labels bot pairs as recorded messages", async () => {
    const p = props();
    render(<ChatColumn {...p} />);
    expect(screen.getByRole("tab", { name: "Everyone" })).toHaveAttribute("aria-selected", "true");
    await userEvent.click(screen.getByRole("tab", { name: "loja kit 1 ↔ loja kit 2, 1 unread" }));
    expect(p.onSelect).toHaveBeenCalledWith("pair:kit-1+kit-2");
  });

  it("explains that a pair tab shows recorded messages, not native chats", () => {
    render(<ChatColumn {...props({ selected: "pair:kit-1+kit-2" })} />);
    expect(screen.getByText(/recorded messages/i)).toHaveTextContent("not their native chats");
    expect(screen.queryByRole("textbox", { name: "Message" })).toBeNull();
  });

  it("keeps sealed records counted while they open", () => {
    render(<ChatColumn {...props({ openingCount: 3 })} />);
    expect(screen.getByText("Opening 3 sealed records…")).toBeInTheDocument();
  });

  it("sends from Everyone to one bot with @name and keeps the principal conversation mounted", async () => {
    const p = props();
    render(<ChatColumn {...p} />);
    expect(screen.getByRole("complementary", { name: "Principal conversation" })).toBeInTheDocument();
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "@loja kit 2 rebase please");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(p.onSend).toHaveBeenCalledWith("rebase please", "kit-2", null);
  });

  it("answers a question with its replyTo and the asker as recipient", async () => {
    const p = props({ selected: "direct:kit-1", threads: [...THREADS, { key: "direct:kit-1", kind: "direct", label: "loja kit 1", participants: ["kit-1"], messages: [] }],
      answering: { asker: "kit-1", signalId: "sig-q" } });
    render(<ChatColumn {...p} />);
    expect(screen.getByText("Answering loja kit 1")).toBeInTheDocument();
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "Wait for review");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(p.onSend).toHaveBeenCalledWith("Wait for review", "kit-1", "sig-q");
  });

  it("puts a Jev suggestion in one dashed card whose Use fills the composer and never sends", async () => {
    const p = props({ jev: { suggestions: [{ to: null, draft: "Ask kit 2 for the cart diff", reason: "kit 2 went quiet", sourceSequences: [1] }], loading: false, issue: null } });
    render(<ChatColumn {...p} />);
    await userEvent.click(screen.getByRole("button", { name: "Use" }));
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Ask kit 2 for the cart diff");
    expect(p.onUseSuggestion).toHaveBeenCalledWith("Ask kit 2 for the cart diff");
    expect(p.onSend).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/components/chat-column.test.tsx`
Expected: FAIL — `Cannot find module './chat-column'`.

- [ ] **Step 3: Implement `chat-column.tsx`**

```tsx
/**
 * The Chat column (spec §4.4): question cards on top, thread tabs, messages, the Jev card and the
 * composer for the selected tab's audience. Native sends stay in MainChat with its gating; this
 * column only decides which composer the tab gets.
 */
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";

import type { ReplySuggestion } from "../runtime/types";
import type { Bot } from "../runtime/team";
import { EVERYONE, parseMention, type ChatThread } from "../runtime/threads";
import { ago, hueOf } from "./format";

export interface ChatColumnProps {
  threads: ChatThread[];
  selected: string;
  onSelect: (key: string) => void;
  unread: Record<string, number>;
  bots: Bot[];
  names: Record<string, string>;
  openingCount: number;
  cards: ReactNode;
  jev: { suggestions: ReplySuggestion[]; loading: boolean; issue: string | null };
  nativeKeys: ReadonlySet<string>;
  principal: ReactNode;
  onSend: (text: string, to: string | null, replyTo: string | null) => void;
  sending: boolean;
  sendError: string;
  answering: { asker: string; signalId: string | null } | null;
  onClearAnswer: () => void;
  composerFocus: number;
  highlight: number | null;
  onUseSuggestion: (text: string) => void;
}

export function composerMode(thread: ChatThread | undefined, nativeKeys: ReadonlySet<string>): "record" | "native" | "record+principal" | "none" {
  if (thread === undefined || thread.kind === "everyone") return nativeKeys.size > 0 ? "native" : "record+principal";
  if (thread.kind === "pair") return "none";
  return nativeKeys.has(thread.participants[0]) ? "native" : "record";
}

export function ChatColumn(props: ChatColumnProps) {
  const thread = props.threads.find((candidate) => candidate.key === props.selected) ?? props.threads[0];
  const mode = composerMode(thread, props.nativeKeys);
  const [draft, setDraft] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { if (props.composerFocus > 0) box.current?.focus(); }, [props.composerFocus]);
  useEffect(() => {
    if (props.highlight !== null) document.getElementById(`chat-msg-${props.highlight}`)?.scrollIntoView({ block: "center" });
  }, [props.highlight, props.selected]);

  const send = () => {
    const text = draft.trim();
    if (text === "" || props.sending) return;
    if (props.answering !== null) props.onSend(text, props.answering.asker, props.answering.signalId);
    else if (thread?.kind === "direct") props.onSend(text, thread.participants[0], null);
    else { const target = parseMention(text, props.bots); props.onSend(target.text, target.to, null); }
    setDraft("");
  };
  const suggestion = props.jev.suggestions[0];

  return (
    <aside className="chat-column" aria-label="Chat">
      {props.cards}
      <div className="chat-tabs" role="tablist" aria-label="Threads">
        {props.threads.map((candidate) => {
          const unread = props.unread[candidate.key] ?? 0;
          return (
            <button key={candidate.key} type="button" role="tab" aria-selected={candidate.key === thread?.key}
              aria-label={unread > 0 ? `${candidate.label}, ${unread} unread` : candidate.label} onClick={() => props.onSelect(candidate.key)}>
              {candidate.label}{unread > 0 && <span className="chat-unread" aria-hidden="true">{unread}</span>}
            </button>
          );
        })}
      </div>
      {thread?.kind === "pair" && <p className="chat-recorded-note">Recorded messages: what these agents recorded to each other through the Runtime, not their native chats.</p>}
      <ol className="chat-messages" role="tabpanel" aria-label={thread?.label ?? "Everyone"}>
        {thread?.key === EVERYONE && props.openingCount > 0 && <li className="chat-opening">Opening {props.openingCount} sealed records…</li>}
        {(thread?.messages ?? []).map((message) => {
          const name = props.names[message.sender] ?? (message.sender === "studio-operator" ? "You" : message.sender);
          return (
            <li key={message.id} id={`chat-msg-${message.sequence}`} className={`chat-message ${props.highlight === message.sequence ? "chat-message-highlight" : ""}`}
              style={{ "--bot-hue": String(hueOf(message.sender)) } as CSSProperties}>
              <span className="chat-avatar" aria-hidden="true">{name.slice(0, 1).toUpperCase()}</span>
              <div>
                <p className="chat-meta"><strong>{name}</strong>{message.to !== null && <> → {props.names[message.to] ?? (message.to === "studio-operator" ? "you" : message.to)}</>} · {ago(message.at)}</p>
                <p className="chat-text">{message.text}</p>
              </div>
            </li>
          );
        })}
      </ol>
      {(props.jev.loading || suggestion !== undefined) && mode !== "none" && (
        <section className="jev-card" aria-label="Jev suggests">
          {props.jev.loading ? <p role="status">Jev is preparing a suggestion…</p> : suggestion && <>
            <p>{suggestion.draft}</p>
            <p className="jev-reason">{suggestion.reason}</p>
            <button type="button" onClick={() => { setDraft(suggestion.draft); props.onUseSuggestion(suggestion.draft); box.current?.focus(); }}>Use</button>
          </>}
        </section>
      )}
      {(mode === "record" || mode === "record+principal") && (
        <div className="chat-composer">
          {props.answering !== null && (
            <p className="chat-answering">Answering {props.names[props.answering.asker] ?? props.answering.asker} <button type="button" onClick={props.onClearAnswer}>Clear</button></p>
          )}
          <label htmlFor="chat-message" className="sr-only">Message</label>
          <textarea id="chat-message" ref={box} value={draft} rows={3} maxLength={4000} onChange={(event) => setDraft(event.target.value)}
            placeholder={thread?.kind === "direct" ? `Message ${thread.label}` : "Message everyone, or @name one bot"} />
          <button type="button" disabled={props.sending || draft.trim() === ""} onClick={send}>Send</button>
          {props.sendError && <p role="alert">{props.sendError}</p>}
        </div>
      )}
      {(mode === "native" || mode === "record+principal") && props.principal}
    </aside>
  );
}
```

`maxLength={4000}` matches `MAX_MESSAGE_LENGTH` in `runtime/client.ts`; import and use the constant instead of the literal.

- [ ] **Step 4: Slim `main-chat.tsx`**

1. Remove `replySuggestions`, `replyLoading`, `replyIssue` from `MainChatProps` and the destructuring; delete `currentAdvice`, `suggestedDrafts` and the whole `<section className="main-chat-guidance" aria-label="JEV next step">…</section>` block; drop the now-unused `ReplySuggestions` import.
2. Add the four new props to the interface and destructuring (`recipientId = null, seed = null, refreshNonce: externalRefresh = 0, onRequestsChange`).
3. After the `selected` line add:

```ts
useEffect(() => {
  if (recipientId !== null && byChat.has(recipientId)) setSelectedId(recipientId);
}, [recipientId, byChat]);
useEffect(() => {
  if (seed !== null && seed.text.length <= messageLimit) setMessage((current) => current === "" ? seed.text : current);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the nonce is the trigger
}, [seed?.nonce]);
useEffect(() => { if (externalRefresh > 0) setRefreshNonce((value) => value + 1); }, [externalRefresh]);
useEffect(() => { onRequestsChange?.(rows); }, [rows, onRequestsChange]);
```

4. Change the "Next step" sentence's second branch to `"Next step: use a Jev suggestion or write your own instruction, then choose who receives it."` (keep `aria-label="Next step"`).
5. Replace the Request status `<section …>` with a slim status line that keeps the id, the live status text and the button name:

```tsx
<div id="main-chat-request-status" className="main-chat-status">
  <p role="status">{/* the existing six-way status expression, unchanged */}</p>
  {(activeRows.length > 0 || readFailed) && <button type="button" className="main-chat-refresh" onClick={() => setRefreshNonce((value) => value + 1)}>Refresh request status</button>}
</div>
```

Delete the `statusRequest` technical-details block and the `statusRequest` constant (the per-request ids now live in the question card's details and in "Request history"). Run `npx tsc -b` to catch any other now-unused local.

6. In `main-chat.test.tsx`: delete the tests that click "Use suggestion 1/2" or read "JEV · Suggested next step" (`grep -n "suggestion\|JEV" src/components/main-chat.test.tsx`); replace any `getByRole("region", { name: "Request status" })` with `document.getElementById("main-chat-request-status")!`; add:

```tsx
it("switches its recipient to the thread the Chat column selected and fills an empty draft from a seed", async () => {
  // reuse this file's existing two-persona fixture and client stub
  const { rerender } = render(<MainChat client={client} executionId="run" personas={PERSONAS} />);
  rerender(<MainChat client={client} executionId="run" personas={PERSONAS} recipientId={PERSONAS[1].chat.id} seed={{ text: "Rebase now", nonce: 1 }} />);
  expect(screen.getByLabelText("Main recipient")).toHaveValue(PERSONAS[1].chat.id);
  expect(screen.getByLabelText("Instruction")).toHaveValue("Rebase now");
});
```

Use the file's actual fixture names for `client` and `PERSONAS` (`grep -n "const .*personas\|function .*client" src/components/main-chat.test.tsx`).

7. In `App.tsx` at the `<MainChat …>` call, delete `replySuggestions={currentReplySuggestions} replyLoading={replyLoading} replyIssue={currentReplyIssue}`.

- [ ] **Step 5: Add styles** (substitute real tokens):

```css
/* Chat column (spec §4.4). */
.chat-column { display: flex; flex-direction: column; min-height: 0; overflow: hidden; border-right: 1px solid var(--line); }
.chat-tabs { display: flex; gap: 4px; overflow-x: auto; padding: 6px 8px; border-bottom: 1px solid var(--line); }
.chat-tabs [role="tab"] { white-space: nowrap; padding: 4px 10px; border-radius: 999px; border: 1px solid transparent; background: none; color: inherit; }
.chat-tabs [aria-selected="true"] { border-color: var(--line); font-weight: 600; }
.chat-unread { margin-left: 6px; padding: 0 6px; border-radius: 999px; background: var(--amber); color: #000; font-size: 11px; }
.chat-recorded-note { margin: 6px 10px; font-size: 12px; color: var(--muted); }
.chat-messages { flex: 1; list-style: none; margin: 0; padding: 8px 10px; overflow: auto; display: grid; align-content: start; gap: 10px; }
.chat-opening { font-size: 12px; color: var(--muted); }
.chat-message { display: grid; grid-template-columns: 28px 1fr; gap: 8px; }
.chat-message-highlight { outline: 2px solid var(--amber); border-radius: 8px; }
.chat-avatar { display: grid; place-items: center; width: 28px; height: 28px; border-radius: 50%; background: hsl(var(--bot-hue) 60% 45%); color: #fff; font-size: 12px; }
.chat-meta { margin: 0; font-size: 12px; color: var(--muted); }
.chat-text { margin: 2px 0 0; white-space: pre-wrap; }
.jev-card { margin: 6px 10px; padding: 8px 10px; border: 1px dashed var(--line); border-radius: 12px; font-size: 13px; }
.jev-card p { margin: 0 0 6px; }
.jev-reason { color: var(--muted); font-size: 12px; }
.chat-composer { display: grid; gap: 6px; padding: 8px 10px; border-top: 1px solid var(--line); }
.chat-answering { margin: 0; font-size: 12px; display: flex; gap: 8px; align-items: center; }
.main-chat-status { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
```

If `.main-chat-request-status` and `.main-chat-guidance` rules become unused, delete them (`styles.test.ts` only demands rules for rendered classes; it does not fail on orphan rules, but they are dead code).

- [ ] **Step 6: Run tests and typecheck**

Run: `npx vitest run src/components/chat-column.test.tsx src/components/main-chat.test.tsx src/styles.test.ts && npx tsc -b`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/components/chat-column.tsx src/components/chat-column.test.tsx src/components/main-chat.tsx src/components/main-chat.test.tsx src/App.tsx src/styles.css
git commit -m "feat(studio): chat column with threads, Jev card and native composer slot (#301)"
```

---

### Task 10: Trim the Board to the draft sheet and delete the Overview

**Files:**
- Modify: `apps/studio/src/components/board.tsx`
- Modify: `apps/studio/src/components/board.test.tsx`
- Modify: `apps/studio/src/graph/model.ts` (receive `isEntryNode`, `isFirstEntryNode`)
- Delete: `apps/studio/src/components/work-overview.tsx`, `apps/studio/src/components/work-overview.test.tsx`
- Modify: `apps/studio/src/components/deliveries.test.tsx:18` (fixture path string only — change to `"apps/studio/src/components/team-canvas.tsx"`)
- Modify: `apps/studio/src/styles.css`, `apps/studio/src/styles.test.ts`

**Interfaces:**
- Consumes: nothing new.
- Produces: `export function isEntryNode(model: GraphModel, nodeId: string): boolean` and `export function isFirstEntryNode(model: GraphModel, nodeId: string): boolean` in `graph/model.ts` (bodies moved verbatim from `work-overview.tsx:75-100`). `Board`'s props lose: `initialLayout`, `onCanvasChange`, `crew`, `recordedSessions`, `workMessages`, `agentWork`, `selectedAgent`, `onSelectAgent`, `talks`, `activity`, `latestEvent`, `subagents`, `runTeam`, `claudeTasks`, `agentReports`, `runStatus`, `attention`, `nextAction`, `replyGuidance`, `onNextAction`, `selectedTalk`, `onSelectTalk`, `projectName`, `projectPath`, `latestRecordedUpdate`, `ended`. Its callers after Task 11: only the draft branch of `App.tsx`.

- [ ] **Step 1: Move the entry-node helpers** — cut `isEntryNode` and `isFirstEntryNode` (with their doc comments) from `work-overview.tsx` into `graph/model.ts`; move their tests (`work-overview.test.tsx` lines ~505–524, the `describe` that calls `isFirstEntryNode`) into `graph/model.test.ts` with the import changed to `./model`.

Run: `npx vitest run src/graph/model.test.ts`
Expected: PASS.

- [ ] **Step 2: Remove the Overview toggle and the overview page from `board.tsx`**
  - Delete the `WorkOverview` import lines (42–43) and import `isEntryNode, isFirstEntryNode` from `../graph/model`.
  - Delete `const [organized, setOrganized] …` and its `useEffect`, the `.work-view-switch` group, the `{organized && <div className="work-overview-scroll">…</div>}` block, and the `hidden={organized}` attribute on `.free-canvas-content`. In the `focusGraphFile` effect delete `setOrganized(false);`.
  - Remove `.work-view-switch` from `CANVAS_CHROME`.

- [ ] **Step 3: Remove regions, ink, notes, cast and talk bubbles from `board.tsx`**
  - Delete the three `.canvas-region` blocks (People / Conversations / Work, ~lines 892–904).
  - Delete the ink tool palette (`Tool`, `TONES`, `tool`/`tone`/`drawing` state, the pen/note/hand buttons in `.tools`, `pathOf`, the `board.strokes` and `drawing` paths in `<svg className="sheet-ink">`, the `board.notes.map(...)` labels, `undo`, the stroke/note branches in `onSurfacePointerDown`). Keep the `<svg className="sheet-ink">` element with `defs` and `edgeGeometry` paths: it draws verified edges.
  - Delete the `.cast` agents block, the talk-bubble block, `huddles`, `talkPlaces`, `agentPlaces`, the `talk-tie` paths, `presenceOf`, `minutesSince`, and every prop listed in **Produces** above.
  - Keep: node cards, verified edges, the `sr-only` "Verified dependencies" list, run capsule, lint strip, navigator, framing, Organize / Undo layout, `<GraphFileRow>`, `canvas-hints` (drop the "drag · moves" hint only if dragging is removed — it is not; keep all four).
  - `BoardState` keeps `strokes` and `notes` fields (stored boards from older versions must still parse); the Board simply no longer renders or edits them.

- [ ] **Step 4: Update `board.test.tsx`** — delete the `describe("drawing on the sheet")`, `describe("notes")` blocks, the test `"offers graph verification as an accessible work-region action"`, and the test `"pans from a section title without native text selection or moving cards"`; remove props listed in **Produces** from every `render(<Board …/>)` call; in the `describe("organizing a saved canvas")` test keep the assertion that Undo restores positions and drop any assertion about strokes/notes being rendered.

- [ ] **Step 5: Delete the overview files and its CSS**

```bash
git rm src/components/work-overview.tsx src/components/work-overview.test.tsx
```

In `styles.css` delete every rule whose selector contains `.work-overview`, `.work-view-switch`, `.work-verification`, `.work-session`, `.canvas-region`, `.sheet-note`, `.stroke`, `.huddle`, `.talk-tie`, `.cast`, `.blob`, `.talk-bubble` (`grep -n "work-overview\|work-view-switch\|work-verification\|work-session\|canvas-region\|sheet-note\|\.stroke\|huddle\|talk-tie\|\.cast\b\|\.blob\|talk-bubble" src/styles.css`). Keep the `.scene .execution-actions` dock rules but rewrite the `.scene:has(.work-overview-scroll)` selectors (styles.css ~2592–2597 and 2746–2751) to `.scene` so the Run actions menu keeps its dropdown behaviour.

In `styles.test.ts` delete `describe("the overview scrolls out from under the dock")` (lines ~117–131) and the `"work-session-tasks"` entry in `NO_RULE_NEEDED`; also delete the `describe("the agent card contains its own text")` block if its selector (`.blob` / `.agent-name`) no longer exists (`grep -n "agent-name\|blob" src/components/*.tsx`).

- [ ] **Step 6: Run tests and typecheck**

Run: `npx vitest run src/components/board.test.tsx src/graph/model.test.ts src/styles.test.ts src/components/deliveries.test.tsx && npx tsc -b`
Expected: board/model/styles/deliveries PASS. `tsc -b` will still FAIL on `App.tsx` passing removed props to the run-view `<Board>`; that call is replaced in Task 11. To keep this task green on its own, in `App.tsx` delete only the removed props from both `<Board …>` calls in this step (the run-view `Board` stays rendered until Task 11).

- [ ] **Step 7: Commit**

```bash
git add -A src/components/board.tsx src/components/board.test.tsx src/graph/model.ts src/graph/model.test.ts src/components/deliveries.test.tsx src/styles.css src/styles.test.ts src/App.tsx
git commit -m "refactor(studio): drop Overview, ink, notes and canvas regions from the board (#301)"
```

---

### Task 11: App wiring — top bar, beacon, four columns, handover, removals

**Files:**
- Modify: `apps/studio/src/App.tsx` (derivations ~1784–1990; resume handler ~3015–3024; render ~2326–2850)
- Modify: `apps/studio/src/App.test.tsx`
- Modify: `apps/studio/src/styles.css`

**Interfaces:**
- Consumes: everything from Tasks 1–10: `teamModel`, `teamLinks`, `botKeyOf`, `needsYou`, `buildHandover`, `shouldShowHandover`, `readLastSeen`, `writeLastSeen`, `chatThreads`, `unreadCounts`, `namesOf`, `describeActivity`, `sealedNotesPending`, `EVERYONE`, `TeamCanvas`, `GraphFileRow`, `Beacon`, `QuestionCards`, `NEEDS_YOU_ID`, `HandoverCard`, `RightPanel`, `ChatColumn`, `MainChat` (new props).
- Produces: no exported symbol. Accessible structure other tasks/tests rely on: complementary `"Principal conversation"`, complementary `"Chat"`, region `"Canvas"` with tablist `"Canvas views"` (tabs `"Team (live)"`, `"Journey"`), region `"Team"`, complementary `"Run side panel"`, section `"Needs you"`, dialog `"While you were away"`, beacon button named by `beaconLabel`.

- [ ] **Step 1: Write the failing App tests** — in `App.test.tsx`, replace the first test of `describe("Studio organization and responsive navigation")` with the block below, and add a `describe("live team layout")` after it. Reuse the file's `stubClient`, `open`, and `STATUS`:

```tsx
it("opens the principal conversation inside the chat column beside the team canvas", async () => {
  render(<App createClient={() => stubClient() as unknown as RuntimeClient} modelContext={null} session={async () => ({ token: "local-token", project: "GraphHelm", projectPath: "fixtures/project" })} />);
  const team = await screen.findByRole("region", { name: "Team" });
  const principal = screen.getByRole("complementary", { name: "Principal conversation" });
  expect(principal).toBeVisible();
  expect(screen.getByRole("complementary", { name: "Chat" })).toContainElement(principal);
  expect(principal.compareDocumentPosition(team) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Free canvas" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Overview" })).toBeNull();
});
```

```tsx
describe("live team layout", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => localStorage.clear());

  it("lights the beacon for a blocked step and opens its card with the legal action", async () => {
    await open(stubClient());
    const beacon = await screen.findByRole("button", { name: "1 decision needs you" });
    await userEvent.click(beacon);
    const cards = screen.getByRole("region", { name: "Needs you" });
    expect(within(cards).getByText("implementation is blocked.")).toBeInTheDocument();
    expect(within(cards).getByRole("button", { name: /approve implementation|allow retry implementation/i })).toBeInTheDocument();
  });

  it("goes dark when the Runtime answers and nothing is open", async () => {
    const calm = { ...STATUS, attention: "can_sleep", attentionReasons: [], nodeStateCounts: { ready: 1 } };
    await open(stubClient({ getStatus: vi.fn(async () => calm) }));
    expect(await screen.findByRole("button", { name: "Nothing needs you" })).toHaveClass("beacon-dark");
  });

  it("shows the handover after a long gap and advances only on Got it", async () => {
    localStorage.setItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy", "1");
    const many = Array.from({ length: 24 }, (_, i) => ({ sequence: 20 + i, kind: "signal_recorded", payload: { kind: "operator_note", signalId: `s${i}` },
      occurredAt: new Date(Date.parse("2026-08-27T12:30:00Z") + i * 60_000).toISOString(), actorId: "kit-1", actorType: "agent",
      idempotencyKey: null, eventId: `e${20 + i}`, evidenceRefs: [] }));
    const client = stubClient({ getEvents: vi.fn(async () => ({ head: 43, events: [
      { sequence: 1, kind: "execution_form_declared", payload: { executionId: "demo-deploy", nodeIds: ["implementation"] }, occurredAt: "2026-08-27T12:00:00Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k0", eventId: "e1", evidenceRefs: [] },
      ...many,
    ] })), getStatus: vi.fn(async () => ({ ...STATUS, headSequence: 43 })) });
    await open(client);
    const card = await screen.findByRole("dialog", { name: "While you were away" });
    expect(card).toHaveTextContent("24 records");
    await userEvent.click(within(card).getByRole("button", { name: "Got it" }));
    expect(localStorage.getItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy")).toBe("43");
    expect(screen.queryByRole("dialog", { name: "While you were away" })).toBeNull();
  });

  it("does not show run A's handover after switching to run B", async () => {
    localStorage.setItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy", "1");
    // same 24-event client as above, extracted into a local helper `longGapClient()` in this describe
    await open(longGapClient());
    expect(await screen.findByRole("dialog", { name: "While you were away" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /demo-calm/ }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "While you were away" })).toBeNull());
  });

  it("resume without a graph file selects the Team tab and focuses the graph field by state", async () => {
    const paused = { ...STATUS, status: "paused", attentionReasons: [] };
    await open(stubClient({ getStatus: vi.fn(async () => paused) }));
    await userEvent.click(screen.getByRole("tab", { name: "Journey" }));
    await userEvent.click(screen.getByRole("button", { name: "resume" }));
    expect(screen.getByRole("tab", { name: "Team (live)" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByLabelText("Graph file path on the Runtime host")).toHaveFocus();
  });
});
```

Extract the 24-event client from the handover test into `function longGapClient()` inside the `describe` and use it in both handover tests. `open()` selects `demo-deploy` under project `dale-api-base` (see the existing "Selected run identity" test); if the project name differs in `open()`, use the name it shows.

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run src/App.test.tsx -t "principal conversation inside the chat column|live team layout"`
Expected: FAIL — no region "Team", no beacon.

- [ ] **Step 3: Add the derivations** — in `App.tsx`, after `durableProposals` is computed (~line 2005) add, with the imports at the top:

```ts
const NO_ALIASES: Record<string, string> = {};   // module scope, above the component: phase 2 fills it from actor_alias records
```

```ts
const [clock, setClock] = useState(() => Date.now());
useEffect(() => { const timer = window.setInterval(() => setClock(Date.now()), 30_000); return () => window.clearInterval(timer); }, []);
useEffect(() => { setClock(Date.now()); }, [eventList]);
const [nativeRequests, setNativeRequests] = useState<NativeChatRequest[] | null>(null);
useEffect(() => { setNativeRequests(null); }, [selected]);
const nodeNames = useMemo(() => Object.fromEntries(model.nodes.flatMap((node) => node.declaredName ? [[node.id, node.declaredName] as const] : [])), [model]);
const needs = useMemo(() => needsYou({ status, stale, events: eventList, envelopes, nativeRequests,
  pendingDraftIds: durableProposals.map((proposal) => proposal.draftId), nodeNames, operatorId: OPERATOR_ACTOR.id }),
  [status, stale, eventList, envelopes, nativeRequests, durableProposals, nodeNames]);
const waitingAskers = useMemo(() => new Set(needs.items.flatMap((item) => item.kind === "question" ? [item.asker] : [])), [needs]);
const team = useMemo(() => teamModel({ events: eventList, envelopes, personas, nativeLinks: nativePersonaLinks, aliases: NO_ALIASES, model,
  claudeTasks: claudeTaskRead?.executionId === selected ? claudeTaskRead : null, waitingAskers, now: clock }),
  [eventList, envelopes, personas, nativePersonaLinks, model, claudeTaskRead, selected, waitingAskers, clock]);
const links = useMemo(() => teamLinks(eventList, envelopes, team.bots, clock), [eventList, envelopes, team, clock]);
const botNames = useMemo(() => namesOf(team.bots), [team]);
const threads = useMemo(() => chatThreads(workMessages, team.bots, OPERATOR_ACTOR.id), [workMessages, team]);
const [thread, setThread] = useState(EVERYONE);
const [threadOpened, setThreadOpened] = useState<Record<string, number>>({});
useEffect(() => { setThread(EVERYONE); setThreadOpened({}); }, [selected]);
useEffect(() => {
  const newest = threads.find((candidate) => candidate.key === thread)?.messages.at(-1)?.sequence ?? 0;
  setThreadOpened((current) => current[thread] === newest ? current : { ...current, [thread]: newest });
}, [thread, threads]);
const unread = useMemo(() => unreadCounts(threads, threadOpened), [threads, threadOpened]);
const openingCount = useMemo(() => sealedNotesPending(eventList, envelopes), [eventList, envelopes]);
const activityLines = useMemo(() => recentActivity.map((item) => describeActivity(item, botNames, envelopes, OPERATOR_ACTOR.id)), [recentActivity, botNames, envelopes]);
const assignedNodeIds = useMemo(() => new Set(team.bots.flatMap((bot) => bot.tasks.flatMap((task) => task.nodeId === null ? [] : [task.nodeId]))), [team]);
const unassignedSteps = useMemo(() => model.nodes.filter((node) => !assignedNodeIds.has(node.id)), [model, assignedNodeIds]);
const nativeKeys = useMemo(() => new Set(Object.keys(nativePersonaLinks)), [nativePersonaLinks]);

const [canvasTab, setCanvasTab] = useState<"team" | "journey">("team");
const [graphFileOpen, setGraphFileOpen] = useState(false);
const [answering, setAnswering] = useState<{ asker: string; signalId: string | null } | null>(null);
const [composerFocus, setComposerFocus] = useState(0);
const [highlight, setHighlight] = useState<number | null>(null);
const [mainChatSeed, setMainChatSeed] = useState<{ text: string; nonce: number } | null>(null);
const [nativeRefresh, setNativeRefresh] = useState(0);
useEffect(() => { setAnswering(null); setHighlight(null); setMainChatSeed(null); setCanvasTab("team"); }, [selected]);

const projectKey = project ?? "this runtime";
const [seenSeq, setSeenSeq] = useState<number | null>(null);
useEffect(() => { setSeenSeq(selected === "" ? null : readLastSeen(projectKey, selected)); }, [projectKey, selected]);
const head = status?.headSequence ?? 0;
const handover = useMemo(() => status !== null && status.executionId === selected && shouldShowHandover(eventList, seenSeq, head)
  ? buildHandover({ events: eventList, bots: team.bots, model, claudeTasks: claudeTaskRead?.executionId === selected ? claudeTaskRead : null,
      openItems: needs.items, fromSeq: seenSeq!, toSeq: head })
  : null, [status, selected, eventList, seenSeq, head, team, model, claudeTaskRead, needs]);
const markSeen = useCallback(() => {
  if (selected === "" || head === 0) return;
  writeLastSeen(projectKey, selected, head);
  setSeenSeq(head);
}, [projectKey, selected, head]);
useEffect(() => {
  // Ten visible seconds of the live view count as seen; an open handover waits for Got it.
  if (handover !== null || selected === "" || head === 0) return;
  const timer = window.setTimeout(() => { if (document.visibilityState === "visible") markSeen(); }, 10_000);
  return () => window.clearTimeout(timer);
}, [handover, selected, head, markSeen]);

const openRecords = (sequences: number[]) => {
  const first = sequences[0];
  if (first === undefined) return;
  const owner = threads.find((candidate) => candidate.messages.some((message) => message.sequence === first));
  if (owner !== undefined) { setThread(owner.key); setHighlight(first); return; }
  setTalkOpen(true);
  setFocus({ kind: "run" });
};
const answerQuestion = (asker: string, signalId: string | null) => {
  const key = botKeyOf(team.bots, asker) ?? asker;
  setThread(`direct:${key}`);
  setAnswering({ asker, signalId });
  setComposerFocus((nonce) => nonce + 1);
};
```

`project`, `stale`, `claudeTaskRead`, `personas`, `nativePersonaLinks`, `envelopes`, `workMessages`, `recentActivity`, `model`, `setTalkOpen`, `setFocus`, `say`, `saying`, `sayError` are existing App state/derivations. If `answerQuestion` direct thread does not exist yet (no message on it), `ChatColumn` falls back to `threads[0]`; add the missing thread in place: `const threadsWithAnswer = answering !== null && !threads.some((t) => t.key === thread) ? [...threads, { key: thread, kind: "direct" as const, label: botNames[answering.asker] ?? answering.asker, participants: [botKeyOf(team.bots, answering.asker) ?? answering.asker], messages: [] }] : threads;` and pass `threadsWithAnswer` to `ChatColumn`.

- [ ] **Step 4: Replace the topstrip with the top bar** — in the `.stage` render, replace `<div className="topstrip">…</div>` with:

```tsx
<header className="topbar">
  <button type="button" className="ghost mobile-toggle projects-toggle" aria-expanded={projectsOpen} aria-controls="projects-rail" onClick={() => setProjectsOpen((open) => !open)} aria-label="Toggle projects"><Menu aria-hidden="true" /></button>
  <div className="topbar-mission">
    {selected ? (
      <button type="button" className="run-name" title={selected}
        onClick={() => { setSayTo(null); setSayAnswer(null); setTalkOpen(true); setFocus({ kind: "run" }); setSayFocusNonce((nonce) => nonce + 1); }}>
        {project ? `${project} · ` : ""}{runLabel(selected, briefing)}
      </button>
    ) : <span className="meta">pick a run, or start one</span>}
    {selected && <p className="run-selection-identity" aria-label="Selected run identity">Project: {project ?? "this runtime"} / Run: {selected}</p>}
  </div>
  {selected && status !== null && <Beacon state={needs.state} onOpen={() => { const target = document.getElementById(NEEDS_YOU_ID); target?.scrollIntoView({ block: "start" }); target?.focus(); }} />}
  <details className="topbar-menu">
    <summary>Run details</summary>
    {/* move the existing verdict `<span className={`tag …`}>…</span>` expression here unchanged */}
    {/* move the existing `webmcp === "available" ? <span className="toolchips">…</span> : null` block here unchanged */}
  </details>
  {/* keep the existing Refresh button and the two-step Disconnect button here unchanged */}
</header>
```

Delete the "Tidy the board" button (the Team canvas has Reset layout; the draft board has Organize). Delete the `conversation-toggle` button only if `conversationVisible` has no other opener — keep it otherwise (RunPanel still lives in `#conversation-panel`).

- [ ] **Step 5: Replace the run-view split** — replace the final `: (<div className="split"> … </div>)` branch (from `<aside className="main-chat-rail" …>` through the end of `.scene`) with:

```tsx
<div className="studio-columns">
  <ChatColumn
    threads={threadsWithAnswer} selected={thread} onSelect={(key) => { setThread(key); setAnswering(null); setHighlight(null); }} unread={unread}
    bots={team.bots} names={botNames} openingCount={openingCount}
    cards={<QuestionCards items={needs.items} names={botNames} busy={busy || saying === "chat"}
      onChoose={(item, choice) => void say(choice, item.asker, "chat", item.signalId)}
      onAnswer={(item) => answerQuestion(item.asker, item.signalId)}
      onCheck={() => setNativeRefresh((nonce) => nonce + 1)}
      stepActions={stepActions} />}
    jev={{ suggestions: currentReplySuggestions?.state === "ready" ? currentReplySuggestions.suggestions : [], loading: replyLoading, issue: currentReplyIssue }}
    nativeKeys={nativeKeys}
    principal={<aside className="main-chat-rail" aria-label="Principal conversation">
      <MainChat key={`main-${selected}`} client={clientRef.current} executionId={selected} personas={mainChatPersonas}
        refreshSequence={status.headSequence ?? 0} recipientId={thread.startsWith("direct:") ? thread.slice("direct:".length) : null}
        seed={mainChatSeed} refreshNonce={nativeRefresh} onRequestsChange={setNativeRequests}
        onConnect={() => { const first = model.nodes[0]; if (first) setFocus({ kind: "node", id: first.id }); }} />
    </aside>}
    onSend={(text, to, replyTo) => { void say(text, to, "chat", replyTo); setAnswering(null); }}
    sending={saying === "chat"} sendError={sayError?.via === "chat" ? sayError.text : ""}
    answering={answering} onClearAnswer={() => setAnswering(null)} composerFocus={composerFocus} highlight={highlight}
    onUseSuggestion={(text) => setMainChatSeed((current) => ({ text, nonce: (current?.nonce ?? 0) + 1 }))}
  />
  {talkOpen && (
    <aside id="conversation-panel" className="talk" hidden={!conversationVisible}>
      {/* the judge-route label and <RunPanel …/> exactly as today; the attention block above them is deleted */}
    </aside>
  )}
  {focus.kind === "agent" && (
    <aside className="talk chat-col">{/* the existing <AgentPanel …/> block, unchanged */}</aside>
  )}
  <section className="canvas-column" aria-label="Canvas">
    <div className="canvas-tabs" role="tablist" aria-label="Canvas views">
      <button type="button" role="tab" aria-selected={canvasTab === "team"} onClick={() => setCanvasTab("team")}>Team (live)</button>
      <button type="button" role="tab" aria-selected={canvasTab === "journey"} onClick={() => setCanvasTab("journey")}>Journey</button>
    </div>
    <div className="scene" style={dockReservePx === null ? undefined : ({ "--dock-reserve": `${dockReservePx}px` } as CSSProperties)}>
      {canvasTab === "team" ? (
        <TeamCanvas storageKey={`graphhelm.team-positions:${projectKey}:${selected}`} bots={team.bots} otherRecorders={team.otherRecorders}
          links={links} unassignedSteps={unassignedSteps} selectedBot={thread.startsWith("direct:") ? thread.slice(7) : null}
          onSelectBot={(key) => { setThread(`direct:${key}`); setAnswering(null); }}
          onOpenBotDetails={(key) => setFocus({ kind: "agent", id: key })}
          onOpenNode={(id) => { nodeFocusOrigin.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; setFocus({ kind: "node", id }); }}
          onOpenTask={(bot, task) => task.nodeId !== null ? setFocus({ kind: "node", id: task.nodeId }) : openRecords([task.sequence])}
          graphFileOpen={graphFileOpen} onGraphFileOpenChange={setGraphFileOpen}
          graphFileRow={<GraphFileRow graphFile={graphFile} onGraphFileChange={onGraphFileChange} onDrawConnections={() => void drawConnections()} busy={busy}
            demonstration={status.executor === "fixture"} fixtureFile={fixtureFile} onFixtureFileChange={setFixtureFile} inputRef={graphFileInput} />} />
      ) : (
        <section className="journey-empty" aria-label="Journey">
          <p>No journeys mapped yet. A journey appears here once its screens are captured.</p>
        </section>
      )}
      {handover !== null && <HandoverCard handover={handover} onOpen={openRecords} onDismiss={markSeen} />}
      {/* the existing <div className="dock" ref={dockRef}>…</div> block, unchanged except the resume handler below */}
    </div>
  </section>
  <RightPanel activity={activityLines} onOpenActivity={(sequence) => openRecords([sequence])} />
</div>
```

Supporting edits in the same step:
- `onGraphFileChange`: extract the existing inline `onGraphFileChange={(value) => { … }}` body from the old run-view `<Board>` into `const onGraphFileChange = (value: string) => { … };` above the return.
- `graphFileInput`: `const graphFileInput = useRef<HTMLInputElement>(null);` and `useEffect(() => { if (fileFocusNonce > 0) { setCanvasTab("team"); setGraphFileOpen(true); window.setTimeout(() => graphFileInput.current?.focus(), 0); } }, [fileFocusNonce]);`
- `stepActions`: define above the return. Move the per-reason button JSX from the deleted attention block (`if ((kind === "blocked_node" || kind === "untriaged_interruption") && node !== null) { … }` body: the approve / allow-retry `<button>` and its `retryFailures` sentence) into:

```tsx
const stepActions = (item: StepItem | DraftItem): ReactNode => {
  if (item.kind === "draft") return <button type="button" onClick={focusPendingProposal}>Review proposal</button>;
  if (item.kind === "waiting_step") return <button type="button" onClick={pendingQuestion !== null ? () => answerQuestion(pendingQuestion.asker, pendingQuestion.signalId) : focusRunReply}>Answer in chat</button>;
  const node = item.nodeId;
  const failure = item.reason === "blocked_node" ? retryFailures.get(node) : undefined;
  return <>
    {failure && <span className="question-failure">{`Failed after retries: ${readable(failure.reason)}. Check ${failure.executor === "model" ? "the model route" : "the failed step"} before allowing another attempt.`}</span>}
    {/* the existing approve/allow-retry <button …> for `node`, moved verbatim from the attention block */}
  </>;
};
```

- Delete: the `<div className="attention" aria-label="Why this run needs you">…</div>` block, the `TalkPanel` render and its import if unused, the run-view `<Board …/>`, the `focus.kind === "talk"` branch, and memo values that become unused (`talks`, `talkThread`, `roomEvents`, `pairTalks`, `crew`, `agentReports`, `latestEvent`, `latestRecordedUpdate`, `agentWork`, `hasRecordedAgentWork` — let `npx tsc -b` with `noUnusedLocals` list them; delete each one it names and nothing else).

- [ ] **Step 6: Replace the resume click hack** — in the resume `onClick` (~line 3015) replace:

```ts
const freeCanvas = [...document.querySelectorAll<HTMLButtonElement>("button")]
  .find((button) => button.textContent?.trim() === "Free canvas");
freeCanvas?.click();
setFileFocusNonce((nonce) => nonce + 1);
return;
```

with:

```ts
setCanvasTab("team");
setGraphFileOpen(true);
setFileFocusNonce((nonce) => nonce + 1);
return;
```

- [ ] **Step 7: Update the existing App tests** — mechanical, one command each, then fix what remains by reading the failure:
  - `sed -i 's/"Why this run needs you"/"Needs you"/g' src/App.test.tsx` — the cards section replaces the attention block; any test that then fails because it expected the old sentence (`"implementation is blocked"` without the period, or "awaits triage - approving it is the triage") updates its text to the card sentence from Task 7 (`"implementation is blocked."`, `"… was interrupted and awaits triage."`).
  - Delete every `await userEvent.click(screen.getByRole("button", { name: "Overview" }))` / `"Free canvas"` line (lines listed by `grep -n '"Overview"\|Free canvas' src/App.test.tsx`); delete the test `"opens the Free canvas and focuses the graph field from the default overview"` (replaced by the resume test in Step 1).
  - Tests that read `findByLabelText("Agents in this room")` move to the Team canvas: replace with `findByRole("region", { name: "Team" })` and query bots by `getByRole("button", { name: /<actor>, / })`.
  - Tests that read `findByRole("main", { name: "Work overview" })` replace it with `findByRole("region", { name: "Team" })`.
  - Tests that clicked a talk bubble now click a thread tab: `screen.getByRole("tab", { name: /<label>/ })`.

Run after each bullet: `npx vitest run src/App.test.tsx`.

- [ ] **Step 8: Layout styles** — append to `styles.css` (substitute real tokens; reduced motion and breakpoints included here):

```css
/* Studio layout (spec §3): Projects · Chat · Canvas · Right panel. */
.topbar { display: flex; align-items: center; gap: 12px; padding: 8px 16px; border-bottom: 1px solid var(--line); }
.topbar-mission { flex: 1; min-width: 0; }
.topbar-mission .run-name { font-size: 15px; font-weight: 600; background: none; border: 0; color: inherit; padding: 0; text-align: left; }
.topbar-menu { position: relative; font-size: 12px; }
.topbar-menu[open] > :not(summary) { position: absolute; right: 0; top: 28px; z-index: 30; display: flex; flex-direction: column; gap: 6px; padding: 8px; background: var(--surface); border: 1px solid var(--line); border-radius: 10px; }
.studio-columns { display: grid; grid-template-columns: minmax(300px, 360px) minmax(0, 1fr) 280px; height: 100%; min-height: 0; }
.canvas-column { position: relative; display: flex; flex-direction: column; min-width: 0; min-height: 0; }
.canvas-tabs { display: flex; gap: 4px; padding: 6px 12px; border-bottom: 1px solid var(--line); }
.canvas-tabs [role="tab"] { padding: 4px 12px; border-radius: 999px; border: 1px solid transparent; background: none; color: inherit; }
.canvas-tabs [aria-selected="true"] { border-color: var(--line); font-weight: 600; }
.journey-empty { display: grid; place-items: center; height: 100%; color: var(--muted); }
.question-failure { font-size: 12px; color: var(--muted); }
@media (max-width: 1100px) {
  .studio-columns { grid-template-columns: minmax(280px, 340px) minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) auto; }
  .right-panel { grid-column: 2; border-left: 0; border-top: 1px solid var(--line); max-height: 40vh; }
}
@media (max-width: 768px) {
  .studio-columns { display: flex; flex-direction: column; overflow: auto; }
  .chat-column, .canvas-column, .right-panel { min-height: 60vh; border: 0; }
}
@media (prefers-reduced-motion: reduce) {
  .team-link-live, .team-bot-working .team-bot-avatar, .beacon-lit .beacon-dot { animation: none; }
}
```

Delete rules for `.topstrip`, `.strip-card` and `.split` if no component renders them anymore (`grep -n "topstrip\|strip-card\|\"split\"" src/*.tsx src/components/*.tsx`); `.split` is still used by the draft branch — keep it if so.

The below-768 px spec asks for one column with tabs Chat / Team / Journeys. Phase 1 stacks the three columns vertically in that order instead of adding a third tab set; record this in the PR body as a known gap for phase 5 (which adds Journeys data).

- [ ] **Step 9: Run the full Studio suite and typecheck**

Run: `npx vitest run && npx tsc -b`
Expected: all files PASS, no type errors. `src/components/main-chat.test.tsx` passing is the main-chat acceptance check for "Principal conversation", "Next step", "Refresh request status".

- [ ] **Step 10: Commit**

```bash
git add -A src/App.tsx src/App.test.tsx src/styles.css
git commit -m "feat(studio): live team layout with beacon, question cards and handover (#301)"
```

---

### Task 12: Docs and the live observer

**Files:**
- Modify: `docs/ux/STUDIO_SPEC.md` (§2 Main structure, §3 Top bar, §5 Graph canvas, §6 Chat and Command Composer, §7 Running agents panel)
- Modify: `docs/ux/STUDIO_MVP.md` (§6 line 137, §7 lines 141–143)
- Modify: `docs/studio/NATIVE_CHAT_BRIDGE.md` (§ Principal conversation; Separate facts)

**Interfaces:**
- Consumes: the running Studio from Tasks 1–11.
- Produces: no code. PR-body evidence: four screenshots (Team tab, beacon lit, beacon dark, handover) or `OBSERVER_MISSING` with the reason.

- [ ] **Step 1: `STUDIO_SPEC.md`** — at the top of each of §2, §3, §5, §6 and §7 insert:

```markdown
> **Superseded (2026-10-05).** The layout in this section is replaced by
> `docs/specs/2026-10-05-studio-live-team-and-proven-journeys-design.md` §3–§4 (Projects · Chat ·
> Team canvas · Right panel, needs-you beacon, question cards, handover). The text below is kept
> for history; where the two disagree, the 2026-10-05 spec wins.
```

- [ ] **Step 2: `STUDIO_MVP.md`** — delete the line `- No mobile layout. The surface targets a desktop control room.` and replace it with `- Below 768 px the Studio stacks Chat, Team and the side panel in one column (phase 1 of the 2026-10-05 redesign); a tabbed phone layout arrives with the Journey tab.` In §7 replace the bullet that starts `**The Studio's own test suite is not part of \`ci/gate.ps1\`.**` with: `- **The Studio's tests run in the local CI script.** Gates are advisory since 2026-09-24; run \`npx vitest run\` and \`npx tsc -b\` from \`apps/studio\` and list the results in the PR body.` Before saving, check the second claim: `grep -n "studio" ci/local-ci.sh ci/gate.ps1 2>/dev/null`. If neither runs the Studio suite, write instead: `- **The Studio's tests are run by hand.** Neither \`ci/local-ci.sh\` nor \`ci/gate.ps1\` runs them; run \`npx vitest run\` and \`npx tsc -b\` from \`apps/studio\` and list the results in the PR body.`

- [ ] **Step 3: `NATIVE_CHAT_BRIDGE.md`** — under "Principal conversation" append:

```markdown
Since the 2026-10-05 redesign the principal conversation lives in the Chat column. Its gating is
unchanged: a pending request blocks team sends and the composer stays locked until the request
ledger is read. The "Request status" box is gone: a request the ledger cannot confirm becomes a
question card ("Your message to <chat> was not confirmed. It may not have arrived.") with
**Check it**, which re-reads the ledger; the request, thread and node ids move to the card's
details. The accessible names "Principal conversation", "Next step" and "Refresh request status"
are kept for `docs/acceptance/studio-main-chat-journey-2026-10-03.json`.
```

and under "Separate facts" append:

```markdown
- Bot-to-bot tabs in the Chat column are **recorded messages**: what two agents recorded to each
  other through the Runtime, not native chats talking directly. The shared `codex` actor appears
  as "Codex (shared)" and is never mapped to a thread.
```

- [ ] **Step 4: Run the live observer** — start the Studio against the ml-saas project's Runtime (`/graphhelm-start` in the ml-saas folder, or `npm run dev` in `apps/studio` with `GRAPHHELM_EVENTS` pointing at that project's `.graphhelm/events`), then open the printed `Studio auto-connect` URL in the Browser pane (`mcp__Claude_Browser__preview_start` with `url`). Select the ml-saas run and take screenshots of:
  1. the Team tab with its bots, states and lines (S1, S2);
  2. the beacon lit, with the Needs you cards opened by clicking it (S3, S4);
  3. the beacon dark on a run with nothing open (pick any calm run in the rail) (S3);
  4. the handover: in the browser console run `localStorage.setItem("graphhelm.handover.last-seen:<project>:<run>", "<a sequence ≥ 20 events and ≥ 15 min back>")`, reload, screenshot the "While you were away" card, click a line and confirm the Chat column scrolls to it (S5).

Also open the page at 1000 px and 390 px wide (`mcp__Claude_Browser__resize_window`) and confirm no horizontal page scroll; reset with preset `desktop`.

Expected: all four screens render; no console errors (`mcp__Claude_Browser__read_console_messages` with `onlyErrors: true`). If the ml-saas Runtime cannot be reached, record `OBSERVER_MISSING: <reason>` in the PR body instead of claiming S1–S5.

- [ ] **Step 5: Final verification**

Run: `npx vitest run && npx tsc -b`
Expected: PASS. Paste both command lines and their summary lines into the PR body under "Tests run".

- [ ] **Step 6: Commit**

```bash
git add docs/ux/STUDIO_SPEC.md docs/ux/STUDIO_MVP.md docs/studio/NATIVE_CHAT_BRIDGE.md
git commit -m "docs(studio): point the old layout at the live-team spec (#301)"
```

---

## Self-Review

**Spec coverage (phase 1):**
- §4.1 team model → Task 2 (bots, other recorders, codex shared, doingNow, state, tasks, colour, role). Aliases: Task 2 accepts them; Task 11 passes the empty `NO_ALIASES` (phase 2).
- §4.2 team canvas → Task 6 (row layout, coordinator first, drag + persisted positions, lines live 60 s, pulse / amber / dim, reduced motion in Task 11 CSS, click bot → thread, click task → node or records). Pan/zoom is a small local implementation rather than a shared extraction from `board.tsx` (see ambiguity 3).
- §4.3 beacon + cards → Tasks 3, 7, 11 (lit/dark/unknown, scroll to first card, recommendations → buttons, Answer, native card wording + Check it, transport ids in details; Refuse omitted). Gating and accessible names kept → Task 9, verified in Task 11 Step 9.
- §4.4 chat column → Tasks 5, 9 (Everyone/pair/direct tabs, unread, avatars/time, "Opening…", recorded-messages label, composer audience, @name, Jev card with Use).
- §4.5 handover → Tasks 4, 8, 11 (storage per project+run, Got it / 10 s visible, missing storage → no card, ≥15 min and ≥20 events, four groups citing sequences, clicking opens records). `jpd.screen_captured` shipped lines wait for phase 3–4 data.
- §4.6 right panel → Task 8 (Journeys and Before/after empty states; What just happened rewritten via `describeActivity`).
- §9 removals → Tasks 9 (JEV box, Request status box), 10 (Overview, ink, notes, regions), 11 (topstrip → top bar + menu, attention block → cards, resume by state, raw ids → card details).
- §8 phase-1 docs → Task 12. §10 observer → Task 12 Step 4.

**Placeholder scan:** the only deferred-content markers are JSX comments in Task 11 that say "move the existing … block here unchanged" for code the executor must cut-and-paste from the same file (verdict tag, toolchips, Refresh/Disconnect, RunPanel, AgentPanel, dock, approve button). Each names its exact source location; no new logic is left unwritten.

**Type consistency:** `EnvelopeRecord` (Task 1) is used by Tasks 2–5; `Bot`/`BotTask`/`TeamLink`/`OtherRecorder` (Task 2) by Tasks 4–6, 9, 11; `NeedsYouItem` family and `BeaconState` (Task 3) by Tasks 4, 7, 11; `Handover`/`HandoverLine` (Task 4) by Task 8; `ChatThread`/`ActivityLine`/`EVERYONE` (Task 5) by Tasks 8, 9, 11; `MainChatProps` new fields (Task 9) by Task 11.

**Review Focus:** each of the five lines has its test in the owning task (Tasks 3+7, 2+9, 3, 4+6, 11).
