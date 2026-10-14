import { describe, expect, it } from "vitest";
import fixtureJson from "../../../../core/execution/tests/fixtures/tasks-route.json";
import liveJson from "../../../../core/execution/tests/fixtures/tasks-live-route.json";
import liveRecords from "../../../../core/execution/tests/fixtures/tasks-live-records.json";
import { foldForTests } from "../test/task-fold";
import { buildWorkGroups } from "./work-groups";
import { RUNTIME_PREDATES_TASKS_ROUTE, tasksReply, type TasksReply } from "./tasks-route";

/* #1192 (plan A1, slice 2): the Studio's cells still fold hand-written records with the browser
 * fold (`src/test/task-fold.ts`) until the fixtures come from Rust; this cell is what keeps that
 * helper honest. `core/execution/tests/tasks.rs::the_route_fixture_matches_the_fold` writes the
 * Runtime's reply for one corpus; the helper must fold the reply's own `records` onto the same
 * slice ids with the same routes and the same live slices, or the two folds have drifted and every
 * Studio cell that trusts the helper is drawing a graph the Runtime would not serve. Cost: one JSON
 * read, milliseconds. */
describe("the tasks route reply and the test fold agree on one corpus (#1192)", () => {
  // The fixture is the Rust test's output, imported as JSON (resolveJsonModule) so the browser
  // tsconfig needs no Node types.
  const fixture = tasksReply(fixtureJson);

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

  /* #1211: seen live on gh-team (469 tasks, 12 open PRs, Lanes kanban all zero). The Studio's
   * TaskState declares `releasedBy?: string` and `claimSequence?: number` — absent means a live
   * claim — and work-groups decides `open` by `releasedBy === undefined`; the Rust reply wrote
   * `null` for every absent Option, so every slice read as released. The cell above compared
   * six fields and let it through. This one compares every task field after a JSON round-trip
   * (which drops `undefined`), so a `null` where the contract says absent is a named diff. */
  it("agrees with the test fold on every task field, null and absent included (#1211)", () => {
    const reply = fixture as TasksReply;
    const ours = foldForTests(reply.records);
    // What is NOT normalised: `releasedBy` and `claimSequence` (absent vs present is the
    // contract the views read). What is: the TS fold writes `pendingReviewer` and
    // `approvedWithRisk` as `null` once CLEARED and leaves them absent when never set, a
    // difference no reader outside the fold consults (grep `=== undefined` on those names:
    // only team-tasks.ts); the Runtime cannot tell the two apart and omits both. `claimedAt`
    // and `leaseSeconds` are the Runtime's (#1193), not in the Studio's type.
    const plain = (tasks: unknown) => JSON.parse(JSON.stringify(tasks)) as Record<string, unknown>[];
    const normalise = (task: Record<string, unknown>) => {
      const { claimedAt: _claimedAt, leaseSeconds: _leaseSeconds, ...rest } = task;
      for (const key of ["pendingReviewer", "approvedWithRisk"]) if (rest[key] === null) delete rest[key];
      return rest;
    };
    expect(plain(reply.tasks).map(normalise)).toEqual(plain(ours.tasks).map(normalise));
    // The live defect, named: no task carries `releasedBy: null`.
    for (const task of plain(reply.tasks)) expect(task).not.toHaveProperty("releasedBy", null);
  });

  it("draws the same open rows from the reply as from the test fold (#1211)", () => {
    const reply = fixture as TasksReply;
    const rows = (tasks: Parameters<typeof buildWorkGroups>[0]) =>
      buildWorkGroups(tasks, []).flatMap((g) => g.rows.filter((r) => r.open).map((r) => r.key)).sort();
    // This corpus ends with its PR merged, one claim released and one closed, so both sides are
    // empty here; the live excerpt below is the cell with open rows.
    expect(rows(reply.tasks)).toEqual(rows(foldForTests(reply.records).tasks));
  });

  /* #1211, with the run's own data: the records of the twelve open-PR tasks of gh-team on
   * 2026-10-14, folded and serialised by the Rust corpus test (`tasks-live-route.json`). On the
   * parent every slice came back with `releasedBy: null` and this drew zero rows. */
  it("draws the twelve open PRs of the live gh-team excerpt (#1211)", () => {
    const reply = tasksReply(liveJson) as TasksReply;
    expect(reply).not.toBeNull();
    const open = buildWorkGroups(reply.tasks, []).flatMap((g) => g.rows.filter((r) => r.open).map((r) => r.key)).sort();
    expect(open).toEqual([...liveRecords.expectedOpenKeys].sort());
    for (const task of reply.tasks) expect(task).not.toHaveProperty("releasedBy", null);
  });

  it("names the older-Runtime sentence the views show in place of the board", () => {
    expect(RUNTIME_PREDATES_TASKS_ROUTE).toBe("Runtime predates the tasks route · restart it on the current binary");
    expect(tasksReply({ executionId: "x", tasks: [] })).toBeNull();
  });
});
