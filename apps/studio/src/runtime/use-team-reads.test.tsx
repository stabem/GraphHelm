import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { RuntimeClient } from "./client";
import type { EventPage, RuntimeEvent } from "./types";
import { useTeamReads } from "./use-team-reads";

const scanned = vi.hoisted(() => ({ count: 0 }));
vi.mock("./team-tasks", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./team-tasks")>();
  return { ...actual, isTaskEventSignal: (event: RuntimeEvent) => { scanned.count += 1; return actual.isTaskEventSignal(event); } };
});

const event = (sequence: number, kind: string): RuntimeEvent => ({
  sequence, kind: "signal_recorded", payload: { kind }, occurredAt: null, actorId: "lane", actorType: "agent",
  idempotencyKey: `k${sequence}`, eventId: `e${sequence}`, evidenceRefs: [],
} as unknown as RuntimeEvent);

describe("useTeamReads", () => {
  /** #1560: on the live gh-team run every poll that brought one new event re-fetched the whole task
   * projection (`/tasks?full=1`, 4.8 MB, 26 times in one two-minute load). The projection only
   * changes when a `task.*` record lands, so other new events must not fetch it again. */
  it("fetches the task projection again only when a task record lands", async () => {
    const getTasks = vi.fn(async () => ({ tasks: [], records: [], slices: [], routes: [], rejected: 0 }));
    const clientRef = { current: { getTasks } as unknown as RuntimeClient };
    const openEvidence = vi.fn(async () => { throw new Error("no evidence in this test"); });
    const page = (events: RuntimeEvent[]): EventPage => ({ head: events.at(-1)?.sequence ?? 0, events });
    const first = [event(1, "task.claimed"), event(2, "note")];
    const { rerender } = renderHook(({ events }) => useTeamReads({ clientRef, selected: "run", events, eventList: events.events, openEvidence }), {
      initialProps: { events: page(first) },
    });
    await waitFor(() => expect(getTasks).toHaveBeenCalledTimes(1));

    const quiet = [...first, event(3, "note"), event(4, "agent_message")];
    rerender({ events: page(quiet) });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(getTasks).toHaveBeenCalledTimes(1);

    rerender({ events: page([...quiet, event(5, "task.planned")]) });
    await waitFor(() => expect(getTasks).toHaveBeenCalledTimes(2));
  });

  /** #1727 review: the task key scans every event (~55k on gh-team). A re-render that brings the
   * same event list must not scan it again. */
  it("does not rescan the event list on a re-render with the same events", async () => {
    const getTasks = vi.fn(async () => ({ tasks: [], records: [], slices: [], routes: [], rejected: 0 }));
    const clientRef = { current: { getTasks } as unknown as RuntimeClient };
    const events = [event(1, "task.claimed"), event(2, "note"), event(3, "note")];
    const page: EventPage = { head: 3, events };
    const { rerender } = renderHook(({ tick }) => { void tick; return useTeamReads({ clientRef, selected: "run", events: page, eventList: events, openEvidence: async () => { throw new Error("none"); } }); }, {
      initialProps: { tick: 0 },
    });
    await waitFor(() => expect(getTasks).toHaveBeenCalledTimes(1));
    const after = scanned.count;
    rerender({ tick: 1 });
    rerender({ tick: 2 });
    expect(scanned.count).toBe(after);
  });
});
