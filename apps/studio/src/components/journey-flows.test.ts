import { describe, expect, it } from "vitest";
import { refusalText } from "./journey-flows";

const refusal = (code: string) => Object.assign(new Error("the journey flow cannot be previewed"), { code });

describe("refusalText (#1448)", () => {
  // Observed on an isolated fixture: the Proof view read only
  // "the journey flow cannot be previewed (flow.scope_path_missing)." with no next step.
  it("tells the owner what to do when a screen's scope names a path the project lacks", () => {
    const text = refusalText(refusal("flow.scope_path_missing"));
    expect(text).toContain("(flow.scope_path_missing)");
    expect(text).toMatch(/scope/);
    expect(text).toMatch(/graphhelm journey validate/);
  });

  it("gives every other flow refusal the validator as its next step", () => {
    const text = refusalText(refusal("flow.schema_invalid"));
    expect(text).toContain("(flow.schema_invalid)");
    expect(text).toMatch(/graphhelm journey validate/);
  });

  it("keeps the known preview refusals' own step and adds nothing to an unknown code", () => {
    expect(refusalText(refusal("preview.busy"))).toContain("Wait for it to finish");
    expect(refusalText(refusal("other.code"))).toBe("the journey flow cannot be previewed (other.code).");
  });
});
