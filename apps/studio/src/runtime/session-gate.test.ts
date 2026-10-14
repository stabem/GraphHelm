import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { authorized, consumeTicket, stableNonce } from "../../vite.config";

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
  it("accepts the studio-open ticket once, then deletes it", () => {
    const path = join(mkdtempSync(join(tmpdir(), "gh-ticket-")), "events.studio-ticket");
    writeFileSync(path, "t".repeat(32));
    expect(consumeTicket(path, "wrong")).toBe(false);
    expect(existsSync(path)).toBe(true);
    expect(consumeTicket(path, "t".repeat(32))).toBe(true);
    expect(existsSync(path)).toBe(false);
    expect(consumeTicket(path, "t".repeat(32))).toBe(false);
    expect(consumeTicket(null, "x")).toBe(false);
  });
});
