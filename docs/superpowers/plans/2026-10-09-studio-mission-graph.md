# Studio Mission Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the owner one Studio view that answers what is proven, what is being done and by which agent, what is next, and lets them validate a step by stepping through its test inside an emulated browser.

**Architecture:** Pure read-model functions in `apps/studio/src/runtime/` fold data the Studio already has: `TaskState[]` from `foldTaskEvents`, `JourneysView` from `/v1/journeys`, and `JourneyRunView` from a flow's preview/replay. They produce a `Mission` (journey steps, linked tasks, trust level, summary counts) and `LaneBar[]` (per-agent time spans). Four presentational React components render those models: `MissionGraph`, `ProofTable`, `TestCanvas` and `LanesTimeline`. `App.tsx` adds a "Graph" tab that hosts them. No Runtime or Rust changes are needed; everything comes from public API contracts (AGENTS.md line 39).

**Tech Stack:** React 19, TypeScript, Vite, Vitest 4 + jsdom + @testing-library/react + user-event.

**Spec:** Design canvas https://claude.ai/artifact/UWWvT9WpkyK5yWBDu4TmsE. The artboards are Main (graph), Proof, TestRun (test canvas), Lanes and Glance (phone). Executors open it before each UI task.

## Global Constraints

- Studio code may use only public Runtime API/CLI contracts. Never import Runtime internals (AGENTS.md:39).
- Tests need no internet, Docker, credentials or browser (AGENTS.md:95). Use jsdom only.
- Tests sit next to the source as `<name>.test.ts(x)`.
- Trust ladder labels, in order and verbatim: `Written`, `Reviewed`, `Merged`, `Proven`, `Seen by you`.
- **Merged is not done.** A step counts as done only when its run result is `pass`.
- The summary strip is one quiet line (owner feedback: "too much emphasis"). No large cards.
- State colours: proven `#4ADE9B`, merged-not-proven `#8FB3D9`, in work `#F5A524`, stalled/blocked `#FF6B5E`, ready `#9AA7FF` (dashed). Every state also carries a text label, so colour is never the only signal.
- The silence threshold for "stalled" is a named constant, `STALL_MS = 2 * 60 * 60 * 1000`.
- Each PR body carries the Keel three-line card: paths in scope, the promise, and the proving command (AGENTS.md:145).
- Proving command for every task: `cd apps/studio && npx vitest run <test file>`. Before the PR: `npm run typecheck && npm test`.

## Known data gap (decided, not open)

`TaskState.journeys` links a task to a journey (`contractId`), but **not to a step**. This version therefore:
- draws step columns from `JourneyView.steps`, coloured by `JourneyRunView.screens[stepId].result`;
- draws tasks as one row of nodes under the steps, ordered `implement → review → merge → merged`;
- makes a click on a step select that column and show its run evidence;
- makes a click on a task node select that task.

Linking a task to a step needs a Runtime field. That is a follow-up issue, not this plan.

## Review Focus

1. **A journey with zero tasks, or a task whose journey is not in `JourneysView`.** Expect: the graph still renders the steps. Orphan tasks go to an "Unlinked work" group and are never dropped. Test: Task 1, `orphan tasks`.
2. **No run yet (`JourneyRunView.state === "none"` or null).** Expect: every step reads `not run`, never `Proven`. Test: Task 1, `no run`.
3. **A `preview` run (draft).** Expect: never counts as proof; steps read `preview only`. Test: Task 1, `preview is not proof`.
4. **An edge result `skipped`** (the destructive guard stopped it). Expect: that step reads `needs you`, and the summary `needYou` count includes it. Test: Task 1, `skipped edge needs you`.
5. **A lane with an open review and no event for longer than `STALL_MS`.** Expect: it is flagged `silent`. A lane at exactly `STALL_MS - 1` is not flagged. Test: Task 2, `silence threshold`.

---

### Task 1: Mission read model

**Files:**
- Create: `apps/studio/src/runtime/mission.ts`
- Test: `apps/studio/src/runtime/mission.test.ts`

**Interfaces:**
- Consumes: `JourneyView`, `JourneyRunView` (`runtime/types.ts`); `TaskState` (`runtime/team-tasks.ts`).
- Produces:
  - `type StepStatus = "proven" | "failed" | "needs_you" | "preview_only" | "not_run"`
  - `type TrustLevel = 0 | 1 | 2 | 3 | 4 | 5`
  - `interface MissionStep { stepId: string; index: number; title: string; status: StepStatus; reason: string | null }`
  - `interface MissionTask { key: string; pr: number | null; issue: number | null; title: string; lane: string | null; reviewers: string[]; step: TaskState["step"]; blocked: boolean; trust: TrustLevel }`
  - `interface MissionSummary { proven: number; total: number; inFlight: number; needYou: number }`
  - `interface Mission { contractId: string; title: string; steps: MissionStep[]; tasks: MissionTask[]; summary: MissionSummary }`
  - `buildMission(journey: JourneyView, run: JourneyRunView | null, tasks: TaskState[]): Mission`
  - `unlinkedTasks(journeys: JourneyView[], tasks: TaskState[]): MissionTask[]`
  - `TRUST_LABELS: readonly ["Written","Reviewed","Merged","Proven","Seen by you"]`

- [ ] **Step 1: Write the failing tests**

