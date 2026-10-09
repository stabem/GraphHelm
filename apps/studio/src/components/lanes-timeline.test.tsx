import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { LanesTimeline } from "./lanes-timeline";
import type { MissionTask } from "../runtime/mission";
import { fastUserEvent } from "../test/user-event";

const now = 1_000_000;
const lanes = [
  { lane: "gh-claude-5", silent: true, lastEventAt: 0, bars: [{ kind: "review" as const, label: "#548", start: now - 500, end: now, open: true }] },
  { lane: "gh-claude-6", silent: false, lastEventAt: now, bars: [{ kind: "implement" as const, label: "#559", start: now - 1000, end: now - 750, open: false }] },
];
const t = (over: Partial<MissionTask>): MissionTask => ({
  key: "k", pr: 1, issue: null, title: "t", lane: "gh-claude-6", reviewers: [], step: "implement", blocked: false, trust: 1,
  blockedBy: null, rounds: [], headSha: null, mergeSha: null, repoUrl: null, ...over,
});
const round = { reviewer: "r", headSha: "h", fixHead: "f" };

describe("LanesTimeline", () => {
  it("places bars on the window", () => {
    render(<LanesTimeline lanes={lanes} now={now} windowMs={1000} />);
    const bar = screen.getByText("#559 implement").closest("[data-kind]") as HTMLElement;
    expect(bar.style.left).toBe("0%");
    expect(bar.style.width).toBe("25%");
  });

  it("flags a silent lane in text, not only colour", () => {
    render(<LanesTimeline lanes={lanes} now={now} windowMs={1000} />);
    const row = within(screen.getByRole("list", { name: "Agent lanes" })).getByText("gh-claude-5").closest("li")!;
    expect(within(row).getByText("silent")).toBeInTheDocument();
    expect(row.querySelector('[data-silent="true"]')).not.toBeNull();
  });

  it("shows the last merged PR per lane, else a dash", () => {
    render(<LanesTimeline lanes={lanes} now={now} windowMs={1000} tasks={[t({ pr: 559, title: "Watch streams", step: "merged" })]} />);
    expect(within(within(screen.getByRole("list", { name: "Agent lanes" })).getByText("gh-claude-6").closest("li")!).getByText("#559 Watch streams")).toBeInTheDocument();
    expect(within(within(screen.getByRole("list", { name: "Agent lanes" })).getByText("gh-claude-5").closest("li")!).getByText("—")).toBeInTheDocument();
  });

  it("stuck, ping-pong and free hands come from the data", () => {
    const { container } = render(<LanesTimeline lanes={lanes} now={now} windowMs={1000}
      tasks={[t({ pr: 560, rounds: [round, round] }), t({ pr: 561, rounds: [round] })]} />);
    expect(container.querySelector('[data-kind="stuck"]')).toHaveTextContent("STUCKgh-claude-5");
    expect(container.querySelector('[data-kind="pingpong"]')).toHaveTextContent("PING-PONG#560 · 2 BLOCKs");
    expect(container.querySelector('[data-kind="free"]')).toHaveTextContent("FREE HANDSgh-claude-6");
  });

  it("a card with nothing reads none right now", () => {
    const { container } = render(<LanesTimeline lanes={[lanes[1]!]} now={now} windowMs={1000} />);
    expect(container.querySelector('[data-kind="stuck"]')).toHaveTextContent("none right now");
    expect(container.querySelector('[data-kind="pingpong"]')).toHaveTextContent("none right now");
  });

  it("draws the time axis from the window back to now", () => {
    render(<LanesTimeline lanes={lanes} now={now} windowMs={14 * 3_600_000} />);
    expect(screen.getByText("−14h")).toBeInTheDocument();
    expect(screen.getByText("−2h")).toBeInTheDocument();
    expect(screen.getByText("now")).toBeInTheDocument();
  });

  it("shows an empty state", () => {
    render(<LanesTimeline lanes={[]} now={now} windowMs={1000} />);
    expect(screen.getByText("No agent has recorded work in this window")).toBeInTheDocument();
  });

  it("the agent board filters by status with counts", async () => {
    const user = fastUserEvent();
    render(<LanesTimeline lanes={lanes} now={now} windowMs={1000} />);
    const board = screen.getByRole("list", { name: "Agent board" });
    expect(within(board).getAllByRole("listitem").map((li) => li.querySelector(".ab-name")!.textContent)).toEqual(["gh-claude-5", "gh-claude-6"]);
    expect(screen.getByRole("button", { name: "All 2" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Working 0" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Free 1" }));
    expect(within(board).getAllByRole("listitem")).toHaveLength(1);
    expect(within(board).getByText("gh-claude-6")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Silent 1" }));
    expect(within(board).getByText("Silent")).toBeInTheDocument();
    expect(within(board).queryByText("gh-claude-6")).toBeNull();
  });

  it("timeline rows follow the board order", () => {
    render(<LanesTimeline lanes={[lanes[1]!, lanes[0]!]} now={now} windowMs={1000} />);
    const rows = within(screen.getByRole("list", { name: "Agent lanes" })).getAllByRole("listitem");
    expect(rows.map((r) => r.querySelector(".lt-name")!.textContent)).toEqual(["gh-claude-5", "gh-claude-6"]);
  });

  it("#591: overlapping bars stack in sub-rows; narrow bars carry no text; every bar has a tooltip; TBD lanes drop", () => {
    const busy = { lane: "gh-claude-6", silent: false, lastEventAt: now, bars: [
      { kind: "implement" as const, label: "#600", start: now - 1000, end: now, open: true },
      { kind: "review" as const, label: "#601", start: now - 800, end: now - 400, open: false },
      { kind: "review" as const, label: "#602", start: now - 300, end: now - 290, open: false },
    ] };
    const tbd = { lane: "TBD", silent: false, lastEventAt: now, bars: [{ kind: "implement" as const, label: "#9", start: now - 10, end: now, open: true }] };
    const blank = { ...tbd, lane: "" };
    render(<LanesTimeline lanes={[busy, tbd, blank]} now={now} windowMs={1000} agents={[{ name: "TBD" } as never]} />);
    const list = screen.getByRole("list", { name: "Agent lanes" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(within(screen.getByRole("list", { name: "Agent board" })).queryByText("TBD")).toBeNull();
    const track = list.querySelector(".lt-track") as HTMLElement;
    expect(track).toHaveAttribute("data-subrows", "2");
    expect(track.style.height).toBe(`${2 * 18 + 2 + 6}px`);
    const bars = Array.from(track.querySelectorAll<HTMLElement>(".lt-bar"));
    expect(bars.map((b) => b.dataset.subrow)).toEqual(["0", "1", "1"]);
    expect(bars.map((b) => b.style.top)).toEqual(["3px", "23px", "23px"]);
    expect(bars[0]).toHaveTextContent("#600 implement");
    expect(bars[2]!.textContent).toBe("");
    expect(bars[2]).toHaveAttribute("title", "#602 review · 0s");
    expect(bars[0]!.getAttribute("title")).toBe("#600 implement · 1s so far");
  });
});
