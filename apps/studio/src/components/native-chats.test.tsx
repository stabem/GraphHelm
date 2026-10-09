import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NativeChats, type NativeChatsProps } from "./native-chats";

function page() {
  return { chats: [{ id: "chat-1", title: "Build chat", projectDirectory: "C:/work/project", updatedAt: 1_000 }], nextCursor: null };
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
  // #585: Main chat's "Connect existing chat" opens the step's window with the picker already
  // open; a second button with the same name to press was all the owner (and a journey) got.
  it("starts open when Main chat asked to connect a chat, and closed otherwise", async () => {
    const { unmount } = render(<NativeChats client={client()} executionId="execution-1" nodeId="node-a" startOpen />);
    expect(await screen.findByRole("list", { name: "Existing native chats" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Search chats" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Connect existing chat" })).toBeNull();
    unmount();
    render(<NativeChats client={client()} executionId="execution-1" nodeId="node-a" />);
    expect(screen.getByRole("button", { name: "Connect existing chat" })).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Existing native chats" })).toBeNull();
  });

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

  it("orders the catalog by newest wire timestamp and keeps a selected chat after refresh", async () => {
    const runtime = client({
      listNativeChats: vi.fn()
        .mockResolvedValueOnce({ chats: [
          { id: "older", title: "Older", projectDirectory: "C:/old", updatedAt: 10 },
          { id: "same-b", title: "Same B", projectDirectory: "C:/b", updatedAt: 20 },
          { id: "same-a", title: "Same A", projectDirectory: "C:/a", updatedAt: 20 },
        ], nextCursor: null })
        .mockResolvedValueOnce({ chats: [
          { id: "older", title: "Older refreshed", projectDirectory: "C:/old", updatedAt: 10 },
          { id: "same-b", title: "Same B", projectDirectory: "C:/b", updatedAt: 20 },
          { id: "same-a", title: "Same A", projectDirectory: "C:/a", updatedAt: 20 },
        ], nextCursor: null }),
    });
    render(<NativeChats client={runtime} executionId="execution-1" nodeId="node-a" />);
    fireEvent.click(screen.getByRole("button", { name: "Connect existing chat" }));
    const sameB = await screen.findByRole("button", { name: /Same B/ });
    expect(screen.getAllByRole("button", { name: /Same/ }).map((button) => button.textContent)).toEqual(["Same AC:/asame-a", "Same BC:/bsame-b"]);
    fireEvent.click(sameB);
    fireEvent.change(screen.getByLabelText("Work message"), { target: { value: "Keep selection" } });
    fireEvent.click(screen.getByRole("button", { name: "Send work" }));
    await screen.findByText(/Request .* accepted/);
    fireEvent.click(screen.getByRole("button", { name: "Refresh request status" }));
    await waitFor(() => expect(screen.getByText(/Using/)).toHaveTextContent("Same B"));
    expect(screen.getByRole("button", { name: /Same B/ })).toHaveClass("selected");
  });

  it("records persona membership without sending a native turn", async () => {
    const runtime = client();
    const onLinkPersona = vi.fn().mockResolvedValue(undefined);
    render(<NativeChats client={runtime} executionId="execution-1" nodeId="node-a" onLinkPersona={onLinkPersona} />);
    fireEvent.click(screen.getByRole("button", { name: "Connect existing chat" }));
    await screen.findByRole("button", { name: /Build chat/ });
    fireEvent.click(screen.getByRole("button", { name: /Build chat/ }));
    fireEvent.change(screen.getByLabelText("Role"), { target: { value: "Reviewer" } });
    fireEvent.change(screen.getByPlaceholderText("How it should work"), { target: { value: "Careful and concise." } });
    fireEvent.click(screen.getByRole("button", { name: "Add persona to activity" }));
    await waitFor(() => expect(onLinkPersona).toHaveBeenCalledWith(expect.objectContaining({ id: "chat-1" }), expect.stringContaining("Reviewer")));
    expect(runtime.sendNativeChat).not.toHaveBeenCalled();
    expect(screen.getByRole("status")).toHaveTextContent(/membership recorded/i);
    expect(screen.getByRole("button", { name: "Add persona to activity" })).toBeDisabled();
  });

  it("locks a bound chat and prepends only its charter within the wire limit", async () => {
    const runtime = client();
    render(<NativeChats client={runtime} executionId="execution-1" nodeId="node-a"
      boundChat={{ id: "chat-1", title: "Build chat", projectDirectory: "C:/work/project", updatedAt: 1_000 }}
      personaCharter="Act as a careful reviewer." />);
    expect(screen.queryByLabelText("Search chats")).toBeNull();
    expect(runtime.listNativeChats).not.toHaveBeenCalled();
    await screen.findByRole("heading", { name: /Persona conversation · Build chat/ });
    fireEvent.change(screen.getByLabelText("Work message"), { target: { value: "Inspect this step" } });
    fireEvent.click(screen.getByRole("button", { name: "Send work" }));
    await waitFor(() => expect(runtime.sendNativeChat).toHaveBeenCalledWith("execution-1", expect.objectContaining({
      threadId: "chat-1",
      message: "Persona instructions for this activity:\nAct as a careful reviewer.\n\nInspect this step",
    })));
  });

  it("shows only the bound chat receipt when two chats share one node", async () => {
    const runtime = client({
      listNativeChatRequests: vi.fn().mockResolvedValue({ requests: [
        { requestId: "bound-request", nodeId: "node-a", threadId: "chat-1", title: "Bound", sourceDirectory: "C:/bound", state: "received" as const, text: "bound receipt" },
        { requestId: "other-request", nodeId: "node-a", threadId: "chat-2", title: "Other", sourceDirectory: "C:/other", state: "completed" as const, text: "wrong receipt" },
      ] }),
    });
    render(<NativeChats client={runtime} executionId="execution-1" nodeId="node-a"
      boundChat={{ id: "chat-1", title: "Bound", projectDirectory: "C:/bound", updatedAt: 1_000 }} />);
    await screen.findByText("bound receipt");
    expect(screen.queryByText("wrong receipt")).toBeNull();
  });

  it("does not let a late persona link disable the next selected chat", async () => {
    let resolveLink!: () => void;
    const onLinkPersona = vi.fn(() => new Promise<void>((resolve) => { resolveLink = resolve; }));
    const runtime = client({
      listNativeChats: vi.fn().mockResolvedValue({ chats: [
        { id: "chat-1", title: "First chat", projectDirectory: "C:/first", updatedAt: 2_000 },
        { id: "chat-2", title: "Second chat", projectDirectory: "C:/second", updatedAt: 1_000 },
      ], nextCursor: null }),
    });
    render(<NativeChats client={runtime} executionId="execution-1" nodeId="node-a" onLinkPersona={onLinkPersona} />);
    fireEvent.click(screen.getByRole("button", { name: "Connect existing chat" }));
    await screen.findByRole("button", { name: /First chat/ });
    fireEvent.click(screen.getByRole("button", { name: /First chat/ }));
    fireEvent.change(screen.getByLabelText("Role"), { target: { value: "Reviewer" } });
    fireEvent.click(screen.getByRole("button", { name: "Add persona to activity" }));
    expect(screen.getByRole("button", { name: /Second chat/ })).toBeDisabled();
    resolveLink();
    await waitFor(() => expect(screen.getByRole("button", { name: "Add persona to activity" })).toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: /Second chat/ }));
    expect(screen.getByRole("button", { name: "Add persona to activity" })).toBeEnabled();
  });
});
