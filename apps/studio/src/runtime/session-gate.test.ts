import { describe, expect, it } from "vitest";

import { authorized, stableNonce } from "../../vite.config";

// #1264: the dev session endpoint trusts only a nonce, never request headers.
describe("the dev session gate", () => {
  it("refuses a request with no nonce, so a forged Sec-Fetch-Site/Origin curl earns nothing", () => {
    expect(authorized("", "launch", "tok")).toBe(false);
  });
  it("accepts the launch nonce or the token-derived key, and nothing else", () => {
    expect(authorized("launch", "launch", "tok")).toBe(true);
    expect(authorized(stableNonce("tok"), "launch", "tok")).toBe(true);
    expect(authorized(stableNonce("old"), "launch", "tok")).toBe(false);
  });
  it("derives the same key as `graphhelm studio open`", () => {
    expect(stableNonce("fixture-token")).toBe("cce0290a9af90e0522bf0de80226e89b4404b2014ee57a6dd316ffd0c68d5bdf");
  });
});
