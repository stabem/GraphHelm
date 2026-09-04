import { describe, expect, it, vi } from "vitest";

import { MAX_NODE_TIMEOUT_SECONDS, RuntimeClient, TIMESTAMP_PATTERN } from "../runtime/client";
import type { MutationEvidence } from "../runtime/types";
import {
  findModelContext,
  registerStudioTools,
  type ModelContextLike,
  type ToolActivity,
  type WebMcpToolDescriptor,
} from "./adapter";

/** A stand-in for the browser's model-context object that keeps what was registered and honours
 * the abort signal the way the spec says a host should. */
function fakeModelContext() {
  const registered: WebMcpToolDescriptor[] = [];
  const aborted: WebMcpToolDescriptor[] = [];
  const modelContext: ModelContextLike = {
    registerTool: (tool, options) => {
      registered.push(tool);
      options?.signal?.addEventListener("abort", () => aborted.push(tool));
      return undefined;
    },
  };
  return { modelContext, registered, aborted };
}

function hooks() {
  const selected: string[] = [];
  const mutations: MutationEvidence[] = [];
  const activity: ToolActivity[] = [];
  return {
    selected,
    mutations,
    activity,
    value: {
      onSelect: (id: string) => selected.push(id),
      onMutation: (evidence: MutationEvidence) => mutations.push(evidence),
      onActivity: (next: ToolActivity) => activity.push(next),
    },
  };
}

/** A client whose methods are stubs: the adapter's job is to CALL the client, and these tests are
 * about that call, not about HTTP. */
function stubClient(overrides: Partial<Record<keyof RuntimeClient, unknown>> = {}) {
  const client = {
    connected: true,
    listExecutions: vi.fn(async () => ({ executions: [{ executionId: "demo" }], hasMore: false, nextCursor: null })),
    getStatus: vi.fn(async () => ({
      executionId: "demo",
      attention: "needs_you",
      attentionReasons: [{ kind: "blocked_node", node: "implementation" }],
      untriagedInterruptions: [],
      silenceUnevaluated: [],
      headSequence: 13,
      startedAt: null,
      lastEventAt: null,
      status: "running",
      nodeStateCounts: {},
    })),
    getEvents: vi.fn(async () => ({ head: 13, events: [] })),
    pause: vi.fn(async () => evidence("pause")),
    pauseImmediately: vi.fn(async () => evidence("pause")),
    approve: vi.fn(async () => evidence("approve", "implementation")),
    resume: vi.fn(async () => evidence("resume")),
    startTask: vi.fn(async () => evidence("start")),
    signal: vi.fn(async () => evidence("signal")),
    cancel: vi.fn(async () => evidence("cancel")),
    sweep: vi.fn(async () => evidence("sweep")),
    amendBudget: vi.fn(async () => evidence("amendBudget", "judge")),
    readEvidence: vi.fn(async () => ({
      evidenceId: "ev-1",
      mediaType: "application/json",
      sensitivity: "confidential",
      contentSha256: "sha256:whatever",
      content: JSON.stringify({ description: "the words that were sealed" }),
    })),
    ...overrides,
  };
  return client as unknown as RuntimeClient & typeof client;
}

function evidence(action: MutationEvidence["action"], node: string | null = null): MutationEvidence {
  return {
    action,
    executionId: "demo",
    node,
    actor: { id: "studio-webmcp-adapter", type: "agent" },
    idempotencyKey: "k",
    headBefore: 13,
    headAfter: 15,
    result: "succeeded",
    statusAfter: null,
    newEvents: [],
    diagnostics: [],
  };
}

const EXPECTED_TOOLS = [
  "graphhelm_list_executions",
  "graphhelm_get_attention",
  "graphhelm_get_execution_status",
  "graphhelm_get_execution_events",
  "graphhelm_read_evidence",
  "graphhelm_pause_execution",
  "graphhelm_approve_node",
  "graphhelm_resume_execution",
  "graphhelm_start_task",
  "graphhelm_send_message",
  "graphhelm_cancel_execution",
  "graphhelm_sweep_execution",
  "graphhelm_amend_node_budget",
];

describe("feature detection", () => {
  it("finds the model context on document, where current browsers expose it", () => {
    const context = { registerTool: () => {} };
    expect(findModelContext({ document: { modelContext: context } })).toBe(context);
  });

  it("also finds it on navigator, where earlier drafts put it", () => {
    const context = { registerTool: () => {} };
    expect(findModelContext({ navigator: { modelContext: context } })).toBe(context);
  });

  it("reports absence rather than throwing when neither exists", () => {
    expect(findModelContext({})).toBeNull();
  });

  it("rejects an object that is not a registrar", () => {
    expect(findModelContext({ document: { modelContext: { other: 1 } } })).toBeNull();
  });
});

