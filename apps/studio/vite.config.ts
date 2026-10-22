import { createHash, createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { readFileSync, realpathSync, statSync, unlinkSync } from "node:fs";
import { request as httpRequest, type IncomingMessage, type ServerResponse } from "node:http";
import { basename, dirname, isAbsolute, join, resolve, toNamespacedPath } from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig, loadEnv, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

import { scaled, TEST_TIMEOUT_BASE_MS } from "./src/test/time-scale.ts";
import { RUNTIME_UNREACHABLE_HEADER } from "./src/runtime/proxy-signal.ts";

/** Where the dev server looks for the bearer token, in order of precedence. */
function tokenPath(env: Record<string, string>): string | null {
  if (env.GRAPHHELM_STUDIO_TOKEN_FILE) return env.GRAPHHELM_STUDIO_TOKEN_FILE;
  if (!env.GRAPHHELM_EVENTS) return null;
  // The Runtime writes `<events-directory-name>.token` as a SIBLING of the events directory, not
  // inside it: the store refuses to open a repository containing an entry outside its own
  // allowlist. Mirrored here rather than guessed.
  const events = env.GRAPHHELM_EVENTS.replace(/[\\/]+$/, "");
  return join(dirname(events), `${basename(events)}.token`);
}

/** Only an explicit path or the conventional project/.graphhelm/events layout identifies a
 * project folder. An arbitrary event store is not evidence of its owning source tree. */
function projectFolder(env: Record<string, string>): string | null {
  const explicit = (env.GRAPHHELM_PROJECT_PATH || "").trim();
  if (explicit) return isAbsolute(explicit) ? resolve(explicit) : null;
  const events = (env.GRAPHHELM_EVENTS || "").replace(/[\\/]+$/, "");
  if (!events || !isAbsolute(events) || basename(events).toLowerCase() !== "events") return null;
  const metadata = dirname(events);
  if (basename(metadata).toLowerCase() !== ".graphhelm") return null;
  return dirname(metadata);
}

/**
 * Hands the page its session so nobody types a token into a local dev loop — but only to a
 * caller holding this run's NONCE.
 *
 * WHY THE NONCE EXISTS. The token file on disk is owner-only (`serve` creates it 0o600 on Unix,
 * deliberately), and loopback is not a user boundary: every local OS user can reach 127.0.0.1.
 * An endpoint that answered any request would therefore DOWNGRADE an owner-only secret into an
 * any-local-user one (PR #467 review, Codex + N). Nothing served over HTTP can carry the proof —
 * any local user can fetch the page's HTML and JS too — so the nonce travels the one channel
 * another local user cannot read: this process's own terminal (the URL printed below) and its
 * environment (`GRAPHHELM_STUDIO_SESSION_NONCE`, for launchers like studio-up.ps1). Opening the
 * plain URL without the nonce lands on the connect gate, which is honest, not broken.
 *
 * WHY THIS IS NOT A NEW SECRET. The token is ALREADY on disk — `graphhelm serve` writes it there
 * itself, and the operator reads it out of that file today by hand. This plugin reads the same
 * file on the same machine and hands it to a page served from the same process. Nothing new is
 * stored, and in particular nothing is written into a settings file that gets committed: a live
 * credential in `.claude/settings.json` would be the same convenience and a real leak.
 *
 * `apply: "serve"` is load-bearing. This endpoint exists ONLY while the dev server runs on the
 * operator's own machine; the production bundle has no such route, and a page served from one
 * falls back to asking for the token.
 *
 * The reply carries the token and the operator's project folder only behind the nonce gate. A
 * missing or unreadable file, like a missing or wrong nonce, is a plain 404: the page then shows
 * its connect screen, which is the correct outcome and not a diagnostic surface.
 */
/** #1264: a session key that survives reloads and Studio restarts but not a token rotation. It is
 * derived from the token itself, so only someone who can already read the owner-only token file
 * can compute it; the dev server hands it to a page that already proved a nonce, and the page
 * keeps it so a later load without `?session=` still connects. Never the token, never reversible. */
export function stableNonce(token: string): string {
  return createHmac("sha256", token).update("graphhelm-studio-session-v1").digest("hex");
}

/** Reads the one-line token file, or `null` when it is missing or is not one line. */
function readToken(env: Record<string, string>): string | null {
  return readTokenFile(tokenPath(env));
}

function readTokenFile(path: string | null): string | null {
  if (path === null) return null;
  try {
    const raw = readFileSync(path, "utf8").trim();
    // A token is one line. Anything else is not the file we meant to read, and guessing which
    // line to take is how a stray file becomes a credential.
    return raw.length > 0 && raw.length <= 512 && !raw.includes("\n") ? raw : null;
  } catch {
    return null;
  }
}

function sameSecret(presented: string, expected: string): boolean {
  const left = Buffer.from(presented, "utf8");
  const right = Buffer.from(expected, "utf8");
  return left.length === right.length && timingSafeEqual(left, right);
}

/** #1264: the only gate. A missing nonce is refused whatever headers came with it. */
export function authorized(presented: string, nonce: string, token: string): boolean {
  return presented.length > 0 && (sameSecret(presented, nonce) || sameSecret(presented, stableNonce(token)));
}

/** #1264: where `graphhelm studio open` leaves its one-shot ticket (`<events>.studio-ticket`,
 * owner-only, beside the token file). */
function ticketPath(env: Record<string, string>): string | null {
  const token = tokenPath(env);
  return token === null || !token.endsWith(".token") ? null : `${token.slice(0, -".token".length)}.studio-ticket`;
}

/** Mirrors execution::delivery::project_id; only the conventional ticket location supports open. */
function studioProjectId(env: Record<string, string>): string | null {
  const folder = projectFolder(env);
  const token = tokenPath(env);
  const ticket = ticketPath(env);
  if (folder === null || token === null || ticket === null || readToken(env) === null) return null;
  try {
    if (basename(ticket) !== "events.studio-ticket"
      || realpathSync.native(dirname(ticket)) !== realpathSync.native(join(folder, ".graphhelm"))) return null;
    if (realpathSync.native(token) !== realpathSync.native(join(folder, ".graphhelm", "events.token"))) return null;
    const canonical = realpathSync.native(folder);
    if (!statSync(canonical).isDirectory()) return null;
    return idOf(canonical);
  } catch {
    // Missing paths cannot establish which project's ticket this server can consume.
    return null;
  }
}

/** Mirrors execution::delivery::project_id over an already canonical folder. */
function idOf(canonical: string): string {
  // Rust canonicalize retains the Windows verbatim prefix and path case.
  const identity = process.platform === "win32" ? toNamespacedPath(canonical).replace(/\\/g, "/") : canonical;
  return createHash("sha256").update(identity).digest("hex");
}

/** #1767: a project this Studio serves besides the one it was started for. Only the conventional
 * `<folder>/.graphhelm/events.token` layout qualifies, exactly as for the first project. */
export interface StudioProject { id: string; folder: string; tokenFile: string; ticketFile: string }

const PROJECT_ID = /^[0-9a-f]{64}$/;

/** #1767: the conventional layout of an absolute project folder, or `null`. */
export function projectAt(folder: unknown): StudioProject | null {
  if (typeof folder !== "string" || folder.length === 0 || folder.length > 4096 || !isAbsolute(folder)) return null;
  try {
    const canonical = realpathSync.native(folder);
    if (!statSync(canonical).isDirectory()) return null;
    const tokenFile = join(canonical, ".graphhelm", "events.token");
    if (readTokenFile(tokenFile) === null) return null;
    return { id: idOf(canonical), folder: canonical, tokenFile, ticketFile: join(canonical, ".graphhelm", "events.studio-ticket") };
  } catch {
    return null;
  }
}

/** #1767: the project's own Runtime, as its lifecycle record names it (`serve` writes the address
 * it listens on). Loopback only: that project's bearer token rides every forwarded request, so the
 * dev server must never be talked into sending it anywhere else. Re-read on every request, so a
 * Runtime restarted on another port is followed rather than cached. */
export function runtimeOf(folder: string): string | null {
  try {
    const raw = readFileSync(join(folder, ".graphhelm", "events.runtime-lifecycle.json"), "utf8");
    if (raw.length > 16 * 1024) return null;
    const record = JSON.parse(raw) as { state?: unknown; address?: unknown };
    if (record.state !== "running" || typeof record.address !== "string") return null;
    return /^(?:127\.0\.0\.1|\[::1\]):\d{1,5}$/.test(record.address) ? `http://${record.address}` : null;
  } catch {
    return null;
  }
}

/** #1767: whether `presented` is the project's current `studio open` ticket. Registration only
 * reads it; the page spends it once on `/__studio/session`, so the link stays one-shot. */
function holdsTicket(project: StudioProject, presented: string): boolean {
  if (presented.length === 0) return false;
  try {
    const ticket = readFileSync(project.ticketFile, "utf8").trim();
    return ticket.length > 0 && sameSecret(presented, ticket);
  } catch {
    return false;
  }
}

function readRoster(folder: string | null): string[] {
  if (folder === null) return [];
  try {
    const team = JSON.parse(readFileSync(join(folder, ".graphhelm", "team.json"), "utf8")) as { onDemandLanes?: unknown };
    return Array.isArray(team.onDemandLanes)
      ? team.onDemandLanes.filter((name): name is string => typeof name === "string" && name.length > 0 && name.length <= 80).slice(0, 100)
      : [];
  } catch {
    // An absent roster does not invent available lanes.
    return [];
  }
}

/** #1767: `/p/<projectId>/v1/...` and `/p/<projectId>/health` reach that project's own Runtime.
 * Only those two prefixes, like the first project's proxy; nothing else is forwarded. */
export function projectRoute(url: string): { id: string; path: string } | null {
  const match = /^\/([0-9a-f]{64})(\/(?:v1|health)(?:[/?].*)?)$/.exec(url);
  return match ? { id: match[1]!, path: match[2]! } : null;
}

function forward(request: IncomingMessage, response: ServerResponse, target: string, path: string) {
  const url = new URL(path, target);
  const upstream = httpRequest(url, { method: request.method, headers: { ...request.headers, host: url.host } }, (reply) => {
    response.writeHead(reply.statusCode ?? 502, reply.headers);
    reply.pipe(response);
  });
  upstream.on("error", (error) => {
    if (response.headersSent) {
      response.destroy();
      return;
    }
    // #1170: a refused connection proves the request never reached the Runtime.
    const refused = (error as { code?: string }).code === "ECONNREFUSED";
    response.writeHead(refused ? 503 : 502, refused ? { [RUNTIME_UNREACHABLE_HEADER]: "connection-refused" } : {});
    response.end();
  });
  request.pipe(upstream);
}

function readBody(request: IncomingMessage, limit: number): Promise<string | null> {
  return new Promise((done) => {
    let size = 0;
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => {
      size += chunk.length;
      if (size <= limit) chunks.push(chunk);
    });
    request.on("end", () => done(size > limit ? null : Buffer.concat(chunks).toString("utf8")));
    request.on("error", () => done(null));
  });
}

