import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";

import { scaled, UI_WAIT_BASE_MS } from "./time-scale";

/** #549: `findBy*` and `waitFor` wait for their condition and return the moment it holds; only
 * the point at which "it never showed" is declared moves with GRAPHHELM_TEST_TIME_SCALE. */
configure({ asyncUtilTimeout: scaled(UI_WAIT_BASE_MS) });

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
