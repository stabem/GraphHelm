import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { MissionView } from "./mission-view";
import { TEST_SILENT_MS as LIVENESS_MS, judgedLane } from "../test/task-fold";
import type { Lane } from "../runtime/lane-bars";
import type { TaskState } from "../runtime/team-tasks";

const wt = (key: string, over: Record<string, unknown>) => ({ key, taskId: key, pr: null, issue: null, lane: "gh-claude-1", title: null, prTitle: "",
  journeys: [], step: "implement", blockedBy: null, reviewers: [], mergeSha: null, headSha: null, repoUrl: null, rounds: [], lastSequence: 0, ...over });
const NOW = 10 * LIVENESS_MS;
const LONG = "A very long pull request title that names everything it does and must never be truncated anywhere";
const tasks = [
  wt("a", { issue: 1, pr: 11, prTitle: LONG, step: "implement", lastSequence: 5 }),
  wt("b", { issue: 2, pr: 22, prTitle: "Review me", step: "review", reviewers: ["gh-claude-9"], lastSequence: 4 }),
  wt("c", { issue: 2, pr: 23, prTitle: "Blocked one", step: "review", blockedBy: { reviewer: "gh-claude-5", headSha: "abcdef0123", commentUrl: "" }, reviewers: ["gh-claude-5"],
    rounds: [{ reviewer: "gh-claude-5", headSha: "abcdef0123", commentUrl: "", fixHead: null, blockedAt: null, fixedAt: null }], lastSequence: 3 }),
  wt("d", { issue: 3, pr: 33, prTitle: "Stalled review", step: "review", reviewers: ["gh-claude-7"], lastSequence: 2 }),
  wt("e", { issue: 3, pr: 34, prTitle: "Merging now", step: "merge", lastSequence: 2 }),
  wt("f", { issue: 3, pr: 35, prTitle: "Done already", step: "merged", mergeSha: "1234567890", lastSequence: 1 }),
] as unknown as TaskState[];
const lane = (name: string, at: number): Lane => judgedLane({ lane: name, bars: [], lastEventAt: at }, NOW);
const lanes = [lane("gh-claude-1", NOW), lane("gh-claude-9", NOW), lane("gh-claude-7", NOW - 2 * LIVENESS_MS)];

