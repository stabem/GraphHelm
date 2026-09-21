import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  DisconnectedError,
  OPERATOR_ACTOR,
  RuntimeClient,
  RuntimeError,
  WEBMCP_ACTOR,
} from "./client";

/** One recorded request, so a test can assert on the exact headers a mutation sent. */
interface Call {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: unknown;
}

interface Reply {
  status?: number;
  envelope: unknown;
}

/**
 * A scripted `fetch`.
 *
 * Replies are matched by a predicate over the URL and method rather than consumed in order: the
 * client issues a read, then the mutation, then two more reads, and a positional queue would
 * make every test depend on that internal ordering. Matching by request keeps the assertions
 * about the CONTRACT rather than about the call sequence.
 */
function scriptedFetch(routes: Array<{ match: (call: Call) => boolean; reply: Reply | ((call: Call) => Reply) }>) {
  const calls: Call[] = [];
  const fetchImpl = (async (url: string, init: RequestInit = {}) => {
    const call: Call = {
      url,
      method: init.method ?? "GET",
      headers: (init.headers ?? {}) as Record<string, string>,
      body: init.body === undefined ? undefined : JSON.parse(init.body as string),
    };
    calls.push(call);
    const route = routes.find((candidate) => candidate.match(call));
    if (!route) throw new Error(`no scripted reply for ${call.method} ${call.url}`);
    const reply = typeof route.reply === "function" ? route.reply(call) : route.reply;
    const status = reply.status ?? 200;
    return {
      ok: status >= 200 && status < 300,
      status,
      json: async () => reply.envelope,
    } as Response;
  }) as unknown as typeof fetch;
  return { fetchImpl, calls };
}

function ok(data: unknown, command = "execution.status") {
  return { envelope: { ok: true, command, data, diagnostics: [] } };
}

function statusData(overrides: Record<string, unknown> = {}) {
  return {
    executionId: "demo",
    mode: "supervised",
    status: "running",
    attention: "needs_you",
    attentionReasons: [{ kind: "blocked_node", node: "implementation" }],
    nodeStateCounts: { blocked: 1 },
    untriagedInterruptions: [],
    silenceUnevaluated: [],
    startedAt: "2026-08-27T12:00:00Z",
    lastEventAt: "2026-08-27T12:01:00Z",
    nodeLastEventAt: {},
    headSequence: 13,
    ...overrides,
  };
}

