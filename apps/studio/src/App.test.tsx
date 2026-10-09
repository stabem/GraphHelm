/**
 * Page-level guards.
 *
 * These drive the real component with a stubbed `RuntimeClient`, so what they observe is what an
 * operator observes: the session that arrives without anyone typing, the projects rail, the board,
 * the window that opens on a node, and — the claim this whole feature rests on — that a WebMCP
 * tool call moves the HUMAN view, not just the agent's transcript.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { scaled } from "./test/time-scale";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Profiler } from "react";
import { fastUserEvent } from "./test/user-event";
// Its own instance: see the helper for why this is not a shared const.
const userEvent = fastUserEvent();

import App from "./App";
import { saveProjectName, saveRemovedRuns } from "./studio-preferences";
import { resetPanelCaches } from "./components/panel";
import type { RuntimeClient } from "./runtime/client";
import { MAX_NODE_TIMEOUT_SECONDS, RuntimeError, RuntimeClient as ReadClient, RUNTIME_REQUEST_TIMEOUT_MS } from "./runtime/client";
import type { GraphTopology, MutationEvidence } from "./runtime/types";
import type { ModelContextLike, WebMcpToolDescriptor } from "./webmcp/adapter";
import { digestOf } from "./runtime/customs";

const STATUS = {
  executionId: "demo-deploy",
  mode: "supervised",
  status: "running",
  attention: "needs_you",
  attentionReasons: [{ kind: "blocked_node", node: "implementation" }],
  nodeStateCounts: { blocked: 1, ready: 1 },
  untriagedInterruptions: [],
  silenceUnevaluated: [],
  startedAt: "2026-08-27T12:00:00Z",
  lastEventAt: "2026-08-27T12:01:00Z",
  nodeLastEventAt: {},
  headSequence: 13,
};

const PAUSED_EVIDENCE: MutationEvidence = {
  action: "pause",
  executionId: "demo-deploy",
  node: null,
  actor: { id: "studio-webmcp-adapter", type: "agent" },
  idempotencyKey: "key-from-the-agent",
  headBefore: 13,
  headAfter: 14,
  result: "succeeded",
  statusAfter: { ...STATUS, status: "paused", headSequence: 14 },
  newEvents: [],
  diagnostics: [],
};

function stubClient(overrides: Record<string, unknown> = {}) {
  return {
    connected: true,
    dispose: vi.fn(),
    health: vi.fn(async () => undefined),
    listExecutions: vi.fn(async () => ({
      executions: [
        {
          executionId: "demo-calm",
          mode: "supervised",
          status: "running",
          attention: "can_sleep",
          startedAt: null,
          lastEventAt: null,
          headSequence: 4,
        },
        {
          executionId: "demo-deploy",
          mode: "supervised",
          status: "running",
          attention: "needs_you",
          startedAt: null,
          lastEventAt: null,
          headSequence: 13,
        },
      ],
      hasMore: false,
      nextCursor: null,
    })),
    getStatus: vi.fn(async (executionId: string) => ({ ...STATUS, executionId })),
    getEvents: vi.fn(async () => ({
      head: 13,
      events: [
        // The roster declaration comes first, exactly as a real stream carries it. Without it the
        // board can only show nodes something happened to.
        {
          sequence: 2,
          kind: "execution_form_declared",
          payload: { executionId: "demo-deploy", nodeIds: ["deploy", "implementation"] },
          occurredAt: "2026-08-27T12:00:30Z",
          actorId: "system-cli",
          actorType: "system",
          idempotencyKey: "k0",
          eventId: "event-2",
          evidenceRefs: [],
        },
        {
          sequence: 13,
          kind: "node_outcome_recorded",
          payload: { nodeId: "implementation", outcome: "retryable_failure", nextState: "blocked" },
          occurredAt: "2026-08-27T12:01:00Z",
          actorId: "system-cli",
          actorType: "system",
          idempotencyKey: "k",
          eventId: "event-13",
          evidenceRefs: [],
        },
      ],
    })),
    getTopology: vi.fn(async () => ({
      graphId: "g",
      graphVersion: 1,
      executionId: "demo-deploy",
      semanticHash: "sha256:whatever",
      entrypoints: [],
      nodes: [],
      edges: [],
    })),
    pause: vi.fn(async () => PAUSED_EVIDENCE),
    approve: vi.fn(async () => ({
      ...PAUSED_EVIDENCE,
      action: "approve" as const,
      node: "implementation",
    })),
    resume: vi.fn(async () => ({ ...PAUSED_EVIDENCE, action: "resume" as const })),
    pauseImmediately: vi.fn(async () => PAUSED_EVIDENCE),
    cancel: vi.fn(async () => ({ ...PAUSED_EVIDENCE, action: "cancel" as const })),
    sweep: vi.fn(async () => ({ ...PAUSED_EVIDENCE, action: "sweep" as const })),
    amendBudget: vi.fn(async () => ({ ...PAUSED_EVIDENCE, action: "amendBudget" as const, node: "judge" })),
    // In the BASE stub, not only in the overrides that use them: a method a test reaches for
    // through `client.startTask` has to exist on the returned type, and one that appears only
    // when overridden does not.
    listRoutes: vi.fn(async () => ({ configured: true, routes: [] })),
    getReplySuggestions: vi.fn(async () => null),
    setRoute: vi.fn(async () => ({ routes: [] })),
    setCredential: vi.fn(async () => ({ id: "secret_route", routes: [] })),
    probeRoute: vi.fn(async () => ({ health: "available" })),
    // `null` is what an older Runtime without the briefing route yields: the base stub is
    // the id-only world, and the tests that name a run by its objective override it.
    getBriefing: vi.fn(async () => null),
    startTask: vi.fn(
      async (
        _executionId: string,
        _graph: Record<string, unknown>,
        _options: { route?: string | null },
      ) => ({ ...PAUSED_EVIDENCE, action: "start" as const }),
    ),
    readEvidence: vi.fn(async () => ({
      evidenceId: "ev-1",
      mediaType: "application/json",
      sensitivity: "confidential",
      contentSha256: "sha256:whatever",
      content: "{\"text\":\"the model did the thing\"}",
    })),
    readDocument: vi.fn(async () => ({ content: "Original rule", contentSha256: "old" })),
    saveDocument: vi.fn(async () => ({
      contentSha256: "new",
      notification: { status: "recorded" as const, notifiedRuns: [], pendingRuns: [] },
    })),
    signal: vi.fn(
      async (_executionId: string, _message: string, _options?: Record<string, unknown>) => ({
        ...PAUSED_EVIDENCE,
        action: "signal" as const,
      }),
    ),
    ...overrides,
  };
}

/** The arguments a mock was called with the first time, or a failure that says so. Reaching into
 * `mock.calls[0]` directly types as possibly-undefined and, when the call never happened, fails
 * with an index error rather than with the thing the test was actually checking. */
function firstCall<T extends unknown[]>(mock: { mock: { calls: T[] } }): T {
  const call = mock.mock.calls[0];
  if (call === undefined) throw new Error("expected the mock to have been called");
  return call;
}

/** The run panel's own Send button: the chat column carries a second one with the same name. */
function runSend(): HTMLElement {
  return within(document.getElementById("conversation-panel") as HTMLElement).getByRole("button", { name: /^send$/i });
}

function fakeModelContext() {
  const registered: WebMcpToolDescriptor[] = [];
  const modelContext: ModelContextLike = {
    registerTool: (tool) => {
      registered.push(tool);
      return undefined;
    },
  };
  return { modelContext, registered };
}

// Module state surviving a remount is the feature; surviving into the NEXT TEST is pollution.
beforeEach(() => resetPanelCaches());
// #583: Graph is the default canvas tab. These suites were written against the Team canvas, so each
// test starts as if the owner had chosen Team this session; the Graph default has its own test.
beforeEach(() => sessionStorage.setItem("graphhelm.studio.canvas-tab", "team"));

describe("Studio organization and responsive navigation", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => localStorage.clear());

  // DOM-only, no native calls: protects the entry point missing from the owner journey.
  it("opens the principal conversation inside the chat column beside the team canvas", async () => {
    render(<App createClient={() => stubClient() as unknown as RuntimeClient} modelContext={null} session={async () => ({ token: "local-token", project: "GraphHelm", projectPath: "fixtures/project" })} />);
    const team = await screen.findByRole("region", { name: "Team" });
    const principal = screen.getByRole("complementary", { name: "Principal conversation" });
    expect(principal).toBeVisible();
    expect(screen.getByRole("complementary", { name: "Chat" })).toContainElement(principal);
    expect(principal.compareDocumentPosition(team) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Free canvas" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Overview" })).toBeNull();
  });

  it("keeps the selected project and run address visible outside the mobile rail without guessing membership", async () => {
    await open(stubClient());
    const identity = screen.getByLabelText("Selected run identity");
    expect(identity).toHaveTextContent("Project: dale-api-base");
    expect(identity).toHaveTextContent("Run: demo-deploy");
    expect(identity).not.toHaveTextContent("Joined sessions");
    await userEvent.click(within(screen.getByRole("navigation", { name: "Projects" })).getByText("demo-calm", { exact: true }).closest("button")!);
    expect(identity).toHaveTextContent("Run: demo-calm");
    expect(identity).not.toHaveTextContent("Run: demo-deploy");
  });


  it.each([
    [false, "demo-deploy"],
    [false, "demo-calm"],
    [true, "demo-deploy"],
    [true, "demo-calm"],
  ])("closes mobile projects navigation after choosing %s / %s while preserving desktop navigation", async (wide, run) => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: wide, media: query }));
    try {
      await open(stubClient(), null, false);
      const toggle = screen.getByRole("button", { name: "Toggle projects" });
      if (!wide) await userEvent.click(toggle);
      expect(toggle).toHaveAttribute("aria-expanded", "true");
      await userEvent.click(within(screen.getByRole("navigation", { name: "Projects" })).getByText(run, { exact: true }));
      expect(toggle).toHaveAttribute("aria-expanded", wide ? "true" : "false");
      expect(screen.getByRole("button", { name: "Toggle conversation" })).toHaveAttribute("aria-expanded", "false");
    } finally {
      cleanup();
      vi.unstubAllGlobals();
    }
  });

  it.each([
    ["needs_you", "Needs you"],
    ["can_sleep", "Can sleep"],
  ])("names lifecycle and %s verdict in the run header", async (attention, label) => {
    const client = stubClient({ getStatus: vi.fn(async () => ({ ...STATUS, attention })) });
    await open(client);
    const strip = document.querySelector(".topbar") as HTMLElement;
    await userEvent.click(within(strip).getByText("Run details"));
    expect(await within(strip).findByText(`running · ${label}`)).toBeVisible();
  });

  it("does not automatically open a removed run after reconnecting", async () => {
    saveRemovedRuns(`${location.origin}:dale-api-base`, ["demo-deploy"]);
    const client = stubClient();
    await open(client);
    await waitFor(() => expect(client.getStatus).toHaveBeenCalledWith("demo-calm"));
    expect(client.getStatus).not.toHaveBeenCalledWith("demo-deploy");
  });

  it.each(["local-token", "another-token"])("scopes preferences on manual reconnect with %s", async (token) => {
    saveProjectName(`${location.origin}:dale-api-base`, "My workspace");
    saveRemovedRuns(`${location.origin}:dale-api-base`, ["demo-deploy"]);
    const first = stubClient();
    const second = stubClient();
    const createClient = vi.fn().mockReturnValueOnce(first).mockReturnValueOnce(second);
    render(<App createClient={createClient} modelContext={null} session={async () => ({ token: "local-token", project: "dale-api-base" })} />);
    await screen.findByRole("button", { name: "Rename My workspace" });
    fireEvent.click(screen.getByRole("button", { name: /^disconnect$/i }), { detail: 1 });
    fireEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }), { detail: 1 });
    await userEvent.type(await screen.findByLabelText(/bearer token/i), token);
    await userEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    await screen.findByRole("navigation", { name: "Projects" });
    if (token === "local-token") {
      expect(screen.getByRole("button", { name: "Rename My workspace" })).toBeVisible();
      await waitFor(() => expect(second.getStatus).toHaveBeenCalledWith("demo-calm"));
      expect(second.getStatus).not.toHaveBeenCalledWith("demo-deploy");
    } else {
      expect(screen.getByRole("button", { name: "Rename this runtime" })).toBeVisible();
      await waitFor(() => expect(second.getStatus).toHaveBeenCalledWith("demo-deploy"));
    }
  });

  it("clears busy when the selected run is removed during a pending action", async () => {
    let finish!: (value: MutationEvidence) => void;
    const pause = vi.fn(() => new Promise<MutationEvidence>((resolve) => { finish = resolve; }));
    const client = stubClient({ pause });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: "pause · finish in-flight" }));
    await waitFor(() => expect(pause).toHaveBeenCalled());
    await userEvent.click(screen.getByRole("button", { name: "Remove demo-deploy from this browser's list" }));
    await userEvent.click(screen.getByRole("button", { name: /^Remove$/ }));
    const newTask = screen.getByRole("button", { name: "New task in dale-api-base" });
    expect(newTask).toBeEnabled();
    await userEvent.click(newTask);
    finish(PAUSED_EVIDENCE);
    expect(await screen.findByLabelText("What should this task do?")).toBeVisible();
    expect(screen.getByLabelText("What should this task do?")).toBeEnabled();
  });

  it("loads a removed run from a later page before restoring its row", async () => {
    saveRemovedRuns(`${location.origin}:dale-api-base`, ["later-run"]);
    const row = { executionId: "later-run", mode: "supervised", status: "running", attention: "can_sleep", startedAt: null, lastEventAt: null, headSequence: 3 };
    const listExecutions = vi.fn(async (options?: { after?: string }) => options?.after === "last" ? {
      executions: [row, { ...row, executionId: "target-page-neighbor" }], hasMore: false, nextCursor: null,
    } : options?.after === "next" ? {
      executions: [{ ...row, executionId: "intermediate-run" }], hasMore: true, nextCursor: "last",
    } : {
      executions: [{ ...row, executionId: "demo-deploy", attention: "needs_you" }], hasMore: true, nextCursor: "next",
    });
    const client = stubClient({ listExecutions });
    const props = {
      createClient: () => client as unknown as RuntimeClient,
      modelContext: null,
      session: async () => ({ token: "local-token", project: "dale-api-base" }),
    };
    const view = render(<App {...props} pollIntervalMs={60_000} />);
    await screen.findByRole("navigation", { name: "Projects" });
    await userEvent.click(screen.getByRole("button", { name: "restore later-run" }));
    const rail = screen.getByRole("navigation", { name: "Projects" });
    expect(await within(rail).findByRole("button", { name: /^later-run/ })).toBeVisible();
    expect(screen.queryByRole("button", { name: "restore later-run" })).not.toBeInTheDocument();
    expect(listExecutions).toHaveBeenCalledWith(expect.objectContaining({ after: "last" }));
    expect(within(rail).queryByRole("button", { name: /^intermediate-run/ })).not.toBeInTheDocument();
    expect(within(rail).queryByRole("button", { name: /^target-page-neighbor/ })).not.toBeInTheDocument();
    const callsBeforePause = listExecutions.mock.calls.length;
    const pause = screen.getByRole("button", { name: "pause · finish in-flight" });
    await userEvent.click(pause);
    await waitFor(() => expect(client.pause).toHaveBeenCalled());
    await waitFor(() => expect(listExecutions.mock.calls.length).toBeGreaterThan(callsBeforePause));
    // The acknowledged pause at head 14 must not be undone by the stub's older head-13 read.
    // Refresh observes that the action finished; pause correctly stays illegal on a paused run.
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled());
    expect(pause).toBeDisabled();
    expect(within(rail).getAllByRole("button", { name: /^later-run/ })).toHaveLength(1);
    listExecutions.mockClear();
    view.rerender(<App {...props} pollIntervalMs={10} />);
    await waitFor(() => expect(listExecutions.mock.calls.length).toBeGreaterThanOrEqual(3));
    view.rerender(<App {...props} pollIntervalMs={60_000} />);
    expect(listExecutions.mock.calls.every(([options]) => options?.after === undefined)).toBe(true);
    expect(within(rail).queryByRole("button", { name: /^intermediate-run/ })).not.toBeInTheDocument();
    expect(within(rail).getAllByRole("button", { name: /^later-run/ })).toHaveLength(1);
    await userEvent.click(within(rail).getByRole("button", { name: /show more/i }));
    expect(await within(rail).findByRole("button", { name: /^intermediate-run/ })).toBeVisible();
    await userEvent.click(within(rail).getByRole("button", { name: /show more/i }));
    expect(await within(rail).findByRole("button", { name: /^target-page-neighbor/ })).toBeVisible();
    expect(within(rail).getAllByRole("button", { name: /^later-run/ })).toHaveLength(1);
  });

  it("keeps the restore entry when a missing run cannot be loaded", async () => {
    saveRemovedRuns(`${location.origin}:dale-api-base`, ["later-run"]);
    const client = stubClient();
    await open(client);
    client.listExecutions.mockRejectedValueOnce(new Error("offline"));
    await userEvent.click(screen.getByRole("button", { name: "restore later-run" }));
    expect(await screen.findByText(/The run could not be loaded/)).toHaveAttribute("role", "status");
    expect(screen.getByRole("button", { name: "restore later-run" })).toBeVisible();
  });

  it("discards a pending restore after disconnecting", async () => {
    saveRemovedRuns(`${location.origin}:dale-api-base`, ["later-run"]);
    const client = stubClient();
    await open(client);
    let finish!: (page: Awaited<ReturnType<typeof client.listExecutions>>) => void;
    client.listExecutions.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    await userEvent.click(screen.getByRole("button", { name: "restore later-run" }));
    fireEvent.click(screen.getByRole("button", { name: /^disconnect$/i }), { detail: 1 });
    fireEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }), { detail: 1 });
    await screen.findByLabelText(/bearer token/i);
    finish({ executions: [{ executionId: "later-run", mode: "supervised", status: "running", attention: "can_sleep", startedAt: null, lastEventAt: null, headSequence: 3 }], hasMore: false, nextCursor: null });
    await userEvent.type(screen.getByLabelText(/bearer token/i), "local-token");
    await userEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    expect(await screen.findByRole("button", { name: "restore later-run" })).toBeVisible();
    expect(within(screen.getByRole("navigation", { name: "Projects" })).queryByRole("button", { name: /^later-run/ })).not.toBeInTheDocument();
  });

  it("keeps the objective when the conversation is hidden and reopened", async () => {
    await open(stubClient());
    await userEvent.click(screen.getByRole("button", { name: "New task in dale-api-base" }));
    await userEvent.type(screen.getByLabelText("What should this task do?"), "Keep my objective");
    await userEvent.click(screen.getByRole("button", { name: "Toggle conversation" }));
    await userEvent.click(screen.getByRole("button", { name: "Toggle conversation" }));
    expect(screen.getByLabelText("What should this task do?")).toHaveValue("Keep my objective");
  });

  it("opens the new task composer after the conversation was closed", async () => {
    await open(stubClient());
    await userEvent.click(screen.getByRole("button", { name: "Toggle conversation" }));
    expect(screen.getByRole("button", { name: "Toggle conversation" })).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(screen.getByRole("button", { name: "New task in dale-api-base" }));
    expect(screen.getByLabelText("What should this task do?")).toBeVisible();
    expect(screen.getByRole("button", { name: "Toggle conversation" })).toHaveAttribute("aria-expanded", "true");
  });

  it("keeps an unsent message when the conversation is hidden and reopened", async () => {
    await open(stubClient());
    await screen.findByLabelText(/^Run demo-deploy/);
    await userEvent.type(screen.getByLabelText(/say something into this run/i), "Do not lose these words");
    await userEvent.click(screen.getByRole("button", { name: "Toggle conversation" }));
    await userEvent.click(screen.getByRole("button", { name: "Toggle conversation" }));
    expect(screen.getByLabelText(/say something into this run/i)).toHaveValue("Do not lose these words");
  });

  it("persists rename and reversible removal across a remount without cancelling execution", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: "Rename dale-api-base" }));
    await userEvent.clear(screen.getByRole("textbox", { name: "Project name" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Project name" }), "My workspace{Enter}");
    await userEvent.click(screen.getByRole("button", { name: "Remove demo-deploy from this browser's list" }));
    await userEvent.click(screen.getByRole("button", { name: /^Remove$/ }));
    expect(within(screen.getByRole("navigation", { name: "Projects" })).queryByRole("button", { name: /^demo-deploy/ })).not.toBeInTheDocument();
    expect(client.cancel).not.toHaveBeenCalled();
    cleanup();
    await open(stubClient());
    expect(screen.getByRole("button", { name: "Rename My workspace" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "restore demo-deploy" }));
    expect(within(screen.getByRole("navigation", { name: "Projects" })).getByRole("button", { name: /^demo-deploy/ })).toBeInTheDocument();
  });

  it("reports failed browser persistence instead of claiming a saved rename", async () => {
    await open(stubClient());
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("full"); });
    await userEvent.click(screen.getByRole("button", { name: "Rename dale-api-base" }));
    await userEvent.clear(screen.getByRole("textbox", { name: "Project name" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Project name" }), "View only{Enter}");
    expect(await screen.findByText(/Project name changed for this view only/)).toHaveAttribute("role", "status");
  });
});

