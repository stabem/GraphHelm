/**
 * Page-level guards.
 *
 * These drive the real component with a stubbed `RuntimeClient`, so what they observe is what an
 * operator observes: the session that arrives without anyone typing, the projects rail, the board,
 * the window that opens on a node, and — the claim this whole feature rests on — that a WebMCP
 * tool call moves the HUMAN view, not just the agent's transcript.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import App from "./App";
import { saveProjectName, saveRemovedRuns } from "./studio-preferences";
import { resetPanelCaches } from "./components/panel";
import type { RuntimeClient } from "./runtime/client";
import { MAX_NODE_TIMEOUT_SECONDS } from "./runtime/client";
import type { MutationEvidence } from "./runtime/types";
import type { ModelContextLike, WebMcpToolDescriptor } from "./webmcp/adapter";

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
    getStatus: vi.fn(async () => ({ ...STATUS })),
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

describe("Studio organization and responsive navigation", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => localStorage.clear());

  it.each([
    ["needs_you", "Needs you"],
    ["can_sleep", "Can sleep"],
  ])("names lifecycle and %s verdict in the run header", async (attention, label) => {
    const client = stubClient({ getStatus: vi.fn(async () => ({ ...STATUS, attention })) });
    await open(client);
    const strip = document.querySelector(".topstrip") as HTMLElement;
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
    await waitFor(() => expect(pause).toBeEnabled());
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

/** The ordinary loop: the dev server hands the page a token and it opens connected. */
async function open(client: ReturnType<typeof stubClient>, modelContext: ModelContextLike | null = null) {
  render(
    <App
      createClient={() => client as unknown as RuntimeClient}
      modelContext={modelContext}
      session={async () => ({ token: "local-token", project: "dale-api-base" })}
    />,
  );
  await screen.findByLabelText("Projects");
  // These existing journeys exercise the free canvas; overview has dedicated default-view coverage.
  await userEvent.click(await screen.findByRole("button", { name: /^Free canvas$/ }));
  await userEvent.click(screen.getByText("Run actions"));
}

