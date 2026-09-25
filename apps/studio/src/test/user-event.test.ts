import { describe, expect, it } from "vitest";

/**
 * The inter-event delay stays removed, and the decision stays in one place.
 *
 * `fastUserEvent()` is worth roughly a minute of every gate (the helper carries the measurement),
 * and the way to lose it is not to argue with it — it is for the next test file to write the obvious
 * `import userEvent from "@testing-library/user-event"` and pay the delay again, silently, while
 * every assertion still passes. Nothing about a slow suite is red.
 *
 * TWO VERSIONS OF THIS FILE WERE WRONG BEFORE THIS ONE, both in ways that a green hid:
 *
 *   1. It read the tree with `node:fs`/`__dirname`. That typechecked for me because I ran `tsc -b`
 *      BEFORE adding this file; the real stage then failed at `npm run typecheck` with TS2591 and
 *      TS2304, because this project declares no `@types/node`. `import.meta.glob` is how the rest
 *      of this suite reads sources (see `src/styles.test.ts`) and needs no dependency.
 *   2. It excluded itself by path, because the pattern it searches for appears literally in the
 *      matcher and the guard reported ITSELF as an offender. That exclusion is not needed: Vite
 *      leaves the globbing module out of its own glob. The arrangement below asserts that rather
 *      than assuming it, because the whole sweep rests on it.
 *
 * The population is DERIVED, never written down: a list would go stale the day somebody adds a
 * file, and stale in the direction that reports green.
 */
const TEST_SOURCES = import.meta.glob("../**/*.test.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Every file that uses `userEvent` today. Named so the sweep cannot be green over a short list. */
const KNOWN_USERS = [
  "App.test.tsx",
  "components/models.test.tsx",
  "components/board.test.tsx",
  "components/panel.saybox.test.tsx",
  "components/compose.test.tsx",
  "components/rail.naming.test.tsx",
];

describe("the user-event delay", () => {
  it("ARRANGEMENT: the glob finds the suite and leaves this file out of it", () => {
    const paths = Object.keys(TEST_SOURCES);
    expect(paths.length).toBeGreaterThan(20);
    expect(paths.some((path) => path.endsWith("App.test.tsx"))).toBe(true);
    // Vite excludes the importing module from its own glob. The sweep below depends on it.
    expect(paths.some((path) => path.endsWith("test/user-event.test.ts"))).toBe(false);
  });

  it("CONTROL: every file that uses userEvent is inside the swept population", () => {
    const missing = KNOWN_USERS.filter(
      (name) => !Object.keys(TEST_SOURCES).some((path) => path.endsWith(name)),
    );
    expect(missing).toEqual([]);
  });

  it("is removed by the helper, not by each caller repeating the option", async () => {
    const helper = (await import("./user-event.ts?raw")).default as string;
    expect(helper).toContain("delay: null");
    // A FUNCTION, because vite.config.ts sets `isolate: false`: a shared instance would be one
    // pointer state for every file in the run.
    expect(helper).toMatch(/export function fastUserEvent\(\)/);
  });

  it("is what every test file goes through — none imports user-event directly", () => {
    const offenders = Object.entries(TEST_SOURCES)
      .filter(([, source]) => /from\s+"@testing-library\/user-event"/.test(source))
      .map(([path]) => path);
    expect(offenders).toEqual([]);
  });
});
