/**
 * The dev server's login handoff: `/__studio/session?nonce=<presented>` answers the Runtime's
 * bearer token only to a caller presenting one of three things:
 *
 * - this server run's nonce (`GRAPHHELM_STUDIO_SESSION_NONCE`, or a random one printed to the
 *   owner's own terminal);
 * - the stable key derived from the token on disk (handed to a page that already proved one of
 *   the others, so a reload stays logged in);
 * - the one-shot ticket `graphhelm studio open` writes beside the token (`<events>.studio-ticket`),
 *   deleted on first use.
 *
 * Same contract as the old Studio's dev-session plugin (apps/studio/vite.config.ts, #1264). #1800
 * adds the #1767 routes that let more projects join this port, in the wire contract
 * `graphhelm studio open` already speaks (apps/cli/src/commands/studio.rs):
 *
 * - `GET /__studio/identity` answers `{projectId, projectIds}`, hashes only;
 * - `POST /__studio/register?nonce=<ticket>` with `{"path": <folder>}` registers another project
 *   whose current ticket the caller holds (read, not spent) and whose Runtime is running;
 * - `/__studio/session?project=<id>` answers that project only against ITS ticket or ITS key;
 * - `/p/<id>/v1/...` and `/p/<id>/health` forward to that project's own loopback Runtime.
 *
 * #1833 adds what the sidebar shows per project and its "Add project folder" action:
 *
 * - every `/__studio/session` answer carries `folder`, the project's folder for display (`~` for the
 *   owner's home). It reaches only a caller already holding that project's token; the folder is
 *   never written to a log line, an error body or the Runtime;
 * - `POST /__studio/add` with header `X-GraphHelm-Studio-Key: <stable key of a project this tab
 *   holds>` and `{"path": <folder>, "ticket": <ticket>}` registers the folder exactly as
 *   `/__studio/register` does (a GraphHelm project whose Runtime is running at loopback) and answers
 *   that project's id and stable key. It needs BOTH proofs: the owner's key, and the new project's
 *   own one-shot ticket (`graphhelm studio open --project <folder>`), spent by the add. Holding one
 *   project's key is never proof of access to another. Registration is keyed by the canonical
 *   folder's id, so the same folder twice is one project.
 *
 * `apply: "serve"` keeps every route out of any production bundle: a built page has no way to
 * obtain a token.
 */
