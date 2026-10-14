/** #1171: the gate is reached from the Studio's own dev server without `?session=`, so it must point at that link. */
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Connect } from "./Connect";

describe("Connect gate note", () => {
  it("sends the user to `graphhelm studio open` (#1264), not to npm run dev", () => {
    render(<Connect onConnect={() => {}} busy={false} error="" />);
    const note = screen.getByText(/opens this page already connected/);
    expect(note.textContent).toContain("graphhelm studio open");
    expect(note.textContent).not.toContain("npm run dev");
    expect(note.textContent).toMatch(/never put\s+the bearer token in a URL/i);
  });
});
