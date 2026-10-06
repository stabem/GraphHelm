import { useEffect, useMemo, useRef, useState } from "react";

import { newIdempotencyKey, RuntimeError, type RuntimeClient } from "../runtime/client";
import type { NativeChatRequest, NativeChatSummary } from "../runtime/types";

const MAX_MESSAGE_LENGTH = 2000;
const MAX_RECOVERY_BYTES = 64 * 1024;
const MAX_RECOVERY_ENTRIES = 256;
const MAX_RECOVERY_FIELD_LENGTH = 256;
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
  /** Thread id the Chat column selected; MainChat switches its recipient to it when linked. */
  recipientId?: string | null;
  /** Fills the instruction when it is empty (Jev "Use"); never sends. */
  seed?: { text: string; nonce: number } | null;
  /** Bumped by a question card's "Check it" to re-read the request ledger. */
  refreshNonce?: number;
  /** Every ledger merge, so the page's needs-you list sees unconfirmed requests. */
  onRequestsChange?: (requests: NativeChatRequest[]) => void;
}

type Row = NativeChatRequest & { ownerMessage?: string; local?: boolean };
type Notice = { tone: "good" | "bad"; text: string } | null;
type RecoveryId = Pick<NativeChatRequest, "requestId" | "nodeId" | "threadId"> & { executionId: string };

function recoveryStorageKey(executionId: string): string {
  const origin = globalThis.location?.origin ?? "unknown-origin";
  return `graphhelm.main-chat.recovery:${origin}:${executionId}`;
}

function readRecovery(executionId: string): { ok: true; entries: RecoveryId[] } | { ok: false } {
  try {
    const raw = globalThis.sessionStorage.getItem(recoveryStorageKey(executionId));
    if (raw === null) return { ok: true, entries: [] };
    if (raw.length > MAX_RECOVERY_BYTES) return { ok: false };
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed) || parsed.length > MAX_RECOVERY_ENTRIES) return { ok: false };
    const entries: RecoveryId[] = [];
    for (const entry of parsed) {
      if (entry === null || typeof entry !== "object") return { ok: false };
      const keys = Object.keys(entry).sort();
      if (keys.join("\0") !== "executionId\0nodeId\0requestId\0threadId") return { ok: false };
      const candidate = entry as Record<string, unknown>;
      if (candidate.executionId !== executionId ||
        typeof candidate.executionId !== "string" || candidate.executionId === "" ||
        typeof candidate.requestId !== "string" || candidate.requestId === "" ||
        typeof candidate.nodeId !== "string" || candidate.nodeId === "" ||
        typeof candidate.threadId !== "string" || candidate.threadId === "" ||
        (candidate.executionId as string).length > MAX_RECOVERY_FIELD_LENGTH ||
        (candidate.requestId as string).length > MAX_RECOVERY_FIELD_LENGTH ||
        (candidate.nodeId as string).length > MAX_RECOVERY_FIELD_LENGTH ||
        (candidate.threadId as string).length > MAX_RECOVERY_FIELD_LENGTH) return { ok: false };
      entries.push(candidate as RecoveryId);
    }
    return { ok: true, entries };
  } catch {
    return { ok: false };
  }
}

function writeRecovery(executionId: string, entries: RecoveryId[]): boolean {
  try {
    globalThis.sessionStorage.setItem(recoveryStorageKey(executionId), JSON.stringify(entries));
    return true;
  } catch {
    return false;
  }
}

function addRecovery(executionId: string, requests: Array<Pick<RecoveryId, "requestId" | "nodeId" | "threadId">>): boolean {
  const existing = readRecovery(executionId);
  if (!existing.ok) return false;
  const byKey = new Map(existing.entries.map((entry) => [keyOf(entry), entry]));
  for (const request of requests) {
    byKey.set(keyOf(request), {
      executionId,
      requestId: request.requestId,
      nodeId: request.nodeId,
      threadId: request.threadId,
    });
  }
  return writeRecovery(executionId, [...byKey.values()]);
}

function removeRecovery(request: RecoveryId): void {
  const existing = readRecovery(request.executionId);
  if (!existing.ok) return;
  writeRecovery(request.executionId, existing.entries.filter((entry) => keyOf(entry) !== keyOf(request)));
}

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

