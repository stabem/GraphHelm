// #353: the owner reviews and approves journey-flow drafts in the Journey tab. Regressions caught:
// an Approve offered on a flow the Runtime would refuse, findings or drift hidden from review, a
// refusal swallowed, and an already-settled approval offered again. Cost: jsdom render, no network.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import type { JourneyFlowView, JourneyFlowsView } from "../runtime/types";
import { JourneyFlows } from "./journey-flows";

const userEvent = fastUserEvent();
afterEach(cleanup);

const flow = (id: string, extra: Partial<JourneyFlowView> = {}): JourneyFlowView => ({
  id, title: `Flow ${id}`, status: "draft", approved: null, drift: [], findings: [], approvable: true,
  screens: [{ id: "cart", url: "/cart", state: "stable" }, { id: "pay", url: "/checkout", state: "stable" }],
  edges: [{ id: "cart.checkout", from: "cart", to: "pay" }],
  paths: { main: ["cart.checkout"] },
  ...extra,
});

const view: JourneyFlowsView = { flows: [
  flow("checkout"),
  flow("broken", { approvable: false, findings: [{ code: "flow.scope_path_missing", pointer: "/screens/0/scope/0", message: "scope does not exist", severity: "error" }] }),
  flow("done", { status: "approved", approvable: false, approved: { revision: "a".repeat(40), digest: `sha256:${"b".repeat(64)}` } }),
  flow("edited", { status: "approval_stale", drift: [{ edge: "cart.checkout", code: "drift.locator_missing" }],
    findings: [{ code: "flow.approval_stale", pointer: "/approved/digest", message: "approval does not bind this flow projection", severity: "error" }] }),
] };

describe("JourneyFlows", () => {
  it("shows each flow's status, drift, findings and graph, and offers Approve only where it would be accepted", () => {
    render(<JourneyFlows view={view} onApprove={vi.fn()} />);
    const checkout = screen.getByRole("article", { name: "Flow checkout" });
    expect(checkout).toHaveTextContent("Draft — waiting for your approval");
    expect(within(checkout).getByLabelText("Edge cart.checkout")).toHaveTextContent("cart.checkout → pay");
    expect(checkout).toHaveTextContent("/checkout");
    expect(within(checkout).getByRole("button", { name: "Approve" })).toBeEnabled();

    const broken = screen.getByRole("article", { name: "Flow broken" });
    expect(within(broken).getByRole("button", { name: "Approve" })).toBeDisabled();
    expect(within(broken).getByLabelText("Findings")).toHaveTextContent("flow.scope_path_missing");
    expect(broken).toHaveTextContent("Fix before approving: flow.scope_path_missing");

    const done = screen.getByRole("article", { name: "Flow done" });
    expect(done).toHaveTextContent("Approved");
    expect(within(done).queryByRole("button", { name: "Approve" })).toBeNull();

    const edited = screen.getByRole("article", { name: "Flow edited" });
    expect(edited).toHaveTextContent("Approval stale — edited after it was approved");
    expect(within(edited).getByLabelText("Drift")).toHaveTextContent("cart.checkout · drift.locator_missing");
    expect(within(edited).getByRole("button", { name: "Approve" })).toBeEnabled();
  });

  it("approves through the callback and shows the Runtime's refusal", async () => {
    const onApprove = vi.fn().mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error("approve requires a canonical flow"));
    render(<JourneyFlows view={view} onApprove={onApprove} />);
    const checkout = screen.getByRole("article", { name: "Flow checkout" });
    await userEvent.click(within(checkout).getByRole("button", { name: "Approve" }));
    expect(onApprove).toHaveBeenCalledWith("checkout");
    expect(within(checkout).queryByRole("alert")).toBeNull();

    const edited = screen.getByRole("article", { name: "Flow edited" });
    await userEvent.click(within(edited).getByRole("button", { name: "Approve" }));
    expect(onApprove).toHaveBeenLastCalledWith("edited");
    expect(await within(edited).findByRole("alert")).toHaveTextContent("approve requires a canonical flow");
  });

  it("renders nothing without flows and the failure when the read failed", () => {
    const { container } = render(<JourneyFlows view={{ flows: [] }} onApprove={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
    cleanup();
    render(<JourneyFlows view={null} failure="journey flows require an explicit --project on this Runtime" onApprove={vi.fn()} />);
    expect(screen.getByRole("alert")).toHaveTextContent("--project");
  });
});
