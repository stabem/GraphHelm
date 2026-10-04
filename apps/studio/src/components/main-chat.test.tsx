import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { MainChat, type MainChatProps } from "./main-chat";
import { RuntimeError } from "../runtime/client";

/* Meaningful-test audit: these tests observe the user-facing dispatch and receipt boundary. They
 * catch wrong principal/fanout targets, missing charter prefixes, duplicate retries, stale scope
 * writes, and controls that call a Runtime with no recipients. Existing NativeChats tests cover a
 * single bound chat, so they do not observe this batch contract. Cost: mocked I/O and jsdom only;
 * the suite runs in a few seconds and uses no network, credentials, or browser session. */

const personas = [
  { chat: { id: "main", title: "Coordinator", projectDirectory: "C:/main", updatedAt: 3 }, charter: "Coordinate the work.", nodeId: "node-main" },
  { chat: { id: "one", title: "Reviewer", projectDirectory: "C:/one", updatedAt: 2 }, charter: "Review carefully.", nodeId: "node-one" },
  { chat: { id: "two", title: "Builder", projectDirectory: "C:/two", updatedAt: 1 }, charter: "Build the fix.", nodeId: "node-two" },
  { chat: { id: "three", title: "Verifier", projectDirectory: "C:/three", updatedAt: 0 }, charter: "Verify the result.", nodeId: "node-three" },
] satisfies MainChatProps["personas"];

type TestClient = { listNativeChatRequests: ReturnType<typeof vi.fn>; sendNativeChat: ReturnType<typeof vi.fn> };

function runtime(overrides: Partial<TestClient> = {}): TestClient & NonNullable<MainChatProps["client"]> {
  return {
    listNativeChatRequests: vi.fn().mockResolvedValue({ requests: [] }),
    sendNativeChat: vi.fn().mockResolvedValue({ requestId: "accepted" }),
    ...overrides,
  } as TestClient & NonNullable<MainChatProps["client"]>;
}

