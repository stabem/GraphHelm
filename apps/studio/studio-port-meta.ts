import type { Plugin } from "vite";

import { STUDIO_OPEN_DEFAULT_PORT, STUDIO_PORT_META } from "./src/login/studio-open.ts";

/** Writes the port this server listens on into the page (#2386), so the connect gate's command
 * names the server's port and not the address bar's, which differs behind a published container
 * port, a tunnel or a proxy. A page not served by a dev server (a build) carries the default. */
export function studioPortMeta(): Plugin {
  return {
    name: "graphhelm-studio-port-meta",
    transformIndexHtml: (_html, context) => [{
      tag: "meta",
      attrs: { name: STUDIO_PORT_META, content: String(context.server?.config.server.port ?? STUDIO_OPEN_DEFAULT_PORT) },
      injectTo: "head",
    }],
  };
}
