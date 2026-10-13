import { act, createElement, Fragment, useLayoutEffect } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KeelChainView } from "./panel";
import type { EvidenceContent, RuntimeEvent } from "../runtime/types";

function signal(sequence: number, kind: string, severity = "low", evidenceRefs: string[] = []): RuntimeEvent {
  return {
    sequence, kind: "signal_recorded", occurredAt: null, actorId: null, actorType: null,
    idempotencyKey: null, eventId: null, evidenceRefs,
    payload: { kind, severity, signalId: `s${sequence}`, sourceKind: "node", sourceId: "implementation", executionId: "demo" },
  } as RuntimeEvent;
}

const evidence = (content: unknown, evidenceId = "e1"): EvidenceContent => ({
  evidenceId, mediaType: "application/json", sensitivity: "internal", contentSha256: "sha256:test",
  content: JSON.stringify(content),
});

let roots: Root[] = [];
afterEach(() => {
  for (const root of roots) act(() => root.unmount());
  roots = [];
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

describe("KeelChainView", () => {
  it("shows the chain and the skipped step in the node panel", () => {
    const html = renderToStaticMarkup(<KeelChainView nodeId="implementation" events={[signal(1, "jpd.journey"), signal(2, "keel.card")]} />);
    expect(html).toContain(`aria-label="Journey recorded"`);
    expect(html).toContain(`aria-label="Obligation missing"`);
    expect(html).toContain(`aria-label="Proof missing"`);
    expect(html).toContain("Obligation was skipped.");
    expect(html).toContain("Card has no proof yet.");
  });

  it("renders nothing without Keel signals", () => {
    expect(renderToStaticMarkup(<KeelChainView nodeId="implementation" events={[]} />)).toBe("");
  });

  it("opens this node's sealed check evidence and renders its head and finding IDs", async () => {
    const element = document.createElement("div");
    document.body.append(element);
    const root = createRoot(element);
    roots.push(root);
    const openEvidence = vi.fn(async (_executionId: string, _evidenceId: string) => evidence({
      head: "abc12345", findingIds: ["keel.scope.path_outside_card", "keel.invariant.persistence"],
      paths: ["apps/studio/src/runtime/keel-chain.ts"],
    }));
    await act(async () => root.render(createElement(KeelChainView, {
      nodeId: "implementation", executionId: "run-a", events: [signal(1, "keel.blocked", "medium", ["e1"])], openEvidence,
    })));
    await vi.waitFor(() => expect(element.textContent).toContain("abc12345"));
    expect(element.textContent).toContain("keel.scope.path_outside_card");
    expect(element.textContent).toContain("keel.invariant.persistence");
    expect(openEvidence).toHaveBeenCalledWith("run-a", "e1");
  });

  it("counts recorded passes without turning them into blocks or proof", async () => {
    const element = document.createElement("div");
    document.body.append(element);
    const root = createRoot(element);
    roots.push(root);
    await act(async () => root.render(createElement(KeelChainView, {
      nodeId: "implementation", executionId: "run-a", events: [signal(1, "keel.card"), signal(2, "keel.passed", "low", ["pass-evidence"])],
      openEvidence: async () => evidence({ head: "pass1234", findingIds: [], paths: [] }, "pass-evidence"),
    })));
    await vi.waitFor(() => expect(element.textContent).toContain("pass1234"));
    expect(element.textContent).toContain("Recorded check: passed");
    expect(element.textContent).toContain("Card has no proof yet.");
    expect(element.textContent).not.toContain("lock stopped");
  });

  it("keeps rejected and stale evidence unavailable", async () => {
    const element = document.createElement("div");
    document.body.append(element);
    const root = createRoot(element);
    roots.push(root);
    await act(async () => root.render(createElement(KeelChainView, {
      nodeId: "implementation", executionId: "run-a", events: [signal(1, "keel.blocked", "medium", ["bad"])],
      openEvidence: async () => { throw new Error("not available"); },
    })));
    await vi.waitFor(() => expect(element.textContent).toContain("Check evidence unavailable."));
    expect(element.textContent).not.toContain("finding:");
  });

  it("does not show an older execution's pending evidence after switching runs", async () => {
    const element = document.createElement("div");
    document.body.append(element);
    const root = createRoot(element);
    roots.push(root);
    let renderedDuringSwitch = "";
    function SwitchObserver({ executionId }: { executionId: string }) {
      useLayoutEffect(() => {
        if (executionId === "new-run") renderedDuringSwitch = element.textContent ?? "";
      }, [executionId]);
      return null;
    }
    const openOld = async () => evidence({ head: "oldhead1", findingIds: ["old.finding"], paths: [] }, "old-evidence");
    await act(async () => root.render(createElement(KeelChainView, {
      nodeId: "implementation", executionId: "old-run", events: [signal(1, "keel.blocked", "medium", ["old-evidence"])], openEvidence: openOld,
    })));
    await vi.waitFor(() => expect(element.textContent).toContain("oldhead1"));
    await act(async () => root.render(createElement(Fragment, null,
      createElement(KeelChainView, {
        nodeId: "implementation", executionId: "new-run", events: [signal(2, "keel.blocked", "medium", ["new-evidence"])],
        openEvidence: () => new Promise<EvidenceContent>(() => {}),
      }),
      createElement(SwitchObserver, { executionId: "new-run" }),
    )));
    expect(renderedDuringSwitch).not.toContain("oldhead1");
    expect(renderedDuringSwitch).not.toContain("old.finding");
    expect(element.textContent).not.toContain("oldhead1");
    expect(element.textContent).not.toContain("old.finding");
  });
});