describe("registration", () => {
  it("registers exactly the thirteen tools, each with a closed schema", () => {
    const { modelContext, registered } = fakeModelContext();
    const result = registerStudioTools(stubClient(), hooks().value, { modelContext });

    expect(result.availability).toBe("available");
    expect(result.names).toEqual(EXPECTED_TOOLS);
    expect(registered.map((tool) => tool.name)).toEqual(EXPECTED_TOOLS);
    for (const tool of registered) {
      expect(tool.inputSchema.additionalProperties).toBe(false);
      expect(tool.description.length).toBeGreaterThan(40);
    }
  });

  it("marks the five reads read-only and the eight writes not", () => {
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(stubClient(), hooks().value, { modelContext });
    const readOnly = registered.filter((tool) => tool.annotations?.readOnlyHint === true).map((tool) => tool.name);
    expect(readOnly).toEqual([
      "graphhelm_list_executions",
      "graphhelm_get_attention",
      "graphhelm_get_execution_status",
      "graphhelm_get_execution_events",
      "graphhelm_read_evidence",
    ]);
    const writes = registered.filter((tool) => tool.annotations?.readOnlyHint === false).map((tool) => tool.name);
    expect(writes).toEqual([
      "graphhelm_pause_execution",
      "graphhelm_approve_node",
      "graphhelm_resume_execution",
      "graphhelm_start_task",
      "graphhelm_send_message",
      "graphhelm_cancel_execution",
      "graphhelm_sweep_execution",
      "graphhelm_amend_node_budget",
    ]);
  });

  /** The runtime distinguishes TWO pauses and the tool surface must not collapse them: graceful
   * (nothing new starts; in-flight work finishes and is joined) is the default, and interrupting
   * is only reachable by an EXPLICIT `mode: "immediate"` - never by omission. The description
   * carries both promises so an agent chooses knowingly (Phase 2, #105). */
  it("offers both pauses, the interrupting one only by explicit mode", () => {
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(stubClient(), hooks().value, { modelContext });
    const pause = registered.find((tool) => tool.name === "graphhelm_pause_execution")!;
    const properties = pause.inputSchema.properties as Record<string, { enum?: string[] }>;

    expect(Object.keys(properties)).toEqual(["executionId", "mode", "ifMatch", "idempotencyKey"]);
    expect(properties.mode.enum).toEqual(["graceful", "immediate"]);
    expect(pause.description).toMatch(/graceful/i);
    expect(pause.description).toMatch(/immediate/i);
    for (const write of registered.filter((tool) => tool.annotations?.readOnlyHint === false)) {
      expect(write.description).toContain("attributable");
      expect(write.description).not.toContain("events actually appended");
    }
  });

  /** `cancel` WAS the destructive verb this surface deliberately withheld, and this guard is the
   * ritual its own comment demanded: Phase 2 (#105) orders operator action buttons WITH WebMCP
   * parity - the operator gets cancel behind a confirmation, and an agent's tool call passes the
   * host's own confirmation prompt, so the parity does not skip the consent the page requires.
   * The tool says DESTRUCTIVE in its first sentence; "delete" stays forbidden - nothing on this
   * API erases, and no tool may imply it. */
  it("exposes cancel as the one argued-for destructive verb, and nothing that erases", () => {
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(stubClient(), hooks().value, { modelContext });
    const cancel = registered.find((tool) => tool.name === "graphhelm_cancel_execution")!;
    expect(cancel.description).toMatch(/destructive/i);
    expect(registered.some((tool) => tool.name.includes("delete"))).toBe(false);
  });

  /** The agent gets `send_message`, and deliberately NOT a raw signal tool. A raw signal lets the
   * caller choose `type` and `severity`, and a recognized type (`no_progress`, `tool_failure`...)
   * is a CONTROL INPUT the Governor acts on - handing that to an agent turns "say something" into
   * "steer the run". `send_message` pins the envelope to the unrecognized note kind inside
   * `client.signal`, so the words always arrive and never steer. */
  it("offers messaging, never the raw signal control surface", () => {
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(stubClient(), hooks().value, { modelContext });
    expect(registered.some((tool) => tool.name === "graphhelm_send_message")).toBe(true);
    expect(registered.some((tool) => tool.name.includes("signal"))).toBe(false);
    const send = registered.find((tool) => tool.name === "graphhelm_send_message")!;
    const properties = send.inputSchema.properties as Record<string, unknown>;
    // No `type`, no `severity`: the schema itself is what makes the pin unforgeable from the
    // agent's side. `to`/`replyTo` joined in schema 1.1.0 and are ADDRESSING, not control - the
    // Governor never reads them; the two fields that could steer stay unreachable.
    expect(Object.keys(properties).sort()).toEqual(["executionId", "idempotencyKey", "message", "replyTo", "to"]);
  });

  /** THE HEAD THE AGENT OBSERVED rides as If-Match (L's follow-up on #662): every write tool
   * against an existing stream accepts `ifMatch` and passes it to the client untouched; a
   * present value of the wrong shape is refused, never dropped; start_task and send_message
   * stay outside the rule (no stream yet / a message answers by replyTo, not by head). */
  it("passes the agent's observed head as ifMatch on every stream-bound write tool, and refuses a malformed one", async () => {
    const client = stubClient();
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(client, hooks().value, { modelContext });
    const named = (name: string) => registered.find((tool) => tool.name === `graphhelm_${name}`)!;
    const cases: Array<[string, Record<string, unknown>, keyof typeof client]> = [
      ["pause_execution", { executionId: "demo", mode: "immediate" }, "pauseImmediately"],
      ["pause_execution", { executionId: "demo" }, "pause"],
      ["approve_node", { executionId: "demo", node: "implementation" }, "approve"],
      ["resume_execution", { executionId: "demo", file: "graph.yaml" }, "resume"],
      ["cancel_execution", { executionId: "demo" }, "cancel"],
      ["sweep_execution", { executionId: "demo" }, "sweep"],
      ["amend_node_budget", { executionId: "demo", node: "judge", seconds: 900, computedAtSequence: 4 }, "amendBudget"],
    ];
    for (const [name, input, method] of cases) {
      expect(Object.keys(named(name).inputSchema.properties as object)).toContain("ifMatch");
      const reply = JSON.parse(await named(name).execute({ ...input, ifMatch: 10 }));
      expect(reply.result).toBe("succeeded");
      const calls = vi.mocked(client[method] as (...args: unknown[]) => unknown).mock.calls;
      const options = calls[calls.length - 1][calls[calls.length - 1].length - 1] as { ifMatch?: number };
      expect(options.ifMatch).toBe(10);
    }
    for (const name of ["start_task", "send_message"]) {
      expect(Object.keys(named(name).inputSchema.properties as object)).not.toContain("ifMatch");
    }
    const before = vi.mocked(client.pauseImmediately).mock.calls.length;
    const refused = JSON.parse(await named("pause_execution").execute({ executionId: "demo", mode: "immediate", ifMatch: "10" }));
    expect(refused.ok).toBe(false);
    expect(vi.mocked(client.pauseImmediately).mock.calls.length).toBe(before);
  });

  /** A RETRY CARRIES THE KEY OF THE ATTEMPT IT RETRIES (PR #662 review, adapter.ts:439): an
   * immediate pause that came back `unknown` hands the agent its key in the evidence; calling
   * again with that key reaches the client with the SAME key, so the Runtime can recognize the
   * retry and the result is tied to the original attempt. Absent -> minted; malformed ->
   * refused, never replaced by a mint. start_task keeps its own mint (it mints the execution
   * id per call too, so a key alone cannot name the attempt it would retry). */
  it("reuses the returned idempotencyKey on a retry, mints one when absent, refuses a malformed one", async () => {
    let attempts = 0;
    const client = stubClient({
      pauseImmediately: vi.fn(async (_id: string, options: { idempotencyKey: string }) => {
        attempts += 1;
        return {
          ...evidence("pause"),
          idempotencyKey: options.idempotencyKey,
          result: attempts === 1 ? ("unknown" as const) : ("succeeded" as const),
        };
      }),
    });
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(client, hooks().value, { modelContext });
    const pause = registered.find((tool) => tool.name === "graphhelm_pause_execution")!;

    const first = JSON.parse(await pause.execute({ executionId: "demo", mode: "immediate" }));
    expect(first.result).toBe("unknown");
    const key: string = first.idempotencyKey;
    expect(key.length).toBeGreaterThan(0);

    const second = JSON.parse(await pause.execute({ executionId: "demo", mode: "immediate", idempotencyKey: key }));
    expect(second.result).toBe("succeeded");
    expect(second.idempotencyKey).toBe(key);
    const calls = vi.mocked(client.pauseImmediately).mock.calls as unknown as Array<[string, { idempotencyKey: string }]>;
    expect(calls[1][1].idempotencyKey).toBe(key);
    expect(calls[0][1].idempotencyKey).toBe(key);

    for (const malformed of ["", "k".repeat(65), 12]) {
      const refused = JSON.parse(await pause.execute({ executionId: "demo", mode: "immediate", idempotencyKey: malformed }));
      expect(refused.ok).toBe(false);
    }
    expect(vi.mocked(client.pauseImmediately).mock.calls.length).toBe(2);

    for (const name of ["approve_node", "resume_execution", "send_message", "cancel_execution", "sweep_execution", "amend_node_budget"]) {
      const tool = registered.find((candidate) => candidate.name === `graphhelm_${name}`)!;
      expect(Object.keys(tool.inputSchema.properties as object)).toContain("idempotencyKey");
    }
    const start = registered.find((tool) => tool.name === "graphhelm_start_task")!;
    expect(Object.keys(start.inputSchema.properties as object)).not.toContain("idempotencyKey");
  });

  it("falls back cleanly when the browser has no model context, and the page keeps working", () => {
    const result = registerStudioTools(stubClient(), hooks().value, { modelContext: null });
    expect(result.availability).toBe("unavailable");
    expect(result.names).toEqual([]);
    expect(() => result.unregister()).not.toThrow();
  });

  it("keeps the human Studio usable when one tool registration throws synchronously", () => {
    const failed = "graphhelm_get_attention";
    const registered: WebMcpToolDescriptor[] = [];
    const aborted: WebMcpToolDescriptor[] = [];
    const modelContext: ModelContextLike = {
      registerTool: (tool, options) => {
        if (tool.name === failed) throw new Error("unsupported descriptor");
        registered.push(tool);
        options?.signal?.addEventListener("abort", () => aborted.push(tool));
      },
    };

    const result = registerStudioTools(stubClient(), hooks().value, { modelContext });

    const successful = EXPECTED_TOOLS.filter((name) => name !== failed);
    expect(result.availability).toBe("available");
    expect(result.names).toEqual(successful);
    expect(registered.map((tool) => tool.name)).toEqual(successful);

    result.unregister();
    expect(aborted.map((tool) => tool.name)).toEqual(successful);
    expect(result.availability).toBe("unavailable");
    expect(result.names).toEqual([]);
  });

  it("reports only confirmed registrations when the host settles asynchronously", async () => {
    const rejected = "graphhelm_get_attention";
    const pending = "graphhelm_get_execution_status";
    const rejectedRegistration = Promise.reject(new Error("unsupported descriptor"));
    let confirmPending!: () => void;
    const pendingRegistration = new Promise<void>((resolve) => {
      confirmPending = resolve;
    });
    const modelContext: ModelContextLike = {
      registerTool: (tool) => {
        if (tool.name === rejected) return rejectedRegistration;
        if (tool.name === pending) return pendingRegistration;
        return undefined;
      },
    };

    const result = registerStudioTools(stubClient(), hooks().value, { modelContext });

    expect(result.ready).toBeInstanceOf(Promise);
    expect(result.names).toEqual(EXPECTED_TOOLS.filter((name) => name !== rejected && name !== pending));

    confirmPending();
    await result.ready;
    expect(result.availability).toBe("available");
    expect(result.names).toEqual(EXPECTED_TOOLS.filter((name) => name !== rejected));
  });

  it("removes its tools on disconnect and makes a leftover handle refuse", async () => {
    const { modelContext, registered, aborted } = fakeModelContext();
    const client = stubClient();
    const result = registerStudioTools(client, hooks().value, { modelContext });
    const listTool = registered.find((tool) => tool.name === "graphhelm_list_executions")!;

    result.unregister();

    expect(aborted.map((tool) => tool.name)).toEqual(EXPECTED_TOOLS);
    const reply = JSON.parse(await listTool.execute({}));
    expect(reply.ok).toBe(false);
    expect(client.listExecutions).not.toHaveBeenCalled();
    // Idempotent: a second unregister must not throw or re-abort.
    expect(() => result.unregister()).not.toThrow();
  });

  it("refuses a read that finishes after unregister without touching the next session", async () => {
    let resolveStatus!: (status: Awaited<ReturnType<RuntimeClient["getStatus"]>>) => void;
    const pendingStatus = new Promise<Awaited<ReturnType<RuntimeClient["getStatus"]>>>((resolve) => {
      resolveStatus = resolve;
    });
    const client = stubClient({ getStatus: vi.fn(() => pendingStatus) });
    const firstHooks = hooks();
    const nextHooks = hooks();
    let currentHooks = firstHooks.value;
    const forwardingHooks = {
      onSelect: (id: string) => currentHooks.onSelect(id),
      onMutation: (mutation: MutationEvidence) => currentHooks.onMutation(mutation),
      onActivity: (activity: ToolActivity) => currentHooks.onActivity(activity),
    };
    const firstContext = fakeModelContext();
    const firstRegistration = registerStudioTools(client, forwardingHooks, {
      modelContext: firstContext.modelContext,
    });
    const oldStatusTool = firstContext.registered.find(
      (tool) => tool.name === "graphhelm_get_execution_status",
    )!;

    const oldResult = oldStatusTool.execute({ executionId: "demo" });
    firstRegistration.unregister();
    currentHooks = nextHooks.value;
    const nextContext = fakeModelContext();
    registerStudioTools(client, nextHooks.value, { modelContext: nextContext.modelContext });
    resolveStatus({
      executionId: "demo",
      attention: "needs_you",
      attentionReasons: [],
      untriagedInterruptions: [],
      silenceUnevaluated: [],
      headSequence: 13,
      startedAt: null,
      lastEventAt: null,
      status: "running",
      mode: "supervised",
      nodeStateCounts: {},
      nodeLastEventAt: {},
    });

    expect(JSON.parse(await oldResult).ok).toBe(false);
    expect(nextHooks.selected).toEqual([]);
    expect(nextHooks.mutations).toEqual([]);
    expect(nextHooks.activity).toEqual([]);
  });

  it("refuses a write that finishes after unregister without completing in either session", async () => {
    let resolvePause!: (result: Awaited<ReturnType<RuntimeClient["pause"]>>) => void;
    const pendingPause = new Promise<Awaited<ReturnType<RuntimeClient["pause"]>>>((resolve) => {
      resolvePause = resolve;
    });
    const client = stubClient({ pause: vi.fn(() => pendingPause) });
    const firstHooks = hooks();
    const nextHooks = hooks();
    let currentHooks = firstHooks.value;
    const forwardingHooks = {
      onSelect: (id: string) => currentHooks.onSelect(id),
      onMutation: (mutation: MutationEvidence) => currentHooks.onMutation(mutation),
      onActivity: (activity: ToolActivity) => currentHooks.onActivity(activity),
    };
    const firstContext = fakeModelContext();
    const firstRegistration = registerStudioTools(client, forwardingHooks, {
      modelContext: firstContext.modelContext,
    });
    const oldPauseTool = firstContext.registered.find((tool) => tool.name === "graphhelm_pause_execution")!;

    const oldResult = oldPauseTool.execute({ executionId: "demo" });
    firstRegistration.unregister();
    currentHooks = nextHooks.value;
    const nextContext = fakeModelContext();
    registerStudioTools(client, nextHooks.value, { modelContext: nextContext.modelContext });
    resolvePause(evidence("pause"));

    expect(JSON.parse(await oldResult).ok).toBe(false);
    expect(firstHooks.mutations).toEqual([]);
    expect(firstHooks.activity).toEqual([]);
    expect(nextHooks.mutations).toEqual([]);
    expect(nextHooks.activity).toEqual([]);
  });

  it("refuses every tool call once the client itself is disconnected", async () => {
    const { modelContext, registered } = fakeModelContext();
    const client = stubClient({ connected: false });
    registerStudioTools(client, hooks().value, { modelContext });
    for (const tool of registered) {
      const reply = JSON.parse(await tool.execute({ executionId: "demo", node: "n", file: "f" }));
      expect(reply.ok).toBe(false);
    }
    expect(client.getStatus).not.toHaveBeenCalled();
    expect(client.pause).not.toHaveBeenCalled();
  });
});

