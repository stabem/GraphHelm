import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import React from "react";

import { awaitingReply, describe as describeEvent, groupTurns, pinnedToLatest, resetPanelCaches, Thread, useEnvelopes } from "./panel";
import { EVIDENCE_READS_TO_QUEUE } from "../runtime/client";
import type { EvidenceContent, RuntimeEvent } from "../runtime/types";

const stateUpdateAudit = vi.hoisted(() => ({ active: false, submissions: 0 }));
vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  const useState = ((initial: unknown) => {
    const [state, setState] = actual.useState(initial);
    if (!stateUpdateAudit.active) return [state, setState];
    return [state, (next: unknown) => {
      stateUpdateAudit.submissions += 1;
      setState(next as never);
    }];
  }) as typeof actual.useState;
  const defaultExport = { ...actual, useState } as unknown as typeof React;
  return { ...actual, default: defaultExport, useState };
});

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

it("summarizes authoring snapshots instead of exposing their payload by default", () => {
  const summary = describeEvent(event(8, "graph_authoring_snapshot_stored", {
    executionId: "run", graphVersion: 3, graphHash: "sha256:safe",
  }));
  expect(summary).toMatch(/sealed authoring snapshot/i);
  expect(summary).not.toContain("sha256:safe");
});

it("keeps authoring evidence closed until the reader opens it", async () => {
  const eventRecord = event(8, "graph_authoring_snapshot_stored", { executionId: "run", graphVersion: 3, graphHash: "sha256:safe" });
  eventRecord.evidenceRefs = ["sealed-authoring"];
  const open = vi.fn(async () => ({ evidenceId: "sealed-authoring", content: "FULL SERIALIZED GRAPH", contentSha256: "raw", mediaType: "application/json", sensitivity: "confidential" as const }));
  render(React.createElement(Thread, { events: [eventRecord], executionId: "run", openEvidence: open }));
  expect(screen.queryByText("FULL SERIALIZED GRAPH")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("show what was said"));
  expect(await screen.findByText("FULL SERIALIZED GRAPH")).toBeVisible();
});

/** Observable contract: bookkeeping payloads stay collapsed while the event remains inspectable.
 * This catches technical JSON overwhelming the conversation and verifies that the evidence is
 * still available behind an explicit control. */
it("collapses technical payloads without dropping the recorded event", () => {
  render(React.createElement(Thread, { events: [event(9, "draft_proposed", { draftId: "draft-1", proposalSha256: "sha256:sealed" })] }));
  expect(screen.getByText("draft proposed")).toBeInTheDocument();
  const details = screen.getByText("Technical details").closest("details");
  expect(details).not.toHaveAttribute("open");
  expect(details).toHaveTextContent("draft-1");
});

it("keeps subagent lifecycle signals out of chat evidence and raw technical payloads", () => {
  const signal = event(10, "signal_recorded", {
    kind: "agent_subagent_started", sourceKind: "tool", sourceId: "codex-session-parent",
    signalId: "sig-1", envelopeSha256: "sha256:sealed",
  });
  signal.evidenceRefs = ["sealed-subagent-envelope"];
  const open = vi.fn(async () => ({ evidenceId: "sealed-subagent-envelope", content: "{\"childAgentId\":\"child\"}", contentSha256: "hash", mediaType: "application/json", sensitivity: "internal" as const }));
  render(React.createElement(Thread, { events: [signal], executionId: "run", openEvidence: open }));
  expect(screen.getByText("Subagent start recorded; see Team for the session link.")).toBeInTheDocument();
  expect(screen.queryByText("Technical details")).not.toBeInTheDocument();
  expect(open).not.toHaveBeenCalled();
});

/** Protects native receipts from eagerly exposing their sealed JSON in conversation.
 * Existing subagent coverage does not reach native-chat signals. Cost: one jsdom
 * render, no native host, model, credentials, or external network. */
it("summarizes native chat receipts without eagerly opening their sealed data", () => {
  const signal = event(10, "signal_recorded", { kind: "native_chat_completed" });
  signal.evidenceRefs = ["sealed-native-receipt"];
  const open = vi.fn(async () => ({ evidenceId: "sealed-native-receipt", content: "RAW NATIVE RECEIPT JSON", contentSha256: "hash", mediaType: "application/json", sensitivity: "internal" as const }));
  render(React.createElement(Thread, { events: [signal], executionId: "run", openEvidence: open }));
  expect(screen.getByText("Native chat reply received. Open its work step to read it.")).toBeInTheDocument();
  expect(screen.queryByText("RAW NATIVE RECEIPT JSON")).not.toBeInTheDocument();
  expect(open).not.toHaveBeenCalled();
});