describe("MainChat", () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  // Mocked ledger/DOM only: an uncertain worker must not lock a different coordinator.
  // Existing same-chat recovery tests miss this cross-recipient lock. This protects both
  // independent dispatch and duplicate prevention, with no production seam or network.
  it("lets the coordinator send while keeping an uncertain worker and team locked", async () => {
    const client = runtime({ listNativeChatRequests: vi.fn().mockResolvedValue({ requests: [
      { requestId: "worker-unknown", nodeId: "node-two", threadId: "two", title: "Builder", sourceDirectory: "C:/two", state: "unobserved" },
    ] }) });
    render(<MainChat client={client} executionId="run-independent" personas={personas} />);
    await screen.findByText("Request history (1)");
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Report the next step" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Send to team (3 others)" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Main recipient"), { target: { value: "two" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Main recipient"), { target: { value: "main" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
    await waitFor(() => expect(client.sendNativeChat).toHaveBeenCalledTimes(1));
    expect(client.sendNativeChat.mock.calls[0][1].threadId).toBe("main");
    expect(screen.getByText("Request history (2)")).toBeInTheDocument();
  });

  // DOM-only: protects advisory/current-head advice and draft-only use while dispatch is locked.
  it("offers current JEV drafts without sending or overwriting a draft and rejects stale advice", async () => {
    const client = runtime({ listNativeChatRequests: vi.fn().mockResolvedValue({ requests: [{ requestId: "unknown", nodeId: "node-main", threadId: "main", title: "Coordinator", sourceDirectory: "C:/main", state: "unobserved" }] }) });
    const suggestions = { executionId: "run-1", headSequence: 7, state: "ready" as const, suggestions: [
      { to: "builder", draft: "Inspect the open issues and propose a work split.", reason: "No work split was recorded.", sourceSequences: [7] },
      { to: null, draft: "Check the pending request before dispatching more work.", reason: "The outcome is unknown.", sourceSequences: [7] },
    ] };
    const view = render(<MainChat client={client} executionId="run-1" personas={personas} refreshSequence={7} replySuggestions={suggestions} />);
    const advice = await screen.findByRole("region", { name: "JEV next step" });
    expect(advice).toHaveTextContent("No work split was recorded.");
    const history = screen.getByText("Request history (1)").closest("details")!;
    expect(history.open).toBe(false);
    expect(screen.getByText("Another suggestion").closest("details")?.open).toBe(false);
    expect(screen.getByLabelText("Instruction").compareDocumentPosition(history) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Use suggestion 1" }));
    expect(screen.getByLabelText("Instruction")).toHaveValue(suggestions.suggestions[0].draft);
    fireEvent.click(screen.getByText("Another suggestion"));
    expect(screen.getByRole("button", { name: "Use suggestion 2" })).toBeDisabled();
    expect(screen.getByLabelText("Instruction")).toHaveFocus();
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    expect(client.sendNativeChat).not.toHaveBeenCalled();
    view.rerender(<MainChat client={client} executionId="run-1" personas={personas} refreshSequence={8} replySuggestions={suggestions} replyIssue="Waiting for fresh JEV advice." />);
    expect(screen.queryByText(suggestions.suggestions[0].reason)).not.toBeInTheDocument();
    expect(screen.getByText("Waiting for fresh JEV advice.")).toBeInTheDocument();
    expect(screen.getByLabelText("Instruction")).toHaveValue(suggestions.suggestions[0].draft);
  });

  it("sends the selected principal or the three real other personas with distinct chartered requests", async () => {
    const client = runtime();
    render(<MainChat client={client} executionId="run-1" personas={personas} />);
    await waitFor(() => expect(client.listNativeChatRequests).toHaveBeenCalledWith("run-1"));
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Inspect this order" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to team (3 others)" }));
    await waitFor(() => expect(client.sendNativeChat).toHaveBeenCalledTimes(3));
    const calls = client.sendNativeChat.mock.calls.map(([, request]) => request);
    expect(calls.map((request) => request.threadId)).toEqual(["one", "two", "three"]);
    expect(new Set(calls.map((request) => request.requestId)).size).toBe(3);
    expect(calls.map((request) => request.message)).toEqual([
      "Persona instructions for this activity:\nReview carefully.\n\nInspect this order",
      "Persona instructions for this activity:\nBuild the fix.\n\nInspect this order",
      "Persona instructions for this activity:\nVerify the result.\n\nInspect this order",
    ]);
  });

  // Mocked Runtime admission is exclusive, as the real durable-intent lock is. Concurrent
  // requests spend their confirmation window queued behind one another. Existing immediate
  // responses miss that pressure. Cost: three 20ms I/O delays and jsdom, no real server.
  it("gets all team admissions confirmed when the Runtime accepts one intent at a time", async () => {
    let admitting = false;
    const client = runtime({ sendNativeChat: vi.fn(async (_execution, request) => {
      if (admitting) throw new Error("Runtime admission deadline elapsed while queued");
      admitting = true;
      await new Promise((resolve) => setTimeout(resolve, 20));
      admitting = false;
      return { requestId: request.requestId };
    }) });
    render(<MainChat client={client} executionId="run-admission" personas={personas} />);
    await waitFor(() => expect(client.listNativeChatRequests).toHaveBeenCalled());
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Take your own lane" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to team (3 others)" }));
    await screen.findByText("The request was accepted. Waiting for authoritative chat receipts.");
    expect(client.sendNativeChat).toHaveBeenCalledTimes(3);
    expect(client.sendNativeChat.mock.calls.map(([, request]) => request.threadId)).toEqual(["one", "two", "three"]);
    expect(screen.queryByText(/could not be proven; no request was retried/)).not.toBeInTheDocument();
  });

  it("keeps partial failures truthful and never retries a failed recipient", async () => {
    const client = runtime({ sendNativeChat: vi.fn()
      .mockResolvedValueOnce({ requestId: "ok" })
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({ requestId: "ok-3" }) });
    render(<MainChat client={client} executionId="run-1" personas={personas} />);
    await waitFor(() => expect(client.listNativeChatRequests).toHaveBeenCalledWith("run-1"));
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Do the work" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to team (3 others)" }));
    await screen.findByText(/1 request could not be proven/);
    expect(client.sendNativeChat).toHaveBeenCalledTimes(3);
    expect(screen.getByText(/Unobserved · outcome not proven/)).toBeInTheDocument();
  });

  it("ignores a late receipt from the previous execution scope", async () => {
    let resolveOld!: (value: { requests: never[] }) => void;
    const old = new Promise<{ requests: never[] }>((resolve) => { resolveOld = resolve; });
    const client = runtime({ listNativeChatRequests: vi.fn().mockImplementation((execution: string) => execution === "old" ? old : Promise.resolve({ requests: [{ requestId: "new", nodeId: "node-main", threadId: "main", title: "Coordinator", sourceDirectory: "C:/main", state: "completed" as const, text: "new reply" }] })) });
    const view = render(<MainChat client={client} executionId="old" personas={personas} />);
    view.rerender(<MainChat client={client} executionId="new" personas={personas} />);
    await screen.findAllByText("new reply");
    resolveOld({ requests: [] });
    expect(screen.queryByText("old")).not.toBeInTheDocument();
  });

  // DOM-only: protects the operator-facing current outcome against stale blocked history.
  // A credible regression is selecting the last blocked row unconditionally, which makes a
  // proven newer reply look blocked and hides the reply in a collapsed disclosure. Existing
  // request-history tests do not cover ordering across terminal outcomes or this visible surface.
  // Cost: mocked ledger read and jsdom only; completes in milliseconds with no network.
  it("shows the newer completed reply as current while retaining an older blocked request in history", async () => {
    sessionStorage.setItem(`graphhelm.main-chat.recovery:${location.origin}:run-ordered`, JSON.stringify([
      { executionId: "run-ordered", requestId: "new-completed", nodeId: "node-main", threadId: "main" },
    ]));
    let reads = 0;
    const client = runtime({ listNativeChatRequests: vi.fn().mockImplementation(() => {
      const pending = reads++ < 2;
      return Promise.resolve({ requests: [
        { requestId: "old-blocked", nodeId: "node-main", threadId: "main", title: "Coordinator", sourceDirectory: "C:/main", state: "blocked" as const, detail: "native resume rejected" },
        { requestId: "new-completed", nodeId: "node-main", threadId: "main", title: "Coordinator", sourceDirectory: "C:/main", state: pending ? "received" as const : "completed" as const, ...(pending ? {} : { text: "The next step is ready." }) },
      ] });
    }) });
    render(<MainChat client={client} executionId="run-ordered" personas={personas} />);

    await screen.findByText("Received · waiting for chat reply");
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Wait for the coordinator" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Refresh request status" }));
    expect(await screen.findByRole("region", { name: "Latest chat reply" })).toHaveTextContent("The next step is ready.");
    expect(within(screen.getByLabelText("Request status")).getByRole("status")).toHaveTextContent("latest request completed");
    expect(screen.queryByText("Check the original chat before sending another instruction.")).not.toBeInTheDocument();
    const history = screen.getByText("Request history (2)").closest("details")!;
    expect(history.open).toBe(false);
    fireEvent.click(history.querySelector("summary")!);
    expect(screen.getByText("native resume rejected")).toBeInTheDocument();
  });

  // DOM-only: a roster refresh must not discard an authoritative unresolved receipt while its
  // next read is pending. Existing recovery tests cover local IDs, not ledger-only requests.
  // Cost: one unresolved mocked I/O read and jsdom; no network or production-only seam.
  it("keeps ledger-only pending requests locked while a changed roster is being reread", async () => {
    let rereading = false;
    const client = runtime({ listNativeChatRequests: vi.fn().mockImplementation(() => rereading
      ? new Promise(() => {})
      : Promise.resolve({ requests: [{ requestId: "ledger-pending", nodeId: "node-main", threadId: "main", title: "Coordinator", sourceDirectory: "C:/main", state: "unobserved" as const }] })) });
    const view = render(<MainChat client={client} executionId="run-roster" personas={personas} />);
    await screen.findAllByText("Unobserved · outcome not proven");
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Keep this draft" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    const previousReads = client.listNativeChatRequests.mock.calls.length;
    rereading = true;
    view.rerender(<MainChat client={client} executionId="run-roster" personas={[...personas]} />);
    await waitFor(() => expect(client.listNativeChatRequests.mock.calls.length).toBeGreaterThan(previousReads));
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    expect(screen.getByLabelText("Instruction")).toHaveValue("Keep this draft");
    expect(screen.getAllByText("Unobserved · outcome not proven").length).toBeGreaterThan(0);
    expect(client.sendNativeChat).not.toHaveBeenCalled();
  });

  it("does not call the Runtime when there are no linked personas", () => {
    const client = runtime();
    const onConnect = vi.fn();
    render(<MainChat client={client} executionId="run-1" personas={[]} onConnect={onConnect} />);
    fireEvent.click(screen.getByRole("button", { name: "Connect existing chat" }));
    expect(onConnect).toHaveBeenCalledTimes(1);
    expect(client.listNativeChatRequests).not.toHaveBeenCalled();
    expect(client.sendNativeChat).not.toHaveBeenCalled();
  });

  it("locks a pending batch and keeps its request rows visible", async () => {
    let resolve!: (value: { requestId: string }) => void;
    const client = runtime({ sendNativeChat: vi.fn(() => new Promise<{ requestId: string }>(resolvePromise => { resolve = resolvePromise; })) });
    render(<MainChat client={client} executionId="run-1" personas={personas.slice(0, 2)} />);
    await waitFor(() => expect(client.listNativeChatRequests).toHaveBeenCalledWith("run-1"));
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Wait for this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    expect(screen.getByText(/Wait for this/)).toBeInTheDocument();
    expect(screen.getByLabelText("Instruction")).toBeEnabled();
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Prepare the next order" } });
    expect(screen.getByLabelText("Instruction")).toHaveValue("Prepare the next order");
    expect(client.sendNativeChat).toHaveBeenCalledTimes(1);
    resolve({ requestId: "accepted" });
  });

  it("unlocks and clears the composer when the execution scope changes during a pending send", async () => {
    let resolve!: (value: { requestId: string }) => void;
    const client = runtime({ sendNativeChat: vi.fn(() => new Promise<{ requestId: string }>(done => { resolve = done; })) });
    const view = render(<MainChat client={client} executionId="old-run" personas={personas.slice(0, 2)} />);
    await waitFor(() => expect(client.listNativeChatRequests).toHaveBeenCalledWith("old-run"));
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Old run instruction" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
    view.rerender(<MainChat client={client} executionId="new-run" personas={personas.slice(0, 2)} />);
    expect(screen.getByLabelText("Instruction")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "New run instruction" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Send to main chat" })).toBeEnabled());
    resolve({ requestId: "old-accepted" });
  });

  it("keeps sending disabled until the initial ledger read finishes or fails", async () => {
    let resolveRead!: (value: { requests: never[] }) => void;
    const client = runtime({ listNativeChatRequests: vi.fn(() => new Promise<{ requests: never[] }>(resolve => { resolveRead = resolve; })) });
    const delayed = render(<MainChat client={client} executionId="run-delayed" personas={personas} />);
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Wait for ledger" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    resolveRead({ requests: [] });
    await waitFor(() => expect(screen.getByRole("button", { name: "Send to main chat" })).toBeEnabled());
    expect(client.sendNativeChat).not.toHaveBeenCalled();
    delayed.unmount();

    const failed = runtime({ listNativeChatRequests: vi.fn().mockRejectedValue(new Error("ledger offline")) });
    render(<MainChat client={failed} executionId="run-failed" personas={personas} />);
    await screen.findByText("ledger offline");
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Do not send" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    expect(failed.sendNativeChat).not.toHaveBeenCalled();
  });

  it("retries a failed ledger read when the roster arrives late and refresh succeeds", async () => {
    const list = vi.fn().mockRejectedValue(new Error("ledger unavailable"));
    const client = runtime({ listNativeChatRequests: list });
    const view = render(<MainChat client={client} executionId="run-late-roster" personas={[]} />);
    view.rerender(<MainChat client={client} executionId="run-late-roster" personas={personas} />);
    await screen.findByText("ledger unavailable");
    expect(screen.getByRole("button", { name: "Refresh request status" })).toBeEnabled();
    list.mockResolvedValue({ requests: [] });
    fireEvent.click(screen.getByRole("button", { name: "Refresh request status" }));
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Retry after recovery" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Send to main chat" })).toBeEnabled());
    expect(screen.queryByText("ledger unavailable")).not.toBeInTheDocument();
  });

  it("retains recovered identities across roster changes and reloads until a terminal receipt", async () => {
    sessionStorage.setItem(`graphhelm.main-chat.recovery:${location.origin}:run-reload`, JSON.stringify([
      { executionId: "run-reload", requestId: "request-unknown", nodeId: "node-removed", threadId: "removed" },
    ]));
    let terminal = false;
    const client = runtime({ listNativeChatRequests: vi.fn(() => terminal ? Promise.resolve({ requests: [{ requestId: "request-unknown", nodeId: "node-removed", threadId: "removed", title: "Recovered", sourceDirectory: "", state: "completed" as const }] }) : Promise.resolve({ requests: [] })) });
    const first = render(<MainChat client={client} executionId="run-reload" personas={personas.slice(0, 2)} />);
    await screen.findAllByText(/Retained native chat request/);
    expect(screen.getAllByText(/request-unknown/)[0]).toBeInTheDocument();
    first.rerender(<MainChat client={client} executionId="run-reload" personas={personas.slice(0, 1)} />);
    expect(screen.getAllByText(/request-unknown/)[0]).toBeInTheDocument();
    first.unmount();
    render(<MainChat client={client} executionId="run-reload" personas={personas.slice(0, 1)} />);
    expect((await screen.findAllByText(/request-unknown/))[0]).toBeInTheDocument();
    expect(JSON.parse(sessionStorage.getItem(`graphhelm.main-chat.recovery:${location.origin}:run-reload`)!)).toEqual([
      { executionId: "run-reload", requestId: "request-unknown", nodeId: "node-removed", threadId: "removed" },
    ]);
    terminal = true;
    fireEvent.click(screen.getByRole("button", { name: "Refresh request status" }));
    await screen.findByText(/Completed · chat reply received/);
    expect(sessionStorage.getItem(`graphhelm.main-chat.recovery:${location.origin}:run-reload`)).toBe("[]");
  });

  it("releases typed pre-intent refusals across reload but retains ambiguous HTTP errors", async () => {
    const refusal = new RuntimeError("A native turn is already in flight", 400, [{
      code: "GHCLI001_ARGUMENT_INVALID", severity: "error", message: "Busy thread", path: "/threadId", source: "serve-cli",
    }]);
    const client = runtime({ sendNativeChat: vi.fn().mockRejectedValue(refusal) });
    const first = render(<MainChat client={client} executionId="run-refused" personas={personas} />);
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Rejected work" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Send to main chat" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
    await screen.findByText(/Blocked · action needed/);
    first.unmount();
    const restored = render(<MainChat client={client} executionId="run-refused" personas={personas} />);
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Explicit new instruction" } });
    await waitFor(() => expect(screen.getByRole("button", { name: "Send to main chat" })).toBeEnabled());
    expect(screen.queryByText(/Unobserved · outcome not proven/)).not.toBeInTheDocument();
    expect(client.sendNativeChat).toHaveBeenCalledTimes(1);
    restored.unmount();

    for (const [id, error] of [
      ["timeout", new RuntimeError("Unknown HTTP failure", 408, [])],
      ["opaque", new RuntimeError("Opaque HTTP refusal", 400, [])],
      ["conflict", new RuntimeError("Immutable request-ID conflict", 400, [{ code: "GHCLI001_ARGUMENT_INVALID", severity: "error", message: "Reused ID", path: "/requestId", source: "serve-cli" }])],
    ] as const) {
      const execution = `run-ambiguous-${id}`;
      const ambiguous = runtime({ sendNativeChat: vi.fn().mockRejectedValue(error) });
      const view = render(<MainChat client={ambiguous} executionId={execution} personas={personas} />);
      fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Keep the original ID" } });
      await waitFor(() => expect(screen.getByRole("button", { name: "Send to main chat" })).toBeEnabled());
      fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
      await screen.findByText(/Unobserved · outcome not proven/);
      expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
      expect(JSON.parse(sessionStorage.getItem(`graphhelm.main-chat.recovery:${location.origin}:${execution}`)!)).toHaveLength(1);
      view.unmount();
    }
  });

  it("shows a retained request identity even when ledger reconciliation fails", async () => {
    sessionStorage.setItem(`graphhelm.main-chat.recovery:${location.origin}:run-offline`, JSON.stringify([
      { executionId: "run-offline", requestId: "pending-offline", nodeId: "node-main", threadId: "main" },
    ]));
    const client = runtime({ listNativeChatRequests: vi.fn().mockRejectedValue(new Error("ledger unavailable")) });
    render(<MainChat client={client} executionId="run-offline" personas={personas} />);
    await screen.findByText("ledger unavailable");
    expect(screen.getAllByText(/pending-offline/)[0]).toBeInTheDocument();
    expect(screen.getByLabelText("Instruction")).toBeEnabled();
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Do not repeat" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    expect(client.sendNativeChat).not.toHaveBeenCalled();
  });

  it("stores only request identity and stops before dispatch when storage fails or is malformed", async () => {
    const client = runtime();
    const storedView = render(<MainChat client={client} executionId="run-storage" personas={personas} />);
    await waitFor(() => expect(client.listNativeChatRequests).toHaveBeenCalledWith("run-storage"));
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Persist identity" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
    await waitFor(() => expect(client.sendNativeChat).toHaveBeenCalledTimes(1));
    const stored = JSON.parse(sessionStorage.getItem(`graphhelm.main-chat.recovery:${location.origin}:run-storage`)!);
    expect(stored).toHaveLength(1);
    expect(Object.keys(stored[0]).sort()).toEqual(["executionId", "nodeId", "requestId", "threadId"]);
    expect(JSON.stringify(stored)).not.toContain("Persist identity");
    storedView.unmount();

    sessionStorage.setItem(`graphhelm.main-chat.recovery:${location.origin}:run-malformed`, JSON.stringify([
      { executionId: "run-malformed", requestId: "id", nodeId: "node", threadId: "thread", message: "secret" },
    ]));
    const malformed = runtime();
    const malformedView = render(<MainChat client={malformed} executionId="run-malformed" personas={personas} />);
    await waitFor(() => expect(malformed.listNativeChatRequests).not.toHaveBeenCalled());
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Must not send" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    expect(malformed.sendNativeChat).not.toHaveBeenCalled();
    malformedView.unmount();

    const failingStorage = runtime();
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("storage full"); });
    render(<MainChat client={failingStorage} executionId="run-write-failed" personas={personas} />);
    await waitFor(() => expect(failingStorage.listNativeChatRequests).toHaveBeenCalledWith("run-write-failed"));
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Must not dispatch" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
    await screen.findByText("Recovery storage is unavailable. Nothing was sent.");
    expect(failingStorage.sendNativeChat).not.toHaveBeenCalled();
    setItem.mockRestore();
  });
});