describe("RuntimeClient reads", () => {
  it("unwraps the four-key envelope and returns only its data", async () => {
    const { fetchImpl } = scriptedFetch([
      { match: (call) => call.url === "/v1/executions", reply: ok({ executions: [], hasMore: false, nextCursor: null }, "execution.list") },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.listExecutions()).resolves.toEqual({ executions: [], hasMore: false, nextCursor: null });
  });

  it("turns a diagnostic envelope into a RuntimeError carrying the code and pointer", async () => {
    const { fetchImpl } = scriptedFetch([
      {
        match: () => true,
        reply: {
          status: 400,
          envelope: {
            ok: false,
            command: "execution.list",
            data: null,
            diagnostics: [
              { code: "GHCLI001_ARGUMENT_INVALID", severity: "error", message: "limit must be between 1 and 100", path: "/limit", source: "execution-cli" },
            ],
          },
        },
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const error = await client.getStatus("demo").catch((reason: unknown) => reason);
    expect(error).toBeInstanceOf(RuntimeError);
    expect((error as RuntimeError).code).toBe("GHCLI001_ARGUMENT_INVALID");
    expect((error as RuntimeError).httpStatus).toBe(400);
    expect((error as RuntimeError).message).toBe("limit must be between 1 and 100");
  });

  it("reports a refused token without echoing the token", async () => {
    const { fetchImpl } = scriptedFetch([
      { match: () => true, reply: { status: 401, envelope: { ok: false, command: "unauthorized", data: null, diagnostics: [] } } },
    ]);
    const client = new RuntimeClient("super-secret-token", { fetch: fetchImpl });
    const error = (await client.getStatus("demo").catch((reason: unknown) => reason)) as RuntimeError;
    expect(error.message).toBe("The bearer token was refused.");
    expect(error.message).not.toContain("super-secret-token");
  });

  it("flattens the externally-tagged kind into kind and payload", async () => {
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.url.includes("/events"),
        reply: ok(
          {
            head: 4,
            events: [
              {
                sequence: 4,
                kind: { type: "node_outcome_recorded", data: { nodeId: "implementation", outcome: "approved" } },
                occurredAt: "2026-08-27T12:01:00Z",
                actor: { id: "studio-webmcp-adapter", type: "agent" },
                idempotencyKey: "k-1",
                eventId: "event-1",
                // A well-formed reference, a reference carrying fields this client does not read,
                // and two entries that are not references at all. Only the ids survive, and one
                // unusable entry must not cost the event its readable ones.
                evidenceRefs: [
                  { evidenceId: "ev-good", contentSha256: "abc", mediaType: "text/plain" },
                  { evidenceId: 7 },
                  "not-an-object",
                  { evidenceId: "" },
                ],
              },
            ],
          },
          "execution.events",
        ),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const page = await client.getEvents("demo");
    expect(page.events[0]).toEqual({
      sequence: 4,
      kind: "node_outcome_recorded",
      payload: { nodeId: "implementation", outcome: "approved" },
      occurredAt: "2026-08-27T12:01:00Z",
      actorId: "studio-webmcp-adapter",
      actorType: "agent",
      idempotencyKey: "k-1",
      eventId: "event-1",
      evidenceRefs: ["ev-good"],
    });
  });

  it("sends the events cursor as an exclusive after", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: () => true, reply: ok({ head: 9, events: [] }, "execution.events") },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.getEvents("demo", { after: 7, limit: 25 });
    expect(calls[0].url).toBe("/v1/executions/demo/events?after=7&limit=25");
  });

  it("refuses a list limit the API would refuse, without spending a request", async () => {
    const { fetchImpl, calls } = scriptedFetch([{ match: () => true, reply: ok({}) }]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.listExecutions({ limit: 500 })).rejects.toBeInstanceOf(RuntimeError);
    expect(calls).toHaveLength(0);
  });
});

describe("the token", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("is sent as a bearer header and written to no storage", async () => {
    const setLocal = vi.spyOn(Storage.prototype, "setItem");
    const { fetchImpl, calls } = scriptedFetch([{ match: () => true, reply: ok(statusData()) }]);
    const client = new RuntimeClient("tok-abc", { fetch: fetchImpl });
    await client.getStatus("demo");

    expect(calls[0].headers.Authorization).toBe("Bearer tok-abc");
    expect(setLocal).not.toHaveBeenCalled();
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
    expect(document.cookie).toBe("");
  });

  it("cannot be serialised out of the client by accident", () => {
    const client = new RuntimeClient("tok-abc");
    expect(JSON.stringify(client)).not.toContain("tok-abc");
    expect(JSON.stringify({ client })).toBe('{"client":{"runtimeClient":"connected"}}');
    expect(String(client)).toBe("[RuntimeClient]");
  });

  it("is forgotten by dispose, and every later call refuses instead of sending Bearer null", async () => {
    const { fetchImpl, calls } = scriptedFetch([{ match: () => true, reply: ok(statusData()) }]);
    const client = new RuntimeClient("tok-abc", { fetch: fetchImpl });
    client.dispose();
    expect(client.connected).toBe(false);
    await expect(client.getStatus("demo")).rejects.toBeInstanceOf(DisconnectedError);
    expect(calls).toHaveLength(0);
  });
});

describe("verified mutations", () => {
  it("always sends a cooperative pause body even when an untrusted caller adds mode", async () => {
    let posted = false;
    const { fetchImpl, calls } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          return ok({ headSequence: 14 }, "execution.pause");
        },
      },
      {
        match: (call) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: posted ? 14 : 13 })),
      },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok(
          {
            head: 14,
            events: [{ sequence: 14, kind: "execution_paused", idempotencyKey: "k-cooperative-paused-0123456789abcdef" }],
          },
          "execution.events",
        ),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.pause("demo", { idempotencyKey: "k-cooperative", mode: "immediate" } as never);

    expect(calls.find((call) => call.method === "POST")?.body).toEqual({});
  });

  /** The addressing fields exist so a multi-agent thread can be threaded by envelope instead of
   * by prose conventions. VERBATIM when given, ABSENT when not - a `"to": null` is a present
   * field with an unusable value, and schema 1.0.0 consumers would refuse the whole envelope. */
  it("addresses a message only when the caller addressed it", async () => {
    const routes = () => [
      {
        match: (call: { method: string }) => call.method === "POST",
        reply: ok({ headSequence: 14 }, "execution.signal"),
      },
      {
        match: (call: { method: string; url: string }) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: 14 })),
      },
      {
        match: (call: { url: string }) => call.url.includes("/events"),
        reply: ok({ head: 14, events: [] }, "execution.events"),
      },
    ];

    const addressed = scriptedFetch(routes());
    const client = new RuntimeClient("tok", { fetch: addressed.fetchImpl });
    await client.signal("demo", "resposta", { to: "codex", replyTo: "signal-17" });
    const sent = (addressed.calls.find((call) => call.method === "POST")?.body as { signal: Record<string, unknown> })
      .signal;
    expect(sent.to).toBe("codex");
    expect(sent.replyTo).toBe("signal-17");

    const plain = scriptedFetch(routes());
    const plainClient = new RuntimeClient("tok", { fetch: plain.fetchImpl });
    await plainClient.signal("demo", "sem endereco");
    const unsent = (plain.calls.find((call) => call.method === "POST")?.body as { signal: Record<string, unknown> })
      .signal;
    expect(Object.keys(unsent)).not.toContain("to");
    expect(Object.keys(unsent)).not.toContain("replyTo");
  });

  it("does not expose immediate pause in the client type", () => {
    type PauseOptions = NonNullable<Parameters<RuntimeClient["pause"]>[1]>;
    // @ts-expect-error Immediate pause is unavailable until the Runtime can attribute it.
    const forbidden: PauseOptions = { mode: "immediate" };
    expect(forbidden).toEqual({ mode: "immediate" });
  });

  /** The full happy path: read the head, mutate against it, read back, report the delta. */
  it("sends every required mutation header and reports the verified head movement", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: ok({ headSequence: 15 }, "execution.approve") },
      { match: (call) => call.url.includes("/events"), reply: ok({ head: 15, events: [{ sequence: 15, kind: { type: "node_outcome_recorded", data: { nodeId: "implementation", outcome: "approved" } }, actor: { id: WEBMCP_ACTOR.id, type: WEBMCP_ACTOR.type }, idempotencyKey: "fixed-key-1-outcome-0123456789abcdef" }] }, "execution.events") },
      {
        match: (call) => call.method === "GET",
        reply: (call) => (calls.filter((c) => c.method === "POST").length === 0 ? ok(statusData()) : ok(statusData({ headSequence: 15, attention: "can_sleep" }))),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.approve("demo", "implementation", {
      actor: WEBMCP_ACTOR,
      idempotencyKey: "fixed-key-1",
    });

    const post = calls.find((call) => call.method === "POST")!;
    expect(post.url).toBe("/v1/executions/demo/approve");
    expect(post.body).toEqual({ node: "implementation" });
    expect(post.headers["Idempotency-Key"]).toBe("fixed-key-1");
    expect(post.headers["X-GraphHelm-Actor"]).toBe("studio-webmcp-adapter");
    expect(post.headers["X-GraphHelm-Actor-Type"]).toBe("agent");
    expect(post.headers["If-Match"]).toBe("13");

    expect(evidence.result).toBe("succeeded");
    expect(evidence.headBefore).toBe(13);
    expect(evidence.headAfter).toBe(15);
    expect(evidence.newEvents.map((event) => event.kind)).toEqual(["node_outcome_recorded"]);
    expect(evidence.actor).toEqual(WEBMCP_ACTOR);
  });

  it("does not accept a pause key attached to the wrong event kind", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: () => {
        posted = true;
        return ok({ headSequence: 14 }, "execution.pause");
      } },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok({
          head: 14,
          events: [{
            sequence: 14,
            kind: "node_held",
            actor: { id: OPERATOR_ACTOR.id, type: OPERATOR_ACTOR.type },
            idempotencyKey: "k-wrong-kind-paused-0123456789abcdef",
          }],
        }, "execution.events"),
      },
      { match: () => true, reply: () => ok(statusData({ headSequence: posted ? 14 : 13 })) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });

    const evidence = await client.pause("demo", { idempotencyKey: "k-wrong-kind" });

    expect(evidence.result).toBe("unknown");
    expect(evidence.newEvents).toEqual([]);
  });

  it("does not accept an approve decision for a different node", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: () => {
        posted = true;
        return ok({ headSequence: 14 }, "execution.approve");
      } },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok({
          head: 14,
          events: [{
            sequence: 14,
            kind: { type: "node_outcome_recorded", data: { nodeId: "other-node", outcome: "approved" } },
            actor: { id: OPERATOR_ACTOR.id, type: OPERATOR_ACTOR.type },
            idempotencyKey: "k-wrong-node-outcome-0123456789abcdef",
          }],
        }, "execution.events"),
      },
      { match: () => true, reply: () => ok(statusData({ headSequence: posted ? 14 : 13 })) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });

    const evidence = await client.approve("demo", "implementation", { idempotencyKey: "k-wrong-node" });

    expect(evidence.result).toBe("unknown");
    expect(evidence.newEvents).toEqual([]);
  });

  it("does not accept a resume decision recorded under a different actor", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: () => {
        posted = true;
        return ok({ headSequence: 14 }, "execution.resume");
      } },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok({
          head: 14,
          events: [{
            sequence: 14,
            kind: "execution_resumed",
            actor: { id: "different-agent", type: "agent" },
            idempotencyKey: "k-wrong-actor-resumed-0123456789abcdef",
          }],
        }, "execution.events"),
      },
      { match: () => true, reply: () => ok(statusData({ headSequence: posted ? 14 : 13 })) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });

    const evidence = await client.resume("demo", "graph.yaml", {
      actor: WEBMCP_ACTOR,
      idempotencyKey: "k-wrong-actor",
    });

    expect(evidence.result).toBe("unknown");
    expect(evidence.newEvents).toEqual([]);
  });

  it("excludes events appended after the confirmed POST head", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          return ok({ headSequence: 15 }, "execution.approve");
        },
      },
      {
        match: (call) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: posted ? 17 : 13 })),
      },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok(
          {
            head: 17,
            events: [
              { sequence: 14, kind: "other_writer", idempotencyKey: "fresh-unrelated-key" },
              { sequence: 15, kind: { type: "node_outcome_recorded", data: { nodeId: "implementation", outcome: "approved" } }, actor: { id: OPERATOR_ACTOR.id, type: OPERATOR_ACTOR.type }, idempotencyKey: "k-concurrent-outcome-0123456789abcdef" },
              { sequence: 16, kind: "other_writer", idempotencyKey: "other-key" },
              { sequence: 17, kind: "other_writer", idempotencyKey: "other-key" },
            ],
          },
          "execution.events",
        ),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.approve("demo", "implementation", { idempotencyKey: "k-concurrent" });

    expect(evidence.result).toBe("succeeded");
    expect(evidence.headAfter).toBe(17);
    expect(evidence.newEvents.map((event) => event.sequence)).toEqual([15]);
  });

  it("pages through the complete event delta before reporting success", async () => {
    let posted = false;
    const { fetchImpl, calls } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          return ok({ headSequence: 411 }, "execution.resume");
        },
      },
      {
        match: (call) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: posted ? 411 : 10 })),
      },
      {
        match: (call) => call.url.includes("/events"),
        reply: (call) => {
          const after = Number(new URL(call.url, "http://studio.local").searchParams.get("after"));
          const last = Math.min(after + 200, 411);
          return ok(
            {
              head: 411,
              events: Array.from({ length: last - after }, (_, index) => ({
                sequence: after + index + 1,
                kind: after + index + 1 === 411 ? "execution_resumed" : "node_state_changed",
                actor: after + index + 1 === 411
                  ? { id: OPERATOR_ACTOR.id, type: OPERATOR_ACTOR.type }
                  : undefined,
                idempotencyKey: after + index + 1 === 411
                  ? "k-many-events-resumed-0123456789abcdef"
                  : `fresh-${after + index + 1}`,
              })),
            },
            "execution.events",
          );
        },
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.resume("demo", "graph.yaml", { idempotencyKey: "k-many-events" });

    expect(evidence.result).toBe("succeeded");
    expect(evidence.newEvents).toHaveLength(1);
    expect(evidence.newEvents[0]?.sequence).toBe(411);
    expect(evidence.newEvents.at(-1)?.sequence).toBe(411);
    expect(calls.filter((call) => call.url.includes("/events")).map((call) => call.url)).toEqual([
      "/v1/executions/demo/events?after=10&limit=200",
      "/v1/executions/demo/events?after=210&limit=200",
      "/v1/executions/demo/events?after=410&limit=200",
    ]);
  });

  it("reports an incomplete event delta as unknown", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          return ok({ headSequence: 250 }, "execution.resume");
        },
      },
      {
        match: (call) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: posted ? 250 : 10 })),
      },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok({ head: 250, events: [] }, "execution.events"),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.resume("demo", "graph.yaml", { idempotencyKey: "k-incomplete-events" });

    expect(evidence.result).toBe("unknown");
    expect(evidence.headAfter).toBe(250);
    expect(evidence.newEvents).toEqual([]);
  });

  it("stops paging and reports unknown when an event page does not advance", async () => {
    let posted = false;
    let eventReads = 0;
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          return ok({ headSequence: 12 }, "execution.resume");
        },
      },
      {
        match: (call) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: posted ? 12 : 10 })),
      },
      {
        match: (call) => call.url.includes("/events"),
        reply: () => {
          eventReads += 1;
          return ok(
            { head: 12, events: eventReads === 1 ? [{ sequence: 10, kind: "duplicate" }] : [] },
            "execution.events",
          );
        },
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.resume("demo", "graph.yaml", { idempotencyKey: "k-stalled-events" });

    expect(evidence.result).toBe("unknown");
    expect(eventReads).toBe(1);
  });

  it("excludes pause fan-out events that use fresh keys", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          return ok({ headSequence: 16 }, "execution.pause");
        },
      },
      {
        match: (call) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: posted ? 16 : 13 })),
      },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok(
          {
            head: 16,
            events: [
              { sequence: 14, kind: "execution_paused", actor: { id: OPERATOR_ACTOR.id, type: OPERATOR_ACTOR.type }, idempotencyKey: "k-pause-paused-0123456789abcdef" },
              { sequence: 15, kind: "node_held", idempotencyKey: "fresh-hold-1" },
              { sequence: 16, kind: "node_held", idempotencyKey: "fresh-hold-2" },
            ],
          },
          "execution.events",
        ),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.pause("demo", { idempotencyKey: "k-pause" });

    expect(evidence.result).toBe("succeeded");
    expect(evidence.newEvents.map((event) => event.sequence)).toEqual([14]);
  });

  it("reports unknown when the confirmed head advances without an attributable decision event", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          return ok({ headSequence: 15 }, "execution.pause");
        },
      },
      {
        match: (call) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: posted ? 15 : 13 })),
      },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok(
          {
            head: 15,
            events: [
              { sequence: 14, kind: "node_held", idempotencyKey: "fresh-hold-1" },
              { sequence: 15, kind: "other_writer", idempotencyKey: "other-key" },
            ],
          },
          "execution.events",
        ),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.pause("demo", { idempotencyKey: "k-missing-decision" });

    expect(evidence.result).toBe("unknown");
    expect(evidence.newEvents).toEqual([]);
  });

  it("records an operator action as owner and an adapter action as agent", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: ok({ headSequence: 13 }, "execution.pause") },
      { match: () => true, reply: ok(statusData()) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.pause("demo", { actor: OPERATOR_ACTOR, idempotencyKey: "k-owner" });
    await client.pause("demo", { actor: WEBMCP_ACTOR, idempotencyKey: "k-agent" });
    const posts = calls.filter((call) => call.method === "POST");
    expect(posts[0].headers["X-GraphHelm-Actor-Type"]).toBe("owner");
    expect(posts[0].headers["X-GraphHelm-Actor"]).toBe("studio-operator");
    expect(posts[1].headers["X-GraphHelm-Actor-Type"]).toBe("agent");
    expect(posts[1].headers["X-GraphHelm-Actor"]).toBe("studio-webmcp-adapter");
  });

  it("reuses the caller's idempotency key so a retry of the same action is one action", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: ok({ headSequence: 13 }, "execution.pause") },
      { match: () => true, reply: ok(statusData()) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.pause("demo", { idempotencyKey: "retry-me" });
    await client.pause("demo", { idempotencyKey: "retry-me" });
    const keys = calls.filter((call) => call.method === "POST").map((call) => call.headers["Idempotency-Key"]);
    expect(keys).toEqual(["retry-me", "retry-me"]);
  });

  /** THE ONE THAT MATTERS MOST. A refusal must never come back wearing the success shape. */
  it("reports a refusal as refused, with the diagnostic, and never as succeeded", async () => {
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: {
          status: 409,
          envelope: {
            ok: false,
            command: "execution.approve",
            data: null,
            diagnostics: [
              { code: "GHCLI005_EXECUTION_STATE", severity: "error", message: "the node is not blocked", path: "/node", source: "execution-cli" },
            ],
          },
        },
      },
      { match: () => true, reply: ok(statusData()) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.approve("demo", "deploy", { idempotencyKey: "k" });
    expect(evidence.result).toBe("refused");
    expect(evidence.diagnostics[0].code).toBe("GHCLI005_EXECUTION_STATE");
    expect(evidence.newEvents).toEqual([]);
  });

  /**
   * An accepted mutation whose verification read fails is `unknown`, never `succeeded`.
   *
   * Reporting the optimistic case here is precisely the defect this client exists to prevent:
   * the one state an operator has to act on would look exactly like the one they can ignore.
   */
  it("reports an unverifiable mutation as unknown rather than as succeeded", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          return ok({ headSequence: 14 }, "execution.pause");
        },
      },
      {
        match: (call) => call.method === "GET",
        reply: () => (posted ? { status: 500, envelope: { ok: false, command: "x", data: null, diagnostics: [] } } : ok(statusData())),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.pause("demo", { idempotencyKey: "k" });
    expect(evidence.result).toBe("unknown");
    expect(evidence.headAfter).toBeNull();
    expect(evidence.statusAfter).toBeNull();
  });

  it("reports a lost POST response as unknown even when the verification head advances", async () => {
    let posted = false;
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: () => {
          posted = true;
          throw new TypeError("response lost");
        },
      },
      {
        match: (call) => call.method === "GET" && !call.url.includes("/events"),
        reply: () => ok(statusData({ headSequence: posted ? 16 : 13 })),
      },
      {
        match: (call) => call.url.includes("/events"),
        reply: ok(
          {
            head: 16,
            events: [
              { sequence: 14, kind: "other_writer", idempotencyKey: "other-key" },
              { sequence: 15, kind: "execution_paused", actor: { id: OPERATOR_ACTOR.id, type: OPERATOR_ACTOR.type }, idempotencyKey: "k-lost-response-paused-0123456789abcdef" },
              { sequence: 16, kind: "other_writer", idempotencyKey: "other-key" },
            ],
          },
          "execution.events",
        ),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.pause("demo", { idempotencyKey: "k-lost-response" });

    expect(evidence.result).toBe("unknown");
    expect(evidence.headAfter).toBe(16);
    expect(evidence.newEvents.map((event) => event.sequence)).toEqual([15]);
    expect(evidence.diagnostics[0]?.message).toBe("The Runtime could not be reached at this address.");
  });

  it("reports an ambiguous server failure as unknown", async () => {
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.method === "POST",
        reply: { status: 500, envelope: { ok: false, command: "execution.pause", data: null, diagnostics: [] } },
      },
      { match: () => true, reply: ok(statusData()) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.pause("demo", { idempotencyKey: "k-server-error" });

    expect(evidence.result).toBe("unknown");
    expect(evidence.diagnostics[0]?.message).toBe("The Runtime replied 500.");
  });

  it("pins If-Match to the head the caller supplies, skipping the pre-read", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: ok({ headSequence: 21 }, "execution.pause") },
      { match: () => true, reply: ok(statusData({ headSequence: 21 })) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.pause("demo", { ifMatch: 20, idempotencyKey: "k" });
    const post = calls.find((call) => call.method === "POST")!;
    expect(post.headers["If-Match"]).toBe("20");
    // Exactly one GET, and it is the verification read: the pre-read was skipped.
    expect(calls.filter((call) => call.method === "GET" && !call.url.includes("/events"))).toHaveLength(1);
  });

  it("relays a resume file path verbatim and never reads it", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: ok({ headSequence: 13 }, "execution.resume") },
      { match: () => true, reply: ok(statusData()) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.resume("demo", "examples/graphs/manual-override-deploy.yaml", { idempotencyKey: "k" });
    const post = calls.find((call) => call.method === "POST")!;
    expect(post.body).toEqual({ file: "examples/graphs/manual-override-deploy.yaml" });
  });

  it("refuses an oversized path before it reaches the wire", async () => {
    const { fetchImpl, calls } = scriptedFetch([{ match: () => true, reply: ok(statusData()) }]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.resume("demo", "x".repeat(513))).rejects.toBeInstanceOf(RuntimeError);
    expect(calls).toHaveLength(0);
  });
});