```ts
// apps/studio/src/runtime/mission.test.ts
import { describe, expect, it } from "vitest";
import { buildMission, unlinkedTasks, TRUST_LABELS } from "./mission";
import type { JourneyView, JourneyRunView } from "./types";
import type { TaskState } from "./team-tasks";

const journey: JourneyView = {
  contractId: "watch",
  title: "Watch plays inside the Studio",
  arrows: [],
  steps: [
    { stepId: "open", screen: { screenId: "open", title: "Open a journey", scopePaths: [] }, promises: [] },
    { stepId: "watch", screen: { screenId: "watch", title: "Watch the page live", scopePaths: [] }, promises: [] },
    { stepId: "mark", screen: { screenId: "mark", title: "Mark a skipped step safe", scopePaths: [] }, promises: [] },
  ],
};

function task(over: Partial<TaskState>): TaskState {
  return {
    key: "k", taskId: "t", branch: null, issue: 519, pr: 1, lane: "gh-claude-1", headSha: "a",
    journeys: ["watch"], step: "implement", blockedBy: null, reviewers: [], mergeSha: null, repoUrl: null,
    strayVerdicts: [], title: "t", summary: null, prTitle: "PR title", prSummary: null, critic: null,
    ...over,
  } as TaskState;
}

describe("buildMission", () => {
  it("no run: every step reads not_run, nothing proven", () => {
    const m = buildMission(journey, null, []);
    expect(m.steps.map((s) => s.status)).toEqual(["not_run", "not_run", "not_run"]);
    expect(m.summary).toEqual({ proven: 0, total: 3, inFlight: 0, needYou: 0 });
  });

  it("replay pass proves a step; fail does not", () => {
    const run: JourneyRunView = { state: "ready", kind: "replay", screens: { open: { frame: true, result: "fail", reason: "x" }, watch: { frame: true, result: "pass" } } };
    const m = buildMission(journey, run, []);
    expect(m.steps.map((s) => s.status)).toEqual(["failed", "proven", "not_run"]);
    expect(m.steps[0].reason).toBe("x");
    expect(m.summary.proven).toBe(1);
  });

  it("preview is not proof", () => {
    const run: JourneyRunView = { state: "ready", kind: "preview", screens: { watch: { frame: true, result: "pass" } } };
    expect(buildMission(journey, run, []).steps[1].status).toBe("preview_only");
  });

  it("skipped edge needs you", () => {
    const run: JourneyRunView = { state: "ready", kind: "replay", screens: {}, edges: { "watch->mark": { result: "skipped", reason: "data-changing" } } };
    const m = buildMission(journey, run, []);
    expect(m.steps[2].status).toBe("needs_you");
    expect(m.summary.needYou).toBe(1);
  });

  it("links tasks by journey and sets trust", () => {
    const m = buildMission(journey, null, [
      task({ key: "a", step: "implement" }),
      task({ key: "b", step: "review", reviewers: ["r"] }),
      task({ key: "c", step: "merged", mergeSha: "m" }),
      task({ key: "d", journeys: ["other"] }),
    ]);
    expect(m.tasks.map((t) => [t.key, t.trust])).toEqual([["a", 1], ["b", 1], ["c", 3]]);
    expect(m.summary.inFlight).toBe(2);
  });

  it("blocked task keeps its flag", () => {
    const m = buildMission(journey, null, [task({ blockedBy: { reviewer: "r", headSha: "h", commentUrl: "u" } })]);
    expect(m.tasks[0].blocked).toBe(true);
  });

  it("orphan tasks", () => {
    const orphans = unlinkedTasks([journey], [task({ key: "x", journeys: [] }), task({ key: "y", journeys: ["gone"] }), task({ key: "z" })]);
    expect(orphans.map((t) => t.key)).toEqual(["x", "y"]);
  });

  it("ladder labels are verbatim", () => {
    expect(TRUST_LABELS).toEqual(["Written", "Reviewed", "Merged", "Proven", "Seen by you"]);
  });
});
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cd apps/studio && npx vitest run src/runtime/mission.test.ts`
Expected: FAIL with `Failed to resolve import "./mission"`.

- [ ] **Step 3: Implement**

```ts
// apps/studio/src/runtime/mission.ts
import type { JourneyRunView, JourneyView } from "./types";
import type { TaskState } from "./team-tasks";

export type StepStatus = "proven" | "failed" | "needs_you" | "preview_only" | "not_run";
export type TrustLevel = 0 | 1 | 2 | 3 | 4 | 5;
export const TRUST_LABELS = ["Written", "Reviewed", "Merged", "Proven", "Seen by you"] as const;

export interface MissionStep { stepId: string; index: number; title: string; status: StepStatus; reason: string | null }
export interface MissionTask { key: string; pr: number | null; issue: number | null; title: string; lane: string | null; reviewers: string[]; step: TaskState["step"]; blocked: boolean; trust: TrustLevel }
export interface MissionSummary { proven: number; total: number; inFlight: number; needYou: number }
export interface Mission { contractId: string; title: string; steps: MissionStep[]; tasks: MissionTask[]; summary: MissionSummary }

const STEP_ORDER: Record<TaskState["step"], number> = { implement: 0, review: 1, merge: 2, merged: 3 };

function stepStatus(stepId: string, run: JourneyRunView | null): { status: StepStatus; reason: string | null } {
  if (!run || run.state === "none") return { status: "not_run", reason: null };
  const skipped = Object.entries(run.edges ?? {}).find(([id, e]) => e.result === "skipped" && id.split("->")[1] === stepId);
  if (skipped) return { status: "needs_you", reason: skipped[1].reason ?? null };
  const screen = run.screens?.[stepId];
  if (!screen?.result) return { status: "not_run", reason: null };
  if (run.kind !== "replay") return { status: "preview_only", reason: screen.reason ?? null };
  return screen.result === "pass" ? { status: "proven", reason: null } : { status: "failed", reason: screen.reason ?? null };
}

export function toMissionTask(t: TaskState): MissionTask {
  const trust: TrustLevel = t.step === "merged" ? 3 : t.step === "merge" ? 2 : 1;
  return {
    key: t.key, pr: t.pr, issue: t.issue, title: t.prTitle ?? t.title ?? t.taskId, lane: t.lane,
    reviewers: t.reviewers, step: t.step, blocked: t.blockedBy !== null, trust,
  };
}

export function buildMission(journey: JourneyView, run: JourneyRunView | null, tasks: TaskState[]): Mission {
  const steps = journey.steps.map((s, index) => ({
    stepId: s.stepId, index, title: s.screen?.title ?? s.stepId, ...stepStatus(s.stepId, run),
  }));
  const linked = tasks
    .filter((t) => t.journeys.includes(journey.contractId))
    .sort((a, b) => STEP_ORDER[a.step] - STEP_ORDER[b.step])
    .map(toMissionTask);
  return {
    contractId: journey.contractId,
    title: journey.title,
    steps,
    tasks: linked,
    summary: {
      proven: steps.filter((s) => s.status === "proven").length,
      total: steps.length,
      inFlight: linked.filter((t) => t.step !== "merged").length,
      needYou: steps.filter((s) => s.status === "needs_you").length,
    },
  };
}

export function unlinkedTasks(journeys: JourneyView[], tasks: TaskState[]): MissionTask[] {
  const known = new Set(journeys.map((j) => j.contractId));
  return tasks.filter((t) => !t.journeys.some((id) => known.has(id))).map(toMissionTask);
}
```

The trust levels mean: `1` Written, `2` Reviewed (merge stage is reached only after an APPROVE), `3` Merged. `4` Proven and `5` Seen by you are step-level: the component reads them from `MissionStep.status` and from the "seen" state in Task 5.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cd apps/studio && npx vitest run src/runtime/mission.test.ts`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add apps/studio/src/runtime/mission.ts apps/studio/src/runtime/mission.test.ts
git commit -m "feat(studio): mission read model ties tasks to journey proof"
```

---

### Task 2: Lane timeline read model

**Files:**
- Create: `apps/studio/src/runtime/lane-bars.ts`
- Test: `apps/studio/src/runtime/lane-bars.test.ts`

**Interfaces:**
- Consumes: `TaskEventRecord` (`runtime/team-tasks.ts`), plus the Runtime event timestamp. The caller passes it as `at` (ISO string).
- Produces:
  - `type TimedTaskEvent = TaskEventRecord & { at: string }`
  - `type BarKind = "implement" | "review" | "merge"`
  - `interface LaneBar { kind: BarKind; label: string; start: number; end: number; open: boolean }` (ms since epoch)
  - `interface Lane { lane: string; bars: LaneBar[]; silent: boolean; lastEventAt: number }`
  - `STALL_MS: number`
  - `laneBars(events: TimedTaskEvent[], now: number, windowMs: number): Lane[]`

Rules:
- `task.claimed` opens an `implement` bar on `lane`. The next `task.pr_opened` on the same task leaves it open; `task.review_assigned` closes it.
- `task.review_assigned` opens a `review` bar on `reviewer`. That reviewer's `task.review_verdict` closes it.
- A verdict of `APPROVE` opens a `merge` bar on the reviewer. `task.merged` closes it.
- Still-open bars end at `now` with `open: true`.
- Bars ending before `now - windowMs` are dropped. Starts are clamped to the window.
- `silent` = the lane has an open `review` or `merge` bar and `now - lastEventAt >= STALL_MS`.
- Lanes are sorted by name.

