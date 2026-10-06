import { cleanup, render, renderHook, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import { buildGraphModel } from "../graph/model";
import type { RuntimeEvent } from "../runtime/types";
import { NodePanel, resetPanelCaches, useActorAliases, useNativePersonaLinks } from "./panel";
import { teamModel } from "../runtime/team";

it("shows one real attempt and the model reply without opening accounting blobs", async () => {
  resetPanelCaches();
  const event = (sequence: number, nextState: string, refs: string[] = []): RuntimeEvent => ({
    sequence,
    kind: "node_outcome_recorded",
    payload: { nodeId: "review_browser_evidence", outcome: nextState === "succeeded" ? "succeeded" : "started", nextState },
    occurredAt: `2026-09-26T02:45:0${sequence}Z`,
    actorId: "system-runtime",
    actorType: "system",
    idempotencyKey: `k-${sequence}`,
    eventId: `event-${sequence}`,
    evidenceRefs: refs,
  });
  const events = [
    event(1, "queued"),
    event(2, "running"),
    event(3, "succeeded", ["run-review-a1-reply", "run-review-a1-context-provenance", "run-review-a1-accounting-receipt"]),
  ];
  const node = buildGraphModel(events).nodes[0];
  const openEvidence = vi.fn(async (_executionId: string, evidenceId: string) => ({
    evidenceId,
    content: JSON.stringify({ text: "Browser check incomplete: no screenshots supplied.", usage: { inputTokens: 10, outputTokens: 8 } }),
    mediaType: "application/json",
    contentSha256: "hash",
    sensitivity: "confidential",
  }));
  render(<NodePanel node={node} events={events} onClose={vi.fn()} executionId="run-review" openEvidence={openEvidence} />);
  const panel = screen.getByRole("region", { name: "Node review_browser_evidence" });
  expect(panel).toHaveTextContent("Attempts1");
  expect(panel).toHaveTextContent("Reply received · acceptance not verified");
  await waitFor(() => expect(panel).toHaveTextContent("Browser check incomplete: no screenshots supplied."));
  expect(openEvidence).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "show context sources" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "show token accounting" })).toBeInTheDocument();
});

// DOM-only, no network: catches the node inspector's outer-scroll container leaving the latest turn below deliveries.
it("opens node history at the newest message before the delivery section", () => {
  cleanup();
  const events: RuntimeEvent[] = [1, 2].map((sequence) => ({ sequence, kind: "signal_recorded", payload: { kind: "operator_note", description: `message-${sequence}`, nodeId: "start" }, occurredAt: `2026-10-02T10:00:0${sequence}Z`, actorId: `codex-${sequence}`, actorType: "agent", idempotencyKey: `n-${sequence}`, eventId: `n-${sequence}`, evidenceRefs: [] }));
  const node = buildGraphModel(events).nodes.find((candidate) => candidate.id === "start")!;
  render(<NodePanel node={node} events={events} onClose={vi.fn()} />);
  const recent = screen.getByRole("region", { name: "Recent messages" });
  const turns = recent.querySelectorAll(".turn:not(.stage)");
  expect(turns[0]).toHaveTextContent("codex-2");
  expect(turns[1]).toHaveTextContent("codex-1");
});

// Sealed evidence I/O only: catches forged membership, cross-run leaks, and later re-chartering; no real chat is resumed.
it("replays only owner membership for this activity and hides it on activity switch", async () => {
  resetPanelCaches();
  const id = "01a0f784-e036-7603-9308-5c6ddabd438b";
  const values = [{ charter: "Careful tester", actorType: "owner", executionId: "run-a" }, { charter: "Replace personality", actorType: "owner", executionId: "run-a" }, { charter: "Agent cannot charter", actorType: "agent", executionId: "run-a" }, { charter: "Other run", actorType: "owner", executionId: "run-b" }];
  const events: RuntimeEvent[] = values.map((value, i) => ({ sequence: i + 1, kind: "signal_recorded", payload: { kind: "native_persona_linked" }, occurredAt: null, actorId: "studio-owner", actorType: value.actorType, idempotencyKey: `link-${i}`, eventId: `link-${i}`, evidenceRefs: [`membership-${i}`] } as RuntimeEvent));
  const openEvidence = vi.fn(async (_run: string, evidenceId: string) => {
    const value = values[Number(evidenceId.split("-").at(-1))];
    return { evidenceId, content: JSON.stringify({ source: { type: "user", id: "studio-owner" }, to: id, description: JSON.stringify({ protocol: "graphhelm-native-persona-v1", executionId: value.executionId, threadId: id, title: "LojaKit tester", sourceDirectory: "F:/github/ml-saas", nodeId: "start", charter: value.charter }) }), mediaType: "application/json", contentSha256: "hash", sensitivity: "confidential" };
  });
  const { result, rerender } = renderHook(({ run }) => useNativePersonaLinks(events, run, openEvidence), { initialProps: { run: "run-a" } });
  await waitFor(() => expect(result.current[id]?.charter).toBe("Careful tester"));
  expect(openEvidence).not.toHaveBeenCalledWith("run-a", "membership-2");
  rerender({ run: "different-run" });
  expect(result.current).toEqual({});
  await waitFor(() => expect(result.current).toEqual({}));
});

