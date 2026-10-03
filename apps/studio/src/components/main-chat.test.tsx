import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { MainChat, type MainChatProps } from "./main-chat";

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
  it("sends the selected principal or the three real other personas with distinct chartered requests", async () => {
    const client = runtime();
    render(<MainChat client={client} executionId="run-1" personas={personas} />);
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

  it("keeps partial failures truthful and never retries a failed recipient", async () => {
    const client = runtime({ sendNativeChat: vi.fn()
      .mockResolvedValueOnce({ requestId: "ok" })
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({ requestId: "ok-3" }) });
    render(<MainChat client={client} executionId="run-1" personas={personas} />);
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
    await screen.findByText("new reply");
    resolveOld({ requests: [] });
    expect(screen.queryByText("old")).not.toBeInTheDocument();
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
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Wait for this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    expect(screen.getByText(/Wait for this/)).toBeInTheDocument();
    resolve({ requestId: "accepted" });
  });

  it("unlocks and clears the composer when the execution scope changes during a pending send", () => {
    let resolve!: (value: { requestId: string }) => void;
    const client = runtime({ sendNativeChat: vi.fn(() => new Promise<{ requestId: string }>(done => { resolve = done; })) });
    const view = render(<MainChat client={client} executionId="old-run" personas={personas.slice(0, 2)} />);
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Old run instruction" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to main chat" }));
    view.rerender(<MainChat client={client} executionId="new-run" personas={personas.slice(0, 2)} />);
    expect(screen.getByLabelText("Instruction")).toHaveValue("");
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "New run instruction" } });
    expect(screen.getByRole("button", { name: "Send to main chat" })).toBeEnabled();
    resolve({ requestId: "old-accepted" });
  });
});