- [ ] **Step 1: Write the failing tests**

```ts
// apps/studio/src/runtime/lane-bars.test.ts
import { describe, expect, it } from "vitest";
import { laneBars, STALL_MS, type TimedTaskEvent } from "./lane-bars";

const T0 = Date.parse("2026-10-09T00:00:00Z");
const at = (ms: number) => new Date(T0 + ms).toISOString();
let seq = 0;
const ev = (e: Partial<TimedTaskEvent>): TimedTaskEvent => ({ actorId: "a", sequence: seq++, taskId: "t1", ...e }) as TimedTaskEvent;

describe("laneBars", () => {
  it("implement then review then merge", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", pr: 9, at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "rev", pr: 9, at: at(100) }),
      ev({ kind: "task.review_verdict", reviewer: "rev", verdict: "APPROVE" as never, pr: 9, at: at(200) }),
      ev({ kind: "task.merged", pr: 9, at: at(300) }),
    ], T0 + 400, 1000);
    const dev = lanes.find((l) => l.lane === "dev")!;
    const rev = lanes.find((l) => l.lane === "rev")!;
    expect(dev.bars).toEqual([{ kind: "implement", label: "#9", start: T0, end: T0 + 100, open: false }]);
    expect(rev.bars.map((b) => [b.kind, b.start - T0, b.end - T0, b.open])).toEqual([["review", 100, 200, false], ["merge", 200, 300, false]]);
  });

  it("open bar runs to now", () => {
    const [lane] = laneBars([ev({ kind: "task.claimed", lane: "dev", at: at(0) })], T0 + 50, 1000);
    expect(lane.bars[0]).toMatchObject({ end: T0 + 50, open: true });
  });

  it("silence threshold", () => {
    const events = [ev({ kind: "task.review_assigned", reviewer: "rev", at: at(0) })];
    expect(laneBars(events, T0 + STALL_MS - 1, STALL_MS * 2)[0].silent).toBe(false);
    expect(laneBars(events, T0 + STALL_MS, STALL_MS * 2)[0].silent).toBe(true);
  });

  it("an open implement bar is never silent", () => {
    const [lane] = laneBars([ev({ kind: "task.claimed", lane: "dev", at: at(0) })], T0 + STALL_MS * 3, STALL_MS * 4);
    expect(lane.silent).toBe(false);
  });

  it("window clamps and drops", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "old", taskId: "o", at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "x", taskId: "o", at: at(10) }),
      ev({ kind: "task.claimed", lane: "dev", taskId: "n", at: at(500) }),
    ], T0 + 1000, 600);
    expect(lanes.find((l) => l.lane === "old")!.bars).toEqual([]);
    expect(lanes.find((l) => l.lane === "x")!.bars[0].start).toBe(T0 + 400);
  });
});
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cd apps/studio && npx vitest run src/runtime/lane-bars.test.ts`
Expected: FAIL with `Failed to resolve import "./lane-bars"`.

- [ ] **Step 3: Implement**

```ts
// apps/studio/src/runtime/lane-bars.ts
import type { TaskEventRecord } from "./team-tasks";

export type TimedTaskEvent = TaskEventRecord & { at: string };
export type BarKind = "implement" | "review" | "merge";
export interface LaneBar { kind: BarKind; label: string; start: number; end: number; open: boolean }
export interface Lane { lane: string; bars: LaneBar[]; silent: boolean; lastEventAt: number }

export const STALL_MS = 2 * 60 * 60 * 1000;

export function laneBars(events: TimedTaskEvent[], now: number, windowMs: number): Lane[] {
  const lanes = new Map<string, { bars: LaneBar[]; last: number }>();
  const open = new Map<string, { lane: string; bar: LaneBar }>();
  const prOf = new Map<string, number>();
  const laneOf = (name: string) => {
    let l = lanes.get(name);
    if (!l) { l = { bars: [], last: 0 }; lanes.set(name, l); }
    return l;
  };
  const label = (taskId: string) => (prOf.has(taskId) ? `#${prOf.get(taskId)}` : taskId);
  const start = (lane: string, kind: BarKind, taskId: string, t: number) => {
    const bar: LaneBar = { kind, label: label(taskId), start: t, end: now, open: true };
    laneOf(lane).bars.push(bar);
    open.set(`${kind}:${taskId}:${lane}`, { lane, bar });
  };
  const close = (kind: BarKind, taskId: string, t: number, lane?: string) => {
    for (const [k, v] of open) {
      if (k.startsWith(`${kind}:${taskId}:`) && (!lane || v.lane === lane)) {
        v.bar.end = t; v.bar.open = false; open.delete(k);
      }
    }
  };
  for (const e of [...events].sort((a, b) => a.sequence - b.sequence)) {
    const t = Date.parse(e.at);
    if (e.pr !== undefined) prOf.set(e.taskId, e.pr);
    const actor = e.kind === "task.claimed" ? e.lane : e.reviewer;
    if (actor) laneOf(actor).last = Math.max(laneOf(actor).last, t);
    switch (e.kind) {
      case "task.claimed": if (e.lane) start(e.lane, "implement", e.taskId, t); break;
      case "task.review_assigned": close("implement", e.taskId, t); if (e.reviewer) start(e.reviewer, "review", e.taskId, t); break;
      case "task.review_verdict":
        close("review", e.taskId, t, e.reviewer);
        if (e.reviewer && String(e.verdict) === "APPROVE") start(e.reviewer, "merge", e.taskId, t);
        break;
      case "task.merged": close("merge", e.taskId, t); break;
      default: break;
    }
  }
  const from = now - windowMs;
  return [...lanes.entries()]
    .map(([lane, l]) => {
      const bars = l.bars.filter((b) => b.end >= from).map((b) => ({ ...b, start: Math.max(b.start, from) }));
      const waiting = l.bars.some((b) => b.open && b.kind !== "implement");
      return { lane, bars, lastEventAt: l.last, silent: waiting && now - l.last >= STALL_MS };
    })
    .sort((a, b) => a.lane.localeCompare(b.lane));
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cd apps/studio && npx vitest run src/runtime/lane-bars.test.ts`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add apps/studio/src/runtime/lane-bars.ts apps/studio/src/runtime/lane-bars.test.ts
git commit -m "feat(studio): lane timeline bars with a silence flag"
```

---

### Task 3: MissionGraph component (journey rail + graph + inspector)

**Files:**
- Create: `apps/studio/src/components/mission-graph.tsx`
- Create: `apps/studio/src/components/mission-graph.css`
- Test: `apps/studio/src/components/mission-graph.test.tsx`

**Interfaces:**
- Consumes: `Mission`, `MissionStep`, `MissionTask`, `TRUST_LABELS` (Task 1).
- Produces: `MissionGraph(props: { mission: Mission; selectedStepId: string | null; selectedTaskKey: string | null; onSelectStep(stepId: string): void; onSelectTask(key: string): void; onOpenTest(stepId: string): void })`.