/**
 * #1083 F1 and F6, as the client meets them. The Runtime now answers a well-formed unknown id with
 * 404 `GHCLI028_EXECUTION_NOT_FOUND`; the client must not read that as "this Runtime has no
 * briefing route", and a START - whose target is supposed not to exist yet - must still go out,
 * guarded at head 0. A fixture-only Runtime's documented route-listing refusal is read once.
 */
describe("an unknown execution id and a fixture-only Runtime", () => {
  const notFound = (command: string): Reply => ({
    status: 404,
    envelope: { ok: false, command, data: null, diagnostics: [{ code: "GHCLI028_EXECUTION_NOT_FOUND", severity: "error", message: "no execution with this id exists in this store", path: "/execution", source: "execution-cli" }] },
  });

  it("throws the unknown-id refusal from getBriefing instead of degrading to 'no route'", async () => {
    const { fetchImpl } = scriptedFetch([{ match: (call) => call.url.endsWith("/briefing"), reply: notFound("execution.briefing") }]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.getBriefing("typo")).rejects.toMatchObject({ code: "GHCLI028_EXECUTION_NOT_FOUND", httpStatus: 404 });
  });

  it("still sends a start whose pre-read says the execution does not exist yet, guarded at head 0", async () => {
    let statusReads = 0;
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "GET" && call.url === "/v1/executions/run-new", reply: () => (statusReads++ === 0 ? notFound("execution.status") : ok(statusData({ executionId: "run-new", headSequence: 1 }))) },
      { match: (call) => call.method === "POST" && call.url === "/v1/executions/run-new/start", reply: ok(statusData({ executionId: "run-new", headSequence: 1 }), "execution.start") },
      { match: (call) => call.url.startsWith("/v1/executions/run-new/events"), reply: ok({ events: [], head: 1 }, "execution.events") },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.startTask("run-new", { apiVersion: "p50.dev/v1" }, { idempotencyKey: "k" });
    const post = calls.find((call) => call.method === "POST");
    expect(post, "the start reached the wire").toBeDefined();
    expect(post!.headers["If-Match"]).toBe("0");
  });

  it("refuses any other verb on an execution the pre-read says does not exist", async () => {
    const { fetchImpl, calls } = scriptedFetch([{ match: (call) => call.method === "GET", reply: notFound("execution.status") }]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.approve("typo", "implementation", { idempotencyKey: "k" })).rejects.toMatchObject({ code: "GHCLI028_EXECUTION_NOT_FOUND" });
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });

  it("reads a current Runtime's 200 configured:false as 'no routes', once per client", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.url === "/v1/gateway/routes", reply: ok({ configured: false, routes: [], reason: "no gateway manifest is configured on this server" }, "gateway.routes") },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.listRoutes()).resolves.toEqual({ configured: false, routes: [] });
    await expect(client.listRoutes()).resolves.toEqual({ configured: false, routes: [] });
    expect(calls.filter((call) => call.url === "/v1/gateway/routes")).toHaveLength(1);
  });

  it("reads an older Runtime's 400 fixture-only refusal as 'no routes' once per connection, silently", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      {
        match: (call) => call.url === "/v1/gateway/routes",
        reply: { status: 400, envelope: { ok: false, command: "gateway.routes", data: null, diagnostics: [{ code: "GHCLI001_ARGUMENT_INVALID", severity: "error", message: "this server has no configured manifest: pass the manifest query parameter", path: "/manifest", source: "serve-cli" }] } },
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.listRoutes()).resolves.toEqual({ configured: false, routes: [] });
    await expect(client.listRoutes()).resolves.toEqual({ configured: false, routes: [] });
    expect(calls.filter((call) => call.url === "/v1/gateway/routes")).toHaveLength(1);
  });

  it("keeps re-asking a configured Runtime and rethrows any other refusal", async () => {
    const configured = scriptedFetch([{ match: () => true, reply: ok({ routes: [] }, "gateway.routes") }]);
    const client = new RuntimeClient("tok", { fetch: configured.fetchImpl });
    await client.listRoutes();
    await client.listRoutes();
    expect(configured.calls).toHaveLength(2);

    const broken = scriptedFetch([
      { match: () => true, reply: { status: 400, envelope: { ok: false, command: "gateway.routes", data: null, diagnostics: [{ code: "GHCLI009_GATEWAY_INVALID", severity: "error", message: "bad manifest", path: "/manifest", source: "serve-cli" }] } } },
    ]);
    await expect(new RuntimeClient("tok", { fetch: broken.fetchImpl }).listRoutes()).rejects.toBeInstanceOf(RuntimeError);
  });
});

