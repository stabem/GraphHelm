/** Loopback browser sessions and authenticated project proxies; Runtime credentials stay here. */
import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import { readFileSync, realpathSync, statSync } from "node:fs";
import { request as httpRequest, type IncomingMessage, type ServerResponse } from "node:http";
import { homedir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve, toNamespacedPath } from "node:path";

import type { Plugin } from "vite";

import { STUDIO_OPEN_DEFAULT_PORT } from "./src/login/studio-open.ts";

const SESSION_PATH = "/__studio/session";

function tokenPath(env: Record<string, string>): string | null {
  if (env.GRAPHHELM_STUDIO_TOKEN_FILE) return env.GRAPHHELM_STUDIO_TOKEN_FILE;
  if (!env.GRAPHHELM_EVENTS) return null;
  const events = env.GRAPHHELM_EVENTS.replace(/[\/]+$/, "");
  return join(dirname(events), `${basename(events)}.token`);
}

/** The project's folder name, only for the conventional `<folder>/.graphhelm/events` layout. */
function projectName(env: Record<string, string>): string | null {
  if (env.GRAPHHELM_PROJECT) return env.GRAPHHELM_PROJECT;
  const events = (env.GRAPHHELM_EVENTS || "").replace(/[\/]+$/, "");
  if (basename(events).toLowerCase() !== "events") return null;
  const metadata = dirname(events);
  return basename(metadata).toLowerCase() === ".graphhelm" ? basename(dirname(metadata)) : null;
}

/** Only an explicit path or the conventional `<folder>/.graphhelm/events` layout names a folder. */
function projectFolder(env: Record<string, string>): string | null {
  const explicit = (env.GRAPHHELM_PROJECT_PATH || "").trim();
  if (explicit) return isAbsolute(explicit) ? resolve(explicit) : null;
  const events = (env.GRAPHHELM_EVENTS || "").replace(/[\\/]+$/, "");
  if (!events || !isAbsolute(events) || basename(events).toLowerCase() !== "events") return null;
  const metadata = dirname(events);
  return basename(metadata).toLowerCase() === ".graphhelm" ? dirname(metadata) : null;
}

/** Mirrors execution::delivery::project_id over an already canonical folder. */
function idOf(canonical: string): string {
  // Rust canonicalize keeps the Windows verbatim prefix and the path case.
  const identity = process.platform === "win32" ? toNamespacedPath(canonical).replace(/\\/g, "/") : canonical;
  return createHash("sha256").update(identity).digest("hex");
}

/** The first project's id, only when its token sits in the conventional place under its folder. */
function firstProjectId(env: Record<string, string>): string | null {
  const folder = projectFolder(env);
  const token = tokenPath(env);
  if (folder === null || token === null) return null;
  try {
    const canonical = realpathSync.native(folder);
    if (realpathSync.native(token) !== realpathSync.native(join(canonical, ".graphhelm", "events.token"))) return null;
    return idOf(canonical);
  } catch {
    return null;
  }
}

function firstFolder(env: Record<string, string>): string | null {
  const folder = projectFolder(env);
  try {
    return folder === null ? null : realpathSync.native(folder);
  } catch {
    return null;
  }
}

/** Another project this Studio serves: only the conventional layout of an absolute folder. */
interface Joined { id: string; folder: string; tokenFile: string }


function projectAt(folder: unknown): Joined | null {
  if (typeof folder !== "string" || folder.length === 0 || folder.length > 4096 || !isAbsolute(folder)) return null;
  try {
    const canonical = realpathSync.native(folder);
    if (!statSync(canonical).isDirectory()) return null;
    const tokenFile = join(canonical, ".graphhelm", "events.token");
    if (readToken(tokenFile) === null) return null;
    return { id: idOf(canonical), folder: canonical, tokenFile };
  } catch {
    return null;
  }
}

/** The project's own Runtime as its lifecycle record names it, loopback only (its token rides every
 * forwarded request). Re-read per request, so a restarted Runtime is followed, not cached. */
