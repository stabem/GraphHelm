import { describe, expect, it, vi } from "vitest";

import { ACTOR_TYPES, RuntimeClient } from "./client";
import type { RecordedActor } from "./types";

/**
 * The immediate pause is verified BY STATE AND BY THE LEDGER: the route signals the cancel
 * channel and the driver appends the pause later, asynchronously, so there is no decision event
 * in the POST's reply to verify against - success needs a confirmed POST, a run that was not
 * already paused, a `paused` state on re-read, AND the pause event read back, whose actor is
 * what the evidence carries (PR #662 review, client.ts:618, adapter.ts:389, client.ts:760).
 */
/** The pause event as the WIRE carries it (tagged kind, nested actor) - the stub speaks the
 * Runtime's shape and the client normalises it, exactly as in production. Since #681 the
 * immediate route carries the request's actor down the cancel channel (`ImmediateCancelRequest`),
 * so the record names the CALLER - here the WebMCP adapter. */
const CALLER = { type: "agent", id: "studio-webmcp-adapter" } as const;
/** THIS request's identity: key "k" (the ledger's derived key is `k-paused-<digest>`) and the
 * adapter's actor. A mutation's identity is (key, actor), never the shape of an event (PR #662
 * review, client.ts:903) - the fixture pause below is "mine" only under these options. */
const MINE = { idempotencyKey: "k", actor: { id: CALLER.id, type: CALLER.type } } as const;
const PAUSED_BY_RUNTIME = {
  sequence: 12,
  kind: { type: "execution_paused", data: { executionId: "demo" } },
  occurredAt: "2026-09-02T10:00:00Z",
  actor: CALLER,
  idempotencyKey: "k-paused-abc",
  eventId: "event-12",
  evidenceRefs: [],
};

function statusBody(status: string, head: number) {
  return { ok: true, data: { executionId: "demo", status, headSequence: head, attention: "can_sleep", attentionReasons: [], untriagedInterruptions: [], silenceUnevaluated: [], nodeStateCounts: {}, startedAt: null, lastEventAt: null, nodeLastEventAt: {} } };
}

/** The stub speaks the wire: the events endpoint honours `after` and `limit` and pages the
 * ledger in 200-event slices, exactly as the Runtime does, so a scan that stops early is caught. */
function clientWith(
  statuses: string[],
  post: { status: number; ok: boolean; reply?: Record<string, unknown> },
  ledger: Array<{ sequence: number }> = [PAUSED_BY_RUNTIME],
) {
  let reads = 0;
  const head = ledger.length > 0 ? ledger[ledger.length - 1].sequence : 11;
  const seen: { ifMatch: string | null; eventPages: number } = { ifMatch: null, eventPages: 0 };
  const fetchImpl = vi.fn(async (url: string, init?: RequestInit) => {
    const json = (body: unknown, status = 200) =>
      new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
    if (init?.method === "POST") {
      seen.ifMatch = (init.headers as Record<string, string>)["If-Match"] ?? null;
      return json(
        post.ok ? { ok: true, data: { status: "paused", ...(post.reply ?? {}) } } : { ok: false, diagnostics: [{ code: "GHCLI999", severity: "error", message: "the driver fell over", path: "/", source: "serve" }] },
        post.status,
      );
    }
    if (url.includes("/events")) {
      seen.eventPages += 1;
      const query = new URL(url).searchParams;
      const after = Number(query.get("after") ?? "0");
      const limit = Number(query.get("limit") ?? "50");
      return json({ ok: true, data: { head, events: ledger.filter((event) => event.sequence > after).slice(0, limit) } });
    }
    const status = statuses[Math.min(reads, statuses.length - 1)];
    reads += 1;
    // The re-read after the POST reports the head the ledger actually reached.
    return json(statusBody(status, reads === 1 ? 11 : head));
  });
  return { client: new RuntimeClient("token", { baseUrl: "http://runtime.test", fetch: fetchImpl as never }), seen };
}

/** A legal 1,024-node ready set interrupted at once: 1,024 `Interrupted` outcomes, THEN the
 * pause (bounds.rs MAX_READY_SET; driver.rs's write_outcome loop before the cancel is honoured). */
function interruptedStorm(): Array<{ sequence: number }> {
  const events: Array<{ sequence: number }> = [];
  for (let index = 0; index < 1024; index += 1) {
    events.push({
      sequence: 12 + index,
      kind: { type: "node_outcome_recorded", data: { nodeId: `node-${index}`, outcome: "interrupted", nextState: "interrupted" } },
      occurredAt: "2026-09-02T10:00:00Z",
      actor: { type: "system", id: "system-runtime" },
      idempotencyKey: `interrupt-${index}`,
      eventId: `event-${12 + index}`,
      evidenceRefs: [],
    } as never);
  }
  events.push({ ...PAUSED_BY_RUNTIME, sequence: 12 + 1024, eventId: "event-paused" } as never);
  return events;
}

