import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { foldForTests } from "../test/task-fold";
import { RUNTIME_PREDATES_TASKS_ROUTE, tasksReply, type TasksReply } from "./tasks-route";

/* #1192 (plan A1, slice 2): the Studio's cells still fold hand-written records with the browser
 * fold (`src/test/task-fold.ts`) until the fixtures come from Rust; this cell is what keeps that
 * helper honest. `core/execution/tests/tasks.rs::the_route_fixture_matches_the_fold` writes the
 * Runtime's reply for one corpus; the helper must fold the reply's own `records` onto the same
 * slice ids with the same routes and the same live slices, or the two folds have drifted and every
 * Studio cell that trusts the helper is drawing a graph the Runtime would not serve. Cost: one JSON
 * read, milliseconds. */
describe("the tasks route reply and the test fold agree on one corpus (#1192)", () => {
  // Vitest runs with `apps/studio` as its working directory; the fixture is the Rust test's output.
  const fixture = tasksReply(JSON.parse(readFileSync(resolve(process.cwd(), "../../core/execution/tests/fixtures/tasks-route.json"), "utf8")));

  it("is a reply this Studio reads", () => {
    expect(fixture).not.toBeNull();
    expect((fixture as TasksReply).records.length).toBeGreaterThan(10);
  });

  it("routes every record to the slice the Runtime routed it to, and lists the same live slices", () => {
    const reply = fixture as TasksReply;
    const ours = foldForTests(reply.records);
    const routeOf = (routes: { sequence: number; slice: number }[]) => Object.fromEntries(routes.map((r) => [r.sequence, r.slice]));
    expect(routeOf(ours.routes)).toEqual(routeOf(reply.routes));
    expect(ours.slices.map((s) => s.id)).toEqual(reply.slices.map((s) => s.id));
    expect(ours.tasks.map((t) => [t.key, t.step, t.pr, t.lane, t.headSha, t.blockedBy?.headSha ?? null]))
      .toEqual(reply.tasks.map((t) => [t.key, t.step, t.pr, t.lane, t.headSha, t.blockedBy?.headSha ?? null]));
  });

  it("names the older-Runtime sentence the views show in place of the board", () => {
    expect(RUNTIME_PREDATES_TASKS_ROUTE).toBe("Runtime predates the tasks route · restart it on the current binary");
    expect(tasksReply({ executionId: "x", tasks: [] })).toBeNull();
  });
});
