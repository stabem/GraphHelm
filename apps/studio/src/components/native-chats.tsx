import { useEffect, useMemo, useRef, useState } from "react";
import { newIdempotencyKey, RuntimeError, type RuntimeClient } from "../runtime/client";
import type { NativeChatRequest, NativeChatSummary } from "../runtime/types";

const MAX_MESSAGE_LENGTH = 2000;
const MAX_PERSONA_LENGTH = 800;

type NativeChatsClient = Pick<RuntimeClient, "listNativeChats" | "listNativeChatRequests" | "sendNativeChat">;

export interface NativeChatsProps {
  client: NativeChatsClient | null;
  executionId: string;
  nodeId: string;
  refreshSequence?: number;
  boundChat?: NativeChatSummary;
  personaCharter?: string;
  onLinkPersona?: (chat: NativeChatSummary, charter: string) => Promise<void>;
  /** #585: opened from Main chat's "Connect existing chat", the picker starts open. */
  startOpen?: boolean;
}

function sortChats(chats: NativeChatSummary[]): NativeChatSummary[] {
  return [...chats].sort((a, b) => b.updatedAt - a.updatedAt || a.id.localeCompare(b.id));
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

export function NativeChats({ client, executionId, nodeId, refreshSequence = 0, boundChat, personaCharter = "", onLinkPersona, startOpen = false }: NativeChatsProps) {
  const [chats, setChats] = useState<NativeChatSummary[]>([]);
  const [requests, setRequests] = useState<NativeChatRequest[]>([]);
  const [selectedChatId, setSelectedChatId] = useState(boundChat?.id ?? "");
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
  const [open, setOpen] = useState(Boolean(boundChat) || startOpen);
  const [role, setRole] = useState("");
  const [personality, setPersonality] = useState("");
  const [linkingPersona, setLinkingPersona] = useState(false);
  const [personaLinked, setPersonaLinked] = useState(false);
  const [refreshNonce, setRefreshNonce] = useState(0);
  const generation = useRef(0);
  const selectedChatIdRef = useRef(boundChat?.id ?? "");
  const scopeRef = useRef("");
  const readingRef = useRef(false);
  const scopeKey = `${executionId}\0${nodeId}`;

  const selectedChat = boundChat ?? chats.find((chat) => chat.id === selectedChatId) ?? null;
  const visibleChats = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const filtered = needle === "" ? chats : chats.filter((chat) =>
      `${chat.title}\n${chat.projectDirectory}\n${chat.id}`.toLowerCase().includes(needle));
    return sortChats(filtered);
  }, [chats, query]);
  const currentRequest = useMemo(() => {
    const matching = requests.filter((request) => (requestId === "" || request.requestId === requestId) && request.nodeId === nodeId && (!boundChat || request.threadId === boundChat.id));
    return matching.at(-1) ?? localRequest;
  }, [boundChat, localRequest, nodeId, requestId, requests]);

  useEffect(() => {
    generation.current += 1;
    const run = generation.current;
    if (!open || client === null || executionId === "") return;
    if (scopeRef.current !== scopeKey) {
      scopeRef.current = scopeKey;
      setChats([]);
      setRequests([]);
      setSelectedChatId(boundChat?.id ?? "");
      selectedChatIdRef.current = boundChat?.id ?? "";
      setPersonaLinked(false);
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
          boundChat ? Promise.resolve(null) : client.listNativeChats(),
          client.listNativeChatRequests(executionId),
        ]);
        if (cancelled || run !== generation.current) return;
        if (chatPage !== null) {
          setChats(sortChats(chatPage.chats));
          setCatalogCursor(chatPage.nextCursor);
        }
        setRequests(requestPage.requests.filter((request) => request.nodeId === nodeId));
      } catch (reason) {
        if (!cancelled && run === generation.current) setError(errorText(reason));
      } finally {
        if (!cancelled && run === generation.current) setLoading(false);
      }
    };
    void read();
    return () => { cancelled = true; };
  }, [boundChat, client, executionId, nodeId, open, scopeKey]);

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
        message: `${personaInstructions}${message.trim()}`,
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
        return sortChats([...byId.values()]);
      });
      setCatalogCursor(page.nextCursor);
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setCatalogLoading(false);
    }
  };

  const activityCharter = personaCharter.trim();
  const personaInstructions = activityCharter === "" ? "" : `Persona instructions for this activity:\n${activityCharter}\n\n`;
  const messageLimit = Math.max(0, MAX_MESSAGE_LENGTH - personaInstructions.length);
  const composedCharter = [role.trim(), personality.trim()].filter(Boolean).join("\n\n");
  const personalityLimit = Math.max(0, MAX_PERSONA_LENGTH - role.length - (role.trim() === "" ? 0 : 2));
  const personaPending = currentRequest?.state === "requested" || currentRequest?.state === "received";

  const linkPersona = async () => {
    if (!selectedChat || !onLinkPersona || composedCharter === "" || composedCharter.length > MAX_PERSONA_LENGTH || linkingPersona || personaPending || sending || personaLinked || activityCharter !== "") return;
    const linkedChatId = selectedChat.id;
    const linkedGeneration = generation.current;
    setLinkingPersona(true);
    setError("");
    try {
      await onLinkPersona(selectedChat, composedCharter);
      if (linkedGeneration !== generation.current || selectedChatIdRef.current !== linkedChatId) return;
      setPersonaLinked(true);
      setNotice("Persona membership recorded for this activity. No native chat turn was sent.");
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setLinkingPersona(false);
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

  if (!open && !boundChat) {
    return <section className="native-chats" aria-label="Connect existing chat"><button type="button" className="native-chat-disclosure" onClick={() => setOpen(true)}>Connect existing chat</button></section>;
  }

  return (
    <section className="native-chats" aria-label="Connect existing chat">
      <div className="native-chats-heading"><h3>{boundChat ? `Persona conversation · ${boundChat.title}` : "Connect existing chat"}</h3>{!boundChat && <button type="button" className="native-chat-collapse" onClick={() => setOpen(false)}>Close</button>}</div>
      <p className="native-chats-help">Bring an existing Codex chat into this step. This does not assign the node or mark it complete.</p>
      {!boundChat && <label className="native-chats-search">Search chats
        <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Project, title, or path" />
      </label>}
      {loading && <p role="status">Loading existing chats…</p>}
      {!boundChat && !loading && visibleChats.length === 0 && <p className="native-chats-empty">No existing native chats match this search.</p>}
      {!boundChat && visibleChats.length > 0 && (
        <div className="native-chat-list" role="list" aria-label="Existing native chats">
          {visibleChats.map((chat) => (
            <button type="button" className={`native-chat-option${chat.id === selectedChatId ? " selected" : ""}`} key={chat.id} disabled={linkingPersona} onClick={() => { selectedChatIdRef.current = chat.id; setSelectedChatId(chat.id); setPersonaLinked(false); setNotice(""); }}>
              <strong>{chat.title}</strong><span>{chat.projectDirectory}</span><small>{chat.id}</small>
            </button>
          ))}
        </div>
      )}
      {!boundChat && catalogCursor && <p className="native-chats-help">This is a bounded page of chats. More results are available.</p>}
      {!boundChat && catalogCursor && <button type="button" className="native-chat-disclosure" disabled={catalogLoading} onClick={() => void loadMoreChats()}>{catalogLoading ? "Loading more chats…" : "Load more chats"}</button>}
      {selectedChat && <div className="native-chat-compose">
        <p className="native-chat-history">Using <strong>{selectedChat.title}</strong> from <code>{selectedChat.projectDirectory}</code>{boundChat && personaCharter && <> · Role: <strong>{personaCharter}</strong></>}. Its existing history stays in that chat.</p>
        {!boundChat && onLinkPersona && <div className="native-chat-persona">
          <label>Role
            <input value={role} maxLength={MAX_PERSONA_LENGTH} onChange={(event) => setRole(event.target.value)} placeholder="What this chat does in the activity" />
          </label>
          <label>Personality
            <textarea value={personality} maxLength={personalityLimit} onChange={(event) => setPersonality(event.target.value)} placeholder="How it should work" rows={3} />
            <small>{composedCharter.length}/{MAX_PERSONA_LENGTH} charter characters</small>
          </label>
          <button type="button" disabled={composedCharter === "" || composedCharter.length > MAX_PERSONA_LENGTH || linkingPersona || personaPending || sending || personaLinked || activityCharter !== ""} onClick={() => void linkPersona()}>{linkingPersona ? "Adding persona…" : "Add persona to activity"}</button>
        </div>}
        <label>Work message
          <textarea value={message} maxLength={messageLimit} disabled={requestId !== ""} onChange={(event) => setMessage(event.target.value)} placeholder="Describe the work for this step" rows={4} />
        </label>
        <div className="native-chat-actions"><span>{message.length}/{messageLimit} message characters available</span>{requestId !== "" ? <button type="button" onClick={startNewRequest}>Start new request</button> : <button type="button" className="act" disabled={sending || message.trim() === "" || message.length > messageLimit} onClick={() => void send()}>{sending ? "Sending…" : "Send work"}</button>}</div>
      </div>}
      {currentRequest && <div className={`native-chat-request native-chat-request-${currentRequest.state}`} aria-live="polite">
        <strong>{statusLabel(currentRequest)}</strong><span>Request id: <code>{currentRequest.requestId}</code></span>
        <span>Chat: {currentRequest.title} · <code>{currentRequest.sourceDirectory}</code></span>
        {currentRequest.text && <p>{currentRequest.text}</p>}
        {currentRequest.detail && <p>{currentRequest.detail}</p>}
      </div>}
      {requestId && <button type="button" className="native-chat-refresh" onClick={() => setRefreshNonce((value) => value + 1)}>Refresh request status</button>}
      {notice && <p className="native-chat-notice" role="status">{notice}</p>}
      {error && <p className="native-chat-error" role="alert">{error}</p>}
    </section>
  );
}