describe("the immediate pause's evidence", () => {
  it("succeeds only when the POST was confirmed, the run was not already paused, and the pause event is in the ledger", async () => {
    const { client } = clientWith(["running", "paused"], { status: 200, ok: true });
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("succeeded");
    expect(evidence.newEvents.map((event) => [event.kind, event.sequence])).toEqual([["execution_paused", 12]]);
  });

  /** THE IDENTITY OF A MUTATION IS (KEY, ACTOR), NEVER THE SHAPE OF AN EVENT (PR #662 review,
   * client.ts:903). Two racing immediate pauses: only one payload survives in the driver, and
   * here B's did - the log holds B's pause, under B's key and actor, and the state reads paused.
   * A's request is NOT succeeded on B's event: unknown, with the identity mismatch spelled out,
   * A's own (sent) identity on the evidence, and no event claimed. */
  it("refuses another request's pause as its own evidence - unknown, with the identity explained", async () => {
    const byB = [{ ...PAUSED_BY_RUNTIME, actor: { type: "owner", id: "studio-operator" }, idempotencyKey: "b-paused-999" }];
    const { client } = clientWith(["running", "paused"], { status: 200, ok: true }, byB as never);
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("unknown");
    expect(evidence.newEvents).toEqual([]);
    expect(evidence.actor).toEqual({ id: CALLER.id, type: CALLER.type });
    const why = evidence.diagnostics.find((d) => /not this request's/.test(d.message))!;
    expect(why.message).toContain("owner:studio-operator");
    expect(why.message).toContain("b-paused-999");
    expect(why.message).toContain("agent:studio-webmcp-adapter");
    expect(why.message).toContain("k-paused-");
  });

  /** A committed at 12, B resumed at 13 and paused again at 14 before A's status read. The last
   * pause in the interval is B's; A's evidence is A's OWN pause at 12 - never B's at 14. */
  it("picks this request's pause out of the interval, not the latest pause anyone made", async () => {
    const resumedByB = { sequence: 13, kind: { type: "execution_resumed", data: { executionId: "demo" } }, occurredAt: "2026-09-02T10:01:00Z", actor: { type: "owner", id: "studio-operator" }, idempotencyKey: "b-resumed-1", eventId: "event-13", evidenceRefs: [] };
    const pausedByB = { ...PAUSED_BY_RUNTIME, sequence: 14, eventId: "event-14", actor: { type: "owner", id: "studio-operator" }, idempotencyKey: "b-paused-2" };
    const { client } = clientWith(["running", "paused"], { status: 200, ok: true }, [PAUSED_BY_RUNTIME, resumedByB, pausedByB] as never);
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("succeeded");
    expect(evidence.newEvents.map((event) => event.sequence)).toEqual([12]);
    expect(evidence.actor).toEqual({ id: CALLER.id, type: CALLER.type });
  });

  /** (c) The actor is compared with the actor of the EVENT read from the ledger - the stub's own
   * object - never with a literal. Since #681 that record names the caller, so the evidence's
   * actor, the event's actor and the caller all agree; and none of the three is a claim the
   * client repeats - it is what was read back. */
  it("reports the caller's actor, read back from the ledger, equal to the event's", async () => {
    const { client } = clientWith(["running", "paused"], { status: 200, ok: true });
    const evidence = await client.pauseImmediately("demo", MINE);
    const recorded = evidence.newEvents.find((event) => event.kind === "execution_paused")!;
    expect(evidence.actor).toEqual({ id: recorded.actorId, type: recorded.actorType });
    expect(evidence.actor).toEqual({ id: CALLER.id, type: CALLER.type });
    expect(evidence.diagnostics.some((d) => /discard/i.test(d.message))).toBe(false);
  });

  /** A record naming someone else under this request's key (a Runtime regression on the
   * attribution #681 fixed) is NOT this request's pause: the identity is (key, actor), and
   * half of it failing is a foreign pause - unknown, the mismatch named, the sent identity
   * kept. The evidence never adopts an actor the request did not send. */
  it("treats a pause under this key but another actor as foreign - unknown, never adopted", async () => {
    const byRuntime = [{ ...PAUSED_BY_RUNTIME, actor: { type: "system", id: "system-runtime" } }];
    const { client } = clientWith(["running", "paused"], { status: 200, ok: true }, byRuntime as never);
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("unknown");
    expect(evidence.actor).toEqual({ id: CALLER.id, type: CALLER.type });
    expect(evidence.diagnostics.some((d) => /not this request's/.test(d.message) && /system:system-runtime/.test(d.message))).toBe(true);
  });

  /** The scan's bound is the head the re-read observed, not a page count: 1,024 Interrupted
   * records stand between the POST and the pause, and the pause is still found - `succeeded`,
   * with more than five pages read (PR #662 review, client.ts:752). */
  it("finds the pause behind a 1,024-node interrupted storm - the observed head is the bound", async () => {
    const { client, seen } = clientWith(["running", "paused"], { status: 200, ok: true }, interruptedStorm());
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("succeeded");
    expect(evidence.newEvents.map((event) => event.sequence)).toEqual([12 + 1024]);
    expect(seen.eventPages).toBeGreaterThan(5);
  });

  /** The evidence's actor is TYPED in the wire's vocabulary: `system` is a legal recorded actor,
   * not a value smuggled through a cast (PR #662 review, client.ts:684). */
  it("types the recorded actor in the wire vocabulary - the ledger's own line, read back", async () => {
    const { client } = clientWith(["running", "paused"], { status: 200, ok: true });
    const evidence = await client.pauseImmediately("demo", MINE);
    const type: RecordedActor["type"] = evidence.actor.type;
    expect(type).toBe("agent");
    expect(ACTOR_TYPES).toContain(evidence.actor.type);
  });

  it("refuses a ledger actor outside the wire vocabulary as proof, never narrows it", async () => {
    const alien = [{ ...PAUSED_BY_RUNTIME, actor: { type: "daemon", id: "something-new" } }];
    const { client } = clientWith(["running", "paused"], { status: 200, ok: true }, alien as never);
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("unknown");
    expect(evidence.actor.type).not.toBe("daemon");
    // An alien actor fails the identity test before anything else: it is not this request's.
    expect(evidence.diagnostics.some((d) => /not this request's/.test(d.message) && /daemon:something-new/.test(d.message))).toBe(true);
  });

  it("does not claim success when the ledger holds no pause event, even if the state reads paused", async () => {
    const { client } = clientWith(["running", "paused"], { status: 200, ok: true }, []);
    const evidence = await client.pauseImmediately("demo");
    expect(evidence.result).toBe("unknown");
  });

  /** The first POST committed, its reply was lost, and the retry under the same key arrives
   * with the run already paused. The Runtime answers `recognizedRetry` and names the original
   * decision's sequence; the evidence reads the pause event THERE and reports `succeeded` -
   * not `unknown` for a mutation the Runtime conclusively recognized (PR #662 review,
   * client.ts:667). */
  it("reconciles a recognized retry at its original decision sequence - succeeded, not unknown", async () => {
    const recognized = { idempotency: { recognizedRetry: true, originalDecisionSequence: 12 } };
    const { client } = clientWith(["paused", "paused"], { status: 200, ok: true, reply: recognized });
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("succeeded");
    expect(evidence.newEvents.map((event) => event.sequence)).toEqual([12]);
    expect(evidence.actor).toEqual({ id: CALLER.id, type: CALLER.type });
    expect(evidence.diagnostics.some((d) => /already paused/i.test(d.message))).toBe(false);
  });

  /** The Runtime's coordinate is checked for IDENTITY too, not only shape: a pause at the named
   * sequence that is another request's is refused as proof. */
  it("refuses a recognized retry whose named sequence holds another request's pause", async () => {
    const recognized = { idempotency: { recognizedRetry: true, originalDecisionSequence: 12 } };
    const byB = [{ ...PAUSED_BY_RUNTIME, actor: { type: "owner", id: "studio-operator" }, idempotencyKey: "b-paused-999" }];
    const { client } = clientWith(["paused", "paused"], { status: 200, ok: true, reply: recognized }, byB as never);
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("unknown");
    expect(evidence.diagnostics.some((d) => /not this request's/.test(d.message) && /sequence 12/.test(d.message))).toBe(true);
  });

  /** Pause committed at 12, ANOTHER actor resumed at 13, then the original caller retries
   * after losing the first reply. The Runtime recognizes the retry and names 12; the pause is
   * read there. The run reads `running` now - and that says nothing against a pause proven at
   * 12: `succeeded`, with the original pause's heads and a note naming the later resume (PR
   * #662 review, client.ts:774). */
  it("succeeds on a recognized retry even after another actor resumed - the record, not the aggregate", async () => {
    const resumedByOwner = {
      sequence: 13,
      kind: { type: "execution_resumed", data: { executionId: "demo" } },
      occurredAt: "2026-09-02T10:01:00Z",
      actor: { type: "owner", id: "studio-operator" },
      idempotencyKey: "k-resumed-xyz",
      eventId: "event-13",
      evidenceRefs: [],
    };
    const recognized = { idempotency: { recognizedRetry: true, originalDecisionSequence: 12 } };
    const { client } = clientWith(["running", "running"], { status: 200, ok: true, reply: recognized }, [PAUSED_BY_RUNTIME, resumedByOwner] as never);
    const evidence = await client.pauseImmediately("demo", MINE);
    expect(evidence.result).toBe("succeeded");
    expect(evidence.newEvents.map((event) => event.sequence)).toEqual([12]);
    expect(evidence.headBefore).toBe(11);
    expect(evidence.headAfter).toBe(12);
    expect(evidence.statusAfter?.status).toBe("running");
    expect(evidence.diagnostics.some((d) => /committed at 12; the run was resumed afterwards by owner:studio-operator at 13/.test(d.message))).toBe(true);
  });

  it("refuses a recognized retry whose named sequence holds no pause event", async () => {
    const elsewhere = { idempotency: { recognizedRetry: true, originalDecisionSequence: 12 } };
    const notAPause = [{ ...PAUSED_BY_RUNTIME, kind: { type: "node_outcome_recorded", data: { nodeId: "n", outcome: "interrupted", nextState: "interrupted" } } }];
    const { client } = clientWith(["paused", "paused"], { status: 200, ok: true, reply: elsewhere }, notAPause as never);
    const evidence = await client.pauseImmediately("demo");
    expect(evidence.result).toBe("unknown");
    expect(evidence.diagnostics.some((d) => /no execution_paused event was read there/.test(d.message))).toBe(true);
  });

  it("refuses a recognized retry that names no usable sequence", async () => {
    const coordinateless = { idempotency: { recognizedRetry: true } };
    const { client } = clientWith(["paused", "paused"], { status: 200, ok: true, reply: coordinateless });
    const evidence = await client.pauseImmediately("demo");
    expect(evidence.result).toBe("unknown");
    expect(evidence.diagnostics.some((d) => /no usable originalDecisionSequence/.test(d.message))).toBe(true);
  });

  it("does not claim success for a run that was already paused before the call", async () => {
    const { client } = clientWith(["paused", "paused"], { status: 200, ok: true });
    const evidence = await client.pauseImmediately("demo");
    expect(evidence.result).toBe("unknown");
    expect(evidence.diagnostics.some((d) => /already paused/i.test(d.message))).toBe(true);
  });

  it("does not claim success when the POST failed with a 5xx, even if the run reads paused after", async () => {
    const { client } = clientWith(["paused", "paused"], { status: 500, ok: false });
    const evidence = await client.pauseImmediately("demo");
    expect(evidence.result).toBe("unknown");
    expect(evidence.diagnostics.some((d) => /the driver fell over/.test(d.message))).toBe(true);
  });

  it("degrades to unknown when a running run reads paused after an unconfirmed POST", async () => {
    const { client } = clientWith(["running", "paused"], { status: 502, ok: false });
    const evidence = await client.pauseImmediately("demo");
    expect(evidence.result).toBe("unknown");
    expect(evidence.diagnostics.some((d) => /not confirmed/i.test(d.message))).toBe(true);
  });

  /** The precondition is the CALLER'S observation (PR #662 review, client.ts:664): they saw
   * head 10, the stream is at 11 by the time the client pre-reads. The header must carry 10 -
   * the fresher 11 the client just read would make the guard pass over work the caller never
   * saw - and the Runtime's conflict is reported as refused, nothing interrupted. */
  it("honours the caller's If-Match over the head the pre-read observed - conflict, not interruption", async () => {
    const { client, seen } = clientWith(["running", "running"], { status: 409, ok: false });
    const evidence = await client.pauseImmediately("demo", { ifMatch: 10 });
    expect(seen.ifMatch).toBe("10");
    expect(evidence.result).toBe("refused");
    expect(evidence.newEvents).toEqual([]);
    expect(evidence.headBefore).toBe(10);
  });

  it("sends the observed head as If-Match and reads a moved head as refused, not paused", async () => {
    const { client, seen } = clientWith(["running", "running"], { status: 409, ok: false });
    const evidence = await client.pauseImmediately("demo");
    expect(seen.ifMatch).toBe("11");
    expect(evidence.result).toBe("refused");
    // Refused appended nothing: the identity that was SENT is the only truthful one.
    expect(evidence.actor).toEqual({ id: "studio-operator", type: "owner" });
  });
});
