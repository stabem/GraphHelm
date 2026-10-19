import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { ProofTable } from "./proof-table";
import type { Mission } from "../runtime/mission";
import type { JourneyRunSource } from "./journey-flows";
import type { JourneyRunView } from "../runtime/types";

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

  it("does not claim observer identity that the journey response does not carry", () => {
    render(<ProofTable mission={mission} onOpenTest={vi.fn()}
      flow={{ id: "j", title: "J", status: "draft", approved: null, drift: [], findings: [], approvable: true,
        screens: [], edges: [], paths: { main: [] } }} />);

    expect(screen.getByRole("heading", { name: "Promise" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Observer and evidence" })).toBeInTheDocument();
    const evidence = screen.getByRole("region", { name: "Observer and evidence" });
    expect(within(evidence).getByText("Observer identity unavailable from this journey response")).toBeInTheDocument();
    expect(within(evidence).getByText("No evidence yet")).toBeInTheDocument();
  });
  it("#947: a stale draft without a replay cannot be approved and explains both blockers", () => {
    const onApprove = vi.fn();
    render(<ProofTable mission={{ ...mission, title: "Old contract" }} onOpenTest={vi.fn()} onApprove={onApprove}
      flow={{ id: "j", title: "Changed flow", status: "draft", approved: null, drift: [], findings: [], approvable: true,
        screens: [], edges: [], paths: { main: [] } }} />);
    const approve = screen.getByRole("button", { name: "Approve" });
    expect(approve).toBeDisabled();
    expect(approve).toHaveAccessibleDescription(/Contract out of date, regenerate first/);
    expect(onApprove).not.toHaveBeenCalled();
  });

  it("#947: a current draft without a completed replay says why approval is unavailable", () => {
    render(<ProofTable mission={mission} onOpenTest={vi.fn()} onApprove={vi.fn()}
      flow={{ id: "j", title: "J", status: "draft", approved: null, drift: [], findings: [], approvable: true,
        screens: [{ id: "mark", url: "/", state: "stable" }, { id: "next", url: "/", state: "stable" }],
        edges: [{ id: "mark.next", from: "mark", to: "next" }], paths: { main: ["mark.next"] } }} />);
    const approve = screen.getByRole("button", { name: "Approve" });
    expect(approve).toBeDisabled();
    expect(approve).toHaveAccessibleDescription(/Not run yet/);
  });
  it("#1448: a draft's passing preview enables approval, a failed one does not", async () => {
    // Decision on #1448: a draft cannot be replayed until it is approved, so a replay-only gate
    // left Approve disabled forever. A passing preview on the current draft digest (the Runtime
    // only returns the run for that digest) unlocks Approve; anything short of pass does not.
    const flow = { id: "j", title: "J", status: "draft" as const, approved: null, drift: [], findings: [], approvable: true,
      screens: [{ id: "mark", url: "/", state: "stable" }, { id: "next", url: "/", state: "stable" }],
      edges: [{ id: "mark.next", from: "mark", to: "next" }], paths: { main: ["mark.next"] } };
    const result = { state: "ready" as const, result: "pass" as const,
      screens: { mark: { result: "pass" as const, frame: true }, next: { result: "pass" as const, frame: true } },
      edges: { "mark.next": { result: "pass" as const } } };
    const run: JourneyRunSource = { start: vi.fn(), read: vi.fn(async () => ({ ...result, kind: "preview" as const,
      result: "fail" as const, edges: { "mark.next": { result: "fail" as const } } })),
      screenFrame: vi.fn(async () => null), liveFrame: vi.fn(async () => null) };
    const onApprove = vi.fn().mockResolvedValue(undefined);
    const view = render(<ProofTable mission={mission} onOpenTest={vi.fn()} flow={flow} run={run} onApprove={onApprove} />);
    await waitFor(() => expect(run.read).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "Approve" })).toBeDisabled();
    view.unmount();
    run.read = vi.fn(async () => ({ ...result, kind: "preview" as const }));
    render(<ProofTable mission={mission} onOpenTest={vi.fn()} flow={flow} run={run} onApprove={onApprove} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled());
    await userEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(onApprove).toHaveBeenCalledWith("j");
  });
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
  // #1635: a Run blocks until the run ends; the owner saw nothing move for that whole time.
  it("#1635: says Running at once and tells the rail, until the run answers", async () => {
    const onRun = vi.fn();
    let finish!: (value: JourneyRunView) => void;
    const run: JourneyRunSource = {
      start: vi.fn(() => new Promise<JourneyRunView>((resolve) => { finish = resolve; })),
      read: vi.fn(async () => ({ state: "none" as const, screens: {} })),
      screenFrame: vi.fn(async () => null), liveFrame: vi.fn(async () => null),
    };
    render(<ProofTable mission={mission} onOpenTest={vi.fn()} run={run} onRun={onRun}
      flow={{ id: "j", title: "J", status: "draft", approved: null, drift: [], findings: [], approvable: true, screens: [], edges: [], paths: {} }} />);
    await userEvent.click(screen.getByRole("button", { name: "Run again" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Running…");
    expect(screen.getByRole("button", { name: "Run again" })).toBeDisabled();
    expect(onRun).toHaveBeenCalledWith(expect.objectContaining({ state: "running" }));
    finish({ state: "ready", kind: "preview", result: "pass", screens: { mark: { result: "pass", frame: false } } });
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(onRun).toHaveBeenLastCalledWith(expect.objectContaining({ state: "ready", result: "pass" }));
  });
  it("#1157: a rerun that failed for want of an observer says so and how to fix it", async () => {
    const failed = { state: "failed" as const, kind: "preview" as const, reason: "driver.observer_missing",
      detail: { code: "replay.observer_missing", pointer: "/observer" }, screens: {} };
    const run: JourneyRunSource = {
      start: vi.fn(async () => failed), read: vi.fn(async () => ({ state: "none" as const, screens: {} })),
      screenFrame: vi.fn(async () => null), liveFrame: vi.fn(async () => null),
    };
    render(<ProofTable mission={mission} onOpenTest={vi.fn()} run={run}
      flow={{ id: "j", title: "J", status: "draft", approved: null, drift: [], findings: [], approvable: true, screens: [], edges: [], paths: {} }} />);
    await userEvent.click(screen.getByRole("button", { name: "Run again" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("no browser observer is installed");
    expect(screen.getByRole("alert")).toHaveTextContent("graphhelm setup --install-observer playwright");
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
