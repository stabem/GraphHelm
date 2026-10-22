// @vitest-environment node
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join, toNamespacedPath } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";
import type { Connect, Plugin, UserConfigFnObject, ViteDevServer } from "vite";

import config, { authorized, consumeTicket, projectRoute, runtimeOf, stableNonce } from "../../vite.config";
import { RUNTIME_UNREACHABLE_HEADER } from "./proxy-signal";

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
    const firstId = createHash("sha256").update(normalized).digest("hex");
    expect(firstIdentity.json).toEqual({ projectId: firstId, projectIds: [firstId] });
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
    expect(sessionRoutes("")("/__studio/identity").json).toEqual({ projectId: null, projectIds: [] });
    expect(sessionRoutes(first.project, second.tokenFile)("/__studio/identity").json).toEqual({ projectId: null, projectIds: [] });
    writeFileSync(first.tokenFile, "");
    expect(sessionRoutes(first.project)("/__studio/identity").json).toEqual({ projectId: null, projectIds: [] });
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

const servers: Server[] = [];
afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) => new Promise((done) => server.close(done))));
});

function listen(server: Server): Promise<number> {
  servers.push(server);
  return new Promise((done) => server.listen(0, "127.0.0.1", () => done((server.address() as AddressInfo).port)));
}

function idOfFolder(folder: string): string {
  const canonical = realpathSync.native(folder);
  const normalized = process.platform === "win32" ? toNamespacedPath(canonical).replace(/\\/g, "/") : canonical;
  return createHash("sha256").update(normalized).digest("hex");
}

/** The dev session middlewares behind a real HTTP server, mounted the way connect mounts them. */
async function liveStudio(first: string): Promise<string> {
  vi.stubEnv("GRAPHHELM_PROJECT_PATH", first);
  vi.stubEnv("GRAPHHELM_EVENTS", join(first, ".graphhelm", "events"));
  vi.stubEnv("GRAPHHELM_STUDIO_TOKEN_FILE", "");
  vi.stubEnv("GRAPHHELM_STUDIO_SESSION_NONCE", "fixture-launch-nonce");
  const resolved = (config as UserConfigFnObject)({ command: "serve", mode: "test" });
  const plugin = (resolved.plugins as Plugin[]).find((entry) => entry.name === "graphhelm-studio-dev-session");
  const configure = plugin?.configureServer;
  if (typeof configure !== "function") throw new Error("dev session middleware is missing");
  const routes: [string, Connect.NextHandleFunction][] = [];
  configure.call({} as ThisParameterType<typeof configure>, {
    middlewares: { use: (route: string, handler: Connect.NextHandleFunction) => routes.push([route, handler]) },
  } as unknown as ViteDevServer);
  const port = await listen(createServer((request, response) => {
    const url = request.url ?? "/";
    const mounted = routes.find(([route]) => url === route || url.startsWith(`${route}/`) || url.startsWith(`${route}?`));
    const fallThrough = () => { response.statusCode = 404; response.end("not proxied"); };
    if (!mounted) { fallThrough(); return; }
    request.url = url.slice(mounted[0].length) || "/";
    mounted[1](request as Connect.IncomingMessage, response, fallThrough);
  }));
  return `http://127.0.0.1:${port}`;
}

/** A project the Studio was not started for, whose Runtime listens at `address`. */
function joiningProject(token: string, ticket: string, address: string | null) {
  const fixture = projectFixture(token);
  writeFileSync(fixture.ticketFile, ticket);
  if (address !== null) {
    writeFileSync(join(fixture.project, ".graphhelm", "events.runtime-lifecycle.json"), JSON.stringify({ pid: 1, state: "running", address }));
  }
  return { ...fixture, id: idOfFolder(fixture.project) };
}

