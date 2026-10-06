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