it("shows the newest sealed report while an older evidence read is still pending", async () => {
  resetPanelCaches();
  const older = event(1, "signal_recorded");
  const newer = event(2, "signal_recorded");
  older.evidenceRefs = ["old"];
  newer.evidenceRefs = ["new"];
  const events = [older, newer];
  const never = new Promise<EvidenceContent>(() => {});
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => id === "old" ? never : {
    evidenceId: "new",
    content: JSON.stringify({ type: "operator_note", description: "Checking the branch", source: { id: "reviewer", type: "user" } }),
    contentSha256: "hash",
    mediaType: "application/json",
    sensitivity: "confidential",
  });
  const { result } = renderHook(() => useEnvelopes(events, "run", open));
  await waitFor(() => expect(result.current[2]?.text).toBe("Checking the branch"));
  expect(result.current[1]).toBeUndefined();
  expect(open.mock.calls[0][1]).toBe("new");
});

it("keeps failed question reads pending and excludes bookkeeping signals", async () => {
  resetPanelCaches();
  const candidate = event(5, "signal_recorded", { kind: "operator_note", signalId: "sig-q" });
  candidate.actorId = "kit-3";
  candidate.evidenceRefs = ["question"];
  let questionAttempts = 0;
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => {
    if (id === "question" && ++questionAttempts === 1) throw new Error("temporarily unavailable");
    return { evidenceId: id, content: JSON.stringify({ type: "operator_note", description: "Merge now?", target: { id: "studio-operator", type: "user" } }), contentSha256: "hash", mediaType: "application/json", sensitivity: "confidential" };
  });
  const { result, rerender } = renderHook(({ rows }) => useEnvelopes(rows, "run", open), { initialProps: { rows: [candidate] } });
  await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
  expect(result.current[5]).toBeUndefined();
  const retryTrigger = event(6, "signal_recorded", { kind: "agent_task_created", signalId: "sig-r" });
  retryTrigger.evidenceRefs = ["retry"];
  rerender({ rows: [candidate, retryTrigger] });
  await waitFor(() => expect(result.current.__pendingEnvelopeSequences).toEqual([5]));
  expect(result.current[6]).toBeUndefined();
});

it("keeps unread question envelopes pending until their evidence read settles", async () => {
  resetPanelCaches();
  const candidate = event(5, "signal_recorded", { kind: "operator_note", signalId: "sig-q" });
  candidate.actorId = "kit-3";
  candidate.evidenceRefs = ["question"];
  const events = [candidate];
  let finishRead!: (content: EvidenceContent) => void;
  const open = vi.fn(() => new Promise<EvidenceContent>((resolve) => { finishRead = resolve; }));
  const { result } = renderHook(() => useEnvelopes(events, "run", open));
  await waitFor(() => expect(open).toHaveBeenCalledOnce());
  expect(result.current[5]).toBeUndefined();
  expect(result.current.__pendingEnvelopeSequences).toEqual([5]);
  finishRead({ evidenceId: "question", content: JSON.stringify({ type: "operator_note", description: "Merge now?", target: { id: "studio-operator", type: "user" } }), contentSha256: "hash", mediaType: "application/json", sensitivity: "confidential" });
  await waitFor(() => expect(result.current[5]?.text).toBe("Merge now?"));
  expect(result.current.__pendingEnvelopeSequences).toEqual([]);
});

it("reads each event once when the caller rebuilds the unchanged array", async () => {
  resetPanelCaches();
  const candidate = event(5, "signal_recorded", { kind: "operator_note", signalId: "sig-q" });
  candidate.evidenceRefs = ["question"];
  const stableEvents = [candidate];
  let renders = 0;
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => ({
    evidenceId: id,
    content: JSON.stringify({ type: "operator_note", description: "Question" }),
    contentSha256: "hash",
    mediaType: "application/json",
    sensitivity: "confidential",
  }));
  const { result } = renderHook(() => useEnvelopes(renders++ < 4 ? [candidate] : stableEvents, "run", open));
  await waitFor(() => expect(result.current[5]?.text).toBe("Question"));
  expect(open).toHaveBeenCalledTimes(1);
});

// A failed open went back to "reading" on every poll, so a Keel card row never resolved; the
// reason the Runtime gave was dropped. Cost: one hook, a stubbed open, no network.
it("keeps a failed envelope resolved with its reason across the next poll", async () => {
  resetPanelCaches();
  const card = event(7, "signal_recorded", { kind: "keel.card", sourceKind: "node", sourceId: "planner" });
  card.evidenceRefs = ["card"];
  const later = event(8, "signal_recorded", { kind: "operator_note", signalId: "later" });
  const open = vi.fn(async (): Promise<EvidenceContent> => { throw new Error("evidence card is sealed for another actor"); });
  const { result, rerender } = renderHook(({ rows }) => useEnvelopes(rows, "run", open), { initialProps: { rows: [card] } });
  await waitFor(() => expect(result.current.__readingEnvelopeSequences).toEqual([]));
  expect(result.current.__envelopeFailures[7]).toBe("evidence card is sealed for another actor");
  rerender({ rows: [card, later] });
  expect(result.current.__readingEnvelopeSequences).toEqual([]);
  expect(result.current.__envelopeFailures[7]).toBe("evidence card is sealed for another actor");
});

