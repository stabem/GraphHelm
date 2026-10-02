import { describe, expect, it, vi } from "vitest";

import { devSession, recordedActorSessions } from "./session";
import type { RuntimeEvent } from "./types";

const presence = (sequence: number, actorId: string, session: unknown, payloadActor = actorId): RuntimeEvent => ({
  sequence, kind: "agent_presence_declared", payload: { actorId: payloadActor, actorType: "agent", session },
  occurredAt: null, actorId, actorType: "agent", idempotencyKey: null, eventId: null, evidenceRefs: [],
});

describe("recorded actor and transport-session boundaries", () => {
  it("keeps distinct typed actor/session pairs while rejecting prose and mismatched declarations", () => {
    const prose = { ...presence(9, "codex", "not-a-session"), kind: "signal_recorded" };
    const owner = { ...presence(8, "owner", "mcp-owner"), actorType: "owner", payload: { actorId: "owner", actorType: "owner", session: "mcp-owner" } };
    expect(recordedActorSessions([
      presence(6, "codex", "mcp-a"), presence(3, "codex", "mcp-b"),
      presence(2, "codex", "mcp-a"), presence(4, "codex", "mcp-a"), presence(5, "claude", "mcp-c"),
      presence(7, "codex", "mcp-spoof", "another-actor"),
      presence(10, "codex", " "), prose, owner,
    ])).toEqual([
      { actorId: "codex", session: "mcp-a", sequences: [2, 4, 6] },
      { actorId: "claude", session: "mcp-c", sequences: [5] },
      { actorId: "codex", session: "mcp-b", sequences: [3] },
    ]);
  });
});

/**
 * The session ask carries the page's nonce, or does not happen at all.
 *
 * The dev server hands the Runtime's bearer token ONLY to a caller presenting the per-run nonce
 * (the token file is owner-only on disk, and loopback is not a user boundary - PR #467 review).
 * The page's half of that contract is here: no nonce in the URL, no request; a nonce present
 * rides the ask.
 */
describe("the dev session", () => {
  const okReply = () =>
    new Response(JSON.stringify({ ok: true, token: "fixture", project: "demo" }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });

  it("never asks without a nonce - the connect gate is the answer", async () => {
    const fetchImpl = vi.fn(async () => okReply());
    expect(await devSession(fetchImpl as never, "")).toBeNull();
    expect(await devSession(fetchImpl as never, "?other=1")).toBeNull();
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("presents the page's own nonce to the endpoint", async () => {
    const fetchImpl = vi.fn(async () => okReply());
    const session = await devSession(fetchImpl as never, "?session=abc123");
    expect(session).toEqual({ token: "fixture", project: "demo", projectPath: null });
    expect(fetchImpl).toHaveBeenCalledWith(
      "/__studio/session?nonce=abc123",
      expect.objectContaining({ headers: { Accept: "application/json" } }),
    );
  });

  it("treats a refusal as no session, not as an error", async () => {
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({ ok: false }), { status: 404 }));
    expect(await devSession(fetchImpl as never, "?session=wrong")).toBeNull();
  });

  it("passes a known folder path to the Studio without treating it as a credential", async () => {
    const fetchImpl = vi.fn(async () => new Response(JSON.stringify({
      token: "fixture", project: "fixture-project", projectPath: "fixtures/project",
    }), { status: 200 }));
    expect(await devSession(fetchImpl as never, "?session=known")).toEqual({
      token: "fixture", project: "fixture-project", projectPath: "fixtures/project",
    });
  });
});