function recoveredRow(recoveryId: RecoveryId, personas: MainChatPersona[]): Row {
  const persona = personas.find((entry) => entry.nodeId === recoveryId.nodeId && entry.chat.id === recoveryId.threadId);
  return {
    requestId: recoveryId.requestId,
    nodeId: recoveryId.nodeId,
    threadId: recoveryId.threadId,
    title: persona?.chat.title ?? "Retained native chat request",
    sourceDirectory: persona?.chat.projectDirectory ?? "",
    state: "unobserved",
    detail: "Recovered request. Refresh until an authoritative receipt appears; it will not be resent automatically.",
    local: true,
  };
}

function mergeRows(current: Row[], requests: NativeChatRequest[], recoveryEntries: RecoveryId[], personas: MainChatPersona[]): Row[] {
  const currentByKey = new Map(current.map((row) => [keyOf(row), row]));
  const requestKeys = new Set(requests.map(keyOf));
  const ordered: Row[] = requests.map((request) => {
    const old = currentByKey.get(keyOf(request));
    return { ...old, ...request, ownerMessage: old?.ownerMessage, local: old?.local };
  });
  // The ledger order is authoritative. Keep only local/recovery rows absent from this page after it
  // so a pending send cannot disappear while its receipt is still being reconciled.
  const retainedKeys = new Set(ordered.map(keyOf));
  for (const row of current) {
    if (row.local === true && !requestKeys.has(keyOf(row)) && !retainedKeys.has(keyOf(row))) {
      ordered.push(row);
      retainedKeys.add(keyOf(row));
    }
  }
  for (const recoveryId of recoveryEntries) {
    if (!requestKeys.has(keyOf(recoveryId)) && !retainedKeys.has(keyOf(recoveryId))) {
      ordered.push(recoveredRow(recoveryId, personas));
      retainedKeys.add(keyOf(recoveryId));
    }
  }
  return ordered;
}

