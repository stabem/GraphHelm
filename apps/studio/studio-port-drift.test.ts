import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import viteConfig from "./vite.config.ts";
import { STUDIO_OPEN_DEFAULT_PORT, STUDIO_PORT_META } from "./src/login/studio-open.ts";
import { studioPortMeta } from "./studio-port-meta.ts";

// `graphhelm studio open` finds the Studio only on the port it looks for. The CLI's default and
// the Studio's own servers must agree; this fails the moment either side changes alone.
const repo = (path: string) => readFileSync(fileURLToPath(new URL(`../../${path}`, import.meta.url)), "utf8");

describe("the Studio's port and the CLI's default agree", () => {
  it("the CLI's studio open default equals the Studio's", () => {
    const studioRs = /const STUDIO_PORT: u16 = (\d+);/.exec(repo("apps/cli/src/commands/studio.rs"))?.[1];
    const openArg = /pub struct StudioOpenArgs \{[\s\S]*?#\[arg\(long, default_value_t = (\d+)\)\]\s*pub port: u16/.exec(repo("apps/cli/src/args.rs"))?.[1];
    expect(Number(studioRs)).toBe(STUDIO_OPEN_DEFAULT_PORT);
    expect(Number(openArg)).toBe(STUDIO_OPEN_DEFAULT_PORT);
  });

  it("the dev and preview servers serve on that port", () => {
    const config = typeof viteConfig === "function" ? viteConfig({ mode: "test", command: "serve" }) : viteConfig;
    const resolved = config as { server: { port: number; strictPort: boolean }; preview: { port: number; strictPort: boolean } };
    expect(resolved.server.port).toBe(STUDIO_OPEN_DEFAULT_PORT);
    expect(resolved.preview.port).toBe(STUDIO_OPEN_DEFAULT_PORT);
    expect(resolved.server.strictPort && resolved.preview.strictPort).toBe(true);
  });

  // #2386: the page is told the port the server listens on, so a gate behind a published port
  // does not name the address bar's.
  it("the page carries the port its dev server listens on", () => {
    const transform = studioPortMeta().transformIndexHtml as unknown as (html: string, context: unknown) => Array<{ attrs: { name: string; content: string } }>;
    const told = (context: unknown) => transform("", context).map((tag) => [tag.attrs.name, tag.attrs.content]);
    expect(told({ server: { config: { server: { port: 5185 } } } })).toEqual([[STUDIO_PORT_META, "5185"]]);
    expect(told({})).toEqual([[STUDIO_PORT_META, String(STUDIO_OPEN_DEFAULT_PORT)]]);
  });
});