// gh-team: 6,944 envelope carriers read newest first left its 15 Keel cards (old records) at the
// back of the queue, "reading" for minutes. The cards are opened first. Cost: one hook, a stubbed open.
it("opens Keel card envelopes before the newer carriers", async () => {
  resetPanelCaches();
  const card = event(32, "signal_recorded", { kind: "keel.card", sourceKind: "node", sourceId: "implementation" });
  card.evidenceRefs = ["signal-gh-codex-2-338-card"];
  const notes = Array.from({ length: 120 }, (_, index) => {
    const note = event(100 + index, "signal_recorded", { kind: "operator_note", signalId: `n${index}` });
    note.evidenceRefs = [`note-${index}`];
    return note;
  });
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => ({
    evidenceId: id, content: JSON.stringify({ description: `text ${id}` }), contentSha256: "hash",
    mediaType: "application/json", sensitivity: "confidential",
  }));
  const { result } = renderHook(() => useEnvelopes([card, ...notes], "gh-team", open));
  await waitFor(() => expect(result.current.__readingEnvelopeSequences).toEqual([]));
  expect(open.mock.calls[0]?.[1]).toBe("signal-gh-codex-2-338-card");
});

it("keeps the settled envelope snapshot stable across unrelated renders", async () => {
  resetPanelCaches();
  const first = event(5, "signal_recorded", { kind: "operator_note", signalId: "sig-q" });
  first.actorId = "kit-3";
  first.evidenceRefs = ["question"];
  const second = event(6, "signal_recorded", { kind: "operator_note", signalId: "sig-next" });
  second.evidenceRefs = ["next"];
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => ({
    evidenceId: id,
    content: JSON.stringify({ type: "operator_note", description: id, to: "studio-operator" }),
    contentSha256: "hash", mediaType: "application/json", sensitivity: "confidential",
  }));
  let memoEvaluations = 0;
  const { result, rerender } = renderHook(({ unrelated, rows, run }) => {
    void unrelated;
    const snapshot = useEnvelopes(rows, run, open);
    React.useMemo(() => { memoEvaluations += 1; return snapshot; }, [snapshot]);
    return snapshot;
  }, { initialProps: { unrelated: 0, rows: [first], run: "run-a" } });
  await waitFor(() => expect(result.current.__pendingEnvelopeSequences).toEqual([]));
  const settled = result.current;
  const evaluationsAfterSettle = memoEvaluations;

  rerender({ unrelated: 1, rows: [first], run: "run-a" });
  expect(result.current).toBe(settled);
  expect(result.current.__pendingEnvelopeSequences).toBe(settled.__pendingEnvelopeSequences);
  expect(result.current.__readingEnvelopeSequences).toBe(settled.__readingEnvelopeSequences);
  expect(memoEvaluations).toBe(evaluationsAfterSettle);

  rerender({ unrelated: 1, rows: [first, second], run: "run-a" });
  await waitFor(() => expect(result.current[6]?.text).toBe("next"));
  expect(result.current).not.toBe(settled);
  expect(result.current[5]).toEqual(expect.objectContaining({ text: "question", to: "studio-operator" }));
  expect(result.current.__pendingEnvelopeSequences).toEqual([]);
});

it("settles cached envelopes after a poll adds another carrier", async () => {
  resetPanelCaches();
  const first = event(5, "signal_recorded", { kind: "operator_note", signalId: "sig-q" });
  first.evidenceRefs = ["question"];
  const next = event(6, "signal_recorded", { kind: "operator_note", signalId: "sig-next" });
  next.evidenceRefs = ["unrelated"];
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => ({
    evidenceId: id, content: JSON.stringify({ type: "operator_note", description: id }),
    contentSha256: "hash", mediaType: "application/json", sensitivity: "confidential",
  }));
  const { result, rerender } = renderHook(({ rows }) => useEnvelopes(rows, "run", open), { initialProps: { rows: [first] } });
  await waitFor(() => expect(result.current.__pendingEnvelopeSequences).toEqual([]));
  rerender({ rows: [first, next] });
  await waitFor(() => expect(result.current[6]?.text).toBe("unrelated"));
  expect(result.current.__pendingEnvelopeSequences).toEqual([]);
});

