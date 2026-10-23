import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AddFolder } from "./Sidebar";

describe("the add-folder ticket (#1898)", () => {
  it("is a real password input in every browser, still addressed as a textbox by its label", () => {
    const html = renderToStaticMarkup(<AddFolder onAdd={async () => null} open setOpen={() => undefined} />);
    const ticket = /<input[^>]*aria-label="Ticket from graphhelm studio open --project"[^>]*>/.exec(html)?.[0] ?? "";
    expect(ticket).toContain('type="password"');
    expect(ticket).toContain('role="textbox"');
    expect(ticket).not.toContain("masked");
  });
});
