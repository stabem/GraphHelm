import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { MissionView } from "./mission-view";
import type { GraphNode } from "../graph/model";
import type { Bot } from "../runtime/team";
import type { Lane } from "../runtime/lane-bars";
import { foldTaskEvents } from "../runtime/team-tasks";

const node = (id: string, declaredName: string, state: GraphNode["state"]): GraphNode =>
  ({ id, declaredName, state, touches: 0, lastEventAt: null, history: [], reopened: null });
const bot = (key: string, name: string, actorId: string | null = key): Bot => ({
  key, actorId, name, hue: 0, role: null, doingNow: "", lastRecordAt: null, lastSequence: 0,
  state: "quiet", quietMinutes: null, shared: false, native: false, tasks: [],
});
const base = { journeys: [], tasks: [], now: 0, runFor: () => null, frameUrl: () => null, onMarkSafe: vi.fn() };

describe("#647 MissionView: the Team canvas controls on the Graph page", () => {
  // Contract: the Lanes entry point exposes recorded handoffs, with no empty box.
  // Regression: forgetting the mount hides a working standalone DelegationTree.
  // Gap: tree component tests never render MissionView. Cost: jsdom, no I/O or test seam.
  it("shows the recorded delegation hierarchy in Lanes and omits it without records", async () => {
    const userEvent = fastUserEvent();
    const delegation = foldTaskEvents([
      { kind: "task.claimed", actorId: "lane-a", sequence: 1, taskId: "issue-901", issue: 901,
        lane: "lane-a", branch: "issue-901-fixture", assignedBy: "coord" },
      { kind: "task.review_assigned", actorId: "lane-a", sequence: 2, taskId: "issue-901",
        pr: 903, headSha: "aaaaaaaa", reviewer: "lane-b" },
      { kind: "task.claimed", actorId: "lane-c", sequence: 3, taskId: "issue-902", issue: 902,
        lane: "lane-c", branch: "issue-902-fixture" },
    ]);
    const props = { ...base, lanes: [], delegation };
    const { rerender } = render(<MissionView {...props} />);
    await userEvent.click(screen.getByRole("tab", { name: "Lanes" }));
    const tree = within(screen.getByRole("region", { name: "Delegation" })).getByRole("tree");
    const coord = within(tree).getByRole("treeitem", { name: "coord" });
    const lane = within(coord).getByRole("treeitem", { name: /^lane-a/ });
    const issue = within(lane).getByRole("treeitem", { name: /^#901/ });
    expect(within(issue).getByRole("treeitem", { name: /^lane-b\s*review_assigned$/ })).toBeInTheDocument();
    const unknown = within(tree).getByRole("treeitem", { name: "assigner unrecorded" });
    expect(within(unknown).getByRole("treeitem", { name: /^lane-c\s*assigner unrecorded$/ })).toBeInTheDocument();
    rerender(<MissionView {...base} lanes={[]} />);
    expect(screen.queryByRole("region", { name: "Delegation" })).toBeNull();
    rerender(<MissionView {...props} delegation={[]} />);
    expect(screen.queryByRole("region", { name: "Delegation" })).toBeNull();
  });

  it("keeps the reopened name field and draft when the saved alias arrives", async () => {
    const userEvent = fastUserEvent();
    const onNameBot = vi.fn();
    const props = { ...base, lanes: [], onNameBot };
    const { rerender } = render(<MissionView {...props} agents={[bot("planner", "planner")]} />);
    await userEvent.click(screen.getByRole("tab", { name: "Lanes" }));
    await userEvent.click(screen.getByRole("button", { name: "Name this bot" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Name for planner" }), "Planny");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await userEvent.click(screen.getByRole("button", { name: "Name this bot" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Name for planner" }), "Next name");
    // The Runtime refresh can arrive after the owner has already reopened the form.
    rerender(<MissionView {...props} agents={[bot("planner", "Planny")]} />);
    expect(screen.getByRole("textbox", { name: "Name for Planny" })).toHaveValue("Next name");
  });

  it("a Run graph node opens its window: the handler gets the node id", async () => {
    const userEvent = fastUserEvent();
    const onOpenNode = vi.fn();
    render(<MissionView {...base} lanes={[]} runNodes={[node("deploy", "Deploy", "ready"), node("plan", "Plan", "succeeded")]}
      unassignedNodeIds={["deploy"]} onOpenNode={onOpenNode} />);
    const strip = screen.getByRole("region", { name: "Run graph" });
    expect(within(strip).getByRole("heading", { name: "Steps without a bot" })).toBeInTheDocument();
    await userEvent.click(within(strip).getByRole("button", { name: "Deploy · ready" }));
    expect(onOpenNode).toHaveBeenCalledWith("deploy");
    await userEvent.click(within(strip).getByRole("button", { name: /^Plan · / }));
    expect(onOpenNode).toHaveBeenLastCalledWith("plan");
  });

  it("Lanes board: Details and Name this bot call their handlers; display names render", async () => {
    const userEvent = fastUserEvent();
    const onOpenBotDetails = vi.fn();
    const onNameBot = vi.fn();
    const lanes: Lane[] = [{ lane: "lane-a", bars: [], silent: false, lastEventAt: 0 }];
    render(<MissionView {...base} lanes={lanes} agents={[bot("planner", "planner"), bot("lane-a", "Alice")]}
      onOpenBotDetails={onOpenBotDetails} onNameBot={onNameBot} />);
    await userEvent.click(screen.getByRole("tab", { name: "Lanes" }));
    const board = screen.getByRole("list", { name: "Agent board" });
    const row = within(board).getByText("planner").closest("li")!;
    await userEvent.click(within(row).getByRole("button", { name: "Details" }));
    expect(onOpenBotDetails).toHaveBeenCalledWith("planner");
    await userEvent.click(within(row).getByRole("button", { name: "Name this bot" }));
    await userEvent.type(within(row).getByRole("textbox", { name: "Name for planner" }), "Planny");
    expect(within(row).getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    await userEvent.click(within(row).getByRole("button", { name: "Save" }));
    expect(onNameBot).toHaveBeenCalledWith("planner", "Planny");
    // The lane row reads the bot's display name, not the raw lane id.
    const laneRows = screen.getByRole("list", { name: "Agent lanes" });
    expect(within(laneRows).getByText("Alice")).toBeInTheDocument();
    expect(within(laneRows).queryByText("lane-a")).toBeNull();
    expect(within(board).getByText("Alice")).toBeInTheDocument();
  });
});
