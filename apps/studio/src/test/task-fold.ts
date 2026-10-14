/* #1192, TEST SUPPORT ONLY. The Studio's views now read the Runtime's fold
 * (`GET /v1/executions/{id}/tasks`); the browser fold in `runtime/team-tasks.ts` has no production
 * caller left and stays only to give the Studio's own cells a `LaneFold` from hand-written records,
 * until the fixtures come from `core/execution/tests/tasks.rs` and the TypeScript fold is deleted
 * with its deletion record (slice 3 of plan A1). The parity cell in `tasks-route.test.ts` pins
 * this helper's output to the Runtime's on the same records, so the two cannot drift unseen. */
import { laneBars, type Lane, type TimedTaskEvent } from "../runtime/lane-bars";
import { foldTaskEvents, type TaskEventRecord, type TaskState } from "../runtime/team-tasks";
import type { LaneFold } from "../runtime/tasks-route";

/** `laneBars` over records that carry their own time, folded the way the Runtime would fold them
 * (the record's `at` is the Runtime's `occurredAt`), for cells written before the route existed. */
export function laneBarsOf(events: TimedTaskEvent[], now: number, windowMs: number): Lane[] {
  return laneBars(events, foldForTests(events.map(({ at, ...event }) => ({ ...event, occurredAt: at }))), now, windowMs);
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
