import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { countdown, Empty, Failure, failureKind, Loading, ReadFailure, RETRY_SECONDS } from "./ViewState";

describe("view states (#1934)", () => {
  it("tells the Runtime not answering apart from a restart and from an answer it could not use", () => {
    expect(failureKind("The Runtime is not answering.")).toBe("offline");
    expect(failureKind("The Runtime is restarting on a new version.")).toBe("restarting");
    expect(failureKind("The Runtime replied 200 with a body the Studio could not read.")).toBe("error");
  });

  it("counts the automatic retry down and fires it once at the end", () => {
    let left = RETRY_SECONDS;
    const fired: number[] = [];
    for (let tick = 1; tick <= RETRY_SECONDS * 2; tick += 1) {
      const next = countdown(left);
      if (next.fire) fired.push(tick);
      left = next.left;
    }
    expect(fired).toEqual([RETRY_SECONDS, RETRY_SECONDS * 2]);
  });

  it("an offline read shows Retry and the countdown, not a bare sentence", () => {
    const html = renderToStaticMarkup(<ReadFailure what="lanes list" route="/v1/x" message="The Runtime is not answering." onRetry={() => undefined} />);
    expect(html).toContain('aria-label="Runtime not answering"');
    expect(html).toContain(">Retry</button>");
    expect(html).toContain(`Retrying in ${RETRY_SECONDS} s`);
  });

  it("loading has a status spinner and a hidden skeleton", () => {
    const html = renderToStaticMarkup(<Loading text="Reading the lanes…" />);
    expect(html).toContain('aria-busy="true"');
    expect(html).toMatch(/role="status"[^>]*><span class="spin" aria-hidden="true">/);
    expect(html).toContain('class="skeleton" aria-hidden="true"');
  });

  it("empty offers Start a team run only when the Studio can start one", () => {
    expect(renderToStaticMarkup(<Empty title="No work yet" hint="h" onStart={() => undefined} />)).toContain("Start a team run");
    expect(renderToStaticMarkup(<Empty title="No work yet" hint="h" onStart={null} />)).not.toContain("Start a team run");
  });

  it("an error folds its detail and offers Copy details", () => {
    const html = renderToStaticMarkup(<Failure title="The lanes list could not be read" message="m" detail={"GET /v1/lanes\nerror: m"} />);
    expect(html).toContain("<details");
    expect(html).toContain("GET /v1/lanes\nerror: m");
    expect(html).toContain("Copy details");
  });
});
