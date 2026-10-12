import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { ProofTable } from "./proof-table";
import type { Mission } from "../runtime/mission";
import type { JourneyRunSource } from "./journey-flows";

const userEvent = fastUserEvent();

const mission: Mission = {
  contractId: "j", title: "J",
  steps: [
    { stepId: "mark", index: 0, title: "Mark a skipped step safe", status: "needs_you", reason: "data-changing", promise: "The owner can mark a skipped step safe." },
    { stepId: "next", index: 1, title: "Next", status: "not_run", reason: null, promise: null },
  ],
  tasks: [{ key: "k", pr: 564, issue: null, title: "t", lane: "gh-claude-8", reviewers: ["gh-claude-2"], step: "review", blocked: false, trust: 1,
    blockedBy: null, rounds: [], headSha: null, mergeSha: null, repoUrl: null }],
  summary: { proven: 0, total: 2, inFlight: 1, needYou: 1, readyUnclaimed: 0 },
};

describe("ProofTable", () => {
  it("reports a Proof rerun so its step evidence can refresh", async () => {
    const onRun = vi.fn();
    const run: JourneyRunSource = {
      start: vi.fn(async () => ({ state: "ready" as const, kind: "preview" as const, result: "pass" as const, screens: { mark: { result: "pass" as const, frame: false } } })),
      read: vi.fn(async () => ({ state: "none" as const, screens: {} })),
      screenFrame: vi.fn(async () => null), liveFrame: vi.fn(async () => null),
    };
    render(<ProofTable mission={mission} onOpenTest={vi.fn()} run={run} onRun={onRun}
      flow={{ id: "j", title: "J", status: "draft", approved: null, drift: [], findings: [], approvable: true, screens: [], edges: [], paths: {} }} />);
    await userEvent.click(screen.getByRole("button", { name: "Run again" }));
    await waitFor(() => expect(onRun).toHaveBeenCalledWith(expect.objectContaining({ state: "ready", result: "pass" })));
  });
  it("#813: draft note is a muted line below the clean question", () => {
    render(<ProofTable mission={{ ...mission, title: "Add a model (draft: the test setup has no model gateway)" }} onOpenTest={vi.fn()} />);
    expect(screen.getByRole("heading", { name: 'Can I trust “Add a model”?' })).toBeInTheDocument();
    expect(screen.getByText("Draft: the test setup has no model gateway")).toHaveClass("proof-sub");
  });

  it("one row per step: promise, status badge, custody chips and the call", async () => {
    const onOpenTest = vi.fn();
    render(<ProofTable mission={mission} onOpenTest={onOpenTest} />);
    expect(screen.getByRole("heading", { name: "Can I trust “J”?" })).toBeInTheDocument();
    const rows = within(screen.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("The owner can mark a skipped step safe.");
    expect(rows[0]).toHaveTextContent("Needs you");
    expect(rows[0]).toHaveTextContent("data-changing");
    expect(rows[0]).toHaveTextContent("journey replay · step 1 SKIPPED");
    expect(rows[0]).toHaveTextContent("impl gh-claude-8");
    expect(rows[0]).toHaveTextContent("rev gh-claude-2");
    expect(rows[1]).toHaveTextContent("No work linked to this step");
    await userEvent.click(screen.getByRole("button", { name: "Open test for step 1" }));
    expect(onOpenTest).toHaveBeenCalledWith("mark");
  });

  it("the replay thumbnail opens the test and shows the real frame when there is one", async () => {
    const onOpenTest = vi.fn();
    render(<ProofTable mission={mission} onOpenTest={onOpenTest} frameUrl={(id) => (id === "mark" ? "blob:m" : null)} />);
    expect(screen.getByRole("img", { name: "Replay frame of step 1" })).toHaveAttribute("src", "blob:m");
    expect(screen.getAllByRole("img")).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "Open the test canvas for step 2" }));
    expect(onOpenTest).toHaveBeenCalledWith("next");
  });


});