describe("live team layout", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => localStorage.clear());

  function longGapClient() {
    const many = Array.from({ length: 24 }, (_, i) => ({ sequence: 20 + i, kind: "signal_recorded", payload: { kind: "operator_note", signalId: `s${i}` },
      occurredAt: new Date(Date.parse("2026-08-27T12:30:00Z") + i * 60_000).toISOString(), actorId: "kit-1", actorType: "agent",
      idempotencyKey: null, eventId: `e${20 + i}`, evidenceRefs: [] }));
    // Run A (demo-deploy) has the long gap; run B (demo-calm) has a short log, so it never earns a card.
    return stubClient({ getEvents: vi.fn(async (executionId: string) => executionId === "demo-calm" ? { head: 4, events: [] } : { head: 43, events: [
      { sequence: 1, kind: "execution_form_declared", payload: { executionId: "demo-deploy", nodeIds: ["implementation"] }, occurredAt: "2026-08-27T12:00:00Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k0", eventId: "e1", evidenceRefs: [] },
      ...many,
    ] }), getStatus: vi.fn(async (executionId: string) => ({ ...STATUS, executionId, headSequence: executionId === "demo-calm" ? 4 : 43 })) });
  }

  it("lights the beacon for a blocked step and opens its card with the legal action", async () => {
    await open(stubClient());
    const beacon = await screen.findByRole("button", { name: "1 decision needs you" });
    await userEvent.click(beacon);
    const cards = screen.getByRole("region", { name: "Needs you" });
    expect(within(cards).getByText("implementation is blocked.")).toBeInTheDocument();
    expect(within(cards).getByRole("button", { name: /(approv|allow.*retry).*implementation/i })).toBeInTheDocument();
    await waitFor(() => expect(cards).toHaveFocus());
  });

  it("goes dark when the Runtime answers and nothing is open", async () => {
    const calm = { ...STATUS, attention: "can_sleep", attentionReasons: [], nodeStateCounts: { ready: 1 } };
    await open(stubClient({ getStatus: vi.fn(async () => calm) }));
    expect(await screen.findByRole("button", { name: "Nothing needs you" })).toHaveClass("beacon-dark");
  });

  it("shows the handover after a long gap and advances only on Got it", async () => {
    localStorage.setItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy", "1");
    await open(longGapClient());
    const card = await screen.findByRole("region", { name: "While you were away" });
    expect(card).toHaveTextContent("24 records");
    await userEvent.click(within(card).getByRole("button", { name: "Got it" }));
    expect(localStorage.getItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy")).toBe("43");
    expect(screen.queryByRole("region", { name: "While you were away" })).toBeNull();
  });

  // The owner: the card opened over the Journey tab on every load and covered the list and its
  // Watch/Approve buttons. It is a bar in the canvas column's flow, before the canvas tabs, and Hide
  // lasts the browser session without marking anything seen.
  it("puts the handover in the canvas column before the tabs, and Hide lasts the session", async () => {
    localStorage.setItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy", "1");
    await open(longGapClient());
    const bar = await screen.findByRole("region", { name: "While you were away" });
    expect(screen.queryByRole("dialog", { name: "While you were away" })).toBeNull();
    const column = screen.getByRole("region", { name: "Canvas" });
    expect(column.contains(bar)).toBe(true);
    const tabs = within(column).getByRole("tablist", { name: "Canvas views" });
    expect(bar.compareDocumentPosition(tabs) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await userEvent.click(within(bar).getByRole("button", { name: "Hide for now" }));
    expect(screen.queryByRole("region", { name: "While you were away" })).toBeNull();
    expect(localStorage.getItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy")).toBe("1");
    cleanup();
    await open(longGapClient());
    await screen.findByRole("tablist", { name: "Canvas views" });
    expect(screen.queryByRole("region", { name: "While you were away" })).toBeNull();
    sessionStorage.clear();
  });

  it("#583: with nothing saved, Graph is the whole page: no handover, no tab strip; Team returns", async () => {
    sessionStorage.clear();
    localStorage.setItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy", "1");
    await open(longGapClient());
    const nav = await screen.findByRole("tablist", { name: "Mission views" });
    expect(screen.getByText("Mission graph")).toBeInTheDocument();
    expect(screen.getByText("Run demo-deploy")).toBeInTheDocument();
    expect(screen.queryByRole("tablist", { name: "Canvas views" })).toBeNull();
    expect(screen.queryByRole("region", { name: "While you were away" })).toBeNull();
    await userEvent.click(within(nav).getByRole("tab", { name: "Team" }));
    expect(await screen.findByRole("tablist", { name: "Canvas views" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Team (live)" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("region", { name: "While you were away" })).toBeInTheDocument();
    expect(sessionStorage.getItem("graphhelm.studio.canvas-tab")).toBe("team");
  });

  it("does not show run A's handover after switching to run B", async () => {
    localStorage.setItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy", "1");
    localStorage.setItem("graphhelm.handover.last-seen:dale-api-base:demo-calm", "1");
    await open(longGapClient());
    expect(await screen.findByRole("region", { name: "While you were away" })).toBeInTheDocument();
    await userEvent.click(within(screen.getByRole("navigation", { name: "Projects" })).getByText("demo-calm", { exact: true }).closest("button")!);
    // Immediately, before run B's own reads land: run B has a last-seen of its own, and run A's
    // events and status are still the ones in memory. select() clears status and events, which hides
    // A's card; the status.executionId guard is defence-in-depth behind it.
    expect(screen.queryByRole("region", { name: "While you were away" })).toBeNull();
    await waitFor(() => expect(screen.queryByRole("region", { name: "While you were away" })).toBeNull());
    expect(screen.queryByText(/24 records/)).toBeNull();
  });

  it("resume without a graph file selects the Team tab and focuses the graph field by state", async () => {
    const paused = { ...STATUS, status: "paused", attentionReasons: [] };
    await open(stubClient({ getStatus: vi.fn(async () => paused) }));
    await userEvent.click(screen.getByRole("tab", { name: "Journey" }));
    await userEvent.click(screen.getByRole("button", { name: "resume" }));
    expect(screen.getByRole("tab", { name: "Team (live)" })).toHaveAttribute("aria-selected", "true");
    await waitFor(() => expect(screen.getByLabelText("Graph file path on the Runtime host")).toHaveFocus());
  });


  it("links mobile tabs to panels and moves selection and focus with arrows, Home and End", async () => {
    await open(stubClient());
    const tabs = within(screen.getByRole("tablist", { name: "Studio columns" }));
    const team = tabs.getByRole("tab", { name: "Team" });
    const chat = tabs.getByRole("tab", { name: "Chat" });
    const journeys = tabs.getByRole("tab", { name: "Journeys" });
    expect(chat).toHaveAttribute("tabindex", "-1");
    team.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(journeys).toHaveFocus();
    expect(journeys).toHaveAttribute("aria-selected", "true");
    const panel = document.getElementById(journeys.getAttribute("aria-controls")!);
    expect(panel).toHaveAttribute("role", "tabpanel");
    expect(panel).toHaveAttribute("aria-labelledby", journeys.id);
    await userEvent.keyboard("{ArrowRight}");
    expect(chat).toHaveFocus();
    expect(document.getElementById(chat.getAttribute("aria-controls")!)).toHaveAttribute("role", "tabpanel");
    await userEvent.keyboard("{End}");
    expect(journeys).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(chat).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(journeys).toHaveFocus();
  });

  it("opens the exact lifecycle record cited by a handover line instead of the whole run", async () => {
    localStorage.setItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy", "1");
    const client = longGapClient();
    const baseEvents = client.getEvents;
    client.getEvents = vi.fn(async (id: string) => {
      const data = await baseEvents();
      return { ...data, events: data.events.map((event) => event.sequence === 20
        ? { ...event, kind: "node_outcome_recorded", payload: { nodeId: "implementation", outcome: "succeeded", nextState: "succeeded" } }
        : event) };
    }) as typeof client.getEvents;
    await open(client);
    const card = await screen.findByRole("region", { name: "While you were away" });
    await userEvent.click(within(card).getByRole("button", { name: "Show details" }));
    await userEvent.click(within(card).getByRole("button", { name: "implementation succeeded" }));
    const records = await screen.findByRole("dialog", { name: "Cited records" });
    expect(within(records).getByRole("button", { name: "#20" })).toBeInTheDocument();
    expect(within(records).queryByRole("button", { name: "#21" })).toBeNull();
    expect(localStorage.getItem("graphhelm.handover.last-seen:dale-api-base:demo-deploy")).toBe("1");
    await userEvent.click(within(records).getByRole("button", { name: "Close cited records" }));
    expect(screen.queryByRole("dialog", { name: "Cited records" })).toBeNull();
  });

  it("below 768 px the Chat / Team / Journeys tabs choose the one visible column", async () => {
    await open(stubClient());
    const layout = document.querySelector(".studio-columns") as HTMLElement;
    const tabs = within(screen.getByRole("tablist", { name: "Studio columns" }));
    await userEvent.click(tabs.getByRole("tab", { name: "Chat" }));
    expect(layout).toHaveAttribute("data-mobile-tab", "chat");
    await userEvent.click(tabs.getByRole("tab", { name: "Journeys" }));
    expect(layout).toHaveAttribute("data-mobile-tab", "journeys");
    expect(screen.getByRole("tab", { name: "Journey" })).toHaveAttribute("aria-selected", "true");
    await userEvent.click(tabs.getByRole("tab", { name: "Team" }));
    expect(layout).toHaveAttribute("data-mobile-tab", "team");
  });

  it("shows a capture recorded in one run while another run is selected (#332)", async () => {
    const capture = { signalId: "cap-cart", executionId: "demo-deploy", sequence: 9, recordedAt: "2026-08-27T12:00:00Z", imageEvidenceId: "img-cart",
      revision: "abcdef1234567", dirty: false, viewport: { width: 800, height: 600 }, observer: "kit-1", freshness: "fresh", changedFiles: [] };
    const view = { head: "abc", journeys: [{ contractId: "checkout", title: "Checkout", arrows: [],
      steps: [{ stepId: "cart", screen: { screenId: "cart", title: "Cart", scopePaths: ["src/cart"] }, capture, promises: [] }] }] };
    const journeys = vi.fn(async (executionId?: string) => (executionId === undefined ? view : { head: "abc", journeys: [] }));
    const readImage = vi.fn(async () => new Blob(["png"], { type: "image/png" }));
    Object.assign(URL, { createObjectURL: vi.fn(() => "blob:cart"), revokeObjectURL: vi.fn() });
    await open(stubClient({ journeys, readImage }));
    await userEvent.click(within(screen.getByRole("navigation", { name: "Projects" })).getByText("demo-calm", { exact: true }).closest("button")!);
    await userEvent.click(screen.getByRole("tab", { name: "Journey" }));
    const card = await screen.findByRole("button", { name: "Cart, open detail" });
    expect(card).not.toHaveTextContent("Not captured yet");
    expect(journeys).toHaveBeenCalledWith();
    expect(journeys.mock.calls.every((call: unknown[]) => call.length === 0)).toBe(true);
    await waitFor(() => expect(readImage).toHaveBeenCalledWith("demo-deploy", "img-cart"));
    await userEvent.click(card);
    expect(screen.getByRole("dialog", { name: "Cart detail" })).toHaveTextContent("Captured in run demo-deploy (not the selected run)");
  });

  // #465: one source of truth for which journey is on screen. The map below followed its own
  // picker while the list above had another journey selected, and a draft showed proof labels
  // ("Not captured yet", Open live) where the owner's only actions are Watch and Approve.
  it("the journey map follows the list, has no picker of its own, and is not shown for a draft", async () => {
    const step = (stepId: string) => ({ stepId, screen: { screenId: stepId, title: stepId, scopePaths: ["src"] }, capture: null, promises: [] });
    const view = { head: "abc", journeys: [
      { contractId: "checkout", title: "Checkout", arrows: [], steps: [step("cart")] },
      { contractId: "paid", title: "Paid order", arrows: [], steps: [step("receipt")] },
    ] };
    const flowOf = (id: string, status: string, approvable: boolean) => ({ id, title: `${id} flow`, status, approvable, drift: [], findings: [],
      approved: status === "approved" ? { revision: "a".repeat(40), digest: "sha256:b" } : null,
      screens: [{ id: "s", url: "/", state: "stable" }], edges: [], paths: {} });
    const journeyFlows = vi.fn(async () => ({ flows: [flowOf("checkout", "draft", true), flowOf("paid", "approved", false)] }));
    await open(stubClient({ journeys: vi.fn(async () => view), journeyFlows }));
    await userEvent.click(screen.getByRole("tab", { name: "Journey" }));
    expect(await screen.findByRole("article", { name: "Journey checkout flow" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Journey" })).toBeNull();
    expect(screen.queryByText("Not captured yet")).toBeNull();

    await userEvent.click(within(screen.getByRole("list", { name: "Journeys" })).getByRole("button", { name: /^paid flow/ }));
    const map = await screen.findByRole("region", { name: "Journey" });
    expect(within(map).queryByRole("combobox")).toBeNull();
    expect(within(map).getByRole("button", { name: "receipt, open detail" })).toBeInTheDocument();
    expect(within(map).queryByRole("button", { name: "cart, open detail" })).toBeNull();
  });

  // #519: an approved flow's run is its replay, and its proof is recorded into a run. Opening a
  // journey without naming the selected run would run the test and record nothing.
  it("opens a journey's test naming the selected run, so an approved flow's proof has a run to land in", async () => {
    const flows = { flows: [{ id: "paid", title: "paid flow", status: "approved", approvable: false, drift: [], findings: [],
      approved: { revision: "a".repeat(40), digest: "sha256:b" }, screens: [{ id: "s", url: "/", state: "stable" }], edges: [], paths: {} }] };
    const startJourneyRun = vi.fn(async () => ({ state: "none" }));
    await open(stubClient({ journeyFlows: vi.fn(async () => flows), startJourneyRun }));
    await userEvent.click(screen.getByRole("tab", { name: "Journey" }));
    await screen.findByRole("article", { name: "Journey paid flow" });
    await waitFor(() => expect(startJourneyRun).toHaveBeenCalledWith("paid", false, "demo-deploy"));
  });
});

/** The ordinary loop: the dev server hands the page a token and it opens connected. */
async function open(client: ReturnType<typeof stubClient>, modelContext: ModelContextLike | null = null, conversation = true) {
  render(
    <App
      createClient={() => client as unknown as RuntimeClient}
      modelContext={modelContext}
      session={async () => ({ token: "local-token", project: "dale-api-base" })}
    />,
  );
  await screen.findByLabelText("Projects");
  // #585: the run's actions open from a control with a role and a name, so a journey can target it.
  const runActions = screen.getByRole("button", { name: "Run actions" });
  await userEvent.click(runActions);
  expect(runActions).toHaveAttribute("aria-expanded", String(runActions.closest("details")?.open));
  // These conversation journeys explicitly open the optional pane.
  if (conversation && screen.getByRole("button", { name: "Toggle conversation" }).getAttribute("aria-expanded") !== "true") {
    await userEvent.click(screen.getByRole("button", { name: "Toggle conversation" }));
  }
  // Legacy journeys below inspect the event log after choosing its Details disclosure.
  const runDetails = document.querySelector("details.run-technical-details > summary");
  if (runDetails) await userEvent.click(runDetails);
}

describe("opening", () => {
  it("reviews a raw-digest DraftProposed before any ghost event and submits plain AddNode ids", async () => {
    const content = JSON.stringify({ id: "draft-wire", operations: [
      { op: "addNode", path: "/spec/nodes/summarize", value: { name: "Summarize", objective: "Make a short summary" } },
      { op: "addNode", path: "/spec/nodes/verify", value: { name: "Verify", objective: "Check the summary" } },
      { op: "addEdge", value: { from: "summarize", to: "verify", type: "data" } },
    ] });
    const rawDigest = (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice("sha256:".length);
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, status: "paused", attention: "can_sleep", attentionReasons: [] })),
      getEvents: vi.fn(async () => ({
        head: 4,
        events: [
          { sequence: 2, kind: "execution_form_declared", payload: { executionId: "demo-deploy", nodeIds: ["implementation"] }, occurredAt: null, actorId: "system-cli", actorType: "system", idempotencyKey: "k2", eventId: "e2", evidenceRefs: [] },
          { sequence: 3, kind: "draft_proposed", payload: { draftId: "draft-wire", expectedVersion: 1, expectedHash: "sha256:expected", proposalSha256: rawDigest, operationCount: 3 }, occurredAt: null, actorId: "agent-a", actorType: "agent", idempotencyKey: "k3", eventId: "e3", evidenceRefs: ["sealed-draft"] },
        ],
      })),
      readEvidence: vi.fn(async () => ({ evidenceId: "sealed-draft", mediaType: "application/json", sensitivity: "confidential", contentSha256: rawDigest, content })),
    });
    await open(client);
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Draft to review" }), "draft-wire");
    expect(await screen.findByText(/Proposed nodes · 2/)).toBeVisible();
    expect(screen.getByText(/paused · Needs you/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "demo-deploy" }));
    expect(await screen.findByRole("heading", { name: "This run needs you" })).toBeVisible();
    expect(screen.getByText(/waiting for owner review/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Close this panel" }));
    const proposalCards = await screen.findByRole("region", { name: "Needs you" });
    expect(proposalCards).toHaveTextContent("A proposal is waiting for your review.");
    expect(within(proposalCards).getByRole("button", { name: "Review proposal" })).toBeInTheDocument();
    expect(proposalCards).not.toHaveTextContent("recommended replies");
    expect(screen.getByText(/summarize · Summarize · Make a short summary/)).toBeVisible();
    const approve = await screen.findByRole("button", { name: "approve proposal for summarize" });
    await waitFor(() => expect(approve).toBeEnabled());
    await userEvent.click(approve);
    await waitFor(() => expect(client.approve).toHaveBeenCalled());
    const [, node, options] = firstCall(client.approve as unknown as { mock: { calls: unknown[][] } });
    expect(node).toBe("summarize");
    expect(options).toEqual(expect.objectContaining({ governed: { draftId: "draft-wire", proposalDigest: rawDigest, assignments: { summarize: "agent-a", verify: "agent-a" } } }));
  });

  it("rejects the obsolete unscoped node pointer", async () => {
    const content = JSON.stringify({ id: "draft-invalid-path", operations: [
      { op: "addNode", path: "/nodes/summarize", value: { name: "Summarize" } },
    ] });
    const rawDigest = (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice("sha256:".length);
    const client = stubClient({
      getEvents: vi.fn(async () => ({ head: 3, events: [
        { sequence: 3, kind: "draft_proposed", payload: { draftId: "draft-invalid-path", proposalSha256: rawDigest }, occurredAt: null, actorId: "agent-a", actorType: "agent", idempotencyKey: "k3", eventId: "e3", evidenceRefs: ["sealed-invalid"] },
      ] })),
      readEvidence: vi.fn(async () => ({ evidenceId: "sealed-invalid", mediaType: "application/json", sensitivity: "confidential", contentSha256: rawDigest, content })),
    });
    await open(client);
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Draft to review" }), "draft-invalid-path");
    expect(await screen.findByText("typed addNode path is invalid")).toBeVisible();
    expect(screen.getByRole("button", { name: "review selected proposal" })).toBeDisabled();
  });

  it("opens the recorded blocked-step controls without calling the action an answer", async () => {
    await open(stubClient());
    const cards = await screen.findByRole("region", { name: "Needs you" });
    expect(cards).toHaveTextContent("implementation is blocked.");
    expect(cards).not.toHaveTextContent(/answer/i);
    expect(within(cards).getByRole("button", { name: /(approv|allow.*retry).*implementation/i })).toBeInTheDocument();
  });

  it("does not invent an owner request when a graph step waits without a question", async () => {
    const client = stubClient({ getStatus: vi.fn(async () => ({
      ...STATUS, attentionReasons: [{ kind: "waiting_input_node", node: "implementation" }],
    })) });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(screen.getByRole("button", { name: "Close this panel" }));
    expect(screen.queryByRole("article", { name: /asks$/ })).not.toBeInTheDocument();
    expect(screen.getByText("graph waiting; no request recorded")).toBeInTheDocument();
    expect(client.getReplySuggestions).not.toHaveBeenCalled();
  });

  it("leads with recorded external work when the only wait is an unanswered graph step without a request", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, attentionReasons: [{ kind: "waiting_input_node", node: "start" }] })),
      getEvents: vi.fn(async () => ({ head: 13, events: [{
        sequence: 13, kind: "signal_recorded", payload: { kind: "operator_note", signalId: "note-13" },
        occurredAt: "2026-08-27T12:01:00Z", actorId: "codex", actorType: "agent",
        idempotencyKey: "mcp-aaaaaaaaaaaaaaaa-s13-record-1111111111111111",
        eventId: "event-13", evidenceRefs: ["evidence-note"],
      }] })),
      readEvidence: vi.fn(async () => ({ evidenceId: "evidence-note", mediaType: "application/json",
        sensitivity: "confidential", contentSha256: "sha256:fixture",
        content: JSON.stringify({ description: "Reviewed the implementation path." }) })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(screen.getByRole("button", { name: "Close this panel" }));
    expect(document.querySelector(".topbar .tag")).toHaveTextContent("agent work recorded · graph step waiting");
    const selected = screen.getByRole("navigation", { name: "Projects" }).querySelector('button[aria-current="true"]')!;
    expect(selected).toHaveAttribute("title", "agent work recorded · graph step waiting");
    expect(selected).toHaveClass("calm");
  });

  it("does not turn a capacity wait into an owner reply request", async () => {
    const client = stubClient({ getStatus: vi.fn(async () => ({
      ...STATUS, attention: "can_sleep", attentionReasons: [{ kind: "waiting_capacity_node", node: "implementation" }],
    })) });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(screen.getByRole("button", { name: "Close this panel" }));
    expect(screen.queryByRole("region", { name: "Next action" })).not.toBeInTheDocument();
    expect(screen.queryByText("Owner action")).not.toBeInTheDocument();
  });

  it("keeps governed approval disabled until a paused Runtime has no running node", async () => {
    const content = JSON.stringify({ id: "draft-paused-gate", operations: [
      { op: "addNode", path: "/spec/nodes/summarize", value: { name: "Summarize", objective: "Make a short summary" } },
    ] });
    const rawDigest = (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice("sha256:".length);
    let phase: "running" | "paused-running" | "paused-ready" = "running";
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        status: phase === "running" ? "running" : "paused",
        nodeStateCounts: { blocked: 1 },
        nodeStates: phase === "paused-running" ? { summarize: "running" } : {},
      })),
      getEvents: vi.fn(async () => ({
        head: 13,
        events: [
          { sequence: 2, kind: "execution_form_declared", payload: { executionId: "demo-deploy", nodeIds: ["summarize"] }, occurredAt: null, actorId: "system-cli", actorType: "system", idempotencyKey: "k2", eventId: "e2", evidenceRefs: [] },
          { sequence: 3, kind: "draft_proposed", payload: { draftId: "draft-paused-gate", proposalSha256: rawDigest }, occurredAt: null, actorId: "agent-a", actorType: "agent", idempotencyKey: "k3", eventId: "e3", evidenceRefs: ["sealed-paused-gate"] },
        ],
      })),
      readEvidence: vi.fn(async () => ({ evidenceId: "sealed-paused-gate", mediaType: "application/json", sensitivity: "confidential", contentSha256: rawDigest, content })),
    });
    render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={20}
      />,
    );
    await screen.findByLabelText("Projects");
    await userEvent.click(screen.getByText("Run actions"));
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Draft to review" }), "draft-paused-gate");

    const approve = await screen.findByRole("button", { name: "approve proposal for summarize" });
    expect(approve).toBeDisabled();
    expect(screen.getByText("Pause the run before approving this proposal.")).toBeVisible();
    expect(client.approve).not.toHaveBeenCalled();

    phase = "paused-running";
    await waitFor(() => expect(screen.getByText("Approval is waiting for the running node to stop.")).toBeVisible());
    expect(approve).toBeDisabled();
    expect(client.approve).not.toHaveBeenCalled();

    phase = "paused-ready";
    await waitFor(() => expect(approve).toBeEnabled());
    await userEvent.click(approve);
    await waitFor(() => expect(client.approve).toHaveBeenCalled());
  });

  /** THE POINT OF THE SESSION WORK. Nobody types anything: the page asks the dev server, gets the
   * token the Runtime already wrote, and is connected before the operator does a thing. */
  it("opens connected, with no token asked for", async () => {
    const client = stubClient();
    await open(client);
    expect(screen.queryByLabelText(/bearer token/i)).not.toBeInTheDocument();
    expect(client.health).toHaveBeenCalled();
  });

  /** A built bundle served elsewhere has no such endpoint. That is the ordinary outcome, not an
   * error, so the gate appears and says nothing alarming. */
  it("falls back to the gate when no session is offered, without reporting an error", async () => {
    render(
      <App
        createClient={() => stubClient() as unknown as RuntimeClient}
        modelContext={null}
        session={async () => null}
      />,
    );
    expect(await screen.findByLabelText(/bearer token/i)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reports a refused token and stays on the gate", async () => {
    const client = stubClient({
      health: vi.fn(async () => {
        throw Object.assign(new Error("The bearer token was refused."), { name: "RuntimeError" });
      }),
    });
    render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => null}
      />,
    );
    await userEvent.type(await screen.findByLabelText(/bearer token/i), "bad");
    await userEvent.click(screen.getByRole("button", { name: /connect/i }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByLabelText(/bearer token/i)).toBeInTheDocument();
  });
});

describe("the projects rail", () => {
  it("shows the real folder under the project name when the local session supplies it", async () => {
    render(<App
      createClient={() => stubClient() as unknown as RuntimeClient}
      modelContext={null}
      session={async () => ({ token: "local-token", project: "fixture-service", projectPath: "fixtures/service" })}
    />);
    const rail = await screen.findByLabelText("Projects");
    expect(within(rail).getByText("fixture-service")).toBeVisible();
    expect(within(rail).getByText("fixtures/service")).toBeVisible();
  });
  /** A project is a FOLDER and the runs live inside it. The rail says how many projects there
   * really are rather than implying a hierarchy the Runtime cannot back. */
  it("lists the folder, its tasks, and the way to add another folder", async () => {
    await open(stubClient());
    const rail = screen.getByLabelText("Projects");
    // The rail calls the folder what the operator named it, not "this runtime".
    expect(within(rail).getByText("dale-api-base")).toBeInTheDocument();
    expect(within(rail).getByText("demo-deploy")).toBeInTheDocument();
    expect(within(rail).getByText("demo-calm")).toBeInTheDocument();
    expect(within(rail).getByRole("button", { name: /add project folder/i })).toBeInTheDocument();
  });

  /**
   * THE STATE SURVIVES A NARROW RAIL, because it is no longer text on the row.
   *
   * It used to sit beside the execution id, and ids are long: "can sleep" arrived as "can sl" and
   * "needs you" as "needs". Reading it out of the run button's ACCESSIBLE NAME is the assertion
   * that holds either way - a truncated word would still be in the DOM, so asserting on text
   * alone would not have caught the defect that shipped.
   */
  it("names each task's state in full, where no width can cut it", async () => {
    await open(stubClient());
    const rail = screen.getByLabelText("Projects");
    expect(
      within(rail).getByRole("button", { name: /demo-deploy.*needs you/i }),
    ).toBeInTheDocument();
    expect(within(rail).getByRole("button", { name: /demo-calm.*can sleep/i })).toBeInTheDocument();
  });

  /** The action on the folder row is icon-only: a label there is what clipped to "+ new ta". Its
   * name has to reach a screen reader anyway, so the accessible name is where it lives. */
  it("offers new task on the folder row without putting a label on it", async () => {
    await open(stubClient());
    const rail = screen.getByLabelText("Projects");
    const action = within(rail).getByRole("button", { name: /new task in dale-api-base/i });
    expect(action).toBeInTheDocument();
    expect(action).toHaveTextContent("");
  });

  it("opens on the run that needs the operator, not merely the first one", async () => {
    const client = stubClient();
    await open(client);
    await waitFor(() => expect(client.getStatus).toHaveBeenCalledWith("demo-deploy"));
  });

  it("opens the most recently active run when several runs need the operator", async () => {
    const client = stubClient({
      listExecutions: vi.fn(async () => ({
        executions: [
          { executionId: "quiet-demo", mode: "supervised", status: "running", attention: "needs_you", startedAt: null, lastEventAt: "2026-09-25T13:00:00Z", headSequence: 7 },
          { executionId: "active-run", mode: "supervised", status: "running", attention: "needs_you", startedAt: null, lastEventAt: "2026-09-25T17:00:00Z", headSequence: 39 },
        ],
        hasMore: false,
        nextCursor: null,
      })),
    });
    await open(client);
    await waitFor(() => expect(client.getStatus).toHaveBeenCalledWith("active-run"));
    expect(client.getStatus).not.toHaveBeenCalledWith("quiet-demo");
  });
});

