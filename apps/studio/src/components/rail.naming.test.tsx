import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";

import { ProjectRail } from "./rail";
import type { ExecutionSummary } from "../runtime/types";

/**
 * #1083 F7: the rail listed CLI- and HTTP-started runs by their execution id although the store
 * held their objective; only composer-started `run-<uuid>` rows were named. Every row with an
 * objective is named by it now, and the id stays ON the row as secondary text - the address a
 * CLI command needs - rather than only in a tooltip. A row whose briefing has no objective keeps
 * its id as its only name, once.
 */
const row = (executionId: string): ExecutionSummary => ({
  executionId,
  mode: "supervised",
  status: "completed",
  attention: "can_sleep",
  startedAt: null,
  lastEventAt: null,
  headSequence: 9,
  executor: "fixture",
});

describe("naming rows on the rail", () => {
  it("reads the objective straight off the index row, with no briefing at all", () => {
    render(
      <ProjectRail
        projects={[{ name: "store", runs: [{ ...row("exec_feature"), objective: "Locate related components and tests" }, { ...row("demo"), objective: null }] }]}
        selected=""
        connected
        hasMore={false}
        busy={false}
        onSelect={vi.fn()}
        onLoadMore={vi.fn()}
        onNewTask={vi.fn()}
        onAddProject={vi.fn()}
      />,
    );
    const feature = screen.getByRole("button", { name: /Locate related components and tests/ });
    expect(within(feature).getByText("exec_feature")).toHaveClass("run-address");
    // Anchored: every row's name also carries "· demonstration".
    expect(screen.getByRole("button", { name: /^demo\b/ }).querySelector(".run-address")).toBeNull();
  });

  it("names a hand-named run and a generated run by their objectives, with the id beneath", () => {
    const generated = "run-9a1b2c3d-4e5f-4a6b-8c7d-0e1f2a3b4c5d";
    render(
      <ProjectRail
        projects={[{ name: "store", runs: [row("exec_feature"), row(generated), row("bare")] }]}
        selected=""
        connected
        hasMore={false}
        busy={false}
        onSelect={vi.fn()}
        onLoadMore={vi.fn()}
        onNewTask={vi.fn()}
        onAddProject={vi.fn()}
        briefings={{
          exec_feature: { objective: "Locate related components and tests", name: "Feature" },
          [generated]: { objective: "Investigate slow login on mobile", name: "New task" },
          bare: { objective: null, name: null },
        }}
      />,
    );

    const feature = screen.getByRole("button", { name: /Locate related components and tests/ });
    expect(within(feature).getByText("exec_feature")).toHaveClass("run-address");

    const started = screen.getByRole("button", { name: /Investigate slow login on mobile/ });
    expect(within(started).getByText(generated)).toHaveClass("run-address");

    // No objective: the id is the name, and it is not printed a second time as an address.
    const bare = screen.getByRole("button", { name: /bare/ });
    expect(within(bare).getAllByText("bare")).toHaveLength(1);
    expect(bare.querySelector(".run-address")).toBeNull();
  });
});
