import { describe, expect, it } from "vitest";
import { observerSetupStatus } from "./observer-setup";

describe("observerSetupStatus (#1775)", () => {
  it("names each phase the Runtime reports in plain words", () => {
    expect(observerSetupStatus("Preparing the browser observer")).toBe("Preparing the browser");
    expect(observerSetupStatus("npm install --save-dev @playwright/test@1.64.0")).toBe("Preparing the browser · Installing test tools");
    expect(observerSetupStatus("npx playwright install chromium")).toBe("Preparing the browser · Downloading Chromium");
    expect(observerSetupStatus("Writing the current browser observer")).toBe("Preparing the browser · Writing the driver");
  });
  it("adds a percentage only when the installer printed one", () => {
    expect(observerSetupStatus("Downloading Chromium 141.0 - 101.2 MiB / 163.4 MiB 62%")).toBe("Preparing the browser · Downloading Chromium 62%");
    expect(observerSetupStatus("npx playwright install chromium")).not.toMatch(/%/);
    expect(observerSetupStatus("chromium 250%")).not.toMatch(/250/);
  });
});