/** #1264: true once for the ticket on disk: a match deletes it, so the link in browser history
 * is dead after its first use. A failed delete refuses, so a ticket can never be used twice. */
export function consumeTicket(path: string | null, presented: string): boolean {
  if (path === null || presented.length === 0) return false;
  let ticket: string;
  try {
    ticket = readFileSync(path, "utf8").trim();
  } catch {
    return false;
  }
  if (ticket.length === 0 || !sameSecret(presented, ticket)) return false;
  try {
    unlinkSync(path);
  } catch {
    return false;
  }
  return true;
}

function devSession(env: Record<string, string>): Plugin {
  // One nonce per server run. A launcher may inject its own so it can open the browser at the
  // right URL; otherwise it is random and lives only in this process and its terminal output.
  const provided = (env.GRAPHHELM_STUDIO_SESSION_NONCE || "").trim();
  const nonce = provided || randomBytes(16).toString("hex");
  return {
    name: "graphhelm-studio-dev-session",
    apply: "serve",
    configureServer(server) {
      // #1767: the other projects this Studio serves, registered by `graphhelm studio open` in their
      // own folders. Held in memory only: a restarted Studio serves its first project again until
      // each other project opens it once more.
      const registered = new Map<string, StudioProject>();
      server.middlewares.use("/__studio/identity", (request, response) => {
        response.setHeader("Content-Type", "application/json");
        response.setHeader("Cache-Control", "no-store");
        response.statusCode = request.method === "GET" ? 200 : 405;
        const first = request.method === "GET" ? studioProjectId(env) : null;
        // Hashes only, like `projectId`: the identity route carries no path and no secret.
        const projectIds = request.method === "GET" ? [...new Set([...(first === null ? [] : [first]), ...registered.keys()])] : [];
        response.end(JSON.stringify({ projectId: first, projectIds }));
      });
      // #1767: `graphhelm studio open` in another project registers it here. The caller proves it can
      // read that project's owner-only ticket; the ticket is not spent (the page spends it). The path
      // travels in the body, never the URL, and is never echoed back.
      server.middlewares.use("/__studio/register", (request, response) => {
        response.setHeader("Content-Type", "application/json");
        response.setHeader("Cache-Control", "no-store");
        const reply = (status: number, body: Record<string, unknown>) => {
          response.statusCode = status;
          response.end(JSON.stringify(body));
        };
        if (request.method !== "POST") {
          reply(405, { ok: false });
          return;
        }
        const presented = new URL(request.url ?? "/", "http://localhost").searchParams.get("nonce") ?? "";
        void readBody(request, 8 * 1024).then((raw) => {
          let path: unknown = null;
          try {
            path = raw === null ? null : (JSON.parse(raw) as { path?: unknown }).path;
          } catch { /* an unreadable body names no project */ }
          const project = projectAt(path);
          if (project === null || !holdsTicket(project, presented)) {
            reply(404, { ok: false });
            return;
          }
          if (runtimeOf(project.folder) === null) {
            // Its Runtime is not running (or predates the recorded address): nothing to proxy to.
            reply(409, { ok: false, reason: "runtime_not_running" });
            return;
          }
          if (project.id !== studioProjectId(env)) registered.set(project.id, project);
          reply(200, { ok: true, projectId: project.id });
        });
      });
      // #1767: each registered project's Runtime, behind the same same-origin rule as `/v1`.
      server.middlewares.use("/p", (request, response, next) => {
        const route = projectRoute(request.url ?? "");
        const project = route === null ? undefined : registered.get(route.id);
        const target = project === undefined ? null : runtimeOf(project.folder);
        if (route === null || project === undefined) {
          next();
          return;
        }
        if (target === null) {
          response.writeHead(503, { [RUNTIME_UNREACHABLE_HEADER]: "connection-refused" });
          response.end();
          return;
        }
        forward(request, response, target, route.path);
      });
      server.httpServer?.once("listening", () => {
        // Printed ONLY when the nonce was minted here - then the terminal is the sole channel
        // that can carry it to the person, and it is the owner's own ephemeral stdout, the same
        // trust class as the token file they could `cat`. A launcher-provided nonce is never
        // echoed: the launcher already holds it, and repeating a credential into a log that
        // gains nothing is pure exposure (PR #467 review). The residual print is a deliberate,
        // dev-only trade-off: apply:"serve" only, dead with the process, absent from the bundle.
        if (provided) return;
        const address = server.httpServer?.address();
        const port = typeof address === "object" && address !== null ? address.port : 5183;
        server.config.logger.info(
          `  Studio auto-connect: http://127.0.0.1:${port}/?session=${nonce}`,
        );
      });
      server.middlewares.use("/__studio/session", (request, response) => {
        response.setHeader("Content-Type", "application/json");
        response.setHeader("Cache-Control", "no-store");
        const refuse = () => {
          response.statusCode = 404;
          response.end(JSON.stringify({ ok: false }));
        };
        // The nonce gate: the run's own nonce or the stable key derived from the token on disk
        // (#1264), or the one-shot ticket of `graphhelm studio open`. Headers are never evidence - any local process can forge them.
        const query = new URL(request.url ?? "/", "http://localhost").searchParams;
        const presented = query.get("nonce") ?? "";
        const wanted = query.get("project") ?? "";
        const first = studioProjectId(env);
        // #1767: another registered project answers only to ITS key or ITS ticket - never to this
        // run's launch nonce, which belongs to the first project.
        if (wanted !== "" && wanted !== first) {
          const project = PROJECT_ID.test(wanted) ? registered.get(wanted) : undefined;
          const token = project === undefined ? null : readTokenFile(project.tokenFile);
          if (project === undefined || token === null
            || !((presented.length > 0 && sameSecret(presented, stableNonce(token))) || consumeTicket(project.ticketFile, presented))) {
            refuse();
            return;
          }
          response.statusCode = 200;
          response.end(JSON.stringify({
            ok: true,
            token,
            project: basename(project.folder),
            projectPath: project.folder,
            projectId: project.id,
            onDemandLanes: readRoster(project.folder),
            sessionKey: stableNonce(token),
          }));
          return;
        }
        const token = readToken(env);
        if (token === null || !(authorized(presented, nonce, token) || consumeTicket(ticketPath(env), presented))) {
          refuse();
          return;
        }
        response.statusCode = 200;
        // This nonce-gated local session may show the source folder because the operator needs
        // to distinguish projects. Only the conventional layout or an explicit path identifies
        // it; an arbitrary event store is never guessed into a source folder.
        const folder = projectFolder(env);
        response.end(JSON.stringify({
          ok: true,
          token,
          project: env.GRAPHHELM_PROJECT || (folder === null ? null : basename(folder)),
          projectPath: folder,
          projectId: first,
          onDemandLanes: readRoster(folder),
          sessionKey: stableNonce(token),
        }));
      });
    },
  };
}

