import { describe, expect, it } from "vitest";

import { sameOriginLoopback } from "../../vite.config";

// #1264: the nonce-less session ask is answered only for this loopback origin's own pages.
describe("the nonce-less session gate", () => {
  const ok = { host: "127.0.0.1:5183", "sec-fetch-site": "same-origin" };
  it("accepts a same-origin loopback page", () => {
    expect(sameOriginLoopback(ok)).toBe(true);
    expect(sameOriginLoopback({ ...ok, origin: "http://127.0.0.1:5183" })).toBe(true);
  });
  it("refuses a foreign Origin, a cross-site fetch and a rebound Host", () => {
    expect(sameOriginLoopback({ ...ok, origin: "http://evil.example" })).toBe(false);
    expect(sameOriginLoopback({ ...ok, "sec-fetch-site": "cross-site" })).toBe(false);
    expect(sameOriginLoopback({ ...ok, host: "evil.example:5183" })).toBe(false);
    expect(sameOriginLoopback({ host: "127.0.0.1:5183" })).toBe(false);
  });
});