// Sealed evidence I/O only: catches an agent renaming a peer, a malformed alias, and the older name winning.
it("reads owner actor aliases, newest wins, and the bot carries the name", async () => {
  resetPanelCaches();
  const rows = [
    { actorType: "owner", to: "kit-1", type: "user", name: "  Old name  " },
    { actorType: "owner", to: "kit-1", type: "user", name: "Cart builder" },
    { actorType: "agent", to: "kit-2", type: "tool", name: "Hijack" },
    { actorType: "owner", to: "codex", type: "user", name: "Shared" },
    { actorType: "owner", to: "kit-3", type: "user", name: "x".repeat(81) },
  ];
  const events: RuntimeEvent[] = rows.map((row, i) => ({ sequence: i + 1, kind: "signal_recorded", payload: { kind: "actor_alias" }, occurredAt: null, actorId: row.actorType === "owner" ? "studio-operator" : "kit-9", actorType: row.actorType, idempotencyKey: `alias-${i}`, eventId: `alias-${i}`, evidenceRefs: [`alias-${i}`] } as RuntimeEvent));
  const openEvidence = vi.fn(async (_run: string, evidenceId: string) => {
    const row = rows[Number(evidenceId.split("-").at(-1))];
    return { evidenceId, content: JSON.stringify({ source: { type: row.type, id: "studio-operator" }, to: row.to, description: JSON.stringify({ protocol: "graphhelm-actor-alias-v1", displayName: row.name }) }), mediaType: "application/json", contentSha256: "hash", sensitivity: "confidential" };
  });
  const { result } = renderHook(() => useActorAliases(events, "run-a", openEvidence));
  await waitFor(() => expect(result.current).toEqual({ "kit-1": "Cart builder" }));
  expect(openEvidence).not.toHaveBeenCalledWith("run-a", "alias-2");
  const team = teamModel({ events: [], envelopes: {}, personas: {}, nativeLinks: {}, aliases: result.current, model: null, claudeTasks: null, waitingAskers: new Set(), now: Date.parse("2026-10-05T12:00:00Z") });
  expect(team.bots.find((bot) => bot.actorId === "kit-1")?.name).toBe("Cart builder");
});

// Spec 4.1: the owner's newest valid alias renames the bot; agent aliases, bad protocol, codex and junk names do not.
it("reads owner actor_alias records into bot names, newest wins", async () => {
  resetPanelCaches();
  const rows = [
    { actorType: "owner", to: "kit-1", protocol: "graphhelm-actor-alias-v1", displayName: "First" },
    { actorType: "owner", to: "kit-1", protocol: "graphhelm-actor-alias-v1", displayName: "  Cart builder  " },
    { actorType: "agent", to: "kit-1", protocol: "graphhelm-actor-alias-v1", displayName: "Hijack" },
    { actorType: "owner", to: "codex", protocol: "graphhelm-actor-alias-v1", displayName: "Shared" },
    { actorType: "owner", to: "kit-2", protocol: "other", displayName: "Wrong" },
    { actorType: "owner", to: "kit-2", protocol: "graphhelm-actor-alias-v1", displayName: "x".repeat(81) },
  ];
  const events: RuntimeEvent[] = rows.map((row, i) => ({ sequence: i + 1, kind: "signal_recorded", payload: { kind: "actor_alias" }, occurredAt: null, actorId: "studio-owner", actorType: row.actorType, idempotencyKey: `alias-${i}`, eventId: `alias-${i}`, evidenceRefs: [`alias-${i}`] } as RuntimeEvent));
  const openEvidence = vi.fn(async (_run: string, evidenceId: string) => {
    const row = rows[Number(evidenceId.split("-").at(-1))];
    return { evidenceId, content: JSON.stringify({ source: { type: "user", id: "studio-owner" }, to: row.to, description: JSON.stringify({ protocol: row.protocol, displayName: row.displayName }) }), mediaType: "application/json", contentSha256: "hash", sensitivity: "confidential" };
  });
  const { result } = renderHook(() => useActorAliases(events, "run-a", openEvidence));
  await waitFor(() => expect(result.current).toEqual({ "kit-1": "Cart builder" }));
  const worked: RuntimeEvent = { sequence: 9, kind: "signal_recorded", payload: { kind: "operator_note" }, occurredAt: null, actorId: "kit-1", actorType: "agent", idempotencyKey: null, eventId: "w", evidenceRefs: [] } as RuntimeEvent;
  const team = teamModel({ events: [worked], envelopes: {}, personas: {}, nativeLinks: {}, aliases: result.current, model: null, claudeTasks: null, waitingAskers: new Set(), now: Date.now() });
  expect(team.bots.find((bot) => bot.actorId === "kit-1")?.name).toBe("Cart builder");
});
