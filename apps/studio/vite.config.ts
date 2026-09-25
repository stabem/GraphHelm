import { randomBytes, timingSafeEqual } from "node:crypto";
import { readFileSync } from "node:fs";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig, loadEnv, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

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
function devSession(env: Record<string, string>): Plugin {
  // One nonce per server run. A launcher may inject its own so it can open the browser at the
  // right URL; otherwise it is random and lives only in this process and its terminal output.
  const provided = (env.GRAPHHELM_STUDIO_SESSION_NONCE || "").trim();
  const nonce = provided || randomBytes(16).toString("hex");
  return {
    name: "graphhelm-studio-dev-session",
    apply: "serve",
    configureServer(server) {
      server.httpServer?.once("listening", () => {
        // Printed ONLY when the nonce was minted here - then the terminal is the sole channel
        // that can carry it to the person, and it is the owner's own ephemeral stdout, the same
        // trust class as the token file they could `cat`. A launcher-provided nonce is never
        // echoed: the launcher already holds it, and repeating a credential into a log that
        // gains nothing is pure exposure (PR #467 review). The residual print is a deliberate,
        // dev-only trade-off: apply:"serve" only, dead with the process, absent from the bundle.
        if (provided) return;
        const address = server.httpServer?.address();
        const port = typeof address === "object" && address !== null ? address.port : 4173;
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
        // The nonce gate, before the file is even read. Constant-time compare: this is a
        // credential check, however small the window.
        const presented = new URL(request.url ?? "/", "http://localhost").searchParams.get("nonce") ?? "";
        const left = Buffer.from(presented, "utf8");
        const right = Buffer.from(nonce, "utf8");
        if (left.length !== right.length || !timingSafeEqual(left, right)) {
          refuse();
          return;
        }
        const path = tokenPath(env);
        let token: string | null = null;
        if (path !== null) {
          try {
            const raw = readFileSync(path, "utf8").trim();
            // A token is one line. Anything else is not the file we meant to read, and guessing
            // which line to take is how a stray file becomes a credential.
            if (raw.length > 0 && raw.length <= 512 && !raw.includes("\n")) token = raw;
          } catch {
            token = null;
          }
        }
        if (token === null) {
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
  const proxy = {
    "/v1": { target, changeOrigin: false },
    "/health": { target, changeOrigin: false },
  };

  return {
    plugins: [react(), devSession(env), devWebMcpShim()],
    // POLLING IS LOAD-BEARING on this machine: the F: drive drops fs events, and the dev
    // server spent a whole afternoon serving stale modules through hard reloads (verified:
    // disk and served output disagreed). Polling costs a little CPU and ends that class.
    server: {
      host: "127.0.0.1",
      port: 4173,
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
    preview: { host: "127.0.0.1", port: 4173, strictPort: true, proxy },
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
      // #1095: Vitest 4.1.11 gives a worker 60 seconds to answer its startup handshake. Isolation
      // creates one worker per file, so a loaded gate gets one chance to miss that fixed wait for
      // every file. Reuse one thread for the run instead. A worker that cannot start still fails
      // the suite; this adds no retry and hides no assertion failure.
      pool: "threads",
      isolate: false,
    },
  };
});