describe("the board", () => {
  it("puts every declared node on it, including one nothing has happened to", async () => {
    await open(stubClient());
    const board = await screen.findByLabelText("Steps without a bot");
    expect(within(board).getByRole("button", { name: /^implementation/ })).toBeInTheDocument();
    expect(within(board).getByRole("button", { name: /^deploy/ })).toBeInTheDocument();
  });

  /** Nothing proves the shape until a graph file's hash matches the run's, so nothing is drawn
   * and the note says so. */
  it("draws no connection until one is proven", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "Graph file" }));
    expect(document.querySelectorAll("path.edge, .team-link")).toHaveLength(0);
    expect(screen.getByText(/work connections unverified/i)).toBeInTheDocument();
  });

  it("renders the log as text, never as markup", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    expect(panel.textContent).toContain("retryable failure");
    expect(panel.innerHTML).not.toContain("<script");
  });
});

describe("the window", () => {
  /** THE CONVERSATION AND THE BOARD ARE PEERS, NOT LAYERS. The first layout floated the run
   * window over the canvas, and a real screenshot (2026-08-30) showed it burying the node cards,
   * the attention line and half the dock. A selected run now opens its conversation BESIDE the
   * board - both visible, neither covering the other; only a node's own window is a layer, and it
   * layers over the canvas it describes, never over the chat. */
  it("opens with the conversation beside the board, and nothing over either", async () => {
    await open(stubClient());
    expect(await screen.findByLabelText("Steps without a bot")).toBeInTheDocument();
    expect(await screen.findByLabelText(/^Run (?!side panel)/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/^Node /)).not.toBeInTheDocument();
  });

  /** The run window carries the sixteen states — zeroes included, because an omitted bucket reads
   * as "no such problem". */
  it("shows only the states that are something, and asserts the zeros in one line", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    // A chip per non-zero state, nothing for the empty ones...
    const chips = panel.querySelectorAll(".state-chip");
    expect(chips.length).toBeGreaterThan(0);
    expect(within(panel).queryByText("cancelled")).not.toBeInTheDocument();
    // ...and the zeros are still asserted, collectively: silence stays a statement. In words a
    // person reads without knowing what a lifecycle state is - "all zero" made a real operator
    // ask what was happening on their own screen (2026-08-30).
    expect(within(panel).getByText(/nothing in the other \d+ states/)).toBeInTheDocument();
    // Each chip keeps the API's own state name reachable for whoever must match the wire.
    expect(chips[0]!.getAttribute("title")).not.toBeNull();
  });

  it("swaps to a node when its block is opened, and back when it is closed", async () => {
    await open(stubClient());
    const board = await screen.findByLabelText("Steps without a bot");
    await userEvent.click(within(board).getByRole("button", { name: /implementation/i }));

    const nodePanel = await screen.findByLabelText("Node implementation");
    expect(within(nodePanel).getByText(/retryable failure/i)).toBeInTheDocument();
    // The roster declaration names no node, so a node thread must not carry it.
    expect(nodePanel.textContent).not.toContain("Declared 2 nodes");

    await userEvent.click(within(nodePanel).getByRole("button", { name: /close this node/i }));
    await waitFor(() => expect(screen.queryByLabelText(/^Node /)).not.toBeInTheDocument());
    expect(screen.getByLabelText("Steps without a bot")).toBeInTheDocument();
  });

  it.each([
    ["a new task", /new task in/i],
    ["the add-project flow", /add project folder/i],
  ])("keeps a document draft open when the operator declines %s", async (_label, action) => {
    const record = JSON.stringify({
      version: 1,
      projectId: "a".repeat(64),
      summary: "Updated project rule",
      reason: "The journey changed.",
      documents: [{ path: "docs/rule.md", title: "Project rule", kind: "business_rule", action: "updated" }],
    });
    const client = stubClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          { sequence: 2, kind: "execution_form_declared", payload: { executionId: "demo-deploy", nodeIds: ["implementation"] }, occurredAt: "2026-08-27T12:00:30Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k0", eventId: "event-2", evidenceRefs: [] },
          { sequence: 14, kind: "signal_recorded", payload: { kind: "node_delivery", sourceKind: "node", sourceId: "implementation" }, occurredAt: "2026-08-27T12:02:00Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k14", eventId: "event-14", evidenceRefs: ["delivery"] },
        ],
      })),
      readEvidence: vi.fn(async () => ({ evidenceId: "delivery", mediaType: "application/json", sensitivity: "internal", contentSha256: "hash", content: record })),
    });
    await open(client);
    const board = await screen.findByLabelText("Steps without a bot");
    await userEvent.click(within(board).getByRole("button", { name: /implementation/i }));
    await userEvent.click(await screen.findByRole("button", { name: /Project rule/ }));
    const content = await screen.findByLabelText("File content");
    await userEvent.type(content, " changed");
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);

    await userEvent.click(screen.getByRole("button", { name: /Project rule/ }));
    expect(confirm).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: action }));

    expect(confirm).toHaveBeenCalledOnce();
    expect(screen.getByLabelText("Project document editor")).toBeInTheDocument();
    expect(screen.getByLabelText("File content")).toHaveValue("Original rule changed");
    expect(screen.queryByRole("heading", { name: "New task" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Add project folder")).not.toBeInTheDocument();
    confirm.mockRestore();
  });

  it("blocks run selection, new task, and Add Project while a document save is in flight", async () => {
    const record = JSON.stringify({
      version: 1,
      projectId: "a".repeat(64),
      summary: "Updated project rule",
      reason: "The journey changed.",
      documents: [{ path: "docs/rule.md", title: "Project rule", kind: "business_rule", action: "updated" }],
    });
    let resolveSave!: (value: { contentSha256: string; notification: { status: "recorded"; notifiedRuns: string[]; pendingRuns: string[] } }) => void;
    const client = stubClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          { sequence: 2, kind: "execution_form_declared", payload: { executionId: "demo-deploy", nodeIds: ["implementation"] }, occurredAt: "2026-08-27T12:00:30Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k0", eventId: "event-2", evidenceRefs: [] },
          { sequence: 14, kind: "signal_recorded", payload: { kind: "node_delivery", sourceKind: "node", sourceId: "implementation" }, occurredAt: "2026-08-27T12:02:00Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k14", eventId: "event-14", evidenceRefs: ["delivery"] },
        ],
      })),
      readEvidence: vi.fn(async () => ({ evidenceId: "delivery", mediaType: "application/json", sensitivity: "internal", contentSha256: "hash", content: record })),
      saveDocument: vi.fn(() => new Promise((resolve) => { resolveSave = resolve; })),
    });
    await open(client);
    const board = await screen.findByLabelText("Steps without a bot");
    await userEvent.click(within(board).getByRole("button", { name: /implementation/i }));
    await userEvent.click(await screen.findByRole("button", { name: /Project rule/ }));
    await userEvent.type(await screen.findByLabelText("File content"), " changed");
    await userEvent.type(screen.getByLabelText("Why are you changing this?"), " reason");
    const confirm = vi.spyOn(window, "confirm");
    fireEvent.click(screen.getByRole("button", { name: "Save project file" }));
    // These clicks happen immediately after Save, before a React effect could publish saving.
    fireEvent.click(within(screen.getByRole("navigation", { name: "Projects" })).getByTitle("can sleep"));
    fireEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    fireEvent.click(screen.getByRole("button", { name: /add project folder/i }));
    expect(screen.getByLabelText("Project document editor")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "New task" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Add project folder")).not.toBeInTheDocument();
    expect(confirm).not.toHaveBeenCalled();

    resolveSave({ contentSha256: "new", notification: { status: "recorded", notifiedRuns: [], pendingRuns: [] } });
    await waitFor(() => expect(screen.queryByRole("button", { name: "Saving…" })).not.toBeInTheDocument());
    await userEvent.click(within(screen.getByRole("navigation", { name: "Projects" })).getByTitle("can sleep"));
    // #549: the editor closes when the selection lands, a render later; checked at once, a loaded
    // machine read it still open (the known flake of this cell). Wait for the condition instead.
    await waitFor(() => expect(screen.queryByLabelText("Project document editor")).not.toBeInTheDocument());
    // The saved file is not a draft: leaving it asks nothing. The prompt was the flake's cause
    // (the App still held "draft" for a render after the save), and it answers "no" in jsdom.
    expect(confirm).not.toHaveBeenCalled();
    confirm.mockRestore();
  });

  it("warns that an uncertain save may have committed before external navigation", async () => {
    const record = JSON.stringify({
      version: 1,
      projectId: "a".repeat(64),
      summary: "Updated project rule",
      reason: "The journey changed.",
      documents: [{ path: "docs/rule.md", title: "Project rule", kind: "business_rule", action: "updated" }],
    });
    const client = stubClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          { sequence: 2, kind: "execution_form_declared", payload: { executionId: "demo-deploy", nodeIds: ["implementation"] }, occurredAt: "2026-08-27T12:00:30Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k0", eventId: "event-2", evidenceRefs: [] },
          { sequence: 14, kind: "signal_recorded", payload: { kind: "node_delivery", sourceKind: "node", sourceId: "implementation" }, occurredAt: "2026-08-27T12:02:00Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k14", eventId: "event-14", evidenceRefs: ["delivery"] },
        ],
      })),
      readEvidence: vi.fn(async () => ({ evidenceId: "delivery", mediaType: "application/json", sensitivity: "internal", contentSha256: "hash", content: record })),
      saveDocument: vi.fn(async () => { throw new Error("network"); }),
    });
    await open(client);
    const board = await screen.findByLabelText("Steps without a bot");
    await userEvent.click(within(board).getByRole("button", { name: /implementation/i }));
    await userEvent.click(await screen.findByRole("button", { name: /Project rule/ }));
    await userEvent.type(await screen.findByLabelText("File content"), " changed");
    await userEvent.type(screen.getByLabelText("Why are you changing this?"), " reason");
    await userEvent.click(screen.getByRole("button", { name: "Save project file" }));
    await screen.findByRole("alert");
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    expect(confirm).toHaveBeenCalledWith(expect.stringMatching(/may have committed.*retry key.*pending run notifications/i));
    expect(confirm).toHaveBeenCalledWith(expect.stringMatching(/unsaved text.*discarded/i));
    expect(screen.getByLabelText("Project document editor")).toBeInTheDocument();

    confirm.mockClear();
    await userEvent.click(screen.getByRole("button", { name: /^disconnect$/i }));
    await userEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }));
    expect(confirm).toHaveBeenCalledWith(expect.stringMatching(/may have committed.*retry key.*pending run notifications/i));
    expect(screen.getByLabelText("Project document editor")).toBeInTheDocument();
    expect(screen.queryByLabelText(/bearer token/i)).not.toBeInTheDocument();
    confirm.mockRestore();
  });
});

describe("operator actions", () => {
  it("records a button press as the owner and shows the verified evidence", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: /pause · finish in-flight/i }));

    await waitFor(() => expect(client.pause).toHaveBeenCalled());
    const [, options] = client.pause.mock.calls[0] as unknown as [string, { actor: { type: string } }];
    expect(options.actor.type).toBe("owner");
    expect(await screen.findByText(/paused — done/i)).toBeInTheDocument();
  });

  /** Resume still never fires without a path — but instead of sitting disabled with its excuse
   * in a tooltip a disabled button never shows, it walks the person to the box. The mutation
   * gate is asserted in "resume with no path walks you to the box". Opened PAUSED: Phase 2's
   * legality map disables resume on a running run, so the path gate is only reachable where
   * resume is legal at all. */
  it("never resumes without a graph path", async () => {
    const client = stubClient({ getStatus: vi.fn(async () => ({ ...STATUS, status: "paused" })) });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: /^resume$/i }));
    expect(client.resume).not.toHaveBeenCalled();
  });

  it("says plainly when a write could not be verified", async () => {
    const client = stubClient({
      pause: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        result: "unknown" as const,
        headAfter: null,
        statusAfter: null,
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: /pause · finish in-flight/i }));
    expect(await screen.findByText(/do not treat this as done/i)).toBeInTheDocument();
  });

  /** Phase 2 (#105): the runtime's TWO pauses are two buttons with two promises, and the
   * interrupting one reaches the client through its own method - never a default. */
  it("keeps the two pauses apart: interrupt fires its own verb", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: /pause now · interrupt/i }));
    await waitFor(() => expect(client.pauseImmediately).toHaveBeenCalled());
    expect(client.pause).not.toHaveBeenCalled();
  });

  /** THE HEAD THE DOCK RENDERED rides as If-Match (L's follow-up on #662, App.tsx:1615): the
   * operator's button is the caller that must send the head it displayed. Rendered 13, the
   * stream at 14 by the time the click lands -> the client is called with ifMatch 13, the
   * Runtime's conflict comes back refused, and the REASON is on screen, nothing interrupted. */
  it("pins every dock verb to the head it rendered, and shows the conflict when the run moved", async () => {
    const client = stubClient({
      sweep: vi.fn(async () => ({ ...PAUSED_EVIDENCE, action: "sweep" as const, statusAfter: { ...STATUS, headSequence: 14 } })),
      pause: vi.fn(async () => ({ ...PAUSED_EVIDENCE, headBefore: 14, headAfter: 15, statusAfter: { ...STATUS, status: "paused", headSequence: 15 } })),
      pauseImmediately: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        actor: { id: "studio-operator", type: "owner" as const },
        result: "refused" as const,
        headAfter: 14,
        statusAfter: null,
        newEvents: [],
        diagnostics: [
          { code: "GHCLI409_PRECONDITION_FAILED", message: "If-Match 13 does not match the current head 14 - the run moved since you looked", path: "/If-Match", severity: "error", source: "serve" },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: /pause now · interrupt/i }));
    await waitFor(() => expect(client.pauseImmediately).toHaveBeenCalled());
    const optionsOf = (calls: unknown[][], index: number) =>
      (calls[0] as unknown as Array<{ ifMatch?: number }>)[index];
    expect(optionsOf(vi.mocked(client.pauseImmediately).mock.calls, 1).ifMatch).toBe(STATUS.headSequence);
    expect(await screen.findByText(/the run moved since you looked/)).toBeInTheDocument();

    // Each verb uses the head currently rendered. Sweeping preserves running and advances to
    // head 14; its older follow-up read must not put the dock back on head 13.
    await userEvent.click(screen.getByRole("button", { name: "sweep" }));
    await waitFor(() => expect(client.sweep).toHaveBeenCalled());
    expect(optionsOf(vi.mocked(client.sweep).mock.calls, 1).ifMatch).toBe(STATUS.headSequence);
    await userEvent.click(screen.getByRole("button", { name: /pause · finish in-flight/i }));
    await waitFor(() => expect(client.pause).toHaveBeenCalled());
    expect(optionsOf(vi.mocked(client.pause).mock.calls, 1).ifMatch).toBe(14);
    expect(client.approve).not.toHaveBeenCalled();
  });

  /** Cancel is destructive on an append-only log, so the first press only ASKS - in the page,
   * where this test can walk the question - and "keep running" backs out without a call. */
  it("cancels only through the in-place confirmation", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: /Cancel execution/i }));
    expect(client.cancel).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: /keep running/i }));
    expect(client.cancel).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: /Cancel execution/i }));
    await userEvent.click(screen.getByRole("button", { name: /yes, cancel it/i }));
    await waitFor(() => expect(client.cancel).toHaveBeenCalled());
  });

  /** An armed cancel must die with the session: disconnect() did not reset it and openWith()'s
   * connection-level pick bypasses select(), so a reconnect - possibly to another Runtime -
   * opened with "yes, cancel it" one click away (PR #662 review, P1). */
  it("disarms the cancel confirmation across a disconnect and reconnect", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: /Cancel execution/i }));
    expect(screen.getByRole("button", { name: /yes, cancel it/i })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /^disconnect$/i }));
    await userEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }));
    await screen.findByLabelText(/bearer token/i);
    await userEvent.type(screen.getByLabelText(/bearer token/i), "local-token");
    await userEvent.click(screen.getByRole("button", { name: /connect/i }));
    await screen.findByLabelText("Projects");

    expect(screen.queryByRole("button", { name: /yes, cancel it/i })).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /Cancel execution/i })).toBeInTheDocument();
    expect(client.cancel).not.toHaveBeenCalled();
  });

  /** The armed confirmation follows the recomputed legality: a poll tick that finishes the run
   * while the question stands withdraws the "yes" - the outer button then wears the reason, and
   * nothing is sent (PR #662 review, App.tsx:1687). */
  it("withdraws the cancel confirmation when a poll finishes the run", async () => {
    // The TEST decides when the run finishes - after the confirmation is armed - so the poll
    // cannot win the race against a slow click and flip the run before the question is asked.
    let finished = false;
    const client = stubClient({
      getStatus: vi.fn(async () =>
        finished
          ? { ...STATUS, status: "completed", attention: "can_sleep", attentionReasons: [] }
          : { ...STATUS },
      ),
    });
    render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={40}
      />,
    );
    await screen.findByLabelText("Projects");
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(await screen.findByRole("button", { name: /Cancel execution/i }));
    expect(screen.getByRole("button", { name: /yes, cancel it/i })).toBeInTheDocument();

    // Now the run finishes underneath the open question; the next poll tick brings it.
    finished = true;
    await waitFor(() => expect(screen.queryByRole("button", { name: /yes, cancel it/i })).not.toBeInTheDocument());
    // #1083: an ended run's dock no longer offers cancel at all, and says why in plain text.
    expect(screen.queryByRole("button", { name: /Cancel execution/i })).not.toBeInTheDocument();
    expect(screen.getByText(/This run is completed: nothing is left to pause, resume or cancel/)).toBeInTheDocument();
    expect(client.cancel).not.toHaveBeenCalled();
  });

  /* #503: on a big run a quiet tick (nothing new) still cost a ~110-130 ms main-thread block: every
   * 4 s poll answered with fresh-but-equal status and run-list objects, so the whole Studio
   * re-rendered. A tick that brings nothing new must not commit. Cost: jsdom, ~1.5 s. */
  it("a quiet poll tick does not re-render the Studio", async () => {
    const client = stubClient();
    let commits = 0;
    render(
      <Profiler id="studio" onRender={() => { commits += 1; }}>
        <App
          createClient={() => client as unknown as RuntimeClient}
          modelContext={null}
          session={async () => ({ token: "local-token", project: "dale-api-base" })}
          pollIntervalMs={40}
        />
      </Profiler>,
    );
    await screen.findByLabelText("Projects");
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await new Promise((resolve) => setTimeout(resolve, 600));
    const before = commits;
    const ticksBefore = client.getStatus.mock.calls.length;
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(client.getStatus.mock.calls.length - ticksBefore).toBeGreaterThanOrEqual(5);
    expect(commits - before).toBeLessThanOrEqual(2);
  });

  /* #503: a hidden tab polled as often as a visible one. Cost: jsdom, ~1.5 s. */
  it("a hidden tab polls every fourth tick and resumes when shown", async () => {
    const client = stubClient();
    render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={40}
      />,
    );
    await screen.findByLabelText("Projects");
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await new Promise((resolve) => setTimeout(resolve, 300));
    const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    try {
      const hiddenStart = client.getStatus.mock.calls.length;
      await new Promise((resolve) => setTimeout(resolve, 800));
      const hidden = client.getStatus.mock.calls.length - hiddenStart;
      visibility.mockReturnValue("visible");
      const shownStart = client.getStatus.mock.calls.length;
      await new Promise((resolve) => setTimeout(resolve, 800));
      const shown = client.getStatus.mock.calls.length - shownStart;
      expect(hidden).toBeLessThanOrEqual(Math.ceil(shown / 4) + 1);
      expect(shown).toBeGreaterThanOrEqual(8);
    } finally {
      visibility.mockRestore();
    }
  });

  it("sweeps on request", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: /^sweep$/i }));
    await waitFor(() => expect(client.sweep).toHaveBeenCalled());
  });

  /** A verb the state makes illegal renders disabled WITH ITS REASON on the wrapping span -
   * a disabled button never shows its own title, and a live button that bounces off the API's
   * refusal would pretend (Phase 2 honesty rule). */
  /** #1083 (orchestrator verification): a COMPLETED run's dock still offered both pauses, resume
   * and cancel. On an ended run they are gone, the reason is plain text, and what the Runtime
   * still accepts stays: `sweep.rs` refuses no lifecycle state. A running run keeps them all. */
  for (const terminal of ["completed", "failed", "cancelled"]) {
    it(`drops pause, resume and cancel on a ${terminal} run, says why, and keeps sweep`, async () => {
      const client = stubClient({
        getStatus: vi.fn(async () => ({ ...STATUS, status: terminal, attention: "can_sleep", attentionReasons: [] })),
      });
      await open(client);
      expect(await screen.findByText(new RegExp(`This run is ${terminal}: nothing is left to pause, resume or cancel`))).toBeInTheDocument();
      for (const gone of [/pause · finish in-flight/i, /pause now · interrupt/i, /^resume$/i, /Cancel execution/i]) {
        expect(screen.queryByRole("button", { name: gone })).not.toBeInTheDocument();
      }
      await userEvent.click(screen.getByRole("button", { name: /^sweep$/i }));
      await waitFor(() => expect(client.sweep).toHaveBeenCalled());
    });
  }

  it("keeps every verb on a running run", async () => {
    await open(stubClient());
    for (const kept of [/pause · finish in-flight/i, /pause now · interrupt/i, /^resume$/i, /Cancel execution/i, /^sweep$/i]) {
      expect(await screen.findByRole("button", { name: kept })).toBeInTheDocument();
    }
    expect(screen.queryByText(/nothing is left to pause, resume or cancel/)).not.toBeInTheDocument();
  });

  /** The attention verdict's own remedy, served where it stands: a silence with a
   * declareNodeBudget remedy gets an input and a button, seconds is the OPERATOR's number
   * (no default anywhere on the path), and computedAtSequence is copied from the verdict. */
  it("offers the declare-budget remedy and sends the operator's own seconds", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        silenceUnevaluated: [
          {
            scope: "node",
            node: "judge",
            reason: "no_declared_budget",
            remedy: {
              remedy: "declareNodeBudget",
              node: "judge",
              observedSilenceSeconds: 240,
              computedAtSequence: 13,
            },
          },
        ],
      })),
    });
    await open(client);
    const seconds = await screen.findByLabelText(/silence budget for judge/i);
    const declare = screen.getByRole("button", { name: /declare budget for judge/i });
    expect(declare).toBeDisabled();
    // One over the envelope schema's bound: the button stays OFF wearing the reason, and the
    // client is never asked (PR #662 review, App.tsx:1743) - the bound is imported, not typed.
    await userEvent.type(seconds, String(MAX_NODE_TIMEOUT_SECONDS + 1));
    expect(declare).toBeDisabled();
    expect(declare).toHaveAttribute("title", expect.stringContaining(String(MAX_NODE_TIMEOUT_SECONDS)));
    expect(seconds).toHaveAttribute("max", String(MAX_NODE_TIMEOUT_SECONDS));
    expect(client.amendBudget).not.toHaveBeenCalled();
    await userEvent.clear(seconds);
    await userEvent.type(seconds, "900");
    await userEvent.click(declare);
    await waitFor(() =>
      expect(client.amendBudget).toHaveBeenCalledWith(
        "demo-deploy",
        { node: "judge", seconds: 900, computedAtSequence: 13 },
        expect.anything(),
      ),
    );
  });

  /** Codex on PR #1091: when the remedies went from non-empty to empty, the shared dock ref got
   * `null`, could not say which dock left, and kept the still-connected remedy dock measured - so
   * `--dock-reserve` stayed at the remedy's size. The reserve must return to the value it had
   * with the main dock alone. Heights are set per dock, so the two values are distinguishable. */
  it("puts the dock reserve back when the remedy dock goes away", async () => {
    let remedies = false;
    const remedy = {
      scope: "node",
      node: "judge",
      reason: "no_declared_budget",
      remedy: { remedy: "declareNodeBudget", node: "judge", observedSilenceSeconds: 240, computedAtSequence: 13 },
    };
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, silenceUnevaluated: remedies ? [remedy] : [] })),
    });
    const original = HTMLElement.prototype.getBoundingClientRect;
    const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      const height = this.classList.contains("remedies") ? 125 : this.classList.contains("dock") ? 82 : null;
      if (height === null) return original.call(this);
      return { x: 0, y: 0, top: 0, left: 0, right: 600, bottom: height, width: 600, height, toJSON() {} } as DOMRect;
    });
    try {
      render(
        <App
          createClient={() => client as unknown as RuntimeClient}
          modelContext={null}
          session={async () => ({ token: "local-token", project: "dale-api-base" })}
          pollIntervalMs={40}
        />,
      );
      await screen.findByLabelText("Projects");
      await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
      const reserve = () =>
        (document.querySelector(".scene") as HTMLElement | null)?.style.getPropertyValue("--dock-reserve") ?? "";
      await waitFor(() => expect(reserve()).not.toBe(""));
      const base = reserve();

      remedies = true;
      await screen.findByLabelText(/silence budget for judge/i);
      await waitFor(() => expect(reserve()).not.toBe(base));

      remedies = false;
      await waitFor(() => expect(screen.queryByLabelText(/silence budget for judge/i)).not.toBeInTheDocument());
      await waitFor(() => expect(reserve()).toBe(base));
    } finally {
      rect.mockRestore();
    }
  });
});

