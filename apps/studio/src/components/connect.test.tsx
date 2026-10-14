/** #1171: the gate is reached from the Studio's own dev server without `?session=`, so it must point at that link. */
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Connect } from "./Connect";

describe("Connect gate note", () => {
  it("sends the user to the session link studio start printed, not to npm run dev", () => {
    render(<Connect onConnect={() => {}} busy={false} error="" />);
    const note = screen.getByText(/opened without its session link/);
    expect(note.textContent).toContain("graphhelm studio start");
    expect(note.textContent).toContain("?session=");
    expect(note.textContent).not.toContain("npm run dev");
    expect(note.textContent).toContain("never put the bearer token in a URL");
  });
});
