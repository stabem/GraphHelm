import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

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
