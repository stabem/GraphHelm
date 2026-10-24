import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { BoardColumn } from "./BoardColumn";

describe("board column fold (#1996)", () => {
  it("a column with no cards folds to a strip with its name, count 0, tooltip and aria label", () => {
    const html = renderToStaticMarkup(<BoardColumn name="silent" label="Silent" count={0}><span className="none">none</span></BoardColumn>);
    expect(html).toContain("board-column folded silent");
    expect(html).toContain('aria-label="Silent, empty"');
    expect(html).toContain("title=\"Silent · none\"");
    expect(html).toContain('<span class="mono count">0</span>');
    expect(html).toContain("folded-name");
    expect(html).not.toContain("<h3");
  });

  it("a column with cards stays open with its header, count and cards", () => {
    const html = renderToStaticMarkup(<BoardColumn name="review" label="Review" count={2}><ul aria-label="Review"><li>a</li><li>b</li></ul></BoardColumn>);
    expect(html).not.toContain("folded");
    expect(html).toContain("<h3>");
    expect(html).toContain('<span class="mono count">2</span>');
    expect(html).toContain('<ul aria-label="Review">');
  });
});
