import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { LanesTimeline } from "./lanes-timeline";
import type { MissionTask } from "../runtime/mission";
import { fastUserEvent } from "../test/user-event";
import css from "./lanes-timeline.css?raw";
import { laneBars, type TimedTaskEvent } from "../runtime/lane-bars";

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
  it("counts every available status in exactly one filter", async () => {
    const H = 3_600_000, at = 10 * H;
    const rows = [
      { lane: "building", silent: true, lastEventAt: at - 5 * H, bars: [{ kind: "implement" as const, label: "#1", start: at - 5 * H, end: at, open: true }] },
      { lane: "stale", silent: true, lastEventAt: at - 5 * H, bars: [{ kind: "implement" as const, label: "issue-2", issue: 2, start: at - 5 * H, end: at, open: true }] },
      { lane: "working", silent: false, lastEventAt: at, bars: [{ kind: "implement" as const, label: "#3", start: at - H, end: at, open: true }] },
      { lane: "awaiting", silent: false, lastEventAt: at, awaiting: [4], bars: [] },
      { lane: "free", silent: false, lastEventAt: at, bars: [] },
    ];
    render(<LanesTimeline lanes={rows} now={at} windowMs={6 * H} slots={[{ root: "D:/gh", ok: true, holder: { lane: "building", label: "#1", pid: 1, worktree: null, heldSeconds: 60 }, waiting: [] }]} />);
    const user = fastUserEvent();
    const board = screen.getByRole("list", { name: "Agent board" });
    const filters = [["Working", ["building", "working"]], ["Waiting", ["awaiting"]], ["Free", ["free"]], ["Silent", ["stale"]]] as const;
    expect(screen.getByRole("button", { name: "Available 5" })).toBeInTheDocument();
    for (const [name, expected] of filters) {
      await user.click(screen.getByRole("button", { name: `${name} ${expected.length}` }));
      expect(Array.from(board.querySelectorAll(".ab-name")).map((row) => row.textContent)).toEqual(expected);
    }
  });

  it("shows an old-record slot holder as busy without hatching its open bar after the slot began", () => {
    const H = 3_600_000, at = 10 * H;
    const { container } = render(<LanesTimeline lanes={[{ lane: "builder", silent: true, lastEventAt: at - 5 * H, bars: [{
      kind: "implement", label: "#1", start: at - 5 * H, end: at, open: true, lastActivityAt: at - 5 * H,
    }] }]} now={at} windowMs={6 * H} slots={[{ root: "D:/gh", ok: true, holder: { lane: "builder", label: "#1", pid: 1, worktree: null, heldSeconds: 60 }, waiting: [] }]} />);
    const row = within(screen.getByRole("list", { name: "Agent lanes" })).getByText("builder").closest("li")!;
    expect(row.querySelector(".lt-dot")).toHaveAttribute("data-state", "busy");
    expect(row.querySelector(".lt-flag[data-flag='silent']")).toBeNull();
    expect(container.querySelector('[data-kind="stuck"]')).not.toHaveTextContent("builder");
    const hatch = container.querySelector<HTMLElement>(".lt-bar .lt-stall");
    expect(hatch).not.toBeNull();
    expect(parseFloat(hatch!.style.left) + parseFloat(hatch!.style.width)).toBeCloseTo(100 - 60_000 / (5 * H) * 100, 1);
  });
  it("renders the PR number and title in a bar, with the full label in its tooltip", () => {
    const events = [
      { sequence: 1, actorId: "dev", taskId: "issue-809", kind: "task.claimed", lane: "dev", issue: 809, title: "Old issue title", at: "2026-10-09T00:00:00Z" },
      { sequence: 2, actorId: "dev", taskId: "issue-809", kind: "task.pr_opened", lane: "dev", pr: 822, title: "Close false timeline bars", at: "2026-10-09T00:00:01Z" },
    ] as TimedTaskEvent[];
    const at = Date.parse("2026-10-09T00:00:10Z");
    const { container } = render(<LanesTimeline lanes={laneBars(events, at, 1000)} now={at} windowMs={1000} />);
    const bar = container.querySelector(".lt-bar")!;
    expect(bar).toHaveTextContent("#822 Close false timeline bars");
    expect(bar.getAttribute("title")).toContain("#822 Close false timeline bars");
    expect(css).toMatch(/\.lt-bar-label[^}]*text-overflow: ellipsis/);
  });

  it("hatches only the middle quiet span and counts it in the tooltip", () => {
    const hour = 3_600_000;
    const closed = { lane: "dev", silent: false, lastEventAt: now, bars: [{
      kind: "implement" as const, label: "#783", start: now - 14 * hour, end: now, open: false,
      lastActivityAt: now - 5 * 60_000,
      quietSpans: [{ start: now - 13 * hour, end: now - hour }],
    }] };
    const { container } = render(<LanesTimeline lanes={[closed]} now={now} windowMs={14 * hour} />);
    const bar = container.querySelector<HTMLElement>('.lt-bar[data-kind="implement"]')!;
    const hatch = bar.querySelector<HTMLElement>(".lt-stall")!;
    expect(bar.dataset.silent).toBe("true");
    expect(parseFloat(hatch.style.left)).toBeCloseTo(100 / 14, 1);
    expect(parseFloat(hatch.style.width)).toBeCloseTo(1200 / 14, 1);
    expect(bar.title).toContain("worked 2h");
    expect(bar.title).toContain("quiet 12h");
  });
  it("hatches a closed bar's quiet time and explains its end record", () => {
    const hour = 3_600_000;
    const closed = { lane: "dev", silent: false, lastEventAt: now, bars: [{
      kind: "implement" as const, label: "#548", start: now - 27 * hour, end: now, open: false,
      lastActivityAt: now - 27 * hour, endedBy: { kind: "released", actor: "supervisor" },
    }] };
    const { container } = render(<LanesTimeline lanes={[closed]} now={now} windowMs={28 * hour} />);
    const bar = container.querySelector<HTMLElement>('.lt-bar[data-kind="implement"]')!;
    expect(bar.dataset.silent).toBe("true");
    expect(parseFloat(bar.querySelector<HTMLElement>(".lt-stall")!.style.left)).toBeCloseTo(100 / 54);
    expect(bar.title).toContain("worked 30m");
    expect(bar.title).toContain("quiet 1d 2h");
    expect(bar.title).toContain("ended by released (supervisor)");
  });
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
    expect(row.querySelector('[data-silent="true"]')).toBeInTheDocument();
  });
  it("hatches only the stalled part of each open bar", () => {
    const hour = 3_600_000;
    const current = 4 * hour;
    const { container } = render(<LanesTimeline lanes={[{
      lane: "dev", silent: false, lastEventAt: current, bars: [
        { kind: "implement", label: "#1", start: 0, end: current, open: true, lastActivityAt: hour },
        { kind: "review", label: "#2", start: 2 * hour, end: current, open: true, lastActivityAt: current },
      ],
    }]} now={current} windowMs={current} />);
    const bars = container.querySelectorAll(".lt-bar");
    expect(bars[0]).toHaveAttribute("data-silent", "true");
    expect(bars[0]!.querySelector(".lt-stall")).toHaveStyle({ left: "37.5%" });
    expect(bars[1]).toHaveAttribute("data-silent", "false");
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
    expect(screen.getByRole("button", { name: "Available 2" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Working 0" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Free 1" }));
    expect(within(board).getAllByRole("listitem")).toHaveLength(1);
    expect(within(board).getByText("gh-claude-6")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Silent 1" }));
    expect(within(board).getByText("Silent")).toBeInTheDocument();
    expect(within(board).queryByText("gh-claude-6")).toBeNull();
  });

  it("offers Waiting rows directly and keeps Offline status in sync with the board", async () => {
    const user = fastUserEvent();
    const offlineLane = { ...lanes[1]!, lane: "old-idle", lastEventAt: now - 60 * 60_000 };
    render(<LanesTimeline lanes={[offlineLane]} now={now} windowMs={1000} tasks={[t({ pr: 4, lane: null, reviewers: ["reviewer"], currentReviewer: "reviewer", step: "review" })]} agents={[{ name: "reviewer", key: "reviewer", actorId: "reviewer", hue: 0, role: null, doingNow: "", lastRecordAt: new Date(now).toISOString(), lastSequence: 0, state: "working", quietMinutes: null, shared: false, native: false, tasks: [] }]} />);
    const timelineRows = within(screen.getByRole("list", { name: "Agent lanes" }));
    expect(within(timelineRows.getByText("old-idle").closest("li")!).getByText("offline")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Waiting 1" }));
    expect(within(screen.getByRole("list", { name: "Agent board" })).getByText("reviewer")).toBeInTheDocument();
  });

  it("Free count matches Free rows while old idle lanes stay collapsed under Offline", async () => {
    const user = fastUserEvent();
    render(<LanesTimeline lanes={[{ ...lanes[1]!, lastEventAt: now - 20 * 3_600_000 }]} now={now} windowMs={1000} onDemandLanes={["codex-14"]} />);
    const board = screen.getByRole("list", { name: "Agent board" });
    await user.click(screen.getByRole("button", { name: "Free 1" }));
    expect(within(board).getAllByRole("listitem")).toHaveLength(1);
    expect(within(board).getByText("codex-14")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Offline 1" }));
    expect(within(board).getByText("gh-claude-6")).toBeInTheDocument();
    expect(within(board).getByText(/last seen/)).toBeInTheDocument();
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

  it("#591 the board shows the latest record, and an author awaiting review is not Free", () => {
    const M = 60_000, T = 10 * 3_600_000;
    const author = { lane: "gh-claude-3", silent: false, lastEventAt: T - 12 * M, bars: [], awaiting: [613], lastRecord: { kind: "pr_opened", pr: 613, at: T - 12 * M } };
    render(<LanesTimeline lanes={[author]} now={T} windowMs={3_600_000} />);
    const row = within(screen.getByRole("list", { name: "Agent board" })).getByText("gh-claude-3").closest("li")!;
    expect(row.querySelector(".ab-for")).toHaveTextContent("pr_opened #613 · 12m ago");
    expect(row.querySelector(".ab-pill")).toHaveAttribute("data-status", "awaiting");
    expect(row.querySelector(".ab-pill")).toHaveTextContent("Awaiting review");
    expect(row.querySelector(".ab-tag")).toHaveTextContent("awaiting review #613");
    expect(document.querySelector('[data-kind="free"]')).toHaveTextContent("none right now");
  });

  it("agent row: one-line pill and rectangular chips that wrap only between parts, full name, labels unchanged", () => {
    const H = 3_600_000, T = 10 * H;
    const stale = { lane: "gh-claude-11", silent: false, lastEventAt: T - 3 * H, bars: [{ kind: "implement" as const, label: "issue 549", issue: 549, start: T - 3 * H, end: T, open: true }] };
    render(<LanesTimeline lanes={[stale]} now={T} windowMs={4 * H} />);
    const row = within(screen.getByRole("list", { name: "Agent board" })).getByText("gh-claude-11").closest("li")!;
    expect(row.querySelector(".ab-name")!.textContent).toBe("gh-claude-11");
    const pill = row.querySelector(".ab-pill")!, tag = row.querySelector(".ab-tag")!;
    expect(pill).toHaveTextContent("Stale claim");
    expect(tag.textContent).toMatch(/^stale claim · /);
    const rule = (sel: string) => css.split(/\r?\n/).find((l: string) => l.startsWith(`${sel} {`)) ?? "";
    expect(rule(".ab-pill")).toContain("white-space: nowrap");
    expect(rule(".ab-pill")).toContain("border-radius: 999px");
    expect(rule(".ab-for > span, .ab-tag > span")).toContain("white-space: nowrap");
    expect(rule(".ab-tag")).toContain("border-radius: 6px");
    expect(css).not.toMatch(/\.ab-[a-z-]+[^{]*\{[^}]*border-radius: 50%/);
  });

  it("keeps agent actions in a bounded mobile grid and full lane names readable", () => {
    const mobile = css.split("@media (max-width: 700px)").at(-1) ?? "";
    const rows = mobile.match(/\.ab-rows\s*\{([^}]*)\}/)?.[1] ?? "";
    const name = mobile.match(/\.lt-name\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(rows).not.toMatch(/(?:^|,)\s*max-content\s*(?=,|$)/);
    expect(rows).toMatch(/minmax\(0,\s*1fr\)/);
    expect(name).not.toMatch(/(?:overflow:\s*hidden|text-overflow:\s*ellipsis|white-space:\s*nowrap)/);
  });

  it("shows the build duration in the pill and the build location once", () => {
    render(<LanesTimeline lanes={[lanes[1]!]} now={now} windowMs={1000} slots={[
      { root: "D:/gh", ok: true, holder: { lane: "gh-claude-6", label: null, pid: 1, worktree: null, heldSeconds: 240 }, waiting: [] },
    ]} />);
    const row = within(screen.getByRole("list", { name: "Agent board" })).getByText("gh-claude-6").closest("li")!;
    expect(row.querySelector(".ab-pill")).toHaveTextContent("Building");
    expect(row.querySelector(".ab-pill")).toHaveTextContent("4m");
    expect(row.querySelector(".ab-tag")).toBeNull();
    expect(row.querySelector(".ab-doing")).toHaveTextContent("D:/gh");
    expect(row.querySelector(".ab-doing")).not.toHaveTextContent("building");
  });

  it("shows queued position and wait duration once with the build location", () => {
    render(<LanesTimeline lanes={[lanes[1]!]} now={now} windowMs={1000} slots={[{
      root: "D:/gh", ok: true, holder: { lane: "gh-claude-5", label: null, pid: 1, worktree: null, heldSeconds: 240 },
      waiting: [
        { lane: "gh-claude-5", label: null, pid: 3, worktree: null, priority: null, ticket: null, waitedSeconds: 180 },
        { lane: "gh-claude-6", label: null, pid: 2, worktree: null, priority: null, ticket: null, waitedSeconds: 120 },
      ],
    }]} />);
    const row = within(screen.getByRole("list", { name: "Agent board" })).getByText("gh-claude-6").closest("li")!;
    expect(row.querySelector(".ab-pill")).toHaveTextContent("Waiting for build");
    expect(row.querySelector(".ab-tag")).toHaveTextContent("2nd · 2m");
    expect(row.querySelectorAll(".ab-tag")).toHaveLength(1);
    expect(row.querySelector(".ab-doing")).toHaveTextContent("D:/gh");
    expect(row.querySelector(".ab-doing")).not.toHaveTextContent("Waiting for build");
    expect(row.textContent?.match(/Waiting for build/g)).toHaveLength(1);
  });
});