// #1767: one Studio port serves several projects; each keeps its own ticket, token and Runtime.
describe("a Studio serving several projects", () => {
  it("joins a project by its own ticket, logs it in once, and proxies only its Runtime with its token", async () => {
    const first = projectFixture("fixture-token-first");
    const seen: { url?: string; authorization?: string }[] = [];
    const runtime = await listen(createServer((request, response) => {
      seen.push({ url: request.url, authorization: request.headers.authorization });
      response.setHeader("Content-Type", "application/json");
      response.end(JSON.stringify({ from: "second" }));
    }));
    const second = joiningProject("fixture-token-second", "second-ticket", `127.0.0.1:${runtime}`);
    const studio = await liveStudio(first.project);

    const joined = await fetch(`${studio}/__studio/register?nonce=second-ticket`, { method: "POST", body: JSON.stringify({ path: second.project }) });
    const reply = await joined.text();
    expect(joined.status).toBe(200);
    expect(JSON.parse(reply)).toEqual({ ok: true, projectId: second.id });
    expect(reply).not.toContain(second.project);
    // Registration only proves the ticket; the page spends it.
    expect(readFileSync(second.ticketFile, "utf8")).toBe("second-ticket");

    const identity = await (await fetch(`${studio}/__studio/identity`)).text();
    expect(JSON.parse(identity).projectIds).toEqual([idOfFolder(first.project), second.id]);
    for (const secret of [second.project, "fixture-token-second", "second-ticket"]) expect(identity).not.toContain(secret);

    // The first project's launch nonce never opens another project's session.
    expect((await fetch(`${studio}/__studio/session?nonce=fixture-launch-nonce&project=${second.id}`)).status).toBe(404);
    const session = await (await fetch(`${studio}/__studio/session?nonce=second-ticket&project=${second.id}`)).json() as Record<string, unknown>;
    expect(session).toMatchObject({ ok: true, token: "fixture-token-second", projectId: second.id, sessionKey: stableNonce("fixture-token-second") });
    expect(existsSync(second.ticketFile)).toBe(false);
    expect((await fetch(`${studio}/__studio/session?nonce=second-ticket&project=${second.id}`)).status).toBe(404);
    expect((await fetch(`${studio}/__studio/session?nonce=${stableNonce("fixture-token-second")}&project=${second.id}`)).status).toBe(200);
    // The first project's own session is untouched by the second.
    expect((await (await fetch(`${studio}/__studio/session?nonce=fixture-launch-nonce`)).json() as Record<string, unknown>).token).toBe("fixture-token-first");

    const proxied = await fetch(`${studio}/p/${second.id}/v1/executions?limit=1`, { headers: { Authorization: "Bearer fixture-token-second" } });
    expect(await proxied.json()).toEqual({ from: "second" });
    expect(seen).toEqual([{ url: "/v1/executions?limit=1", authorization: "Bearer fixture-token-second" }]);
    // Only /v1 and /health are forwarded, and only for a registered project.
    expect((await fetch(`${studio}/p/${second.id}/monitor`)).status).toBe(404);
    expect((await fetch(`${studio}/p/${"0".repeat(64)}/v1/executions`)).status).toBe(404);
    expect(seen).toHaveLength(1);
  });
  it("refuses a wrong ticket, a relative path and a project whose Runtime is not running", async () => {
    const first = projectFixture("fixture-token-first");
    const studio = await liveStudio(first.project);
    const post = (nonce: string, path: unknown) => fetch(`${studio}/__studio/register?nonce=${nonce}`, { method: "POST", body: JSON.stringify({ path }) });
    const running = joiningProject("fixture-token-second", "second-ticket", "127.0.0.1:9");
    expect((await post("wrong-ticket", running.project)).status).toBe(404);
    expect((await post("second-ticket", "relative/path")).status).toBe(404);
    expect((await fetch(`${studio}/__studio/register?nonce=second-ticket`)).status).toBe(405);
    const stopped = joiningProject("fixture-token-third", "third-ticket", null);
    expect(await (await post("third-ticket", stopped.project)).json()).toEqual({ ok: false, reason: "runtime_not_running" });
    // An address off loopback is never a proxy target for a project's token.
    const remote = joiningProject("fixture-token-fourth", "fourth-ticket", "10.0.0.5:80");
    expect((await post("fourth-ticket", remote.project)).status).toBe(409);
    expect(JSON.parse(await (await fetch(`${studio}/__studio/identity`)).text()).projectIds).toEqual([idOfFolder(first.project)]);
  });
  it("says the Runtime is unreachable once a registered project's Runtime stops", async () => {
    const first = projectFixture("fixture-token-first");
    const second = joiningProject("fixture-token-second", "second-ticket", "127.0.0.1:9");
    const studio = await liveStudio(first.project);
    expect((await fetch(`${studio}/__studio/register?nonce=second-ticket`, { method: "POST", body: JSON.stringify({ path: second.project }) })).status).toBe(200);
    rmSync(join(second.project, ".graphhelm", "events.runtime-lifecycle.json"));
    const gone = await fetch(`${studio}/p/${second.id}/health`);
    expect(gone.status).toBe(503);
    expect(gone.headers.get(RUNTIME_UNREACHABLE_HEADER)).toBe("connection-refused");
  });
  it("reads a Runtime address from the lifecycle record only on loopback, and routes only /v1 and /health", () => {
    const project = projectFixture("t").project;
    const record = (body: unknown) => writeFileSync(join(project, ".graphhelm", "events.runtime-lifecycle.json"), JSON.stringify(body));
    record({ state: "running", address: "127.0.0.1:8792" });
    expect(runtimeOf(project)).toBe("http://127.0.0.1:8792");
    for (const body of [{ state: "clean", address: "127.0.0.1:8792" }, { state: "running" }, { state: "running", address: "example.com:80" }, { state: "running", address: "127.0.0.1:80/x" }]) {
      record(body);
      expect(runtimeOf(project)).toBeNull();
    }
    const id = "a".repeat(64);
    expect(projectRoute(`/${id}/v1/executions?limit=1`)).toEqual({ id, path: "/v1/executions?limit=1" });
    expect(projectRoute(`/${id}/health`)).toEqual({ id, path: "/health" });
    for (const url of [`/${id}/monitor`, `/${id}/v1x`, `/${id}`, `/../${id}/v1`, `/${id.slice(1)}/v1`]) expect(projectRoute(url)).toBeNull();
  });
});