describe("the shared surface", () => {
  it("stays silent about site tools, and the page still works, without a model context", async () => {
    await open(stubClient(), null);
    expect(screen.getByRole("button", { name: /pause · finish in-flight/i })).toBeEnabled();
    // No chips and no announcement: the absence of tools is not an error to report.
    expect(document.querySelector(".toolchips")).toBeNull();
    expect(screen.queryByText("no site tools")).not.toBeInTheDocument();
  });

  it("registers the thirteen site tools once a connection exists, and not before", async () => {
    const { modelContext, registered } = fakeModelContext();
    render(
      <App
        createClient={() => stubClient() as unknown as RuntimeClient}
        modelContext={modelContext}
        session={async () => null}
      />,
    );
    await screen.findByLabelText(/bearer token/i);
    expect(registered).toHaveLength(0);

    await userEvent.type(screen.getByLabelText(/bearer token/i), "local-token");
    await userEvent.click(screen.getByRole("button", { name: /connect/i }));
    await screen.findByLabelText("Projects");

    expect(registered).toHaveLength(13);
    expect(await screen.findByText("pause_execution")).toBeInTheDocument();
  });

  /**
   * THE CLAIM THE WHOLE FEATURE RESTS ON. An agent's tool call must move the page the person is
   * looking at — not a copy of it, not on the next manual refresh.
   */
  it("moves the human view when an agent's tool writes", async () => {
    const { modelContext, registered } = fakeModelContext();
    const client = stubClient();
    await open(client, modelContext);

    const pauseTool = registered.find((tool) => tool.name === "graphhelm_pause_execution")!;
    await pauseTool.execute({ executionId: "demo-deploy" });

    // The actor on screen is the AGENT, even though a person confirmed the call in the browser.
    expect(await screen.findByText(/by agent:studio-webmcp-adapter/)).toBeInTheDocument();
  });

  it("forgets the token and removes the tools on disconnect", async () => {
    const { modelContext, registered } = fakeModelContext();
    const client = stubClient();
    await open(client, modelContext);

    // Two clicks, on purpose: disconnect erases the token and every stored board, and it sits
    // one slip away from Refresh.
    await userEvent.click(screen.getByRole("button", { name: /^disconnect$/i }));
    await userEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }));

    expect(client.dispose).toHaveBeenCalled();
    expect(await screen.findByLabelText(/bearer token/i)).toBeInTheDocument();
    const listTool = registered.find((tool) => tool.name === "graphhelm_list_executions")!;
    const reply = JSON.parse(await listTool.execute({}));
    expect(reply.ok).toBe(false);
  });
});

describe("starting a new task", () => {
  const ROUTES = {
    configured: true,
    routes: [
      {
        id: "fast_route",
        provider: "anthropic",
        transport: "direct_api",
        billingMode: "per_token",
        baseUrl: "https://api.anthropic.com",
        credentialRef: "secret_fast_route",
        model: "claude-sonnet-5",
        profiles: ["critical_reasoning"],
        enabled: true,
      },
      {
        id: "retired_route",
        provider: "anthropic",
        transport: "direct_api",
        billingMode: "per_token",
        baseUrl: "https://api.anthropic.com",
        credentialRef: "secret_retired_route",
        model: "claude-haiku-4-5",
        profiles: [],
        enabled: false,
      },
    ],
  };

  function withRoutes(overrides: Record<string, unknown> = {}) {
    return stubClient({
      listRoutes: vi.fn(async () => ROUTES),
      ...overrides,
    });
  }

  /** The canvas a new task opens on: the start node and nothing else. The operator asked for an
   * empty board with one node, and an empty board with one node is a claim - a board that still
   * showed the previous run's nodes would say this task already has a shape. */
  it("opens an empty canvas holding only the start node", async () => {
    const client = withRoutes();
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task/i }));

    expect(await screen.findByRole("heading", { name: "New task" })).toBeInTheDocument();
    const sheet = document.querySelector(".sheet") as HTMLElement;
    expect(sheet.querySelectorAll(".node")).toHaveLength(1);
    expect(within(sheet).getByText("start")).toBeInTheDocument();
  });

  /** The picker is the RUNTIME's list. A disabled route is shown and unselectable rather than
   * hidden: "my model is missing" and "my model is turned off" send an operator to two different
   * places, and hiding it makes the second look like the first. */
  it("offers the Runtime's own models, showing a disabled one as disabled", async () => {
    const client = withRoutes();
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task/i }));

    const picker = (await screen.findByLabelText("Model")) as HTMLSelectElement;
    expect(client.listRoutes).toHaveBeenCalled();
    expect(within(picker).getByRole("option", { name: /claude-sonnet-5/ })).not.toBeDisabled();
    expect(within(picker).getByRole("option", { name: /claude-haiku-4-5/ })).toBeDisabled();
  });

  it("writes the route before the key and keeps Models busy for both writes", async () => {
    let releaseRoute!: () => void;
    let releaseKey!: () => void;
    const order: string[] = [];
    const client = withRoutes({
      setRoute: vi.fn(() => { order.push("route-start"); return new Promise((resolve) => { releaseRoute = () => { order.push("route-done"); resolve({ routes: ROUTES.routes }); }; }); }),
      setCredential: vi.fn(() => { order.push("key-start"); return new Promise((resolve) => { releaseKey = () => { order.push("key-done"); resolve({ id: "secret_fast_route", routes: ["fast_route"] }); }; }); }),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /^models$/i }));
    await userEvent.click(screen.getByRole("button", { name: /Edit fast_route/i }));
    await userEvent.type(screen.getByLabelText(/API key/i), "fresh-key");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    await waitFor(() => expect(order).toEqual(["route-start"]));
    expect(screen.getByRole("button", { name: "applying" })).toBeDisabled();
    releaseRoute();
    await waitFor(() => expect(order).toEqual(["route-start", "route-done", "key-start"]));
    expect(screen.getByRole("button", { name: "applying" })).toBeDisabled();
    releaseKey();
    await waitFor(() => expect(order).toEqual(["route-start", "route-done", "key-start", "key-done"]));
  });

  it("does not write a key when the route is refused and keeps the error visible", async () => {
    const client = withRoutes({
      setRoute: vi.fn(async () => { throw new RuntimeError("route refused", 400, []); }),
      setCredential: vi.fn(),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /^models$/i }));
    await userEvent.click(screen.getByRole("button", { name: /Edit fast_route/i }));
    await userEvent.type(screen.getByLabelText(/API key/i), "never-send");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("route refused");
    expect(screen.getByRole("form", { name: "Edit route" })).toBeInTheDocument();
    expect(client.setCredential).not.toHaveBeenCalled();
  });

  it("does not call Runtime when adding a model without an API key", async () => {
    const client = withRoutes({
      setRoute: vi.fn(async () => ({ routes: ROUTES.routes })),
      setCredential: vi.fn(),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /^models$/i }));
    await userEvent.click(screen.getByRole("button", { name: /Add model/i }));
    await userEvent.type(screen.getByLabelText("Route id"), "new_route");
    await userEvent.type(screen.getByLabelText("Base URL"), "https://api.example.com");
    await userEvent.type(screen.getByLabelText("Model"), "example-model");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/new model needs an API key/i);
    expect(client.setRoute).not.toHaveBeenCalled();
    expect(client.setCredential).not.toHaveBeenCalled();
  });

  it("keeps a failed key draft retryable after the route was saved", async () => {
    const client = withRoutes({
      setRoute: vi.fn(async () => ({ routes: ROUTES.routes })),
      setCredential: vi.fn()
        .mockRejectedValueOnce(new RuntimeError("key refused", 400, []))
        .mockResolvedValueOnce({ id: "secret_fast_route", routes: ["fast_route"] }),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /^models$/i }));
    await userEvent.click(screen.getByRole("button", { name: /Edit fast_route/i }));
    await userEvent.type(screen.getByLabelText(/API key/i), "retry-me");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/route was saved, but the API key was not stored/i);
    expect(screen.getByLabelText(/API key/i)).toHaveValue("retry-me");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    await waitFor(() => expect(client.setCredential).toHaveBeenCalledTimes(2));
    expect(client.setRoute).toHaveBeenCalledTimes(2);
  });

  it("does not call a key write or paint stale save state after disconnect", async () => {
    let releaseRoute!: (value: { routes: typeof ROUTES.routes }) => void;
    const client = withRoutes({
      setRoute: vi.fn(() => new Promise<{ routes: typeof ROUTES.routes }>((resolve) => { releaseRoute = resolve; })),
      setCredential: vi.fn(),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /^models$/i }));
    await userEvent.click(screen.getByRole("button", { name: /Edit fast_route/i }));
    await userEvent.type(screen.getByLabelText(/API key/i), "must-not-cross-runtime");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    await waitFor(() => expect(client.setRoute).toHaveBeenCalled());
    await userEvent.click(screen.getByRole("button", { name: /^disconnect$/i }));
    await userEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }));
    releaseRoute({ routes: ROUTES.routes });
    await waitFor(() => expect(client.dispose).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(client.setCredential).not.toHaveBeenCalled();
    expect(screen.queryByText(/route was saved, but the API key was not stored/i)).not.toBeInTheDocument();
  });

  it("ignores an older probe after a newer probe for the same route", async () => {
    const replies: Array<(value: { health: string }) => void> = [];
    const client = withRoutes({
      probeRoute: vi.fn(() => new Promise<{ health: string }>((resolve) => replies.push(resolve))),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /^models$/i }));
    const card = screen.getByText("fast_route").closest("li") as HTMLElement;
    const check = within(card).getByRole("button", { name: "check" });
    await userEvent.click(check);
    await userEvent.click(check);
    replies[1]({ health: "auth_required" });
    await waitFor(() => expect(screen.getByRole("status", { name: /auth_required/i })).toBeInTheDocument());
    replies[0]({ health: "available" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.getByRole("status", { name: /auth_required/i })).toBeInTheDocument();
  });

  it("does not let a probe from before a route save restore a stale result", async () => {
    let finishProbe!: (value: { health: string }) => void;
    const client = withRoutes({
      probeRoute: vi.fn(() => new Promise<{ health: string }>((resolve) => { finishProbe = resolve; })),
      setRoute: vi.fn(async () => ({ routes: ROUTES.routes })),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /^models$/i }));
    const card = screen.getByText("fast_route").closest("li") as HTMLElement;
    await userEvent.click(within(card).getByRole("button", { name: "check" }));
    await userEvent.click(screen.getByRole("button", { name: /Edit fast_route/i }));
    await userEvent.clear(screen.getByLabelText("Model"));
    await userEvent.type(screen.getByLabelText("Model"), "new-model");
    await userEvent.click(screen.getByRole("button", { name: "apply" }));
    finishProbe({ health: "available" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(within(card).getByRole("status", { name: "probe: not checked" })).toBeInTheDocument();
  });

  /** THE WHOLE POINT: the operator's sentence becomes the first node's objective, verbatim, and
   * the model they picked is the one sent. A surface that summarised the message, or that quietly
   * sent the server default, would be answering a different request than the one made. */
  it("sends the objective verbatim on the chosen model", async () => {
    const client = withRoutes();
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task/i }));

    await userEvent.selectOptions(await screen.findByLabelText("Model"), "fast_route");
    await userEvent.type(
      screen.getByLabelText(/what should this task do/i),
      "Map the repository and write the first graph.",
    );
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));

    await waitFor(() => expect(client.startTask).toHaveBeenCalled());
    const [executionId, graph, options] = firstCall(client.startTask);
    expect(executionId).toMatch(/^run-/);
    expect(options.route).toBe("fast_route");
    const nodes = (graph as { spec: { nodes: Record<string, { objective: string }> } }).spec.nodes;
    expect(nodes.start.objective).toBe("Map the repository and write the first graph.");
  });

  /** Not picking a model sends NO route, rather than sending a null one. Absent means "the
   * server's own default"; a present-but-null field is refused by the Runtime, correctly, because
   * it cannot tell that from a client that meant something by it. */
  it("omits the route entirely when no model was picked", async () => {
    const client = withRoutes();
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task/i }));
    await userEvent.type(await screen.findByLabelText(/what should this task do/i), "Do the thing.");
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));

    await waitFor(() => expect(client.startTask).toHaveBeenCalled());
    expect(firstCall(client.startTask)[2].route).toBeNull();
  });

  /**
   * THE WARNING THAT USED TO ARRIVE TOO LATE.
   *
   * A Runtime with no gateway manifest accepts the task, parks the first node in `waiting_input`
   * and never calls a model. The Runtime does say so - `GHCLI021_FIXTURE_ONLY_WAITING_INPUT` - but
   * only in the reply to the mutation, which means an operator learns it after sending and while
   * waiting for something that will never come. Measured by running a one-node graph against a
   * default `serve`: `attention: needs_you`, `waiting_input: 1`, zero model calls.
   */
  it("says the Runtime is fixture-only BEFORE anything is sent", async () => {
    const client = withRoutes({
      listRoutes: vi.fn(async () => ({ configured: false, routes: [] })),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task/i }));

    expect(await screen.findByText(/no model is wired/i)).toBeInTheDocument();
    expect(client.startTask).not.toHaveBeenCalled();
  });

  /** A refused start keeps the operator's text on screen. Clearing the draft first would lose what
   * they wrote to a refusal they did not cause, and they would have to retype it to retry. */
  it("keeps the composed text when the Runtime refuses the start", async () => {
    const client = withRoutes({
      startTask: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        action: "start" as const,
        result: "refused" as const,
        diagnostics: [
          { code: "GHCLI001_ARGUMENT_INVALID", message: "that route is not in the manifest", path: "/route", severity: "error", source: "serve-cli" },
        ],
      })),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task/i }));
    await userEvent.type(await screen.findByLabelText(/what should this task do/i), "Keep me.");
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));

    expect(await screen.findByText(/that route is not in the manifest/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/what should this task do/i)).toHaveValue("Keep me.");
  });
});

describe("the shell's own layout", () => {
  // The rail width is deliberately persisted, which means it deliberately survives a test. Each
  // of these starts from the default, or the second one measures the first one's drag.
  beforeEach(() => window.localStorage.removeItem("graphhelm.studio.rail-width"));

  /**
   * THE BAR YOU COULD NOT RESIZE. The rail was `grid-template-columns: 268px ...` - a constant,
   * with no control anywhere that changed it. The width now rides a custom property the grip
   * writes, so this asserts the property rather than a computed pixel: jsdom lays nothing out, and
   * a test that asked for a rendered width here would be asserting on jsdom's defaults.
   */
  it("resizes the rail when the grip is dragged, and remembers where it was left", async () => {
    await open(stubClient());
    const grip = screen.getByRole("separator", { name: /resize the projects rail/i });
    const shell = document.querySelector(".app") as HTMLElement;

    fireEvent.pointerDown(grip, { clientX: 268 });
    fireEvent.pointerMove(window, { clientX: 340 });
    expect(shell.style.getPropertyValue("--rail")).toBe("340px");
    expect(grip).toHaveAttribute("aria-valuenow", "340");

    fireEvent.pointerUp(window);
    expect(window.localStorage.getItem("graphhelm.studio.rail-width")).toBe("340");
  });

  /** A drag past either end must not store a width the next session cannot recover from - the
   * rail has to stay usable, and so does the board beside it. */
  it("refuses a width that would make either side unusable", async () => {
    await open(stubClient());
    const grip = screen.getByRole("separator", { name: /resize the projects rail/i });
    const shell = document.querySelector(".app") as HTMLElement;

    fireEvent.pointerDown(grip, { clientX: 268 });
    fireEvent.pointerMove(window, { clientX: 20 });
    expect(shell.style.getPropertyValue("--rail")).toBe("200px");

    fireEvent.pointerMove(window, { clientX: 4000 });
    expect(shell.style.getPropertyValue("--rail")).toBe("460px");
    fireEvent.pointerUp(window);
  });

  /**
   * THE HOLD MUST BE RELEASED. This is the same defect that shipped on the board: listeners
   * installed from the pointerdown handler, with the drag flag in a ref that never re-renders, so
   * pointerup was never wired and the next press anywhere kept resizing. A move after a release
   * must move nothing.
   */
  it("stops resizing when the pointer is released", async () => {
    await open(stubClient());
    const grip = screen.getByRole("separator", { name: /resize the projects rail/i });
    const shell = document.querySelector(".app") as HTMLElement;

    fireEvent.pointerDown(grip, { clientX: 268 });
    fireEvent.pointerMove(window, { clientX: 300 });
    fireEvent.pointerUp(window);
    const parked = shell.style.getPropertyValue("--rail");

    fireEvent.pointerMove(window, { clientX: 440 });
    expect(shell.style.getPropertyValue("--rail")).toBe(parked);
  });

  /** The keyboard is not a second-class way to size a pane. */
  it("resizes from the keyboard", async () => {
    await open(stubClient());
    const grip = screen.getByRole("separator", { name: /resize the projects rail/i });
    const shell = document.querySelector(".app") as HTMLElement;

    grip.focus();
    fireEvent.keyDown(grip, { key: "ArrowRight" });
    expect(shell.style.getPropertyValue("--rail")).toBe("284px");
    fireEvent.keyDown(grip, { key: "ArrowLeft" });
    expect(shell.style.getPropertyValue("--rail")).toBe("268px");
  });


  /** The button exists because a project is a folder and the operator has more than one. What it
   * can do today is tell them exactly what to run; it must not pretend to do more. */
  it("says what adding a project folder actually takes", async () => {
    await open(stubClient());
    await userEvent.click(screen.getByRole("button", { name: /add project folder/i }));

    const panel = await screen.findByLabelText("Add project folder");
    await userEvent.type(within(panel).getByLabelText(/^folder$/i), "F:/projects/example/dale-api-base");
    expect(
      within(panel).getByText(
        /Set-Location -LiteralPath 'F:\/projects\/example\/dale-api-base'; if \(\$\?\) \{ graphhelm serve --events \.graphhelm\/events --bind 127\.0\.0\.1:8791 \}/i,
      ),
    ).toBeInTheDocument();
    expect(
      within(panel).getByText(
        /cd 'F:\/projects\/example\/dale-api-base' && graphhelm serve --events \.graphhelm\/events --bind 127\.0\.0\.1:8791/i,
      ),
    ).toBeInTheDocument();
    expect(within(panel).getByLabelText(/^folder$/i)).toHaveValue("F:/projects/example/dale-api-base");
  });
});

/**
 * The conversation moves on its own.
 *
 * A person sent a message from this page and stared at a silent board (2026-08-30): nothing said
 * the message was waiting on anyone, and when the reply DID land in the log, the page would not
 * show it until they pressed refresh. A conversation surface that needs manual refresh is a log
 * viewer wearing a chat costume.
 */
describe("the conversation moves on its own", () => {
  function conversationEvents(withReply: boolean) {
    const base = [
      {
        sequence: 8,
        kind: "signal_recorded",
        payload: { executionId: "demo-deploy", kind: "operator_note", sourceKind: "user" },
        occurredAt: "2026-08-30T13:14:00Z",
        actorId: "studio-operator",
        actorType: "owner",
        idempotencyKey: "k8",
        eventId: "event-8",
        evidenceRefs: ["ev-oi"],
      },
    ];
    if (withReply) {
      base.push({
        sequence: 9,
        kind: "signal_recorded",
        payload: { executionId: "demo-deploy", kind: "operator_note", sourceKind: "user" },
        occurredAt: "2026-08-30T13:15:00Z",
        actorId: "claude-code",
        actorType: "agent",
        idempotencyKey: "k9",
        eventId: "event-9",
        evidenceRefs: ["ev-reply"],
      });
    }
    return base;
  }

  it("shows that a sent message is waiting on the other side", async () => {
    const client = stubClient({
      getEvents: vi.fn(async () => ({ head: 8, events: conversationEvents(false) })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    expect(await within(panel).findByText(/waiting for a reply/i)).toBeInTheDocument();
  });

  /** #1083: nobody replies on an ended run, so the receipt says the run has ended instead of
   * promising a reply that cannot come. */
  it("says the run has ended instead of waiting for a reply on a completed run", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, status: "completed", attention: "can_sleep", attentionReasons: [] })),
      getEvents: vi.fn(async () => ({ head: 8, events: conversationEvents(false) })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    expect(await within(panel).findByText(/delivered — this run is completed, so nobody is working on it to reply/)).toBeInTheDocument();
    expect(within(panel).queryByText(/waiting for a reply/i)).not.toBeInTheDocument();
  });

  it("drops the waiting line once the reply is in the log", async () => {
    const client = stubClient({
      getEvents: vi.fn(async () => ({ head: 9, events: conversationEvents(true) })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    await within(panel).findAllByText(/Said:/);
    expect(within(panel).queryByText(/waiting for a reply/i)).not.toBeInTheDocument();
  });

  /** THE REPLY ARRIVES WITHOUT ANYONE PRESSING REFRESH. The page polls the selected run; the
   * interval is a prop so this test does not wait wall-clock seconds. */
  it("shows the reply on its own, with no refresh click", async () => {
    let pages = 0;
    const client = stubClient({
      getEvents: vi.fn(async () => {
        pages += 1;
        return pages <= 1
          ? { head: 8, events: conversationEvents(false) }
          : { head: 9, events: conversationEvents(true) };
      }),
      readEvidence: vi.fn(async (_id: string, evidenceId: string) => ({
        evidenceId,
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        content: JSON.stringify({
          description: evidenceId === "ev-reply" ? "oi de volta" : "oi",
        }),
      })),
    });
    render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={40}
      />,
    );
    await screen.findByLabelText("Projects");
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    // No clicks after this point: the reply must surface by itself.
    expect(await within(within(panel).getByRole("region", { name: "Shared conversation" })).findByText("oi de volta")).toBeInTheDocument();
  });
});

/**
 * The verdict explains itself, and carries its own actions.
 *
 * "NEEDS YOU" alone sent a real person to ask "needs me FOR WHAT?" (2026-08-30) - the reasons
 * were in the status the page already held, and the deployment context (no model wired) was in
 * the routes list the page could already read. A verdict without its reason is a question, not
 * an answer; an answer without an action is homework.
 */
describe("the attention verdict explains itself", () => {
  it("shows an exhausted model crash as a retry decision, not approval of the original start", async () => {
    let paused = false;
    const event = (sequence: number, outcome: string, nextState: string, reason?: string) => ({
      sequence,
      kind: "node_outcome_recorded",
      payload: { nodeId: "start", outcome, nextState, ...(reason ? { reason } : {}) },
      occurredAt: "2026-08-27T12:01:00Z",
      actorId: "system-runtime",
      actorType: "system",
      idempotencyKey: `k-${sequence}`,
      eventId: `event-${sequence}`,
      evidenceRefs: [],
    });
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        status: paused ? "paused" : "running",
        headSequence: paused ? 13 : 12,
        attentionReasons: [{ kind: "blocked_node", node: "start" }],
        nodeStateCounts: { blocked: 1 },
      })),
      pause: vi.fn(async () => {
        paused = true;
        return { ...PAUSED_EVIDENCE, headBefore: 12, headAfter: 13, statusAfter: {
          ...STATUS, status: "paused", headSequence: 13,
          attentionReasons: [{ kind: "blocked_node", node: "start" }], nodeStateCounts: { blocked: 1 },
        } };
      }),
      getEvents: vi.fn(async () => ({
        head: 12,
        events: [
          event(3, "approved", "ready"),
          event(6, "retryable_failure", "queued", "runtime_crashed"),
          event(8, "retryable_failure", "queued", "runtime_crashed"),
          event(10, "retryable_failure", "queued", "runtime_crashed"),
          event(12, "retryable_failure", "blocked", "runtime_crashed"),
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Needs you");
    expect(why).toHaveTextContent(/Failed after retries: runtime crashed/i);
    expect(within(screen.getByLabelText(/^Run (?!side panel)/)).getByText(/waiting for your decision to retry a failed step/i)).toBeInTheDocument();
    expect(screen.getByText("blocked · retry decision")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "approve start" })).toBeNull();
    await userEvent.click(within(why).getByRole("button", { name: "pause before allowing retry start" }));
    await waitFor(() => expect(client.pause).toHaveBeenCalled());
    await userEvent.click(within(await screen.findByLabelText("Needs you")).getByRole("button", { name: "allow retry start" }));
    await waitFor(() => expect(client.approve).toHaveBeenCalled());
  });

  it("says in words why the run needs you, next to the tag", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Needs you");
    expect(within(why).getByText(/implementation is blocked/)).toBeInTheDocument();
  });

  /** The HEADLINE answers "needs me for what?" before the eye ever reaches the reason block:
   * the subtitle under "This run needs you" says what is owed, in words, not the wire state. */
  it("the header itself says what the run is waiting for", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    expect(within(panel).getByText("waiting for your go-ahead")).toBeInTheDocument();
  });

  it("pauses a running run before approval, then allows the approval", async () => {
    let paused = false;
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, status: paused ? "paused" : "running" })),
      pause: vi.fn(async () => {
        paused = true;
        return { ...PAUSED_EVIDENCE, statusAfter: { ...STATUS, status: "paused" } };
      }),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Needs you");
    await userEvent.click(within(why).getByRole("button", { name: /pause before approving implementation/i }));

    await waitFor(() => expect(client.pause).toHaveBeenCalled());
    expect(client.approve).not.toHaveBeenCalled();
    await userEvent.click(within(await screen.findByLabelText("Needs you")).getByRole("button", { name: /approve implementation/i }));
    await waitFor(() => expect(client.approve).toHaveBeenCalled());
    expect((vi.mocked(client.approve).mock.calls[0] as unknown as [string, string])[1]).toBe("implementation");
  });

  /** An immediate pause that interrupted a node leaves it `Blocked`/`Interrupted`; the Runtime
   * refuses resume until it is triaged, and approve IS the triage (PR #662 review,
   * legality.ts:66). So: resume off with the reason, approve offered on that node, and once the
   * approval lands and the list empties, resume comes back on. */
  it("gates resume on triage after an immediate pause, offers approve on the interrupted node, and frees resume once triaged", async () => {
    let triaged = false;
    const client = stubClient({
      getStatus: vi.fn(async () =>
        triaged
          ? { ...STATUS, status: "paused", attention: "can_sleep", attentionReasons: [], untriagedInterruptions: [], headSequence: 15 }
          : {
              ...STATUS,
              status: "paused",
              attentionReasons: [{ kind: "untriaged_interruption", node: "implementation" }],
              untriagedInterruptions: ["implementation"],
              headSequence: 14,
            },
      ),
      approve: vi.fn(async () => {
        triaged = true;
        return { ...PAUSED_EVIDENCE, action: "approve" as const, node: "implementation", headBefore: 14, headAfter: 15 };
      }),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const resume = await screen.findByRole("button", { name: "resume" });
    expect(resume).toBeDisabled();
    expect(resume.parentElement?.getAttribute("title")).toMatch(/triage/i);
    expect(resume.parentElement?.getAttribute("title")).toContain("implementation");

    const why = await screen.findByLabelText("Needs you");
    expect(within(why).getByText(/implementation was interrupted/)).toBeInTheDocument();
    await userEvent.click(within(why).getByRole("button", { name: /approve implementation/i }));
    await waitFor(() => expect(client.approve).toHaveBeenCalled());
    const call = vi.mocked(client.approve).mock.calls[0] as unknown as [string, string];
    expect(call[1]).toBe("implementation");

    await waitFor(() => expect(screen.getByRole("button", { name: "resume" })).toBeEnabled());
  });

  it("shows a graph wait as diagnostic when no request or model route is recorded", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        attentionReasons: [{ kind: "waiting_input_node", node: "start" }],
      })),
      listRoutes: vi.fn(async () => ({ configured: false, routes: [] })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    expect(screen.getByText("graph waiting; no request recorded")).toBeInTheDocument();
    expect(screen.queryByRole("article", { name: /asks$/ })).not.toBeInTheDocument();
    expect(client.getReplySuggestions).not.toHaveBeenCalled();
  });

  it("keeps manual room messaging available without inventing a waiting-input action", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        attentionReasons: [{ kind: "waiting_input_node", node: "start" }],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const box = await screen.findByLabelText(/say something into this run/i);
    expect(screen.queryByRole("article", { name: /asks$/ })).not.toBeInTheDocument();
    await userEvent.click(box);
    await waitFor(() => expect(box).toHaveFocus());
  });
});

