import { describe, expect, it } from "vitest";

import { forgetKey, SESSION_KEYS_STORAGE } from "./session";

const A = "a".repeat(64);
const B = "b".repeat(64);
const KEY = "c".repeat(32);

function store(initial: Record<string, string>) {
  const data = new Map([[SESSION_KEYS_STORAGE, JSON.stringify(initial)]]);
  return { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v), data };
}

describe("a refused token drops its stored key (#1897)", () => {
  it("removes that project's key and keeps the others", () => {
    const s = store({ [A]: KEY, [B]: KEY });
    forgetKey(A, s);
    expect(JSON.parse(s.data.get(SESSION_KEYS_STORAGE)!)).toEqual({ [B]: KEY });
  });

  it("leaves an empty store, so a reload reaches the connect screen", () => {
    const s = store({ [A]: KEY });
    forgetKey(A, s);
    expect(JSON.parse(s.data.get(SESSION_KEYS_STORAGE)!)).toEqual({});
  });
});