import { createHash, createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { readFileSync, realpathSync, statSync, unlinkSync } from "node:fs";
import { request as httpRequest, type IncomingMessage, type ServerResponse } from "node:http";
import { homedir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve, toNamespacedPath } from "node:path";

import type { Plugin } from "vite";

const SESSION_PATH = "/__studio/session";

function tokenPath(env: Record<string, string>): string | null {
  if (env.GRAPHHELM_STUDIO_TOKEN_FILE) return env.GRAPHHELM_STUDIO_TOKEN_FILE;
  if (!env.GRAPHHELM_EVENTS) return null;
  const events = env.GRAPHHELM_EVENTS.replace(/[\/]+$/, "");
  return join(dirname(events), `${basename(events)}.token`);
}

function ticketPath(env: Record<string, string>): string | null {
  const token = tokenPath(env);
  return token === null || !token.endsWith(".token") ? null : `${token.slice(0, -".token".length)}.studio-ticket`;
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
interface Joined { id: string; folder: string; tokenFile: string; ticketFile: string }

const PROJECT_ID = /^[0-9a-f]{64}$/;

function projectAt(folder: unknown): Joined | null {
  if (typeof folder !== "string" || folder.length === 0 || folder.length > 4096 || !isAbsolute(folder)) return null;
  try {
    const canonical = realpathSync.native(folder);
    if (!statSync(canonical).isDirectory()) return null;
    const tokenFile = join(canonical, ".graphhelm", "events.token");
    if (readToken(tokenFile) === null) return null;
    return { id: idOf(canonical), folder: canonical, tokenFile, ticketFile: join(canonical, ".graphhelm", "events.studio-ticket") };
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

/** Whether `presented` is the project's current ticket; registration reads it, the page spends it. */
function holdsTicket(project: Joined, presented: string): boolean {
  if (presented.length === 0) return false;
  try {
    const ticket = readFileSync(project.ticketFile, "utf8").trim();
    return ticket.length > 0 && sameSecret(presented, ticket);
  } catch {
    return false;
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

function forward(request: IncomingMessage, response: ServerResponse, target: string, path: string) {
  const url = new URL(path, target);
  const upstream = httpRequest(url, { method: request.method, headers: { ...request.headers, host: url.host } }, (reply) => {
    response.writeHead(reply.statusCode ?? 502, reply.headers);
    reply.pipe(response);
  });
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

export function stableKey(token: string): string {
  return createHmac("sha256", token).update("graphhelm-studio-session-v1").digest("hex");
}

/** True once for the ticket on disk: a match deletes it; a failed delete refuses. */
function consumeTicket(path: string | null, presented: string): boolean {
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

export function devSession(env: Record<string, string>): Plugin {
  const provided = (env.GRAPHHELM_STUDIO_SESSION_NONCE || "").trim();
  const nonce = provided || randomBytes(16).toString("hex");
  return {
    name: "graphhelm-studio-next-session",
    apply: "serve",
    configureServer(server) {
      server.httpServer?.once("listening", () => {
        if (provided) return;
        const address = server.httpServer?.address();
        const port = typeof address === "object" && address !== null ? address.port : 5185;
        server.config.logger.info(`  Studio login: http://127.0.0.1:${port}/?session=${nonce}`);
      });
      // Held in memory only: a restarted Studio serves its first project until each other project
      // runs `graphhelm studio open` again.
      const joined = new Map<string, Joined>();
      server.middlewares.use("/__studio/identity", (request, response) => {
        if (request.method !== "GET") return json(response, 405, { projectId: null, projectIds: [] });
        const first = firstProjectId(env);
        json(response, 200, { projectId: first, projectIds: [...new Set([...(first === null ? [] : [first]), ...joined.keys()])] });
      });
      server.middlewares.use("/__studio/register", (request, response) => {
        if (request.method !== "POST") return json(response, 405, { ok: false });
        const presented = new URL(request.url ?? "/", "http://localhost").searchParams.get("nonce") ?? "";
        void readBody(request, 8 * 1024).then((raw) => {
          let path: unknown = null;
          try {
            path = raw === null ? null : (JSON.parse(raw) as { path?: unknown }).path;
          } catch { /* an unreadable body names no project */ }
          const project = projectAt(path);
          if (project === null || !holdsTicket(project, presented)) return json(response, 404, { ok: false });
          if (runtimeOf(project.folder) === null) return json(response, 409, { ok: false, reason: "runtime_not_running" });
          if (project.id !== firstProjectId(env)) joined.set(project.id, project);
          json(response, 200, { ok: true, projectId: project.id });
        });
      });
      server.middlewares.use("/__studio/add", (request, response) => {
        if (request.method !== "POST") return json(response, 405, { ok: false });
        const header = request.headers["x-graphhelm-studio-key"];
        const presented = typeof header === "string" ? header : "";
        const firstToken = readToken(tokenPath(env));
        const owner = presented.length > 0 && [firstToken, ...[...joined.values()].map((known) => readToken(known.tokenFile))]
          .some((token) => token !== null && sameSecret(presented, stableKey(token)));
        void readBody(request, 8 * 1024).then((raw) => {
          if (!owner) return json(response, 404, { ok: false });
          let path: unknown = null;
          let ticket: unknown = null;
          try {
            const body = raw === null ? null : (JSON.parse(raw) as { path?: unknown; ticket?: unknown });
            path = body?.path;
            ticket = body?.ticket;
          } catch { /* an unreadable body names no folder */ }
          const project = projectAt(path);
          const token = project === null ? null : readToken(project.tokenFile);
          // The reason codes never repeat the folder the caller sent.
          if (project === null || token === null) return json(response, 422, { ok: false, reason: "not_a_project" });
          // The owner's key does not reach another project: that project's own ticket is spent first.
          if (typeof ticket !== "string" || !consumeTicket(project.ticketFile, ticket)) return json(response, 404, { ok: false });
          if (runtimeOf(project.folder) === null) return json(response, 409, { ok: false, reason: "runtime_not_running" });
          if (project.id !== firstProjectId(env)) joined.set(project.id, project);
          json(response, 200, { ok: true, projectId: project.id, sessionKey: stableKey(token) });
        });
      });
      server.middlewares.use("/p", (request, response, next) => {
        const match = /^\/([0-9a-f]{64})(\/(?:v1|health)(?:[/?].*)?)$/.exec(request.url ?? "");
        const project = match === null ? undefined : joined.get(match[1]!);
        if (match === null || project === undefined) return next();
        const target = runtimeOf(project.folder);
        if (target === null) {
          response.writeHead(503);
          response.end();
          return;
        }
        forward(request, response, target, match[2]!);
      });
      server.middlewares.use(SESSION_PATH, (request, response) => {
        const query = new URL(request.url ?? "/", "http://localhost").searchParams;
        const presented = query.get("nonce") ?? "";
        const wanted = query.get("project") ?? "";
        const first = firstProjectId(env);
        if (wanted !== "" && wanted !== first) {
          // Another project answers only to ITS key or ITS ticket, never to this run's nonce.
          const project = PROJECT_ID.test(wanted) ? joined.get(wanted) : undefined;
          const token = project === undefined ? null : readToken(project.tokenFile);
          if (project === undefined || token === null || presented.length === 0
            || !(sameSecret(presented, stableKey(token)) || consumeTicket(project.ticketFile, presented))) {
            return json(response, 404, { ok: false });
          }
          return json(response, 200, {
            ok: true, token, project: basename(project.folder), projectId: project.id, folder: folderOf(project.folder), base: `/p/${project.id}`, sessionKey: stableKey(token),
          });
        }
        const token = readToken(tokenPath(env));
        const allowed = token !== null && presented.length > 0
          && (sameSecret(presented, nonce) || sameSecret(presented, stableKey(token)) || consumeTicket(ticketPath(env), presented));
        if (!allowed || token === null) return json(response, 404, { ok: false });
        json(response, 200, { ok: true, token, project: projectName(env), projectId: first, folder: folderOf(firstFolder(env)), base: "", sessionKey: stableKey(token) });
      });
    },
  };
}