describe("tool behaviour", () => {
  const toolsOf = (client: RuntimeClient, hook: ReturnType<typeof hooks>) => {
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(client, hook.value, { modelContext });
    return new Map(registered.map((tool) => [tool.name, tool]));
  };

  it("routes every read through the client and never through a second request path", async () => {
    const client = stubClient();
    const hook = hooks();
    const tools = toolsOf(client, hook);

    await tools.get("graphhelm_list_executions")!.execute({ limit: 5 });
    await tools.get("graphhelm_get_attention")!.execute({ executionId: "demo" });
    await tools.get("graphhelm_get_execution_status")!.execute({ executionId: "demo" });
    await tools.get("graphhelm_get_execution_events")!.execute({ executionId: "demo", after: 3, limit: 10 });

    expect(client.listExecutions).toHaveBeenCalledWith({ after: undefined, limit: 5 });
    expect(client.getEvents).toHaveBeenCalledWith("demo", { after: 3, limit: 10 });
    expect(client.getStatus).toHaveBeenCalledTimes(2);
  });

  it("brings the execution a tool addressed into the page's view", async () => {
    const hook = hooks();
    const tools = toolsOf(stubClient(), hook);
    await tools.get("graphhelm_get_attention")!.execute({ executionId: "demo" });
    expect(hook.selected).toEqual(["demo"]);
  });

  /** The two pauses route to two different client methods - the interrupting one is never
   * reachable by default or by typo, only by the exact word. */
  it("routes graceful and immediate pause to their own verbs, and refuses a third word", async () => {
    const client = stubClient();
    const tools = toolsOf(client, hooks());

    await tools.get("graphhelm_pause_execution")!.execute({ executionId: "demo" });
    expect(client.pause).toHaveBeenCalledTimes(1);
    expect(client.pauseImmediately).not.toHaveBeenCalled();

    await tools.get("graphhelm_pause_execution")!.execute({ executionId: "demo", mode: "graceful" });
    expect(client.pause).toHaveBeenCalledTimes(2);

    await tools.get("graphhelm_pause_execution")!.execute({ executionId: "demo", mode: "immediate" });
    expect(client.pauseImmediately).toHaveBeenCalledTimes(1);

    const reply = JSON.parse(
      await tools.get("graphhelm_pause_execution")!.execute({ executionId: "demo", mode: "now" }),
    );
    expect(reply.ok).toBe(false);
    expect(client.pauseImmediately).toHaveBeenCalledTimes(1);
  });

  it("routes cancel, sweep and amend-budget through the client, inputs validated first", async () => {
    const client = stubClient();
    const tools = toolsOf(client, hooks());

    await tools.get("graphhelm_cancel_execution")!.execute({ executionId: "demo" });
    expect(client.cancel).toHaveBeenCalledWith("demo", expect.anything());

    await tools.get("graphhelm_sweep_execution")!.execute({ executionId: "demo" });
    expect(client.sweep).toHaveBeenCalledWith("demo", expect.anything());

    await tools.get("graphhelm_amend_node_budget")!.execute({
      executionId: "demo",
      node: "judge",
      seconds: 900,
      computedAtSequence: 41,
    });
    expect(client.amendBudget).toHaveBeenCalledWith(
      "demo",
      { node: "judge", seconds: 900, computedAtSequence: 41 },
      expect.anything(),
    );

    const refused = JSON.parse(
      await tools.get("graphhelm_amend_node_budget")!.execute({ executionId: "demo", node: "judge", seconds: 0, computedAtSequence: 41 }),
    );
    expect(refused.ok).toBe(false);
    expect(client.amendBudget).toHaveBeenCalledTimes(1);
  });

  /** INVALID INPUT IS A REFUSAL, NEVER ANOTHER OPERATION (PR #662 review, two threads of one
   * class): an empty `asOf` used to be dropped and became "sweep now" - a different verb that
   * journals and can spend overdue-exception episodes. */
  it("refuses an empty or unparseable asOf instead of sweeping now", async () => {
    const client = stubClient();
    const tools = toolsOf(client, hooks());
    for (const asOf of ["", "yesterday", "2026-09-02T10:00:00+01:00", "2026-02-30T10:00:00Z", "2026-09-02T99:99:99Z"]) {
      const reply = JSON.parse(await tools.get("graphhelm_sweep_execution")!.execute({ executionId: "demo", asOf }));
      expect(reply.ok, asOf).toBe(false);
      expect(reply.error).toMatch(/Z-terminated/);
    }
    expect(client.sweep).not.toHaveBeenCalled();
    await tools.get("graphhelm_sweep_execution")!.execute({ executionId: "demo", asOf: "2026-12-31T23:59:60Z" });
    expect(client.sweep).toHaveBeenCalledWith("demo", expect.objectContaining({ asOf: "2026-12-31T23:59:60Z" }));
    // The tool's constraint IS the schema's pattern, imported - not a hand-rolled format word.
    const schema = tools.get("graphhelm_sweep_execution")!.inputSchema.properties as Record<string, { pattern?: string; format?: string }>;
    expect(schema.asOf.pattern).toBe(TIMESTAMP_PATTERN);
    expect(schema.asOf.format).toBeUndefined();
  });

  /** The seconds bound is the envelope schema's own (imported, not copied): one above it is
   * refused HERE, with zero calls, instead of becoming the store's refusal one hop later. */
  it("bounds seconds by the envelope schema - one over refuses locally, the maximum passes", async () => {
    const client = stubClient();
    const tools = toolsOf(client, hooks());
    const over = JSON.parse(
      await tools.get("graphhelm_amend_node_budget")!.execute({ executionId: "demo", node: "judge", seconds: MAX_NODE_TIMEOUT_SECONDS + 1, computedAtSequence: 41 }),
    );
    expect(over.ok).toBe(false);
    expect(client.amendBudget).not.toHaveBeenCalled();
    await tools.get("graphhelm_amend_node_budget")!.execute({ executionId: "demo", node: "judge", seconds: MAX_NODE_TIMEOUT_SECONDS, computedAtSequence: 41 });
    expect(client.amendBudget).toHaveBeenCalledWith("demo", { node: "judge", seconds: MAX_NODE_TIMEOUT_SECONDS, computedAtSequence: 41 }, expect.anything());
    const schema = tools.get("graphhelm_amend_node_budget")!.inputSchema.properties as Record<string, { maximum?: number }>;
    expect(schema.seconds.maximum).toBe(MAX_NODE_TIMEOUT_SECONDS);
    expect(MAX_NODE_TIMEOUT_SECONDS).toBe(315576000);
  });

  /** The attention tool serves the SAME unanswered-question ledger the page's banner shows
   * (`graph/ledger.ts`): the agent learns WHAT the run is waiting to hear, and the signalId its
   * answer's replyTo must cite to settle the debt. */
  it("serves the unanswered questions behind the verdict, each with its signalId", async () => {
    const client = stubClient({
      getEvents: vi.fn(async () => ({
        head: 5,
        events: [
          {
            sequence: 5,
            kind: "signal_recorded",
            payload: { signalId: "sig-5" },
            occurredAt: "2026-08-31T09:00:05Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k5",
            eventId: "event-5",
            evidenceRefs: ["ev-5"],
          },
        ],
      })),
      readEvidence: vi.fn(async () => ({
        evidenceId: "ev-5",
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        content: JSON.stringify({ description: "which region?", to: "studio-operator" }),
      })),
    });
    const tools = toolsOf(client, hooks());
    const reply = JSON.parse(await tools.get("graphhelm_get_attention")!.execute({ executionId: "demo" }));
    expect(reply.openQuestions).toEqual([
      { asker: "codex", at: "2026-08-31T09:00:05Z", text: "which region?", signalId: "sig-5" },
    ]);
  });

  it("degrades to an empty question list when the log cannot be read, never a failed verdict", async () => {
    const client = stubClient({
      getEvents: vi.fn(async () => {
        throw new Error("boom");
      }),
    });
    const tools = toolsOf(client, hooks());
    const reply = JSON.parse(await tools.get("graphhelm_get_attention")!.execute({ executionId: "demo" }));
    expect(reply.attention).toBe("needs_you");
    expect(reply.openQuestions).toEqual([]);
  });

  it("returns the full verification evidence from a write, not a bare acknowledgement", async () => {
    const hook = hooks();
    const tools = toolsOf(stubClient(), hook);
    const reply = JSON.parse(await tools.get("graphhelm_approve_node")!.execute({ executionId: "demo", node: "implementation" }));
    expect(reply.action).toBe("approve");
    expect(reply.result).toBe("succeeded");
    expect(reply.headBefore).toBe(13);
    expect(reply.headAfter).toBe(15);
    expect(reply.actor).toEqual({ id: "studio-webmcp-adapter", type: "agent" });
    expect(hook.mutations).toHaveLength(1);
  });

  it("does not report an unverified write as ok", async () => {
    const unknownEvidence = { ...evidence("pause"), result: "unknown" as const, headAfter: null };
    const hook = hooks();
    const tools = toolsOf(stubClient({ pause: vi.fn(async () => unknownEvidence) }), hook);
    const reply = JSON.parse(await tools.get("graphhelm_pause_execution")!.execute({ executionId: "demo" }));
    expect(reply.result).toBe("unknown");
    expect(hook.activity.at(-1)?.outcome).toBe("unknown");
  });

  /** This guard was born when the schema had no `mode` and a smuggled "immediate" had to die
   * before the client. Phase 2 (#105) makes `mode` a declared, enum-closed field that routes to
   * a DIFFERENT client method - so what the guard defends now is the seam it always defended:
   * the word never rides into `client.pause`'s options (the graceful verb cannot be steered),
   * and the interrupting verb is reached only through its own method. */
  it("keeps mode out of the graceful verb's options - the two pauses stay separate methods", async () => {
    const client = stubClient();
    const tools = toolsOf(client, hooks());
    await tools.get("graphhelm_pause_execution")!.execute({ executionId: "demo", mode: "immediate" });
    expect(client.pause).not.toHaveBeenCalled();
    const options = vi.mocked(client.pauseImmediately).mock.calls[0]?.[1];
    expect(options).toEqual(expect.objectContaining({ actor: { id: "studio-webmcp-adapter", type: "agent" } }));
    expect(options).not.toHaveProperty("mode");

    await tools.get("graphhelm_pause_execution")!.execute({ executionId: "demo", mode: "graceful" });
    const graceful = vi.mocked(client.pause).mock.calls[0]?.[1];
    expect(graceful).not.toHaveProperty("mode");
  });

  it("refuses a malformed executionId with a named error instead of calling the client", async () => {
    const client = stubClient();
    const hook = hooks();
    const tools = toolsOf(client, hook);
    const reply = JSON.parse(await tools.get("graphhelm_get_attention")!.execute({ executionId: "" }));
    expect(reply.ok).toBe(false);
    expect(reply.error).toContain("executionId");
    expect(client.getStatus).not.toHaveBeenCalled();
  });

  it("replaces an unexpected error's text rather than relaying it to the model", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => {
        throw new Error("ENOENT: open C:/Users/someone/.graphhelm/secret.token");
      }),
    });
    const tools = toolsOf(client, hooks());
    const reply = JSON.parse(await tools.get("graphhelm_get_execution_status")!.execute({ executionId: "demo" }));
    expect(reply.ok).toBe(false);
    expect(reply.error).toBe("The Studio could not complete this tool call.");
    expect(JSON.stringify(reply)).not.toContain("secret.token");
  });
});

