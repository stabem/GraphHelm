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
 * Same contract as the old Studio's dev-session plugin (apps/studio/vite.config.ts, #1264), reduced
 * to the first project; serving several projects on one port is #1800 (S2). `apply: "serve"` keeps
 * the route out of any production bundle: a built page has no way to obtain the token.
 */
import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { readFileSync, unlinkSync } from "node:fs";
import { basename, dirname, join } from "node:path";

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
      server.middlewares.use(SESSION_PATH, (request, response) => {
        response.setHeader("Content-Type", "application/json");
        response.setHeader("Cache-Control", "no-store");
        const presented = new URL(request.url ?? "/", "http://localhost").searchParams.get("nonce") ?? "";
        const token = readToken(tokenPath(env));
        const allowed = token !== null && presented.length > 0
          && (sameSecret(presented, nonce) || sameSecret(presented, stableKey(token)) || consumeTicket(ticketPath(env), presented));
        if (!allowed || token === null) {
          response.statusCode = 404;
          response.end(JSON.stringify({ ok: false }));
          return;
        }
        response.statusCode = 200;
        response.end(JSON.stringify({ ok: true, token, project: projectName(env), sessionKey: stableKey(token) }));
      });
    },
  };
}
