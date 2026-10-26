import { describe, expect, it } from "vitest";

import { rereadOnReturn } from "./reread-on-return";

describe("a Studio left open reads the journeys again on return (#1732)", () => {
  it("reads on focus and on turning visible, never while hidden, and stops after cleanup", () => {
    const win = new EventTarget();
    const doc = Object.assign(new EventTarget(), { visibilityState: "hidden" });
    let reads = 0;
    const stop = rereadOnReturn(win, doc, () => { reads += 1; });
    doc.dispatchEvent(new Event("visibilitychange"));
    expect(reads).toBe(0);
    doc.visibilityState = "visible";
    doc.dispatchEvent(new Event("visibilitychange"));
    win.dispatchEvent(new Event("focus"));
    expect(reads).toBe(2);
    stop();
    win.dispatchEvent(new Event("focus"));
    doc.dispatchEvent(new Event("visibilitychange"));
    expect(reads).toBe(2);
  });
});
