import { describe, expect, it } from "vitest";
import { ordinal, parseSlots, slotStatus, slotText, unavailableRoots } from "./slots";

const envelope = {
  ok: true,
  data: {
    slots: [
      { root: "D:/gh", holder: { lane: "a", label: "#1", pid: 7, worktree: null, heldSeconds: 240 },
        waiting: [
          { lane: "b", label: "x", pid: 8, worktree: "D:/w/b", priority: 0, waitedSeconds: 30, ticket: "t1" },
          { lane: "c", label: null, pid: 9, worktree: null, priority: 1, waitedSeconds: 720, ticket: "t2" },
        ] },
      { root: "E:/gh", error: [{ code: "workspace.slot_root_missing", message: "m" }] },
      { root: "F:/gh", holder: null, waiting: [] },
      { holder: null, waiting: [] },
      "junk",
    ],
  },
};

describe("parseSlots", () => {
  const slots = parseSlots(envelope);
  it("reads holder, waiters in order, null worktree", () => {
    expect(slots).toHaveLength(3);
    const s = slots[0]!;
    expect(s.ok && s.holder).toEqual({ lane: "a", label: "#1", pid: 7, worktree: null, heldSeconds: 240 });
    expect(s.ok && s.waiting.map((w) => w.lane)).toEqual(["b", "c"]);
  });
  it("an error entry is unavailable, never an empty queue", () => {
    expect(slots[1]).toEqual({ root: "E:/gh", ok: false, errorCodes: ["workspace.slot_root_missing"] });
    expect(unavailableRoots(slots)).toEqual(["E:/gh"]);
  });
  it("an idle slot is an empty queue", () => expect(slots[2]).toEqual({ root: "F:/gh", ok: true, holder: null, waiting: [] }));
  it("no slot roots and garbage parse to nothing", () => {
    expect(parseSlots({ slots: [] })).toEqual([]);
    expect(parseSlots(null)).toEqual([]);
    expect(parseSlots({ slots: "x" })).toEqual([]);
  });
});

describe("slotStatus", () => {
  const slots = parseSlots(envelope);
  it("building, waiting Nth, otherwise null", () => {
    expect(slotStatus("a", slots)).toEqual({ kind: "building", root: "D:/gh", seconds: 240 });
    expect(slotStatus("c", slots)).toEqual({ kind: "waiting", root: "D:/gh", position: 2, seconds: 720 });
    expect(slotStatus("z", slots)).toBeNull();
    expect(slotStatus(null, slots)).toBeNull();
  });
  it("text", () => {
    expect(slotText(slotStatus("a", slots)!)).toBe("building · 4m (D:/gh)");
    expect(slotText(slotStatus("c", slots)!)).toBe("waiting for build · 2nd · 12m");
    expect([1, 2, 3, 4, 11, 12, 21, 22].map(ordinal)).toEqual(["1st", "2nd", "3rd", "4th", "11th", "12th", "21st", "22nd"]);
  });
});