Layout follows the Main artboard:
- **Summary line:** `{proven}/{total} proven · {inFlight} in flight · {needYou} need you`.
- **Step rail:** one numbered `<button>` per step, coloured by status. Its accessible name is `Step N: <title>, <status label>`.
- **Graph:** one column per step. The selected column has a highlight. Task nodes are `<button aria-pressed>` with the PR, title, lane and status label.
- **Inspector:** the selected task's trust ladder (5 segments, `TRUST_LABELS`) and an "Open its test" button that calls `onOpenTest(selectedStepId ?? first step)`.

Status labels: `proven → "Proven"`, `failed → "Failed"`, `needs_you → "Needs you"`, `preview_only → "Preview only"`, `not_run → "Not run"`.

- [ ] **Step 1: Write the failing tests**

```tsx
// apps/studio/src/components/mission-graph.test.tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MissionGraph } from "./mission-graph";
import type { Mission } from "../runtime/mission";

const mission: Mission = {
  contractId: "watch", title: "Watch plays inside the Studio",
  steps: [
    { stepId: "open", index: 0, title: "Open a journey", status: "not_run", reason: null },
    { stepId: "mark", index: 1, title: "Mark a skipped step safe", status: "needs_you", reason: "data-changing" },
  ],
  tasks: [{ key: "k564", pr: 564, issue: 519, title: "Owner marks a skipped step safe", lane: "gh-claude-8", reviewers: ["gh-claude-2"], step: "merged", blocked: false, trust: 3 }],
  summary: { proven: 0, total: 2, inFlight: 0, needYou: 1 },
};

function setup(over: Partial<Parameters<typeof MissionGraph>[0]> = {}) {
  const props = { mission, selectedStepId: null, selectedTaskKey: null, onSelectStep: vi.fn(), onSelectTask: vi.fn(), onOpenTest: vi.fn(), ...over };
  render(<MissionGraph {...props} />);
  return props;
}

describe("MissionGraph", () => {
  it("shows the quiet summary line", () => {
    setup();
    expect(screen.getByText("0/2 proven · 0 in flight · 1 need you")).toBeInTheDocument();
  });

  it("step rail names each step with its status, and a click selects it", async () => {
    const p = setup();
    await userEvent.click(screen.getByRole("button", { name: "Step 2: Mark a skipped step safe, Needs you" }));
    expect(p.onSelectStep).toHaveBeenCalledWith("mark");
  });

  it("task node click selects the task", async () => {
    const p = setup();
    await userEvent.click(screen.getByRole("button", { name: /#564/ }));
    expect(p.onSelectTask).toHaveBeenCalledWith("k564");
  });

  it("inspector shows the ladder lit to the task's trust", () => {
    setup({ selectedTaskKey: "k564" });
    const ladder = screen.getByRole("list", { name: "How far it got" });
    expect(ladder.querySelectorAll('[data-lit="true"]').length).toBe(3);
    expect(ladder).toHaveTextContent("WrittenReviewedMergedProvenSeen by you");
  });

  it("open its test uses the selected step", async () => {
    const p = setup({ selectedTaskKey: "k564", selectedStepId: "mark" });
    await userEvent.click(screen.getByRole("button", { name: "Open its test" }));
    expect(p.onOpenTest).toHaveBeenCalledWith("mark");
  });

  it("a journey with no tasks still draws its steps", () => {
    setup({ mission: { ...mission, tasks: [] } });
    expect(screen.getAllByRole("button", { name: /^Step / })).toHaveLength(2);
    expect(screen.getByText("No work linked to this journey yet")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cd apps/studio && npx vitest run src/components/mission-graph.test.tsx`
Expected: FAIL with `Failed to resolve import "./mission-graph"`.

- [ ] **Step 3: Implement**

```tsx
// apps/studio/src/components/mission-graph.tsx
import "./mission-graph.css";
import { TRUST_LABELS, type Mission, type MissionTask, type StepStatus } from "../runtime/mission";

export const STATUS_LABEL: Record<StepStatus, string> = {
  proven: "Proven", failed: "Failed", needs_you: "Needs you", preview_only: "Preview only", not_run: "Not run",
};
const TASK_LABEL: Record<MissionTask["step"], string> = { implement: "Writing", review: "In review", merge: "Merging", merged: "Merged · not proven" };

interface Props {
  mission: Mission;
  selectedStepId: string | null;
  selectedTaskKey: string | null;
  onSelectStep(stepId: string): void;
  onSelectTask(key: string): void;
  onOpenTest(stepId: string): void;
}

export function MissionGraph({ mission, selectedStepId, selectedTaskKey, onSelectStep, onSelectTask, onOpenTest }: Props) {
  const { summary } = mission;
  const task = mission.tasks.find((t) => t.key === selectedTaskKey) ?? null;
  return (
    <div className="mg">
      <p className="mg-summary">{`${summary.proven}/${summary.total} proven · ${summary.inFlight} in flight · ${summary.needYou} need you`}</p>
      <div className="mg-body">
        <nav className="mg-rail" aria-label="Journey steps">
          <h2>{mission.title}</h2>
          {mission.steps.map((s) => (
            <button key={s.stepId} type="button" className="mg-step" data-status={s.status} aria-pressed={s.stepId === selectedStepId}
              aria-label={`Step ${s.index + 1}: ${s.title}, ${STATUS_LABEL[s.status]}`} onClick={() => onSelectStep(s.stepId)}>
              <span className="mg-step-n">{s.index + 1}</span>
              <span className="mg-step-title">{s.title}</span>
              <span className="mg-step-status">{STATUS_LABEL[s.status]}</span>
            </button>
          ))}
        </nav>
        <section className="mg-graph" aria-label="Work graph">
          <div className="mg-cols" style={{ gridTemplateColumns: `repeat(${mission.steps.length}, minmax(140px, 1fr))` }}>
            {mission.steps.map((s) => (
              <div key={s.stepId} className="mg-col" data-status={s.status} data-selected={s.stepId === selectedStepId}>
                <span className="mg-col-head">{`STEP ${s.index + 1} · ${STATUS_LABEL[s.status]}`}</span>
                <span>{s.title}</span>
              </div>
            ))}
          </div>
          {mission.tasks.length === 0 ? (
            <p className="mg-empty">No work linked to this journey yet</p>
          ) : (
            <div className="mg-tasks">
              {mission.tasks.map((t) => (
                <button key={t.key} type="button" className="mg-node" data-step={t.step} data-blocked={t.blocked}
                  aria-pressed={t.key === selectedTaskKey} onClick={() => onSelectTask(t.key)}>
                  <span className="mg-node-head">{`${t.blocked ? "Blocked" : TASK_LABEL[t.step]} · ${t.pr ? `#${t.pr}` : "no PR"}`}</span>
                  <span className="mg-node-title">{t.title}</span>
                  <span className="mg-node-who">{[t.lane, ...t.reviewers].filter(Boolean).join(" → ")}</span>
                </button>
              ))}
            </div>
          )}
        </section>
        {task && (
          <aside className="mg-inspector" aria-label="Selected work">
            <h3>{task.title}</h3>
            <ul className="mg-ladder" aria-label="How far it got">
              {TRUST_LABELS.map((label, i) => (
                <li key={label} data-lit={i < task.trust}>{label}</li>
              ))}
            </ul>
            <button type="button" onClick={() => onOpenTest(selectedStepId ?? mission.steps[0]?.stepId ?? "")}>Open its test</button>
          </aside>
        )}
      </div>
    </div>
  );
}
```

