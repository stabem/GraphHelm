import { useEffect, useMemo, useRef, useState } from "react";

import { newIdempotencyKey, RuntimeError, type RuntimeClient } from "../runtime/client";
import type { NativeChatRequest, NativeChatSummary } from "../runtime/types";

const MAX_MESSAGE_LENGTH = 2000;
const POLL_MS = 2500;
const TIMEOUT_MS = 15000;

type MainChatClient = Pick<RuntimeClient, "sendNativeChat" | "listNativeChatRequests">;

export interface MainChatPersona {
  chat: NativeChatSummary;
  charter: string;
  nodeId: string;
}

export interface MainChatProps {
  client: MainChatClient | null;
  executionId: string;
  personas: MainChatPersona[];
  refreshSequence?: number;
  onConnect?: () => void;
}

type Row = NativeChatRequest & { ownerMessage?: string; local?: boolean };
type Notice = { tone: "good" | "bad"; text: string } | null;

function errorText(error: unknown): string {
  if (error instanceof RuntimeError) return error.message;
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  return "The Runtime could not be reached. The send outcome is unobserved; reconcile by request id.";
}

function stateLabel(state: NativeChatRequest["state"]): string {
  switch (state) {
    case "requested": return "Requested · waiting for Codex";
    case "received": return "Received · waiting for chat reply";
    case "completed": return "Completed · chat reply received";
    case "blocked": return "Blocked · action needed";
    case "unobserved": return "Unobserved · outcome not proven";
  }
}

function keyOf(request: Pick<NativeChatRequest, "requestId" | "nodeId" | "threadId">): string {
  return `${request.requestId}\0${request.nodeId}\0${request.threadId}`;
}

function isPending(state: NativeChatRequest["state"]): boolean {
  return state === "requested" || state === "received" || state === "unobserved";
}

function withCharter(charter: string, message: string): string {
  const prefix = charter.trim() === "" ? "" : `Persona instructions for this activity:\n${charter.trim()}\n\n`;
  return `${prefix}${message.trim()}`;
}

