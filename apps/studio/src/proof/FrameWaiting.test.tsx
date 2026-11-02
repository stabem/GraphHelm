import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { FrameWaiting } from "./FrameWaiting";

// #2440: the loader is announced as busy with its words, and carries the static "waiting" word
// that prefers-reduced-motion shows in place of the spinner.
describe("the frame loader", () => {
  it("is busy, labelled 'waiting for frame', with a spinner and the reduced-motion word", () => {
    const html = renderToStaticMarkup(<FrameWaiting />);
    expect(html).toContain('aria-busy="true"');
    expect(html).toContain('aria-label="waiting for frame"');
    expect(html).toContain('role="status"');
    expect(html).toContain('class="spin"');
    expect(html).toContain(">waiting<");
  });
});
