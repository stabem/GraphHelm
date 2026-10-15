import { render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import { fastUserEvent } from "../test/user-event";
import { AddProject } from "./addproject";

// Its own instance: see the helper for why this is not a shared const.
const userEvent = fastUserEvent();

/** The button exists because a project is a folder and the operator has more than one. What it
 * can do today is tell them exactly what to run; it must not pretend to do more. Moved from
 * App.test.tsx (plan F2): the commands are this component's, and App.test.tsx keeps only the
 * button that opens it. DOM only, no network. */
it("says what adding a project folder actually takes", async () => {
  render(<AddProject onClose={vi.fn()} />);

  const panel = screen.getByLabelText("Add project folder");
  await userEvent.type(within(panel).getByLabelText(/^folder$/i), "F:/projects/example/dale-api-base");
  expect(
    within(panel).getByText(
      /Set-Location -LiteralPath 'F:\/projects\/example\/dale-api-base'; if \(\$\?\) \{ graphhelm init --bind 127\.0\.0\.1:8792; if \(\$\?\) \{ graphhelm studio start --bind 127\.0\.0\.1:8792 \} \}/i,
    ),
  ).toBeInTheDocument();
  expect(
    within(panel).getByText(
      /cd 'F:\/projects\/example\/dale-api-base' && graphhelm init --bind 127\.0\.0\.1:8792 && graphhelm studio start --bind 127\.0\.0\.1:8792/i,
    ),
  ).toBeInTheDocument();
  expect(within(panel).getByLabelText(/^folder$/i)).toHaveValue("F:/projects/example/dale-api-base");
  // No bare `serve` on the port this page's Runtime already holds, and no promise of a pickup
  // this Studio cannot make.
  expect(panel).not.toHaveTextContent("--bind 127.0.0.1:8791");
  expect(panel).not.toHaveTextContent("picks it up");
});
