import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  DisconnectedError,
  OPERATOR_ACTOR,
  RuntimeClient,
  RuntimeError,
  RUNTIME_REQUEST_TIMEOUT_MS,
  RUNTIME_READ_TIMEOUT_MS,
  WEBMCP_ACTOR,
  RUNTIME_UNREACHABLE_HEADER,
} from "./client";
import { digestOf } from "./customs";
import { foldTaskEvents, readTaskEventRecords } from "./team-tasks";
import type { RuntimeEvent } from "./types";

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
  it("says in words that no Runtime answers when the dev proxy replies 502 with no JSON", async () => {
    const fetchImpl = vi.fn(async () => new Response("", { status: 502 })) as unknown as typeof fetch;
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.listExecutions()).rejects.toThrow(/cannot reach the Runtime\. Start graphhelm serve/);
  });

  it("bounds a fetch that never resolves headers", async () => {
    vi.useFakeTimers();
    try {
      const fetchImpl = vi.fn(() => new Promise<Response>(() => {})) as unknown as typeof fetch;
      const client = new RuntimeClient("tok", { fetch: fetchImpl });
      const read = client.listExecutions();
      const rejection = expect(read).rejects.toMatchObject({
        code: "GHSTUDIO_REQUEST_TIMEOUT",
        httpStatus: 0,
      });
      // The run list replays runs: it gets the longer read budget.
      await vi.advanceTimersByTimeAsync(RUNTIME_READ_TIMEOUT_MS);
      await rejection;
      expect(fetchImpl).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });

  it("bounds a response body that never settles", async () => {
    vi.useFakeTimers();
    try {
      const fetchImpl = vi.fn(async () => ({
        ok: true,
        status: 200,
        json: () => new Promise<unknown>(() => {}),
      }) as Response) as unknown as typeof fetch;
      const client = new RuntimeClient("tok", { fetch: fetchImpl });
      const read = client.listExecutions();
      const rejection = expect(read).rejects.toMatchObject({
        code: "GHSTUDIO_REQUEST_TIMEOUT",
        httpStatus: 0,
      });
      // The run list replays runs: it gets the longer read budget.
      await vi.advanceTimersByTimeAsync(RUNTIME_READ_TIMEOUT_MS);
      await rejection;
      expect(fetchImpl).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });

  it("lets the read-only reply suggestions call outlive the ordinary deadline", async () => {
    vi.useFakeTimers();
    try {
      let resolveBody!: (value: unknown) => void;
      const fetchImpl = vi.fn(async () => ({
        ok: true,
        status: 200,
        json: () => new Promise<unknown>((resolve) => { resolveBody = resolve; }),
      }) as Response) as unknown as typeof fetch;
      const client = new RuntimeClient("tok", { fetch: fetchImpl });
      const read = client.getReplySuggestions("demo", "judge");
      const result = read.then((value) => value, (error: unknown) => error);
      await vi.advanceTimersByTimeAsync(RUNTIME_REQUEST_TIMEOUT_MS);
      resolveBody({ ok: true, command: "execution.reply_suggestions", data: {
        executionId: "demo", headSequence: 3, state: "ready", suggestions: [],
      }, diagnostics: [] });
      expect(await result).toMatchObject({ state: "ready", headSequence: 3 });
    } finally {
      vi.useRealTimers();
    }
  });

  it("waits for a slow gateway probe past the Runtime's own 30 s probe bound (#600 review)", async () => {
    // #549 measured a slow but alive runtime answering --version after 20.9 s; the Runtime probes
    // for up to 30 s. A Studio that gives up first reads that runtime as refused.
    vi.useFakeTimers();
    try {
      let resolveBody!: (value: unknown) => void;
      const fetchImpl = vi.fn(async () => ({
        ok: true,
        status: 200,
        json: () => new Promise<unknown>((resolve) => { resolveBody = resolve; }),
      }) as Response) as unknown as typeof fetch;
      const client = new RuntimeClient("tok", { fetch: fetchImpl });
      const probe = client.probeRoute("native_probe").then((value) => value, (error: unknown) => error);
      await vi.advanceTimersByTimeAsync(31_000);
      resolveBody({ ok: true, command: "gateway.probe", data: { route: "native_probe", health: "available" }, diagnostics: [] });
      expect(await probe).toEqual({ health: "available" });
    } finally {
      vi.useRealTimers();
    }
  });

  it("uses the capped reply suggestions deadline and keeps replay reads at sixty seconds", async () => {
    vi.useFakeTimers();
    try {
      const fetchImpl = vi.fn(() => new Promise<Response>(() => {})) as unknown as typeof fetch;
      const client = new RuntimeClient("tok", { fetch: fetchImpl });
      const suggestions = client.getReplySuggestions("demo", "judge");
      let settled = false;
      const result = suggestions.then((value) => { settled = true; return value; }, (error: unknown) => { settled = true; return error; });
      const ordinary = client.getStatus("demo").then((value) => value, (error: unknown) => error);
      await vi.advanceTimersByTimeAsync(RUNTIME_REQUEST_TIMEOUT_MS);
      let statusSettled = false;
      void ordinary.then(() => { statusSettled = true; });
      await Promise.resolve();
      expect(statusSettled).toBe(false);
      await vi.advanceTimersByTimeAsync(RUNTIME_READ_TIMEOUT_MS - RUNTIME_REQUEST_TIMEOUT_MS);
      expect(await ordinary).toMatchObject({ code: "GHSTUDIO_REQUEST_TIMEOUT", httpStatus: 0 });
      expect(settled).toBe(false);
      await vi.advanceTimersByTimeAsync(120_000 - RUNTIME_READ_TIMEOUT_MS);
      expect(await result).toMatchObject({ code: "GHSTUDIO_REQUEST_TIMEOUT", httpStatus: 0 });
    } finally {
      vi.useRealTimers();
    }
  });

  it("reports disposal as disconnect after suggestions outlive the ordinary deadline", async () => {
    vi.useFakeTimers();
    try {
      const fetchImpl = vi.fn(() => new Promise<Response>(() => {})) as unknown as typeof fetch;
      const client = new RuntimeClient("tok", { fetch: fetchImpl });
      const suggestions = client.getReplySuggestions("demo", "judge");
      const result = suggestions.then((value) => value, (error: unknown) => error);
      await vi.advanceTimersByTimeAsync(RUNTIME_REQUEST_TIMEOUT_MS);
      client.dispose();
      expect(await result).toBeInstanceOf(DisconnectedError);
    } finally {
      vi.useRealTimers();
    }
  });

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

  it("rejects invalid evidence IDs before making a request the Runtime would answer 400", async () => {
    const fetchImpl = vi.fn() as unknown as typeof fetch;
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    for (const id of ["bad id", "bad/id", "x".repeat(129)]) {
      await expect(client.readEvidence("demo", id)).rejects.toBeInstanceOf(RuntimeError);
    }
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("coalesces two waves of eight evidence reads and keeps each result bound to its ID", async () => {
    const calls: Call[] = [];
    const fetchImpl = (async (url: string, init: RequestInit = {}) => {
      const call: Call = {
        url,
        method: init.method ?? "GET",
        headers: (init.headers ?? {}) as Record<string, string>,
        body: init.body,
      };
      calls.push(call);
      const parsed = new URL(url, "http://runtime");
      const ids = JSON.parse(parsed.searchParams.get("ids") ?? "[]") as string[];
      const results = ids.map((id) => ({
        status: "available",
        evidenceId: id,
        mediaType: "text/plain",
        sensitivity: "internal",
        contentSha256: `hash-${id}`,
        content: `content-${id}`,
      }));
      return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: { results }, diagnostics: [] }), { status: 200 });
    }) as typeof fetch;
    const client = new RuntimeClient("token", { fetch: fetchImpl });

    for (const wave of [0, 1]) {
      const ids = Array.from({ length: 8 }, (_, index) => `wave-${wave}-${index}`);
      const values = await Promise.all(ids.map((id) => client.readEvidence("demo", id)));
      expect(values.map((value) => [value.evidenceId, value.content])).toEqual(ids.map((id) => [id, `content-${id}`]));
    }

    expect(calls).toHaveLength(2);
    expect(calls.map((call) => new URL(call.url, "http://runtime").pathname)).toEqual([
      "/v1/executions/demo/evidence",
      "/v1/executions/demo/evidence",
    ]);
  });

  it("maps reordered collection results by ID and keeps an item refusal local", async () => {
    const fetchImpl = (async (url: string) => {
      const parsed = new URL(url, "http://runtime");
      const ids = JSON.parse(parsed.searchParams.get("ids") ?? "[]") as string[];
      const result = (id: string) => ({ status: "available", evidenceId: id, mediaType: "text/plain", sensitivity: "internal", contentSha256: id, content: `content-${id}` });
      return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: { results: [
        { evidenceId: ids[1], status: "refused", message: "not referenced" },
        result(ids[0]),
      ] }, diagnostics: [] }), { status: 200 });
    }) as typeof fetch;
    const client = new RuntimeClient("token", { fetch: fetchImpl });
    const first = client.readEvidence("demo", "wanted-a");
    const second = client.readEvidence("demo", "wanted-b");
    await expect(first).resolves.toMatchObject({ evidenceId: "wanted-a", content: "content-wanted-a" });
    await expect(second).rejects.toMatchObject({ httpStatus: 409, message: "not referenced" });
  });

  it("keeps collection queues isolated by execution and client", async () => {
    const dispatched: Array<{ authorization: string; execution: string; ids: string[] }> = [];
    const fetchImpl = (async (url: string, init: RequestInit = {}) => {
      const parsed = new URL(url, "http://runtime");
      const parts = parsed.pathname.split("/");
      const execution = parts[3];
      const ids = JSON.parse(parsed.searchParams.get("ids") ?? "[]") as string[];
      dispatched.push({ authorization: new Headers(init.headers).get("Authorization") ?? "", execution, ids });
      const results = ids.map((id) => ({ status: "available", evidenceId: id, mediaType: "text/plain", sensitivity: "internal", contentSha256: id, content: `${execution}:${id}` }));
      return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: { results }, diagnostics: [] }), { status: 200 });
    }) as typeof fetch;
    const firstClient = new RuntimeClient("first-token", { fetch: fetchImpl });
    const secondClient = new RuntimeClient("second-token", { fetch: fetchImpl });
    const values = await Promise.all([
      firstClient.readEvidence("run-a", "a1"), firstClient.readEvidence("run-a", "a2"),
      firstClient.readEvidence("run-b", "b1"), firstClient.readEvidence("run-b", "b2"),
      secondClient.readEvidence("run-a", "c1"), secondClient.readEvidence("run-a", "c2"),
    ]);
    expect(values.map((item) => item.content)).toEqual([
      "run-a:a1", "run-a:a2", "run-b:b1", "run-b:b2", "run-a:c1", "run-a:c2",
    ]);
    expect(dispatched).toHaveLength(3);
    expect(dispatched.map(({ authorization, execution, ids }) => `${authorization}:${execution}:${ids.length}`).sort()).toEqual([
      "Bearer first-token:run-a:2", "Bearer first-token:run-b:2", "Bearer second-token:run-a:2",
    ]);
  });

  it("rejects duplicate, missing and unrequested collection entries", async () => {
    for (const results of [
      (ids: string[]) => [
        { status: "available", evidenceId: ids[0], mediaType: "text/plain", sensitivity: "internal", contentSha256: "a", content: "a" },
        { status: "available", evidenceId: ids[0], mediaType: "text/plain", sensitivity: "internal", contentSha256: "a", content: "a" },
      ],
      (ids: string[]) => [
        { status: "available", evidenceId: ids[0], mediaType: "text/plain", sensitivity: "internal", contentSha256: "a", content: "a" },
      ],
      (ids: string[]) => [
        { status: "available", evidenceId: "unrequested", mediaType: "text/plain", sensitivity: "internal", contentSha256: "a", content: "a" },
        { status: "available", evidenceId: ids[1], mediaType: "text/plain", sensitivity: "internal", contentSha256: "b", content: "b" },
      ],
    ]) {
      const fetchImpl = (async (url: string) => {
        const ids = JSON.parse(new URL(url, "http://runtime").searchParams.get("ids") ?? "[]") as string[];
        return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: { results: results(ids) }, diagnostics: [] }), { status: 200 });
      }) as typeof fetch;
      const client = new RuntimeClient("token", { fetch: fetchImpl });
      const reads = [client.readEvidence("demo", "first"), client.readEvidence("demo", "second")];
      await expect(Promise.all(reads)).rejects.toBeInstanceOf(RuntimeError);
    }
  });

  it("uses single reads only for a confirmed unsupported route or an oversized batch item", async () => {
    const calls: string[] = [];
    const fetchImpl = (async (url: string) => {
      const parsed = new URL(url, "http://runtime");
      calls.push(parsed.pathname);
      const ids = parsed.searchParams.get("ids");
      if (ids !== null) {
        return new Response(JSON.stringify({ ok: false, command: "serve.not_found", data: null, diagnostics: [
          { code: "GHCLI008_SERVE_NOT_FOUND", severity: "error", message: "no route matches this request", path: "/", source: "serve-cli" },
        ] }), { status: 404 });
      }
      const evidenceId = decodeURIComponent(parsed.pathname.split("/").at(-1)!);
      return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: {
        evidenceId, mediaType: "text/plain", sensitivity: "internal", contentSha256: evidenceId, content: `single-${evidenceId}`,
      }, diagnostics: [] }), { status: 200 });
    }) as typeof fetch;
    const client = new RuntimeClient("token", { fetch: fetchImpl });
    const oldServerValues = await Promise.all([client.readEvidence("old", "old-a"), client.readEvidence("old", "old-b")]);
    expect(oldServerValues.map((item) => item.content)).toEqual(["single-old-a", "single-old-b"]);
    expect(calls.filter((path) => path.endsWith("/evidence"))).toHaveLength(1);
    expect(calls.filter((path) => path.includes("/evidence/")).length).toBe(2);

    const refusalFetch = (async (url: string) => {
      const parsed = new URL(url, "http://runtime");
      if (parsed.searchParams.has("ids")) {
        const ids = JSON.parse(parsed.searchParams.get("ids")!) as string[];
        return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: { results: [
          { evidenceId: ids[0], status: "available", mediaType: "text/plain", sensitivity: "internal", contentSha256: "a", content: "a" },
          { evidenceId: ids[1], status: "single_read_required", message: "bounded item" },
        ] }, diagnostics: [] }), { status: 200 });
      }
      const evidenceId = decodeURIComponent(parsed.pathname.split("/").at(-1)!);
      return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: {
        evidenceId, mediaType: "text/plain", sensitivity: "internal", contentSha256: evidenceId, content: "single-large-item",
      }, diagnostics: [] }), { status: 200 });
    }) as typeof fetch;
    const boundedClient = new RuntimeClient("token", { fetch: refusalFetch });
    const bounded = await Promise.all([boundedClient.readEvidence("demo", "small"), boundedClient.readEvidence("demo", "large")]);
    expect(bounded.map((item) => item.content)).toEqual(["a", "single-large-item"]);
  });

  it("does not use an execution 404 as proof that collection reads are unsupported", async () => {
    const paths: string[] = [];
    const fetchImpl = (async (url: string) => {
      const parsed = new URL(url, "http://runtime");
      paths.push(parsed.pathname);
      return new Response(JSON.stringify({ ok: false, command: "execution.not_found", data: null, diagnostics: [
        { code: "GHE008_EXECUTION_NOT_FOUND", severity: "error", message: "execution was not found", path: "/executionId", source: "runtime" },
      ] }), { status: 404 });
    }) as typeof fetch;
    const client = new RuntimeClient("token", { fetch: fetchImpl });
    const reads = [client.readEvidence("missing", "item-a"), client.readEvidence("missing", "item-b")];
    await expect(Promise.all(reads)).rejects.toMatchObject({ httpStatus: 404 });
    expect(paths).toEqual(["/v1/executions/missing/evidence"]);
  });

  it("folds only task envelopes whose sealed content hash and actor match the event", async () => {
    async function sealed(evidenceId: string, actorId: string, kind: string, issue: number) {
      const content = JSON.stringify({
        id: `signal-${evidenceId}`,
        source: { type: "user", id: actorId },
        type: kind,
        description: JSON.stringify({
          schema: "graphhelm-task-event-v1", taskId: `issue-${issue}`, revision: 1,
          at: "2026-10-07T20:00:00Z", issue, lane: actorId, branch: `issue-${issue}-x`,
        }),
      });
      const contentSha256 = (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice("sha256:".length);
      return { evidenceId, mediaType: "application/json", sensitivity: "internal", contentSha256, content };
    }
    function event(sequence: number, actorId: string, evidence: Awaited<ReturnType<typeof sealed>>): RuntimeEvent {
      return {
        sequence, kind: "signal_recorded",
        payload: { kind: "task.claimed", sourceId: actorId, sourceKind: "user", signalId: `signal-${evidence.evidenceId}`, envelopeSha256: evidence.contentSha256 },
        occurredAt: null, actorId, actorType: "agent", idempotencyKey: null, eventId: `event-${sequence}`,
        evidenceRefs: [evidence.evidenceId],
      };
    }
    const valid = await sealed("valid-task", "lane-a", "task.claimed", 41);
    const forgedActor = await sealed("forged-task", "lane-a", "task.claimed", 42);
    const forgedHash = await sealed("bad-hash-task", "lane-a", "task.claimed", 43);
    const fixtures = new Map([[valid.evidenceId, valid], [forgedActor.evidenceId, forgedActor], [forgedHash.evidenceId, { ...forgedHash, contentSha256: "0".repeat(64) }]]);
    const client = new RuntimeClient("token", {
      fetch: (async (url: string) => {
        const parsed = new URL(url, "http://runtime");
        const requested = parsed.searchParams.get("ids");
        if (requested !== null) {
          const results = (JSON.parse(requested) as string[]).map((id) => ({ status: "available", ...fixtures.get(id)! }));
          return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: { results }, diagnostics: [] }), { status: 200 });
        }
        const id = decodeURIComponent(parsed.pathname.split("/").at(-1)!);
        return new Response(JSON.stringify({ ok: true, command: "execution.evidence", data: fixtures.get(id), diagnostics: [] }), { status: 200 });
      }) as typeof fetch,
    });
    const tasks = foldTaskEvents(await readTaskEventRecords({
      executionId: "demo",
      events: [event(1, "lane-a", valid), event(2, "other-actor", forgedActor), event(3, "lane-a", forgedHash)],
      readEvidence: (executionId, evidenceId) => client.readEvidence(executionId, evidenceId),
    }));

    expect(tasks.map(({ issue, step }) => ({ issue, step }))).toEqual([{ issue: 41, step: "plan" }]);
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

  it("adds process liveness to the board read without changing ordinary status", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.url === "/v1/executions/demo", reply: ok(statusData()) },
      { match: (call) => call.url === "/v1/seen-actors", reply: ok({ seenActors: ["lane-a"], seenActorAgesMs: { "lane-a": 180000 }, seenActorSources: { "lane-a": "claimed" } }, "serve.seen_actors") },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    expect((await client.getStatus("demo")).seenActors).toBeUndefined();
    expect(await client.getStatus("demo", true)).toMatchObject({ seenActors: ["lane-a"], seenActorAgesMs: { "lane-a": 180000 }, seenActorSources: { "lane-a": "claimed" } });
    expect(calls.filter((call) => call.url === "/v1/seen-actors")).toHaveLength(1);
  });

  it("stops requesting unsupported liveness after a 404 while status polling continues", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.url === "/v1/executions/demo", reply: ok(statusData()) },
      { match: (call) => call.url === "/v1/seen-actors", reply: { status: 404, envelope: { ok: false, diagnostics: [] } } },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.getStatus("demo", true);
    await client.getStatus("demo", true);
    expect(calls.filter((call) => call.url === "/v1/executions/demo")).toHaveLength(2);
    expect(calls.filter((call) => call.url === "/v1/seen-actors")).toHaveLength(1);
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
    await client.signal("demo", "resposta", { to: "codex", replyTo: "signal-17", kind: "native_persona_linked" });
    const sent = (addressed.calls.find((call) => call.method === "POST")?.body as { signal: Record<string, unknown> })
      .signal;
    expect(sent.to).toBe("codex");
    expect(sent.replyTo).toBe("signal-17");
    expect(sent.type).toBe("native_persona_linked");

    const plain = scriptedFetch(routes());
    const plainClient = new RuntimeClient("tok", { fetch: plain.fetchImpl });
    await plainClient.signal("demo", "sem endereco");
    const unsent = (plain.calls.find((call) => call.method === "POST")?.body as { signal: Record<string, unknown> })
      .signal;
    expect(Object.keys(unsent)).not.toContain("to");
    expect(Object.keys(unsent)).not.toContain("replyTo");
    expect(unsent.type).toBe("operator_note");

    // #396 (signal 1.2.0): a message spoken inside a task thread names its task; a malformed one
    // is never sent (the Runtime would refuse the whole message).
    const tagged = scriptedFetch(routes());
    await new RuntimeClient("tok", { fetch: tagged.fetchImpl }).signal("demo", "rebase please", { task: "issue-384" });
    expect((tagged.calls.find((call) => call.method === "POST")?.body as { signal: Record<string, unknown> }).signal.task).toBe("issue-384");
    const bad = scriptedFetch(routes());
    await new RuntimeClient("tok", { fetch: bad.fetchImpl }).signal("demo", "x", { task: "../etc" });
    expect(Object.keys((bad.calls.find((call) => call.method === "POST")?.body as { signal: Record<string, unknown> }).signal)).not.toContain("task");
    expect(Object.keys(unsent)).not.toContain("task");

    for (const kind of ["actor_alias", "owner_refusal"] as const) {
      const owned = scriptedFetch(routes());
      await new RuntimeClient("tok", { fetch: owned.fetchImpl }).signal("demo", "{}", { to: "kit-1", replyTo: "sig-q", kind });
      const body = (owned.calls.find((call) => call.method === "POST")?.body as { signal: Record<string, unknown> }).signal;
      expect(body.type).toBe(kind);
      expect(body.source).toMatchObject({ type: "user" });
    }
  });

  it("does not expose immediate pause in the client type", () => {
    type PauseOptions = NonNullable<Parameters<RuntimeClient["pause"]>[1]>;
    // @ts-expect-error Immediate pause is unavailable until the Runtime can attribute it.
    const forbidden: PauseOptions = { mode: "immediate" };
    expect(forbidden).toEqual({ mode: "immediate" });
  });

  it("rejects native persona links from non-owner callers before network I/O", async () => {
    const scripted = scriptedFetch([]);
    const client = new RuntimeClient("tok", { fetch: scripted.fetchImpl });
    await expect(client.signal("demo", "{}", {
      kind: "native_persona_linked",
      actor: { id: "agent-1", type: "agent" },
    })).rejects.toThrow(/must be recorded by the owner/i);
    for (const kind of ["actor_alias", "owner_refusal"] as const) {
      await expect(client.signal("demo", "{}", { kind, actor: { id: "agent-1", type: "agent" } })).rejects.toThrow(/must be recorded by the owner/i);
    }
    expect(scripted.calls).toHaveLength(0);
  });

  /** #1170: a connection the Runtime refused was never delivered, so it is a refusal, not "maybe applied". */
  it("reports a mutation as not sent when the dev proxy says the Runtime refused the connection", async () => {
    let up = true;
    const down = () => new Response("", { status: 503, headers: { [RUNTIME_UNREACHABLE_HEADER]: "connection-refused" } });
    const fetchImpl = vi.fn(async (_url: string, init: RequestInit = {}) => {
      if (up && (init.method ?? "GET") === "GET") {
        up = false;
        return new Response(JSON.stringify(ok(statusData()).envelope), { status: 200 });
      }
      return down();
    }) as unknown as typeof fetch;
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    const evidence = await client.approve("demo", "implementation", { idempotencyKey: "fixed-key-1" });
    expect(evidence.result).toBe("refused");
    expect(evidence.diagnostics[0]?.message).toMatch(/not answering, so nothing was sent/);
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

  /** Observable contract: governed approval posts the exact reviewed draft, digest, and assigned
   * actor. It catches the plausible defect of reusing the legacy node-only body, which could
   * approve a different proposal. Existing approval coverage only asserts that legacy body. */
  it("sends the exact governed proposal identity and assignment", async () => {
    const digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: ok({ headSequence: 15 }, "execution.approve") },
      { match: (call) => call.url.includes("/events"), reply: ok({ head: 15, events: [{ sequence: 15, kind: { type: "node_outcome_recorded", data: { nodeId: "review", outcome: "approved" } }, actor: { id: OPERATOR_ACTOR.id, type: OPERATOR_ACTOR.type }, idempotencyKey: "fixed-key-1-outcome-0123456789abcdef" }] }, "execution.events") },
      { match: () => true, reply: ok(statusData({ headSequence: 15, attention: "can_sleep" })) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.approve("demo", "review", {
      actor: OPERATOR_ACTOR,
      idempotencyKey: "fixed-key-1",
      governed: { draftId: "draft-1", proposalDigest: digest, assignments: { review: "reviewer" } },
    });
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({
      node: "review", draftId: "draft-1", proposalDigest: digest, assignments: { review: "reviewer" },
    });
  });

  it("rejects a malformed governed proposal digest before making HTTP", async () => {
    const fetchImpl = vi.fn();
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.approve("demo", "review", {
      governed: { draftId: "draft-1", proposalDigest: "sha256:short", assignments: { review: "reviewer" } },
    })).rejects.toMatchObject({ message: "proposalDigest must be 64 lowercase hex characters." });
    expect(fetchImpl).not.toHaveBeenCalled();
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

  it("relays an inline resume graph without turning it into a browser path", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: ok({ headSequence: 13 }, "execution.resume") },
      { match: () => true, reply: ok(statusData()) },
    ]);
    const graph = { apiVersion: "p50.dev/graph/v1", kind: "ExecutionGraph", metadata: { executionId: "demo" } };
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.resume("demo", graph, { idempotencyKey: "k" });
    const post = calls.find((call) => call.method === "POST")!;
    expect(post.body).toEqual({ graph });
  });

  it("omits the source fields when resuming from the Runtime's sealed snapshot", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.method === "POST", reply: ok({ headSequence: 13 }, "execution.resume") },
      { match: () => true, reply: ok(statusData()) },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await client.resume("demo", undefined, { idempotencyKey: "k" });
    expect(calls.find((call) => call.method === "POST")?.body).toEqual({});
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

