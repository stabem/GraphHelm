import { describe, expect, it, vi } from "vitest";

import { RuntimeClient } from "./client";

/**
 * Two preconditions that were advertised and not kept (PR #662 review, adapter.ts:157 and :178).
 *
 * The first is the `If-Match` head pin. The client sent it only when the head was above zero, so
 * a caller that observed an EMPTY run and asked to be refused if anything appeared had its
 * precondition silently dropped. The Runtime reports an empty stream as head 0 -- `current_head`
 * is `history.last().map_or(0, ..)` -- and reports no head at all only when the store cannot be
 * read, so 0 is a checkable claim and not a value streams never issue.
 *
 * The second is what a recognized retry proves. `recognizedRetry` alone was accepted as the
 * missing evidence for the seven verbs that go through the verified-mutation path, so a retry
 * reported `succeeded` with an EMPTY event list while the reply advertised verification against
 * the original decision. The immediate-pause path already read the decision back at
 * `originalDecisionSequence`; these cells hold the rest to the same rule.
 */

const CALLER = { type: "agent", id: "studio-webmcp-adapter" } as const;
const MINE = { idempotencyKey: "k", actor: { id: CALLER.id, type: CALLER.type } } as const;

/** The cancel decision as the WIRE carries it: `cancel` records `execution_completed` under the
 * derived key `{key}-cancelled-{digest}` (MUTATION_KEY_SUFFIX, copied from the serve route). */
function cancelledBy(
  actor: { type: string; id: string },
  idempotencyKey: string,
  sequence: number,
) {
  return {
    sequence,
    kind: { type: "execution_completed", data: { executionId: "demo", outcome: "cancelled" } },
    occurredAt: "2026-09-04T02:00:00Z",
    actor,
    idempotencyKey,
    eventId: `event-${sequence}`,
    evidenceRefs: [],
  };
}

function statusBody(status: string, head: number) {
  return {
    ok: true,
    data: {
      executionId: "demo",
      status,
      headSequence: head,
      attention: "can_sleep",
      attentionReasons: [],
      untriagedInterruptions: [],
      silenceUnevaluated: [],
      nodeStateCounts: {},
      startedAt: null,
      lastEventAt: null,
      nodeLastEventAt: {},
    },
  };
}

/**
 * `heads` is read one per status call, the last value repeating, so a cell can say what the
 * pre-read saw and what the re-read saw. `post.reply` is merged into the POST's data, which is
 * how `headSequence` and the `idempotency` block reach the client.
 */
function clientWith(
  heads: number[],
  post: { status: number; ok: boolean; reply?: Record<string, unknown> },
  ledger: Array<{ sequence: number }> = [],
) {
  let reads = 0;
  const head = ledger.length > 0 ? ledger[ledger.length - 1].sequence : heads[heads.length - 1];
  const seen: { ifMatch: string | null; ifMatchSent: boolean } = { ifMatch: null, ifMatchSent: false };
  const fetchImpl = vi.fn(async (url: string, init?: RequestInit) => {
    const json = (body: unknown, status = 200) =>
      new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
    if (init?.method === "POST") {
      const headers = init.headers as Record<string, string>;
      seen.ifMatchSent = "If-Match" in headers;
      seen.ifMatch = headers["If-Match"] ?? null;
      return json(
        post.ok
          ? { ok: true, data: { status: "cancelled", ...(post.reply ?? {}) } }
          : { ok: false, diagnostics: [] },
        post.status,
      );
    }
    if (url.includes("/events")) {
      const query = new URL(url).searchParams;
      const after = Number(query.get("after") ?? "0");
      const limit = Number(query.get("limit") ?? "50");
      return json({
        ok: true,
        data: { head, events: ledger.filter((event) => event.sequence > after).slice(0, limit) },
      });
    }
    const at = heads[Math.min(reads, heads.length - 1)];
    reads += 1;
    return json(statusBody("running", at));
  });
  return {
    client: new RuntimeClient("token", { baseUrl: "http://runtime.test", fetch: fetchImpl as never }),
    seen,
  };
}

