import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { MissionView } from "./mission-view";
import { LIVENESS_MS } from "../runtime/lane-bars";
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
const lane = (name: string, at: number) => ({ lane: name, bars: [], lastEventAt: at }) as unknown as Lane;
const lanes = [lane("gh-claude-1", NOW), lane("gh-claude-9", NOW), lane("gh-claude-7", NOW - 2 * LIVENESS_MS)];

const renderLanes = async () => {
  const userEvent = fastUserEvent();
  render(<MissionView journeys={[]} tasks={tasks} lanes={lanes} now={NOW} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
  await userEvent.click(screen.getByRole("tab", { name: "Lanes" }));
  return userEvent;
};
const col = (name: RegExp) => screen.getByRole("region", { name });

describe("WorkKanban (#668)", () => {
  it("shows a loading state instead of empty counts until history settles", async () => {
    render(<MissionView journeys={[]} tasks={[]} lanes={[]} now={NOW} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} historyLoading />);
    await fastUserEvent().click(screen.getByRole("tab", { name: "Lanes" }));
    expect(screen.getAllByRole("status")).toHaveLength(2);
    expect(screen.getAllByText("Loading the team's history…")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: /Free 0/ })).toBeNull();
    expect(screen.queryByText(/· 0/)).toBeNull();
  });

  it("shows every open PR in its column with counts, without clicks; merged work is absent", async () => {
    await renderLanes();
    const board = screen.getByRole("region", { name: "Work by stage" });
    expect(Array.from(board.querySelectorAll(".wk-head")).map((h) => h.textContent))
      .toEqual(["IMPLEMENT · 1", "REVIEW · 1", "BLOCKED · 1", "BUILD QUEUE · 0", "SILENT · 1", "MERGE · 1"]);
    expect(within(col(/^IMPLEMENT/)).getByText(LONG)).toBeInTheDocument();
    expect(within(col(/^REVIEW/)).getByText("Review me")).toBeInTheDocument();
    expect(within(col(/^BLOCKED/)).getByText("Blocked one")).toBeInTheDocument();
    expect(within(col(/^SILENT/)).getByText("Stalled review")).toBeInTheDocument();
    expect(within(col(/^MERGE/)).getByText("Merging now")).toBeInTheDocument();
    expect(within(col(/^Waiting for build · 0/)).getByText("none")).toBeInTheDocument();
    expect(board).not.toHaveTextContent("Done already");
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
