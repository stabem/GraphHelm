import { describe, expect, it, vi } from "vitest";

import { devSession } from "./session";

/**
 * The session ask carries the page's nonce, or does not happen at all.
 *
 * The dev server hands the Runtime's bearer token ONLY to a caller presenting the per-run nonce
 * (the token file is owner-only on disk, and loopback is not a user boundary - PR #467 review).
 * The page's half of that contract is here: no nonce in the URL, no request; a nonce present
 * rides the ask.
 */
describe("the dev session", () => {
  const okReply = () =>
    new Response(JSON.stringify({ ok: true, token: "local-token", project: "demo" }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });

  it("never asks without a nonce - the connect gate is the answer", async () => {
    const fetchImpl = vi.fn(async () => okReply());
    expect(await devSession(fetchImpl as never, "")).toBeNull();
    expect(await devSession(fetchImpl as never, "?other=1")).toBeNull();
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("presents the page's own nonce to the endpoint", async () => {
    const fetchImpl = vi.fn(async () => okReply());
    const session = await devSession(fetchImpl as never, "?session=abc123");
    expect(session).toEqual({ token: "local-token", project: "demo" });
    expect(fetchImpl).toHaveBeenCalledWith(
      "/__studio/session?nonce=abc123",
      expect.objectContaining({ headers: { Accept: "application/json" } }),
    );
  });

  it("treats a refusal as no session, not as an error", async () => {
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({ ok: false }), { status: 404 }));
    expect(await devSession(fetchImpl as never, "?session=wrong")).toBeNull();
  });
});
