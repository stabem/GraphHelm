import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
// Its own instance: see the helper for why this is not a shared const.
const userEvent = fastUserEvent();

import { RunPanel, resetPanelCaches } from "./panel";
import type { ExecutionStatus } from "../runtime/types";

/**
 * The message box follows the RUN, not the mount.
 *
 * The run panel keeps its tree position across a run switch, so React preserves the SayBox's
 * state: run A's half-typed message sat in the box with run B selected, one Enter away from
 * landing in the wrong append-only log (PR #467 review, P1 at panel.tsx:373). The draft store
 * was already keyed by execution; the STATE was not - a useState initializer runs once.
 */
const statusOf = (executionId: string): ExecutionStatus =>
  ({
    executionId,
    status: "running",
    mode: "supervised",
    attention: "can_sleep",
    attentionReasons: [],
    untriagedInterruptions: [],
    silenceUnevaluated: [],
    headSequence: 3,
    startedAt: null,
    lastEventAt: null,
    nodeStateCounts: {},
  }) as never;

/**
 * #1083 F5, the second box (orchestrator verification of PR #1091): the run's message box had no
 * key handler, so Enter left the text in the box and sent nothing. It now reads the same
 * `sendsOnEnter` the composer does. Native `keydown` events, dispatched on the focused textarea
 * the way a browser delivers them.
 */
describe("Enter in the run's message box", () => {
  beforeEach(() => resetPanelCaches());
  const press = (target: HTMLElement, init: KeyboardEventInit & { keyCode?: number }) => {
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    if (init.keyCode !== undefined) Object.defineProperty(event, "keyCode", { value: init.keyCode });
    target.dispatchEvent(event);
    return event;
  };
  async function typed(text: string) {
    const onSay = vi.fn();
    render(<RunPanel status={statusOf("run-a")} events={[]} onClose={vi.fn()} onSay={onSay} />);
    const box = screen.getByLabelText("Say something into this run");
    await userEvent.type(box, text);
    return { box, onSay };
  }

  it("sends on a native Enter and on an automation driver's Enter, keeping the newline out", async () => {
    const { box, onSay } = await typed("is the deploy safe?");
    const event = press(box, { key: "Enter", code: "Enter", keyCode: 13 });
    expect(event.defaultPrevented).toBe(true);
    expect(onSay).toHaveBeenCalledWith("is the deploy safe?", null);
    press(box, { key: "Enter", code: "", keyCode: 0 });
    expect(onSay).toHaveBeenCalledTimes(2);
  });

  it("sends through the keyboard path a user-event Enter takes", async () => {
    const { onSay } = await typed("status please");
    await userEvent.keyboard("{Enter}");
    expect(onSay).toHaveBeenCalledWith("status please", null);
  });

  it("never sends on Shift+Enter, on an IME's committing Enter, or with nothing typed", async () => {
    const { box, onSay } = await typed("日本語");
    press(box, { key: "Enter", code: "Enter", shiftKey: true, keyCode: 13 });
    press(box, { key: "Enter", code: "Enter", isComposing: true, keyCode: 13 });
    press(box, { key: "Process", code: "Enter", keyCode: 229 });
    expect(onSay).not.toHaveBeenCalled();
    await userEvent.clear(box);
    press(box, { key: "Enter", code: "Enter", keyCode: 13 });
    expect(onSay).not.toHaveBeenCalled();
  });
});

describe("the message box across run switches", () => {
  beforeEach(() => resetPanelCaches());

  it("never carries one run's unsent words into another run's box", async () => {
    const view = render(
      <RunPanel status={statusOf("run-a")} events={[]} onClose={vi.fn()} onSay={vi.fn()} />,
    );
    const box = screen.getByPlaceholderText(/say something/i);
    await userEvent.type(box, "the answer meant for run A");

    view.rerender(
      <RunPanel status={statusOf("run-b")} events={[]} onClose={vi.fn()} onSay={vi.fn()} />,
    );
    expect(screen.getByPlaceholderText(/say something/i)).toHaveValue("");
  });

  it("brings a run's own unsent words back when the operator returns to it", async () => {
    const view = render(
      <RunPanel status={statusOf("run-a")} events={[]} onClose={vi.fn()} onSay={vi.fn()} />,
    );
    await userEvent.type(screen.getByPlaceholderText(/say something/i), "half-typed thought");

    view.rerender(
      <RunPanel status={statusOf("run-b")} events={[]} onClose={vi.fn()} onSay={vi.fn()} />,
    );
    view.rerender(
      <RunPanel status={statusOf("run-a")} events={[]} onClose={vi.fn()} onSay={vi.fn()} />,
    );
    expect(screen.getByPlaceholderText(/say something/i)).toHaveValue("half-typed thought");
  });

  /** The delivered-clears-the-box edge must belong to THIS surface: run A's send completing
   * after a switch arrived at run B's box as a busy-to-idle edge, and the box read it as B's
   * own delivery and deleted B's half-written words (PR #467 review, P1). */
  it("never reads another run's completion as this run's delivery", async () => {
    const view = render(
      <RunPanel status={statusOf("run-a")} events={[]} onClose={vi.fn()} onSay={vi.fn()} saying={false} />,
    );
    await userEvent.type(screen.getByPlaceholderText(/say something/i), "for run A");
    // A's send goes into flight...
    view.rerender(
      <RunPanel status={statusOf("run-a")} events={[]} onClose={vi.fn()} onSay={vi.fn()} saying />,
    );
    // ...and the operator switches to B mid-send (selection clears the busy display).
    view.rerender(
      <RunPanel status={statusOf("run-b")} events={[]} onClose={vi.fn()} onSay={vi.fn()} saying={false} />,
    );
    await userEvent.type(screen.getByPlaceholderText(/say something/i), "for run B");
    view.rerender(
      <RunPanel status={statusOf("run-b")} events={[]} onClose={vi.fn()} onSay={vi.fn()} saying={false} />,
    );
    // B's words survive the edge that was not theirs; A's draft is still waiting at home.
    expect(screen.getByPlaceholderText(/say something/i)).toHaveValue("for run B");
    view.rerender(
      <RunPanel status={statusOf("run-a")} events={[]} onClose={vi.fn()} onSay={vi.fn()} saying={false} />,
    );
    expect(screen.getByPlaceholderText(/say something/i)).toHaveValue("for run A");
  });
});

/**
 * #1098 D3: the composer prints "Enter sends · Shift+Enter for a new line" under its box; the run's
 * message box, which sends on Enter since #1091, printed no hint at all. Two boxes on one page with
 * the same key contract and one of them silent about it.
 */
describe("the run's message box says what Enter does", () => {
  beforeEach(() => resetPanelCaches());

  it("prints the same Enter hint the composer prints", () => {
    render(<RunPanel status={statusOf("run-a")} events={[]} onClose={vi.fn()} onSay={vi.fn()} />);
    expect(screen.getByText("Enter sends · Shift+Enter for a new line")).toBeInTheDocument();
  });
});
