import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { Composer } from "./compose";

/**
 * Every control the composer offers is gated on `busy` - INCLUDING discard.
 *
 * Discard was the one sibling without the gate (PR #467 review): while `startTask` was in
 * flight the operator could throw the draft away, and the pending call then landed state for a
 * task that no longer existed on screen. The N-review lesson attached to this exact finding:
 * checking that the parent passes `busy` down is not checking that a child uses it.
 */
describe("the composer under a pending start", () => {
  const mount = (busy: boolean) =>
    render(
      <Composer choice={null} busy={busy} error="" onSend={vi.fn()} onCancel={vi.fn()} />,
    );

  it("gates discard while the start is in flight", () => {
    mount(true);
    expect(screen.getByRole("button", { name: "Discard this task" })).toBeDisabled();
  });

  it("frees discard the moment nothing is pending", () => {
    mount(false);
    expect(screen.getByRole("button", { name: "Discard this task" })).toBeEnabled();
  });
});
