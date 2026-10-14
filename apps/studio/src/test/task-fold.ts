/* #1192, TEST SUPPORT ONLY. The Studio's views now read the Runtime's fold
 * (`GET /v1/executions/{id}/tasks`); the browser fold in `runtime/team-tasks.ts` has no production
 * caller left and stays only to give the Studio's own cells a `LaneFold` from hand-written records,
 * until the fixtures come from `core/execution/tests/tasks.rs` and the TypeScript fold is deleted
 * with its deletion record (slice 3 of plan A1). The parity cell in `tasks-route.test.ts` pins
 * this helper's output to the Runtime's on the same records, so the two cannot drift unseen. */
import { laneBars, type Lane, type TimedTaskEvent } from "../runtime/lane-bars";
import { foldTaskEvents, type TaskEventRecord, type TaskState } from "../runtime/team-tasks";
import type { ClaimLease, LaneFold, Liveness } from "../runtime/tasks-route";

/* #1234, TEST SUPPORT ONLY: the Studio keeps no liveness threshold; the Runtime serves budgets and
 * verdicts (`liveness`, judged in `apps/cli/src/commands/execution/tasks.rs::lane_liveness`, whose
 * edges are pinned by Rust cells). These stand in for a Runtime reply in the Studio's own cells:
 * the same budgets, and a lane silent when it owes (an open bar) and has been quiet for the silent
 * budget, the rule the Runtime applies. */
export const TEST_SILENT_MS = 30 * 60_000;
export const TEST_FREE_MS = 15 * 60_000;
export const TEST_BUDGETS = { silentAfterSeconds: TEST_SILENT_MS / 1000, freeAfterSeconds: TEST_FREE_MS / 1000, stallAfterSeconds: 7200 };

/** A hand-built lane judged the way the Runtime would judge it at `now`, with the served budgets. */
export function judgedLane(lane: Omit<Lane, "silent"> & { silent?: boolean }, now: number, owes = true): Lane {
  return { ...lane, silent: lane.silent ?? (owes && now - lane.lastEventAt >= TEST_SILENT_MS), silentAfterMs: TEST_SILENT_MS, freeAfterMs: TEST_FREE_MS };
}

/** `laneBars` over records that carry their own time, folded the way the Runtime would fold them
 * (the record's `at` is the Runtime's `occurredAt`), for cells written before the route existed.
 * The fold carries the test budgets and a stand-in verdict per lane; `leases` adds claim leases. */
export function laneBarsOf(events: TimedTaskEvent[], now: number, windowMs: number, leases: ClaimLease[] = []): Lane[] {
  const fold = foldForTests(events.map(({ at, ...event }) => ({ ...event, occurredAt: at })));
  const unjudged = laneBars(events, { ...fold, liveness: { budgets: TEST_BUDGETS, lanes: [] }, leases }, now, windowMs);
  const liveness: Liveness = {
    budgets: TEST_BUDGETS,
    lanes: unjudged.map((lane) => {
      const owes = lane.bars.some((bar) => bar.open);
      return { lane: lane.lane, owes, silent: owes && now - lane.lastEventAt >= TEST_SILENT_MS, free: now - lane.lastEventAt < TEST_FREE_MS };
    }),
  };
  return laneBars(events, { ...fold, liveness, leases }, now, windowMs);
}

/** The fold the Runtime would serve for these records: every slice with a stable id (in creation
 * order, the same numbering `fold_task_events_with_routes` uses) and each record's slice. */
export function foldForTests(records: TaskEventRecord[]): LaneFold & { tasks: TaskState[] } {
  const ids = new Map<TaskState, number>();
  const routes: { sequence: number; slice: number }[] = [];
  const tasks = foldTaskEvents(records, (event, state) => {
    let id = ids.get(state);
    if (id === undefined) { id = ids.size; ids.set(state, id); }
    routes.push({ sequence: event.sequence, slice: id });
  });
  const live = new Set(tasks);
  const slices = [...ids.entries()].map(([state, id]) => ({ id, ended: false, absorbed: !live.has(state), state }));
  return { tasks, slices, routes };
}