`mission-graph.css` uses the colours from Global Constraints. Key on `[data-status]` and `[data-step]`. Use a dashed border for `not_run`. The selected column gets `background: rgba(233,231,226,.035)`. The summary is 12.5px and `#A9AEB8`. Layout is `flex-wrap: wrap`, so the rail and inspector stack at phone width (the Glance artboard). Copy the measurements from the Main artboard.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cd apps/studio && npx vitest run src/components/mission-graph.test.tsx`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add apps/studio/src/components/mission-graph.*
git commit -m "feat(studio): mission graph shows steps, linked work and its trust"
```

---

### Task 4: LanesTimeline component

**Files:**
- Create: `apps/studio/src/components/lanes-timeline.tsx`
- Create: `apps/studio/src/components/lanes-timeline.css`
- Test: `apps/studio/src/components/lanes-timeline.test.tsx`

**Interfaces:**
- Consumes: `Lane`, `LaneBar` (Task 2).
- Produces: `LanesTimeline(props: { lanes: Lane[]; now: number; windowMs: number })`.

Each lane is a `<li>` with its name, plus a `silent` badge when `lane.silent`. Each bar is absolutely positioned: `left = (start - (now - windowMs)) / windowMs * 100%` and `width = (end - start) / windowMs * 100%`. Its accessible text is `"<label> <kind>"`, and `data-kind` drives the colour (implement `#6F8CFF`, review `#F5A524`, merge `#4ADE9B`). A silent lane's open bars get `data-silent="true"` and a hatched red fill. The axis shows `−Nh … now`.

- [ ] **Step 1: Write the failing tests**

```tsx
// apps/studio/src/components/lanes-timeline.test.tsx
import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { LanesTimeline } from "./lanes-timeline";

const now = 1_000_000;
const lanes = [
  { lane: "gh-claude-5", silent: true, lastEventAt: 0, bars: [{ kind: "review" as const, label: "#548", start: now - 500, end: now, open: true }] },
  { lane: "gh-claude-6", silent: false, lastEventAt: now, bars: [{ kind: "implement" as const, label: "#559", start: now - 1000, end: now - 750, open: false }] },
];

describe("LanesTimeline", () => {
  it("places bars on the window", () => {
    render(<LanesTimeline lanes={lanes} now={now} windowMs={1000} />);
    const bar = screen.getByText("#559 implement").closest("[data-kind]") as HTMLElement;
    expect(bar.style.left).toBe("0%");
    expect(bar.style.width).toBe("25%");
  });

  it("flags a silent lane in text, not only colour", () => {
    render(<LanesTimeline lanes={lanes} now={now} windowMs={1000} />);
    const row = screen.getByText("gh-claude-5").closest("li")!;
    expect(within(row).getByText("silent")).toBeInTheDocument();
    expect(row.querySelector('[data-silent="true"]')).not.toBeNull();
  });

  it("shows an empty state", () => {
    render(<LanesTimeline lanes={[]} now={now} windowMs={1000} />);
    expect(screen.getByText("No agent has recorded work in this window")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cd apps/studio && npx vitest run src/components/lanes-timeline.test.tsx`
Expected: FAIL, unresolved import.

- [ ] **Step 3: Implement**

```tsx
// apps/studio/src/components/lanes-timeline.tsx
import "./lanes-timeline.css";
import type { Lane } from "../runtime/lane-bars";

const pct = (n: number) => `${Math.round(n * 10000) / 100}%`;

export function LanesTimeline({ lanes, now, windowMs }: { lanes: Lane[]; now: number; windowMs: number }) {
  if (lanes.length === 0) return <p className="lt-empty">No agent has recorded work in this window</p>;
  const from = now - windowMs;
  const hours = Math.round(windowMs / 3_600_000);
  return (
    <div className="lt">
      <div className="lt-axis" aria-hidden="true"><span>{`−${hours}h`}</span><span>now</span></div>
      <ul className="lt-lanes" aria-label="Agent lanes">
        {lanes.map((l) => (
          <li key={l.lane} className="lt-lane">
            <span className="lt-name">{l.lane}</span>
            {l.silent && <span className="lt-flag">silent</span>}
            <div className="lt-track">
              {l.bars.map((b, i) => (
                <div key={i} className="lt-bar" data-kind={b.kind} data-silent={l.silent && b.open}
                  style={{ left: pct((b.start - from) / windowMs), width: pct((b.end - b.start) / windowMs) }}>
                  <span>{`${b.label} ${b.kind}`}</span>
                </div>
              ))}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
```

`lanes-timeline.css` follows the Lanes artboard: 44px rows, a 30px track, bar colours keyed on `[data-kind]`, and `[data-silent="true"]` with `repeating-linear-gradient(135deg,#FF6B5E 0 4px,#3A1E1A 4px 8px)`. On narrow screens, wrap the track in `overflow-x: auto`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cd apps/studio && npx vitest run src/components/lanes-timeline.test.tsx`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add apps/studio/src/components/lanes-timeline.*
git commit -m "feat(studio): lanes timeline shows who works on what over time"
```

---

### Task 5: ProofTable + TestCanvas (emulated browser)

**Files:**
- Create: `apps/studio/src/runtime/test-frames.ts`
- Create: `apps/studio/src/components/proof-table.tsx`
- Create: `apps/studio/src/components/test-canvas.tsx`
- Create: `apps/studio/src/components/test-canvas.css`
- Test: `apps/studio/src/runtime/test-frames.test.ts`
- Test: `apps/studio/src/components/test-canvas.test.tsx`
- Test: `apps/studio/src/components/proof-table.test.tsx`

**Interfaces:**
- Consumes: `JourneyView`, `StepView`, `JourneyRunView` (`types.ts`); `actionText` (exported from `components/journey-canvas.tsx`); `Mission`, `STATUS_LABEL` (Tasks 1 and 3).
- Produces:
  - `type FrameStatus = "passed" | "failed" | "waits_for_you" | "not_run"`
  - `interface TestFrame { stepId: string; n: number; verb: "SEES" | "DOES" | "EXPECT"; text: string; status: FrameStatus; reason: string | null; expected: string[] }`
  - `testFrames(journey: JourneyView, run: JourneyRunView | null): TestFrame[]`
  - `ProofTable(props: { mission: Mission; onOpenTest(stepId: string): void })`
  - `TestCanvas(props: { frames: TestFrame[]; selected: number; onSelect(n: number): void; frameUrl(stepId: string): string | null; onMarkSafe(stepId: string): void; onSendBack(stepId: string): void })`

Frame rules:
- Each step gives one frame. `verb` is `DOES` when `step.action` exists, otherwise `SEES`. The last step is `EXPECT` when it has `expectedStates`.
- `text` is `actionText(step.action)` for DOES, otherwise the screen title.
- `status` comes from the run: screen `pass` → `passed`; `fail`/`drift` → `failed`; an incoming edge `skipped` → `waits_for_you`; nothing → `not_run`.

