import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { MissionView } from "./mission-view";
import type { JourneyView } from "../runtime/types";
import type { TaskState } from "../runtime/team-tasks";

const journeys: JourneyView[] = [{
  contractId: "watch", title: "Watch plays inside the Studio", arrows: [],
  steps: [{ stepId: "mark", screen: { screenId: "mark", title: "Mark a skipped step safe", scopePaths: [] }, promises: [] }],
}];

describe("MissionView", () => {
  it("goes graph → proof → test canvas on the chosen step", async () => {
    const userEvent = fastUserEvent();
    const onMarkSafe = vi.fn();
    render(<MissionView journeys={journeys} tasks={[]} lanes={[]} now={0}
      runFor={() => ({ state: "ready", kind: "replay", screens: {}, edges: { mark: { result: "skipped", reason: "data-changing" } } })}
      frameUrl={() => null} onMarkSafe={onMarkSafe} onSendBack={vi.fn()} />);
    expect(document.querySelector(".mg-summary")).toHaveTextContent("0/1 proven · 0 in flight · 0 ready, unclaimed · 1 need you");
    await userEvent.click(screen.getByRole("tab", { name: "Proof" }));
    await userEvent.click(screen.getByRole("button", { name: "Open test for step 1" }));
    expect(screen.getByRole("region", { name: "Emulated browser" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "I watched it — mark safe" }));
    expect(onMarkSafe).toHaveBeenCalledWith("mark", "watch");
  });

  it("no journeys: says so", () => {
    render(<MissionView journeys={[]} tasks={[]} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} onSendBack={vi.fn()} />);
    expect(screen.getByText("No journeys in this project yet")).toBeInTheDocument();
  });
  it("is wide, shows journeys as a rail, and folds unlinked work", async () => {
    const userEvent = fastUserEvent();
    const two: JourneyView[] = [...journeys, { ...journeys[0], contractId: "other", title: "Another journey" }];
    const tasks = [{ key: "t1", taskId: "t1", pr: 40, title: "Loose PR", prTitle: "", journeys: [], step: "claimed", blockedBy: null, reviewers: [] }] as unknown as TaskState[];
    const { container } = render(<MissionView journeys={two} tasks={tasks} lanes={[]} now={0} runFor={() => null}
      frameUrl={() => null} onMarkSafe={vi.fn()} />);
    expect(container.querySelector(".mv")).toHaveAttribute("data-wide", "true");
    const rail = screen.getByRole("navigation", { name: "Journeys" });
    const rows = Array.from(rail.querySelectorAll<HTMLButtonElement>("button.mv-journey:not(.mv-group)"));
    expect(rows).toHaveLength(2);
    expect(rail.querySelector("button.mv-group")).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(rows[0]);
    expect(rows[0]).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(rows[1]);
    expect(rows[1]).toHaveAttribute("aria-pressed", "true");
    expect(rows[0]).toHaveAttribute("aria-pressed", "false");
    const fold = screen.getByRole("button", { name: "Unlinked work · 1" });
    expect(fold).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("#40 Loose PR")).toBeNull();
    await userEvent.click(fold);
    expect(fold).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("#40 Loose PR")).toBeInTheDocument();
  });

  it("clicking a step chip selects its column, and a task node opens its custody", async () => {
    const userEvent = fastUserEvent();
    const two: JourneyView[] = [{ ...journeys[0]!, steps: [...journeys[0]!.steps, { stepId: "next", screen: { screenId: "next", title: "Next step", scopePaths: [] }, promises: [] }] }];
    const tasks = [{ key: "t9", taskId: "t9", pr: 9, issue: 519, lane: "gh-claude-8", title: "Work", prTitle: "", journeys: ["watch"], step: "review",
      blockedBy: { reviewer: "gh-claude-2", headSha: "abcdef0123", commentUrl: "" }, reviewers: ["gh-claude-2"], mergeSha: null, headSha: "abcdef0123", repoUrl: null,
      rounds: [{ reviewer: "gh-claude-2", headSha: "abcdef0123", commentUrl: "", fixHead: null, blockedAt: null, fixedAt: null }] }] as unknown as TaskState[];
    render(<MissionView journeys={two} tasks={tasks} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    expect(screen.getByText("issue #519 · 1 task")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Step 2: Next step, Not run" }));
    expect(document.querySelector("button.mv-journey:not(.mv-group)")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Column 2: Next step" })).toHaveAttribute("data-selected", "true");
    expect(screen.getByRole("button", { name: "Column 1: Mark a skipped step safe" })).toHaveAttribute("data-selected", "false");
    expect(screen.getByRole("complementary", { name: "Selected step" })).toHaveTextContent("STEP 2 · Not run");
    await userEvent.click(within(screen.getByRole("region", { name: "Work graph" })).getByRole("button", { name: /#9/ }));
    const ins = screen.getByRole("complementary", { name: "Selected work" });
    expect(ins).toHaveTextContent("BLOCK by gh-claude-2 at abcdef01");
    expect(screen.getByRole("button", { name: "Column 1: Mark a skipped step safe" })).toHaveAttribute("data-selected", "true");
  });

  it("lanes tab shows the lane cards", async () => {
    const userEvent = fastUserEvent();
    const lanes = [{ lane: "gh-claude-3", silent: false, lastEventAt: 0, bars: [] }];
    render(<MissionView journeys={journeys} tasks={[]} lanes={lanes} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    await userEvent.click(screen.getByRole("tab", { name: "Lanes" }));
    expect(screen.getByText("FREE HANDS").parentElement).toHaveTextContent("gh-claude-3");
    expect(screen.queryByRole("navigation", { name: "Journeys" })).toBeNull();
  });

  const wt = (key: string, over: Record<string, unknown>) => ({ key, taskId: key, pr: null, issue: null, lane: "gh-claude-1", title: null, prTitle: "",
    journeys: [], step: "implement", blockedBy: null, reviewers: [], mergeSha: null, headSha: null, repoUrl: null, rounds: [], lastSequence: 0, ...over });

  it("lists work by issue first, opens the open group's stage graph and its inspector", async () => {
    const userEvent = fastUserEvent();
    const tasks = [
      wt("a", { issue: 519, pr: 548, title: "Watch plays inside the Studio", prTitle: "Proof recorded like replay", step: "review", journeys: ["watch"],
        blockedBy: { reviewer: "gh-claude-5", headSha: "abcdef0123", commentUrl: "" }, reviewers: ["gh-claude-5"],
        rounds: [{ reviewer: "gh-claude-5", headSha: "abcdef0123", commentUrl: "", fixHead: null, blockedAt: null, fixedAt: null }], lastSequence: 9 }),
      wt("b", { issue: 519, pr: 560, title: "Watch plays inside the Studio", prTitle: "Opening a journey runs it", step: "merged", journeys: ["watch"], lastSequence: 8 }),
      wt("c", { issue: 400, pr: 401, title: "Old work", step: "merged", lastSequence: 99 }),
    ] as unknown as TaskState[];
    render(<MissionView journeys={journeys} tasks={tasks} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    const work = screen.getByRole("region", { name: "Work by issue" });
    const cards = Array.from(work.querySelectorAll("button.mv-group")).map((b) => b.textContent);
    expect(cards).toEqual(["#519 Watch plays inside the Studio0 / 2", "#400 Old work0 / 1"]);
    expect(work.querySelector("button.mv-group")).toHaveAttribute("aria-pressed", "true");
    expect(within(work).getByText("issue #519 · 2 PRs")).toBeInTheDocument();
    expect(within(work).getByRole("button", { name: "PR #548: Proof recorded like replay" })).toHaveAttribute("data-stage", "fix");
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("#519 Watch plays inside the Studio");
    const graph = screen.getByRole("region", { name: "Work graph" });
    expect(Array.from(graph.querySelectorAll(".mg-col-head")).map((h) => h.textContent))
      .toEqual(["PLAN · 0", "IMPLEMENT · 0", "REVIEW · 0", "FIX · 1", "MERGE · 0", "MERGED · 1", "PROVEN · 0"]);
    expect(graph.querySelectorAll(".mg-seg").length).toBeGreaterThan(0);
    expect(within(graph).getByRole("button", { name: "Merged · 1" })).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(within(graph).getByRole("button", { name: "Row PR #548: Proof recorded like replay" }));
    expect(screen.getByRole("button", { name: "Column Fix: 1" })).toHaveAttribute("data-selected", "true");
    const ins = screen.getByRole("complementary", { name: "Selected work" });
    expect(within(ins).getByRole("list", { name: "Who touched it" })).toHaveTextContent("BLOCK");
    expect(ins).toHaveTextContent("Evidence on this head");
    await userEvent.click(screen.getByRole("tab", { name: "Proof" }));
    expect(screen.getByRole("button", { name: "Open test for step 1" })).toBeInTheDocument();
  });

  it("a group naming no journey: Proof says so", async () => {
    const userEvent = fastUserEvent();
    const tasks = [wt("a", { issue: 7, pr: 70 })] as unknown as TaskState[];
    render(<MissionView journeys={journeys} tasks={tasks} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Issue #7");
    await userEvent.click(screen.getByRole("tab", { name: "Proof" }));
    expect(screen.getByText("This work names no journey yet — agents pass --journeys when they claim.")).toBeInTheDocument();
  });

  it("full page: breadcrumb, no Team in the nav, live indicator", () => {
    const onTeam = vi.fn();
    render(<MissionView journeys={journeys} tasks={[]} lanes={[]} now={12_000} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()}
      runName="gh-team" lastRecordAt={0} onTeam={onTeam} />);
    expect(screen.getByText("Run gh-team")).toBeInTheDocument();
    expect(screen.getByText("Mission graph")).toBeInTheDocument();
    expect(screen.getByText("live · last record 12 s ago")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Team" })).toBeNull();
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["Graph", "Lanes", "Proof"]);
  });

  it("no last record time: the live indicator is omitted", () => {
    render(<MissionView journeys={journeys} tasks={[]} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} runName="r" lastRecordAt={Number.NaN} />);
    expect(screen.queryByText(/live · last record/)).toBeNull();
  });
  it("#591: the selected task defaults to the one that most needs the owner", () => {
    const pick = (rows: Record<string, unknown>[]) => {
      const { unmount } = render(<MissionView journeys={journeys} tasks={rows as unknown as TaskState[]} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
      const ins = screen.getByRole("complementary", { name: "Selected work" });
      const text = ins.querySelector("h3")?.textContent ?? null;
      unmount();
      return text;
    };
    const block = { reviewer: "r", headSha: "abcdef0123", commentUrl: "" };
    expect(pick([wt("a", { issue: 1, pr: 1, prTitle: "Old merged", step: "merged", lastSequence: 9 }), wt("b", { issue: 1, pr: 2, prTitle: "In review", step: "review", lastSequence: 3 }),
      wt("c", { issue: 1, pr: 3, prTitle: "Blocked", step: "review", blockedBy: block, lastSequence: 1 })])).toBe("Blocked");
    expect(pick([wt("a", { issue: 1, pr: 1, prTitle: "Old merged", step: "merged", lastSequence: 9 }), wt("b", { issue: 1, pr: 2, prTitle: "In review", step: "review", lastSequence: 3 })])).toBe("In review");
    expect(pick([wt("a", { issue: 1, pr: 1, prTitle: "Older", step: "merged", lastSequence: 2 }), wt("b", { issue: 1, pr: 2, prTitle: "Newest", step: "merged", lastSequence: 7 })])).toBe("Newest");
    render(<MissionView journeys={journeys} tasks={[wt("a", { issue: 1, pr: 1 })] as unknown as TaskState[]} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    expect(screen.queryByText("Pick a PR to see who touched it.")).toBeNull();
  });

  it("#591: the selected card carries numbered step chips and PR rows; other cards show no PR chips", () => {
    const tasks = [wt("a", { issue: 5, pr: 50, prTitle: "First", step: "review", summary: "The owner sees it" }), wt("b", { issue: 5, pr: 51, prTitle: "Second" }),
      wt("c", { issue: 6, pr: 60, prTitle: "Elsewhere", step: "merged" })] as unknown as TaskState[];
    render(<MissionView journeys={journeys} tasks={tasks} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    const cards = Array.from(screen.getByRole("region", { name: "Work by issue" }).querySelectorAll(".mv-jcard"));
    expect(Array.from(cards[0]!.querySelectorAll(".mv-pr-chip")).map((c) => c.textContent)).toEqual(["1", "2"]);
    expect(Array.from(cards[0]!.querySelectorAll(".mv-step-ids")).map((c) => c.textContent)).toEqual(["PR #50", "PR #51"]);
    expect(cards[1]!.querySelectorAll(".mv-pr-chip")).toHaveLength(0);
    expect(cards[1]!.querySelectorAll(".mv-seg")).toHaveLength(1);
    expect(screen.getByText("The owner sees it")).toBeInTheDocument();
  });

  it("#591: open work spins on the rail; blocked work shows a red ! instead; merged work shows neither", () => {
    const block = { reviewer: "r", headSha: "abcdef0123", commentUrl: "" };
    const tasks = [wt("a", { issue: 5, pr: 50, prTitle: "Moving", step: "review" }), wt("b", { issue: 5, pr: 51, prTitle: "Held", step: "review", blockedBy: block }),
      wt("m", { issue: 5, pr: 52, prTitle: "Done", step: "merged" }), wt("c", { issue: 6, pr: 60, prTitle: "Elsewhere", step: "merged" })] as unknown as TaskState[];
    render(<MissionView journeys={journeys} tasks={tasks} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} />);
    const cards = Array.from(screen.getByRole("region", { name: "Work by issue" }).querySelectorAll<HTMLElement>(".mv-jcard"));
    const title = (c: HTMLElement) => c.querySelector(".mv-group")!;
    expect(within(title(cards[0]!) as HTMLElement).getByRole("img", { name: "in progress" })).toHaveAttribute("data-kind", "spin");
    expect(within(title(cards[1]!) as HTMLElement).queryByRole("img")).toBeNull();
    const steps = Array.from(cards[0]!.querySelectorAll<HTMLElement>(".mv-step"));
    const icon = (el: HTMLElement) => el.querySelector(".mv-prog")?.getAttribute("aria-label") ?? null;
    const byTitle = (t: string) => steps.find((s) => s.textContent!.includes(t))!;
    expect([icon(byTitle("Moving")), icon(byTitle("Held")), icon(byTitle("Done"))]).toEqual(["in progress", "blocked", null]);
    const chips = Array.from(cards[0]!.querySelectorAll<HTMLElement>(".mv-pr-chip"));
    const chip = (pr: number) => chips.find((c) => c.getAttribute("aria-label")!.startsWith(`PR #${pr}`))!;
    expect([icon(chip(50)), icon(chip(51)), icon(chip(52))]).toEqual(["in progress", "blocked", null]);
    expect(chip(51).querySelector(".mv-prog")).toHaveAttribute("data-kind", "alert");
  });

  it("#591: agents right now come from the bots; away/shipped from the handover", () => {
    const bot = (name: string, state: string, doingNow: string, quietMinutes: number | null) => ({ key: name, actorId: name, name, hue: 0, role: null, doingNow,
      lastRecordAt: null, lastSequence: 0, state, quietMinutes, shared: false, native: false, tasks: [] });
    const agents = [bot("gh-claude-5", "working", "merging #565", null), bot("gh-claude-1", "quiet", "#548 re-review", 300), bot("codex-gh", "done", "", 23)] as never;
    render(<MissionView journeys={journeys} tasks={[wt("a", { issue: 1, pr: 1 })] as unknown as TaskState[]} lanes={[]} now={0} runFor={() => null} frameUrl={() => null}
      onMarkSafe={vi.fn()} agents={agents} away={{ minutes: 840, shipped: 42 }} />);
    const list = screen.getByRole("region", { name: "Agents right now" });
    const rows = Array.from(list.querySelectorAll(".mv-agent")).map((r) => [r.querySelector(".mv-agent-dot")!.getAttribute("data-tone"), r.textContent]);
    expect(rows).toEqual([["work", "gh-claude-5merging #565"], ["stalled", "gh-claude-1#548 re-review · silent 300 min"], ["idle", "codex-ghidle 23 min"]]);
    expect(screen.getByText("14 h away · 42 shipped")).toBeInTheDocument();
  });

  it("#591: the inspector nudges the current step's owner lane through onSignal", async () => {
    const userEvent = fastUserEvent();
    const onSignal = vi.fn().mockResolvedValue(undefined);
    const tasks = [wt("a", { issue: 5, pr: 609, prTitle: "Two CLI", step: "implement", lane: "gh-claude-4" })] as unknown as TaskState[];
    render(<MissionView journeys={journeys} tasks={tasks} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} onSignal={onSignal} />);
    await userEvent.click(screen.getByRole("button", { name: "Nudge gh-claude-4" }));
    expect(onSignal).toHaveBeenCalledWith({ type: "operator_note", to: "gh-claude-4", description: "Owner asks: status of Implement on PR #609?" });
  });
});
