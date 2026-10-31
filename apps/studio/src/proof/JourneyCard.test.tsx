import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { JourneyFlow } from "../runtime/client";
import { journeysToFix } from "../sidebar/project-summary";
import { JourneyCard } from "./ProofView";

const card = (flow: JourneyFlow) =>
  renderToStaticMarkup(<JourneyCard flow={flow} selected={false} disabled={false} onPick={() => {}} onRun={() => {}} run={null} playing={false} />);

describe("journey card findings (#2325)", () => {
  const flow: JourneyFlow = {
    id: "quickstart", title: "Client greets and lists items", status: "draft",
    lastRun: { status: "not_run", ranAt: null, kind: "replay", reason: null, failingStep: null, flowChanged: null } as never,
    findings: [
      { severity: "warning", code: "flow.unexplained", pointer: "/screens/0", message: "a warning" },
      { severity: "error", code: "flow.step_why_missing", pointer: "/edges/1/why", message: "say in the edge `why` what this step proves for the user" },
    ],
  };

  it("lists each finding with its code, where it is and what to write, errors first, apart from the run", () => {
    const html = card(flow);
    expect(html).toContain("flow.step_why_missing");
    expect(html).toContain("/edges/1/why");
    expect(html).toContain("say in the edge `why` what this step proves for the user");
    expect(html.indexOf("flow.step_why_missing")).toBeLessThan(html.indexOf("flow.unexplained"));
    expect(html).toContain("not run");
    expect(html).toContain(">1 to fix<");
    expect(html).toContain("chip fix");
    expect(html).not.toContain("failed");
  });

  it("shows nothing extra for a journey without findings", () => {
    expect(card({ ...flow, findings: [] })).not.toContain("journey-findings");
  });

  it("the rail's to-fix count equals the Proof cards with a to-fix chip, unreadable ones included (#2345)", () => {
    const flows: JourneyFlow[] = [
      flow,
      { ...flow, id: "warned", findings: [{ severity: "warning", code: "flow.unexplained" }] },
      { id: "broken", title: null, status: "unreadable", findings: [] },
      { id: "broken-no-list", title: null, status: "unreadable" },
      { ...flow, id: "clean", findings: [] },
    ];
    const chips = flows.filter((each) => card(each).includes("chip fix")).length;
    expect(journeysToFix(flows)).toBe(3);
    expect(chips).toBe(journeysToFix(flows));
    expect(card(flows[2])).toContain(">1 to fix<");
  });
});
