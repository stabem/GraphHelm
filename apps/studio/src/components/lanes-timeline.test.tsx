import reviewActivityRoute from "../../../../core/execution/tests/fixtures/tasks-review-activity-route.json";
import { describe, expect, it } from "vitest";
import { TEST_FREE_MS, TEST_SILENT_MS, laneBarsOf } from "../test/task-fold";

// #1234: hand-built lanes carry the budgets the Runtime would serve with them.
const B = { silentAfterMs: TEST_SILENT_MS, freeAfterMs: TEST_FREE_MS };
import { render, screen, within } from "@testing-library/react";
import { LanesTimeline } from "./lanes-timeline";
import type { MissionTask } from "../runtime/mission";
import { fastUserEvent } from "../test/user-event";
import css from "./lanes-timeline.css?raw";
import { laneBars, type TimedTaskEvent } from "../runtime/lane-bars";
import { tasksReply } from "../runtime/tasks-route";
import { toMissionTask } from "../runtime/mission";
import liveTasks from "../../../../core/execution/tests/fixtures/tasks-live-route.json";
import { teamModel } from "../runtime/team";
import type { RuntimeEvent } from "../runtime/types";

const now = 1_000_000;
const lanes = [
  { ...B, lane: "gh-claude-5", silent: true, lastEventAt: 0, bars: [{ kind: "review" as const, label: "#548", start: now - 500, end: now, open: true }] },
  { ...B, lane: "gh-claude-6", silent: false, lastEventAt: now, bars: [{ kind: "implement" as const, label: "#559", start: now - 1000, end: now - 750, open: false }] },
];
const t = (over: Partial<MissionTask>): MissionTask => ({
  key: "k", pr: 1, issue: null, title: "t", lane: "gh-claude-6", reviewers: [], step: "implement", blocked: false, trust: 1,
  blockedBy: null, rounds: [], headSha: null, mergeSha: null, repoUrl: null, ...over,
});
const round = { reviewer: "r", headSha: "h", fixHead: "f", blockedAt: null };