/**
 * A dev-only stand-in for the browser's `document.modelContext`, so the WebMCP path can be
 * exercised END TO END without an agent browser: the page registers its real tools against it,
 * and a driver (a test, or a person in the console) calls them through `window.__webmcpShim`
 * exactly as an agent would - real schemas, real Runtime, real UI reactions.
 *
 * THREE GATES keep this out of anything that matters: it exists only while the dev server runs
 * (`apply: "serve"`, like the session endpoint above); it activates only when the operator asks
 * by URL (`?webmcp-shim`); and it installs nothing when the browser HAS a real modelContext -
 * a shim must never shadow the thing it stands in for.
 */
function devWebMcpShim(): Plugin {
  return {
    name: "graphhelm-studio-dev-webmcp-shim",
    apply: "serve",
    transformIndexHtml() {
      return [
        {
          tag: "script",
          injectTo: "head-prepend",
          children: `
            if (new URLSearchParams(location.search).has("webmcp-shim") && !document.modelContext) {
              const tools = new Map();
              document.modelContext = {
                registerTool(tool) {
                  tools.set(tool.name, tool);
                  return { unregister: () => tools.delete(tool.name) };
                },
              };
              window.__webmcpShim = {
                list: () => [...tools.keys()],
                schema: (name) => tools.get(name)?.inputSchema ?? null,
                call: (name, input) => {
                  const tool = tools.get(name);
                  if (!tool) return Promise.reject(new Error("no such tool: " + name));
                  return tool.execute(input ?? {});
                },
              };
            }
          `,
        },
      ];
    },
  };
}

