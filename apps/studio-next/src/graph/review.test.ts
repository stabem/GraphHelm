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
    // #1777: a viewport's own frame, named only by the closed pair the Runtime serves.
    expect(isFrameUrl("/v1/journey-flows/checkout/screens/cart/frame?viewport=mobile")).toBe(true);
    expect(isFrameUrl("/v1/journey-flows/checkout/history/00000001760000000000/screens/cart/frame?viewport=web")).toBe(true);
    expect(isFrameUrl("/v1/journey-flows/checkout/screens/cart/frame?viewport=tablet")).toBe(false);
    expect(isFrameUrl("/v1/journey-flows/checkout/screens/cart/frame?viewport=mobile&x=1")).toBe(false);
    for (const url of ["https://example.com/v1/journey-flows/a/screens/b/frame", "//evil/v1/journey-flows/a/screens/b/frame",
      "/v1/journey-flows/../screens/b/frame", "/v1/journey-flows/a/history/123/screens/b/frame", "/v1/executions/x/events"]) {
      expect(isFrameUrl(url)).toBe(false);
    }
  });

  it("fetches a PR head's sealed frame only on the open run's own evidence route (#1867)", () => {
    const now = "/v1/executions/studio-v2/evidence/signal-gh-claude-20-issue-9-journey_frames-abc-image-1";
    expect(isFrameUrl(now, "studio-v2")).toBe(true);
    expect(isFrameUrl(now)).toBe(false); // no run named: never fetched
    expect(isFrameUrl(now, "other-run")).toBe(false); // another run's evidence
    for (const url of [
      "/v1/executions/studio-v2/evidence/signal-x-image-5", // a signal carries at most 4 images
      "/v1/executions/studio-v2/evidence/signal-x", // the sealed envelope, not a frame
      "/v1/executions/studio-v2/evidence/some-other-evidence",
      "/v1/executions/studio-v2/evidence/signal-x-image-1/../../events",
      "/v1/executions/studio-v2/evidence/signal-x-image-1?y=1",
      "https://example.com/v1/executions/studio-v2/evidence/signal-x-image-1",
      "/v1/executions/studio-v2/events",
    ]) {
      expect(isFrameUrl(url, "studio-v2")).toBe(false);
    }
    expect(isFrameUrl("/v1/executions/../evidence/signal-x-image-1", "..")).toBe(false); // the run id is checked too
  });

  it("shows Before alone until a Now frame exists, never a compare against nothing", () => {
    expect(modesOn(true, false)).toEqual({ frame: true, side: false, overlay: false, changes: false });
    expect(modesOn(false, false).frame).toBe(false);
    expect(modesOn(true, true)).toEqual({ frame: true, side: true, overlay: true, changes: true });
  });
});
