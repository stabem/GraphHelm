// @vitest-environment node
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { tmpdir } from "node:os";
import { join, toNamespacedPath } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";
import type { Connect, Plugin, UserConfigFnObject, ViteDevServer } from "vite";

import config, { authorized, consumeTicket, stableNonce } from "../../vite.config";

const projects: string[] = [];
afterEach(() => {
  vi.unstubAllEnvs();
  for (const project of projects.splice(0)) rmSync(project, { recursive: true, force: true });
});

function projectFixture(token: string) {
  const project = mkdtempSync(join(tmpdir(), "gh-studio-identity-"));
  projects.push(project);
  mkdirSync(join(project, ".graphhelm"));
  const tokenFile = join(project, ".graphhelm", "events.token");
  const ticketFile = join(project, ".graphhelm", "events.studio-ticket");
  writeFileSync(tokenFile, token);
  return { project, tokenFile, ticketFile };
}

function sessionRoutes(project: string, tokenFile = "") {
  vi.stubEnv("GRAPHHELM_PROJECT_PATH", project);
  vi.stubEnv("GRAPHHELM_EVENTS", project ? join(project, ".graphhelm", "events") : "");
  vi.stubEnv("GRAPHHELM_STUDIO_TOKEN_FILE", tokenFile);
  vi.stubEnv("GRAPHHELM_STUDIO_SESSION_NONCE", "fixture-launch-nonce");
  const resolved = (config as UserConfigFnObject)({ command: "serve", mode: "test" });
  const plugin = (resolved.plugins as Plugin[]).find((entry) => entry.name === "graphhelm-studio-dev-session");
  const configure = plugin?.configureServer;
  if (typeof configure !== "function") throw new Error("dev session middleware is missing");
  const routes = new Map<string, Connect.NextHandleFunction>();
  configure.call({} as ThisParameterType<typeof configure>, {
    middlewares: { use: (route: string, handler: Connect.NextHandleFunction) => routes.set(route, handler) },
  } as unknown as ViteDevServer);
  return (route: string, url = "/", method = "GET") => {
    const headers: Record<string, string> = {};
    let body = "";
    const response = {
      statusCode: 200,
      setHeader: (name: string, value: string) => { headers[name] = value; },
      end: (value: string) => { body = value; },
    };
    const handler = routes.get(route);
    if (!handler) throw new Error(`route missing: ${route}`);
    handler({ url, method } as IncomingMessage, response as unknown as ServerResponse, () => { throw new Error("route unexpectedly fell through"); });
    return { status: response.statusCode, headers, body, json: JSON.parse(body) as Record<string, unknown> };
  };
}

// #1264: the dev session endpoint trusts only a nonce, never request headers.
describe("the dev session gate", () => {
  it("publishes only the canonical project hash without spending or cross-wiring either project's ticket", () => {
    const first = projectFixture("fixture-token-first");
    const second = projectFixture("fixture-token-second");
    writeFileSync(first.ticketFile, "first-ticket");
    writeFileSync(second.ticketFile, "second-ticket");
    const firstRoutes = sessionRoutes(first.project);
    const secondRoutes = sessionRoutes(second.project);
    const firstIdentity = firstRoutes("/__studio/identity");
    const secondIdentity = secondRoutes("/__studio/identity");
    const canonical = realpathSync.native(first.project);
    const normalized = process.platform === "win32" ? toNamespacedPath(canonical).replace(/\\/g, "/") : canonical;
    expect(firstIdentity.status).toBe(200);
    expect(firstIdentity.headers["Cache-Control"]).toBe("no-store");
    expect(firstIdentity.json).toEqual({ projectId: createHash("sha256").update(normalized).digest("hex") });
    expect(secondIdentity.json.projectId).not.toBe(firstIdentity.json.projectId);
    for (const secret of [first.project, first.tokenFile, "fixture-token-first", "first-ticket", "fixture-launch-nonce", stableNonce("fixture-token-first")]) {
      expect(firstIdentity.body).not.toContain(secret);
    }
    expect(readFileSync(first.ticketFile, "utf8")).toBe("first-ticket");
    expect(readFileSync(second.ticketFile, "utf8")).toBe("second-ticket");
    expect(secondRoutes("/__studio/session", "/?nonce=first-ticket").status).toBe(404);
    expect(existsSync(second.ticketFile)).toBe(true);
    expect(firstRoutes("/__studio/session", "/?nonce=first-ticket").json.token).toBe("fixture-token-first");
    expect(secondRoutes("/__studio/session", "/?nonce=second-ticket").json.token).toBe("fixture-token-second");
    expect(existsSync(first.ticketFile)).toBe(false);
    expect(existsSync(second.ticketFile)).toBe(false);
  });
  it("advertises no usable project for absent credentials or another ticket location", () => {
    const first = projectFixture("fixture-token-first");
    const second = projectFixture("fixture-token-second");
    expect(sessionRoutes("")("/__studio/identity").json).toEqual({ projectId: null });
    expect(sessionRoutes(first.project, second.tokenFile)("/__studio/identity").json).toEqual({ projectId: null });
    writeFileSync(first.tokenFile, "");
    expect(sessionRoutes(first.project)("/__studio/identity").json).toEqual({ projectId: null });
    expect(sessionRoutes(first.project)("/__studio/identity", "/", "POST").status).toBe(405);
  });
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
