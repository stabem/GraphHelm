import { describe, expect, it } from "vitest";

import { yamlLines, yamlText } from "./yaml-view";

const text = (raw: string) => yamlLines(raw)?.map(yamlText);

describe("stdout as YAML-like lines (#1914)", () => {
  it("indents nested objects and lists arrays as - items", () => {
    expect(text('{"ok":true,"n":3,"who":"a b","gone":null,"scope":{"paths":["a.ts","b.ts"],"deep":{"x":1}},"list":[{"id":1,"t":"x"},2]}')).toEqual([
      "ok: true", "n: 3", "who: a b", "gone: null",
      "scope:", "  paths:", "    - a.ts", "    - b.ts", "  deep:", "    x: 1",
      "list:", "  - id: 1", "    t: x", "  - 2",
    ]);
  });

  it("lists a top-level array and empty containers", () => {
    expect(text('[1,{"a":[]}]')).toEqual(["- 1", "- a: []"]);
  });

  it("types each scalar for the colours", () => {
    expect(yamlLines('{"a":"s","b":1,"c":false}')!.map((line) => [line.key, line.scalar!.kind])).toEqual([["a", "string"], ["b", "number"], ["c", "boolean"]]);
  });

  it("is null for plain text, bare scalars and broken JSON", () => {
    expect(yamlLines("hello\n")).toBeNull();
    expect(yamlLines("42")).toBeNull();
    expect(yamlLines("{oops")).toBeNull();
    expect(yamlLines("")).toBeNull();
  });

  it("caps the depth and never throws on a 10000-deep input", () => {
    const deep = "[".repeat(10000) + "]".repeat(10000);
    const lines = yamlLines(deep);
    expect(lines === null || lines.length < 200).toBe(true);
    const nested = '{"a":'.repeat(10000) + "1" + "}".repeat(10000);
    const keyed = yamlLines(nested);
    expect(keyed === null || keyed.some((line) => line.scalar?.text === "…")).toBe(true);
  });
});