function runtimeOf(folder: string): string | null {
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

/** The folder as the sidebar shows it: forward slashes, the owner's home as `~`. */
export function displayFolder(canonical: string, home: string): string {
  const slash = (value: string) => value.replace(/\\/g, "/").replace(/\/+$/, "");
  const folder = slash(canonical.replace(/^\\\\\?\\/, ""));
  const root = slash(home);
  const fold = process.platform === "win32" ? (value: string) => value.toLowerCase() : (value: string) => value;
  if (root.length > 0 && (fold(folder) === fold(root) || fold(folder).startsWith(`${fold(root)}/`))) return `~${folder.slice(root.length)}`;
  return folder;
}

function folderOf(canonical: string | null): string | null {
  return canonical === null ? null : displayFolder(canonical, homedir());
}

/** #1901: the run the project's ignored `.graphhelm/team.json` names as its team run (the file the
 * lane hooks bind to), only its `executionId`; null when the file is absent or names none. */
export function teamRunOf(canonical: string | null): string | null {
  if (canonical === null) return null;
  try {
    const raw = readFileSync(join(canonical, ".graphhelm", "team.json"), "utf8");
    if (raw.length > 65_536) return null;
    const id = (JSON.parse(raw) as { executionId?: unknown } | null)?.executionId;
    return typeof id === "string" && /^[A-Za-z0-9._:-]{1,200}$/.test(id) ? id : null;
  } catch {
    return null;
  }
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

function forward(request: IncomingMessage, response: ServerResponse, target: string, path: string, token: string) {
  const url = new URL(path, target);
  const headers: import("node:http").OutgoingHttpHeaders = { ...request.headers, host: url.host, authorization: `Bearer ${token}` };
  delete headers.cookie;
  delete headers.origin;
  delete headers["x-graphhelm-studio-key"];
  const upstream = httpRequest(url, { method: request.method, headers }, (reply) => {
    response.writeHead(reply.statusCode ?? 502, reply.headers);
    reply.pipe(response);
  });
  upstream.setTimeout(30_000, () => upstream.destroy(new Error("Runtime response timed out")));
  upstream.on("error", () => {
    if (response.headersSent) {
      response.destroy();
      return;
    }
    response.writeHead(502);
    response.end();
  });
  request.pipe(upstream);
}

function json(response: ServerResponse, status: number, body: Record<string, unknown>) {
  response.setHeader("Content-Type", "application/json");
  response.setHeader("Cache-Control", "no-store");
  response.statusCode = status;
  response.end(JSON.stringify(body));
}

function readToken(path: string | null): string | null {
  if (path === null) return null;
  try {
    const raw = readFileSync(path, "utf8").trim();
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

export function devSession(env: Record<string, string>): Plugin {
  const session = randomBytes(32).toString("hex");
  return {
    name: "graphhelm-loopback-session",
    apply: "serve",
    configureServer(server) {
      if (server.config.server.host !== "127.0.0.1" && server.config.server.host !== "localhost")
        throw new Error("Studio must bind to 127.0.0.1 or localhost");
      const joined = new Map<string, Joined>();
      server.middlewares.use((request, response, next) => {
        const address = server.httpServer?.address();
        const port = typeof address === "object" && address !== null ? address.port : server.config.server.port;
        // Each allowed authority is explicit; forwarded headers never participate in trust.
        const authorities = [`127.0.0.1:${port}`, `localhost:${port}`];
        const host = request.headers.host;
        const origin = request.headers.origin;
        const site = request.headers["sec-fetch-site"];
        if (!host || !authorities.includes(host)
          || (origin !== undefined && origin !== `http://${host}`)
          || (site !== undefined && site !== "same-origin" && site !== "none")) {
          return json(response, 403, { ok: false, reason: "foreign_origin" });
        }
        const path = new URL(request.url ?? "/", `http://${host}`).pathname;
        const protectedPath = /^\/(?:__studio|v1|health|p)(?:\/|$)/.test(path);
        const cookieName = `graphhelm_studio_${port}=`;
        const cookie = (request.headers.cookie ?? "").split(";").map(part => part.trim()).find(part => part.startsWith(cookieName))?.slice(cookieName.length) ?? "";
        const authenticated = sameSecret(cookie, session);
        const read = request.method === "GET" || request.method === "HEAD";
        // The CLI authenticates only this endpoint against the submitted project's credential.
        const machineRegistration = path === "/__studio/register" && request.method === "POST"
          && origin === undefined && site === undefined;
        if (protectedPath && !machineRegistration && (!authenticated || (read ? (origin === undefined && site !== "same-origin") : origin === undefined))) {
          return json(response, 403, { ok: false, reason: "session_required" });
        }
        if (!protectedPath && read
          && request.headers["sec-fetch-mode"] === "navigate" && request.headers["sec-fetch-dest"] === "document") {
          response.setHeader("Set-Cookie", `${cookieName}${session}; HttpOnly; SameSite=Strict; Path=/`);
          response.setHeader("Cache-Control", "no-store");
        }
        next();
      });
      server.httpServer?.once("listening", () => {
        const address = server.httpServer?.address();
        const port = typeof address === "object" && address !== null ? address.port : STUDIO_OPEN_DEFAULT_PORT;
        server.config.logger.info(`  Studio: http://127.0.0.1:${port}/`);
      });
      server.middlewares.use("/__studio/identity", (request, response) => {
        if (request.method !== "GET") return json(response, 405, { ok: false });
        const first = firstProjectId(env);
        json(response, 200, { projectId: first, projectIds: [...new Set([...(first === null ? [] : [first]), ...joined.keys()])] });
      });
      for (const route of ["/__studio/register", "/__studio/add"]) {
        server.middlewares.use(route, (request, response) => {
          if (request.method !== "POST") return json(response, 405, { ok: false });
          void readBody(request, 8 * 1024).then(raw => {
            let path: unknown;
            try { path = raw === null ? null : (JSON.parse(raw) as {path?: unknown}).path; }
            catch { return json(response, 422, {ok:false, reason:"not_a_project"}); }
            const project = projectAt(path);
            const token = project === null ? null : readToken(project.tokenFile);
            if (project === null || token === null) return json(response, 422, {ok:false,reason:"not_a_project"});
            if (route === "/__studio/register") {
              const auth = request.headers.authorization ?? "";
              if (request.headers.origin !== undefined || request.headers["sec-fetch-site"] !== undefined || !sameSecret(auth, `Bearer ${token}`))
                return json(response, 403, {ok:false,reason:"registration_refused"});
            }
            if (runtimeOf(project.folder) === null) return json(response, 409, {ok:false,reason:"runtime_not_running"});
            if (project.id !== firstProjectId(env)) joined.set(project.id, project);
            json(response, 200, {ok:true,projectId:project.id});
          });
        });
      }
      server.middlewares.use((request, response, next) => {
        const path = request.url ?? "/";
        const selected = /^\/p\/([^/]+)(\/(?:v1|health)(?:[/?].*)?)$/.exec(path);
        const first = firstProjectId(env);
        let folder = firstFolder(env);
        let file = tokenPath(env);
        let upstreamPath = path;
        if (path.startsWith("/p/")) {
          if (!selected) return json(response,404,{ok:false,reason:"project_unavailable"});
          const id = selected[1]!;
          if (id !== first) {
            const project = joined.get(id);
            if (!project) return json(response,404,{ok:false,reason:"project_unavailable"});
            folder = project.folder; file = project.tokenFile;
          }
          upstreamPath = selected[2]!;
        } else if (!/^\/(?:v1|health)(?:[/?]|$)/.test(path)) return next();
        const token = readToken(file);
        const target = (folder === null ? null : runtimeOf(folder)) ?? (file === tokenPath(env) ? env.GRAPHHELM_RUNTIME_URL : null);
        if (token === null || !target || !/^http:\/\/(?:127\.0\.0\.1|localhost|\[::1\]):[0-9]+\/?$/.test(target))
          return json(response,503,{ok:false,reason:"runtime_unavailable"});
        forward(request,response,target,upstreamPath,token);
      });
      server.middlewares.use(SESSION_PATH, (request,response) => {
        if (request.method !== "GET") return json(response,405,{ok:false});
        const wanted = new URL(request.url ?? "/","http://localhost").searchParams.get("project");
        const first = firstProjectId(env);
        const project = wanted && wanted !== first ? joined.get(wanted) : undefined;
        if (wanted && wanted !== first && !project) return json(response,404,{ok:false,reason:"project_unavailable"});
        const folder = project?.folder ?? firstFolder(env);
        if (readToken(project?.tokenFile ?? tokenPath(env)) === null) return json(response,503,{ok:false,reason:"project_unavailable"});
        json(response,200,{ok:true,project:project ? basename(project.folder) : projectName(env),projectId:project?.id ?? first,
          folder:folderOf(folder),base:project ? `/p/${project.id}` : "",teamRun:teamRunOf(folder)});
      });
    },
  };
}