const renderLanes = async () => {
  const userEvent = fastUserEvent();
  render(<MissionView journeys={[]} tasks={tasks} lanes={lanes} now={NOW} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
  await userEvent.click(screen.getByRole("tab", { name: "Lanes" }));
  return userEvent;
};
const col = (name: RegExp) => screen.getByRole("region", { name });

describe("WorkKanban (#668)", () => {
  it("describes a held slot instead of calling a four-hour-old record active", async () => {
    const old = { lane: "gh-claude-1", silent: true, lastEventAt: NOW - 4 * 3_600_000, bars: [{
      kind: "implement" as const, label: "#11", taskId: "a", start: NOW - 4 * 3_600_000, end: NOW, open: true,
    }] } as Lane;
    const slots = [{ root: "D:/gh", ok: true as const, holder: { lane: "gh-claude-1", label: "#11", pid: 1, worktree: null, heldSeconds: 60 }, waiting: [] }];
    render(<MissionView journeys={[]} tasks={[tasks[0]!]} lanes={[old]} slots={slots} now={NOW} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    await fastUserEvent().click(screen.getByRole("tab", { name: "Lanes" }));
    const card = within(col(/^Waiting for build/)).getByRole("button", { name: /A very long pull request title/ });
    expect(card).toHaveTextContent("building");
    expect(card.querySelector(".mg-act")).toBeNull();
    expect(card).not.toHaveTextContent(/author active 4h.*ago/);
  });
  it("shows a loading state instead of empty counts until history settles", async () => {
    render(<MissionView journeys={[]} tasks={[]} lanes={[]} now={NOW} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} historyLoading />);
    await fastUserEvent().click(screen.getByRole("tab", { name: "Lanes" }));
    expect(screen.getAllByRole("status")).toHaveLength(2);
    expect(screen.getAllByText("Loading the team's history…")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: /Free 0/ })).toBeNull();
    expect(screen.queryByText(/· 0/)).toBeNull();
  });

  it("#1469: a Backlog column comes first, listing the route's backlog in its order with number, title and wait", async () => {
    const backlog = [
      { issue: 7, title: "Oldest wave issue", openedAt: new Date(NOW - 26 * 3_600_000).toISOString() },
      { issue: 9, title: "Newer wave issue", openedAt: new Date(NOW - 90 * 60_000).toISOString() },
    ];
    render(<MissionView journeys={[]} tasks={[]} lanes={[]} backlog={backlog} now={NOW} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    await fastUserEvent().click(screen.getByRole("tab", { name: "Lanes" }));
    const board = screen.getByRole("region", { name: "Work by stage" });
    expect(board.querySelector(".wk-head")?.textContent).toBe("BACKLOG · 2");
    expect(Array.from(col(/^BACKLOG/).querySelectorAll("li")).map((li) => li.textContent))
      .toEqual(["#7Oldest wave issuewaiting 1d 2h", "#9Newer wave issuewaiting 1h 30m"]);
  });
  it("shows every open PR in its column with counts, without clicks; merged work is absent", async () => {
    await renderLanes();
    const board = screen.getByRole("region", { name: "Work by stage" });
    expect(Array.from(board.querySelectorAll(".wk-head")).map((h) => h.textContent))
      .toEqual(["BACKLOG · 0", "IMPLEMENT · 1", "REVIEW · 1", "BLOCKED · 1", "BUILD QUEUE · 0", "SILENT · 1", "MERGE · 1"]);
    expect(within(col(/^IMPLEMENT/)).getByText(LONG)).toBeInTheDocument();
    expect(within(col(/^REVIEW/)).getByText("Review me")).toBeInTheDocument();
    expect(within(col(/^BLOCKED/)).getByText("Blocked one")).toBeInTheDocument();
    expect(within(col(/^SILENT/)).getByText("Stalled review")).toBeInTheDocument();
    expect(within(col(/^MERGE/)).getByText("Merging now")).toBeInTheDocument();
    expect(within(col(/^Waiting for build · 0/)).getByText("none")).toBeInTheDocument();
    expect(board).not.toHaveTextContent("Done already");
  });
  it("shows a blocked card as not started when its author has no later task record", async () => {
    const blocked = wt("blocked", { issue: 4, pr: 44, prTitle: "Blocked", step: "review", lane: "codex-13",
      blockedBy: { reviewer: "codex-review", headSha: "abcdef0123", commentUrl: "" },
      blockedSequence: 20, authorLastSequence: 10,
      rounds: [{ reviewer: "codex-review", headSha: "abcdef0123", commentUrl: "", fixHead: null, blockedAt: new Date(NOW - 5 * 60_000).toISOString(), fixedAt: null }] }) as unknown as TaskState;
    render(<MissionView journeys={[]} tasks={[blocked]} lanes={[]} now={NOW} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    await fastUserEvent().click(screen.getByRole("tab", { name: "Lanes" }));
    expect(within(col(/^BLOCKED/)).getByRole("button", { name: /Needs fix · not started/ })).toBeInTheDocument();
  });
  it("renders full names: the title and the author → reviewer line are whole", async () => {
    await renderLanes();
    expect(screen.getByText(LONG).textContent).toBe(LONG);
    // #706: each lane is its own unbreakable span; the line reads whole.
    expect(Array.from(col(/^REVIEW/).querySelectorAll(".mg-node-who")).map((w) => w.textContent)).toContain("gh-claude-1 → gh-claude-9");
  });
  it("clicking a card opens the Graph with that PR selected in the inspector", async () => {
    const userEvent = await renderLanes();
    await userEvent.click(within(col(/^SILENT/)).getByRole("button", { name: /Stalled review/ }));
    expect(screen.getByRole("tab", { name: "Graph" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("complementary", { name: "Selected work" })).toHaveTextContent("Stalled review");
  });
  it("Enter on a focused card opens it too", async () => {
    const userEvent = await renderLanes();
    within(col(/^MERGE/)).getByRole("button", { name: /Merging now/ }).focus();
    await userEvent.keyboard("{Enter}");
    expect(screen.getByRole("complementary", { name: "Selected work" })).toHaveTextContent("Merging now");
  });
});