/**
 * THE ANSWER PATH. Five independent review personas, blind to each other, found the same defect
 * (2026-08-30): the banner's "answer in the thread" dropped the operator into a composer still
 * locked to whoever they clicked earlier, so the promised answer shipped as a DM to the wrong
 * recipient. And the banner named the debtor ("start is waiting") but never the debt — the
 * question itself was nowhere on screen.
 */

/** A run whose thread carries a real question: codex asked the operator something, by envelope. */
function askedClient(overrides: Record<string, unknown> = {}) {
  const envelopes: Record<string, string> = {
    "ev-question": JSON.stringify({
      description: "Qual porta devo usar para o deploy?",
      to: "studio-operator",
    }),
    "ev-reply": JSON.stringify({ description: "usa a 8080", replyTo: "q-1" }),
  };
  return stubClient({
    getStatus: vi.fn(async () => ({
      ...STATUS,
      attentionReasons: [{ kind: "waiting_input_node", node: "start" }],
    })),
    getEvents: vi.fn(async () => ({
      head: 14,
      events: [
        {
          sequence: 13,
          kind: "signal_recorded",
          payload: { kind: "operator_note", signalId: "q-1", sourceKind: "agent" },
          occurredAt: "2026-08-27T12:01:00Z",
          actorId: "codex",
          actorType: "agent",
          idempotencyKey: "k13",
          eventId: "event-13",
          evidenceRefs: ["ev-question"],
        },
      ],
    })),
    readEvidence: vi.fn(async (_executionId: string, evidenceId: string) => ({
      evidenceId,
      mediaType: "application/json",
      sensitivity: "confidential",
      contentSha256: "sha256:whatever",
      content: envelopes[evidenceId] ?? "{}",
    })),
    ...overrides,
  });
}

describe("the answer path", () => {
  it("sends a manual room message after the earlier question was answered", async () => {
    // codex spoke (so a reply hint offers it) but its question is already answered — the banner
    // owes nothing to codex, and its action must speak to the ROOM, undoing any stale lock.
    const client = askedClient({
      getEvents: vi.fn(async () => ({
        head: 15,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "q-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: ["ev-question"],
          },
          {
            sequence: 14,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "a-1", sourceKind: "user" },
            occurredAt: "2026-08-27T12:02:00Z",
            actorId: "studio-operator",
            actorType: "owner",
            idempotencyKey: "k14",
            eventId: "event-14",
            evidenceRefs: ["ev-reply"],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    await screen.findByLabelText(/^Run demo-deploy/);
    expect(screen.queryByRole("article", { name: /asks$/ })).not.toBeInTheDocument();

    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "resposta pra sala");
    await userEvent.click(runSend());
    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    expect(firstCall(client.signal)[2]).not.toHaveProperty("to");
  });

  /** #410: landing on Needs you is the thread the owner is shown, so it is the one marked read.
   * The derived landing left the state on Everyone and silently marked the room's messages read. */
  it("lands on Needs you without marking the room's messages read", async () => {
    const room = { description: "Room: deploy window opens at 18:00" };
    const base = askedClient();
    const client = askedClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          { sequence: 12, kind: "signal_recorded", payload: { kind: "operator_note", signalId: "n-0", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:00:00Z", actorId: "codex", actorType: "agent", idempotencyKey: "k12", eventId: "event-12",
            evidenceRefs: ["ev-room"] },
          ...(await (base.getEvents as () => Promise<{ events: unknown[] }>)()).events,
        ],
      })),
      readEvidence: vi.fn(async (executionId: string, evidenceId: string) => evidenceId === "ev-room"
        ? { evidenceId, mediaType: "application/json", sensitivity: "confidential", contentSha256: "sha256:whatever", content: JSON.stringify(room) }
        : (base.readEvidence as (a: string, b: string) => Promise<unknown>)(executionId, evidenceId)),
    });
    await open(client);
    await screen.findByLabelText("Needs you");
    const chat = screen.getByRole("complementary", { name: "Chat" });
    expect(await within(chat).findByRole("tab", { name: /^Needs you/ })).toHaveAttribute("aria-selected", "true");
    expect(await within(chat).findByRole("tab", { name: "Everyone, 1 unread" })).toBeInTheDocument();
  });

  it("quotes the unanswered question and addresses its asker", async () => {
    const client = askedClient();
    await open(client);

    const why = await screen.findByLabelText("Needs you");
    // The debt itself, on screen - not just "waiting for input".
    expect(await within(why).findByText(/Qual porta devo usar para o deploy\?/)).toBeInTheDocument();

    // Answering from here delivers to the creditor.
    await userEvent.click(within(why).getByRole("button", { name: "Answer" }));
    const chat = screen.getByRole("complementary", { name: "Chat" });
    await userEvent.type(within(chat).getByLabelText("Message"), "usa a 8080");
    await userEvent.click(within(chat).getByRole("button", { name: "Send" }));
    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    expect(firstCall(client.signal)[2]).toMatchObject({ to: "codex" });
  });

  it("keeps the answer text and the Answering chip when the Runtime refuses the answer", async () => {
    const client = askedClient({
      signal: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        action: "signal" as const,
        result: "refused" as const,
        diagnostics: [{ code: "GHCLI001_ARGUMENT_INVALID", message: "not now", path: "/to", severity: "error", source: "serve-cli" }],
      })),
    });
    await open(client);
    const why = await screen.findByLabelText("Needs you");
    await within(why).findByText(/Qual porta devo usar para o deploy\?/);
    await userEvent.click(within(why).getByRole("button", { name: "Answer" }));
    const chat = screen.getByRole("complementary", { name: "Chat" });
    await userEvent.type(within(chat).getByLabelText("Message"), "usa a 8080");
    await userEvent.click(within(chat).getByRole("button", { name: "Send" }));
    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    expect(await within(chat).findByRole("alert")).toHaveTextContent("not now");
    expect(within(chat).getByLabelText("Message")).toHaveValue("usa a 8080");
    expect(within(chat).getByText(/^Answering/)).toBeInTheDocument();
  });

  it("an answered question is no longer owed", async () => {
    const client = askedClient({
      getEvents: vi.fn(async () => ({
        head: 15,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "q-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: ["ev-question"],
          },
          {
            sequence: 14,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "a-1", sourceKind: "user" },
            occurredAt: "2026-08-27T12:02:00Z",
            actorId: "studio-operator",
            actorType: "owner",
            idempotencyKey: "k14",
            eventId: "event-14",
            evidenceRefs: ["ev-reply"],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const conversation = await screen.findByRole("region", { name: "Shared conversation" });
    expect(await within(conversation).findByText(/Qual porta devo usar/)).toBeInTheDocument();
    expect(screen.queryByRole("article", { name: /asks$/ })).not.toBeInTheDocument();
  });
});

/**
 * THE THREAD TELLS THE TRUTH ABOUT WHAT IT SHOWS. An empty "Said:" bubble (the operator's own
 * message, screenshot 2026-08-30) is ambiguous between "the log holds nothing" and "the UI
 * swallowed it" — in an auditable console those must be distinct claims. And a sealed item the
 * store refused was a dead end: the words existed, the failure was transient, and the only way
 * back was remounting the panel.
 */
describe("the thread's own honesty", () => {
  it("a record that promises words but carries none says so, marked as elision", async () => {
    const client = stubClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "s-1", sourceKind: "user" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "studio-operator",
            actorType: "owner",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: [],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    expect(await screen.findByText(/no words attached to this record/)).toBeInTheDocument();
  });

  it("a sealed item the store refused is an alert with a way back in", async () => {
    let available = false;
    const client = stubClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "s-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: ["ev-flaky"],
          },
        ],
      })),
      readEvidence: vi.fn(async () => {
        if (!available) throw new Error("repository storage operation failed");
        return {
          evidenceId: "ev-flaky",
          mediaType: "application/json",
          sensitivity: "confidential",
          contentSha256: "sha256:whatever",
          content: '{"description":"agora abriu"}',
        };
      }),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const failure = await screen.findByRole("alert");
    expect(failure).toHaveTextContent(/cannot open/);
    available = true;
    await userEvent.click(screen.getByRole("button", { name: /try again/i }));
    expect(await screen.findByText("agora abriu")).toBeInTheDocument();
  });

  it("folds a run of machine narration into one strip the reader can open", async () => {
    const lifecycle = (sequence: number, kind: string, payload: Record<string, unknown>) => ({
      sequence,
      kind,
      payload,
      occurredAt: "2026-08-27T12:00:30Z",
      actorId: "system-runtime",
      actorType: "system",
      idempotencyKey: `k${sequence}`,
      eventId: `event-${sequence}`,
      evidenceRefs: [],
    });
    const client = stubClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          lifecycle(2, "execution_form_declared", { nodeIds: ["start"] }),
          lifecycle(3, "node_outcome_recorded", { nodeId: "start", outcome: "approved", nextState: "ready" }),
          lifecycle(4, "node_outcome_recorded", { nodeId: "start", outcome: "started", nextState: "queued" }),
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "s-1", sourceKind: "user" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "studio-operator",
            actorType: "owner",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: [],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    // One strip, counting its contents; the lines are still the log, one click away.
    expect(await screen.findByText(/3 lifecycle events/)).toBeInTheDocument();
  });
});

/**
 * CONTROLS THAT STOPPED LYING. "Disconnect" sat one slip away from "Refresh" and erased the token
 * and every stored board in one unconfirmed click; "resume" was disabled with its excuse in a
 * tooltip a disabled button never shows.
 */
describe("controls that act instead of excusing", () => {
  // The resume cells below type a graph file path, and the page REMEMBERS it with the run's board
  // in localStorage. Left behind, it opened `demo-deploy` in a later describe with the connect row
  // already showing and its "Verify graph" toggle gone (#1083: a cross-test leak, not a flake).
  afterEach(() => localStorage.clear());

  it("disconnect asks twice before erasing the session", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    await userEvent.click(screen.getByRole("button", { name: /^disconnect$/i }));
    // Still connected: the gate has not appeared, the button now says what a second click does.
    expect(screen.queryByLabelText(/bearer token/i)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }));
    expect(await screen.findByLabelText(/bearer token/i)).toBeInTheDocument();
  });

  it("resume with no path walks you to the box instead of sitting disabled with an excuse", async () => {
    // Paused, because Phase 2's legality map disables resume anywhere else - the walk-to-box
    // only exists where the verb is legal at all.
    const client = stubClient({ getStatus: vi.fn(async () => ({ ...STATUS, status: "paused" })) });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    await userEvent.click(screen.getByRole("button", { name: /^resume$/i }));
    expect(client.resume).not.toHaveBeenCalled();
    const box = screen.getByLabelText(/graph file path on the runtime host/i);
    await waitFor(() => expect(box).toHaveFocus());
  });


  it("resumes a Studio-created run with its durable one-node graph when no file is named", async () => {
    const executionId = "run-0f250aef-8a0a-4777-84ca-01f08ea55796";
    const objective = "Recover the paused task";
    const client = stubClient({
      listExecutions: vi.fn(async () => ({
        executions: [{ executionId, mode: "autopilot", status: "paused", attention: "can_sleep", startedAt: null, lastEventAt: null, headSequence: 13, objective }],
        hasMore: false,
        nextCursor: null,
      })),
      getStatus: vi.fn(async () => ({ ...STATUS, executionId, status: "paused", attention: "can_sleep", attentionReasons: [], headSequence: 13 })),
      getBriefing: vi.fn(async () => ({ name: "New task", objective, executor: "gateway", nextStep: { kind: "resume_held" }, asOfSequence: 13 })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: objective }));
    await userEvent.click(await screen.findByRole("button", { name: /^resume$/i }));
    await waitFor(() => expect(client.resume).toHaveBeenCalled());
    const graph = (client.resume.mock.calls[0] as unknown[])[1] as { metadata: { executionId: string }; spec: { nodes: { start: { objective: string } } } };
    expect(graph.metadata.executionId).toBe(executionId);
    expect(graph.spec.nodes.start.objective).toBe(objective);
  });

  it("resumes an accepted governed successor from its verified sealed snapshot", async () => {
    const authoringHash = "sha256:" + "a".repeat(64);
    const safeGenesis = "sha256:" + "b".repeat(64);
    const safeSuccessor = "sha256:" + "c".repeat(64);
    const version = (number: number, hash: string, predecessor: unknown = null) => ({ number, semanticHash: hash, predecessor, topology: { executionId: "demo-deploy", entrypoints: ["implementation"], nodes: { implementation: {} }, edges: [] } });
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, status: "paused" })),
      getEvents: vi.fn(async () => ({ head: 9, events: [
        { sequence: 1, kind: "execution_started", payload: { executionId: "demo-deploy", graphHash: authoringHash, graphVersion: 1 }, occurredAt: null, actorId: "system-cli", actorType: "system", idempotencyKey: "k1", eventId: "e1", evidenceRefs: [] },
        { sequence: 2, kind: "graph_authoring_snapshot_stored", payload: { executionId: "demo-deploy", graphVersion: 1, graphHash: safeGenesis }, occurredAt: null, actorId: "system-cli", actorType: "system", idempotencyKey: "k2", eventId: "e2", evidenceRefs: ["authoring-1"] },
        { sequence: 3, kind: "graph_version_published", payload: { version: version(1, safeGenesis) }, occurredAt: null, actorId: "system-cli", actorType: "system", idempotencyKey: "k3", eventId: "e3", evidenceRefs: [] },
        { sequence: 4, kind: "mutation_accepted", payload: { executionId: "demo-deploy", graphVersion: 2 }, occurredAt: null, actorId: "owner-local", actorType: "owner", idempotencyKey: "k4", eventId: "e4", evidenceRefs: [] },
        { sequence: 5, kind: "graph_authoring_snapshot_stored", payload: { executionId: "demo-deploy", graphVersion: 2, graphHash: safeSuccessor }, occurredAt: null, actorId: "system-cli", actorType: "system", idempotencyKey: "k5", eventId: "e5", evidenceRefs: ["authoring-2"] },
        { sequence: 6, kind: "graph_version_published", payload: { version: version(2, safeSuccessor, { number: 1, semanticHash: safeGenesis }) }, occurredAt: null, actorId: "system-cli", actorType: "system", idempotencyKey: "k6", eventId: "e6", evidenceRefs: [] },
      ] })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(await screen.findByRole("button", { name: /^resume$/i }));
    await waitFor(() => expect(client.resume).toHaveBeenCalled());
    expect((client.resume.mock.calls[0] as unknown[])[1]).toBeUndefined();
  });

  /** #1083 F2: the Studio's resume could not name a fixture, so a demonstration run's resumed
   * node had no outcome and parked `waiting_input` where the CLI's `--fixtures` decides it. The
   * API's existing `fixtures` field now rides the resume when the operator names a file. */
  it("sends a named fixture file with a demonstration run's resume, and nothing on a real run", async () => {
    // PASTED, not typed key by key: every keystroke re-renders the whole App, and ~70 typed
    // characters made this cell take 4.2s on an idle machine - one busy neighbour from its 5s
    // timeout (#1083 round 3). A paste still goes through each input's change handler.
    // The graph-file row is closed until asked for (or until resume walks to it).
    const openGraphFile = async () => {
      if (screen.queryByLabelText(/graph file path on the runtime host/i) === null) await userEvent.click(screen.getByRole("button", { name: "Graph file" }));
    };
    const fill = async (label: RegExp, text: string) => {
      await openGraphFile();
      await userEvent.click(screen.getByLabelText(label));
      await userEvent.paste(text);
    };
    const demo = stubClient({ getStatus: vi.fn(async () => ({ ...STATUS, status: "paused", executor: "fixture" })) });
    await open(demo);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(screen.getByRole("button", { name: /^resume$/i }));
    await fill(/graph file path on the runtime host/i, "/srv/graphs/deploy.yaml");
    await fill(/fixture file path on the runtime host/i, "/srv/fixtures/failure.json");
    await userEvent.click(screen.getByRole("button", { name: /^resume$/i }));
    await waitFor(() => expect(demo.resume).toHaveBeenCalled());
    const demoCall = demo.resume.mock.calls[0] as unknown[];
    expect(demoCall[1]).toBe("/srv/graphs/deploy.yaml");
    expect(demoCall[2]).toMatchObject({ fixtures: "/srv/fixtures/failure.json" });
    cleanup();

    const real = stubClient({ getStatus: vi.fn(async () => ({ ...STATUS, status: "paused", executor: "gateway" })) });
    await open(real);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(screen.getByRole("button", { name: /^resume$/i }));
    await openGraphFile();
    expect(screen.queryByLabelText(/fixture file path on the runtime host/i)).not.toBeInTheDocument();
    await fill(/graph file path on the runtime host/i, "/srv/graphs/deploy.yaml");
    await userEvent.click(screen.getByRole("button", { name: /^resume$/i }));
    await waitFor(() => expect(real.resume).toHaveBeenCalled());
    expect((real.resume.mock.calls[0] as unknown[])[2]).not.toHaveProperty("fixtures");
  });

  /** #1083 F5, through the whole page: the composer's printed `Enter sends` starts the task from
   * the keyboard, with focus where typing left it - not only from the button. */
  it("starts the composed task when Enter is pressed in the objective box", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task/i }));
    await userEvent.type(await screen.findByLabelText(/what should this task do/i), "Summarize the README");
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(client.startTask).toHaveBeenCalled());
    const nodes = (firstCall(client.startTask)[1] as { spec: { nodes: Record<string, { objective: string }> } }).spec.nodes;
    expect(Object.values(nodes)[0].objective).toBe("Summarize the README");
  });
});

/**
 * THE RUN REMEMBERS WHERE IT WAS BORN — as far as this browser can carry it. The operator pointed
 * the board at the graph file once; making them re-type a Runtime-host path on every visit was
 * homework the page could remember. The path is the operator's own note (same contract as board
 * marks: browser-only, never sent anywhere but the verify call they already made), and the
 * verification itself is unchanged — verifyTopology still refuses to draw an unproven edge.
 */
/**
 * ROUND-2 VALIDATION FIXES. A second five-persona review of the live screen confirmed the first
 * wave and found what it missed or introduced. Every test here is one of those findings.
 */
describe("round-2: the recipient cannot go stale", () => {
  it("reopening the panel from the run's name speaks to the room, not to a stale asker", async () => {
    const client = askedClient();
    await open(client);

    // Address the pending asker through the question card, which aims the chat composer at codex.
    const why = await screen.findByLabelText("Needs you");
    await userEvent.click(within(why).getByRole("button", { name: "Answer" }));

    // Reopen the panel from the top bar: the old choice must NOT come back into the run panel.
    const strip = document.querySelector(".topbar")!;
    await userEvent.click(within(strip as HTMLElement).getByRole("button", { name: "demo-deploy" }));
    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "pra sala");
    await userEvent.click(runSend());
    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    expect(firstCall(client.signal)[2]).not.toHaveProperty("to");
  });
});

