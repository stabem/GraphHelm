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
