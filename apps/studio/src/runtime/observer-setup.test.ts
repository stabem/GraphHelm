import { describe, expect, it } from "vitest";
import { observerSetupStatus } from "./observer-setup";

describe("observerSetupStatus (#1775)", () => {
  it("names each phase the Runtime reports in plain words", () => {
    expect(observerSetupStatus("Preparing the browser observer")).toBe("Preparing the browser for this run");
    expect(observerSetupStatus("npm install --save-dev @playwright/test@1.64.0")).toBe("Preparing the browser for this run · test tools");
    expect(observerSetupStatus("npx playwright install chromium")).toBe("Preparing the browser for this run · Chromium download");
    expect(observerSetupStatus("Writing the current browser observer")).toBe("Preparing the browser for this run · driver");
  });
  it("adds a percentage only when the installer printed one", () => {
    expect(observerSetupStatus("Downloading Chromium 140.0 - 42%")).toBe("Preparing the browser for this run · Chromium download · 42%");
    expect(observerSetupStatus("npx playwright install chromium")).not.toMatch(/%/);
    expect(observerSetupStatus("chromium 250%")).not.toMatch(/250/);
  });
});
