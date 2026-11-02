import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { displayFolder, teamRunOf } from "./dev-session";

describe("displayFolder", () => {
  it("shows the home as ~ and only at a path boundary", () => {
    expect(displayFolder("/home/ana/code/zuvemo", "/home/ana")).toBe("~/code/zuvemo");
    expect(displayFolder("/home/anabel/code", "/home/ana")).toBe("/home/anabel/code");
    expect(displayFolder(String.raw`\\?\C:\Users\ana\code\x`, String.raw`C:\Users\ana`)).toBe("~/code/x");
  });
});

describe("teamRunOf (#1901)", () => {
  it("reads only a well-formed executionId from the project's team.json", () => {
    const root = mkdtempSync(join(tmpdir(), "studio-team-"));
    try {
      expect(teamRunOf(root)).toBeNull();
      mkdirSync(join(root, ".graphhelm"));
      writeFileSync(join(root, ".graphhelm", "team.json"), JSON.stringify({ executionId: "gh-team", tokenFile: "secret" }));
      expect(teamRunOf(root)).toBe("gh-team");
      writeFileSync(join(root, ".graphhelm", "team.json"), JSON.stringify({ executionId: "../x y" }));
      expect(teamRunOf(root)).toBeNull();
      writeFileSync(join(root, ".graphhelm", "team.json"), "not json");
      expect(teamRunOf(root)).toBeNull();
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
    expect(teamRunOf(null)).toBeNull();
  });
});