describe("the If-Match precondition", () => {
  /** THE FINDING. An empty run's head is 0, and 0 is what the caller observed. */
  it("sends a head of zero, because zero is an observation and not an absence", async () => {
    const { client, seen } = clientWith(
      [0, 1],
      { status: 200, ok: true, reply: { headSequence: 1 } },
      [cancelledBy(CALLER, "k-cancelled-abc", 1)],
    );

    await client.cancel("demo", MINE);

    expect(seen.ifMatchSent).toBe(true);
    expect(seen.ifMatch).toBe("0");
  });

  /**
   * THE CONTROL. The same call with a non-zero head must send that head, or the cell above would
   * pass on a client that sends the header unconditionally with a constant.
   */
  it("sends a non-zero observed head unchanged", async () => {
    const { client, seen } = clientWith(
      [5, 6],
      { status: 200, ok: true, reply: { headSequence: 6 } },
      [cancelledBy(CALLER, "k-cancelled-abc", 6)],
    );

    await client.cancel("demo", MINE);

    expect(seen.ifMatch).toBe("5");
  });

  /**
   * A caller who NAMES zero outranks a fresher pre-read. This is the WebMCP case the review
   * found: `ifMatch: 0` passes a schema whose `minimum` is 0, and dropping it would have told the
   * agent its verb was guarded while it ran against a stream created after it looked.
   */
  it("keeps a caller's explicit zero rather than the head the client just read", async () => {
    const { client, seen } = clientWith(
      [9, 10],
      { status: 200, ok: true, reply: { headSequence: 10 } },
      [cancelledBy(CALLER, "k-cancelled-abc", 10)],
    );

    await client.cancel("demo", { ...MINE, ifMatch: 0 });

    expect(seen.ifMatch).toBe("0");
  });
});

describe("what a recognized retry proves", () => {
  const RETRY = { headSequence: 12, idempotency: { recognizedRetry: true, originalDecisionSequence: 12 } };

  /**
   * THE FINDING, in its succeeding direction: the head did not move, so no forward scan runs, and
   * the decision is read back where the Runtime said it landed. Success now carries the event.
   */
  it("reads the decision back at the sequence the Runtime named", async () => {
    const { client } = clientWith([12, 12], { status: 200, ok: true, reply: RETRY }, [
      cancelledBy(CALLER, "k-cancelled-abc", 12),
    ]);

    const evidence = await client.cancel("demo", MINE);

    expect(evidence.result).toBe("succeeded");
    expect(evidence.newEvents.map((event) => [event.kind, event.sequence])).toEqual([
      ["execution_completed", 12],
    ]);
  });

  /**
   * A PROOF WITHOUT A COORDINATE IS NOT A PROOF. The predecessor answered `succeeded` here with an
   * empty event list -- the exact shape the review named. It is `unknown`: the mutation may well
   * have committed, and this client could not read the evidence for it.
   */
  it("is unknown when the Runtime names no usable sequence", async () => {
    const { client } = clientWith(
      [12, 12],
      { status: 200, ok: true, reply: { headSequence: 12, idempotency: { recognizedRetry: true } } },
      [cancelledBy(CALLER, "k-cancelled-abc", 12)],
    );

    const evidence = await client.cancel("demo", MINE);

    expect(evidence.result).toBe("unknown");
    expect(evidence.newEvents).toEqual([]);
    expect(
      evidence.diagnostics.some((diagnostic) =>
        diagnostic.path === "/idempotency/originalDecisionSequence"
      ),
    ).toBe(true);
  });

  /**
   * THE ATTRIBUTION CONTROL, and the reason the read-back goes through the same predicate the
   * forward scan uses. There IS an `execution_completed` at the named sequence, so a check that
   * only asked "is something there" would pass -- but it is another actor's, under another key.
   */
  it("refuses a decision at that sequence that belongs to someone else", async () => {
    const { client } = clientWith([12, 12], { status: 200, ok: true, reply: RETRY }, [
      cancelledBy({ type: "owner", id: "studio-operator" }, "b-cancelled-999", 12),
    ]);

    const evidence = await client.cancel("demo", MINE);

    expect(evidence.result).toBe("unknown");
    expect(evidence.newEvents).toEqual([]);
  });
});
