import { render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import { buildGraphModel } from "../graph/model";
import type { RuntimeEvent } from "../runtime/types";
import { NodePanel, resetPanelCaches } from "./panel";

it("shows one real attempt and the model reply without opening accounting blobs", async () => {
  resetPanelCaches();
  const event = (sequence: number, nextState: string, refs: string[] = []): RuntimeEvent => ({
    sequence,
    kind: "node_outcome_recorded",
    payload: { nodeId: "review_browser_evidence", outcome: nextState === "succeeded" ? "succeeded" : "started", nextState },
    occurredAt: `2026-09-26T02:45:0${sequence}Z`,
    actorId: "system-runtime",
    actorType: "system",
    idempotencyKey: `k-${sequence}`,
    eventId: `event-${sequence}`,
    evidenceRefs: refs,
  });
  const events = [
    event(1, "queued"),
    event(2, "running"),
    event(3, "succeeded", ["run-review-a1-reply", "run-review-a1-context-provenance", "run-review-a1-accounting-receipt"]),
  ];
  const node = buildGraphModel(events).nodes[0];
  const openEvidence = vi.fn(async (_executionId: string, evidenceId: string) => ({
    evidenceId,
    content: JSON.stringify({ text: "Browser check incomplete: no screenshots supplied.", usage: { inputTokens: 10, outputTokens: 8 } }),
    mediaType: "application/json",
    contentSha256: "hash",
    sensitivity: "confidential",
  }));
  render(<NodePanel node={node} events={events} onClose={vi.fn()} executionId="run-review" openEvidence={openEvidence} />);
  const panel = screen.getByRole("region", { name: "Node review_browser_evidence" });
  expect(panel).toHaveTextContent("Attempts1");
  expect(panel).toHaveTextContent("Reply received · acceptance not verified");
  await waitFor(() => expect(panel).toHaveTextContent("Browser check incomplete: no screenshots supplied."));
  expect(openEvidence).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "show context sources" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "show token accounting" })).toBeInTheDocument();
});
