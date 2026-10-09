import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { MissionView } from "./mission-view";
import type { JourneyView } from "../runtime/types";

const journeys: JourneyView[] = [{
  contractId: "watch", title: "Watch plays inside the Studio", arrows: [],
  steps: [{ stepId: "mark", screen: { screenId: "mark", title: "Mark a skipped step safe", scopePaths: [] }, promises: [] }],
}];

describe("MissionView", () => {
  it("goes graph → proof → test canvas on the chosen step", async () => {
    const userEvent = fastUserEvent();
    const onMarkSafe = vi.fn();
    render(<MissionView journeys={journeys} tasks={[]} lanes={[]} now={0}
      runFor={() => ({ state: "ready", kind: "replay", screens: {}, edges: { mark: { result: "skipped", reason: "data-changing" } } })}
      frameUrl={() => null} onMarkSafe={onMarkSafe} onSendBack={vi.fn()} />);
    expect(screen.getByText("0/1 proven · 0 in flight · 1 need you")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Proof" }));
    await userEvent.click(screen.getByRole("button", { name: "Open test for step 1" }));
    expect(screen.getByRole("region", { name: "Emulated browser" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "I watched it — mark safe" }));
    expect(onMarkSafe).toHaveBeenCalledWith("mark", "watch");
  });

  it("no journeys: says so", () => {
    render(<MissionView journeys={[]} tasks={[]} lanes={[]} now={0} runFor={() => null} frameUrl={() => null} onMarkSafe={vi.fn()} onSendBack={vi.fn()} />);
    expect(screen.getByText("No journeys in this project yet")).toBeInTheDocument();
  });
});
