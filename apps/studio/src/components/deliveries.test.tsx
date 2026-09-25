import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NodeDeliveries } from "./deliveries";
import type { EvidenceContent, RuntimeEvent } from "../runtime/types";

const event: RuntimeEvent = { sequence: 1, kind: "signal_recorded", payload: { kind: "node_delivery", sourceKind: "node", sourceId: "docs" }, occurredAt: null, actorId: null, actorType: null, idempotencyKey: null, eventId: null, evidenceRefs: ["evidence"] };
const evidence: EvidenceContent = { evidenceId: "evidence", mediaType: "application/json", sensitivity: "internal", contentSha256: "hash", content: JSON.stringify({ description: JSON.stringify({ version: 1, projectId: "a".repeat(64), summary: "Updated refund policy", reason: "<script>untrusted</script>", documents: [{ path: "docs/rule.md", title: "Refund rule", kind: "business_rule", action: "updated", journeyIds: ["refund-request"] }] }) }) };
describe("node delivery records", () => {
  it("opens the referenced document and renders prose as text", async () => {
    const onOpenDocument = vi.fn();
    const { container } = render(<NodeDeliveries nodeId="docs" executionId="run" events={[event]} openEvidence={async () => evidence} onOpenDocument={onOpenDocument} />);
    fireEvent.click(await screen.findByRole("button", { name: /Refund rule/ }));
    expect(onOpenDocument).toHaveBeenCalledWith({ evidenceId: "evidence", index: 0, path: "docs/rule.md", title: "Refund rule", projectId: "a".repeat(64) });
    expect(screen.getByText("<script>untrusted</script>")).toBeInTheDocument();
    expect(container.querySelector("script")).toBeNull();
    expect(screen.getByText("Journeys: refund-request")).toBeInTheDocument();
  });
  it("does not display evidence from a previous run after navigation", async () => {
    let resolve!: (value: EvidenceContent) => void;
    const openEvidence = vi.fn(() => new Promise<EvidenceContent>((done) => { resolve = done; }));
    const props = { nodeId: "docs", openEvidence, onOpenDocument: vi.fn() };
    const { rerender } = render(<NodeDeliveries {...props} executionId="old" events={[event]} />);
    await waitFor(() => expect(openEvidence).toHaveBeenCalled());
    rerender(<NodeDeliveries {...props} executionId="new" events={[]} />);
    resolve(evidence);
    expect(await screen.findByText(/No deliveries recorded/)).toBeInTheDocument();
    expect(screen.queryByText("Updated refund policy")).not.toBeInTheDocument();
  });
});
