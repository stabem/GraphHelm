/// <reference types="vitest/config" />
import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

import { devSession } from "./dev-session.ts";
import { studioPortMeta } from "./studio-port-meta.ts";
import { STUDIO_OPEN_DEFAULT_PORT } from "./src/login/studio-open.ts";

/**
 * The page talks only to its own origin: `/v1` and `/health` are proxied to the Runtime, so the
 * bearer token never rides a cross-origin request and the Runtime needs no CORS relaxation.
 * It serves on the port `graphhelm studio open` looks for by default, so that command finds it.
 */
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, ".", "");
  return {
    plugins: [react(), devSession(env), studioPortMeta()],
    server: { host: "127.0.0.1", port: STUDIO_OPEN_DEFAULT_PORT, strictPort: true, cors: false },
    preview: { host: "127.0.0.1", port: STUDIO_OPEN_DEFAULT_PORT, strictPort: true, cors: false },
    test: { environment: "node", maxWorkers: 1, css: { include: [/styles.css/] } },
  };
});
