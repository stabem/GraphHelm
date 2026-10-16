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

  // #1286: one missing heading must not paint the elements the screen does show; each assertion
  // reads its own verdict and the failed one says what the screen showed instead.
  it("judges each assertion on its own and names what the screen showed (#1286)", () => {
    const three = { ...flow, screens: [flow.screens[0]!, { ...flow.screens[1]!, expect: [
      { role: "heading", name: "Produto encontrado" }, { role: "region", name: "Também encontramos em" }, { role: "button", name: "Salvar item" }] }] };
    const checks = [
      { role: "heading", name: "Produto encontrado", seen: false, closest: { role: "heading", name: "Produto conferido" } as { role: string; name: string } | null },
      { role: "region", name: "Também encontramos em", seen: true, closest: null },
      { role: "button", name: "Salvar item", seen: true, closest: null }];
    const runWith = (judged: typeof checks) => ({ state: "failed", screens: { lojas: { frame: false, result: "pass" },
      salvo: { frame: false, result: "drift", reason: "expect_missing", checks: judged } } }) as never;
    const [, failed] = testFrames(journey, runWith(checks), null, three);
    expect(failed!.checkStatus).toEqual(["failed", "passed", "passed"]);
    expect(failed!.failure).toBe("Expected heading 'Produto encontrado' — the screen shows heading 'Produto conferido'");
    const none = runWith([{ ...checks[0]!, closest: null }, checks[1]!, checks[2]!]);
    expect(testFrames(journey, none, null, three)[1]!.failure).toBe("Expected heading 'Produto encontrado' — the screen shows no heading");
  });

  it("an old run without checks: passed stays seen, anything else is not evaluated (#1286)", () => {
    const run = { state: "failed", screens: { lojas: { frame: false, result: "pass" }, salvo: { frame: false, result: "drift" } } } as never;
    const [passed, drifted] = testFrames(journey, run, null, flow);
    expect(passed!.checkStatus).toEqual(["passed"]);
    expect(drifted!.checkStatus).toEqual(["not_evaluated"]);
    expect(drifted!.failure).toBeNull();
  });

  it("captions each typed value of the entering edge, secrets by name only (#1286)", () => {
    const [first, second] = testFrames(journey, null, null, flow);
    expect(first!.typed).toEqual([]);
    expect(second!.typed).toEqual(['Typed "parceiro" into "Tag Amazon"', 'Typed the secret admin into "Senha"']);
  });

  it("carries the screen's own intent, null when the flow does not say (#1286)", () => {
    const said = { ...flow, screens: [flow.screens[0]!, { ...flow.screens[1]!, intent: "Confirma que a tag ficou salva" }] };
    const [first, second] = testFrames(journey, null, null, said);
    expect(first!.screenIntent).toBeNull();
    expect(second!.screenIntent).toBe("Confirma que a tag ficou salva");
  });
});