export function MainChat({ client, executionId, personas, refreshSequence = 0, onConnect, recipientId = null, seed = null, refreshNonce: externalRefresh = 0, onRequestsChange }: MainChatProps) {
  const principal = personas[0] ?? null;
  const [selectedId, setSelectedId] = useState(principal?.chat.id ?? "");
  const [message, setMessage] = useState("");
  const [rows, setRows] = useState<Row[]>([]);
  const [sending, setSending] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const [readError, setReadError] = useState("");
  const [refreshNonce, setRefreshNonce] = useState(0);
  const [hydrated, setHydrated] = useState(false);
  const [readFailed, setReadFailed] = useState(false);
  const generation = useRef(0);
  const scopeRef = useRef("");
  const reading = useRef(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const scopeKey = executionId;

  const byChat = useMemo(() => new Map(personas.map((persona) => [persona.chat.id, persona])), [personas]);
  const selected = byChat.get(selectedId) ?? principal;
  useEffect(() => {
    if (recipientId !== null && byChat.has(recipientId)) setSelectedId(recipientId);
  }, [recipientId, byChat]);
  const prefixLength = (charter: string) => charter.trim() === "" ? 0 : `Persona instructions for this activity:\n${charter.trim()}\n\n`.length;
  const messageLimitFor = (persona: MainChatPersona | null) => Math.max(0, MAX_MESSAGE_LENGTH - (persona ? prefixLength(persona.charter) : 0));
  const messageLimit = messageLimitFor(selected);
  useEffect(() => {
    if (seed !== null && seed.text.length <= messageLimit) setMessage((current) => current === "" ? seed.text : current);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the nonce is the trigger
  }, [seed?.nonce]);
  useEffect(() => { if (externalRefresh > 0) setRefreshNonce((value) => value + 1); }, [externalRefresh]);
  useEffect(() => { onRequestsChange?.(rows); }, [rows, onRequestsChange]);
  const teamTargets = personas.filter((persona) => persona.chat.id !== selected?.chat.id);
  const activeRows = rows;
  const blockedByPending = activeRows.some((row) => isPending(row.state) &&
    (row.threadId === selected?.chat.id || !byChat.has(row.threadId)));
  const teamBlockedByPending = activeRows.some((row) => isPending(row.state) &&
    (teamTargets.some((persona) => persona.chat.id === row.threadId) || !byChat.has(row.threadId)));
  const latestBlocked = activeRows.filter((row) => row.state === "blocked").at(-1);
  // Rows are merged in ledger order and local sends append at the end. Keep the latest
  // observed chat outcome ahead of older blocked history so a stale refusal cannot become
  // the current instruction for the operator.
  const latestActual = activeRows.filter((row) => row.state === "received" || row.state === "completed").at(-1);
  const latestActualIndex = latestActual === undefined ? -1 : activeRows.lastIndexOf(latestActual);
  const latestBlockedIndex = latestBlocked === undefined ? -1 : activeRows.lastIndexOf(latestBlocked);
  const latestReply = activeRows.filter((row) => row.state === "completed" && row.text).at(-1) ?? null;
  const hasReceiptPending = activeRows.some((row) => isPending(row.state));
  const canPrepareSend = client !== null && selected !== null && hydrated && !readFailed && message.trim() !== "" && message.length <= messageLimit && !sending;
  const canSend = canPrepareSend && !blockedByPending;
  const canSendTeam = canPrepareSend && !teamBlockedByPending && teamTargets.length > 0;
  const nativeApiAvailable = client !== null && typeof client.sendNativeChat === "function" && typeof client.listNativeChatRequests === "function";

  useEffect(() => {
    const scopeChanged = scopeRef.current !== scopeKey;
    if (scopeChanged) {
      generation.current += 1;
      scopeRef.current = scopeKey;
      setRows([]);
      setSelectedId(principal?.chat.id ?? "");
      setSending(false);
      setMessage("");
      setNotice(null);
      setReadError("");
      setHydrated(false);
      setReadFailed(false);
    }
    const run = generation.current;
    if (!nativeApiAvailable || executionId === "" || personas.length === 0) return;
    if (scopeChanged || !hydrated) setHydrated(false);
    let cancelled = false;
    const read = async () => {
      const recovery = readRecovery(executionId);
      if (!recovery.ok) {
        if (!cancelled && run === generation.current) {
          setReadFailed(true);
          setHydrated(true);
          setReadError("Recovery state could not be read. Sending is disabled until storage is available.");
        }
        return;
      }
      setRows((current) => {
        return mergeRows(current, current, recovery.entries, personas);
      });
      try {
        const page = await client.listNativeChatRequests(executionId);
        if (cancelled || run !== generation.current) return;
        setRows((current) => mergeRows(current, page.requests, recovery.entries, personas));
        for (const request of page.requests) {
          if (request.state === "completed" || request.state === "blocked") removeRecovery({ ...request, executionId });
        }
        setReadError("");
        setReadFailed(false);
      } catch (error) {
        if (!cancelled && run === generation.current) {
          setReadFailed(true);
          setReadError(errorText(error));
        }
      } finally {
        if (!cancelled && run === generation.current) setHydrated(true);
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
        const recovery = readRecovery(executionId);
        if (!recovery.ok) {
          if (!cancelled && run === generation.current) {
            setReadFailed(true);
            setHydrated(true);
            setReadError("Recovery state could not be read. Sending is disabled until storage is available.");
          }
          return;
        }
        const page = await client.listNativeChatRequests(executionId);
        if (cancelled || run !== generation.current) return;
        setRows((current) => {
          return mergeRows(current, page.requests, recovery.entries, personas);
        });
        for (const request of page.requests) {
          if (request.state === "completed" || request.state === "blocked") removeRecovery({ ...request, executionId });
        }
        setReadError("");
        setReadFailed(false);
        setHydrated(true);
      } catch (error) {
        if (!cancelled && run === generation.current) {
          setReadFailed(true);
          setReadError(errorText(error));
        }
      } finally {
        reading.current = false;
      }
    };
    if (hasReceiptPending || refreshNonce > 0 || refreshSequence > 0) void read();
    const timer = hasReceiptPending ? window.setInterval(() => void read(), POLL_MS) : undefined;
    return () => { cancelled = true; if (timer !== undefined) window.clearInterval(timer); };
  }, [client, executionId, hasReceiptPending, nativeApiAvailable, refreshNonce, refreshSequence, scopeKey]);

  const send = async (fanout: boolean) => {
    if (!(fanout ? canSendTeam : canSend) || selected === null || client === null || !nativeApiAvailable) return;
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
    if (!addRecovery(executionId, requests)) {
      setNotice({ tone: "bad", text: "Recovery storage is unavailable. Nothing was sent." });
      setReadFailed(true);
      return;
    }
    setSending(true);
    setNotice(null);
    const run = generation.current;
    const pendingRows: Row[] = requests.map((request) => ({ ...request, state: "requested", local: true }));
    setRows((current) => [...current, ...pendingRows]);
    setMessage("");
    // The Runtime serializes durable admission. Give each recipient its own confirmation
    // window instead of spending all three windows while the same intent lock is held.
    const outcomes: PromiseSettledResult<Awaited<ReturnType<MainChatClient["sendNativeChat"]>>>[] = [];
    for (const request of requests) {
      let timeout!: number;
      const timeoutPromise = new Promise<never>((_, reject) => {
        timeout = window.setTimeout(() => reject(new Error("Timed out before the Runtime confirmed this request.")), TIMEOUT_MS);
      });
      try {
        outcomes.push({ status: "fulfilled", value: await Promise.race([client.sendNativeChat(executionId, request), timeoutPromise]) });
      } catch (reason) {
        outcomes.push({ status: "rejected", reason });
      } finally {
        window.clearTimeout(timeout);
      }
    }
    // These public diagnostics reject a fresh request before durable intent/native dispatch.
    // An opaque HTTP error or immutable request-ID conflict remains uncertain.
    const rejectedBeforeDispatch = outcomes.map((result) => result.status === "rejected" &&
      result.reason instanceof RuntimeError && result.reason.httpStatus === 400 &&
      result.reason.diagnostics.some((diagnostic) => diagnostic.code === "GHCLI001_ARGUMENT_INVALID" &&
        diagnostic.source === "serve-cli" && ["/actorType", "/idempotencyKey", "/nativeChats", "/threadId", "/execution", "/nodeId"].includes(diagnostic.path)));
    requests.forEach((request, index) => {
      if (rejectedBeforeDispatch[index]) removeRecovery({ executionId, requestId: request.requestId, nodeId: request.nodeId, threadId: request.threadId });
    });
    if (run !== generation.current) return;
    setRows((current) => current.map((row) => {
      const index = requests.findIndex((request) => request.requestId === row.requestId);
      const result = index < 0 ? null : outcomes[index];
      if (!result || row.local !== true) return row;
      if (row.state === "completed" || row.state === "blocked") return row;
      if (result.status === "fulfilled") return row;
      const definitive = rejectedBeforeDispatch[index];
      return { ...row, state: definitive ? "blocked" : "unobserved", detail: definitive ? errorText(result.reason) : "Send outcome is unobserved. Reconcile this request id before trying again." };
    }));
    const failed = outcomes.filter((outcome) => outcome.status === "rejected").length;
    setNotice(failed === 0 ? { tone: "good", text: "The request was accepted. Waiting for authoritative chat receipts." } : { tone: "bad", text: `${failed} request${failed === 1 ? "" : "s"} could not be proven; no request was retried.` });
    setSending(false);
  };

  if (!nativeApiAvailable) return <section className="main-chat" aria-label="Main chat"><h2>Main chat</h2><p>This Runtime does not expose native chat requests yet. Connect an existing chat to continue.</p>{onConnect && <button type="button" onClick={onConnect}>Connect existing chat</button>}</section>;
  if (personas.length === 0) return <section className="main-chat" aria-label="Main chat"><h2>Main chat</h2><p>Connect existing chat to choose a coordinator.</p>{onConnect && <button type="button" onClick={onConnect}>Connect existing chat</button>}</section>;

  return <section className="main-chat" aria-label="Main chat">
    <header><h2>Main chat</h2><p aria-label="Next step">{blockedByPending || readFailed
      ? "Next step: refresh the previous request's status. You can prepare your next instruction below; sending waits for confirmation."
      : "Next step: use a Jev suggestion or write your own instruction, then choose who receives it."}</p></header>
    <label htmlFor="main-chat-recipient">Main recipient</label>
    <select id="main-chat-recipient" value={selected?.chat.id ?? ""} disabled={sending} onChange={(event) => setSelectedId(event.target.value)}>
      {personas.map((persona) => <option key={persona.chat.id} value={persona.chat.id}>{persona.chat.title} · {persona.chat.id}</option>)}
    </select>
    <label htmlFor="main-chat-message">Instruction</label>
    <textarea ref={textareaRef} id="main-chat-message" aria-describedby="main-chat-request-status" value={message} maxLength={messageLimit} rows={4} onChange={(event) => setMessage(event.target.value)} placeholder="Tell the coordinator what to do" />
    <p>{message.length}/{messageLimit} characters</p>
    <p className="main-chat-team-targets">Team targets: {teamTargets.length === 0 ? "none" : teamTargets.map((persona) => persona.chat.title).join(", ")}</p>
    <div id="main-chat-request-status" className="main-chat-status">
      <p role="status">{readFailed ? "Request status could not be checked. Refresh before sending. Your instruction remains editable."
        : blockedByPending ? "Sending is paused until the previous request is confirmed. You can keep writing your next instruction."
        : hasReceiptPending ? "Another chat has an unresolved request. You can send to this recipient; team sending waits for its recipients to be confirmed."
        : latestBlockedIndex > latestActualIndex ? "The previous request needs attention in Codex. Check the original chat before sending another instruction."
        : latestReply ? "The latest request completed. Review the reply below before sending the next instruction."
        : "No pending native requests."}</p>
      {(activeRows.length > 0 || readFailed) && <button type="button" className="main-chat-refresh" onClick={() => setRefreshNonce((value) => value + 1)}>Refresh request status</button>}
    </div>
    {latestReply && <section className="main-chat-latest-reply" aria-label="Latest chat reply">
      <strong>Latest reply · {latestReply.title}</strong>
      <p>{latestReply.text}</p>
    </section>}
    <div className="main-chat-actions" aria-describedby="main-chat-request-status"><button type="button" disabled={!canSend} onClick={() => void send(false)}>Send to main chat</button><button type="button" disabled={!canSendTeam} onClick={() => void send(true)}>Send to team ({teamTargets.length} other{teamTargets.length === 1 ? "" : "s"})</button></div>
    {readError && <p role="alert">{readError}</p>}{notice && <p role={notice.tone === "bad" ? "alert" : "status"}>{notice.text}</p>}
    <details className="main-chat-history">
      <summary>Request history ({activeRows.length})</summary>
      <div aria-label="Main chat conversation" className="main-chat-conversation">
        {activeRows.map((row) => <article key={keyOf(row)} className={`main-chat-request main-chat-request-${row.state}`}>
          {row.ownerMessage && <p><strong>You</strong>: {row.ownerMessage}</p>}
          <strong>{stateLabel(row.state)}</strong><span> {row.title}</span>
          {row.text && <p>{row.text}</p>}
          <details className="main-chat-technical-details">
            <summary>Technical details</summary>
            <dl>
              <dt>Request ID</dt><dd><code>{row.requestId}</code></dd>
              <dt>Node ID</dt><dd><code>{row.nodeId}</code></dd>
              <dt>Thread ID</dt><dd><code>{row.threadId}</code></dd>
              <dt>Execution ID</dt><dd><code>{executionId}</code></dd>
              {row.turnId && <><dt>Turn ID</dt><dd><code>{row.turnId}</code></dd></>}
              <dt>Source directory</dt><dd><code>{row.sourceDirectory}</code></dd>
              <dt>State</dt><dd>{row.state}</dd>
              {row.detail && <><dt>Diagnostic</dt><dd>{row.detail}</dd></>}
            </dl>
          </details>
        </article>)}
        {activeRows.length === 0 && <p className="main-chat-empty">No orders sent yet. Your next message will appear here.</p>}
      </div>
    </details>
  </section>;
}
