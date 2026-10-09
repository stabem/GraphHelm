import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { IssueGraph, MissionGraph } from "./mission-graph";
import { buildWorkGroups } from "../runtime/work-groups";
import type { TaskState } from "../runtime/team-tasks";
import type { Mission, MissionTask } from "../runtime/mission";
const userEvent = fastUserEvent();

const merged: MissionTask = {
  key: "k564", pr: 564, issue: 519, title: "Owner marks a skipped step safe", lane: "gh-claude-8", reviewers: ["gh-claude-2"], step: "merged",
  blocked: false, trust: 3, blockedBy: null, rounds: [], headSha: "4f1ead4c00", mergeSha: "9a9a9a9a11", repoUrl: "https://github.com/o/r",
};
const blocked: MissionTask = {
  ...merged, key: "k560", pr: 560, title: "Opening a journey runs it", lane: "gh-claude-6", reviewers: ["gh-claude-7"], step: "review", trust: 1,
  blocked: true, blockedBy: { reviewer: "gh-claude-7", headSha: "bf76a762ff" }, mergeSha: null, repoUrl: null,
  rounds: [{ reviewer: "gh-claude-7", headSha: "bf76a762ff", fixHead: null }],
};

const mission: Mission = {
  contractId: "watch", title: "Watch plays inside the Studio",
  steps: [
    { stepId: "open", index: 0, title: "Open a journey", status: "not_run", reason: null, promise: null },
    { stepId: "mark", index: 1, title: "Mark a skipped step safe", status: "needs_you", reason: "data-changing", promise: "The owner can mark it safe" },
  ],
  tasks: [merged],
  summary: { proven: 0, total: 2, inFlight: 0, needYou: 1, readyUnclaimed: 0 },
};

function setup(over: Partial<Parameters<typeof MissionGraph>[0]> = {}) {
  const props = { mission, selectedStepId: null, selectedTaskKey: null, onSelectStep: vi.fn(), onSelectTask: vi.fn(), onOpenTest: vi.fn(), ...over };
  const view = render(<MissionGraph {...props} />);
  return { ...props, ...view };
}

