/// <reference types="vitest/config" />
import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

import { devSession } from "./dev-session.ts";

/**
 * The page talks only to its own origin: `/v1` and `/health` are proxied to the Runtime, so the
 * bearer token never rides a cross-origin request and the Runtime needs no CORS relaxation.
 * Port 5185 by default so it runs beside the old Studio (5183) and its journey fixture (5184).
 */
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, ".", "");
  const target = env.GRAPHHELM_RUNTIME_URL || "http://127.0.0.1:8080";
  const proxy = {
    "/v1": { target, changeOrigin: false },
    "/health": { target, changeOrigin: false },
  };
  return {
    plugins: [react(), devSession(env)],
    server: { host: "127.0.0.1", port: 5185, strictPort: true, proxy },
    preview: { host: "127.0.0.1", port: 5185, strictPort: true, proxy },
    test: { environment: "node", maxWorkers: 1, css: { include: [/styles.css/] } },
  };
});
