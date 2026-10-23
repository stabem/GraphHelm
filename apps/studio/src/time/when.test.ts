import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { when } from "./when";

// A fixed clock in a fixed zone: the stamps below are UTC, the screen reads them in São Paulo (UTC-3).
const env = (globalThis as unknown as { process: { env: Record<string, string | undefined> } }).process.env;
let zone: string | undefined;
beforeAll(() => { zone = env.TZ; env.TZ = "America/Sao_Paulo"; });
afterAll(() => { if (zone === undefined) delete env.TZ; else env.TZ = zone; });

const now = new Date("2026-10-23T15:30:00Z"); // 12:30 local

describe("when (#1934)", () => {
  it("reads minutes under an hour", () => {
    expect(when("2026-10-23T15:28:00Z", now)).toEqual({ text: "2 min ago", full: "2026-10-23 12:28:00 local" });
    expect(when("2026-10-23T15:29:50Z", now).text).toBe("just now");
  });
  it("reads the local clock earlier today", () => {
    expect(when("2026-10-23T03:02:08.470987300+00:00", now)).toEqual({ text: "00:02", full: "2026-10-23 00:02:08 local" });
  });
  it("reads day, month and time this year, by the local date", () => {
    // 02:00 UTC on the 23rd is still the 22nd in São Paulo.
    expect(when("2026-10-23T02:00:00Z", now).text).toBe("22 Oct 23:00");
    expect(when("2026-01-05T12:00:00Z", now).text).toBe("5 Jan 09:00");
  });
  it("reads day, month and year for an older stamp", () => {
    expect(when("2025-10-23T03:02:08Z", now).text).toBe("23 Oct 2025");
  });
  it("shows a dash, never the raw string, when the time is missing or unreadable", () => {
    expect(when(null, now)).toEqual({ text: "—", full: "" });
    expect(when("not a time", now)).toEqual({ text: "—", full: "" });
  });
  it("a time ahead of the clock is not an age", () => {
    expect(when("2026-10-23T15:40:00Z", now).text).toBe("12:40");
  });
});
