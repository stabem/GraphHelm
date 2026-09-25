import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

/**
 * Unmount between tests.
 *
 * Testing Library only auto-registers this when the runner exposes globals, and this project
 * runs vitest without them. Without it every `render` leaves its tree in the document and the
 * NEXT test's `getByRole` finds two matching elements - a failure that looks like a component
 * bug and is not one.
 */
afterEach(() => {
  cleanup();
});