**The emulated browser:**
- A window frame with a URL bar showing `step.screen.title`, back/next buttons that call `onSelect(n ± 1)` (clamped), and a "frame N/M" label.
- The viewport is an `<img>` of the step's recorded frame, from `frameUrl(stepId)`. The frame bytes come from the existing frame endpoint that `JourneyFlows` already reads; `App` passes a URL built with `URL.createObjectURL` on `JourneyFrame.blob`. With no frame, the viewport says `No frame recorded for this step`.
- Under it, an inspector shows the verb, the text, an `expectedStates` list as assertions marked ✓, ✕ or ○ by status, and the reason.
- A `waits_for_you` frame shows two buttons, `I watched it — mark safe` and `Send back`.
- Above the browser, a card strip has one `<button aria-pressed>` per frame, joined by arrows, mirroring the existing Journey canvas.

- [ ] **Step 1: Write the failing tests**

```ts
// apps/studio/src/runtime/test-frames.test.ts
import { describe, expect, it } from "vitest";
import { testFrames } from "./test-frames";
import type { JourneyView } from "./types";

const journey: JourneyView = {
  contractId: "j", title: "J", arrows: [],
  steps: [
    { stepId: "a", screen: { screenId: "a", title: "Journey tab", scopePaths: [] }, promises: [] },
    { stepId: "b", screen: { screenId: "b", title: "Step 4 row", scopePaths: [] }, promises: [], action: { kind: "click", role: "button", name: "Mark safe" } as never },
    { stepId: "c", screen: { screenId: "c", title: "Ladder", scopePaths: [] }, promises: [], expectedStates: ["ladder 5/5"] },
  ],
};

describe("testFrames", () => {
  it("verbs and statuses", () => {
    const f = testFrames(journey, { state: "ready", kind: "replay", screens: { a: { frame: true, result: "pass" } }, edges: { "a->b": { result: "skipped", reason: "data-changing" } } });
    expect(f.map((x) => [x.n, x.verb, x.status])).toEqual([[1, "SEES", "passed"], [2, "DOES", "waits_for_you"], [3, "EXPECT", "not_run"]]);
    expect(f[1].reason).toBe("data-changing");
    expect(f[2].expected).toEqual(["ladder 5/5"]);
  });

  it("no run: all not_run", () => {
    expect(testFrames(journey, null).every((x) => x.status === "not_run")).toBe(true);
  });
});
```

```tsx
// apps/studio/src/components/test-canvas.test.tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TestCanvas } from "./test-canvas";
import type { TestFrame } from "../runtime/test-frames";

const frames: TestFrame[] = [
  { stepId: "a", n: 1, verb: "SEES", text: "Journey tab", status: "passed", reason: null, expected: ["tab selected"] },
  { stepId: "b", n: 2, verb: "DOES", text: "Click Mark safe", status: "waits_for_you", reason: "data-changing", expected: [] },
];

function setup(selected = 0, frameUrl = (id: string) => (id === "a" ? "blob:a" : null)) {
  const p = { frames, selected, onSelect: vi.fn(), frameUrl, onMarkSafe: vi.fn(), onSendBack: vi.fn() };
  render(<TestCanvas {...p} />);
  return p;
}

describe("TestCanvas", () => {
  it("shows the recorded frame in the emulated browser", () => {
    setup(0);
    expect(screen.getByRole("img", { name: "Frame 1: Journey tab" })).toHaveAttribute("src", "blob:a");
    expect(screen.getByText("frame 1/2")).toBeInTheDocument();
  });

  it("card click and next button move the frame", async () => {
    const p = setup(0);
    await userEvent.click(screen.getByRole("button", { name: /^2 DOES/ }));
    await userEvent.click(screen.getByRole("button", { name: "Next frame" }));
    expect(p.onSelect.mock.calls).toEqual([[1], [1]]);
  });

  it("previous is clamped at the first frame", async () => {
    const p = setup(0);
    await userEvent.click(screen.getByRole("button", { name: "Previous frame" }));
    expect(p.onSelect).toHaveBeenCalledWith(0);
  });

  it("a frame that waits for you offers mark safe and send back", async () => {
    const p = setup(1);
    expect(screen.getByText("No frame recorded for this step")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "I watched it — mark safe" }));
    await userEvent.click(screen.getByRole("button", { name: "Send back" }));
    expect(p.onMarkSafe).toHaveBeenCalledWith("b");
    expect(p.onSendBack).toHaveBeenCalledWith("b");
  });

  it("a passed frame offers no decision buttons", () => {
    setup(0);
    expect(screen.queryByRole("button", { name: "I watched it — mark safe" })).toBeNull();
  });
});
```

```tsx
// apps/studio/src/components/proof-table.test.tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProofTable } from "./proof-table";
import type { Mission } from "../runtime/mission";

const mission: Mission = {
  contractId: "j", title: "J",
  steps: [{ stepId: "mark", index: 0, title: "Mark a skipped step safe", status: "needs_you", reason: "data-changing" }],
  tasks: [], summary: { proven: 0, total: 1, inFlight: 0, needYou: 1 },
};

describe("ProofTable", () => {
  it("one row per step, with status text and an open-test action", async () => {
    const onOpenTest = vi.fn();
    render(<ProofTable mission={mission} onOpenTest={onOpenTest} />);
    expect(screen.getByRole("row", { name: /Mark a skipped step safe.*Needs you/ })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open test for step 1" }));
    expect(onOpenTest).toHaveBeenCalledWith("mark");
  });
});
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cd apps/studio && npx vitest run src/runtime/test-frames.test.ts src/components/test-canvas.test.tsx src/components/proof-table.test.tsx`
Expected: FAIL, unresolved imports.

- [ ] **Step 3: Implement**

```ts
// apps/studio/src/runtime/test-frames.ts
import type { JourneyRunView, JourneyView } from "./types";
import { actionText } from "../components/journey-canvas";

export type FrameStatus = "passed" | "failed" | "waits_for_you" | "not_run";
export interface TestFrame { stepId: string; n: number; verb: "SEES" | "DOES" | "EXPECT"; text: string; status: FrameStatus; reason: string | null; expected: string[] }

export function testFrames(journey: JourneyView, run: JourneyRunView | null): TestFrame[] {
  const last = journey.steps.length - 1;
  return journey.steps.map((s, i) => {
    const title = s.screen?.title ?? s.stepId;
    const expected = s.expectedStates ?? [];
    const verb: TestFrame["verb"] = s.action ? "DOES" : i === last && expected.length > 0 ? "EXPECT" : "SEES";
    const skipped = Object.entries(run?.edges ?? {}).find(([id, e]) => e.result === "skipped" && id.split("->")[1] === s.stepId);
    const screen = run?.screens?.[s.stepId];
    let status: FrameStatus = "not_run";
    let reason: string | null = null;
    if (skipped) { status = "waits_for_you"; reason = skipped[1].reason ?? null; }
    else if (screen?.result === "pass") status = "passed";
    else if (screen?.result) { status = "failed"; reason = screen.reason ?? null; }
    return { stepId: s.stepId, n: i + 1, verb, text: s.action ? actionText(s.action) : title, status, reason, expected };
  });
}
```