/**
 * The exchange, reachable from the browser's own agent.
 *
 * These three tools are what makes the Studio a place where a person and their agent work
 * TOGETHER rather than a dashboard the agent can only look at: the agent can open a task, say
 * something into its log, and read back the sealed words a person or another agent left. Without
 * them, everything the human interface can do about a conversation is invisible to the agent
 * standing next to the human.
 */
describe("the exchange tools", () => {
  const toolsOf = (client: ReturnType<typeof stubClient>, hook: ReturnType<typeof hooks> = hooks()) => {
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(client, hook.value, { modelContext });
    return new Map(registered.map((tool) => [tool.name, tool]));
  };

  it("starts a task through the same one-node draft the human composer publishes", async () => {
    const client = stubClient();
    const hook = hooks();
    const tools = toolsOf(client, hook);

    const reply = JSON.parse(
      await tools.get("graphhelm_start_task")!.execute({ objective: "Cache the player-leagues endpoint" }),
    );

    expect(reply.action).toBe("start");
    const [executionId, graph, options] = vi.mocked(client.startTask).mock.calls[0]!;
    // The id is minted, never caller-supplied: two agents naming their own ids is how two tabs
    // collide on one stream.
    expect(executionId).toMatch(/^run-/);
    // VERBATIM. The objective is the person-visible statement of what this task is; a tool that
    // paraphrases or wraps it puts words in the agent's mouth on the human's board.
    const node = (graph as { spec: { nodes: Record<string, { objective: string }> } }).spec.nodes.start;
    expect(node.objective).toBe("Cache the player-leagues endpoint");
    expect(options).toEqual(
      expect.objectContaining({ actor: { id: "studio-webmcp-adapter", type: "agent" } }),
    );
    expect(options).not.toHaveProperty("route");
    expect(hook.selected).toEqual([executionId]);
    expect(hook.mutations).toHaveLength(1);
  });

  /** Order is load-bearing, and it crashed a real page before it was pinned (2026-08-30). The
   * Runtime answers an id it has never seen with an EMPTY projection (200, status null), so
   * selecting the new id BEFORE starting it put the page on a shape its renderer had never met.
   * The select must follow the start: by then the execution exists and has a status. */
  it("selects the new task only after the start was sent, never before", async () => {
    const hook = hooks();
    // Recorded at call time, ASSERTED OUTSIDE the tool body. An `expect` inside the stub is
    // swallowed by the adapter's own `guarded` wrapper - it converts the throw into a polite
    // refusal and the test stays green over the exact defect it exists to catch (measured on
    // this test's own first version).
    let selectedWhenStartWasCalled = -1;
    const client = stubClient({
      startTask: vi.fn(async () => {
        selectedWhenStartWasCalled = hook.selected.length;
        return evidence("start");
      }),
    });
    const tools = toolsOf(client, hook);
    await tools.get("graphhelm_start_task")!.execute({ objective: "do the thing" });
    expect(client.startTask).toHaveBeenCalled();
    expect(selectedWhenStartWasCalled).toBe(0);
    expect(hook.selected).toHaveLength(1);
  });

  it("forwards a route only when the agent actually chose one", async () => {
    const client = stubClient();
    const tools = toolsOf(client);
    await tools.get("graphhelm_start_task")!.execute({ objective: "do the thing", route: "sonnet" });
    const options = vi.mocked(client.startTask).mock.calls[0]?.[2];
    expect(options).toEqual(expect.objectContaining({ route: "sonnet" }));
  });

  it("refuses an empty objective with a named error instead of calling the client", async () => {
    const client = stubClient();
    const reply = JSON.parse(await toolsOf(client).get("graphhelm_start_task")!.execute({ objective: "   " }));
    expect(reply.ok).toBe(false);
    expect(reply.error).toContain("objective");
    expect(client.startTask).not.toHaveBeenCalled();
  });

  it("sends a message verbatim through the client's own signal path", async () => {
    const client = stubClient();
    const hook = hooks();
    const tools = toolsOf(client, hook);

    const reply = JSON.parse(
      await tools.get("graphhelm_send_message")!.execute({
        executionId: "demo",
        message: "the migration needs a decision before I go further",
      }),
    );

    expect(reply.action).toBe("signal");
    const [executionId, message, options] = vi.mocked(client.signal).mock.calls[0]!;
    expect(executionId).toBe("demo");
    expect(message).toBe("the migration needs a decision before I go further");
    expect(options).toEqual(
      expect.objectContaining({ actor: { id: "studio-webmcp-adapter", type: "agent" } }),
    );
    expect(hook.selected).toEqual(["demo"]);
    expect(hook.mutations).toHaveLength(1);
  });

  /** The addressing that makes a group thread a thread. Forwarded verbatim when given; when not
   * given, the options carry NO addressing keys at all - `to: undefined` still reads as "the
   * caller addressed someone" to anything that iterates keys. */
  it("forwards addressing to the client only when the agent addressed someone", async () => {
    const client = stubClient();
    const tools = toolsOf(client);
    await tools.get("graphhelm_send_message")!.execute({
      executionId: "demo",
      message: "concordo",
      to: "claude-code",
      replyTo: "signal-15",
    });
    const addressed = vi.mocked(client.signal).mock.calls[0]?.[2] as Record<string, unknown>;
    expect(addressed.to).toBe("claude-code");
    expect(addressed.replyTo).toBe("signal-15");

    await tools.get("graphhelm_send_message")!.execute({ executionId: "demo", message: "para a sala" });
    const plain = vi.mocked(client.signal).mock.calls[1]?.[2] as Record<string, unknown>;
    expect(Object.keys(plain)).not.toContain("to");
    expect(Object.keys(plain)).not.toContain("replyTo");
  });

  it("refuses an empty message without calling the client", async () => {
    const client = stubClient();
    const reply = JSON.parse(
      await toolsOf(client).get("graphhelm_send_message")!.execute({ executionId: "demo", message: "" }),
    );
    expect(reply.ok).toBe(false);
    expect(reply.error).toContain("message");
    expect(client.signal).not.toHaveBeenCalled();
  });

  it("opens sealed evidence through the client and returns the content as data", async () => {
    const client = stubClient();
    const hook = hooks();
    const tools = toolsOf(client, hook);

    const reply = JSON.parse(
      await tools.get("graphhelm_read_evidence")!.execute({ executionId: "demo", evidenceId: "ev-1" }),
    );

    expect(client.readEvidence).toHaveBeenCalledWith("demo", "ev-1");
    expect(reply.evidenceId).toBe("ev-1");
    expect(reply.content).toContain("the words that were sealed");
    expect(hook.selected).toEqual(["demo"]);
  });

  it("refuses a malformed evidenceId without calling the client", async () => {
    const client = stubClient();
    const reply = JSON.parse(
      await toolsOf(client).get("graphhelm_read_evidence")!.execute({ executionId: "demo", evidenceId: "" }),
    );
    expect(reply.ok).toBe(false);
    expect(reply.error).toContain("evidenceId");
    expect(client.readEvidence).not.toHaveBeenCalled();
  });

  /** Sealed content is free-form text written by whoever sealed it - a model reply, a person's
   * message. To the agent reading it, that is DATA and potentially adversarial, never
   * instructions; the annotation is how the browser knows to treat it that way. */
  it("marks read_evidence as carrying untrusted content", () => {
    const { modelContext, registered } = fakeModelContext();
    registerStudioTools(stubClient(), hooks().value, { modelContext });
    const read = registered.find((tool) => tool.name === "graphhelm_read_evidence")!;
    expect(read.annotations?.untrustedContentHint).toBe(true);
    expect(read.description).toMatch(/data.*not.*instruction|never.*instruction/i);
  });
});
