import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { DEMONSTRATION_SENTENCE, RunPanel, resetPanelCaches } from "./panel";
import type { ExecutionStatus } from "../runtime/types";

/**
 * A completed fixture run is a demonstration and the panel says so (#1064).
 *
 * Before the executor was declared at start, a run whose every outcome came from a fixture file
 * was byte-identical on this surface to one a model produced. The Runtime now publishes
 * `executor` on the status, and this is the guard that the panel turns `"fixture"` into words -
 * and turns nothing else into those words, so a real run is never labelled a rehearsal.
 */
const statusWith = (executor: ExecutionStatus["executor"]): ExecutionStatus =>
  ({
    executionId: "demo",
    status: "completed",
    mode: "supervised",
    attention: "can_sleep",
    attentionReasons: [],
    untriagedInterruptions: [],
    silenceUnevaluated: [],
    headSequence: 9,
    startedAt: null,
    lastEventAt: null,
    nodeStateCounts: { succeeded: 2 },
    nodeLastEventAt: {},
    executor,
  }) as ExecutionStatus;

describe("the demonstration label on the run panel", () => {
  beforeEach(() => resetPanelCaches());

  it("names a fixture run as a demonstration, in the sentence the monitor page uses", () => {
    render(<RunPanel status={statusWith("fixture")} events={[]} onClose={vi.fn()} />);
    expect(screen.getByText(DEMONSTRATION_SENTENCE)).toBeInTheDocument();
    expect(DEMONSTRATION_SENTENCE).toMatch(/^Demonstration run/);
    expect(DEMONSTRATION_SENTENCE).toContain("not produced by a model or a tool");
    expect(screen.getByText("Demonstration finished · scripted outcomes")).toBeInTheDocument();
    expect(screen.getByText("scripted steps", { exact: false })).toHaveTextContent("2");
    expect(screen.queryByText("This run is completed")).not.toBeInTheDocument();
  });

  it("says nothing of the kind for a gateway run", () => {
    render(<RunPanel status={statusWith("gateway")} events={[]} onClose={vi.fn()} />);
    expect(screen.queryByText(DEMONSTRATION_SENTENCE)).toBeNull();
  });

  it("separates a completed execution from unverified model replies", () => {
    render(<RunPanel status={statusWith("gateway")} events={[]} unverifiedResults={3} onClose={vi.fn()} />);
    expect(screen.getByText("Execution finished · review needed")).toBeInTheDocument();
    expect(screen.getByRole("note")).toHaveTextContent("3 node results finished without a confirmed acceptance verdict");
    expect(screen.getByText("steps finished", { exact: false })).toHaveTextContent("2");
    expect(screen.queryByText("This run is completed")).not.toBeInTheDocument();
  });

  it("claims nothing either way when the stream never declared an executor", () => {
    render(<RunPanel status={statusWith(null)} events={[]} onClose={vi.fn()} />);
    expect(screen.queryByText(DEMONSTRATION_SENTENCE)).toBeNull();
    const { executor: _absent, ...undeclared } = statusWith(null);
    render(<RunPanel status={undeclared as ExecutionStatus} events={[]} onClose={vi.fn()} />);
    expect(screen.queryByText(DEMONSTRATION_SENTENCE)).toBeNull();
  });
});