export function MainChat({ client, executionId, personas, refreshSequence = 0, onConnect }: MainChatProps) {
  const principal = personas[0] ?? null;
  const [selectedId, setSelectedId] = useState(principal?.chat.id ?? "");
  const [message, setMessage] = useState("");
  const [rows, setRows] = useState<Row[]>([]);
  const [sending, setSending] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const [readError, setReadError] = useState("");
  const [refreshNonce, setRefreshNonce] = useState(0);
  const generation = useRef(0);
  const scopeRef = useRef("");
  const reading = useRef(false);
  const conversationRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);
  const scopeKey = `${executionId}\0${personas.map((persona) => `${persona.nodeId}\0${persona.chat.id}`).join("\x01")}`;

  const byChat = useMemo(() => new Map(personas.map((persona) => [persona.chat.id, persona])), [personas]);
  const selected = byChat.get(selectedId) ?? principal;
  const prefixLength = (charter: string) => charter.trim() === "" ? 0 : `Persona instructions for this activity:\n${charter.trim()}\n\n`.length;
  const messageLimitFor = (persona: MainChatPersona | null) => Math.max(0, MAX_MESSAGE_LENGTH - (persona ? prefixLength(persona.charter) : 0));
  const messageLimit = messageLimitFor(selected);
  const teamTargets = personas.filter((persona) => persona.chat.id !== selected?.chat.id);
  const activeRows = rows.filter((row) => byChat.has(row.threadId) && personas.some((persona) => persona.nodeId === row.nodeId && persona.chat.id === row.threadId));
  const blockedByPending = activeRows.some((row) => isPending(row.state));
  const hasReceiptPending = activeRows.some((row) => row.state === "requested" || row.state === "received");
  const canSend = client !== null && selected !== null && message.trim() !== "" && message.length <= messageLimit && !sending && !blockedByPending;
  const nativeApiAvailable = client !== null && typeof client.sendNativeChat === "function" && typeof client.listNativeChatRequests === "function";

  useEffect(() => {
    if (scopeRef.current === scopeKey) return;
    generation.current += 1;
    const run = generation.current;
    scopeRef.current = scopeKey;
    setRows([]);
    setSelectedId(principal?.chat.id ?? "");
    setSending(false);
    setMessage("");
    setNotice(null);
    setReadError("");
    if (!nativeApiAvailable || executionId === "" || personas.length === 0) return;
    let cancelled = false;
    const read = async () => {
      try {
        const page = await client.listNativeChatRequests(executionId);
        if (cancelled || run !== generation.current) return;
        const allowed = new Set(personas.map((persona) => `${persona.nodeId}\0${persona.chat.id}`));
        setRows((current) => {
          const merged = new Map(current.map((row) => [keyOf(row), row]));
          for (const request of page.requests) if (allowed.has(`${request.nodeId}\0${request.threadId}`)) {
            const old = merged.get(keyOf(request));
            merged.set(keyOf(request), { ...old, ...request, ownerMessage: old?.ownerMessage, local: old?.local });
          }
          return [...merged.values()];
        });
        setReadError("");
      } catch (error) {
        if (!cancelled && run === generation.current) setReadError(errorText(error));
      }
    };
    void read();
    return () => { cancelled = true; };
  }, [client, executionId, nativeApiAvailable, personas, principal, scopeKey]);

  useEffect(() => {
    if (!nativeApiAvailable || executionId === "" || personas.length === 0) return;
    const run = generation.current;
    let cancelled = false;
    const read = async () => {
      if (reading.current) return;
      reading.current = true;
      try {
        const page = await client.listNativeChatRequests(executionId);
        if (cancelled || run !== generation.current) return;
        const allowed = new Set(personas.map((persona) => `${persona.nodeId}\0${persona.chat.id}`));
        setRows((current) => {
          const merged = new Map(current.map((row) => [keyOf(row), row]));
          for (const request of page.requests) if (allowed.has(`${request.nodeId}\0${request.threadId}`)) {
            const old = merged.get(keyOf(request));
            merged.set(keyOf(request), { ...old, ...request, ownerMessage: old?.ownerMessage, local: old?.local });
          }
          return [...merged.values()];
        });
      } catch (error) {
        if (!cancelled && run === generation.current) setReadError(errorText(error));
      } finally {
        reading.current = false;
      }
    };
    if (hasReceiptPending || refreshNonce > 0 || refreshSequence > 0) void read();
    const timer = hasReceiptPending ? window.setInterval(() => void read(), POLL_MS) : undefined;
    return () => { cancelled = true; if (timer !== undefined) window.clearInterval(timer); };
  }, [client, executionId, hasReceiptPending, nativeApiAvailable, refreshNonce, refreshSequence, scopeKey]);

  const conversationSignature = activeRows.map((row) => `${keyOf(row)}\0${row.state}\0${row.text ?? ""}`).join("\x01");
  useEffect(() => {
    const conversation = conversationRef.current;
    if (conversation !== null && atBottomRef.current) conversation.scrollTop = conversation.scrollHeight;
  }, [conversationSignature]);

  const send = async (fanout: boolean) => {
    if (!canSend || selected === null || client === null || !nativeApiAvailable) return;
    const recipients = fanout ? teamTargets : [selected];
    if (recipients.length === 0) {
      setNotice({ tone: "bad", text: "There are no other linked personas to receive this team message." });
      return;
    }
    if (new Set(recipients.map((persona) => persona.chat.id)).size !== recipients.length) {
      setNotice({ tone: "bad", text: "The linked personas contain duplicate chat threads. Nothing was sent." });
      return;
    }
    const trimmed = message.trim();
    const requests = recipients.map((persona) => {
      const requestId = newIdempotencyKey();
      return {
        requestId,
        nodeId: persona.nodeId,
        threadId: persona.chat.id,
        message: withCharter(persona.charter, trimmed),
        sourceDirectory: persona.chat.projectDirectory,
        title: persona.chat.title,
        ownerMessage: trimmed,
      };
    });
    if (requests.some((request) => request.message.length > MAX_MESSAGE_LENGTH)) {
      setNotice({ tone: "bad", text: "The instruction plus one or more persona charters exceeds the Runtime's 2000 character limit. Shorten the instruction." });
      return;
    }
    if (new Set(requests.map((request) => request.requestId)).size !== requests.length) {
      setNotice({ tone: "bad", text: "The Runtime request identities were not unique. Nothing was sent." });
      return;
    }
    setSending(true);
    setNotice(null);
    const run = generation.current;
    const pendingRows: Row[] = requests.map((request) => ({ ...request, state: "requested", local: true }));
    setRows((current) => [...current, ...pendingRows]);
    setMessage("");
    const outcomes = await Promise.allSettled(requests.map(async (request) => {
      let timeout!: number;
      const timeoutPromise = new Promise<never>((_, reject) => {
        timeout = window.setTimeout(() => reject(new Error("Timed out before the Runtime confirmed this request.")), TIMEOUT_MS);
      });
      try {
        return await Promise.race([client.sendNativeChat(executionId, request), timeoutPromise]);
      } finally {
        window.clearTimeout(timeout);
      }
    }));
    if (run !== generation.current) return;
    setRows((current) => current.map((row) => {
      const index = requests.findIndex((request) => request.requestId === row.requestId);
      const result = index < 0 ? null : outcomes[index];
      if (!result || row.local !== true) return row;
      if (row.state === "completed" || row.state === "blocked") return row;
      if (result.status === "fulfilled") return row;
      const definitive = result.reason instanceof RuntimeError && result.reason.httpStatus >= 400 && result.reason.httpStatus < 500;
      return { ...row, state: definitive ? "blocked" : "unobserved", detail: definitive ? errorText(result.reason) : "Send outcome is unobserved. Reconcile this request id before trying again." };
    }));
    const failed = outcomes.filter((outcome) => outcome.status === "rejected").length;
    setNotice(failed === 0 ? { tone: "good", text: "The request was accepted. Waiting for authoritative chat receipts." } : { tone: "bad", text: `${failed} request${failed === 1 ? "" : "s"} could not be proven; no request was retried.` });
    setSending(false);
  };

  if (!nativeApiAvailable) return <section className="main-chat" aria-label="Main chat"><h2>Main chat</h2><p>This Runtime does not expose native chat requests yet. Connect an existing chat to continue.</p>{onConnect && <button type="button" onClick={onConnect}>Connect existing chat</button>}</section>;
  if (personas.length === 0) return <section className="main-chat" aria-label="Main chat"><h2>Main chat</h2><p>Connect existing chat to choose a coordinator.</p>{onConnect && <button type="button" onClick={onConnect}>Connect existing chat</button>}</section>;

  return <section className="main-chat" aria-label="Main chat">
    <header><h2>Main chat</h2><p aria-label="Next step">Next step: choose the coordinator, write one instruction, then send it to main chat or the listed team targets.</p></header>
    <label htmlFor="main-chat-recipient">Main recipient</label>
    <select id="main-chat-recipient" value={selected?.chat.id ?? ""} disabled={sending || blockedByPending} onChange={(event) => setSelectedId(event.target.value)}>
      {personas.map((persona) => <option key={persona.chat.id} value={persona.chat.id}>{persona.chat.title} · {persona.chat.id}</option>)}
    </select>
    <div aria-label="Main chat conversation" className="main-chat-conversation" ref={conversationRef} onScroll={(event) => {
      const element = event.currentTarget;
      atBottomRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 48;
    }}>
      {activeRows.map((row) => <article key={keyOf(row)} className={`main-chat-request main-chat-request-${row.state}`}>
        {row.ownerMessage && <p><strong>You</strong>: {row.ownerMessage}</p>}
        <strong>{stateLabel(row.state)}</strong><span> {row.title} · <code>{row.requestId}</code></span>
        {row.text && <p>{row.text}</p>}{row.detail && <p role="status">{row.detail}</p>}
      </article>)}
      {activeRows.length === 0 && <p className="main-chat-empty">No orders sent yet. Your next message will appear here.</p>}
    </div>
    <label htmlFor="main-chat-message">Instruction</label>
    <textarea id="main-chat-message" value={message} disabled={sending || blockedByPending} maxLength={messageLimit} rows={4} onChange={(event) => setMessage(event.target.value)} placeholder="Tell the coordinator what to do" />
    <p>{message.length}/{messageLimit} characters available after the selected charter. Team sends validate every target before dispatch.</p>
    <p className="main-chat-team-targets">Team targets: {teamTargets.length === 0 ? "none" : teamTargets.map((persona) => persona.chat.title).join(", ")}</p>
    <div className="main-chat-actions"><button type="button" disabled={!canSend} onClick={() => void send(false)}>Send to main chat</button><button type="button" disabled={!canSend || teamTargets.length === 0} onClick={() => void send(true)}>Send to team ({teamTargets.length} other{teamTargets.length === 1 ? "" : "s"})</button></div>
    {activeRows.length > 0 && <button type="button" className="main-chat-refresh" onClick={() => setRefreshNonce((value) => value + 1)}>Refresh request status</button>}
    {blockedByPending && <p role="status">Reconcile the active request before starting a new batch.</p>}
    {readError && <p role="alert">{readError}</p>}{notice && <p role={notice.tone === "bad" ? "alert" : "status"}>{notice.text}</p>}
  </section>;
}