it("publishes warm envelopes once per bounded page with complete contents", async () => {
  resetPanelCaches();
  const rows = Array.from({ length: 500 }, (_, index) => {
    const row = event(index + 1, "signal_recorded", { kind: "operator_note", signalId: `sig-${index + 1}` });
    row.evidenceRefs = [`evidence-${index + 1}`];
    return row;
  });
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => ({
    evidenceId: id,
    content: JSON.stringify({ type: "operator_note", description: `report ${id}`, to: `agent-${id}` }),
    contentSha256: "hash", mediaType: "application/json", sensitivity: "confidential",
  }));
  const first = renderHook(() => useEnvelopes(rows, "run", open));
  await waitFor(() => expect(first.result.current.__pendingEnvelopeSequences).toEqual([]));
  first.unmount();

  stateUpdateAudit.active = true;
  stateUpdateAudit.submissions = 0;
  try {
    const second = renderHook(() => useEnvelopes(rows, "run", open));
    await waitFor(() => expect(second.result.current.__pendingEnvelopeSequences).toEqual([]));
    expect(stateUpdateAudit.submissions).toBeLessThanOrEqual(1 + Math.ceil(rows.length / 50));
    expect(open).toHaveBeenCalledTimes(500);
    for (const row of rows) {
      expect(second.result.current[row.sequence]).toEqual(expect.objectContaining({
        to: `agent-evidence-${row.sequence}`, text: `report evidence-${row.sequence}`,
      }));
    }
  } finally {
    stateUpdateAudit.active = false;
  }
});

it("settles 4,900 simultaneous envelopes without beacon flicker", async () => {
  resetPanelCaches();
  vi.useFakeTimers();
  const rows = Array.from({ length: 4_900 }, (_, index) => {
    const row = event(index + 1, "signal_recorded", { kind: "operator_note", signalId: `sig-${index + 1}` });
    row.evidenceRefs = [`evidence-${index + 1}`];
    return row;
  });
  const firstBatch = new Map<string, (content: EvidenceContent) => void>();
  let firstBatchResolved = false;
  const contentFor = (id: string): EvidenceContent => ({
    evidenceId: id,
    content: JSON.stringify({ type: "operator_note", description: `report ${id}`, to: `agent-${id}` }),
    contentSha256: "hash", mediaType: "application/json", sensitivity: "confidential",
  });
  const open = vi.fn((_run: string, id: string): Promise<EvidenceContent> => {
    if (firstBatchResolved) return Promise.resolve(contentFor(id));
    return new Promise((resolve) => { firstBatch.set(id, resolve); });
  });
  stateUpdateAudit.active = true;
  stateUpdateAudit.submissions = 0;
  const beaconTransitions: boolean[] = [];
  try {
    const { result } = renderHook(() => {
      const envelopes = useEnvelopes(rows, "run", open);
      const checking = envelopes.__pendingEnvelopeSequences.length > 0;
      if (beaconTransitions.at(-1) !== checking) beaconTransitions.push(checking);
      return envelopes;
    });
    setTimeout(() => {
      firstBatchResolved = true;
      for (const [id, resolve] of firstBatch) resolve(contentFor(id));
    }, 1);
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });

    expect(vi.getTimerCount()).toBe(0);
    // #1560: enough reads wait at once to fill the client's collection reads in flight.
    expect(firstBatch.size).toBe(EVIDENCE_READS_TO_QUEUE);
    expect(beaconTransitions).toEqual([false, true, false]);
    expect(result.current.__pendingEnvelopeSequences).toEqual([]);
    expect(open).toHaveBeenCalledTimes(rows.length);
    for (const row of rows) {
      expect(result.current[row.sequence]).toEqual(expect.objectContaining({
        to: `agent-evidence-${row.sequence}`, text: `report evidence-${row.sequence}`,
      }));
    }
    expect(stateUpdateAudit.submissions).toBeLessThanOrEqual(8);
  } finally {
    stateUpdateAudit.active = false;
    vi.useRealTimers();
  }
});

it("does not reuse a previously opened envelope when execution sequence numbers repeat", async () => {
  resetPanelCaches();
  const first = event(5, "signal_recorded", { kind: "operator_note", signalId: "sig-a" });
  first.evidenceRefs = ["first"];
  const second = event(5, "signal_recorded", { kind: "operator_note", signalId: "sig-b" });
  second.evidenceRefs = ["second"];
  const open = vi.fn(async (_run: string, id: string): Promise<EvidenceContent> => ({ evidenceId: id,
    content: JSON.stringify({ type: "operator_note", description: id, target: { id: "studio-operator", type: "user" } }),
    contentSha256: "hash", mediaType: "application/json", sensitivity: "confidential" }));
  const { result, rerender } = renderHook(({ rows, run }) => useEnvelopes(rows, run, open), { initialProps: { rows: [first], run: "run-a" } });
  await waitFor(() => expect(result.current[5]?.text).toBe("first"));
  rerender({ rows: [second], run: "run-b" });
  expect(result.current[5]).toBeUndefined();
  await waitFor(() => expect(result.current[5]?.text).toBe("second"));
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
