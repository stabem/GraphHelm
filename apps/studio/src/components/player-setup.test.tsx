import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PlayerSetup } from "./journey-flows";

describe("PlayerSetup (#519)", () => {
  it("says what it changes before anything runs, then names the files it changed", async () => {
    const setup = vi.fn().mockResolvedValue({ changed: [{ path: "package.json", change: "created" }, { path: ".graphhelm/observers/journey_driver.mjs", change: "created" }] });
    const onDone = vi.fn();
    render(<PlayerSetup setup={setup} onDone={onDone} />);
    fireEvent.click(screen.getByRole("button", { name: "Set up journey player" }));
    expect(setup).not.toHaveBeenCalled();
    expect(screen.getByText(/Adds @playwright\/test to package\.json .* downloads Chromium, ~150 MB/)).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Install" })); });
    expect(setup).toHaveBeenCalledTimes(1);
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toContain("Changed: package.json (created), .graphhelm/observers/journey_driver.mjs (created).");
  });

  it("Cancel runs nothing, and a failure says why and can be retried", async () => {
    const setup = vi.fn().mockRejectedValue(new Error("an install step failed"));
    render(<PlayerSetup setup={setup} onDone={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Set up journey player" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(setup).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Set up journey player" }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Install" })); });
    expect(screen.getByRole("alert").textContent).toContain("an install step failed");
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });
});
