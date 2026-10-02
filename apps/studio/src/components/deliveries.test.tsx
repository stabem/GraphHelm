import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NodeDeliveries } from "./deliveries";
import type { EvidenceContent, RuntimeEvent } from "../runtime/types";
import journeyCandidate from "../../../../extensions/builtin/graphhelm-jpd/fixtures/positive/journey-verification-first-pass.json";

const event: RuntimeEvent = { sequence: 1, kind: "signal_recorded", payload: { kind: "node_delivery", sourceKind: "node", sourceId: "docs" }, occurredAt: null, actorId: null, actorType: null, idempotencyKey: null, eventId: null, evidenceRefs: ["evidence"] };
const evidence: EvidenceContent = { evidenceId: "evidence", mediaType: "application/json", sensitivity: "internal", contentSha256: "hash", content: JSON.stringify({ description: JSON.stringify({ version: 1, projectId: "a".repeat(64), summary: "Updated refund policy", reason: "<script>untrusted</script>", documents: [{ path: "docs/rule.md", title: "Refund rule", kind: "business_rule", action: "updated", journeyIds: ["refund-request"] }] }) }) };
const work = { version: 1, sessionId: "reported-session", stage: "implement", revision: "a".repeat(40), skills: [{ id: "meaningful-tests", version: "1.0.0", digest: `sha256:${"b".repeat(64)}`, status: "reported" }], checks: [
  { id: "focused", command: "npm test -- deliveries", observer: "vitest", outcome: "failed", attemptId: "attempt-2", previousAttemptId: "attempt-1", evidence: { evidenceId: "check-evidence", contentHash: `sha256:${"c".repeat(64)}`, size: 12 } },
  { id: "unobserved", command: "browser journey", observer: "browser", outcome: "unobserved", attemptId: "attempt-2" },
] };
describe("node delivery records", () => {
  // Protects against offering unsupported source files to the document API.
  // Existing coverage opens only Markdown. Cost: one local component render; no I/O.
  it("shows source files as references while keeping supported documents openable", async () => {
    const record = { version: 1, projectId: "a".repeat(64), summary: "Workspace update", reason: "Updated code and guide", documents: [
      { path: "apps/studio/src/components/work-overview.tsx", title: "Workspace source", kind: "file", action: "updated" },
      { path: "docs/guide.MD", title: "Workspace guide", kind: "file", action: "updated" },
    ] };
    const onOpenDocument = vi.fn();
    render(<NodeDeliveries nodeId="docs" executionId="run" events={[event]} openEvidence={async () => ({ ...evidence, content: JSON.stringify({ description: JSON.stringify(record) }) })} onOpenDocument={onOpenDocument} />);
    expect(await screen.findByText("Workspace source")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Workspace source/ })).toBeNull();
    expect(screen.getByText("Source reference. Open this path in your code editor.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Workspace guide/ }));
    expect(onOpenDocument).toHaveBeenCalledOnce();
    expect(onOpenDocument).toHaveBeenCalledWith({ evidenceId: "evidence", index: 1, path: "docs/guide.MD", title: "Workspace guide", projectId: "a".repeat(64) });
  });
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
  it("renders reported work before documents and keeps non-passes visible", async () => {
    const workEvidence: EvidenceContent = { ...evidence, content: JSON.stringify({ description: JSON.stringify({ version: 1, projectId: "a".repeat(64), summary: "Work report", reason: "reason", work, documents: evidenceFrom(evidence).documents }) }) };
    render(<NodeDeliveries nodeId="docs" executionId="run" events={[event]} openEvidence={async () => workEvidence} onOpenDocument={vi.fn()} />);
    expect(await screen.findByText("Reported work")).toBeInTheDocument();
    expect(screen.getByText("reported-session")).toBeInTheDocument();
    expect(screen.getByText(/failed/)).toBeInTheDocument();
    expect(within(screen.getByText("browser journey").closest("li")!).getByText("unobserved")).toBeInTheDocument();
    expect(screen.getByText("No JPD candidate recorded. Certification is unobserved.")).toBeInTheDocument();
    expect(screen.getByText(/browser journey/)).toBeInTheDocument();
    expect(screen.getByText("Refund rule")).toBeInTheDocument();
  });
  it("rejects malformed work instead of partially rendering it", async () => {
    const malformed: EvidenceContent = { ...evidence, content: JSON.stringify({ description: JSON.stringify({ version: 1, projectId: "a".repeat(64), summary: "Bad", reason: "bad", work: { ...work, revision: "UPPERCASE" }, documents: [] }) }) };
    render(<NodeDeliveries nodeId="docs" executionId="run" events={[event]} openEvidence={async () => malformed} onOpenDocument={vi.fn()} />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByText("Bad")).not.toBeInTheDocument();
  });
  it("rejects malformed optional evidence even for a non-passing check", async () => {
    const malformedEvidence = { ...work, checks: [{ ...work.checks[1], evidence: { evidenceId: "bad", contentHash: "sha256:not-a-digest", size: 17_000_000 } }] };
    const record = { version: 1, projectId: "a".repeat(64), summary: "Bad evidence", reason: "bad", work: malformedEvidence, documents: [] };
    const malformed: EvidenceContent = { ...evidence, content: JSON.stringify({ description: JSON.stringify(record) }) };
    render(<NodeDeliveries nodeId="docs" executionId="run" events={[event]} openEvidence={async () => malformed} onOpenDocument={vi.fn()} />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByText("Bad evidence")).not.toBeInTheDocument();
  });
  // Protects candidate-vs-certificate rendering and malformed nested input; jsdom only, <1s.
  it("shows a bound JPD candidate with its missing authority and retry history", async () => {
    const record = { ...evidenceFrom(evidence), version: 1, projectId: "a".repeat(64), summary: "Candidate report", reason: "reason", work: { ...work, journeyVerification: journeyCandidate } };
    const content = { ...evidence, content: JSON.stringify({ description: JSON.stringify(record) }) };
    const unrelated = { ...event, actorId: "unrelated", payload: { kind: "operator_note" } };
    render(<NodeDeliveries nodeId="docs" executionId="run" events={[unrelated, { ...event, actorId: "scoped-reporter" }]} openEvidence={async () => content} onOpenDocument={vi.fn()} />);
    expect(await screen.findByText("JPD candidate (not certification)")).toBeInTheDocument();
    expect(screen.getByText(/jpd.registered-deterministic-validator/)).toBeInTheDocument();
    expect(screen.getByText(/Retry: first_pass_success/)).toBeInTheDocument();
    expect(screen.getByText(/Recorded reporter: scoped-reporter/)).toBeInTheDocument();
    expect(screen.getByText(/Review and merge receipts: not observed/)).toBeInTheDocument();
  });
  it("refuses a malformed JPD obligation without crashing the inspector", async () => {
    const record = { ...evidenceFrom(evidence), version: 1, projectId: "a".repeat(64), summary: "Malformed candidate", reason: "reason", work: { ...work, journeyVerification: { ...journeyCandidate, obligations: [null] } } };
    render(<NodeDeliveries nodeId="docs" executionId="run" events={[event]} openEvidence={async () => ({ ...evidence, content: JSON.stringify(record) })} onOpenDocument={vi.fn()} />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByText("Malformed candidate")).not.toBeInTheDocument();
  });
});

function evidenceFrom(value: EvidenceContent): { documents: Array<Record<string, unknown>> } {
  const envelope = JSON.parse(value.content) as { description: string };
  return JSON.parse(envelope.description) as { documents: Array<Record<string, unknown>> };
}
