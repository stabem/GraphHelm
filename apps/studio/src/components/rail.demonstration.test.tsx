import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { ProjectRail } from "./rail";
import type { ExecutionSummary } from "../runtime/types";

/**
 * The index marks a demonstration too (#1064). The run panel says it once a run is opened; the
 * rail is where an operator picks WHICH run to open, and a fixture run that looked exactly like
 * a real one there was the gap the review named. `executor` rides `GET /v1/executions` rows.
 */
const summary = (executionId: string, executor: ExecutionSummary["executor"]): ExecutionSummary => ({
  executionId,
  mode: "supervised",
  status: "completed",
  attention: "can_sleep",
  startedAt: null,
  lastEventAt: null,
  headSequence: 9,
  executor,
});

describe("the demonstration mark on the rail", () => {
  it("marks a fixture run and leaves a gateway run and an undeclared one alone", () => {
    render(
      <ProjectRail
        projects={[
          { name: "store", runs: [summary("demo", "fixture"), summary("real", "gateway"), summary("old", null)] },
        ]}
        selected=""
        connected
        hasMore={false}
        busy={false}
        onSelect={vi.fn()}
        onLoadMore={vi.fn()}
        onNewTask={vi.fn()}
        onAddProject={vi.fn()}
        onOpenModels={vi.fn()}
      />,
    );
    const marks = screen.getAllByText(/demonstration/);
    expect(marks).toHaveLength(1);
    expect(marks[0].closest("button")).toHaveTextContent("demo");
    expect(screen.getByText("real").closest("button")).not.toHaveTextContent("demonstration");
    expect(screen.getByText("old").closest("button")).not.toHaveTextContent("demonstration");
  });
});
