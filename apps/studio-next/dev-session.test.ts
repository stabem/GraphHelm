import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough } from "node:stream";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { devSession, displayFolder, stableKey } from "./dev-session";

type Handler = (request: PassThrough & { method: string; url: string; headers: Record<string, string> }, response: FakeResponse, next: () => void) => void;
interface FakeResponse { statusCode: number; body: string; setHeader: () => void; end: (body?: string) => void; done: Promise<void> }

function project(root: string, name: string, token: string): string {
  const folder = join(root, name);
  mkdirSync(join(folder, ".graphhelm"), { recursive: true });
  writeFileSync(join(folder, ".graphhelm", "events.token"), token);
  writeFileSync(join(folder, ".graphhelm", "events.runtime-lifecycle.json"), JSON.stringify({ state: "running", address: "127.0.0.1:1" }));
  return folder;
}

describe("dev server add-folder (#1833)", () => {
  let root: string;
  let routes: Map<string, Handler>;
  const logged: string[] = [];

  beforeEach(() => {
    root = mkdtempSync(join(tmpdir(), "studio-next-add-"));
    const first = project(root, "first", "first-token");
    routes = new Map();
    const logger = { info: (line: string) => logged.push(line), warn: (line: string) => logged.push(line), error: (line: string) => logged.push(line) };
    for (const level of ["log", "info", "warn", "error"] as const) vi.spyOn(console, level).mockImplementation((...args) => { logged.push(args.join(" ")); });
    const plugin = devSession({ GRAPHHELM_EVENTS: join(first, ".graphhelm", "events"), GRAPHHELM_STUDIO_SESSION_NONCE: "n" });
    (plugin.configureServer as (server: unknown) => void)({
      config: { logger },
      httpServer: null,
      middlewares: { use: (path: string, handler: Handler) => routes.set(path, handler) },
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    logged.length = 0;
    rmSync(root, { recursive: true, force: true });
  });

  async function call(path: string, method: string, headers: Record<string, string>, body?: unknown): Promise<{ status: number; body: Record<string, unknown> }> {
    const [route, query = ""] = path.split("?");
    const request = Object.assign(new PassThrough(), { method, url: query ? `/?${query}` : "/", headers });
    let finish!: () => void;
    const response: FakeResponse = {
      statusCode: 200, body: "", setHeader: () => {}, done: new Promise((resolve) => { finish = resolve; }),
      end(text = "") { this.body = text; finish(); },
    };
    routes.get(route!)!(request, response, () => {});
    request.end(body === undefined ? undefined : JSON.stringify(body));
    await response.done;
    return { status: response.statusCode, body: JSON.parse(response.body) as Record<string, unknown> };
  }

  it("adds a folder once for the owner, and never logs or echoes it", async () => {
    const second = project(root, "second", "second-token");
    const owner = { "x-graphhelm-studio-key": stableKey("first-token") };
    const first = await call("/__studio/add", "POST", owner, { path: second });
    const again = await call("/__studio/add", "POST", owner, { path: second });
    expect(first.status).toBe(200);
    expect(again.body.projectId).toBe(first.body.projectId);
    expect(first.body.sessionKey).toBe(stableKey("second-token"));
    const identity = await call("/__studio/identity", "GET", {});
    expect((identity.body.projectIds as string[]).filter((id) => id === first.body.projectId)).toHaveLength(1);
    const missing = await call("/__studio/add", "POST", owner, { path: join(root, "nowhere") });
    expect(missing).toEqual({ status: 422, body: { ok: false, reason: "not_a_project" } });
    expect(logged.join("\n")).not.toContain("second");
    expect(logged.join("\n")).not.toContain("nowhere");
  });

  it("refuses a caller without an owner key, before reading the folder", async () => {
    const second = project(root, "second", "second-token");
    for (const headers of [{} as Record<string, string>, { "x-graphhelm-studio-key": stableKey("other-token") }, { "x-graphhelm-studio-key": "first-token" }]) {
      expect(await call("/__studio/add", "POST", headers, { path: second })).toEqual({ status: 404, body: { ok: false } });
    }
    expect((await call("/__studio/identity", "GET", {})).body.projectIds).toHaveLength(1);
  });

  it("answers the folder only inside a session reply", async () => {
    const session = await call("/__studio/session?nonce=n", "GET", {});
    expect(typeof session.body.folder).toBe("string");
    expect(String(session.body.folder).endsWith("/first")).toBe(true);
  });
});

describe("displayFolder", () => {
  it("shows the home as ~ and only at a path boundary", () => {
    expect(displayFolder("/home/ana/code/zuvemo", "/home/ana")).toBe("~/code/zuvemo");
    expect(displayFolder("/home/anabel/code", "/home/ana")).toBe("/home/anabel/code");
    expect(displayFolder(String.raw`\\?\C:\Users\ana\code\x`, String.raw`C:\Users\ana`)).toBe("~/code/x");
  });
});