describe("round-2: the ledger settles debts honestly", () => {
  /** answeredIds was author-blind: another agent's side-reply erased a question addressed to
   * the OPERATOR, and the banner denied a message the thread showed five turns up. */
  it("an agent's reply does not settle a question addressed to you", async () => {
    const client = askedClient({
      readEvidence: vi.fn(async (_executionId: string, evidenceId: string) => ({
        evidenceId,
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        content:
          evidenceId === "ev-question"
            ? JSON.stringify({
                description: "Qual porta devo usar para o deploy?",
                to: "studio-operator",
              })
            : JSON.stringify({ description: "eu respondo por ele", to: "codex", replyTo: "q-1" }),
      })),
      getEvents: vi.fn(async () => ({
        head: 15,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "q-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: ["ev-question"],
          },
          // Another AGENT replies to q-1. The operator still owes their answer.
          {
            sequence: 14,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "x-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:02:00Z",
            actorId: "claude-revisor",
            actorType: "agent",
            idempotencyKey: "k14",
            eventId: "event-14",
            evidenceRefs: ["ev-side"],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Needs you");
    expect(await within(why).findByText(/Qual porta devo usar/)).toBeInTheDocument();
  });

  /** A report that ANSWERS the operator's own request is a return receipt, not a new debt:
   * quoting it as "X asked you" would invert a settled exchange into a fresh demand. */
  it("does not quote an agent's answer to your own request as a question", async () => {
    const client = askedClient({
      readEvidence: vi.fn(async (_executionId: string, evidenceId: string) => ({
        evidenceId,
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        content:
          evidenceId === "ev-request"
            ? JSON.stringify({ description: "me da um relatorio", to: "codex" })
            : JSON.stringify({
                description: "aqui esta o relatorio completo",
                to: "studio-operator",
                replyTo: "req-1",
              }),
      })),
      getEvents: vi.fn(async () => ({
        head: 15,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "req-1", sourceKind: "user" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "studio-operator",
            actorType: "owner",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: ["ev-request"],
          },
          {
            sequence: 14,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "rep-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:02:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k14",
            eventId: "event-14",
            evidenceRefs: ["ev-report"],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const conversation = await screen.findByRole("region", { name: "Shared conversation" });
    expect(await within(conversation).findByText(/aqui esta o relatorio/)).toBeInTheDocument();
    expect(screen.queryByRole("article", { name: /asks$/ })).not.toBeInTheDocument();
  });

  /** The composer's reply hints claimed "Who is waiting for an answer" while listing mere
   * recent speakers — contradicting the banner one block above on the same screen. */
  it("does not claim anyone is waiting when the ledger says nothing is owed", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        attentionReasons: [{ kind: "waiting_input_node", node: "start" }],
      })),
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "s-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: [],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await screen.findByLabelText(/^Run demo-deploy/);

    // codex spoke recently but asked nothing: it may be offered as someone to talk to, but
    // never under a heading that asserts it is waiting.
    expect(screen.queryByLabelText(/who is waiting for an answer/i)).not.toBeInTheDocument();
  });

  /** Selecting a recommendation must fill the owner's draft, never send it. The old address-only
   * cards could not satisfy this, and an eager signal would write an unapproved answer. */
  it("offers two judged replies for the current head and leaves sending to the owner", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, attentionReasons: [{ kind: "waiting_input_node", node: "implementation" }] })),
      listRoutes: vi.fn(async () => ({ configured: true, routes: [{
        id: "judge", provider: "typesafe", transport: "direct_api", billingMode: "per_token",
        model: "jev-latest", profiles: [], enabled: true,
      }] })),
      getReplySuggestions: vi.fn(async () => ({
        executionId: "demo-deploy", headSequence: 13, state: "ready", suggestions: [
          { to: "codex", draft: "Please verify the failing test before approving.", reason: "The node is blocked.", sourceSequences: [13] },
          { to: null, draft: "Pause this run while I review the failure.", reason: "This keeps the current work safe.", sourceSequences: [13] },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const choices = await screen.findByRole("group", { name: "Recommended replies" });
    expect(within(choices).getAllByRole("button")).toHaveLength(2);
    await userEvent.click(within(choices).getByRole("button", { name: /Option 1/ }));
    expect(screen.getByLabelText(/say something into this run/i)).toHaveValue("Please verify the failing test before approving.");
    expect(client.signal).not.toHaveBeenCalled();
    expect(client.getReplySuggestions).toHaveBeenCalledWith("demo-deploy", "judge");
    await userEvent.click(screen.getByRole("button", { name: "Close this panel" }));
  });

  /** #327: on a busy run the head moves every few seconds while Jev takes ~30s. Advice written for
   * an older head still answers the SAME pending question, so it is shown (labelled in the chat
   * column), and the head moving must not restart the request. */
  it("shows recommendations prepared for an older head while the question is unchanged, without re-asking", async () => {
    const client = stubClient({
      listRoutes: vi.fn(async () => ({ configured: true, routes: [{
        id: "judge", provider: "typesafe", transport: "direct_api", billingMode: "per_token",
        model: "jev-latest", profiles: [], enabled: true,
      }] })),
      getReplySuggestions: vi.fn(async () => ({
        executionId: "demo-deploy", headSequence: 12, state: "ready", suggestions: [
          { to: null, draft: "Old answer one", reason: "old", sourceSequences: [12] },
          { to: null, draft: "Old answer two", reason: "old", sourceSequences: [12] },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const choices = await screen.findByRole("group", { name: "Recommended replies" });
    expect(within(choices).getByRole("button", { name: /Old answer one/ })).toBeInTheDocument();
    expect(await screen.findByText("Based on the run as of a moment ago")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(client.getReplySuggestions).toHaveBeenCalledTimes(1);
  });

  /** #396 (spec §8): a Runtime with no Jev model is a state, not an error. The chat shows no Jev
   * card and no Retry (nothing can be retried), and run details says so in one line. */
  it("shows no Jev card when the Runtime has no Jev model, only a line in run details", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, attentionReasons: [{ kind: "waiting_input_node", node: "implementation" }] })),
      listRoutes: vi.fn(async () => ({ configured: true, routes: [] })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    expect(await screen.findByText("No Jev model: suggested replies are off (Models → Add model)")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Jev suggests" })).toBeNull();
    expect(client.getReplySuggestions).not.toHaveBeenCalled();
  });

  /** #327: an unavailable answer is a visible state with a Retry button, never "preparing"
   * forever, and nothing asks again until the owner presses Retry. */
  it("shows why Jev could not prepare a suggestion and retries only when asked", async () => {
    const client = stubClient({
      listRoutes: vi.fn(async () => ({ configured: true, routes: [{
        id: "judge", provider: "typesafe", transport: "direct_api", billingMode: "per_token",
        model: "jev-latest", profiles: [], enabled: true,
      }] })),
      getReplySuggestions: vi.fn(async () => ({
        executionId: "demo-deploy", headSequence: 13, state: "unavailable", reason: "model or Jev could not produce valid suggestions", suggestions: [],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const jev = await screen.findByRole("region", { name: "Jev suggests" });
    expect(await within(jev).findByText(/Jev could not prepare a suggestion: model or Jev/)).toBeInTheDocument();
    expect(within(jev).queryByText(/preparing a suggestion/)).toBeNull();
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(client.getReplySuggestions).toHaveBeenCalledTimes(1);
    await userEvent.click(within(jev).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(client.getReplySuggestions).toHaveBeenCalledTimes(2));
  });
});

describe("round-2: the live tail neither starves nor freezes", () => {
  /** The 4s poll replaced the events array every tick even when nothing changed, re-running
   * the envelope fetches forever — and past 200 events the page silently stopped seeing new
   * ones while still looking live. */
  it("does not re-open envelopes when a poll tick brings nothing new", async () => {
    const client = askedClient();
    render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={40}
      />,
    );
    await screen.findByLabelText("Projects");
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const why = await screen.findByLabelText("Needs you");
    await within(why).findByText(/Qual porta devo usar/);

    const opened = vi.mocked(client.readEvidence).mock.calls.length;
    // Let several ticks pass with an unchanged log.
    await new Promise((resolve) => setTimeout(resolve, 200));
    expect(vi.mocked(client.readEvidence).mock.calls.length).toBe(opened);
  });

  it("reads past the first page, so event 201 exists on screen", async () => {
    const eventAt = (sequence: number) => ({
      sequence,
      kind: "signal_recorded",
      payload: { kind: "operator_note", signalId: `s-${sequence}`, sourceKind: "agent" },
      occurredAt: "2026-08-27T12:01:00Z",
      actorId: "codex",
      actorType: "agent",
      idempotencyKey: `k${sequence}`,
      eventId: `event-${sequence}`,
      evidenceRefs: [],
    });
    const total = 205;
    const client = stubClient({
      getEvents: vi.fn(async (_id: string, options: { after: number; limit: number }) => {
        const all = Array.from({ length: total }, (_, index) => eventAt(index + 1));
        const slice = all.filter((entry) => entry.sequence > options.after).slice(0, options.limit);
        return { head: total, events: slice };
      }),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run demo-deploy/);
    await waitFor(() =>
      expect(panel.querySelectorAll(".turn:not(.stage), .turn.stage li").length).toBeGreaterThanOrEqual(total),
    );
  });
});

describe("round-2: controls stop betraying their own guards", () => {
  it("calls a filtered empty conversation a search miss and restores the log on clear", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run demo-deploy/);
    const search = within(panel).getByRole("textbox", { name: "Search this conversation" });
    await userEvent.type(search, "no-such-report-987");
    expect(within(panel).getByText("No messages match your search.")).toBeInTheDocument();
    expect(within(panel).queryByText("Nothing has been said about this yet.")).not.toBeInTheDocument();
    await userEvent.click(within(panel).getByRole("button", { name: "clear" }));
    expect(search).toHaveValue("");
    expect(within(panel).queryByText("No messages match your search.")).not.toBeInTheDocument();
  });

  it("moves keyboard focus into node details and returns it to the node", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const opener = within(await screen.findByLabelText("Steps without a bot")).getByRole("button", { name: /implementation/i });
    opener.focus();
    await userEvent.keyboard("{Enter}");
    const details = await screen.findByLabelText("Node implementation");
    expect(within(details).getByRole("button", { name: "Close this node" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("returns focus to the conversation toggle when its panel closes", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run demo-deploy/);
    const close = within(panel).getByRole("button", { name: "Close this panel" });
    close.focus();
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(screen.getByRole("button", { name: "Toggle conversation" })).toHaveFocus());
  });

  it("a double-click cannot fire the armed disconnect", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const disconnect = screen.getByRole("button", { name: /^disconnect$/i });
    // The second click of a double-click arrives with detail 2 - the accidental case the
    // arm exists for. It must not erase the session.
    fireEvent.click(disconnect, { detail: 1 });
    fireEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }), {
      detail: 2,
    });
    expect(screen.queryByLabelText(/bearer token/i)).not.toBeInTheDocument();
  });

  it("closing the panel keeps the run and its board; the run's name brings the panel back", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run demo-deploy/);

    await userEvent.click(within(panel).getByRole("button", { name: /close this panel/i }));
    // The panel is gone - and ONLY the panel: the board still stands, nothing claims emptiness.
    expect(screen.queryByLabelText(/^Run demo-deploy/)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Steps without a bot")).toBeInTheDocument();
    expect(screen.queryByText(/This board is empty/)).not.toBeInTheDocument();

    const strip = document.querySelector(".topbar")!;
    await userEvent.click(within(strip as HTMLElement).getByRole("button", { name: "demo-deploy" }));
    expect(await screen.findByLabelText(/^Run demo-deploy/)).toBeInTheDocument();
  });

  it("typing in the graph-file box does not fire the shape check by itself", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await screen.findByLabelText(/^Run demo-deploy/);

    await userEvent.click(screen.getByRole("button", { name: "Graph file" }));
    await userEvent.type(screen.getByLabelText(/graph file path on the runtime host/i), "f");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(client.getTopology).not.toHaveBeenCalled();
  });
});

describe("round-2: the rail says each thing once", () => {
  it("a run's state reaches the accessible name exactly once", async () => {
    await open(stubClient());
    const rail = screen.getByLabelText("Projects");
    const row = within(rail).getByRole("button", { name: /^demo-deploy/i });
    const matches = (row.textContent ?? "").match(/needs you/gi) ?? [];
    expect(matches.length).toBeLessThanOrEqual(1);
    expect(row.querySelector("[title='needs you']")).toBeNull();
  });
});

/**
 * ROUND-3 VALIDATION FIXES. A third five-persona review confirmed round 2 (9 of 10 outright)
 * and found the finer defects below. Each test is one of them.
 */
describe("round-3: what you typed survives, and what waits is true", () => {
  /** A WebMCP agent selecting another run — or closing and reopening the panel — unmounted the
   * composer and silently discarded the operator's half-typed answer. */
  it("keeps the unsent draft across closing and reopening the panel", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run demo-deploy/);

    await userEvent.type(
      within(panel).getByLabelText(/say something into this run/i),
      "resposta pela metade",
    );
    await userEvent.click(within(panel).getByRole("button", { name: /close this panel/i }));
    const strip = document.querySelector(".topbar")!;
    await userEvent.click(within(strip as HTMLElement).getByRole("button", { name: "demo-deploy" }));

    const box = await screen.findByLabelText(/say something into this run/i);
    expect(box).toHaveValue("resposta pela metade");
  });

  /** A speaker with unreadable words must not become a fabricated person or answer request. */
  it("does not promote an unreadable recent speaker into an answer request", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        attentionReasons: [{ kind: "waiting_input_node", node: "start" }],
      })),
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "s-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: [],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    expect(screen.queryByLabelText(/talk to someone/i)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Close this panel" }));
    expect(screen.queryByRole("article", { name: /asks$/ })).not.toBeInTheDocument();
  });
});

describe("round-3: the rail is live and the screen belongs to one run", () => {
  /** The 4s poll tailed only the selected run; another run flipping to needs-you stayed
   * invisible until the operator happened to click something. */
  it("the rail's attention marks move without anyone clicking", async () => {
    let calls = 0;
    const client = stubClient({
      listExecutions: vi.fn(async () => {
        calls += 1;
        return {
          executions: [
            {
              executionId: "demo-calm",
              mode: "supervised",
              status: "running",
              attention: calls > 1 ? "needs_you" : "can_sleep",
              startedAt: null,
              lastEventAt: null,
              headSequence: 4,
            },
          ],
          hasMore: false,
          nextCursor: null,
        };
      }),
    });
    render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={40}
      />,
    );
    await screen.findByLabelText("Projects");
    const rail = screen.getByLabelText("Projects");
    await waitFor(() => {
      const row = within(rail).getByRole("button", { name: /^demo-calm/i });
      expect(row.textContent).toMatch(/needs you/i);
    });
  });

  /** Two fast rail clicks could leave run A's data rendered under run B's name: the slower
   * read committed unconditionally. */
  it("a slow read for the previous run cannot overwrite the newly selected one", async () => {
    let releaseCalm: () => void = () => {};
    const client = stubClient({
      getStatus: vi.fn(async (id: string) => {
        if (id === "demo-calm") {
          await new Promise<void>((resolve) => {
            releaseCalm = resolve;
          });
          return { ...STATUS, executionId: "demo-calm", attention: "can_sleep" };
        }
        return { ...STATUS };
      }),
    });
    await open(client);
    // demo-calm's read hangs; select demo-deploy while it is in flight.
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await screen.findByLabelText(/^Run demo-deploy/);
    releaseCalm();
    await new Promise((resolve) => setTimeout(resolve, 50));
    // The stale read must not have replaced the selected run's panel.
    expect(screen.getByLabelText(/^Run demo-deploy/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/^Run demo-calm/)).not.toBeInTheDocument();
  });
});

describe("round-3: guards without side doors", () => {
  /** Holding Enter blew through the armed disconnect: keyboard clicks carry detail 0, and the
   * detail>1 guard never saw them. Key repeat is one continuous gesture, not a second decision. */
  it("key auto-repeat cannot fire the armed disconnect", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    fireEvent.click(screen.getByRole("button", { name: /^disconnect$/i }), { detail: 1 });
    const armed = screen.getByRole("button", { name: /click again to disconnect/i });
    fireEvent.keyDown(armed, { key: "Enter", repeat: true });
    fireEvent.click(armed, { detail: 0 });
    expect(screen.queryByLabelText(/bearer token/i)).not.toBeInTheDocument();

    // A released key and a fresh, deliberate press still works.
    fireEvent.keyUp(armed, { key: "Enter" });
    fireEvent.click(armed, { detail: 0 });
    expect(await screen.findByLabelText(/bearer token/i)).toBeInTheDocument();
  });

  /** The approve button's explanation lived in a tooltip on a DISABLED button — the channel
   * this codebase's own resume fix declared unreachable. */
  it("says in plain dock text why approval is the wrong instrument", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        attentionReasons: [{ kind: "waiting_input_node", node: "start" }],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await waitFor(() => expect(document.querySelector(".dock .hint")).toHaveTextContent(/This waiting node needs a direction in the thread/i));
    const approve = screen.getByRole("button", { name: /nothing to approve/i });
    expect(approve).not.toHaveAttribute("title");
  });
});

/**
 * ROUND-4 VALIDATION FIXES. The fourth five-persona review returned 59/60 FIXED on round 3 and
 * found the finer defects below — the worst being a reflex arc the UI itself had severed.
 */
describe("round-4: the debt can actually be settled", () => {
  it("answering through the banner sends the replyTo the ledger settles by", async () => {
    const client = askedClient();
    await open(client);

    const why = await screen.findByLabelText("Needs you");
    await within(why).findByText(/Qual porta devo usar/);
    await userEvent.click(within(why).getByRole("button", { name: "Answer" }));
    const chat = screen.getByRole("complementary", { name: "Chat" });
    await userEvent.type(within(chat).getByLabelText("Message"), "usa a 8080");
    await userEvent.click(within(chat).getByRole("button", { name: "Send" }));

    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    expect(firstCall(client.signal)[2]).toMatchObject({ to: "codex", replyTo: "q-1" });
  });

  /** With two nodes waiting, every waiting reason quoted the SAME newest question — one of the
   * two attributions was necessarily wrong. The quote renders once; further reasons say only
   * what they know. */
  it("quotes the pending question once, not once per waiting node", async () => {
    const client = askedClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        attentionReasons: [
          { kind: "waiting_input_node", node: "start" },
          { kind: "waiting_input_node", node: "deploy" },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Needs you");
    await within(why).findByText(/Qual porta devo usar/);
    expect(within(why).getAllByText(/Qual porta devo usar/)).toHaveLength(1);
  });
});

describe("round-4: nothing leaks across surfaces, runs or sessions", () => {
  it("a failed send keeps the words in the box", async () => {
    const client = askedClient({
      signal: vi.fn(async () => {
        throw new Error("the wire broke");
      }),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "nao me perde");
    await userEvent.click(runSend());

    await screen.findByRole("alert");
    expect(screen.getByLabelText(/say something into this run/i)).toHaveValue("nao me perde");
  });

  it("a delivered send clears the box", async () => {
    const client = askedClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "entregue");
    await userEvent.click(runSend());
    await waitFor(() =>
      expect(screen.getByLabelText(/say something into this run/i)).toHaveValue(""),
    );
  });

  /** A send failure — including the one warning the code says cannot be walked back — stayed
   * painted under the NEXT selected run, accusing a message nobody sent there. */
  it("selecting another run clears the previous run's send failure and act-note", async () => {
    const client = stubClient({
      signal: vi.fn(async () => {
        throw new Error("only demo-deploy's problem");
      }),
      // The status echoes the id asked for - the base stub answers demo-deploy for everyone,
      // and this test must SEE the panel switch to demo-calm.
      getStatus: vi.fn(async (id: string) => ({ ...STATUS, executionId: id })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "oi");
    await userEvent.click(runSend());
    await screen.findByRole("alert");
    await userEvent.click(screen.getByRole("button", { name: /pause · finish in-flight/i }));
    await screen.findByText(/paused — done/i);

    await userEvent.click(screen.getByRole("button", { name: /^demo-calm/ }));
    await screen.findByLabelText(/^Run demo-calm/);
    expect(screen.queryByText(/only demo-deploy's problem/)).not.toBeInTheDocument();
    expect(screen.queryByText(/paused — done/i)).not.toBeInTheDocument();
  });

  /** Disconnect's own title promises erasure; the module maps survived it — a reconnect
   * restored a half-typed draft from the "erased" session. */
  it("disconnect forgets the drafts, as its label promises", async () => {
    const client = stubClient();
    const first = render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
      />,
    );
    await screen.findByLabelText("Projects");
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.type(
      await screen.findByLabelText(/say something into this run/i),
      "segredo da sessao velha",
    );
    fireEvent.click(screen.getByRole("button", { name: /^disconnect$/i }), { detail: 1 });
    fireEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }), {
      detail: 1,
    });
    await screen.findByLabelText(/bearer token/i);
    first.unmount();

    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const box = await screen.findByLabelText(/say something into this run/i);
    expect(box).toHaveValue("");
  });

  it("disconnect forgets a parked new-task draft before another Runtime connects", async () => {
    let releaseA!: () => void;
    const pendingA = new Promise<void>((resolve) => {
      releaseA = resolve;
    });
    let settledA = false;
    const client = stubClient({
      startTask: vi.fn(async () => {
        await pendingA;
        settledA = true;
        return { ...PAUSED_EVIDENCE, action: "start" as const };
      }),
    });
    const nextClient = stubClient({
      startTask: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        action: "start" as const,
        result: "unknown" as const,
      })),
    });
    const clients = [client, nextClient];
    const first = render(
      <App
        createClient={() => clients.shift() as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
      />,
    );
    await screen.findByLabelText("Projects");
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    await userEvent.type(
      await screen.findByLabelText("What should this task do?"),
      "Runtime A must not cross the connection boundary",
    );
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(1));
    const idA = client.startTask.mock.calls[0][0] as string;
    const keyA = (client.startTask.mock.calls[0][2] as { idempotencyKey: string }).idempotencyKey;
    await userEvent.click(screen.getByRole("button", { name: /^demo-deploy/ }));
    fireEvent.click(screen.getByRole("button", { name: /^disconnect$/i }), { detail: 1 });
    fireEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }), {
      detail: 1,
    });
    await screen.findByLabelText(/bearer token/i);
    await userEvent.type(screen.getByLabelText(/bearer token/i), "runtime-b-token");
    await userEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    await screen.findByLabelText("Projects");
    await userEvent.click(screen.getByRole("button", { name: /new task in this runtime/i }));
    await userEvent.type(
      await screen.findByLabelText("What should this task do?"),
      "Runtime B is a fresh task",
    );
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(nextClient.startTask).toHaveBeenCalledTimes(1));
    const idB = nextClient.startTask.mock.calls[0][0] as string;
    const keyB = (nextClient.startTask.mock.calls[0][2] as { idempotencyKey: string }).idempotencyKey;
    expect(idB).not.toBe(idA);
    expect(keyB).not.toBe(keyA);

    releaseA();
    await waitFor(() => expect(settledA).toBe(true));
    expect(await screen.findByLabelText("What should this task do?")).toHaveValue(
      "Runtime B is a fresh task",
    );
    first.unmount();
  });

  /** The agent window is re-addressed by a prop change without remounting: half of A's message
   * would ship to B on the next Enter. The window is keyed by who it belongs to. */
  it("switching agent windows never carries the draft over", async () => {
    const client = askedClient({
      getEvents: vi.fn(async () => ({
        head: 15,
        events: [
          {
            sequence: 11,
            kind: "signal_recorded",
            payload: { kind: "persona_created", signalId: "persona-codex", sourceKind: "user" },
            occurredAt: "2026-08-27T12:00:00Z",
            actorId: "studio-operator",
            actorType: "owner",
            idempotencyKey: "k11",
            eventId: "event-11",
            evidenceRefs: ["ev-persona-codex"],
          },
          {
            sequence: 12,
            kind: "signal_recorded",
            payload: { kind: "persona_created", signalId: "persona-claude", sourceKind: "user" },
            occurredAt: "2026-08-27T12:00:30Z",
            actorId: "studio-operator",
            actorType: "owner",
            idempotencyKey: "k12",
            eventId: "event-12",
            evidenceRefs: ["ev-persona-claude"],
          },
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "s-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: [],
          },
          {
            sequence: 14,
            kind: "signal_recorded",
            payload: { kind: "operator_note", signalId: "s-2", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:02:00Z",
            actorId: "claude-revisor",
            actorType: "agent",
            idempotencyKey: "k14",
            eventId: "event-14",
            evidenceRefs: [],
          },
        ],
      })),
      readEvidence: vi.fn(async (_executionId: string, evidenceId: string) => ({
        evidenceId,
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:persona",
        content: evidenceId === "ev-persona-codex"
          ? JSON.stringify({ to: "codex", description: "Codex persona" })
          : JSON.stringify({ to: "claude-revisor", description: "Claude persona" }),
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    await screen.findByRole("region", { name: "Team" });
    await userEvent.click(within(await screen.findByTestId("team-bot-codex")).getByRole("button", { name: "Details" }));
    const agentBox = await within(
      await screen.findByLabelText("Agent codex"),
    ).findByLabelText(/say something into this run/i);
    await userEvent.type(agentBox, "so para codex");

    await userEvent.click(within(screen.getByTestId("team-bot-claude-revisor")).getByRole("button", { name: "Details" }));
    const otherBox = await within(
      await screen.findByLabelText("Agent claude-revisor"),
    ).findByLabelText(/say something into this run/i);
    expect(otherBox).toHaveValue("");
  });
});

describe("round-4: the instruments admit their own state", () => {
  it("lets a new connection poll while the old connection's read is still pending", async () => {
    let releaseOldRead!: () => void;
    const oldRead = new Promise<void>((resolve) => { releaseOldRead = resolve; });
    let statusReads = 0;
    const first = stubClient({
      getStatus: vi.fn(async () => {
        statusReads += 1;
        if (statusReads > 1) await oldRead;
        return { ...STATUS };
      }),
    });
    const second = stubClient();
    const createClient = vi.fn().mockReturnValueOnce(first).mockReturnValueOnce(second);
    render(
      <App
        createClient={createClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={25}
      />,
    );
    await screen.findByLabelText("Projects");
    await waitFor(() => expect(statusReads).toBeGreaterThan(1));

    await userEvent.click(screen.getByRole("button", { name: /^disconnect$/i }));
    await userEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }));
    await userEvent.type(await screen.findByLabelText(/bearer token/i), "new-token");
    await userEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    await screen.findByRole("navigation", { name: "Projects" });
    const readsAfterConnect = second.getStatus.mock.calls.length;
    await waitFor(() => expect(second.getStatus.mock.calls.length).toBeGreaterThan(readsAfterConnect));
    releaseOldRead();
  });

  /** Every sealed item was opened twice (hooks + the Said bubble), and a second panel
   * re-fetched everything: the cache's own comment claimed sharing the code did not do. */
  it("opens each sealed item exactly once across the whole page", async () => {
    const client = askedClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Needs you");
    await within(why).findByText(/Qual porta devo usar/);
    await screen.findAllByText(/Qual porta devo usar/);
    const opens = vi
      .mocked(client.readEvidence)
      // The stub type erases the params tuple, so the destructure trips tsc; index instead.
      .mock.calls.filter((call) => (call as unknown[])[1] === "ev-question").length;
    expect(opens).toBe(1);
  });

  /** A dead Runtime was indistinguishable from a quiet room: every background read failed
   * silently under a badge stuck on "live". */
  it("says the screen is stale when the Runtime stops answering", async () => {
    let healthy = true;
    const client = stubClient({
      getStatus: vi.fn(async () => {
        if (!healthy) throw new Error("dead");
        return { ...STATUS };
      }),
      getEvents: vi.fn(async () => {
        if (!healthy) throw new Error("dead");
        return { head: 13, events: [] };
      }),
      listExecutions: vi.fn(async () => {
        if (!healthy) throw new Error("dead");
        return {
          executions: [
            {
              executionId: "demo-deploy",
              mode: "supervised",
              status: "running",
              attention: "needs_you",
              startedAt: null,
              lastEventAt: null,
              headSequence: 13,
            },
          ],
          hasMore: false,
          nextCursor: null,
        };
      }),
    });
    render(
      <App
        createClient={() => client as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={30}
      />,
    );
    await screen.findByLabelText("Projects");
    healthy = false;
    await waitFor(() => expect(screen.getByText(/stale/i)).toBeInTheDocument(), {
      timeout: scaled(2000),
    });
  });

  /** A failed Runtime left its health badge behind after reconnecting to a healthy empty Runtime.
   * The empty list has no selected run to poll, so only the connection boundary can clear it. */
  it("clears stale health after reconnecting to a healthy empty Runtime", async () => {
    let firstHealthy = true;
    const first = stubClient({
      listExecutions: vi.fn(async () => ({
        executions: [{
          executionId: "demo-deploy",
          mode: "supervised",
          status: "running",
          attention: "needs_you",
          startedAt: null,
          lastEventAt: null,
          headSequence: 13,
        }],
        hasMore: false,
        nextCursor: null,
      })),
      getStatus: vi.fn(async () => {
        if (!firstHealthy) throw new Error("dead");
        return { ...STATUS };
      }),
      getEvents: vi.fn(async () => {
        if (!firstHealthy) throw new Error("dead");
        return { head: 13, events: [] };
      }),
    });
    const second = stubClient({
      listExecutions: vi.fn(async () => ({ executions: [], hasMore: false, nextCursor: null })),
    });
    const makeClient = vi.fn().mockReturnValueOnce(first).mockReturnValueOnce(second);
    render(
      <App
        createClient={(token) => makeClient(token) as unknown as RuntimeClient}
        modelContext={null}
        session={async () => ({ token: "local-token", project: "dale-api-base" })}
        pollIntervalMs={30}
      />,
    );
    await screen.findByLabelText("Projects");
    firstHealthy = false;
    await waitFor(() => expect(screen.getByText(/stale/i)).toBeInTheDocument(), { timeout: scaled(2000) });

    fireEvent.click(screen.getByRole("button", { name: /^disconnect$/i }), { detail: 1 });
    fireEvent.click(screen.getByRole("button", { name: /click again to disconnect/i }), { detail: 1 });
    await userEvent.type(await screen.findByLabelText(/bearer token/i), "local-token");
    await userEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    await waitFor(() => expect(screen.queryByText(/stale/i)).not.toBeInTheDocument());
    expect(await screen.findByText(/No task yet/i)).toBeInTheDocument();
  });

  /** A refused WebMCP tool call was invisible: the chip lit identically for success and
   * refusal, and the detail reached nobody. */
  it("shows a refused tool call as refused", async () => {
    const { modelContext, registered } = fakeModelContext();
    const client = stubClient({
      pause: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        result: "refused" as const,
        diagnostics: [{ code: "GHE000", message: "not while waiting" }],
      })),
    });
    await open(client, modelContext);
    const pauseTool = registered.find((tool) => tool.name === "graphhelm_pause_execution")!;
    await pauseTool.execute({ executionId: "demo-deploy" });

    await waitFor(() => expect(document.querySelector(".toolchips .refused")).not.toBeNull());
  });

  /** The roster accepted a re-charter of an existing persona (last-writer-wins) and even a
   * chartered "studio-operator" — the operator is the person AT the screen, never a blob on
   * it. First charter wins; reserved ids never enter. */
  it("the crew refuses a chartered operator", async () => {
    const envelopes: Record<string, string> = {
      "ev-op": JSON.stringify({
        description: "eu sou voce agora",
        to: "studio-operator",
      }),
    };
    const client = stubClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          {
            sequence: 13,
            kind: "signal_recorded",
            payload: { kind: "persona_created", signalId: "b-1", sourceKind: "agent" },
            occurredAt: "2026-08-27T12:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k13",
            eventId: "event-13",
            evidenceRefs: ["ev-op"],
          },
        ],
      })),
      readEvidence: vi.fn(async (_id: string, evidenceId: string) => ({
        evidenceId,
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        content: envelopes[evidenceId] ?? "{}",
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const team = await screen.findByRole("region", { name: "Team" });
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(
      within(team).queryByRole("button", { name: /studio-operator/i }),
    ).not.toBeInTheDocument();
  });
});

