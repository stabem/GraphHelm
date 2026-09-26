import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { awaitingReply, groupTurns, pinnedToLatest, resetPanelCaches, useEnvelopes } from "./panel";
import type { EvidenceContent, RuntimeEvent } from "../runtime/types";

function event(sequence: number, kind: string, payload: Record<string, unknown> = {}): RuntimeEvent {
  return {
    sequence,
    kind,
    payload,
    occurredAt: `2026-08-30T12:00:0${sequence % 10}Z`,
    actorId: kind === "signal_recorded" ? "codex" : "system-runtime",
    actorType: kind === "signal_recorded" ? "agent" : "system",
    idempotencyKey: `k${sequence}`,
    eventId: `event-${sequence}`,
    evidenceRefs: [],
  } as RuntimeEvent;
}

it("shows the newest sealed report while an older evidence read is still pending", async () => {
  resetPanelCaches();
  const older = event(1, "signal_recorded");
  const newer = event(2, "signal_recorded");
  older.evidenceRefs = ["old"];
  newer.evidenceRefs = ["new"];
  const never = new Promise<EvidenceContent>(() => {});
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => id === "old" ? never : {
    evidenceId: "new",
    content: JSON.stringify({ type: "operator_note", description: "Checking the branch", source: { id: "reviewer", type: "user" } }),
    contentSha256: "hash",
    mediaType: "application/json",
    sensitivity: "confidential",
  });
  const { result } = renderHook(() => useEnvelopes([older, newer], "run", open));
  await waitFor(() => expect(result.current[2]?.text).toBe("Checking the branch"));
  expect(result.current[1]).toBeUndefined();
  expect(open.mock.calls[0][1]).toBe("new");
});

/**
 * Pure, because jsdom lays nothing out: every height there is zero, so an "is it scrolled"
 * assertion in a component test measures the test double, not the behavior. The predicate is
 * tested here; that the thread WIRES it is verified against a real browser, where heights exist.
 */
describe("pinnedToLatest", () => {
  it("stays pinned while the reader is at or near the newest message", () => {
    expect(pinnedToLatest(1000, 600, 400)).toBe(true);
    expect(pinnedToLatest(1000, 530, 400)).toBe(true);
  });

  it("unpins when the reader deliberately scrolled up into history", () => {
    expect(pinnedToLatest(1000, 0, 400)).toBe(false);
    expect(pinnedToLatest(1000, 500, 400)).toBe(false);
  });

  it("treats an unmeasured layout as pinned, so the first paint lands at the end", () => {
    expect(pinnedToLatest(0, 0, 0)).toBe(true);
  });
});

/**
 * The dialogue and the stage directions. Lifecycle narration interleaved the human conversation at
 * equal weight — the owner's own screen showed four "Recorded … on start" lines and two doorbells
 * between two spoken sentences (2026-08-30). Consecutive machine events fold into one strip; the
 * folding is a pure pre-pass so the scroll pinning (two prior regressions) never sees it.
 */
describe("groupTurns", () => {
  it("keeps every event, exactly once, in the original order", () => {
    const events = [
      event(1, "execution_started"),
      event(2, "wake_lease"),
      event(3, "signal_recorded", { kind: "operator_note" }),
      event(4, "wake_lease_consumed"),
    ];
    const flat = groupTurns(events).flatMap((group) =>
      group.kind === "stage" ? group.events : [group.event],
    );
    expect(flat.map((entry) => entry.sequence)).toEqual([1, 2, 3, 4]);
  });

  it("folds consecutive machine events, and never a spoken one", () => {
    const groups = groupTurns([
      event(1, "execution_started"),
      event(2, "execution_form_declared"),
      event(3, "wake_lease"),
      event(4, "signal_recorded", { kind: "operator_note" }),
      event(5, "signal_recorded", { kind: "operator_note" }),
    ]);
    expect(groups[0]).toMatchObject({ kind: "stage" });
    expect((groups[0] as { events: unknown[] }).events).toHaveLength(3);
    expect(groups[1]).toMatchObject({ kind: "talk" });
    expect(groups[2]).toMatchObject({ kind: "talk" });
  });

  it("leaves an alarming transition out of any fold — it answers 'why does this run need me'", () => {
    const groups = groupTurns([
      event(1, "wake_lease"),
      event(2, "node_outcome_recorded", { nodeId: "start", outcome: "needs_input", nextState: "waiting_input" }),
      event(3, "wake_lease_consumed"),
      event(4, "signal_recorded", { kind: "operator_note" }),
    ]);
    // The alarming event stands alone; its calm neighbours are too few to fold around it.
    expect(groups.every((group) => group.kind !== "stage" || !group.events.some(
      (entry) => (entry.payload as { nextState?: string }).nextState === "waiting_input",
    ))).toBe(true);
  });

  it("never folds the trailing machine events — they are what is happening NOW", () => {
    const groups = groupTurns([
      event(1, "signal_recorded", { kind: "operator_note" }),
      event(2, "wake_lease"),
      event(3, "wake_lease_consumed"),
      event(4, "sweep_performed"),
    ]);
    expect(groups.filter((group) => group.kind === "stage")).toHaveLength(0);
  });

  it("a lone machine event between speech stays a plain turn", () => {
    const groups = groupTurns([
      event(1, "signal_recorded", { kind: "operator_note" }),
      event(2, "wake_lease"),
      event(3, "signal_recorded", { kind: "operator_note" }),
    ]);
    expect(groups).toHaveLength(3);
    expect(groups.every((group) => group.kind === "talk")).toBe(true);
  });
});

/**
 * The waiting receipt is settled only by an answer that is actually FOR the operator. Three
 * round-3 reviewers, blind to each other: any later non-owner signal — two agents chatting in
 * their own bubble — cleared "delivered, waiting for a reply" while the operator's question sat
 * unanswered. The same author-blindness the ledger was cured of, surviving in the pill below it.
 */
describe("awaitingReply", () => {
  const owner = (sequence: number, signalId: string): RuntimeEvent =>
    ({
      ...event(sequence, "signal_recorded", { kind: "operator_note", signalId }),
      actorId: "studio-operator",
      actorType: "owner",
    }) as RuntimeEvent;

  it("stays waiting while agents only talk among themselves", () => {
    const events = [owner(1, "q-1"), event(2, "signal_recorded", { kind: "operator_note" })];
    const envelopes = { 2: { to: "claude-revisor", replyTo: null, text: "entre nos" } };
    expect(awaitingReply(events, envelopes)).toBe(true);
  });

  it("settles when a later signal is addressed to the operator", () => {
    const events = [owner(1, "q-1"), event(2, "signal_recorded", { kind: "operator_note" })];
    const envelopes = { 2: { to: "studio-operator", replyTo: null, text: "resposta" } };
    expect(awaitingReply(events, envelopes)).toBe(false);
  });

  it("settles when a later signal replies to the operator's own signal", () => {
    const events = [owner(1, "q-1"), event(2, "signal_recorded", { kind: "operator_note" })];
    const envelopes = { 2: { to: "codex", replyTo: "q-1", text: "respondendo" } };
    expect(awaitingReply(events, envelopes)).toBe(false);
  });

  it("an unopenable envelope counts as a possible reply, never as proof of silence", () => {
    // With no envelope for the later signal, claiming "still waiting" would be an absence
    // asserted through an erasing filter. The pill stands down.
    const events = [owner(1, "q-1"), event(2, "signal_recorded", { kind: "operator_note" })];
    expect(awaitingReply(events, {})).toBe(false);
  });
});
