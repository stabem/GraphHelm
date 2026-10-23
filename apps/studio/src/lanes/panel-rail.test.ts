import { describe, expect, it } from "vitest";

import { PANEL_KEY, readCollapsed, writeCollapsed } from "./panel-rail";

const memory = () => {
  const rows = new Map<string, string>();
  return { getItem: (key: string) => rows.get(key) ?? null, setItem: (key: string, value: string) => void rows.set(key, value), rows };
};

describe("agent panel rail (#1934)", () => {
  it("remembers a fold and an unfold for this viewer", () => {
    const store = memory();
    expect(readCollapsed(store)).toBe(false);
    writeCollapsed(true, store);
    expect(store.rows.get(PANEL_KEY)).toBe("collapsed");
    expect(readCollapsed(store)).toBe(true);
    writeCollapsed(false, store);
    expect(readCollapsed(store)).toBe(false);
  });

  it("a store that throws or is missing leaves the panel open and does not throw", () => {
    const broken = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
    expect(readCollapsed(broken)).toBe(false);
    expect(() => writeCollapsed(true, broken)).not.toThrow();
    expect(readCollapsed(null)).toBe(false);
  });
});