/**
 * #1077: the briefing is where a run's NAME and OBJECTIVE live. Both are sealed out of the
 * event payloads (D-036), so `GET /v1/executions/{id}/briefing` (#1063) is the only public
 * read that can say what a task is about - and an older Runtime without the route must degrade
 * to today's id-only naming, never to an error banner.
 */
describe("the briefing", () => {
  it("reads it under the same bearer and returns its data", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      {
        match: (call) => call.url === "/v1/executions/run-1/briefing",
        reply: ok({ name: "New task", objective: "Investigate slow login on mobile", executor: "gateway", nextStep: { kind: "dispatch", nodes: ["start"] }, asOfSequence: 3 }, "execution.briefing"),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const briefing = await client.getBriefing("run-1");
    expect(briefing?.objective).toBe("Investigate slow login on mobile");
    expect(briefing?.name).toBe("New task");
    expect(calls[0].method).toBe("GET");
    expect(calls[0].headers.Authorization).toBe("Bearer tok");
  });

  it("degrades to null when the Runtime has no briefing route, and still throws on anything else", async () => {
    const { fetchImpl } = scriptedFetch([
      {
        match: (call) => call.url.endsWith("/briefing"),
        reply: { status: 404, envelope: { ok: false, command: "serve.not_found", data: null, diagnostics: [{ code: "GHCLI008_SERVE_NOT_FOUND", severity: "error", message: "no route", path: "", source: "serve-cli" }] } },
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.getBriefing("run-1")).resolves.toBeNull();

    const broken = scriptedFetch([
      { match: (call) => call.url.endsWith("/briefing"), reply: { status: 500, envelope: { ok: false, command: "execution.briefing", data: null, diagnostics: [] } } },
    ]);
    const other = new RuntimeClient("tok", { fetch: broken.fetchImpl });
    await expect(other.getBriefing("run-1")).rejects.toBeInstanceOf(RuntimeError);
  });
});

describe("the gateway writes (#1171)", () => {
  /** A value distinctive enough that finding it anywhere is unambiguous. */
  const KEY = "sk-SENTINEL-client-1171-0123456789";

  /**
   * setCredential documents that the value goes in the BODY and nowhere else, and until now
   * nothing held it. A review lane proved the gap by appending
   * ?value=${encodeURIComponent(draft.value)} to the path -- the exact failure the comment
   * names -- and the whole suite stayed green, typecheck included.
   *
   * The URL matters more than it looks: the Runtime read audit records request paths and query
   * strings and response bodies, and never a request body. A key in the path would land in a
   * plaintext file on the operator machine.
   */
  it("puts the key in the body and never in the url", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      {
        match: (call) => call.method === "PUT",
        reply: ok(
          { id: "secret_deepseek_official", routes: ["deepseek_official"] },
          "gateway.credential.set",
        ),
      },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.setCredential({
      reference: "secret_deepseek_official",
      provider: "openai",
      usableBy: ["deepseek_official"],
      value: KEY,
    });

    const put = calls.find((call) => call.method === "PUT");
    expect(put).toBeDefined();
    // THE CONTROL, so a sweep that could never match cannot pass this cell: the body DOES carry
    // the value, and the reference IS in the url.
    expect(put?.body).toMatchObject({ value: KEY, provider: "openai" });
    expect(put?.url).toContain("secret_deepseek_official");
    // The sweep itself, over both spellings a caller could produce.
    expect(put?.url).not.toContain(KEY);
    expect(put?.url).not.toContain(encodeURIComponent(KEY));
    expect(JSON.stringify(put?.headers ?? {})).not.toContain(KEY);
  });
});
