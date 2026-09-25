import { beforeEach, describe, expect, it } from "vitest";

import {
  loadProjectName,
  loadRemovedRuns,
  saveProjectName,
  saveRemovedRuns,
  validProjectName,
} from "./studio-preferences";

describe("studio browser preferences", () => {
  beforeEach(() => window.localStorage.clear());

  it("bounds project names and rejects control characters", () => {
    expect(validProjectName("  Dale  ")).toBe("Dale");
    expect(validProjectName(" ")).toBeNull();
    expect(validProjectName("bad\nname")).toBeNull();
    expect(validProjectName("x".repeat(121))).toBeNull();
  });

  it("persists a project display name per runtime", () => {
    expect(loadProjectName("runtime-a", "Default")).toBe("Default");
    expect(saveProjectName("runtime-a", "  Dale  ")).toBe(true);
    expect(loadProjectName("runtime-a", "Default")).toBe("Dale");
    expect(loadProjectName("runtime-b", "Default")).toBe("Default");
  });

  it("persists removed run ids without accepting unbounded or invalid data", () => {
    expect(saveRemovedRuns("runtime-a", ["run-1", "", "run-1", "x".repeat(300)])).toBe(true);
    expect(loadRemovedRuns("runtime-a")).toEqual(["run-1"]);
  });
});
