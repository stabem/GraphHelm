import { describe, expect, it, vi } from "vitest";

import { devSession, SESSION_KEY_STORAGE } from "./session";

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
    new Response(JSON.stringify({ ok: true, token: "fixture", project: "demo" }), {
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
    expect(session).toEqual({ token: "fixture", project: "demo", projectPath: null });
    expect(fetchImpl).toHaveBeenCalledWith(
      "/__studio/session?nonce=abc123",
      expect.objectContaining({ headers: { Accept: "application/json" } }),
    );
  });

  it("treats a refusal as no session, not as an error", async () => {
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({ ok: false }), { status: 404 }));
    expect(await devSession(fetchImpl as never, "?session=wrong")).toBeNull();
  });

  it("passes a known folder path to the Studio without treating it as a credential", async () => {
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({
      token: "fixture", project: "fixture-project", projectPath: "fixtures/project",
    }), { status: 200 }));
    expect(await devSession(fetchImpl as never, "?session=known")).toEqual({
      token: "fixture", project: "fixture-project", projectPath: "fixtures/project",
    });
  });

  it("accepts only bounded on-demand lane names from the local session", async () => {
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({ token: "fixture", onDemandLanes: ["codex-14", 5, ""] }), { status: 200 }));
    expect((await devSession(fetchImpl as never, "?session=known"))?.onDemandLanes).toEqual(["codex-14"]);
  });

  // #1264: F5 on a URL without `?session=` must not log the owner out while the token is unchanged.
  const memory = () => {
    const items = new Map<string, string>();
    return {
      getItem: (key: string) => items.get(key) ?? null,
      setItem: (key: string, value: string) => { items.set(key, value); },
      removeItem: (key: string) => { items.delete(key); },
      items,
    };
  };
  const key = "a".repeat(64);

  it("keeps the stable key the server hands out, never the token", async () => {
    const store = memory();
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({ token: "fixture", sessionKey: key }), { status: 200 }));
    await devSession(fetchImpl as never, "?session=launch", store);
    expect([...store.items.entries()]).toEqual([[SESSION_KEY_STORAGE, key]]);
  });

  it("reconnects a reload without ?session= from the remembered key", async () => {
    const store = memory();
    store.setItem(SESSION_KEY_STORAGE, key);
    const fetchImpl = vi.fn(async () => okReply());
    expect((await devSession(fetchImpl as never, "?run=zuvemo-v1-onda-1", store))?.token).toBe("fixture");
    expect(fetchImpl).toHaveBeenCalledWith(`/__studio/session?nonce=${key}`, expect.anything());
  });

  it("falls back to the remembered key when a stale launch nonce is refused", async () => {
    const store = memory();
    store.setItem(SESSION_KEY_STORAGE, key);
    const fetchImpl = vi.fn(async (url: string) => url.endsWith(key) ? okReply() : new Response("{}", { status: 404 }));
    expect((await devSession(fetchImpl as never, "?session=old", store))?.token).toBe("fixture");
  });

  it("keeps the key in this tab's sessionStorage, never in localStorage", async () => {
    localStorage.clear(); sessionStorage.clear();
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({ token: "fixture", sessionKey: key }), { status: 200 }));
    await devSession(fetchImpl as never, "?session=launch");
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.getItem(SESSION_KEY_STORAGE)).toBe(key);
    expect(JSON.stringify({ ...sessionStorage })).not.toContain("fixture");
    sessionStorage.clear();
  });

  it("shares one ask between two boots of the page, so a one-shot ticket is spent once", async () => {
    const fetchImpl = vi.fn(async () => okReply());
    const [first, second] = await Promise.all([
      devSession(fetchImpl as never, "?session=ticket", memory()),
      devSession(fetchImpl as never, "?session=ticket", memory()),
    ]);
    expect(first?.token).toBe("fixture");
    expect(second?.token).toBe("fixture");
    expect(fetchImpl).toHaveBeenCalledTimes(1);
  });

  it("forgets a remembered key the server refuses (token rotated) and shows the gate", async () => {
    const store = memory();
    store.setItem(SESSION_KEY_STORAGE, key);
    const fetchImpl = vi.fn(async () => new Response("{}", { status: 404 }));
    expect(await devSession(fetchImpl as never, "", store)).toBeNull();
    expect(store.items.size).toBe(0);
  });
});