> Before writing this file, check that `actionText` is exported from `journey-canvas.tsx` and accepts a `StepAction`. If it is not exported, export it in this task. That is a one-word change, so add the file to this task's `git add`.
>
> Before committing Task 5, check the `JourneyRunView.edges` key format against `apps/cli/src/commands/serve/mod.rs` (search for `edges`). If it is not `"<from>-><to>"`, change the `split("->")` in both `mission.ts` and `test-frames.ts`, and change the fixtures to match. Record the real format in a comment.

```tsx
// apps/studio/src/components/test-canvas.tsx
import "./test-canvas.css";
import type { FrameStatus, TestFrame } from "../runtime/test-frames";

const LABEL: Record<FrameStatus, string> = { passed: "passed", failed: "failed", waits_for_you: "waits for you", not_run: "not run" };
const MARK: Record<FrameStatus, string> = { passed: "✓", failed: "✕", waits_for_you: "!", not_run: "○" };

interface Props {
  frames: TestFrame[];
  selected: number;
  onSelect(n: number): void;
  frameUrl(stepId: string): string | null;
  onMarkSafe(stepId: string): void;
  onSendBack(stepId: string): void;
}

export function TestCanvas({ frames, selected, onSelect, frameUrl, onMarkSafe, onSendBack }: Props) {
  const cur = frames[selected];
  if (!cur) return <p className="tc-empty">This journey has no steps to test</p>;
  const src = frameUrl(cur.stepId);
  return (
    <div className="tc">
      <ol className="tc-strip" aria-label="Test actions">
        {frames.map((f, i) => (
          <li key={f.stepId}>
            <button type="button" className="tc-card" data-status={f.status} aria-pressed={i === selected} onClick={() => onSelect(i)}>
              {`${f.n} ${f.verb} ${f.text} · ${LABEL[f.status]}`}
            </button>
          </li>
        ))}
      </ol>
      <div className="tc-main">
        <section className="tc-browser" aria-label="Emulated browser">
          <div className="tc-chrome">
            <button type="button" aria-label="Previous frame" onClick={() => onSelect(Math.max(0, selected - 1))}>‹</button>
            <button type="button" aria-label="Next frame" onClick={() => onSelect(Math.min(frames.length - 1, selected + 1))}>›</button>
            <span className="tc-url">{cur.text}</span>
            <span className="tc-count">{`frame ${cur.n}/${frames.length}`}</span>
          </div>
          <div className="tc-viewport" data-status={cur.status}>
            {src ? <img src={src} alt={`Frame ${cur.n}: ${cur.text}`} /> : <p>No frame recorded for this step</p>}
          </div>
          <div className="tc-status">{`${LABEL[cur.status]} · ${cur.verb.toLowerCase()} · ${cur.text}`}</div>
        </section>
        <aside className="tc-inspector" aria-label="Action inspector">
          <span className="tc-kicker">{`ACTION ${cur.n} · ${cur.verb}`}</span>
          <h3>{cur.text}</h3>
          {cur.expected.length > 0 && (
            <ul aria-label="Assertions">
              {cur.expected.map((e) => <li key={e}><span aria-hidden="true">{MARK[cur.status]}</span> {e}</li>)}
            </ul>
          )}
          {cur.reason && <p className="tc-reason">{cur.reason}</p>}
          {cur.status === "waits_for_you" && (
            <div className="tc-decide">
              <button type="button" onClick={() => onMarkSafe(cur.stepId)}>I watched it — mark safe</button>
              <button type="button" onClick={() => onSendBack(cur.stepId)}>Send back</button>
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}
```

```tsx
// apps/studio/src/components/proof-table.tsx
import type { Mission } from "../runtime/mission";
import { STATUS_LABEL } from "./mission-graph";

export function ProofTable({ mission, onOpenTest }: { mission: Mission; onOpenTest(stepId: string): void }) {
  return (
    <table className="proof">
      <caption>{`Can I trust “${mission.title}”?`}</caption>
      <thead><tr><th>Step</th><th>Promise</th><th>Status</th><th>Your call</th></tr></thead>
      <tbody>
        {mission.steps.map((s) => (
          <tr key={s.stepId} data-status={s.status}>
            <td>{s.index + 1}</td>
            <td>{s.title}</td>
            <td>{STATUS_LABEL[s.status]}{s.reason ? ` · ${s.reason}` : ""}</td>
            <td><button type="button" aria-label={`Open test for step ${s.index + 1}`} onClick={() => onOpenTest(s.stepId)}>Open test</button></td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
```

`test-canvas.css` follows the TestRun artboard. The card strip has a dotted background (`radial-gradient(#22252C 1px, transparent 1px) 0 0/16px 16px`), 232px cards and 40px arrows (a `::after` on each `li` except the last). The browser chrome is `#17181D` with a 28px URL bar. The viewport is `object-fit: contain`. The inspector is 420px wide and wraps under the browser below 900px.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cd apps/studio && npx vitest run src/runtime/test-frames.test.ts src/components/test-canvas.test.tsx src/components/proof-table.test.tsx`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add apps/studio/src/runtime/test-frames.* apps/studio/src/components/test-canvas.* apps/studio/src/components/proof-table.* apps/studio/src/components/journey-canvas.tsx
git commit -m "feat(studio): proof table and test canvas with an emulated browser"
```

---

### Task 6: Wire the Graph tab into App

**Files:**
- Modify: `apps/studio/src/App.tsx` (the top tabs around line 2892, the journeys data near the `/v1/journeys` fetch, and the task fold near the `foldTaskEvents` call)
- Create: `apps/studio/src/components/mission-view.tsx`
- Test: `apps/studio/src/components/mission-view.test.tsx`

**Interfaces:**
- Consumes: everything above; `JourneysView` (already fetched in App); `TaskState[]` (already folded in App); the run per flow (already fetched for Watch).
- Produces: `MissionView(props: { journeys: JourneyView[]; tasks: TaskState[]; runFor(contractId: string): JourneyRunView | null; lanes: Lane[]; now: number; frameUrl(stepId: string): string | null; onMarkSafe(stepId: string): void; onSendBack(stepId: string): void })`.

`MissionView` owns `journeyId`, `stepId`, `taskKey`, `sub: "graph" | "proof" | "test" | "lanes"` and `frame`. It renders:
- a journey picker, a list of `<button>`s showing `proven / total` from `buildMission`;
- the sub-tabs Graph · Proof · Lanes;
- the chosen component.

`onOpenTest(stepId)` sets `sub = "test"` and `frame = index of stepId`. Unlinked tasks show as a group under the picker, titled `Unlinked work`.

In `App.tsx`:
- Add a fourth top tab, `"graph"`, labelled `Graph`.
- Pass `journeysView.journeys`, the folded tasks, and `laneBars(timedEvents, Date.now(), 14 * 3_600_000)`. `timedEvents` maps the Runtime task events with their event timestamp, using the same source `handover.ts` uses for time.
- Pass a `runFor` that reads the cached flow run.
- `onMarkSafe` / `onSendBack` call the existing #564 mark-safe action that `JourneyFlows` uses. Find it with `grep -n "safe" apps/studio/src/components/journey-flows.tsx apps/studio/src/runtime/client.ts`, and reuse that function; do not add a new endpoint.

- [ ] **Step 1: Write the failing test**

