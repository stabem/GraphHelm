import { describe, expect, it } from "vitest";
import { testFrames } from "./test-frames";
import type { JourneyFlowView, JourneyView } from "./types";

// #1287: a frame reads the flow's own words: the acts that entered its screen (typed text
// included), the edge intent, and every element the screen must show.
describe("testFrames with the flow source", () => {
  const journey: JourneyView = { contractId: "lojas", title: "Tag stores", arrows: [], steps: [
    { stepId: "lojas", promises: [], action: { kind: "navigate", target: "/admin/lojas", strategy: "url" }, expectedStates: ["stable"] },
    { stepId: "salvo", promises: [], action: { kind: "submit", target: "Salvar", strategy: "accessible_name" }, expectedStates: ["success"] },
  ] };
  const flow = { id: "lojas", title: "Tag stores", purpose: "Tags stay after a reload.", status: "draft", approved: null, drift: [], findings: [], approvable: false,
    screens: [{ id: "lojas", url: "/admin/lojas", state: "stable", expect: [{ role: "button", name: "Salvar" }] },
      { id: "salvo", url: "/admin/lojas", state: "success", expect: [{ role: "cell", name: "parceiro" }] }],
    edges: [{ id: "lojas.salvar", from: "lojas", to: "salvo", intent: "Tag Amazon", acts: [
      { kind: "enter_text", role: "textbox", name: "Tag Amazon", text: "parceiro" },
      { kind: "enter_text", role: "textbox", name: "Senha", secret: "admin" },
      { kind: "submit", role: "button", name: "Salvar" }] }],
    paths: { main: ["lojas.salvar"] } } satisfies JourneyFlowView;

  it("names the inputs, intent and checks of each step", () => {
    const [first, second] = testFrames(journey, null, null, flow);
    expect(first).toMatchObject({ inputs: [], intent: null, checks: ["shows button “Salvar”"] });
    expect(second).toMatchObject({
      inputs: ["Types “parceiro” into “Tag Amazon”", "Types the secret admin into “Senha”", "Submits “Salvar”"],
      intent: "Tag Amazon", checks: ["shows cell “parceiro”"],
    });
  });

  it("without the flow, a frame has no inputs or checks", () => {
    expect(testFrames(journey, null)[1]).toMatchObject({ inputs: [], intent: null, checks: [] });
  });

  it("a weak flow's cached replay is never shown as passed (#1287)", () => {
    const run = { state: "done", kind: "replay", screens: { lojas: { result: "pass" }, salvo: { result: "pass" } } } as never;
    expect(testFrames(journey, run, null, flow).map((f) => f.status)).toEqual(["passed", "passed"]);
    const weak = testFrames(journey, run, null, { ...flow, status: "weak", approved: null });
    expect(weak.map((f) => f.status)).toEqual(["not_run", "not_run"]);
    expect(weak[0]!.reason).toMatch(/does not say what it proves/);
  });
});