/**
 * Development and preview both PROXY the Runtime rather than pointing the browser at it.
 *
 * That is a security decision, not a convenience one. Same-origin requests mean the bearer token
 * never rides a cross-origin request, no CORS relaxation is needed on the Runtime, and the
 * production build - served from wherever the operator puts it - uses the same relative paths.
 *
 * Only `/v1` and `/health` are proxied. `/monitor` deliberately is NOT: the Studio reads the
 * public JSON API and nothing else, and a proxy entry for the HTML monitor would quietly make
 * scraping it possible again.
 */
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, ".", "");
  const target = env.GRAPHHELM_RUNTIME_URL || "http://127.0.0.1:8080";
  // #1170: a refused connection proves the request never reached the Runtime. Say so in a header
  // the client can trust, so a mutation is reported as not sent instead of "maybe committed".
  const configure = (proxy: { on: (event: "error", listener: (...args: unknown[]) => void) => void }) => {
    proxy.on("error", (error, _req, res) => {
      const code = (error as { code?: string }).code;
      const reply = res as { headersSent?: boolean; writeHead?: (s: number, h: Record<string, string>) => void; end?: () => void };
      if (code !== "ECONNREFUSED" || reply.headersSent || !reply.writeHead || !reply.end) return;
      reply.writeHead(503, { [RUNTIME_UNREACHABLE_HEADER]: "connection-refused" });
      reply.end();
    });
  };
  const proxy = {
    "/v1": { target, changeOrigin: false, configure },
    "/health": { target, changeOrigin: false, configure },
  };

  return {
    plugins: [react(), devSession(env), devWebMcpShim()],
    // POLLING IS LOAD-BEARING on this machine: the F: drive drops fs events, and the dev
    // server spent a whole afternoon serving stale modules through hard reloads (verified:
    // disk and served output disagreed). Polling costs a little CPU and ends that class.
    server: {
      host: "127.0.0.1",
      port: 5183,
      strictPort: true,
      proxy,
      watch: { usePolling: true, interval: 300 },
      // The client reads bounds and the timestamp pattern out of `schemas/event-envelope.schema.json`
      // at the repository root - imported, not copied, so they move with the schema. The dev
      // server's default allow-list stops at this app's root; ONLY `<repo>/schemas` is opened for
      // that read - not the repository root, which would expose `.git/` to the dev server
      // (L's review of #662).
      fs: { allow: [fileURLToPath(new URL("../../schemas", import.meta.url)), "."] },
    },
    preview: { host: "127.0.0.1", port: 5183, strictPort: true, proxy },
    test: {
      environment: "jsdom",
      setupFiles: "./src/test/setup.ts",
      css: true,
      restoreMocks: true,
      // #1075: without a cap Vitest forks one worker per core minus one (31 on the gate host) and
      // every worker loads jsdom at once. Under load the simultaneous starts exceed the pool's
      // startup timeout and the stage ends RED with zero tests executed — measured twice on
      // 2026-09-13 (gate runs 1070-20260913T180143 and 1069-20260913T184006: 19 and 20 files,
      // "Failed to start forks worker", no assertion run). Four workers on the same loaded host:
      // 19 files, 306 tests, 44 s. The number is a ceiling on contention, not a tuning knob.
      // Second measurement (gate runs 1070-20260913T195350 and 1069-20260913T201316, both WITH the
      // cap at four): 17 and 18 files passed, and two to four workers still failed to start on the
      // loaded host. One worker starts once; the files then run in it sequentially. Measured on this
      // host: 21 files, 343 tests, under three minutes. `npx vitest run --maxWorkers=N` still
      // overrides it for a developer on an idle machine.
      maxWorkers: 1,
      // #1019: One-fork trial. On 2026-10-12 both one-thread and one-fork runs
      // printed the banner but no result for team-tasks.ts within 90 seconds.
      // This choice has no proven host-level winner yet. Keep isolation disabled.
      pool: "forks",
      isolate: false,
      // #549: the test timeout is a hang catcher scaled by GRAPHHELM_TEST_TIME_SCALE (unset: 1),
      // the knob the CLI tests already read. The default 5 s failed App cells on a busy host with
      // nothing wrong; see src/test/time-scale.ts for what may and may not be scaled.
      testTimeout: scaled(TEST_TIMEOUT_BASE_MS),
    },
  };
});