describe("LanesTimeline", () => {
  it("shows recent note-only actors in the timeline without inventing task work", () => {
    const at = Date.parse("2026-10-14T12:00:00Z");
    const events: RuntimeEvent[] = ["codex-one", "claude-two"].map((actorId, index) => ({
      sequence: index + 1, kind: "signal_recorded", payload: { kind: "operator_note", signalId: `note-${index + 1}` },
      occurredAt: new Date(at - (index + 2) * 60_000).toISOString(), actorId, actorType: "agent",
      idempotencyKey: null, eventId: `event-${index + 1}`, evidenceRefs: [],
    }));
    const agents = teamModel({ events, envelopes: {}, personas: {}, nativeLinks: {}, aliases: {}, model: null,
      claudeTasks: null, waitingAskers: new Set(), now: at }).bots;

    render(<LanesTimeline lanes={laneBarsOf([], at, 3_600_000)} freeAfterMs={TEST_FREE_MS} agents={agents} now={at} windowMs={3_600_000} />);
    const timeline = within(screen.getByRole("list", { name: "Agent lanes" }));
    for (const [name, age] of [["codex-one", "2m ago"], ["claude-two", "3m ago"]]) {
      const row = timeline.getByText(name).closest("li")!;
      expect(row).toHaveTextContent("No task work recorded");
      expect(row).toHaveTextContent(`Last record ${age}`);
      expect(row.querySelectorAll(".lt-bar")).toHaveLength(0);
    }
    expect(timeline.getAllByText(/No task work recorded/)).toHaveLength(2);
  });

  it("shows an author awaiting review as waiting in every Lanes status view", async () => {
    const at = 10 * 3_600_000;
    const events = [
      { sequence: 1, actorId: "codex-12", taskId: "issue-945", kind: "task.claimed", lane: "codex-12", issue: 945, at: new Date(at - 1000).toISOString() },
      { sequence: 2, actorId: "codex-12", taskId: "issue-945", kind: "task.pr_opened", lane: "codex-12", pr: 923, headSha: "head1", at: new Date(at).toISOString() },
    ] as TimedTaskEvent[];
    const { container } = render(<LanesTimeline lanes={laneBarsOf(events, at, 3_600_000)} now={at} windowMs={3_600_000} />);
    const board = screen.getByRole("list", { name: "Agent board" });
    await fastUserEvent().click(screen.getByRole("button", { name: "Waiting 1" }));
    expect(within(board).getByText("codex-12").closest("li")!.querySelector(".ab-pill")).toHaveAttribute("data-status", "waiting");
    const timeline = within(screen.getByRole("list", { name: "Agent lanes" })).getByText("codex-12").closest("li")!;
    expect(timeline.querySelector('.lt-flag[data-flag="waiting"]')).toHaveTextContent("waiting");
    expect(timeline.querySelector('.lt-dot')).toHaveAttribute("data-state", "waiting");
    expect(screen.getByRole("button", { name: "Waiting 1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Free 0" })).toBeInTheDocument();
    expect(container.querySelector('[data-kind="free"]')).toHaveTextContent("FREE HANDSnone right now");
  });

  it("Available count matches free rows while other statuses stay selectable", async () => {
    const H = 3_600_000, at = 10 * H;
    const rows = [
      { ...B, lane: "building", silent: true, lastEventAt: at - 5 * H, bars: [{ kind: "implement" as const, label: "#1", start: at - 5 * H, end: at, open: true }] },
      { ...B, lane: "stale", silent: true, lastEventAt: at - 5 * H, overdueTasks: ["issue-2"], bars: [{ kind: "implement" as const, label: "issue-2", taskId: "issue-2", issue: 2, start: at - 5 * H, end: at, open: true }] },
      { ...B, lane: "working", silent: false, lastEventAt: at, bars: [{ kind: "implement" as const, label: "#3", start: at - H, end: at, open: true }] },
      { ...B, lane: "awaiting", silent: false, lastEventAt: at, awaiting: [4], bars: [] },
      { ...B, lane: "free", silent: false, lastEventAt: at, bars: [] },
    ];
    render(<LanesTimeline lanes={rows} now={at} windowMs={6 * H} slots={[{ root: "D:/gh", ok: true, holder: { lane: "building", label: "#1", pid: 1, worktree: null, heldSeconds: 60 }, waiting: [] }]} />);
    const user = fastUserEvent();
    const board = screen.getByRole("list", { name: "Agent board" });
    const filters = [["Working", ["building", "working"]], ["Waiting", ["awaiting"]], ["Free", ["free"]], ["Stale claim", ["stale"]]] as const;
    expect(screen.getByRole("button", { name: "Available 1" })).toBeInTheDocument();
    for (const [name, expected] of filters) {
      await user.click(screen.getByRole("button", { name: `${name} ${expected.length}` }));
      expect(Array.from(board.querySelectorAll(".ab-name")).map((row) => row.textContent)).toEqual(expected);
    }
  });

  it("counts Silent only for Silent rows and labels the all row count Available", async () => {
    const user = fastUserEvent();
    const H = 3_600_000, at = 10 * H;
    const rows = [
      { ...B, lane: "working", silent: false, lastEventAt: at, bars: [{ kind: "implement" as const, label: "#1", start: at - H, end: at, open: true }] },
      { ...B, lane: "stale", silent: false, lastEventAt: at - 3 * H, overdueTasks: ["issue-2"], bars: [{ kind: "implement" as const, label: "issue 2", taskId: "issue-2", start: at - 3 * H, end: at, open: true }] },
    ];
    render(<LanesTimeline lanes={rows} now={at} windowMs={4 * H} />);
    const board = screen.getByRole("list", { name: "Agent board" });
    expect(screen.getByRole("button", { name: "Available 0" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Silent 0" }));
    expect(within(board).queryByText("stale")).toBeNull();
    expect(screen.getByRole("button", { name: "Stale claim 1" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Stale claim 1" }));
    expect(within(board).getByText("stale")).toBeInTheDocument();
  });

  it("shows an old-record slot holder as busy without hatching its open bar after the slot began", () => {
    const H = 3_600_000, at = 10 * H;
    const { container } = render(<LanesTimeline lanes={[{ ...B, lane: "builder", silent: true, lastEventAt: at - 5 * H, bars: [{
      kind: "implement", label: "#1", start: at - 5 * H, end: at, open: true, lastActivityAt: at - 5 * H,
    }] }]} now={at} windowMs={6 * H} slots={[{ root: "D:/gh", ok: true, holder: { lane: "builder", label: "#1", pid: 1, worktree: null, heldSeconds: 60 }, waiting: [] }]} />);
    const row = within(screen.getByRole("list", { name: "Agent lanes" })).getByText("builder").closest("li")!;
    expect(row.querySelector(".lt-dot")).toHaveAttribute("data-state", "building");
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
    const { container } = render(<LanesTimeline lanes={laneBarsOf(events, at, 1000)} now={at} windowMs={1000} />);
    const bar = container.querySelector(".lt-bar")!;
    expect(bar).toHaveTextContent("#822 Close false timeline bars");
    expect(bar.getAttribute("title")).toContain("#822 Close false timeline bars");
    expect(css).toMatch(/\.lt-bar-label[^}]*text-overflow: ellipsis/);
  });

  it("hatches only the middle quiet span and counts it in the tooltip", () => {
    const hour = 3_600_000;
    const closed = { ...B, lane: "dev", silent: false, lastEventAt: now, bars: [{
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
    const closed = { ...B, lane: "dev", silent: false, lastEventAt: now, bars: [{
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
      ...B, lane: "dev", silent: false, lastEventAt: current, bars: [
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

  it("shows current ping-pong first and keeps merged PRs only for recent BLOCKs", () => {
    const H = 3_600_000, at = Date.parse("2026-10-12T12:00:00Z");
    const roundAt = (hoursAgo: number) => ({ ...round, blockedAt: new Date(at - hoursAgo * H).toISOString() });
    const { container } = render(<LanesTimeline lanes={lanes} now={at} windowMs={14 * H} tasks={[
      t({ pr: 554, step: "merged", rounds: [roundAt(90), roundAt(80), roundAt(72)] }),
      t({ pr: 923, step: "review", rounds: [roundAt(3), roundAt(1)] }),
      t({ pr: 924, step: "merged", rounds: [roundAt(2), roundAt(1)] }),
    ]} />);
    const pingPong = container.querySelector('[data-kind="pingpong"]')!;
    expect(pingPong).toHaveTextContent("#923 · 2 BLOCKs, #924 · 2 BLOCKs");
    expect(pingPong).not.toHaveTextContent("#554");
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
    expect(within(board).getAllByRole("listitem").map((li) => li.querySelector(".ab-name")!.textContent)).toEqual(["gh-claude-6"]);
    expect(screen.getByRole("button", { name: "Available 1" })).toHaveAttribute("aria-pressed", "true");
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
  it("counts live idle lanes as Free and names the source", async () => {
    const user = fastUserEvent();
    const old = ["gh-claude-6", "coordinator"].map((name) => ({ ...lanes[1]!, lane: name, lastEventAt: now - 10 * 3_600_000 }));
    render(<LanesTimeline lanes={old} now={now} windowMs={1000} liveLanes={["gh-claude-6", "coordinator"]} />);
    await user.click(screen.getByRole("button", { name: "Free 2" }));
    const board = screen.getByRole("list", { name: "Agent board" });
    expect(within(board).getAllByRole("listitem")).toHaveLength(2);
    expect(within(board).getAllByText(/wake listener/)).toHaveLength(2);
  });

  it("timeline rows follow the board order", () => {
    render(<LanesTimeline lanes={[lanes[1]!, lanes[0]!]} now={now} windowMs={1000} />);
    const rows = within(screen.getByRole("list", { name: "Agent lanes" })).getAllByRole("listitem");
    expect(rows.map((r) => r.querySelector(".lt-name")!.textContent)).toEqual(["gh-claude-5", "gh-claude-6"]);
  });

  it("#591: overlapping bars stack in sub-rows; narrow bars carry no text; every bar has a tooltip; TBD lanes drop", () => {
    const busy = { ...B, lane: "gh-claude-6", silent: false, lastEventAt: now, bars: [
      { kind: "implement" as const, label: "#600", start: now - 1000, end: now, open: true },
      { kind: "review" as const, label: "#601", start: now - 800, end: now - 400, open: false },
      { kind: "review" as const, label: "#602", start: now - 300, end: now - 290, open: false },
    ] };
    const tbd = { ...B, lane: "TBD", silent: false, lastEventAt: now, bars: [{ kind: "implement" as const, label: "#9", start: now - 10, end: now, open: true }] };
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

  it("#591 the board shows the latest record, and an author awaiting review is not Free", async () => {
    const M = 60_000, T = 10 * 3_600_000;
    const author = { ...B, lane: "gh-claude-3", silent: false, lastEventAt: T - 12 * M, bars: [], awaiting: [613], lastRecord: { kind: "pr_opened", pr: 613, at: T - 12 * M } };
    render(<LanesTimeline lanes={[author]} now={T} windowMs={3_600_000} />);
    await fastUserEvent().click(screen.getByRole("button", { name: "Waiting 1" }));
    const row = within(screen.getByRole("list", { name: "Agent board" })).getByText("gh-claude-3").closest("li")!;
    expect(row.querySelector(".ab-for")).toHaveTextContent("pr_opened #613 · 12m ago");
    expect(row.querySelector(".ab-pill")).toHaveAttribute("data-status", "waiting");
    expect(row.querySelector(".ab-pill")).toHaveTextContent("Waiting");
    expect(row.querySelector(".ab-tag")).toHaveTextContent("awaiting review #613");
    expect(document.querySelector('[data-kind="free"]')).toHaveTextContent("none right now");
  });

  it("agent row: one-line pill and rectangular chips that wrap only between parts, full name, labels unchanged", async () => {
    const H = 3_600_000, T = 10 * H;
    const stale = { ...B, lane: "gh-claude-11", silent: false, lastEventAt: T - 3 * H, overdueTasks: ["issue-549"], bars: [{ kind: "implement" as const, label: "issue 549", taskId: "issue-549", issue: 549, start: T - 3 * H, end: T, open: true }] };
    render(<LanesTimeline lanes={[stale]} now={T} windowMs={4 * H} />);
    await fastUserEvent().click(screen.getByRole("button", { name: "Stale claim 1" }));
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
    const row = mobile.match(/\.ab-row\s*\{([^}]*)\}/)?.[1] ?? "";
    const name = mobile.match(/\.lt-name\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(rows).toMatch(/grid-template-columns:\s*minmax\(0,\s*1fr\)/);
    expect(row).toMatch(/grid-template-columns:\s*max-content\s+minmax\(0,\s*1fr\)/);
    expect(mobile).toMatch(/\.ab-name\s*\{[^}]*grid-row:\s*1[^}]*grid-column:\s*1\s*\/\s*-1/);
    expect(mobile).toMatch(/\.ab-acts\s*\{[^}]*grid-row:\s*3[^}]*grid-column:\s*1\s*\/\s*-1/);
    expect(name).not.toMatch(/(?:overflow:\s*hidden|text-overflow:\s*ellipsis|white-space:\s*nowrap)/);
  });

  it("shows the build duration in the pill and the build location once", async () => {
    render(<LanesTimeline lanes={[lanes[1]!]} now={now} windowMs={1000} slots={[
      { root: "D:/gh", ok: true, holder: { lane: "gh-claude-6", label: null, pid: 1, worktree: null, heldSeconds: 240 }, waiting: [] },
    ]} />);
    await fastUserEvent().click(screen.getByRole("button", { name: "Working 1" }));
    const row = within(screen.getByRole("list", { name: "Agent board" })).getByText("gh-claude-6").closest("li")!;
    expect(row.querySelector(".ab-pill")).toHaveTextContent("Building");
    expect(row.querySelector(".ab-pill")).toHaveTextContent("4m");
    expect(row.querySelector(".ab-tag")).toBeNull();
    expect(row.querySelector(".ab-doing")).toHaveTextContent("D:/gh");
    expect(row.querySelector(".ab-doing")).not.toHaveTextContent("building");
  });

  it("#1556: a card shows the step and the slot command beside the root", async () => {
    render(<LanesTimeline lanes={[lanes[1]!]} now={now} windowMs={1000} slots={[
      { root: "D:/gh/c", ok: true, holder: { lane: "gh-claude-6", label: "cargo test -p studio", pid: 1, worktree: null, heldSeconds: 240 }, waiting: [] },
    ]} />);
    await fastUserEvent().click(screen.getByRole("button", { name: "Working 1" }));
    const row = within(screen.getByRole("list", { name: "Agent board" })).getByText("gh-claude-6").closest("li")!;
    expect(row.querySelector(".ab-step")).toHaveTextContent("proving");
    expect(row.querySelector(".ab-cmd")).toHaveTextContent("cargo test -p studio");
    expect(row.querySelector(".ab-doing")).toHaveTextContent("D:/gh/c");
  });

  it("#1556: a card is named by what its lane is doing, without its running clock", async () => {
    render(<LanesTimeline lanes={[lanes[1]!]} now={now} windowMs={1000} slots={[
      { root: "D:/gh/c", ok: true, holder: { lane: "gh-claude-6", label: "cargo test -p studio", pid: 1, worktree: null, heldSeconds: 240 }, waiting: [] },
    ]} />);
    await fastUserEvent().click(screen.getByRole("button", { name: "Working 1" }));
    // The journey flow (studio-lanes-board) addresses the card by this name; the pill's "4m" is
    // left out, so the name only changes when what the lane is doing changes.
    expect(screen.getByRole("listitem", { name: "gh-claude-6 · Building · proving · cargo test -p studio" })).toBeInTheDocument();
  });

  it("shows queued position and wait duration once with the build location", async () => {
    render(<LanesTimeline lanes={[lanes[1]!]} now={now} windowMs={1000} slots={[{
      root: "D:/gh", ok: true, holder: { lane: "gh-claude-5", label: null, pid: 1, worktree: null, heldSeconds: 240 },
      waiting: [
        { lane: "gh-claude-5", label: null, pid: 3, worktree: null, priority: null, ticket: null, waitedSeconds: 180 },
        { lane: "gh-claude-6", label: null, pid: 2, worktree: null, priority: null, ticket: null, waitedSeconds: 120 },
      ],
    }]} />);
    await fastUserEvent().click(screen.getByRole("button", { name: "Waiting 1" }));
    const row = within(screen.getByRole("list", { name: "Agent board" })).getByText("gh-claude-6").closest("li")!;
    expect(row.querySelector(".ab-pill")).toHaveTextContent("Waiting for build");
    expect(row.querySelector(".ab-tag")).toHaveTextContent("2nd · 2m");
    expect(row.querySelectorAll(".ab-tag")).toHaveLength(1);
    expect(row.querySelector(".ab-doing")).toHaveTextContent("D:/gh");
    expect(row.querySelector(".ab-doing")).not.toHaveTextContent("Waiting for build");
    expect(row.textContent?.match(/Waiting for build/g)).toHaveLength(1);
  });
});

// Runtime-produced approved merge slice; liveness cells below are controlled scenario inputs,
// not verdicts inferred from bars. The same fixture helper is used in work-kanban.test.tsx.
function pendingMergeProjection(options: { reviewerSilent?: boolean; authorSilent?: boolean } = {}) {
  const projection = tasksReply(liveTasks)!;
  const source = projection.slices.find((slice) => slice.state.pr === 1013)!;
  const task = structuredClone(source.state);
  const routes = projection.routes.filter((route) => route.slice === source.id);
  const sequences = new Set(routes.map((route) => route.sequence));
  const records = projection.records.filter((record) => sequences.has(record.sequence))
    .map((record) => ({ ...record, at: record.occurredAt! }));
  const at = Date.parse(task.clock.lastAt!) + 60_000;
  const liveness = {
    budgets: { silentAfterSeconds: 1800, freeAfterSeconds: 900, stallAfterSeconds: 7200 },
    lanes: [
      { lane: task.lane!, silent: options.authorSilent ?? false },
      { lane: task.currentReviewer!, silent: options.reviewerSilent ?? false },
    ].map((row) => ({ ...row, owes: true, free: false,
      lastRecordAt: new Date(at - (row.silent ? 2 * 3_600_000 : 60_000)).toISOString(),
      owesSince: new Date(at - 2 * 3_600_000).toISOString() })),
  };
  return { at, task, records, fold: { slices: [{ ...source, state: task }], routes, liveness } };
}

describe("pending merge reviewer obligation", () => {
  it.each([
    { reviewerSilent: false, status: "Waiting" },
    { reviewerSilent: true, status: "Silent" },
  ])("keeps the closed-review reviewer $status until merge, not available or working", async ({ reviewerSilent, status }) => {
    const { at, task, records, fold } = pendingMergeProjection({ reviewerSilent });
    const projected = laneBars(records, fold, at, 4 * 3_600_000);
    const reviewer = task.currentReviewer!;
    expect(projected.find((lane) => lane.lane === reviewer)!.bars.filter((bar) => bar.kind === "review")
      .every((bar) => !bar.open)).toBe(true);
    const props = { now: at, windowMs: 4 * 3_600_000, seenActors: [reviewer] };
    const view = render(<LanesTimeline {...props} lanes={projected} tasks={[toMissionTask(task)]} />);
    const board = screen.getByRole("list", { name: "Agent board" });
    expect(within(board).queryByText(reviewer)).toBeNull();
    expect(screen.getByRole("button", { name: "Working 0" })).toBeInTheDocument();
    await fastUserEvent().click(screen.getByRole("button", { name: `${status} 1` }));
    const row = within(board).getByRole("listitem", { name: `${reviewer} · ${status} · merge` });
    expect(within(row).getByText(status, { selector: ".ab-pill" })).toBeInTheDocument();
    expect(within(row).getByRole("link", { name: /awaiting merge.*#1013/i }))
      .toHaveAttribute("href", "https://github.com/stabem/GraphHelm/pull/1013");

    const merged = { ...task, step: "merged" as const, mergeSha: "abcdef0123456789" };
    const mergeRecord: TimedTaskEvent = { sequence: task.lastSequence + 1, taskId: task.taskId,
      kind: "task.merged", actorId: reviewer, reviewer, pr: task.pr!, mergeSha: merged.mergeSha,
      at: new Date(at).toISOString() };
    const mergedFold = { ...fold, slices: [{ ...fold.slices[0]!, state: merged }],
      routes: [...fold.routes, { sequence: mergeRecord.sequence, slice: fold.slices[0]!.id }] };
    view.rerender(<LanesTimeline {...props} tasks={[toMissionTask(merged)]}
      lanes={laneBars([...records, mergeRecord], mergedFold, at, props.windowMs)} />);
    expect(within(board).queryByText(reviewer)).toBeNull();
    expect(within(board).queryByRole("link", { name: /awaiting merge/i })).toBeNull();
  });
});

it("retains explicit null versus legacy pending-merge assignment in the rendered board", async () => {
  const { at, task, records, fold } = pendingMergeProjection();
  const reviewer = task.currentReviewer!;
  task.currentReviewer = null;
  const props = { now: at, windowMs: 4 * 3_600_000, seenActors: [reviewer] };
  const view = render(<LanesTimeline {...props} lanes={laneBars(records, fold, at, props.windowMs)} tasks={[toMissionTask(task)]} />);
  const board = screen.getByRole("list", { name: "Agent board" });
  const available = within(board).getByText(reviewer).closest("li")!;
  expect(available).toHaveAttribute("aria-label", `${reviewer} · Free`);
  expect(within(available).queryByRole("link")).toBeNull();
  delete task.currentReviewer;
  view.rerender(<LanesTimeline {...props} lanes={laneBars(records, fold, at, props.windowMs)} tasks={[toMissionTask(task)]} />);
  expect(within(board).queryByText(reviewer)).toBeNull();
  await fastUserEvent().click(screen.getByRole("button", { name: "Waiting 1" }));
  expect(within(board).getByRole("listitem", { name: `${reviewer} · Waiting · merge` })).toBeInTheDocument();
});

it("keeps real implementation and build-slot precedence over the pending merge obligation", async () => {
  const { at, task, records, fold } = pendingMergeProjection({ reviewerSilent: true });
  const reviewer = task.currentReviewer!;
  const work = { ...structuredClone(task), key: "issue-2000#claim-0", taskId: "issue-2000", issue: 2000, pr: null,
    lane: reviewer, step: "implement" as const, prTitle: "Actual implementation", reviewers: [], currentReviewer: null };
  const next = task.lastSequence + 1, slice = 9999;
  const activeRecords: TimedTaskEvent[] = [
    { sequence: next, kind: "task.claimed", taskId: work.taskId, actorId: reviewer, lane: reviewer, issue: 2000, at: new Date(at - 20_000).toISOString() },
    { sequence: next + 1, kind: "task.planned", taskId: work.taskId, actorId: reviewer, lane: reviewer, at: new Date(at - 10_000).toISOString() },
  ];
  const activeFold = { ...fold, slices: [...fold.slices, { id: slice, ended: false, absorbed: false, state: work }],
    routes: [...fold.routes, ...activeRecords.map((record) => ({ sequence: record.sequence, slice }))],
    liveness: { ...fold.liveness, lanes: fold.liveness.lanes.map((row) => row.lane === reviewer ? { ...row, silent: false } : row) } };
  const props = { now: at, windowMs: 4 * 3_600_000, tasks: [task, work].map(toMissionTask) };
  const view = render(<LanesTimeline {...props} lanes={laneBars([...records, ...activeRecords], activeFold, at, props.windowMs)} />);
  await fastUserEvent().click(screen.getByRole("button", { name: "Working 1" }));
  const board = screen.getByRole("list", { name: "Agent board" });
  expect(within(board).getByRole("listitem", { name: `${reviewer} · Working · claimed` }))
    .not.toHaveTextContent("awaiting merge");
  const slots = [{ root: "D:/gh", ok: true as const, holder: { lane: reviewer, label: "#1013", pid: 1, worktree: null, heldSeconds: 10 }, waiting: [] }];
  view.rerender(<LanesTimeline {...props} slots={slots} lanes={laneBars(records, fold, at, props.windowMs)} />);
  expect(within(board).getByRole("listitem", { name: `${reviewer} · Building · proving · #1013` })).toBeInTheDocument();
  view.rerender(<LanesTimeline {...props} slots={[{ ...slots[0]!, holder: null, waiting: [{ lane: reviewer, label: "#1013", pid: 2, worktree: null, priority: null, ticket: null, waitedSeconds: 10 }] }]}
    lanes={laneBars(records, fold, at, props.windowMs)} />);
  await fastUserEvent().click(screen.getByRole("button", { name: "Waiting 1" }));
  expect(within(board).getByRole("listitem", { name: `${reviewer} · Waiting for build · proving · #1013` })).toBeInTheDocument();
});


it("renders the real reviewer wait projection and expires its reason at the served boundary", async () => {
  // Advancing frontend time expires text, not the captured Runtime liveness verdict.
  const projection = tasksReply(reviewActivityRoute)!;
  expect(projection).not.toBeNull();
  const task = projection.tasks[0]!;
  const source = reviewActivityRoute.tasks[0]!;
  const activity = source.reviewActivity;
  expect(projection.routes.find((route) => route.sequence === activity.sequence)?.slice).toBe(projection.slices[0]!.id);
  expect(projection.records.find((record) => record.sequence === activity.sequence)?.kind).toBe("task.review_activity");
  expect(task.step).toBe("merge");
  expect(task.lane).toBe("review-author");
  expect(task.currentReviewer).toBe("review-reader");
  expect(source.reviewContextSequence).toBe(activity.reviewContextSequence);
  expect(source.reviewActivityRevision).toBe(activity.revision);
  const at = Date.parse(activity.occurredAt);
  const records = projection.records.map((record) => ({ ...record, at: record.occurredAt! }));
  const props = { windowMs: 4 * 3_600_000, seenActors: [task.currentReviewer!], tasks: [toMissionTask(task)] };
  const view = render(<LanesTimeline {...props} now={at + 1} lanes={laneBars(records, projection, at + 1, props.windowMs)} />);
  expect(screen.getByRole("button", { name: "Working 0" })).toBeInTheDocument();
  await fastUserEvent().click(screen.getByRole("button", { name: "Waiting 1" }));
  const row = within(screen.getByRole("list", { name: "Agent board" })).getByRole("listitem", { name: "review-reader · Waiting · merge" });
  expect(row).toHaveTextContent(`Waiting: ${activity.reason}`);
  expect(row).toHaveTextContent("#17");
  const boundary = at + projection.liveness!.budgets.silentAfterSeconds * 1000;
  view.rerender(<LanesTimeline {...props} now={boundary - 1} lanes={laneBars(records, projection, boundary - 1, props.windowMs)} />);
  expect(row).toHaveTextContent(`Waiting: ${activity.reason}`);
  view.rerender(<LanesTimeline {...props} now={boundary} lanes={laneBars(records, projection, boundary, props.windowMs)} />);
  expect(row).not.toHaveTextContent(`Waiting: ${activity.reason}`);
  expect(row).toHaveTextContent(/last reported wait/i);
  // Controlled invalidations of the captured task binding, retaining the Runtime liveness input.
  for (const changed of [
    { reviewActivity: null },
    { reviewActivity: { ...task.reviewActivity!, activity: "resumed" as const } },
    { reviewContextSequence: task.reviewContextSequence! + 1 },
    { reviewActivityRevision: task.reviewActivityRevision! + 1 },
    { headSha: "f".repeat(40) },
  ]) {
    view.rerender(<LanesTimeline {...props} tasks={[toMissionTask({ ...task, ...changed })]} now={at + 1}
      lanes={laneBars(records, projection, at + 1, props.windowMs)} />);
    expect(row).not.toHaveTextContent(`Waiting: ${activity.reason}`);
    expect(row).not.toHaveTextContent(/last reported wait/i);
  }
  // Controlled N/10N histories derived from the captured record/binding. This measures the
  // public projection -> laneBars -> real render path, not Runtime authentication/liveness.
  const seed = projection.records.find((record) => record.sequence === activity.sequence)!;
  for (const count of [100, 1000]) {
    const added = Array.from({ length: count }, (_, index) => ({ ...seed, sequence: activity.sequence + index,
      revision: index + 1 }));
    const last = added.at(-1)!;
    const scaledTask = { ...task, lastSequence: last.sequence, reviewActivityRevision: count,
      reviewActivity: { ...task.reviewActivity!, sequence: last.sequence, revision: count } };
    const expanded = { ...projection, tasks: [scaledTask],
      records: [...projection.records.filter((record) => record.sequence !== activity.sequence), ...added],
      slices: projection.slices.map((slice) => ({ ...slice, state: scaledTask })),
      routes: [...projection.routes.filter((route) => route.sequence !== activity.sequence),
        ...added.map((record) => ({ sequence: record.sequence, slice: projection.slices[0]!.id }))] };
    const started = performance.now();
    const parsed = tasksReply(expanded)!;
    const bars = laneBars(parsed.records.map((record) => ({ ...record, at: record.occurredAt! })), parsed, at + 1, props.windowMs);
    view.rerender(<LanesTimeline {...props} tasks={parsed.tasks.map(toMissionTask)} now={at + 1} lanes={bars} />);
    console.info(`review_activity controlled projection/render records=${count} tasks=1 lanes=${bars.length} elapsed_ms=${(performance.now() - started).toFixed(3)}`);
    expect(row).toHaveTextContent(`Waiting: ${activity.reason}`);
    expect(screen.getByRole("button", { name: "Working 0" })).toBeInTheDocument();
  }
});
