import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
// Its own instance: see the helper for why this is not a shared const.
const userEvent = fastUserEvent();

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
        onOpenModels={vi.fn()}
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
        onOpenModels={vi.fn()}
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

/**
 * #1098 D4: two runs whose objectives share a long prefix read identically on the rail — the name
 * line ellipsises and the grey address line is the only difference. The blind visual judge hovered
 * for the full name and got the id instead: the name line's `title` was the execution id, which the
 * row already prints underneath. The hover must carry what is CUT (the full name), and the address
 * line must carry its own full id, which is the part that ellipsises at 248px.
 */
describe("a truncated row says its whole name on hover", () => {
  const long = "Produzir build de homologacao para o cliente e publicar o relatorio";
  const other = "Produzir build de homologacao para o cliente e arquivar o anterior";

  function railWithCollidingNames() {
    render(
      <ProjectRail
        projects={[{ name: "store", runs: [{ ...row("run-aaaa1111"), objective: long }, { ...row("run-bbbb2222"), objective: other }] }]}
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
  }

  it("titles the name line with the full name, not with the id printed below it", () => {
    railWithCollidingNames();
    const first = screen.getByRole("button", { name: new RegExp(long) });
    const name = within(first).getByText(long);
    expect(name).toHaveClass("run-id");
    expect(name).toHaveAttribute("title", long);
  });

  it("titles the address line with the full id, which is what gets cut", () => {
    railWithCollidingNames();
    const second = screen.getByRole("button", { name: new RegExp(other) });
    expect(within(second).getByText("run-bbbb2222")).toHaveAttribute("title", "run-bbbb2222");
  });

  // #1171: the models entry point had no cell at all. A review lane proved it by rewiring
  // `onClick={onOpenModels}` to `onClick={onAddProject}` -- typecheck stayed at 0 and all 465
  // tests stayed green, because both rail suites only PASSED the new prop and never pressed the
  // button. A prop a suite tolerates is not a prop a suite constrains.
  it("opens the models screen from the rail, and not the add-project screen", async () => {
    const onOpenModels = vi.fn();
    const onAddProject = vi.fn();
    render(
      <ProjectRail
        projects={[{ name: "store", runs: [row("exec_feature")] }]}
        selected=""
        connected
        hasMore={false}
        busy={false}
        onSelect={vi.fn()}
        onLoadMore={vi.fn()}
        onNewTask={vi.fn()}
        onAddProject={onAddProject}
        onOpenModels={onOpenModels}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "models" }));
    expect(onOpenModels).toHaveBeenCalledTimes(1);
    // The sibling button is the decoy: wiring this one to it is the exact defect the lane staged.
    expect(onAddProject).not.toHaveBeenCalled();
  });
});
