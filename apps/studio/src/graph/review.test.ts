import { describe, expect, it } from "vitest";

import { isFrameUrl, modesOn, reviewable } from "./review";

// #1802: when the review opens, which frame URLs get the bearer, and which compare modes are on.
describe("the Graph review screen", () => {
  it("opens only from an In review or Merged card that has a pull request", () => {
    expect(reviewable("review", 12)).toBe(true);
    expect(reviewable("merged", 12)).toBe(true);
    for (const column of ["ready", "implementing", "proven"] as const) expect(reviewable(column, 12)).toBe(false);
    expect(reviewable("review", null)).toBe(false);
  });

  it("fetches only the Runtime's own journey frame routes", () => {
    expect(isFrameUrl("/v1/journey-flows/checkout/screens/cart/frame")).toBe(true);
    expect(isFrameUrl("/v1/journey-flows/checkout/history/00000001760000000000/screens/cart/frame")).toBe(true);
    for (const url of ["https://example.com/v1/journey-flows/a/screens/b/frame", "//evil/v1/journey-flows/a/screens/b/frame",
      "/v1/journey-flows/../screens/b/frame", "/v1/journey-flows/a/history/123/screens/b/frame", "/v1/executions/x/events"]) {
      expect(isFrameUrl(url)).toBe(false);
    }
  });

  it("shows Before alone until a Now frame exists, never a compare against nothing", () => {
    expect(modesOn(true, false)).toEqual({ frame: true, side: false, overlay: false, changes: false });
    expect(modesOn(false, false).frame).toBe(false);
    expect(modesOn(true, true)).toEqual({ frame: true, side: true, overlay: true, changes: true });
  });
});
