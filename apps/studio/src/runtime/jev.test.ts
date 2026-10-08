import { describe, expect, it } from "vitest";
import { JEV_ABSENT_NOTE, jevAvailability } from "./jev";

/* #396 (spec §8): Jev absent is a state, not an error. With no judge route the chat shows no Jev
 * card and no Retry (nothing can be retried), and run details carries one line; the card returns
 * only for a configured route. This cell catches the old behaviour, a per-question error card
 * for a Runtime that simply has no Jev model. Cost: pure function, microseconds. */
describe("jevAvailability", () => {
  it("is checking until the routes are read, absent with no judge route, configured otherwise", () => {
    expect(jevAvailability(null)).toBe("checking");
    expect(jevAvailability(0)).toBe("absent");
    expect(jevAvailability(2)).toBe("configured");
  });

  it("names the one line run details shows when Jev is absent", () => {
    expect(JEV_ABSENT_NOTE).toBe("No Jev model: suggested replies are off (Models → Add model)");
  });
});