/**
 * #1192 (plan A1, slice 2): the task projection comes from the Runtime (`GET
 * /v1/executions/{id}/tasks`, #1149). The cells pin the three answers the views act on: the reply
 * under the bearer; `null` for a Runtime that predates the route (404 `serve.not_found` or 401 on
 * this route), which the views turn into one sentence instead of an empty board; and a throw for
 * an unknown execution, a broken store or a reply of an unknown shape, so none of those is ever
 * read as "old server". Cost: scripted fetch, milliseconds.
 */
describe("the tasks route (#1192)", () => {
  const reply = { executionId: "run-1", headSequence: 7, tasks: [{ key: "issue-1#pr-2", taskId: "issue-1", step: "review" }], records: [{ kind: "task.claimed", actorId: "lane-a", sequence: 1, taskId: "issue-1" }], rejected: 0 };

  it("reads the projection under the same bearer and hands the reply through whole", async () => {
    const { fetchImpl, calls } = scriptedFetch([
      { match: (call) => call.url === "/v1/executions/run-1/tasks", reply: ok(reply, "execution.tasks") },
    ]);
    const client = new RuntimeClient("tok", { fetch: fetchImpl });
    await expect(client.getTasks("run-1")).resolves.toEqual(reply);
    expect(calls[0].method).toBe("GET");
    expect(calls[0].headers.Authorization).toBe("Bearer tok");
  });

  it("answers null for a Runtime that predates the route: 404 without the unknown-id code, or 401", async () => {
    const notFound = scriptedFetch([
      { match: (call) => call.url.endsWith("/tasks"), reply: { status: 404, envelope: { ok: false, command: "serve.not_found", data: null, diagnostics: [{ code: "GHCLI008_SERVE_NOT_FOUND", severity: "error", message: "no route", path: "", source: "serve-cli" }] } } },
    ]);
    await expect(new RuntimeClient("tok", { fetch: notFound.fetchImpl }).getTasks("run-1")).resolves.toBeNull();
    const unauthorized = scriptedFetch([
      { match: (call) => call.url.endsWith("/tasks"), reply: { status: 401, envelope: { ok: false, command: "serve.unauthorized", data: null, diagnostics: [{ code: "GHCLI007_SERVE_UNAUTHORIZED", severity: "error", message: "a valid Authorization: Bearer token is required", path: "/authorization", source: "serve-cli" }] } } },
    ]);
    await expect(new RuntimeClient("tok", { fetch: unauthorized.fetchImpl }).getTasks("run-1")).resolves.toBeNull();
  });

  it("throws for an unknown execution, a broken store and an unknown reply shape", async () => {
    const unknownId = scriptedFetch([
      { match: (call) => call.url.endsWith("/tasks"), reply: { status: 404, envelope: { ok: false, command: "execution.tasks", data: null, diagnostics: [{ code: "GHCLI028_EXECUTION_NOT_FOUND", severity: "error", message: "no execution with this id exists in this store", path: "/execution", source: "execution-cli" }] } } },
    ]);
    await expect(new RuntimeClient("tok", { fetch: unknownId.fetchImpl }).getTasks("typo")).rejects.toMatchObject({ code: "GHCLI028_EXECUTION_NOT_FOUND", httpStatus: 404 });
    const broken = scriptedFetch([
      { match: (call) => call.url.endsWith("/tasks"), reply: { status: 500, envelope: { ok: false, command: "execution.tasks", data: null, diagnostics: [] } } },
    ]);
    await expect(new RuntimeClient("tok", { fetch: broken.fetchImpl }).getTasks("run-1")).rejects.toBeInstanceOf(RuntimeError);
    const odd = scriptedFetch([
      { match: (call) => call.url.endsWith("/tasks"), reply: ok({ executionId: "run-1", tasks: "not a list" }, "execution.tasks") },
    ]);
    await expect(new RuntimeClient("tok", { fetch: odd.fetchImpl }).getTasks("run-1")).rejects.toBeInstanceOf(RuntimeError);
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

// #673: scripted HTTP only; health previously discarded these diagnostic records.
it("returns independently validated previous exit and startup records from one health request", async () => {
  const prior = "0123456789abcdef0123456789abcdef";
  for (const [previousExit, expected] of [
    [{ state: "vanished", pid: 1, at: 2, instance: prior }, { state: "vanished", pid: 1, at: 2, instance: prior }],
    [{ state: "vanished", pid: 1, at: 2, instance: null }, { state: "vanished", pid: 1, at: 2 }],
    ...["a".repeat(33), "../private", "a\n"].map(instance => [
      { state: "vanished", pid: 1, at: 2, instance }, { state: "vanished", pid: 1, at: 2 },
    ]),
    [{ state: "vanished", pid: 1, at: 2 }, { state: "vanished", pid: 1, at: 2 }],
    [{ state: "<script>", pid: 1, at: 2 }, null],
    [{ state: "clean", pid: 1, at: 2 }, { state: "clean", pid: 1, at: 2 }],
    [{ state: "panicked", pid: 1, at: 2, location: "runtime/src/driver.rs:42", lastPanic: { location: "secret text" } }, { state: "panicked", pid: 1, at: 2, location: "runtime/src/driver.rs:42", lastPanic: { location: "unknown" } }],
    [{ state: "serve_error", pid: -1, at: 2 }, null],
    [{ state: "vanished", pid: 1, at: 1e30 }, null],
    [{ state: "vanished", pid: 1, at: 2, location: "C:/Users/private/src/main.rs:1", lastPanic: { location: "crate/src/../secret.rs:1" } }, { state: "vanished", pid: 1, at: 2, location: "unknown", lastPanic: { location: "unknown" } }],
    [undefined, null],
  ]) {
    const { fetchImpl, calls } = scriptedFetch([{ match: c => c.url === "/health", reply: ok({ previousExit, startup: { journalVerifyMs: 121, lifecycleStartMs: 60 } }, "health") }]);
    const client = new RuntimeClient("token", { fetch: fetchImpl });
    expect(await client.health()).toEqual({ previousExit: expected, startup: { journalVerifyMs: 121, lifecycleStartMs: 60 }, currentRun: null });
    expect(calls).toHaveLength(1);
  }
  for (const startup of [
    { journalVerifyMs: 0, lifecycleStartMs: 0 },
    undefined, null, [], "121", { journalVerifyMs: -1, lifecycleStartMs: 60 },
    { journalVerifyMs: 1.5, lifecycleStartMs: 60 },
    { journalVerifyMs: Number.MAX_SAFE_INTEGER + 1, lifecycleStartMs: 60 },
  ]) {
    const { fetchImpl, calls } = scriptedFetch([{ match: c => c.url === "/health", reply: ok({ previousExit: null, startup }, "health") }]);
    const client = new RuntimeClient("token", { fetch: fetchImpl });
    const valid = startup !== null && typeof startup === "object" && !Array.isArray(startup) &&
      Number.isSafeInteger((startup as { journalVerifyMs?: unknown }).journalVerifyMs) && (startup as { journalVerifyMs: number }).journalVerifyMs >= 0 &&
      Number.isSafeInteger((startup as { lifecycleStartMs?: unknown }).lifecycleStartMs) && (startup as { lifecycleStartMs: number }).lifecycleStartMs >= 0;
    expect(await client.health()).toEqual({ previousExit: null, startup: valid ? startup : null, currentRun: null });
    expect(calls).toHaveLength(1);
  }
});

it("decodes only the current Runtime identity from the same health response", async () => {
  const instance = "abcdef0123456789abcdef0123456789";
  const previousExit = { state: "clean", pid: 9, at: 8, instance: "0123456789abcdef0123456789abcdef" };
  const currentRun = { instance, pid: 42, startedAt: 1_700_000_000 };
  for (const [topInstance, run, expected] of [
    [instance, currentRun, currentRun],
    [instance, { ...currentRun, pid: 1, startedAt: 0 }, { ...currentRun, pid: 1, startedAt: 0 }],
    [undefined, currentRun, null],
    ["0123456789abcdef0123456789abcdef", currentRun, null],
    [instance, undefined, null],
    [instance, [], null],
    [instance, { ...currentRun, pid: 0 }, null],
    [instance, { ...currentRun, instance: "A".repeat(32) }, null],
    ["A".repeat(32), { ...currentRun, instance: "A".repeat(32) }, null],
    [instance, { ...currentRun, pid: 1.5 }, null],
    [instance, { ...currentRun, pid: "42" }, null],
    [instance, { ...currentRun, pid: 0x100000000 }, null],
    [instance, { ...currentRun, startedAt: -1 }, null],
    [instance, { ...currentRun, startedAt: 8_640_000_000_001 }, null],
    [instance, { ...currentRun, startedAt: 1.5 }, null],
    [instance, { ...currentRun, startedAt: "1700000000" }, null],
  ] as const) {
    const { fetchImpl, calls } = scriptedFetch([{ match: c => c.url === "/health", reply: ok({ instance: topInstance, currentRun: run, previousExit, startup: null }, "health") }]);
    const client = new RuntimeClient("token", { fetch: fetchImpl });
    expect(await client.health()).toEqual({ previousExit, startup: null, currentRun: expected });
    expect(calls).toHaveLength(1);
  }
  const { fetchImpl } = scriptedFetch([{ match: c => c.url === "/health", reply: ok({ instance, currentRun, previousExit: { state: "bad" }, startup: { journalVerifyMs: -1 } }, "health") }]);
  expect(await new RuntimeClient("token", { fetch: fetchImpl }).health()).toEqual({ previousExit: null, startup: null, currentRun });
});