/**
 * THE LAST THREE INCHES TO 100%: the log is searchable, and every turn is addressable. The
 * crude version of this product (a text file) was greppable end to end and every line had a
 * number; the polished one owes the same two properties.
 */
describe("the log is searchable and addressable", () => {
  const twoMessages = () => ({
    getEvents: vi.fn(async () => ({
      head: 15,
      events: [
        {
          sequence: 13,
          kind: "signal_recorded",
          payload: { kind: "operator_note", signalId: "s-1", sourceKind: "agent" },
          occurredAt: "2026-08-27T12:01:00Z",
          actorId: "codex",
          actorType: "agent",
          idempotencyKey: "k13",
          eventId: "event-13",
          evidenceRefs: ["ev-porta"],
        },
        {
          sequence: 14,
          kind: "signal_recorded",
          payload: { kind: "operator_note", signalId: "s-2", sourceKind: "agent" },
          occurredAt: "2026-08-27T12:02:00Z",
          actorId: "claude-revisor",
          actorType: "agent",
          idempotencyKey: "k14",
          eventId: "event-14",
          evidenceRefs: ["ev-fila"],
        },
      ],
    })),
    readEvidence: vi.fn(async (_id: string, evidenceId: string) => ({
      evidenceId,
      mediaType: "application/json",
      sensitivity: "confidential",
      contentSha256: "sha256:whatever",
      content:
        evidenceId === "ev-porta"
          ? JSON.stringify({ description: "a porta certa e a 8080" })
          : JSON.stringify({ description: "a fila esta vazia" }),
    })),
  });

  it("search narrows the thread to matching turns, and says what it hid", async () => {
    const client = stubClient(twoMessages());
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await within(document.querySelector(".thread") as HTMLElement).findByText(/porta certa/);

    await userEvent.type(screen.getByLabelText(/search this conversation/i), "porta");
    expect(within(document.querySelector(".thread") as HTMLElement).getByText(/porta certa/)).toBeInTheDocument();
    expect(within(document.querySelector(".thread") as HTMLElement).queryByText(/fila esta vazia/)).not.toBeInTheDocument();
    // The narrowing is announced - a thread that silently hides is a thread that lies.
    expect(screen.getByText(/1 of 2/)).toBeInTheDocument();

    await userEvent.clear(screen.getByLabelText(/search this conversation/i));
    expect(await within(document.querySelector(".thread") as HTMLElement).findByText(/fila esta vazia/)).toBeInTheDocument();
  });

  it("every spoken turn carries its coordinate, and clicking copies it", async () => {
    const written: string[] = [];
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: (text: string) => {
          written.push(text);
          return Promise.resolve();
        },
      },
    });
    const client = stubClient(twoMessages());
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await within(document.querySelector(".thread") as HTMLElement).findByText(/porta certa/);

    await userEvent.click(screen.getByRole("button", { name: "#13" }));
    expect(written).toContain("demo-deploy#13");
  });
});

describe("the board remembers its graph file", () => {
  afterEach(() => localStorage.removeItem("graphhelm.studio.board.demo-deploy"));

  it("does not accept a topology response for a path that was edited while the read waited", async () => {
    const hash = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    let answer!: (value: GraphTopology) => void;
    const pending = new Promise<GraphTopology>((resolve) => { answer = resolve; });
    const client = stubClient({
      getEvents: vi.fn(async () => ({ head: 1, events: [{
        sequence: 1, kind: "execution_started", payload: { executionId: "demo-deploy", graphHash: hash },
        occurredAt: "2026-08-27T12:00:00Z", actorId: "system-cli", actorType: "system",
        idempotencyKey: "k1", eventId: "event-1", evidenceRefs: [],
      }] })),
      getTopology: vi.fn(() => pending),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(screen.getByRole("button", { name: "Graph file" }));
    const field = screen.getByLabelText(/graph file path on the runtime host/i);
    await userEvent.type(field, "/graphs/A.yaml");
    await userEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    expect(client.getTopology).toHaveBeenCalledWith("/graphs/A.yaml");
    await userEvent.clear(field);
    await userEvent.type(field, "/graphs/B.yaml");
    await act(async () => answer({ graphId: "g", graphVersion: 1, executionId: "demo-deploy", semanticHash: hash,
      entrypoints: [], nodes: [], edges: [] }));
    expect(field).toHaveValue("/graphs/B.yaml");
    expect(screen.queryByText(/Connections verified:/i)).not.toBeInTheDocument();
  });

  it("an old topology failure cannot erase a newer path's proof or busy state", async () => {
    const hash = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    let rejectOld!: (reason: Error) => void;
    let answerNew!: (value: GraphTopology) => void;
    const oldRead = new Promise<GraphTopology>((_, reject) => { rejectOld = reject; });
    const newRead = new Promise<GraphTopology>((resolve) => { answerNew = resolve; });
    const client = stubClient({
      getEvents: vi.fn(async () => ({ head: 1, events: [{
        sequence: 1, kind: "execution_started", payload: { executionId: "demo-deploy", graphHash: hash },
        occurredAt: "2026-08-27T12:00:00Z", actorId: "system-cli", actorType: "system",
        idempotencyKey: "k1", eventId: "event-1", evidenceRefs: [],
      }] })),
      getTopology: vi.fn((file: string) => file === "/graphs/A.yaml" ? oldRead : newRead),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await userEvent.click(screen.getByRole("button", { name: "Graph file" }));
    const field = screen.getByLabelText(/graph file path on the runtime host/i);
    await userEvent.type(field, "/graphs/A.yaml");
    await userEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    await userEvent.clear(field);
    await userEvent.type(field, "/graphs/B.yaml");
    await userEvent.click(screen.getByRole("button", { name: /^connect$/i }));
    await act(async () => rejectOld(new Error("Old path failed")));
    expect(screen.queryByText(/Old path failed/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^connect$/i })).toBeDisabled();
    await act(async () => answerNew({ graphId: "g", graphVersion: 1, executionId: "demo-deploy",
      semanticHash: hash, entrypoints: [], nodes: [], edges: [] }));
    expect(screen.getByText(/Connections verified:/i)).toBeInTheDocument();
  });

  it("re-verifies from the remembered path on open, without being asked", async () => {
    localStorage.setItem(
      "graphhelm.studio.board.demo-deploy",
      JSON.stringify({ positions: {}, agents: {}, strokes: [], notes: [], graphFile: "flows/demo.json" }),
    );
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await waitFor(() => expect(client.getTopology).toHaveBeenCalledWith("flows/demo.json"));
  });

  it("shows journal connections on a fresh browser without asking for a graph file", async () => {
    const hash = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    const recorded = (sequence: number, kind: string, payload: unknown) => ({
      sequence, kind, payload, occurredAt: "2026-08-27T12:00:00Z", actorId: "system-cli",
      actorType: "system", idempotencyKey: `k${sequence}`, eventId: `event-${sequence}`, evidenceRefs: [],
    });
    const client = stubClient({ getEvents: vi.fn(async () => ({ head: 2, events: [
      recorded(1, "execution_started", { executionId: "demo-deploy", graphHash: hash }),
      recorded(2, "execution_form_declared", {
        executionId: "demo-deploy", nodeIds: ["implementation", "deploy"],
        topology: { graphHash: hash, entrypoints: ["implementation"], edges: [
          { id: "implementation_to_deploy", from: "implementation", to: "deploy", type: "data" },
        ] },
      }),
    ] })) });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "Graph file" }));
    expect(await screen.findByText(/1 connection drawn/)).toBeInTheDocument();
    expect(client.getTopology).not.toHaveBeenCalled();
  });

  it.each(["mismatched hash", "null snapshot"])(
    "keeps a %s unverified even when a remembered file matches the hash",
    async (snapshotKind) => {
      const hash = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
      localStorage.setItem(
        "graphhelm.studio.board.demo-deploy",
        JSON.stringify({ positions: {}, agents: {}, strokes: [], notes: [], graphFile: "flows/demo.json" }),
      );
      const recorded = (sequence: number, kind: string, payload: unknown) => ({
        sequence, kind, payload, occurredAt: "2026-08-27T12:00:00Z", actorId: "system-cli",
        actorType: "system", idempotencyKey: `k${sequence}`, eventId: `event-${sequence}`, evidenceRefs: [],
      });
      const client = stubClient({
        getEvents: vi.fn(async () => ({ head: 2, events: [
          recorded(1, "execution_started", { executionId: "demo-deploy", graphHash: hash }),
          recorded(2, "execution_form_declared", {
            executionId: "demo-deploy", nodeIds: ["implementation", "deploy"],
            topology: snapshotKind === "null snapshot" ? null : {
              graphHash: "sha256:wrong", entrypoints: ["implementation"], edges: [
                { id: "implementation_to_deploy", from: "implementation", to: "deploy", type: "data" },
              ],
            },
          }),
        ] })),
        getTopology: vi.fn(async () => ({
          graphId: "g", graphVersion: 1, executionId: "demo-deploy", semanticHash: hash,
          entrypoints: ["implementation"], nodes: [], edges: [
            { id: "implementation_to_deploy", from: "implementation", to: "deploy", type: "data" },
          ],
        })),
      });
      await open(client);
      await waitFor(() => expect(client.getTopology).toHaveBeenCalledWith("flows/demo.json"));
      await userEvent.click(await screen.findByRole("button", { name: "Graph file" }));
      expect(await screen.findByText(/work connections unverified/i)).toBeInTheDocument();
      expect(document.querySelectorAll("path.edge, .team-link")).toHaveLength(0);
    },
  );
});

/**
 * THE ASYNCHRONOUS EXCHANGE, from the watching end.
 *
 * An agent working in a repository leaves records as it goes; this is the surface where a person
 * reads them and answers. Both halves are guarded here because both can fail silently and look
 * fine: a thread that renders every event and opens none of them still LOOKS like a full log, and
 * a message box that posts nothing still clears itself after you press send.
 */
