import { useEffect, useMemo, useRef, useState } from "react";
import { newIdempotencyKey, RuntimeError, type RuntimeClient } from "../runtime/client";
import type { NativeChatRequest, NativeChatSummary } from "../runtime/types";

const MAX_MESSAGE_LENGTH = 2000;

type NativeChatsClient = Pick<RuntimeClient, "listNativeChats" | "listNativeChatRequests" | "sendNativeChat">;

export interface NativeChatsProps {
  client: NativeChatsClient | null;
  executionId: string;
  nodeId: string;
  refreshSequence?: number;
}

function errorText(error: unknown): string {
  if (error instanceof RuntimeError) return error.message;
  return "The Runtime could not be reached. Try again, or inspect the request by its id.";
}

function statusLabel(request: NativeChatRequest): string {
  switch (request.state) {
    case "requested": return "Requested · waiting for Codex";
    case "received": return "Received from Codex · waiting for chat reply";
    case "completed": return "Chat reply received · completed";
    case "blocked": return "Blocked · action needed";
    case "unobserved": return "Unobserved · outcome not proven";
  }
}

export function NativeChats({ client, executionId, nodeId, refreshSequence = 0 }: NativeChatsProps) {
  const [chats, setChats] = useState<NativeChatSummary[]>([]);
  const [requests, setRequests] = useState<NativeChatRequest[]>([]);
  const [selectedChatId, setSelectedChatId] = useState("");
  const [message, setMessage] = useState("");
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [requestId, setRequestId] = useState("");
  const [localRequest, setLocalRequest] = useState<NativeChatRequest | null>(null);
  const [catalogCursor, setCatalogCursor] = useState<string | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [open, setOpen] = useState(false);
  const [refreshNonce, setRefreshNonce] = useState(0);
  const generation = useRef(0);
  const scopeRef = useRef("");
  const readingRef = useRef(false);
  const scopeKey = `${executionId}\0${nodeId}`;

  const selectedChat = chats.find((chat) => chat.id === selectedChatId) ?? null;
  const visibleChats = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const filtered = needle === "" ? chats : chats.filter((chat) =>
      `${chat.title}\n${chat.projectDirectory}\n${chat.id}`.toLowerCase().includes(needle));
    return [...filtered].sort((a, b) => `${a.projectDirectory}/${a.title}`.localeCompare(`${b.projectDirectory}/${b.title}`));
  }, [chats, query]);
  const currentRequest = useMemo(() => {
    const matching = requests.filter((request) => (requestId === "" || request.requestId === requestId) && request.nodeId === nodeId);
    return matching.at(-1) ?? localRequest;
  }, [localRequest, nodeId, requestId, requests]);

  useEffect(() => {
    generation.current += 1;
    const run = generation.current;
    if (!open || client === null || executionId === "") return;
    if (scopeRef.current !== scopeKey) {
      scopeRef.current = scopeKey;
      setChats([]);
      setRequests([]);
      setSelectedChatId("");
      setRequestId("");
      setLocalRequest(null);
      setCatalogCursor(null);
      setError("");
      setNotice("");
    }
    let cancelled = false;
    const read = async () => {
      setLoading(true);
      try {
        const [chatPage, requestPage] = await Promise.all([
          client.listNativeChats(),
          client.listNativeChatRequests(executionId),
        ]);
        if (cancelled || run !== generation.current) return;
        setChats(chatPage.chats);
        setCatalogCursor(chatPage.nextCursor);
        setRequests(requestPage.requests.filter((request) => request.nodeId === nodeId));
      } catch (reason) {
        if (!cancelled && run === generation.current) setError(errorText(reason));
      } finally {
        if (!cancelled && run === generation.current) setLoading(false);
      }
    };
    void read();
    return () => { cancelled = true; };
  }, [client, executionId, nodeId, open, scopeKey]);

  useEffect(() => {
    if (!open || client === null || executionId === "") return;
    const pending = currentRequest?.state === "requested" || currentRequest?.state === "received";
    if (!pending && refreshNonce === 0) return;
    let cancelled = false;
    const run = generation.current;
    const read = async () => {
      if (readingRef.current) return;
      readingRef.current = true;
      try {
        const page = await client.listNativeChatRequests(executionId);
        if (!cancelled && run === generation.current) setRequests(page.requests.filter((request) => request.nodeId === nodeId));
      } catch (reason) {
        if (!cancelled && run === generation.current) setError(errorText(reason));
      } finally {
        readingRef.current = false;
      }
    };
    void read();
    const timer = pending ? window.setInterval(() => void read(), 2500) : undefined;
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [client, currentRequest?.state, executionId, nodeId, open, refreshNonce, refreshSequence]);

  const send = async () => {
    if (client === null || selectedChat === null || message.trim() === "" || sending) return;
    const id = requestId || newIdempotencyKey();
    setRequestId(id);
    setSending(true);
    setError("");
    setNotice("");
    const run = generation.current;
    const pending: NativeChatRequest = {
      requestId: id,
      nodeId,
      threadId: selectedChat.id,
      title: selectedChat.title,
      sourceDirectory: selectedChat.projectDirectory,
      state: "requested",
    };
    setLocalRequest(pending);
    try {
      await client.sendNativeChat(executionId, {
        requestId: id,
        nodeId,
        threadId: selectedChat.id,
        message: message.trim(),
        sourceDirectory: selectedChat.projectDirectory,
        title: selectedChat.title,
      });
      if (run === generation.current) setNotice(`Request ${id} was accepted for observation. A receipt is shown only after a matching read.`);
    } catch (reason) {
      if (run === generation.current) {
        setLocalRequest({ ...pending, state: "unobserved", detail: "Send outcome is unconfirmed. Reconcile this request id before trying again." });
        setNotice(`Request ${id} was not proven. Keep this id and refresh to reconcile; it will not be sent again automatically.`);
        setError(errorText(reason));
      }
    } finally {
      setSending(false);
    }
  };

  const loadMoreChats = async () => {
    if (client === null || catalogCursor === null || catalogLoading) return;
    setCatalogLoading(true);
    try {
      const run = generation.current;
      const page = await client.listNativeChats({ cursor: catalogCursor });
      if (run !== generation.current) return;
      setChats((previous) => {
        const byId = new Map(previous.map((chat) => [chat.id, chat]));
        for (const chat of page.chats) byId.set(chat.id, chat);
        return [...byId.values()];
      });
      setCatalogCursor(page.nextCursor);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setCatalogLoading(false);
    }
  };

  const startNewRequest = () => {
    setRequestId("");
    setLocalRequest(null);
    setMessage("");
    setNotice("");
    setError("");
  };

  if (client === null) {
    return <section className="native-chats" aria-label="Connect existing chat"><h3>Connect existing chat</h3><p>Connect to the Runtime to load native chats.</p></section>;
  }

  if (!open) {
    return <section className="native-chats" aria-label="Connect existing chat"><button type="button" className="native-chat-disclosure" onClick={() => setOpen(true)}>Connect existing chat</button></section>;
  }

  return (
    <section className="native-chats" aria-label="Connect existing chat">
      <div className="native-chats-heading"><h3>Connect existing chat</h3><button type="button" className="native-chat-collapse" onClick={() => setOpen(false)}>Close</button></div>
      <p className="native-chats-help">Bring an existing Codex chat into this step. This does not assign the node or mark it complete.</p>
      <label className="native-chats-search">Search chats
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Project, title, or path" />
      </label>
      {loading && <p role="status">Loading existing chats…</p>}
      {!loading && visibleChats.length === 0 && <p className="native-chats-empty">No existing native chats match this search.</p>}
      {visibleChats.length > 0 && (
        <div className="native-chat-list" role="list" aria-label="Existing native chats">
          {visibleChats.map((chat) => (
            <button type="button" className={`native-chat-option${chat.id === selectedChatId ? " selected" : ""}`} key={chat.id} onClick={() => { setSelectedChatId(chat.id); setNotice(""); }}>
              <strong>{chat.title}</strong><span>{chat.projectDirectory}</span><small>{chat.id}</small>
            </button>
          ))}
        </div>
      )}
      {catalogCursor && <p className="native-chats-help">This is a bounded page of chats. More results are available.</p>}
      {catalogCursor && <button type="button" className="native-chat-disclosure" disabled={catalogLoading} onClick={() => void loadMoreChats()}>{catalogLoading ? "Loading more chats…" : "Load more chats"}</button>}
      {selectedChat && <div className="native-chat-compose">
        <p className="native-chat-history">Using <strong>{selectedChat.title}</strong> from <code>{selectedChat.projectDirectory}</code>. Its existing history stays in that chat.</p>
        <label>Work message
          <textarea value={message} maxLength={MAX_MESSAGE_LENGTH} disabled={requestId !== ""} onChange={(event) => setMessage(event.target.value)} placeholder="Describe the work for this step" rows={4} />
        </label>
        <div className="native-chat-actions"><span>{message.length}/{MAX_MESSAGE_LENGTH}</span>{requestId !== "" ? <button type="button" onClick={startNewRequest}>Start new request</button> : <button type="button" className="act" disabled={sending || message.trim() === ""} onClick={() => void send()}>{sending ? "Sending…" : "Send work"}</button>}</div>
      </div>}
      {currentRequest && <div className={`native-chat-request native-chat-request-${currentRequest.state}`} aria-live="polite">
        <strong>{statusLabel(currentRequest)}</strong><span>Request id: <code>{currentRequest.requestId}</code></span>
        {currentRequest.text && <p>{currentRequest.text}</p>}
        {currentRequest.detail && <p>{currentRequest.detail}</p>}
      </div>}
      {requestId && <button type="button" className="native-chat-refresh" onClick={() => setRefreshNonce((value) => value + 1)}>Refresh request status</button>}
      {notice && <p className="native-chat-notice" role="status">{notice}</p>}
      {error && <p className="native-chat-error" role="alert">{error}</p>}
    </section>
  );
}
