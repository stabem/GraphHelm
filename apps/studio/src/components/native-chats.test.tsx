import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NativeChats, type NativeChatsProps } from "./native-chats";

function page() {
  return { chats: [{ id: "chat-1", title: "Build chat", projectDirectory: "C:/work/project", updatedAt: "2026-10-02T10:00:00Z" }], nextCursor: null };
}

function client(overrides: Partial<NonNullable<NativeChatsProps["client"]>> = {}): NonNullable<NativeChatsProps["client"]> {
  return {
    listNativeChats: vi.fn().mockResolvedValue(page()),
    listNativeChatRequests: vi.fn().mockResolvedValue({ requests: [] }),
    sendNativeChat: vi.fn().mockResolvedValue({ requestId: "request-1" }),
    ...overrides,
  };
}

describe("NativeChats", () => {
  it("keeps a request visible as requested when POST succeeds but no read receipt exists", async () => {
    const runtime = client();
    render(<NativeChats client={runtime} executionId="execution-1" nodeId="node-a" />);
    fireEvent.click(screen.getByRole("button", { name: "Connect existing chat" }));
    await screen.findByRole("button", { name: /Build chat/ });
    fireEvent.click(screen.getByRole("button", { name: /Build chat/ }));
    fireEvent.change(screen.getByLabelText("Work message"), { target: { value: "Inspect the failing step" } });
    fireEvent.click(screen.getByRole("button", { name: "Send work" }));
    await screen.findByText(/Requested · waiting for Codex/);
    expect(screen.queryByText(/completed/i)).toBeNull();
    expect(runtime.sendNativeChat).toHaveBeenCalledWith("execution-1", expect.objectContaining({ nodeId: "node-a", message: "Inspect the failing step" }));
  });

  it("retains the request id after an uncertain send so the operator can reconcile without an automatic retry", async () => {
    const runtime = client({ sendNativeChat: vi.fn().mockRejectedValue(new Error("network")) });
    render(<NativeChats client={runtime} executionId="execution-1" nodeId="node-a" />);
    fireEvent.click(screen.getByRole("button", { name: "Connect existing chat" }));
    await screen.findByRole("button", { name: /Build chat/ });
    fireEvent.click(screen.getByRole("button", { name: /Build chat/ }));
    fireEvent.change(screen.getByLabelText("Work message"), { target: { value: "Keep this request" } });
    fireEvent.click(screen.getByRole("button", { name: "Send work" }));
    await screen.findByText(/was not proven/);
    expect(screen.getByText(/Request id:/)).toBeTruthy();
    expect(runtime.sendNativeChat).toHaveBeenCalledTimes(1);
  });

  it("does not render a late request read from a previous execution or node", async () => {
    let resolveOld: ((value: { requests: never[] }) => void) | undefined;
    const oldRead = new Promise<{ requests: never[] }>((resolve) => { resolveOld = resolve; });
    const runtime = client({
      listNativeChatRequests: vi.fn().mockImplementation((executionId: string) => executionId === "old" ? oldRead : Promise.resolve({ requests: [{ requestId: "new-request", nodeId: "node-b", threadId: "chat-1", title: "Build chat", sourceDirectory: "C:/work/project", state: "received" as const, text: "new record" }] })),
    });
    const view = render(<NativeChats client={runtime} executionId="old" nodeId="node-a" />);
    fireEvent.click(screen.getByRole("button", { name: "Connect existing chat" }));
    view.rerender(<NativeChats client={runtime} executionId="new" nodeId="node-b" />);
    await screen.findByText(/Received from Codex/);
    await act(async () => { resolveOld?.({ requests: [] }); });
    await waitFor(() => expect(screen.queryByText("old")).toBeNull());
    expect(screen.getByText("new record")).toBeTruthy();
  });
});