describe("opening", () => {
  it("opens the organized overview without applying saved canvas coordinates", async () => {
    render(<App createClient={() => stubClient() as unknown as RuntimeClient} modelContext={null} session={async () => ({token:"local-token",project:"GraphHelm"})} />);
    expect(await screen.findByRole("main",{name:"Work overview"})).toBeVisible();
    expect(screen.getByRole("button",{name:/^Overview$/})).toHaveAttribute("aria-pressed","true");
    expect(screen.queryByRole("combobox",{name:"Find on board"})).not.toBeInTheDocument();
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
});

describe("the board", () => {
  it("puts every declared node on it, including one nothing has happened to", async () => {
    await open(stubClient());
    const board = await screen.findByLabelText("Execution board");
    expect(within(board).getByText("implementation")).toBeInTheDocument();
    expect(within(board).getByText("deploy")).toBeInTheDocument();
  });

  /** Nothing proves the shape until a graph file's hash matches the run's, so nothing is drawn
   * and the note says so. */
  it("draws no connection until one is proven", async () => {
    await open(stubClient());
    const board = await screen.findByLabelText("Execution board");
    expect(board.querySelectorAll("path.edge")).toHaveLength(0);
    expect(screen.getByText(/work connections unverified/i)).toBeInTheDocument();
  });

  it("renders the log as text, never as markup", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run /);
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
    expect(await screen.findByLabelText("Execution board")).toBeInTheDocument();
    expect(await screen.findByLabelText(/^Run /)).toBeInTheDocument();
    expect(screen.queryByLabelText(/^Node /)).not.toBeInTheDocument();
  });

  /** The run window carries the sixteen states — zeroes included, because an omitted bucket reads
   * as "no such problem". */
  it("shows only the states that are something, and asserts the zeros in one line", async () => {
    await open(stubClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run /);
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
    const board = await screen.findByLabelText("Execution board");
    await userEvent.click(within(board).getByRole("button", { name: /implementation/i }));

    const nodePanel = await screen.findByLabelText("Node implementation");
    expect(within(nodePanel).getByText(/retryable failure/i)).toBeInTheDocument();
    // The roster declaration names no node, so a node thread must not carry it.
    expect(nodePanel.textContent).not.toContain("Declared 2 nodes");

    await userEvent.click(within(nodePanel).getByRole("button", { name: /close this node/i }));
    await waitFor(() => expect(screen.queryByLabelText(/^Node /)).not.toBeInTheDocument());
    expect(screen.getByLabelText("Execution board")).toBeInTheDocument();
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

    // The same head on the other verbs the dock fires against the rendered run.
    await userEvent.click(screen.getByRole("button", { name: "sweep" }));
    await waitFor(() => expect(client.sweep).toHaveBeenCalled());
    expect(optionsOf(vi.mocked(client.sweep).mock.calls, 1).ifMatch).toBe(STATUS.headSequence);
    await userEvent.click(screen.getByRole("button", { name: /pause · finish in-flight/i }));
    await waitFor(() => expect(client.pause).toHaveBeenCalled());
    expect(optionsOf(vi.mocked(client.pause).mock.calls, 1).ifMatch).toBe(STATUS.headSequence);
    const why = await screen.findByLabelText("Why this run needs you");
    await userEvent.click(within(why).getByRole("button", { name: /approve implementation/i }));
    await waitFor(() => expect(client.approve).toHaveBeenCalled());
    expect(optionsOf(vi.mocked(client.approve).mock.calls, 2).ifMatch).toBe(STATUS.headSequence);
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
    const cancel = screen.getByRole("button", { name: /Cancel execution/i });
    expect(cancel).toBeDisabled();
    expect(cancel.closest("span")).toHaveAttribute("title", expect.stringContaining("completed"));
    expect(client.cancel).not.toHaveBeenCalled();
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
  it("disables pause and cancel on a finished run, each wearing why", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({ ...STATUS, status: "completed", attention: "can_sleep", attentionReasons: [] })),
    });
    await open(client);
    const pause = await screen.findByRole("button", { name: /pause · finish in-flight/i });
    expect(pause).toBeDisabled();
    expect(pause.closest("span")).toHaveAttribute("title", expect.stringContaining("completed"));
    const cancel = screen.getByRole("button", { name: /Cancel execution/i });
    expect(cancel).toBeDisabled();
    expect(cancel.closest("span")).toHaveAttribute("title", expect.stringContaining("completed"));
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
        model: "claude-sonnet-5",
        profiles: ["critical_reasoning"],
        enabled: true,
      },
      {
        id: "retired_route",
        provider: "anthropic",
        transport: "direct_api",
        billingMode: "per_token",
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

  /** Blocks keep where they were dragged, per run, across reloads - so there has to be a way back
   * to the grid when a stored position buries one block under another. */
  it("tidies moved blocks back onto the grid", async () => {
    await open(stubClient());
    const sheet = document.querySelector(".sheet") as HTMLElement;
    const card = within(sheet).getByText("implementation").closest(".node") as HTMLElement;
    const home = card.style.left;

    fireEvent.pointerDown(card, { clientX: 10, clientY: 10, bubbles: true });
    fireEvent.pointerMove(window, { clientX: 300, clientY: 260, bubbles: true });
    fireEvent.pointerUp(window, { bubbles: true });
    const moved = card.style.left;

    await userEvent.click(screen.getByRole("button", { name: /tidy the board/i }));
    const tidied = (
      within(sheet).getByText("implementation").closest(".node") as HTMLElement
    ).style.left;
    expect(tidied).not.toBe(moved);
    expect(tidied).toBe(home);
  });

  /** The button exists because a project is a folder and the operator has more than one. What it
   * can do today is tell them exactly what to run; it must not pretend to do more. */
  it("says what adding a project folder actually takes", async () => {
    await open(stubClient());
    await userEvent.click(screen.getByRole("button", { name: /add project folder/i }));

    const panel = await screen.findByLabelText("Add project folder");
    await userEvent.type(within(panel).getByLabelText(/^folder$/i), "F:/github/Dale/dale-api-base");
    expect(within(panel).getByText(/graphhelm serve --events \.graphhelm\/events/i)).toBeInTheDocument();
    expect(within(panel).getByText(/F:\/github\/Dale\/dale-api-base/)).toBeInTheDocument();
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
    const panel = await screen.findByLabelText(/^Run /);
    expect(await within(panel).findByText(/waiting for a reply/i)).toBeInTheDocument();
  });

  it("drops the waiting line once the reply is in the log", async () => {
    const client = stubClient({
      getEvents: vi.fn(async () => ({ head: 9, events: conversationEvents(true) })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const panel = await screen.findByLabelText(/^Run /);
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
    const panel = await screen.findByLabelText(/^Run /);
    // No clicks after this point: the reply must surface by itself.
    expect(await within(panel).findByText("oi de volta")).toBeInTheDocument();
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
  it("says in words why the run needs you, next to the tag", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Why this run needs you");
    expect(within(why).getByText(/implementation is blocked/)).toBeInTheDocument();
  });

  /** The HEADLINE answers "needs me for what?" before the eye ever reaches the reason block:
   * the subtitle under "This run needs you" says what is owed, in words, not the wire state. */
  it("the header itself says what the run is waiting for", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const panel = await screen.findByLabelText(/^Run /);
    expect(within(panel).getByText("waiting for your go-ahead")).toBeInTheDocument();
  });

  it("offers approve right where the blocked reason is shown", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Why this run needs you");
    await userEvent.click(within(why).getByRole("button", { name: /approve implementation/i }));

    await waitFor(() => expect(client.approve).toHaveBeenCalled());
    const call = vi.mocked(client.approve).mock.calls[0] as unknown as [string, string];
    expect(call[0]).toBe("demo-deploy");
    expect(call[1]).toBe("implementation");
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

    const why = await screen.findByLabelText("Why this run needs you");
    expect(within(why).getByText(/implementation was interrupted/)).toBeInTheDocument();
    await userEvent.click(within(why).getByRole("button", { name: /approve implementation/i }));
    await waitFor(() => expect(client.approve).toHaveBeenCalled());
    const call = vi.mocked(client.approve).mock.calls[0] as unknown as [string, string];
    expect(call[1]).toBe("implementation");

    await waitFor(() => expect(screen.getByRole("button", { name: "resume" })).toBeEnabled());
  });

  it("names the missing model when a node waits for input and nothing is wired", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        attentionReasons: [{ kind: "waiting_input_node", node: "start" }],
      })),
      listRoutes: vi.fn(async () => ({ configured: false, routes: [] })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Why this run needs you");
    // No agent has actually asked anything, and the block says so instead of demanding an
    // answer to nothing — the debt, not just the debtor.
    expect(within(why).getByText(/nothing has asked you anything yet/)).toBeInTheDocument();
    expect(within(why).getByText(/no model is wired/i)).toBeInTheDocument();
  });

  it("the waiting-input action opens the thread and puts the cursor in the message box", async () => {
    const client = stubClient({
      getStatus: vi.fn(async () => ({
        ...STATUS,
        attentionReasons: [{ kind: "waiting_input_node", node: "start" }],
      })),
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Why this run needs you");
    await userEvent.click(within(why).getByRole("button", { name: /answer/i }));

    const box = await screen.findByLabelText(/say something into this run/i);
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
  it("the answer button retargets the composer to the room, visibly", async () => {
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

    // Lock the composer onto codex the way a person actually does: through a reply hint. With
    // codex's question already answered, the cards carry the honest "talk to" claim, never
    // "waiting for an answer".
    await screen.findByLabelText(/^Run demo-deploy/);
    const hints = await screen.findByLabelText(/talk to someone/i);
    await userEvent.click(within(hints).getByRole("button", { name: /message codex/i }));
    expect(screen.getAllByText(/→ codex/).length).toBeGreaterThan(0);

    // The banner's own action must undo that lock before promising the answer lands.
    const why = await screen.findByLabelText("Why this run needs you");
    await userEvent.click(within(why).getByRole("button", { name: /answer in the thread/i }));

    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "resposta pra sala");
    await userEvent.click(screen.getByRole("button", { name: /^send$/i }));
    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    expect(firstCall(client.signal)[2]).not.toHaveProperty("to");
  });

  it("quotes the unanswered question and addresses its asker", async () => {
    const client = askedClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Why this run needs you");
    // The debt itself, on screen — not just "waiting for input".
    expect(await within(why).findByText(/Qual porta devo usar para o deploy\?/)).toBeInTheDocument();

    // Answering from here delivers to the creditor.
    await userEvent.click(within(why).getByRole("button", { name: /answer codex/i }));
    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "usa a 8080");
    await userEvent.click(screen.getByRole("button", { name: /^send$/i }));
    await waitFor(() => expect(client.signal).toHaveBeenCalled());
    expect(firstCall(client.signal)[2]).toMatchObject({ to: "codex" });
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

    const why = await screen.findByLabelText("Why this run needs you");
    await waitFor(() =>
      expect(within(why).queryByText(/Qual porta devo usar/)).not.toBeInTheDocument(),
    );
    expect(await within(why).findByText(/nothing has asked you anything yet/)).toBeInTheDocument();
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
    let attempts = 0;
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
        attempts += 1;
        if (attempts === 1) throw new Error("repository storage operation failed");
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
  /** Four of five reviewers, blind to each other: the run-name button bumps the focus nonce
   * without owning the recipient, so a long-dead "answer X" choice re-applied itself. */
  it("reopening the panel from the run's name speaks to the room, not to a stale asker", async () => {
    const client = askedClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    // Address the pending asker through the banner, then un-choose it.
    const why = await screen.findByLabelText("Why this run needs you");
    await userEvent.click(within(why).getByRole("button", { name: /answer codex/i }));
    await userEvent.click(screen.getByRole("button", { name: /speak to the room instead/i }));

    // Reopen the panel from the top strip: the old choice must NOT come back.
    const strip = document.querySelector(".topstrip")!;
    await userEvent.click(within(strip as HTMLElement).getByRole("button", { name: "demo-deploy" }));
    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "pra sala");
    await userEvent.click(screen.getByRole("button", { name: /^send$/i }));
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

    const why = await screen.findByLabelText("Why this run needs you");
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

    const why = await screen.findByLabelText("Why this run needs you");
    await waitFor(() =>
      expect(within(why).queryByText(/aqui esta o relatorio/)).not.toBeInTheDocument(),
    );
    expect(within(why).getByText(/nothing has asked you anything yet/)).toBeInTheDocument();
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
    const why = await screen.findByLabelText("Why this run needs you");
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
    expect(screen.getByLabelText("Execution board")).toBeInTheDocument();
    expect(screen.queryByText(/This board is empty/)).not.toBeInTheDocument();

    const strip = document.querySelector(".topstrip")!;
    await userEvent.click(within(strip as HTMLElement).getByRole("button", { name: "demo-deploy" }));
    expect(await screen.findByLabelText(/^Run demo-deploy/)).toBeInTheDocument();
  });

  it("typing in the graph-file box does not fire the shape check by itself", async () => {
    const client = stubClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    await screen.findByLabelText(/^Run demo-deploy/);

    await userEvent.click(screen.getByRole("button", { name: /Verify graph/i }));
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
    const strip = document.querySelector(".topstrip")!;
    await userEvent.click(within(strip as HTMLElement).getByRole("button", { name: "demo-deploy" }));

    const box = await screen.findByLabelText(/say something into this run/i);
    expect(box).toHaveValue("resposta pela metade");
  });

  /** Under the honest "Talk to someone" claim, the button verb was still "answer" — asserting
   * the very debt the heading denies. */
  it("recent speakers are offered with an honest verb, not 'answer'", async () => {
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
    const hints = await screen.findByLabelText(/talk to someone/i);
    expect(within(hints).getByRole("button", { name: /message codex/i })).toBeInTheDocument();
    expect(within(hints).queryByRole("button", { name: /answer codex/i })).not.toBeInTheDocument();
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
    expect(
      await screen.findByText(/a waiting node wants an answer in the thread/i),
    ).toBeInTheDocument();
    const approve = screen.getByRole("button", { name: /nothing to approve/i });
    expect(approve).not.toHaveAttribute("title");
  });
});

/**
 * ROUND-4 VALIDATION FIXES. The fourth five-persona review returned 59/60 FIXED on round 3 and
 * found the finer defects below — the worst being a reflex arc the UI itself had severed.
 */
describe("round-4: the debt can actually be settled", () => {
  /** The ledger retires a question ONLY via an owner reply whose replyTo names it — and the
   * banner's own "answer X" button sent {to} with no replyTo, so answering through the UI left
   * the debt immortal (two blind reviewers). */
  it("answering through the banner sends the replyTo the ledger settles by", async () => {
    const client = askedClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Why this run needs you");
    await within(why).findByText(/Qual porta devo usar/);
    await userEvent.click(within(why).getByRole("button", { name: /answer codex/i }));
    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "usa a 8080");
    await userEvent.click(screen.getByRole("button", { name: /^send$/i }));

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

    const why = await screen.findByLabelText("Why this run needs you");
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
    await userEvent.click(screen.getByRole("button", { name: /^send$/i }));

    await screen.findByRole("alert");
    expect(screen.getByLabelText(/say something into this run/i)).toHaveValue("nao me perde");
  });

  it("a delivered send clears the box", async () => {
    const client = askedClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const box = await screen.findByLabelText(/say something into this run/i);
    await userEvent.type(box, "entregue");
    await userEvent.click(screen.getByRole("button", { name: /^send$/i }));
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
    await userEvent.click(screen.getByRole("button", { name: /^send$/i }));
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

  /** The agent window is re-addressed by a prop change without remounting: half of A's message
   * would ship to B on the next Enter. The window is keyed by who it belongs to. */
  it("switching agent windows never carries the draft over", async () => {
    const client = askedClient({
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
    });
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const board = await screen.findByLabelText("Execution board");
    await userEvent.click(within(board).getByRole("button", { name: /codex/i }));
    const agentBox = await within(
      await screen.findByLabelText("Agent codex"),
    ).findByLabelText(/say something into this run/i);
    await userEvent.type(agentBox, "so para codex");

    await userEvent.click(within(board).getByRole("button", { name: /claude-revisor/i }));
    const otherBox = await within(
      await screen.findByLabelText("Agent claude-revisor"),
    ).findByLabelText(/say something into this run/i);
    expect(otherBox).toHaveValue("");
  });
});

describe("round-4: the instruments admit their own state", () => {
  /** Every sealed item was opened twice (hooks + the Said bubble), and a second panel
   * re-fetched everything: the cache's own comment claimed sharing the code did not do. */
  it("opens each sealed item exactly once across the whole page", async () => {
    const client = askedClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const why = await screen.findByLabelText("Why this run needs you");
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
      timeout: 2000,
    });
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
    const board = await screen.findByLabelText("Execution board");
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(
      within(board).queryByRole("button", { name: /studio-operator/i }),
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
    await screen.findByText(/porta certa/);

    await userEvent.type(screen.getByLabelText(/search this conversation/i), "porta");
    expect(screen.getByText(/porta certa/)).toBeInTheDocument();
    expect(within(document.querySelector(".talk") as HTMLElement).queryByText(/fila esta vazia/)).not.toBeInTheDocument();
    // The narrowing is announced - a thread that silently hides is a thread that lies.
    expect(screen.getByText(/1 of 2/)).toBeInTheDocument();

    await userEvent.clear(screen.getByLabelText(/search this conversation/i));
    expect(await within(document.querySelector(".talk") as HTMLElement).findByText(/fila esta vazia/)).toBeInTheDocument();
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
    await screen.findByText(/porta certa/);

    await userEvent.click(screen.getByRole("button", { name: "#13" }));
    expect(written).toContain("demo-deploy#13");
  });
});

describe("the board remembers its graph file", () => {
  afterEach(() => localStorage.removeItem("graphhelm.studio.board.demo-deploy"));

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

    const panel = await screen.findByLabelText(/^Run /);
    const said = await within(panel).findByText(
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

    const panel = await screen.findByLabelText(/^Run /);
    expect(await within(panel).findByText(/cannot open it/i)).toBeInTheDocument();
    expect(within(panel).getByText(/no key for this evidence/)).toBeInTheDocument();
  });

  /** The way back in. The assertion is on what reached the CLIENT, not on the box emptying: a
   * composer that clears itself and sends nothing is the failure this is here to catch. */
  it("sends what was typed into the run's log", async () => {
    const client = talkingClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));

    const panel = await screen.findByLabelText(/^Run /);
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
    const panel = await screen.findByLabelText(/^Run /);
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

  /** THE CREW LIVES ON THE CANVAS - the owner drew it: agents in their own card, avatar and
   * name, distinct from nodes. Personas wear the "persona" role tag; plain agents do not. */
  it("shows the crew on the canvas, personas marked as personas", async () => {
    await open(roomClient());
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const crew = await screen.findByLabelText("Agents in this room");
    const persona = await within(crew).findByRole("button", { name: /seguranca/ });
    expect(within(persona).getByText("persona")).toBeInTheDocument();
    const agent = within(crew).getByRole("button", { name: /^codex$/i });
    expect(within(agent).queryByText("persona")).not.toBeInTheDocument();
  });

  /** Clicking a crew member opens the INDIVIDUAL conversation: the thread filtered to that
   * agent's exchanges, and a say box locked to them - a window named after someone that posted
   * to the room would put words where nobody sent them. */
  it("opens an individual conversation from the crew, addressed and filtered", async () => {
    const client = roomClient();
    await open(client);
    await userEvent.click(await screen.findByRole("button", { name: "demo-deploy" }));
    const crew = await screen.findByLabelText("Agents in this room");
    await userEvent.click(await within(crew).findByRole("button", { name: /seguranca/ }));

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
    const panel = await screen.findByLabelText(/^Run /);
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
    const panel = await screen.findByLabelText(/^Run /);
    expect(await within(panel).findByText("concordo com o plano")).toBeInTheDocument();
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

    const panel = await screen.findByLabelText(/^Run /);
    await userEvent.type(
      within(panel).getByLabelText(/say something into this run/i),
      "did that land?",
    );
    await userEvent.click(within(panel).getByRole("button", { name: /^send$/i }));

    expect(await within(panel).findByText(/could not be confirmed/i)).toBeInTheDocument();
  });
});
