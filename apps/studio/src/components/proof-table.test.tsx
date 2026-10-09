import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { ProofTable } from "./proof-table";
import type { Mission } from "../runtime/mission";

const userEvent = fastUserEvent();

const mission: Mission = {
  contractId: "j", title: "J",
  steps: [{ stepId: "mark", index: 0, title: "Mark a skipped step safe", status: "needs_you", reason: "data-changing" }],
  tasks: [], summary: { proven: 0, total: 1, inFlight: 0, needYou: 1 },
};

describe("ProofTable", () => {
  it("one row per step, with status text and an open-test action", async () => {
    const onOpenTest = vi.fn();
    render(<ProofTable mission={mission} onOpenTest={onOpenTest} />);
    expect(screen.getByRole("row", { name: /Mark a skipped step safe.*Needs you/ })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open test for step 1" }));
    expect(onOpenTest).toHaveBeenCalledWith("mark");
  });
});
