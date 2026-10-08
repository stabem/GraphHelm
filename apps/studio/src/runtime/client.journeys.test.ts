import { describe, expect, it } from "vitest";

import { DisconnectedError, RuntimeClient, RuntimeError } from "./client";

interface Seen { url: string; headers: Record<string, string> }

function client(reply: () => Response) {
  const seen: Seen[] = [];
  const fetchImpl = (async (url: string, init: RequestInit = {}) => {
    seen.push({ url, headers: (init.headers ?? {}) as Record<string, string> });
    return reply();
  }) as unknown as typeof fetch;
  return { seen, runtime: new RuntimeClient("secret-token", { fetch: fetchImpl }) };
}
const json = (data: unknown, status = 200) =>
  new Response(JSON.stringify({ ok: status < 400, data, diagnostics: [] }), { status, headers: { "content-type": "application/json" } });
const bytes = (type: string, status = 200) => new Response(new Uint8Array([1, 2, 3]), { status, headers: { "content-type": type } });

describe("RuntimeClient.journeys", () => {
  it("reads the typed view from the journeys route", async () => {
    const view = { head: "abc", journeys: [{ contractId: "c", title: "C", steps: [], arrows: [] }] };
    const { seen, runtime } = client(() => json(view));
    await expect(runtime.journeys("run-1")).resolves.toEqual(view);
    expect(seen[0].url).toBe("/v1/executions/run-1/journeys");
    expect(seen[0].headers.Authorization).toBe("Bearer secret-token");
  });
  it("reads the project-level map from GET /v1/journeys when no run is named (#332)", async () => {
    const view = { head: "abc", journeys: [] };
    const { seen, runtime } = client(() => json(view));
    await expect(runtime.journeys()).resolves.toEqual(view);
    expect(seen[0].url).toBe("/v1/journeys");
    expect(seen[0].headers.Authorization).toBe("Bearer secret-token");
  });
  it("refuses an empty id", async () => {
    await expect(client(() => json({})).runtime.journeys("")).rejects.toBeInstanceOf(RuntimeError);
  });
});

// #353: the Journey tab's flow review reads and approves through the public routes only.
describe("RuntimeClient journey flows", () => {
  it("lists flows from GET /v1/journey-flows", async () => {
    const view = { flows: [] };
    const { seen, runtime } = client(() => json(view));
    await expect(runtime.journeyFlows()).resolves.toEqual(view);
    expect(seen[0].url).toBe("/v1/journey-flows");
  });
  it("approves through POST /v1/journey-flows/{id}/approve and surfaces a refusal", async () => {
    const { seen, runtime } = client(() => json({ id: "a b", status: "approved" }));
    await expect(runtime.approveJourneyFlow("a b")).resolves.toEqual({ id: "a b", status: "approved" });
    expect(seen[0].url).toBe("/v1/journey-flows/a%20b/approve");
    const refused = client(() => new Response(JSON.stringify({ ok: false, data: { files: [] },
      diagnostics: [{ code: "GHCLI034_JOURNEY_FLOW_INVALID", message: "scope does not exist", path: "/screens/0/scope/0", severity: "error" }] }),
      { status: 400, headers: { "content-type": "application/json" } }));
    await expect(refused.runtime.approveJourneyFlow("broken")).rejects.toBeInstanceOf(RuntimeError);
    await expect(client(() => json({})).runtime.approveJourneyFlow("")).rejects.toBeInstanceOf(RuntimeError);
  });
});

describe("RuntimeClient.readImage", () => {
  it.each(["image/png", "image/jpeg", "image/webp"])("returns a blob for %s", async (type) => {
    const { seen, runtime } = client(() => bytes(type));
    const blob = await runtime.readImage("run 1", "img/1");
    expect(blob.size).toBe(3);
    expect(seen[0].url).toBe("/v1/executions/run%201/evidence/img%2F1");
    expect(seen[0].headers.Authorization).toBe("Bearer secret-token");
  });
  it("refuses a non-image content type", async () => {
    await expect(client(() => json({ text: "x" })).runtime.readImage("r", "e")).rejects.toThrow(/image/);
    await expect(client(() => bytes("image/svg+xml")).runtime.readImage("r", "e")).rejects.toBeInstanceOf(RuntimeError);
  });
  it("refuses a 401 without leaking the token", async () => {
    const error = await client(() => bytes("image/png", 401)).runtime.readImage("r", "e").catch((e) => e);
    expect(error).toBeInstanceOf(RuntimeError);
    expect(error.httpStatus).toBe(401);
    expect(String(error.message)).not.toContain("secret-token");
  });
  it("throws DisconnectedError once disposed", async () => {
    const { runtime } = client(() => bytes("image/png"));
    runtime.dispose();
    await expect(runtime.readImage("r", "e")).rejects.toBeInstanceOf(DisconnectedError);
  });
});

// #409 (journey-first spec §5 points 4-5): Open live, act, close and the session list go through
// the public routes phase C (#398) ships, and nothing else. A refusal surfaces the Runtime's own
// message; the Studio never fabricates a session from the open reply.
describe("RuntimeClient live journey sessions", () => {
  it("lists sessions from GET /v1/journeys/sessions", async () => {
    const data = { sessions: [{ sessionId: "s-1", contractId: "checkout", flowId: "checkout", path: "main", stepId: "pay", state: "pass", code: null, at: "pay", screen: null, since: "2026-10-08T03:00:00Z", lastActAt: null, expiresAt: null }] };
    const { seen, runtime } = client(() => json(data));
    await expect(runtime.liveSessions()).resolves.toEqual(data);
    expect(seen[0].url).toBe("/v1/journeys/sessions");
  });
  it("opens through POST /v1/journeys/{contractId}/open with the step, path and run", async () => {
    const { seen, runtime } = client(() => json({ sessionId: "s-1", state: "pass" }));
    await expect(runtime.openLive("checkout", { stepId: "pay", path: "main", executionId: "run-1" })).resolves.toMatchObject({ sessionId: "s-1" });
    expect(seen[0].url).toBe("/v1/journeys/checkout/open");
    await expect(runtime.openLive("../x", { stepId: "pay" })).rejects.toBeInstanceOf(RuntimeError);
  });
  it("acts through POST /v1/journeys/sessions/{id}/act and closes through DELETE", async () => {
    const { seen, runtime } = client(() => json({ sessionId: "s-1", state: "pass", code: null }));
    await runtime.actLive("s-1", { kind: "activate", role: "button", name: "Pay now" });
    expect(seen[0].url).toBe("/v1/journeys/sessions/s-1/act");
    const closed = client(() => json({ sessionId: "s-1", closed: true }));
    await expect(closed.runtime.closeLive("s-1")).resolves.toEqual({ sessionId: "s-1", closed: true });
    expect(closed.seen[0].url).toBe("/v1/journeys/sessions/s-1");
  });
  it("surfaces a refusal (drift, destructive act, gone session) as the Runtime's message", async () => {
    const refusal = new Response(JSON.stringify({ ok: false, data: { sessionId: "s-1", state: "drift" }, diagnostics: [{ code: "drift.locator_missing", severity: "error", message: "the Checkout button is gone", path: "/paths/main/edges/cart.checkout/acts/0", source: "graphhelm" }] }), { status: 409, headers: { "content-type": "application/json" } });
    await expect(client(() => refusal).runtime.openLive("checkout", { stepId: "pay" })).rejects.toThrow(/Checkout button is gone/);
  });
});