describe("the exchange", () => {
  /** A run whose log carries one sealed message from the agent. */
  function talkingClient(overrides: Record<string, unknown> = {}) {
    return stubClient({
      getEvents: vi.fn(async () => ({
        head: 14,
        events: [
          {
            sequence: 14,
            kind: "signal_recorded",
            payload: {
              executionId: "demo-deploy",
              signalId: "sig-1",
              sourceKind: "user",
              sourceId: "claude-code",
              kind: "operator_note",
              severity: "low",
              envelopeSha256: "sha256:whatever",
            },
            occurredAt: "2026-08-27T12:02:00Z",
            actorId: "claude-code",
            actorType: "agent",
            idempotencyKey: "k-14",
            eventId: "event-14",
            evidenceRefs: ["ev-signal"],
          },
        ],
      })),
      ...overrides,
    });
  }

  /** THE CLAIM THE WHOLE FEATURE RESTS ON, and the one an operator cannot verify for themselves.
   *
   * D-036 keeps free-form content OUT of event payloads, so the words of a message are not in the
   * event and no amount of reading `payload` will find them - the event carries a reference and the
   * content is sealed. A thread that renders only what the event says would report "Said:" and stop
   * there, which is indistinguishable from a message that was empty. This asserts the TEXT. */
  it("opens the sealed words and shows them in the thread", async () => {
    const client = talkingClient({
      readEvidence: vi.fn(async () => ({
        evidenceId: "ev-signal",
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        // THE SHAPE THE RUNTIME ACTUALLY SEALS: the whole envelope, not the sentence. An earlier
        // version of this fixture returned the sentence alone, which is what a browser would show
        // if the Studio were already right — so the test passed while the real surface rendered a
        // wall of JSON where somebody's words belonged. The fixture now carries the document, and
        // the assertion below is `toHaveTextContent` on the sentence ALONE, so a render that
        // includes the envelope around it fails.
        content: JSON.stringify({
          id: "sig-1",
          source: { type: "user", id: "claude-code" },
          type: "operator_note",
          severity: "low",
          description: "the migration needs a decision before I go further",
          evidence: ["demo-deploy"],
          emittedAt: "2026-08-27T12:02:00Z",
        }),
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    const said = await within(within(panel).getByRole("region", { name: "Shared conversation" })).findByText(
      /the migration needs a decision before I go further/,
    );
    // EXACTLY the sentence. A `contains`-shaped assertion is satisfied by the sentence buried in
    // its envelope, which is the failure this test exists to catch.
    expect(said.textContent).toBe("the migration needs a decision before I go further");
    expect(client.readEvidence).toHaveBeenCalledWith("demo-deploy", "ev-signal");
  });

  /** An envelope this Runtime holds no key for is a real state. It is reported in the thread, in
   * place, rather than rendering as an absence - which would read as "nothing was said". */
  it("says so when a sealed message cannot be opened, instead of rendering nothing", async () => {
    const client = talkingClient({
      readEvidence: vi.fn(async () => {
        throw new Error("no key for this evidence");
      }),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    expect(await within(panel).findByText(/cannot open it/i)).toBeInTheDocument();
    expect(within(panel).getByText(/no key for this evidence/)).toBeInTheDocument();
  });

  /** The way back in. The assertion is on what reached the CLIENT, not on the box emptying: a
   * composer that clears itself and sends nothing is the failure this is here to catch. */
  it("sends what was typed into the run's log", async () => {
    const client = talkingClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    const box = within(panel).getByLabelText(/say something into this run/i);
    await userEvent.type(box, "use the second option, and say why in the log");
    await userEvent.click(within(panel).getByRole("button", { name: /^send$/i }));

    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    const call = vi.mocked(client.signal).mock.calls[0]!;
    expect(call[0]).toBe("demo-deploy");
    expect(call[1]).toBe("use the second option, and say why in the log");
    // No recipient was picked, so the message speaks to the room: no `to` key at all.
    expect(Object.keys((call[2] ?? {}) as Record<string, unknown>)).not.toContain("to");
  });

  /** Bookkeeping is not conversation. A wake lease renders as raw JSON in the middle of the chat
   * (measured on a real screenshot, 2026-08-30: `{"cursor":22,"executionId":...}` between two
   * human sentences) unless the thread can say it in words. It gets words, and a quiet voice. */
  it("says bookkeeping in words instead of dumping its payload into the chat", async () => {
    const client = talkingClient({
      getEvents: vi.fn(async () => ({
        head: 15,
        events: [
          {
            sequence: 15,
            kind: "wake_lease",
            payload: { cursor: 14, executionId: "demo-deploy", rendezvousId: "claude-listener-a4" },
            occurredAt: "2026-08-30T14:00:00Z",
            actorId: "claude-code",
            actorType: "agent",
            idempotencyKey: "k-wake",
            eventId: "event-15w",
            evidenceRefs: [],
          },
        ],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    expect(await within(panel).findByText(/armed a wake doorbell/i)).toBeInTheDocument();
    expect(within(panel).queryByText(/"cursor"/)).not.toBeInTheDocument();
  });

  /** The room's roster, from the log itself. A persona exists because a `persona_created`
   * signal chartered it; the panel derives the roster from those signals and shows it, personas
   * marked as personas. Nobody maintains a second list that could drift from the journal. */
  function roomClient() {
    return talkingClient({
      getEvents: vi.fn(async () => ({
        head: 17,
        events: [
          {
            sequence: 16,
            kind: "signal_recorded",
            payload: {
              executionId: "demo-deploy",
              signalId: "persona-birth-seguranca",
              sourceKind: "user",
              sourceId: "claude-code",
              kind: "persona_created",
              severity: "low",
            },
            occurredAt: "2026-08-30T15:00:00Z",
            actorId: "claude-code",
            actorType: "agent",
            idempotencyKey: "k-16",
            eventId: "event-16",
            evidenceRefs: ["ev-persona"],
          },
          {
            sequence: 17,
            kind: "signal_recorded",
            payload: {
              executionId: "demo-deploy",
              signalId: "msg-17",
              sourceKind: "user",
              sourceId: "codex",
              kind: "operator_note",
              severity: "low",
            },
            occurredAt: "2026-08-30T15:01:00Z",
            actorId: "codex",
            actorType: "agent",
            idempotencyKey: "k-17",
            eventId: "event-17",
            evidenceRefs: ["ev-msg"],
          },
        ],
      })),
      readEvidence: vi.fn(async (_id: string, evidenceId: string) => ({
        evidenceId,
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        content:
          evidenceId === "ev-persona"
            ? JSON.stringify({
                description: "Es a persona de seguranca desta task.",
                to: "seguranca",
                type: "persona_created",
              })
            : JSON.stringify({ description: "uma mensagem", type: "operator_note" }),
      })),
    });
  }

  it("shows recorded agent messages in the run side panel while the graph has no moving nodes", async () => {
    await open(roomClient());
    const side = await screen.findByRole("complementary", { name: "Run side panel" });
    expect(await within(side).findByText(/uma mensagem/)).toBeInTheDocument();
  });

  it("shows only an explicitly chartered persona on the team canvas", async () => {
    await open(roomClient());
    const team = await screen.findByRole("region", { name: "Team" });
    expect(await within(team).findByRole("button", { name: /^seguranca,/ })).toBeInTheDocument();
    expect(within(team).queryByRole("button", { name: /^codex/i })).not.toBeInTheDocument();
  });

  /** Clicking a crew member opens the INDIVIDUAL conversation: the thread filtered to that
   * agent's exchanges, and a say box locked to them - a window named after someone that posted
   * to the room would put words where nobody sent them. */
  it("opens an individual conversation from the team canvas, addressed and filtered", async () => {
    const client = roomClient();
    await open(client);
    const team = await screen.findByRole("region", { name: "Team" });
    await within(team).findByRole("button", { name: /^seguranca,/ });
    await userEvent.click(within(screen.getByTestId("team-bot-seguranca")).getByRole("button", { name: "Details" }));

    const window = await screen.findByLabelText("Agent seguranca");
    // The filter is real: codex's unrelated message stays out of seguranca's window.
    expect(within(window).queryByText(/uma mensagem/)).not.toBeInTheDocument();

    await userEvent.type(
      within(window).getByLabelText(/say something into this run/i),
      "qual o maior risco agora?",
    );
    await userEvent.click(within(window).getByRole("button", { name: /^send$/i }));

    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    const options = vi.mocked(client.signal).mock.calls[0]?.[2] as Record<string, unknown>;
    expect(options.to).toBe("seguranca");
  });

  it("does not turn technical actors into bots, and counts them as other recorders", async () => {
    const note = (sequence: number, actorId: string) => ({
      sequence, kind: "signal_recorded",
      payload: { executionId: "demo-deploy", signalId: `sig-${sequence}`, sourceKind: "user", sourceId: actorId, kind: "operator_note", severity: "low" },
      occurredAt: "2026-08-27T12:02:00Z", actorId, actorType: "agent",
      idempotencyKey: `k-${sequence}`, eventId: `event-${sequence}`, evidenceRefs: [],
    });
    const client = talkingClient({
      getEvents: vi.fn(async () => ({ head: 17, events: [note(14, "claude-code"), note(15, "codex")] })),
    });
    await open(client);
    const team = await screen.findByRole("region", { name: "Team" });
    expect(within(team).queryByRole("button", { name: /^codex/i })).not.toBeInTheDocument();
    expect(within(team).queryByRole("button", { name: /^claude-code/i })).not.toBeInTheDocument();
    expect(within(team).getByText(/2 other recorders/)).toBeInTheDocument();
  });

  /** A persona's birth is a thread event, said as one. `persona_created` is an unrecognized
   * signal kind on purpose (recorded, never steers); the thread names who was chartered rather
   * than dumping the envelope. */
  it("announces a chartered persona in words", async () => {
    const client = talkingClient({
      getEvents: vi.fn(async () => ({
        head: 16,
        events: [
          {
            sequence: 16,
            kind: "signal_recorded",
            payload: {
              executionId: "demo-deploy",
              signalId: "persona-seguranca-1",
              sourceKind: "user",
              sourceId: "claude-code",
              kind: "persona_created",
              severity: "low",
            },
            occurredAt: "2026-08-30T15:00:00Z",
            actorId: "claude-code",
            actorType: "agent",
            idempotencyKey: "k-16",
            eventId: "event-16",
            evidenceRefs: ["ev-persona"],
          },
        ],
      })),
      readEvidence: vi.fn(async () => ({
        evidenceId: "ev-persona",
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        content: JSON.stringify({
          description: "Es a persona de seguranca desta task.",
          to: "seguranca",
        }),
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    expect(await within(panel).findByText(/a new persona joined/i)).toBeInTheDocument();
  });

  /** The envelope's address, visible where the words are. Since schema 1.1.0 a message can carry
   * `to`/`replyTo`; a thread that hides them makes the group chat unthreadable by eye. */
  it("shows who a message was addressed to", async () => {
    const client = talkingClient({
      readEvidence: vi.fn(async () => ({
        evidenceId: "ev-signal",
        mediaType: "application/json",
        sensitivity: "confidential",
        contentSha256: "sha256:whatever",
        content: JSON.stringify({
          description: "concordo com o plano",
          to: "codex",
          replyTo: "signal-17",
        }),
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    expect(await within(within(panel).getByRole("region", { name: "Shared conversation" })).findByText("concordo com o plano")).toBeInTheDocument();
    expect(within(panel).getByText(/→ codex/)).toBeInTheDocument();
  });

  /** `unknown` means the Runtime accepted it and the verifying read could not confirm it landed.
   * Reporting that as sent is the one error that cannot be walked back, so it is reported as
   * unconfirmed - and the operator is told not to simply send it again. */
  it("does not report an unconfirmed message as sent", async () => {
    const client = talkingClient({
      signal: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        action: "signal" as const,
        result: "unknown" as const,
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const panel = await screen.findByLabelText(/^Run (?!side panel)/);
    await userEvent.type(
      within(panel).getByLabelText(/say something into this run/i),
      "did that land?",
    );
    await userEvent.click(within(panel).getByRole("button", { name: /^send$/i }));

    expect(await within(panel).findByText(/could not be confirmed/i)).toBeInTheDocument();
  });
});

/**
 * #1077, from a blind judge's report. After `start this task` the page showed "This board is
 * empty." (the draft was cleared but nothing was selected: the identity check lived inside a
 * setState updater React ran too late), and the run it created was `run-<uuid>` everywhere,
 * the typed sentence shown nowhere. The objective lives in the briefing (#1063), and the
 * Studio reads it from there.
 */
describe("the run it just started", () => {
  const OBJECTIVE = "Investigate slow login on mobile";
  const BRIEFING = {
    name: "New task",
    objective: OBJECTIVE,
    executor: "gateway",
    graphHash: null,
    graphVersion: 1,
    decisions: [],
    workDone: [],
    pending: [],
    unevaluated: [],
    nextStep: { kind: "dispatch", nodes: ["start"] },
    asOfSequence: 3,
  };
  /** A stub whose list grows by the run `startTask` created, the way the real index does. */
  function withStarted(overrides: Record<string, unknown> = {}) {
    const started: string[] = [];
    const base = stubClient();
    return stubClient({
      startTask: vi.fn(async (executionId: string) => {
        started.push(executionId);
        return { ...PAUSED_EVIDENCE, action: "start" as const, executionId };
      }),
      listExecutions: vi.fn(async () => {
        const page = await base.listExecutions();
        return {
          ...page,
          executions: [
            ...started.map((executionId) => ({ executionId, mode: "supervised", status: "running", attention: "can_sleep", startedAt: null, lastEventAt: null, headSequence: 3 })),
            ...page.executions,
          ],
        };
      }),
      getEvents: vi.fn(async (id: string) =>
        id.startsWith("run-")
          ? {
              head: 2,
              events: [
                { sequence: 2, kind: "execution_form_declared", payload: { executionId: id, nodeIds: ["start"] }, occurredAt: "2026-09-13T12:00:30Z", actorId: "system-cli", actorType: "system", idempotencyKey: "k0", eventId: "event-2", evidenceRefs: [] },
              ],
            }
          : base.getEvents(),
      ),
      ...overrides,
    });
  }
  async function startOne(client: ReturnType<typeof stubClient>) {
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task/i }));
    await userEvent.type(await screen.findByLabelText(/what should this task do/i), OBJECTIVE);
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalled());
    return firstCall(client.startTask)[0] as string;
  }

  it("opens a durably listed run while the start response is still waiting for the model", async () => {
    let committed: string | null = null;
    let finish!: (value: MutationEvidence) => void;
    const base = stubClient();
    const client = stubClient({
      startTask: vi.fn((executionId: string) => {
        committed = executionId;
        return new Promise<MutationEvidence>((resolve) => { finish = resolve; });
      }),
      listExecutions: vi.fn(async () => {
        const page = await base.listExecutions();
        return committed === null ? page : {
          ...page,
          executions: [
            { executionId: committed, mode: "supervised", status: "running", attention: "can_sleep", startedAt: null, lastEventAt: null, headSequence: 3 },
            ...page.executions,
          ],
        };
      }),
    });
    render(<App createClient={() => client as unknown as RuntimeClient} modelContext={null} session={async () => ({ token: "local-token", project: "dale-api-base" })} pollIntervalMs={25} />);
    await screen.findByLabelText("Projects");
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    await userEvent.type(await screen.findByLabelText(/what should this task do/i), OBJECTIVE);
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(within(screen.getByLabelText("Projects")).getByRole("button", { current: true })).toHaveTextContent(committed!));
    expect(screen.queryByRole("button", { name: /starting/i })).not.toBeInTheDocument();
    finish({ ...PAUSED_EVIDENCE, action: "start", executionId: committed! });
  });

  it("selects the new run and opens its team canvas instead of an empty board", async () => {
    const client = withStarted();
    const id = await startOne(client);
    expect(await screen.findByRole("region", { name: "Team" })).toBeInTheDocument();
    expect(screen.queryByText("This board is empty.")).not.toBeInTheDocument();
    expect(screen.queryByText("pick a run, or start one")).not.toBeInTheDocument();
    const rail = screen.getByLabelText("Projects");
    await waitFor(() => expect(within(rail).getByRole("button", { current: true })).toHaveAttribute("title", expect.any(String)));
    expect(within(rail).getByRole("button", { current: true }).textContent).toContain(id.slice(0, 8));
  });

  /** #1079 review: `select` clears the act-note, and the start path used to set the note BEFORE
   * selecting the new run - last write won, and "Started — done" never appeared. */
  it("keeps the start act-note on screen after selecting the run it started", async () => {
    const client = withStarted();
    await startOne(client);
    expect(await screen.findByRole("region", { name: "Team" })).toBeInTheDocument();
    expect(await screen.findByText(/started — done/i)).toBeInTheDocument();
  });

  it("does not re-park the consumed draft after a normal successful start", async () => {
    const client = withStarted();
    const firstId = await startOne(client);

    await userEvent.click(screen.getByRole("button", { name: /new task/i }));
    await userEvent.type(
      await screen.findByLabelText("What should this task do?"),
      "A genuinely different task",
    );
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(2));

    expect(client.startTask.mock.calls[1][0]).not.toBe(firstId);
  });

  it("names the run by its objective wherever the id stood, and never by 'New task'", async () => {
    // A real Runtime answers 200 for every id it can replay - a hand-named run simply has no
    // objective; only a MISSING ROUTE answers 404 (which the client reports as null).
    const client = withStarted({ getBriefing: vi.fn(async (id: string) => (id.startsWith("run-") ? BRIEFING : { ...BRIEFING, name: null, objective: null })) });
    const id = await startOne(client);
    // The rail row: objective as the name, id kept on the row as its address.
    const rail = screen.getByLabelText("Projects");
    const row = await within(rail).findByRole("button", { name: new RegExp(OBJECTIVE) });
    expect(row).toHaveAttribute("aria-current", "true");
    expect(within(rail).getByTitle(id)).toBeInTheDocument();
    // The header names it too; the id stays reachable on hover.
    const strip = document.querySelector(".topbar") as HTMLElement;
    expect(await within(strip).findByRole("button", { name: OBJECTIVE })).toHaveAttribute("title", id);
    // The run panel quotes it verbatim.
    const panel = screen.getByRole("region", { name: /^Run (?!side panel)/ });
    expect(within(panel).getByText(OBJECTIVE)).toBeInTheDocument();
    expect(screen.queryByText("New task")).not.toBeInTheDocument();
  });

  it("keeps the id and raises no error when the Runtime has no briefing route", async () => {
    const client = withStarted({ getBriefing: vi.fn(async () => null) });
    const id = await startOne(client);
    const strip = document.querySelector(".topbar") as HTMLElement;
    expect(await within(strip).findByRole("button", { name: id })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});

/** #1079 review (P2): selecting a run whose briefing the rail is ALREADY fetching must await
 * that read, not conclude "no briefing" because the id was asked - the header, panel and card
 * stayed nameless until a deselect/reselect. */
describe("a selection that lands while the rail is prefetching", () => {
  const GENERATED = "run-9a1b2c3d-4e5f-4a6b-8c7d-0e1f2a3b4c5d";
  it("awaits the in-flight briefing and names the run once it arrives", async () => {
    let settle: (value: unknown) => void = () => {};
    const inFlight = new Promise((resolve) => { settle = resolve; });
    const client = stubClient({
      listExecutions: vi.fn(async () => ({
        executions: [
          { executionId: "demo-deploy", mode: "supervised", status: "running", attention: "needs_you", startedAt: null, lastEventAt: null, headSequence: 13 },
          { executionId: GENERATED, mode: "supervised", status: "running", attention: "can_sleep", startedAt: null, lastEventAt: null, headSequence: 3 },
        ],
        hasMore: false,
        nextCursor: null,
      })),
      getBriefing: vi.fn((id: string) =>
        id === GENERATED
          ? inFlight.then(() => ({ name: "New task", objective: "Investigate slow login on mobile", executor: "gateway", nextStep: { kind: "dispatch", nodes: ["start"] }, asOfSequence: 3 }))
          : Promise.resolve({ name: null, objective: null, executor: null, nextStep: { kind: "nothing" }, asOfSequence: 13 }),
      ),
    });
    await open(client);
    // The rail's prefetch is in flight for the generated row; the operator clicks it now.
    await waitFor(() => expect(client.getBriefing).toHaveBeenCalledWith(GENERATED));
    await userEvent.click(within(screen.getByLabelText("Projects")).getByTitle(GENERATED).closest("button")!);
    const strip = document.querySelector(".topbar") as HTMLElement;
    expect(await within(strip).findByRole("button", { name: GENERATED })).toBeInTheDocument();
    settle(undefined);
    expect(await within(strip).findByRole("button", { name: "Investigate slow login on mobile" })).toHaveAttribute("title", GENERATED);
    // One read for that run, not one per caller.
    expect(client.getBriefing.mock.calls.filter((call) => (call as unknown[])[0] === GENERATED)).toHaveLength(1);
  });
});

/**
 * #1098 D1 (MAJOR). The composer is chosen by `draft !== null` AHEAD of the selection, and `select`
 * never cleared the draft. So clicking a run in the rail moved the header to that run while the
 * BODY stayed on the New task draft, showing its fake one-node overview ("1 nodes", a `ready`
 * `start` card, "Nothing has run yet") over a run that has a real graph. It survived a second run
 * click and only `discard` cleared it.
 *
 * Selecting a run must leave the draft. And it must not DESTROY it: the operator's half-typed
 * objective is the one thing on that surface they cannot get back, so it is kept and restored the
 * next time the composer opens. `discard` — the control that promises erasure — still erases it.
 */
describe("selecting a run leaves the new-task draft", () => {
  const ROUTES = { configured: true, routes: [] };

  it("shows the selected run instead of the draft, and keeps the typed objective for later", async () => {
    const client = stubClient({ listRoutes: vi.fn(async () => ROUTES) });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    const box = await screen.findByLabelText("What should this task do?");
    await userEvent.type(box, "Rotate the staging credentials");

    const rail = screen.getByLabelText("Projects");
    await userEvent.click(within(rail).getByRole("button", { name: /^demo-calm/ }));

    // The body is the run's, not the draft's.
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "New task" })).not.toBeInTheDocument(),
    );
    expect(screen.queryByLabelText("What should this task do?")).not.toBeInTheDocument();

    // And the sentence was not thrown away: reopening the composer brings it back.
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    expect(await screen.findByLabelText("What should this task do?")).toHaveValue(
      "Rotate the staging credentials",
    );
  });

  it("still erases the objective when discard is pressed, because discard promises that", async () => {
    const client = stubClient({ listRoutes: vi.fn(async () => ROUTES) });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    await userEvent.type(
      await screen.findByLabelText("What should this task do?"),
      "Rotate the staging credentials",
    );
    await userEvent.click(screen.getByRole("button", { name: "Discard this task" }));

    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    expect(await screen.findByLabelText("What should this task do?")).toHaveValue("");
  });
});

/**
 * PR #1167 review, /root's BLOCKING P1. The repair above parked only the draft's TEXT. A draft is
 * not its text: it is an execution id reserved locally, and `start-<executionId>` is the
 * idempotency key every retry of that draft carries. Dropping the id on selection means the
 * composer that reopens MINTS A NEW ONE, so the retry of an UNKNOWN start is no longer a retry —
 * it is a second start under a second identity, and the Runtime has nothing left to reconcile it
 * against. The banner the operator is reading at that moment says the opposite in so many words:
 * "the retry carries the same identity, so it cannot start a second run."
 *
 * The mirror of the same gap: a selection during an IN-FLIGHT start made `stillMine` false, so a
 * start that then SUCCEEDED never cleared the sentence it consumed. Reopening the composer offered
 * the already-started objective back, ready to be sent again.
 */
describe("a parked draft keeps its identity, not just its words", () => {
  const ROUTES = { configured: true, routes: [] };

  it("retries an unknown start under the SAME identity after a selection parked the draft", async () => {
    const client = stubClient({
      listRoutes: vi.fn(async () => ROUTES),
      startTask: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        action: "start" as const,
        result: "unknown" as const,
      })),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    await userEvent.type(
      await screen.findByLabelText("What should this task do?"),
      "Rotate the staging credentials",
    );
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(1));
    const firstId = client.startTask.mock.calls[0][0] as string;
    const firstKey = (client.startTask.mock.calls[0][2] as { idempotencyKey: string }).idempotencyKey;

    // The operator goes to look at another run while the outcome is unknown, then comes back.
    const rail = screen.getByLabelText("Projects");
    await userEvent.click(within(rail).getByRole("button", { name: /^demo-calm/ }));
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    expect(await screen.findByLabelText("What should this task do?")).toHaveValue(
      "Rotate the staging credentials",
    );

    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(2));
    expect(client.startTask.mock.calls[1][0]).toBe(firstId);
    expect((client.startTask.mock.calls[1][2] as { idempotencyKey: string }).idempotencyKey).toBe(
      firstKey,
    );
  });

  it("keeps an UNKNOWN draft identity when New task is clicked directly", async () => {
    const client = stubClient({
      listRoutes: vi.fn(async () => ROUTES),
      startTask: vi.fn(async () => ({
        ...PAUSED_EVIDENCE,
        action: "start" as const,
        result: "unknown" as const,
      })),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    await userEvent.type(
      await screen.findByLabelText("What should this task do?"),
      "Retry this exact task",
    );
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(1));
    const firstId = client.startTask.mock.calls[0][0] as string;
    const firstKey = (client.startTask.mock.calls[0][2] as { idempotencyKey: string }).idempotencyKey;
    expect(await screen.findByText(/retry carries the same identity/i)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    expect(await screen.findByText(/retry carries the same identity/i)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(2));
    expect(client.startTask.mock.calls[1][0]).toBe(firstId);
    expect((client.startTask.mock.calls[1][2] as { idempotencyKey: string }).idempotencyKey).toBe(firstKey);
  });

  it("does not offer an already-started objective back when the selection landed mid-start", async () => {
    let release: () => void = () => {};
    const landed = new Promise<void>((resolve) => {
      release = resolve;
    });
    const client = stubClient({
      listRoutes: vi.fn(async () => ROUTES),
      startTask: vi.fn(async () => {
        await landed;
        return { ...PAUSED_EVIDENCE, action: "start" as const };
      }),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    await userEvent.type(
      await screen.findByLabelText("What should this task do?"),
      "Rotate the staging credentials",
    );
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(1));

    // The selection lands while the start is still in flight; THEN the start succeeds.
    const rail = screen.getByLabelText("Projects");
    await userEvent.click(within(rail).getByRole("button", { name: /^demo-calm/ }));
    release();
    await waitFor(() => expect(screen.queryByLabelText("What should this task do?")).not.toBeInTheDocument());

    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    expect(await screen.findByLabelText("What should this task do?")).toHaveValue("");
  });

  it("keeps a parked in-flight start pending when its draft is reopened", async () => {
    let release!: () => void;
    const landed = new Promise<void>((resolve) => {
      release = resolve;
    });
    let startSettled = false;
    let releaseRefresh!: () => void;
    const refresh = new Promise<void>((resolve) => {
      releaseRefresh = resolve;
    });
    let holdRefresh = false;
    const client = stubClient({
      listRoutes: vi.fn(async () => ROUTES),
      startTask: vi.fn(async () => {
        await landed;
        startSettled = true;
        return { ...PAUSED_EVIDENCE, action: "start" as const };
      }),
      getStatus: vi.fn(async (id: string) => {
        if (holdRefresh) await refresh;
        return { ...STATUS, executionId: id };
      }),
    });
    await open(client);
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    await userEvent.type(
      await screen.findByLabelText("What should this task do?"),
      "must not be sent twice",
    );
    await userEvent.click(screen.getByRole("button", { name: /start this task/i }));
    await waitFor(() => expect(client.startTask).toHaveBeenCalledTimes(1));
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    expect(client.startTask).toHaveBeenCalledTimes(1);

    await userEvent.click(within(screen.getByLabelText("Projects")).getByRole("button", { name: /^demo-calm/ }));
    await userEvent.click(screen.getByRole("button", { name: /new task in dale-api-base/i }));
    expect(await screen.findByLabelText("What should this task do?")).toBeDisabled();
    expect(screen.getByRole("button", { name: /starting/i })).toBeDisabled();

    // The old start is still pending, but the selected run starts its own read. Its busy state
    // must survive the old completion and its receipt must not be painted onto this run.
    await userEvent.click(within(screen.getByLabelText("Projects")).getByRole("button", { name: /^demo-calm/ }));
    holdRefresh = true;
    const refreshButton = screen.getByRole("button", { name: "Refresh" });
    const statusCallsBeforeRefresh = client.getStatus.mock.calls.length;
    await userEvent.click(refreshButton);
    await waitFor(() => expect(client.getStatus.mock.calls.length).toBeGreaterThan(statusCallsBeforeRefresh));
    release();
    await waitFor(() => expect(startSettled).toBe(true));
    expect(refreshButton).toBeDisabled();
    expect(screen.queryByText(/started — done/i)).not.toBeInTheDocument();
    releaseRefresh();
    await waitFor(() => expect(refreshButton).not.toBeDisabled());
  });

});


/** #197: optional naming must not hold required state behind its response, and neither a full
 * load nor a poll may overwrite a newer observation of the same run. These tests use the real
 * client deadline with controlled response bodies. Existing cross-run/error tests cover identity
 * and refusal; they cannot catch a late snapshot of the SAME run. Cost: jsdom/fake clock only. */
describe("execution reads preserve newer observed state", () => {
  function readingFixture() {
    let head = 2;
    const reads: { path: string; after: number; head: number }[] = [];
    let hold: (path: string, head: number) => Promise<void> | null = () => null;
    const row = () => ({
      executionId: "read-order", mode: "supervised", status: head === 2 ? "paused" : "running",
      attention: "can_sleep", startedAt: null, lastEventAt: null, headSequence: head,
    });
    const fetch = vi.fn(async (url: string) => {
      const parsed = new URL(url, "http://fixture.invalid");
      const capturedHead = head;
      const after = Number(parsed.searchParams.get("after") ?? 0);
      reads.push({ path: parsed.pathname, after, head: capturedHead });
      let data: unknown;
      if (parsed.pathname === "/health") data = {};
      else if (parsed.pathname === "/v1/executions") data = { executions: [row()], hasMore: false, nextCursor: null };
      else if (parsed.pathname === "/v1/gateway/routes") data = { configured: false, routes: [] };
      else if (parsed.pathname.endsWith("/briefing")) data = {
        name: null, objective: "The current run objective", executor: null,
        nextStep: { kind: "nothing" }, asOfSequence: capturedHead,
      };
      else if (parsed.pathname.endsWith("/events")) data = {
        head: capturedHead,
        events: [1, 2, 3].filter((sequence) => sequence > after && sequence <= capturedHead).map((sequence) => ({
          sequence, eventId: `read-event-${sequence}`, occurredAt: "2026-09-30T12:00:00Z",
          actor: { type: "system", id: "runtime" }, evidenceRefs: [],
          kind: { type: sequence === 1 ? "execution_form_declared" : sequence === 2 ? "execution_paused" : "execution_resumed",
            data: sequence === 1 ? { executionId: "read-order", nodeIds: ["step"], nodeTimeoutSeconds: {} } : { executionId: "read-order" } },
        })),
      };
      else if (parsed.pathname === "/v1/executions/read-order") data = {
        ...STATUS, ...row(), attentionReasons: [], nodeStateCounts: { ready: 1 },
      };
      else throw new Error(`unexpected fixture read: ${url}`);
      const pending = hold(parsed.pathname, capturedHead);
      return { ok: true, status: 200, json: async () => { if (pending) await pending; return { ok: true, data, diagnostics: [] }; } } as Response;
    });
    const client = new ReadClient("fixture-token", { fetch: fetch as typeof globalThis.fetch });
    return {
      reads,
      setHead: (next: number) => { head = next; },
      hold: (filter: typeof hold) => { hold = filter; },
      render: () => render(<App createClient={() => client} modelContext={null} session={async () => ({ token: "fixture-token", project: null })} />),
    };
  }

  async function settleReads() {
    await act(async () => { for (let turn = 0; turn < 30; turn += 1) await Promise.resolve(); });
  }

  function expectCurrent() {
    expect(screen.getByText("Execution / running")).toBeInTheDocument();
    expect(screen.queryByText("Execution / paused")).not.toBeInTheDocument();
  }

  it.each(["success", "body timeout"])("publishes required state before an optional briefing and preserves it after %s", async (outcome) => {
    vi.useFakeTimers();
    let release!: () => void;
    const pending = new Promise<void>((resolve) => { release = resolve; });
    try {
      const fixture = readingFixture();
      fixture.hold((path) => path.endsWith("/briefing") ? pending : null);
      fixture.render();
      await settleReads();
      expect(screen.getByRole("region", { name: "Team" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
      fixture.setHead(3);
      await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
      expectCurrent();
      if (outcome === "success") {
        await act(async () => { release(); });
        await settleReads();
        expect(within(document.querySelector(".topbar") as HTMLElement).getByRole("button", { name: "The current run objective" })).toBeInTheDocument();
      } else {
        await act(async () => { await vi.advanceTimersByTimeAsync(RUNTIME_REQUEST_TIMEOUT_MS - 4000); });
      }
      expectCurrent();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(fixture.reads.filter((read) => read.path.endsWith("/briefing"))).toHaveLength(1);
    } finally { cleanup(); vi.useRealTimers(); }
  });

  it("does not replace a newer poll with an older full event read", async () => {
    vi.useFakeTimers();
    let release!: () => void;
    const pending = new Promise<void>((resolve) => { release = resolve; });
    try {
      const fixture = readingFixture();
      fixture.hold((path, head) => path.endsWith("/events") && head === 2 ? pending : null);
      fixture.render();
      await settleReads();
      expect(screen.getByText("Opening this run…")).toBeInTheDocument();
      fixture.setHead(3);
      await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
      expectCurrent();
      await act(async () => { release(); });
      await settleReads();
      expectCurrent();
    } finally { cleanup(); vi.useRealTimers(); }
  });

  it("does not replace a newer manual refresh with an older poll status", async () => {
    vi.useFakeTimers();
    let release!: () => void;
    const pending = new Promise<void>((resolve) => { release = resolve; });
    try {
      const fixture = readingFixture();
      fixture.render();
      await settleReads();
      fixture.hold((path, head) => path === "/v1/executions/read-order" && head === 2 ? pending : null);
      await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
      fixture.setHead(3);
      fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
      await settleReads();
      expectCurrent();
      await act(async () => { release(); });
      await settleReads();
      expectCurrent();
    } finally { cleanup(); vi.useRealTimers(); }
  });
});