describe("MissionGraph", () => {
  it("shows the quiet summary line", () => {
    const { container } = setup();
    expect(container.querySelector(".mg-summary")).toHaveTextContent("0/2 proven · 0 in flight · 0 ready, unclaimed · 1 need you");
  });

  it("a column head click selects its step; the selected column is marked", async () => {
    const p = setup({ selectedStepId: "mark" });
    await userEvent.click(screen.getByRole("button", { name: "Column 1: Open a journey" }));
    expect(p.onSelectStep).toHaveBeenCalledWith("open");
    expect(screen.getByRole("button", { name: "Column 2: Mark a skipped step safe" })).toHaveAttribute("data-selected", "true");
  });

  it("task node click selects the task; merged work sits past the first unproven step", async () => {
    const p = setup();
    const node = screen.getByRole("button", { name: /#564/ });
    expect(node.style.left).toBe("168px");
    await userEvent.click(node);
    expect(p.onSelectTask).toHaveBeenCalledWith("k564");
  });

  it("inspector shows the ladder lit to the task's trust, the note, custody and a PR link", () => {
    setup({ selectedTaskKey: "k564" });
    const ins = screen.getByRole("complementary", { name: "Selected work" });
    const ladder = within(ins).getByRole("list", { name: "How far it got" });
    expect(ladder.querySelectorAll('[data-lit="true"]').length).toBe(3);
    expect(ladder).toHaveTextContent("WrittenReviewedMergedProvenSeen by you");
    expect(ins).toHaveTextContent("Merged, not proven yet");
    expect(ins).toHaveTextContent("Proves step 2");
    const rows = within(within(ins).getByRole("list", { name: "Who touched it" })).getAllByRole("listitem").map((li) => li.textContent);
    expect(rows).toEqual(["Implementgh-claude-8done", "Reviewgh-claude-2APPROVE", "Merge9a9a9a9amerged"]);
    expect(within(ins).getByRole("link", { name: "Open PR" })).toHaveAttribute("href", "https://github.com/o/r/pull/564");
    expect(ins).toHaveTextContent("Journey replay · step 2Needs you · data-changing");
  });

  it("a blocked task names the BLOCK, its reviewer and head", () => {
    setup({ mission: { ...mission, tasks: [blocked] }, selectedTaskKey: "k560" });
    const ins = screen.getByRole("complementary", { name: "Selected work" });
    expect(ins).toHaveTextContent("BLOCK by gh-claude-7 at bf76a762");
    expect(within(ins).queryByRole("link", { name: "Open PR" })).toBeNull();
    expect(ins).toHaveTextContent("No evidence recorded on this head");
  });

  it("open its test uses the task's step", async () => {
    const p = setup({ selectedTaskKey: "k564" });
    await userEvent.click(screen.getByRole("button", { name: "Open its test" }));
    expect(p.onOpenTest).toHaveBeenCalledWith("mark");
  });

  it("with no task selected, the inspector shows the selected step", async () => {
    const p = setup({ selectedStepId: "mark" });
    const ins = screen.getByRole("complementary", { name: "Selected step" });
    expect(ins).toHaveTextContent("STEP 2 · Needs you");
    expect(ins).toHaveTextContent("data-changing");
    await userEvent.click(within(ins).getByRole("button", { name: "Open test" }));
    expect(p.onOpenTest).toHaveBeenCalledWith("mark");
  });

  it("draws edges between journey tasks with an arrow", () => {
    const { container } = setup({ mission: { ...mission, tasks: [merged, { ...blocked, pr: 600, key: "k600" }] } });
    expect(container.querySelectorAll(".mg-seg").length).toBeGreaterThan(1);
    expect(container.querySelector('.mg-seg[data-done="true"]')).not.toBeNull();
  });

  it("a journey with no tasks still draws its steps", () => {
    setup({ mission: { ...mission, tasks: [] } });
    expect(screen.getAllByRole("button", { name: /^Column / })).toHaveLength(2);
    expect(screen.getByText("No work linked to this journey yet")).toBeInTheDocument();
    expect(screen.getByText("Merged is not done. Done = the replay proves the step.")).toBeInTheDocument();
  });
});

const ts = (key: string, over: Partial<TaskState>): TaskState => ({
  key, taskId: key, branch: null, issue: 1, pr: null, lane: "gh-claude-1", headSha: null, journeys: [], step: "implement",
  blockedBy: null, reviewers: [], mergeSha: null, repoUrl: null, strayVerdicts: [], title: "T", summary: null, prTitle: null,
  prSummary: null, critic: null, recordedHeads: [], parent: null, rounds: [], clock: { since: null, spent: {} }, lastSequence: 0, ...over,
});

describe("IssueGraph (#591)", () => {
  const rnd = { reviewer: "gh-claude-7", headSha: "a", commentUrl: "", fixHead: "b", blockedAt: null, fixedAt: null };
  const group = buildWorkGroups([
    ts("m1", { pr: 10, prTitle: "Merged one", step: "merged", mergeSha: "9a9a9a9a11", rounds: [rnd], reviewers: ["gh-claude-7"] }),
    ts("m2", { pr: 11, prTitle: "Merged two", step: "merged", mergeSha: "8b8b8b8b22" }),
    ts("o", { pr: 12, prTitle: "Open one", step: "review", blockedBy: { reviewer: "gh-claude-7", headSha: "a", commentUrl: "" }, rounds: [{ ...rnd, fixHead: null }] }),
  ], [])[0]!;
  const draw = (sel: string | null = null) => {
    const onSelectTask = vi.fn();
    const view = render(<IssueGraph group={group} stepFor={() => undefined} selectedTaskKey={sel} selectedCol={null} onSelectTask={onSelectTask} onSelectCol={vi.fn()} onOpenTest={vi.fn()} />);
    return { onSelectTask, ...view };
  };
  it("open rows show; merged rows fold under Merged · N, collapsed by default and expandable", async () => {
    const { container } = draw();
    expect(Array.from(container.querySelectorAll(".mg-row")).map((r) => r.getAttribute("data-row"))).toEqual(["o"]);
    const fold = screen.getByRole("button", { name: "Merged · 2" });
    expect(fold).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(fold);
    expect(fold).toHaveAttribute("aria-expanded", "true");
    expect(Array.from(container.querySelectorAll(".mg-row")).map((r) => r.getAttribute("data-row"))).toEqual(["o", "m1", "m2"]);
  });
  it("a row draws done cells, BLOCK in red, the current stage as the card, and selects its PR on click", async () => {
    const { container, onSelectTask } = draw("o");
    const row = container.querySelector('.mg-row[data-row="o"]')!;
    expect(row.querySelector('.mg-cell[data-stage="review"] .mg-cell-mark')).toHaveAttribute("data-tone", "block");
    expect(row.querySelector('.mg-node[data-current="true"]')).toHaveAttribute("data-stage", "fix");
    expect(row.querySelectorAll('.mg-cell[data-cell="ahead"]').length).toBe(3);
    await userEvent.click(within(row as HTMLElement).getByRole("button", { name: "PR #12 Implement · gh-claude-1 · ✓" }));
    await userEvent.click(within(row as HTMLElement).getByRole("button", { name: "Row PR #12: Open one" }));
    expect(onSelectTask.mock.calls).toEqual([["o"], ["o"]]);
  });
  it("the current card and the inspector header show the time in stage and the health flag", () => {
    const health = { o: { flag: "blocked" as const, text: "Blocked by gh-claude-7", tone: "orange" as const, elapsedMs: 77 * 60_000, elapsed: "1h 17m" } };
    const { container } = render(<IssueGraph group={group} stepFor={() => undefined} selectedTaskKey="o" selectedCol={null} onSelectTask={vi.fn()} onSelectCol={vi.fn()} onOpenTest={vi.fn()} health={health} />);
    const card = container.querySelector('.mg-row[data-row="o"] .mg-node[data-current="true"]')!;
    // #591: a BLOCK opens a Fixing card for the author; the sub-line names the BLOCK, the time sits on the who line.
    expect(card.querySelector(".mg-node-label")).toHaveTextContent("Fixing");
    expect(card).toHaveAttribute("data-state", "work");
    expect(card.querySelector(".mg-node-sub")).toHaveTextContent("after BLOCK by gh-claude-7");
    expect(card.querySelector(".mg-node-foot")).toHaveTextContent("gh-claude-11h 17m");
    expect(card.querySelector(".mg-node-time")).toHaveTextContent("1h 17m");
    expect(card.querySelector(".mg-health")).toBeNull();
    expect(within(container.querySelector(".mg-custody") as HTMLElement).getByText("pending").closest("li")).toHaveTextContent("Fixgh-claude-1pending");
    const head = container.querySelector(".mg-ins-head")!;
    expect(head.querySelector('.mg-health[data-flag="blocked"]')).toHaveTextContent("Blocked by gh-claude-71h 17m");
  });
  it("a stalled card turns red and reads Stalled; the flag is its own truncated line with a tooltip", () => {
    const text = "gh-claude-6 silent 5h 57m";
    const health = { o: { flag: "stalled" as const, text, tone: "red" as const, elapsedMs: 27 * 60_000, elapsed: "27m" } };
    const { container } = render(<IssueGraph group={group} stepFor={() => undefined} selectedTaskKey={null} selectedCol={null} onSelectTask={vi.fn()} onSelectCol={vi.fn()} onOpenTest={vi.fn()} health={health} />);
    const card = container.querySelector('.mg-row[data-row="o"] .mg-node[data-current="true"]')!;
    expect(card).toHaveAttribute("data-state", "stalled");
    expect(card.querySelector(".mg-node-label")).toHaveTextContent("Stalled");
    const flag = card.querySelector(".mg-health")!;
    expect(flag.textContent).toBe(text);
    expect(flag.querySelector(".mg-health-text")).toHaveAttribute("title", text);
    expect(flag.querySelector(".mg-health-time")).toBeNull();
    expect(card.querySelector(".mg-node-foot .mg-node-time")).toHaveTextContent("27m");
    expect(card).toHaveAttribute("data-dense", "true");
  });
});

describe("IssueGraph live pace (#591)", () => {
  it("the shared clock ticks the card timer and the progressbar reports the share of the usual time", async () => {
    vi.useFakeTimers();
    try {
      const { MissionView } = await import("./mission-view");
      const { act } = await import("@testing-library/react");
      const NOW = Date.parse("2026-10-09T12:00:00Z");
      const task = ts("o", { pr: 12, prTitle: "Open one", step: "review", lane: "gh-claude-6", reviewers: ["gh-claude-7"],
        blockedBy: { reviewer: "gh-claude-7", headSha: "a", commentUrl: "" }, clock: { since: new Date(NOW - 3_600_000 - 4_000).toISOString(), spent: {} } });
      const lanes = [{ lane: "gh-claude-6", bars: [], silent: false, lastEventAt: NOW - 12 * 60_000 }];
      render(<MissionView journeys={[]} tasks={[task]} runFor={() => null} lanes={lanes} now={NOW} frameUrl={() => ""} onMarkSafe={vi.fn()} />);
      const bars = screen.getAllByRole("progressbar");
      expect(bars.length).toBe(2); // the card and the inspector header
      const bar = bars[0]!;
      expect(bar).toHaveAttribute("aria-valuenow", "50");
      expect(bar).toHaveAttribute("aria-label", "Fixing for 1h, expected about 2h");
      expect(screen.getAllByText("1h 00m 04s").length).toBe(2);
      expect(screen.getAllByText("last activity 12m ago").length).toBe(2);
      act(() => { vi.advanceTimersByTime(1000); });
      expect(screen.getAllByText("1h 00m 05s").length).toBe(2);
    } finally { vi.useRealTimers(); }
  });
});

describe("PR #581's shape on the Graph (#591)", () => {
  it("a pushed fix reads Re-review waiting on the reviewer, timed from the push, with no Blocked by flag", async () => {
    vi.useFakeTimers();
    try {
      const { MissionView } = await import("./mission-view");
      const NOW = Date.parse("2026-10-09T12:00:00Z"), H = 3_600_000;
      const task = ts("pr-581", { issue: 549, pr: 581, prTitle: "Fix the thing", step: "review", lane: "gh-claude-11", reviewers: ["gh-claude-8"],
        headSha: "66613c95aa", blockedBy: { reviewer: "gh-claude-8", headSha: "1a1a1a1a00", commentUrl: "" },
        rounds: [{ reviewer: "gh-claude-8", headSha: "1a1a1a1a00", commentUrl: "", fixHead: "66613c95aa", blockedAt: new Date(NOW - 5.5 * H).toISOString(), fixedAt: new Date(NOW - 3 * H).toISOString() }],
        clock: { since: new Date(NOW - 5.5 * H).toISOString(), spent: {} } });
      const lanes = [{ lane: "gh-claude-8", bars: [], silent: false, lastEventAt: NOW - 5 * 60_000 }];
      const { container } = render(<MissionView journeys={[]} tasks={[task]} runFor={() => null} lanes={lanes} now={NOW} frameUrl={() => ""} onMarkSafe={vi.fn()} />);
      const card = container.querySelector('.mg-node[data-current="true"]')!;
      expect(card.querySelector(".mg-node-label")).toHaveTextContent("Re-review");
      expect(card.querySelector(".mg-node-sub")).toHaveTextContent("waiting on gh-claude-8");
      expect(card.querySelector(".mg-node-who")).toHaveTextContent("gh-claude-8");
      expect(card).toHaveTextContent("3h 00m 00s");
      expect(container).not.toHaveTextContent("Fixing");
      expect(container).not.toHaveTextContent("Blocked by");
      expect(screen.getByText("fix pushed 66613c95")).toBeInTheDocument();
    } finally { vi.useRealTimers(); }
  });
  it("a lane silent 1h 10m reads Stalled on the card and in the inspector, and the dot is red", async () => {
    vi.useFakeTimers();
    try {
      const { MissionView } = await import("./mission-view");
      const NOW = Date.parse("2026-10-09T12:00:00Z");
      const task = ts("w", { pr: 9, prTitle: "Work", step: "implement", lane: "gh-claude-9", clock: { since: new Date(NOW - 7_200_000).toISOString(), spent: {} } });
      const lanes = [{ lane: "gh-claude-9", bars: [], silent: true, lastEventAt: NOW - 70 * 60_000 }];
      const { container } = render(<MissionView journeys={[]} tasks={[task]} runFor={() => null} lanes={lanes} now={NOW} frameUrl={() => ""} onMarkSafe={vi.fn()} />);
      expect(container.querySelector('.mg-node[data-current="true"] .mg-node-label')).toHaveTextContent("Stalled");
      expect(screen.getAllByText("gh-claude-9 silent 1h 10m").length).toBe(2);
      expect(container.querySelector('.mg-act-dot[data-tone="green"]')).toBeNull();
    } finally { vi.useRealTimers(); }
  });
});
