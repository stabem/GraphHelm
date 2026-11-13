import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ActionError } from "./ActionError";

describe("shared action error (#2445)", () => {
  it("shows the failed action, known Runtime detail, next step and dismiss control in one alert", () => {
    const html = renderToStaticMarkup(<ActionError
      error={{
        title: "Run could not start",
        code: "preview.flow_invalid",
        cause: "The flow has an invalid action at /edges/0.",
        nextStep: "Check the flow, then run this command.",
        command: "graphhelm journey validate --all",
      }}
      onDismiss={() => undefined}
    />);

    expect(html).toContain('role="alert"');
    expect(html).toContain("Run could not start");
    expect(html).toContain("preview.flow_invalid");
    expect(html).toContain("The flow has an invalid action at /edges/0.");
    expect(html).toContain("graphhelm journey validate --all");
    expect(html).toContain(">Dismiss</button>");
  });

  it("names the alert by its title, so a reader of the page can find it by what failed", () => {
    const html = renderToStaticMarkup(<ActionError
      error={{ title: "Decision could not be recorded", code: null, cause: "Say why.", nextStep: "Try again." }}
      onDismiss={() => undefined}
    />);
    const labelled = /role="alert"[^>]*aria-labelledby="([^"]+)"|aria-labelledby="([^"]+)"[^>]*role="alert"/.exec(html);
    const id = labelled?.[1] ?? labelled?.[2];
    expect(id).toBeDefined();
    expect(html).toMatch(new RegExp(`<strong[^>]*id="${id!.replace(/[:]/g, "\\:")}"[^>]*>Decision could not be recorded</strong>`));
  });

  it("has no Dismiss button where the caller gives no way to dismiss, and still offers Retry (#2913)", () => {
    const html = renderToStaticMarkup(<ActionError
      error={{ title: "Journeys could not be read", code: null, cause: "The Runtime did not answer.", nextStep: "Retry reading the journeys." }}
      onRetry={() => undefined}
    />);

    expect(html).not.toContain("Dismiss");
    expect(html).toContain(">Retry</button>");
  });

  it("says Runtime code is unavailable for a legacy failure without inventing one", () => {
    const html = renderToStaticMarkup(<ActionError
      error={{ title: "Action failed", code: null, cause: "The Runtime refused.", nextStep: "Try the action again." }}
      onDismiss={() => undefined}
    />);

    expect(html).toContain("Code not supplied");
    expect(html).not.toContain("preview.");
  });
});