```tsx
// apps/studio/src/components/mission-view.test.tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MissionView } from "./mission-view";
import type { JourneyView } from "../runtime/types";

const journeys: JourneyView[] = [{
  contractId: "watch", title: "Watch plays inside the Studio", arrows: [],
  steps: [{ stepId: "mark", screen: { screenId: "mark", title: "Mark a skipped step safe", scopePaths: [] }, promises: [] }],
}];

describe("MissionView", () => {
  it("goes graph → proof → test canvas on the chosen step", async () => {
    render(<MissionView journeys={journeys} tasks={[]} lanes={[]} now={0}
      runFor={() => ({ state: "ready", kind: "replay", screens: {}, edges: { "x->mark": { result: "skipped", reason: "data-changing" } } })}
      frameUrl={() => null} onMarkSafe={vi.fn()} onSendBack={vi.fn()} />);
    expect(screen.getByText("0/1 proven · 0 in flight · 1 need you")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Proof" }));
    await userEvent.click(screen.getByRole("button", { name: "Open test for step 1" }));
    expect(screen.getByRole("region", { name: "Emulated browser" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "I watched it — mark safe" })).toBeInTheDocument();
  });

  it("no journeys: says so", () => {
    render(<MissionView journeys={[]} tasks={[]} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} onSendBack={vi.fn()} />);
    expect(screen.getByText("No journeys in this project yet")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cd apps/studio && npx vitest run src/components/mission-view.test.tsx`
Expected: FAIL, unresolved import.

- [ ] **Step 3: Implement**

```tsx
// apps/studio/src/components/mission-view.tsx
import { useMemo, useState } from "react";
import type { JourneyRunView, JourneyView } from "../runtime/types";
import type { TaskState } from "../runtime/team-tasks";
import type { Lane } from "../runtime/lane-bars";
import { buildMission, unlinkedTasks } from "../runtime/mission";
import { testFrames } from "../runtime/test-frames";
import { MissionGraph } from "./mission-graph";
import { ProofTable } from "./proof-table";
import { TestCanvas } from "./test-canvas";
import { LanesTimeline } from "./lanes-timeline";

type Sub = "graph" | "proof" | "test" | "lanes";
const WINDOW_MS = 14 * 3_600_000;

interface Props {
  journeys: JourneyView[];
  tasks: TaskState[];
  runFor(contractId: string): JourneyRunView | null;
  lanes: Lane[];
  now: number;
  frameUrl(stepId: string): string | null;
  onMarkSafe(stepId: string): void;
  onSendBack(stepId: string): void;
}

export function MissionView({ journeys, tasks, runFor, lanes, now, frameUrl, onMarkSafe, onSendBack }: Props) {
  const [journeyId, setJourneyId] = useState<string | null>(journeys[0]?.contractId ?? null);
  const [stepId, setStepId] = useState<string | null>(null);
  const [taskKey, setTaskKey] = useState<string | null>(null);
  const [sub, setSub] = useState<Sub>("graph");
  const [frame, setFrame] = useState(0);
  const journey = journeys.find((j) => j.contractId === journeyId) ?? journeys[0];
  const missions = useMemo(() => journeys.map((j) => buildMission(j, runFor(j.contractId), tasks)), [journeys, tasks, runFor]);
  const orphans = useMemo(() => unlinkedTasks(journeys, tasks), [journeys, tasks]);
  if (!journey) return <p>No journeys in this project yet</p>;
  const mission = missions.find((m) => m.contractId === journey.contractId)!;
  const openTest = (id: string) => {
    setStepId(id);
    setFrame(Math.max(0, journey.steps.findIndex((s) => s.stepId === id)));
    setSub("test");
  };
  return (
    <div className="mv">
      <nav aria-label="Journeys" className="mv-journeys">
        {missions.map((m) => (
          <button key={m.contractId} type="button" aria-pressed={m.contractId === journey.contractId}
            onClick={() => { setJourneyId(m.contractId); setStepId(null); setTaskKey(null); }}>
            {`${m.title} · ${m.summary.proven} / ${m.summary.total}`}
          </button>
        ))}
        {orphans.length > 0 && (
          <section aria-label="Unlinked work">
            <h3>Unlinked work</h3>
            <ul>{orphans.map((t) => <li key={t.key}>{`${t.pr ? `#${t.pr}` : "no PR"} ${t.title}`}</li>)}</ul>
          </section>
        )}
      </nav>
      <div role="tablist" aria-label="Mission views">
        {(["graph", "proof", "lanes"] as const).map((s) => (
          <button key={s} role="tab" type="button" aria-selected={sub === s} onClick={() => setSub(s)}>
            {s === "graph" ? "Graph" : s === "proof" ? "Proof" : "Lanes"}
          </button>
        ))}
      </div>
      {sub === "graph" && <MissionGraph mission={mission} selectedStepId={stepId} selectedTaskKey={taskKey} onSelectStep={setStepId} onSelectTask={setTaskKey} onOpenTest={openTest} />}
      {sub === "proof" && <ProofTable mission={mission} onOpenTest={openTest} />}
      {sub === "test" && <TestCanvas frames={testFrames(journey, runFor(journey.contractId))} selected={frame} onSelect={setFrame} frameUrl={frameUrl} onMarkSafe={onMarkSafe} onSendBack={onSendBack} />}
      {sub === "lanes" && <LanesTimeline lanes={lanes} now={now} windowMs={WINDOW_MS} />}
    </div>
  );
}
```

Then edit `App.tsx`:
1. Extend the `tab` union with `"graph"` and add its tab button next to Team/Journeys.
2. Render `<MissionView … />` when `tab === "graph"`, passing the props named in this task's Interfaces block.

- [ ] **Step 4: Run the whole Studio gate**

Run: `cd apps/studio && npx vitest run src/components/mission-view.test.tsx && npm run typecheck && npm test`
Expected: the new test passes (2 passed), typecheck is clean, and the full suite has no new failures.

- [ ] **Step 5: Prove it in the running Studio (JPD observer)**

Start the Studio dev server (preview `apps/studio`, `npm run dev`). Open the Graph tab and capture one screenshot each:
- the graph with a step selected;
- Proof;
- the Test canvas on a `waits for you` frame;
- Lanes.

Attach them to the PR. If no journey run with a skipped edge exists locally, say `OBSERVER_MISSING` for that screen in the PR body instead of claiming it.

- [ ] **Step 6: Commit**

```bash
git add apps/studio/src/components/mission-view.* apps/studio/src/App.tsx
git commit -m "feat(studio): Graph tab joins mission graph, proof, test canvas and lanes"
```

---

## Delivery

- One issue (`current-wave`), branch `issue-<N>-studio-mission-graph`, one PR per task or one stacked PR. The executor decides; prefer one PR per task, since each task is independently testable.
- PR body:
  - the Keel card (paths, promise, proving command);
  - the tests run, with results;
  - security: read-only views, plus the existing mark-safe action and no new endpoint;
  - rollback: revert the PR, which removes the tab;
  - a `Closes #N` line.
- Review by a blind subagent, then merge by the approving reviewer (DELIVERY.md).
- Follow-up issue to open after merge: a Runtime field that links a task to a **step**, so nodes can sit in their step's column.
