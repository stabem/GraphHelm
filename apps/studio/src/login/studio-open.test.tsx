import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { LoginGate } from "../App";
import { refusalText } from "../sidebar/Sidebar";
import { STUDIO_OPEN_DEFAULT_PORT, studioOpenCommand } from "./studio-open";

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
});
