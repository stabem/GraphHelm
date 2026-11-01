import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { LoginGate } from "../App";
import { refusalText } from "../sidebar/Sidebar";
import { STUDIO_OPEN_DEFAULT_PORT, STUDIO_PORT_META, studioOpenCommand } from "./studio-open";

// The commands the page hands the user must open THIS Studio. Off the CLI's default port, a bare
// `graphhelm studio open` is refused (GHCLI032), so the page names its own port.
describe("graphhelm studio open, as the page says it", () => {
  it("names the page's port when it is not the CLI's default", () => {
    expect(renderToStaticMarkup(<LoginGate port="5185" />)).toContain("<code class=\"mono\">graphhelm studio open --port 5185</code>");
    expect(refusalText("runtime_not_running", "5185")).toBe("That project's Runtime is not running. Run graphhelm studio open --port 5185 there first.");
    expect(refusalText("refused", "5185")).toContain("run graphhelm studio open --port 5185 --project there for a new one.");
  });

  it("stays as it was on the CLI's default port, or with no port", () => {
    for (const port of [String(STUDIO_OPEN_DEFAULT_PORT), null]) {
      expect(renderToStaticMarkup(<LoginGate port={port} />)).toContain("<code class=\"mono\">graphhelm studio open</code>");
      expect(refusalText("runtime_not_running", port)).toBe("That project's Runtime is not running. Run graphhelm studio open there first.");
      expect(refusalText("refused", port)).toContain("run graphhelm studio open --project there for a new one.");
    }
    expect(studioOpenCommand("", null)).toBe("graphhelm studio open");
  });

  // #2386: behind a published container port (5183 inside, 15183 outside) the address bar says
  // 15183; the command must name the SERVER's port, which the dev server writes into the page.
  it("names the server's port, never the address bar's", () => {
    const served = (port: string | null) => vi.stubGlobal("document", {
      querySelector: (selector: string) => port !== null && selector === `meta[name="${STUDIO_PORT_META}"]` ? { getAttribute: () => port } : null,
    });
    try {
      vi.stubGlobal("location", { port: "15183" });
      served(String(STUDIO_OPEN_DEFAULT_PORT));
      expect(studioOpenCommand("--project")).toBe("graphhelm studio open --project");
      expect(renderToStaticMarkup(<LoginGate />)).toContain("<code class=\"mono\">graphhelm studio open</code>");
      served("5185");
      expect(studioOpenCommand()).toBe("graphhelm studio open --port 5185");
      served(null);
      expect(studioOpenCommand()).toBe("graphhelm studio open");
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
