/**
 * The Chat column (spec §4.4): question cards on top, thread tabs, messages, the Jev card and the
 * composer for the selected tab's audience. Native sends stay in MainChat with its gating; this
 * column only decides which composer the tab gets. MainChat (the `principal` slot) is always
 * mounted and only hidden when the tab does not use it, so its gating and request ledger stay alive.
 *
 * Layout (#325): the column is two rows. `.chat-scroll` is the ONE vertical scroll region (cards,
 * tabs, messages, Jev, and MainChat's history, portalled in through MainChatHistorySlot);
 * `.chat-dock` holds the composers and never scrolls away. No child scrolls on its own.
 *
 * Reading (#327): the scroll region opens at the NEWEST message, chat convention. Status and request
 * history sit above the messages; question cards and Jev sit right above the docked composer. A
 * viewer at the bottom stays there as messages arrive; a viewer who scrolled up is never moved and
 * gets a "New messages" pill instead. The column is resizable from its right edge.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";

import { CHAT_DEFAULT, CHAT_MAX, CHAT_MIN, clampChatWidth, loadChatWidth, saveChatWidth } from "../rail-width";

import { MainChatHistorySlot } from "./main-chat";

import { MAX_MESSAGE_LENGTH } from "../runtime/client";
import type { ReplySuggestion } from "../runtime/types";
import type { Bot } from "../runtime/team";
import { EVERYONE, parseMention, type ChatThread } from "../runtime/threads";
import { ago, hueOf } from "./format";

export interface ChatColumnProps {
  threads: ChatThread[];
  selected: string;
  onSelect: (key: string) => void;
  unread: Record<string, number>;
  bots: Bot[];
  names: Record<string, string>;
  openingCount: number;
  cards: ReactNode;
  jev: { suggestions: ReplySuggestion[]; loading: boolean; issue: string | null; older?: boolean; onRetry?: () => void };
  nativeKeys: ReadonlySet<string>;
  principal: ReactNode;
  /** Resolves true only once the Runtime confirmed the message; the draft is kept otherwise. */
  onSend: (text: string, to: string | null, replyTo: string | null, task?: string | null) => Promise<boolean>;
  sending: boolean;
  sendError: string;
  answering: { asker: string; signalId: string | null } | null;
  onClearAnswer: () => void;
  composerFocus: number;
  highlight: number | null;
  onUseSuggestion: (text: string) => void;
}

export function composerMode(thread: ChatThread | undefined, nativeKeys: ReadonlySet<string>): "record" | "native" | "record+principal" | "none" {
  if (thread === undefined || thread.kind === "everyone") return nativeKeys.size > 0 ? "native" : "record+principal";
  // #396: speaking inside a task thread records a message tagged with that task.
  if (thread.kind === "task") return "record";
  return nativeKeys.has(thread.participants[0]) ? "native" : "record";
}

/** Within this many pixels of the end counts as "at the bottom" (sub-pixel rounding, a last line). */
const BOTTOM_SLACK = 48;

function prefersReducedMotion(): boolean {
  try {
    return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
  } catch {
    return false;
  }
}

export function ChatColumn(props: ChatColumnProps) {
  const thread = props.threads.find((candidate) => candidate.key === props.selected) ?? props.threads[0];
  // #393: task threads that are merged or quiet fold under "older", shown on request.
  const [showOlder, setShowOlder] = useState(false);
  const olderCount = props.threads.filter((candidate) => candidate.older && candidate.key !== thread?.key).length;
  const mode = composerMode(thread, props.nativeKeys);
  // One draft per thread: a single shared draft followed the operator across tabs, so text
  // written for one bot could be sent to another.
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const draftKey = thread?.key ?? EVERYONE;
  const draft = drafts[draftKey] ?? "";
  const setDraftFor = (key: string, text: string) => setDrafts((current) => ({ ...current, [key]: text }));
  const box = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { if (props.composerFocus > 0) box.current?.focus(); }, [props.composerFocus]);
  useEffect(() => {
    if (props.highlight !== null) document.getElementById(`chat-msg-${props.highlight}`)?.scrollIntoView({ block: "center" });
  }, [props.highlight, props.selected]);

  const send = async () => {
    const text = draft.trim();
    if (text === "" || props.sending) return;
    const key = draftKey;
    let ok: boolean;
    if (props.answering !== null) ok = await props.onSend(text, props.answering.asker, props.answering.signalId);
    else if (thread?.kind === "direct") ok = await props.onSend(text, thread.participants[0], null);
    else if (thread?.kind === "task") { const target = parseMention(text, props.bots); ok = await props.onSend(target.text, target.to, null, thread.key.slice("task:".length)); }
    else { const target = parseMention(text, props.bots); ok = await props.onSend(target.text, target.to, null); }
    // A refused or unconfirmed send keeps its words; only a delivered one clears its own thread.
    if (ok) setDraftFor(key, "");
  };
  const suggestion = props.jev.suggestions[0];
  const showPrincipal = mode === "native" || mode === "record+principal";
  const [historySlot, setHistorySlot] = useState<HTMLElement | null>(null);

  // Width: dragged from the right edge or set with the arrow keys; remembered in this browser only.
  const column = useRef<HTMLElement>(null);
  const [width, setWidth] = useState(loadChatWidth);
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    if (!dragging) return;
    const move = (event: PointerEvent) => {
      const left = column.current?.getBoundingClientRect().left ?? 0;
      setWidth(clampChatWidth(event.clientX - left));
    };
    const release = () => {
      setDragging(false);
      setWidth((current) => { saveChatWidth(current); return current; });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
    };
  }, [dragging]);
  const resizeBy = (next: number) => { const clamped = clampChatWidth(next); setWidth(clamped); saveChatWidth(clamped); };

  // Scrolling: open at the newest message, follow it only while the viewer is at the bottom.
  const scroller = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const [unseen, setUnseen] = useState(false);
  const messages = thread?.messages ?? [];
  const lastId = messages.at(-1)?.id ?? "";
  const toBottom = useCallback((smooth: boolean) => {
    const el = scroller.current;
    if (el === null) return;
    const top = el.scrollHeight;
    if (smooth && !prefersReducedMotion() && typeof el.scrollTo === "function") el.scrollTo({ top, behavior: "smooth" });
    else el.scrollTop = top;
    atBottom.current = true;
    setUnseen(false);
  }, []);
  // A thread switch is a fresh reading: start at its newest message.
  useLayoutEffect(() => { toBottom(false); }, [thread?.key, toBottom]);
  // A new message follows the viewer to the bottom only if they were there already.
  useLayoutEffect(() => {
    if (lastId === "") return;
    if (atBottom.current) toBottom(false);
    else setUnseen(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the newest id is the trigger
  }, [lastId]);
  // Content above or below can grow after paint (sealed records opening, the request history
  // arriving); a viewer pinned to the bottom stays pinned through that too.
  useEffect(() => {
    const el = scroller.current;
    if (el === null || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => { if (atBottom.current) el.scrollTop = el.scrollHeight; });
    for (const child of Array.from(el.children)) observer.observe(child);
    return () => observer.disconnect();
  });
  const onScroll = () => {
    const el = scroller.current;
    if (el === null) return;
    atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK;
    if (atBottom.current) setUnseen(false);
  };

  return (
    <aside className="chat-column" aria-label="Chat" ref={column} style={{ "--chat-w": `${width}px` } as CSSProperties} data-resizing={dragging || undefined}>
      <div className="chat-tabpanel" id="studio-panel-chat" role="tabpanel" aria-labelledby="studio-tab-chat">
      <div className="chat-scroll" ref={scroller} onScroll={onScroll}>
      <div className="chat-tabs" role="tablist" aria-label="Threads">
        {props.threads.filter((candidate) => showOlder || !candidate.older || candidate.key === thread?.key).map((candidate) => {
          const unread = props.unread[candidate.key] ?? 0;
          return (
            <button key={candidate.key} type="button" role="tab" aria-selected={candidate.key === thread?.key}
              className={candidate.older ? "chat-tab-older" : undefined}
              aria-label={unread > 0 ? `${candidate.label}, ${unread} unread` : candidate.label} onClick={() => props.onSelect(candidate.key)}>
              {candidate.label}{unread > 0 && <span className="chat-unread" aria-hidden="true">{unread}</span>}
            </button>
          );
        })}
        {olderCount > 0 && (
          <button type="button" className="chat-older-toggle" aria-expanded={showOlder} onClick={() => setShowOlder((open) => !open)}>
            {showOlder ? "Hide older" : `older (${olderCount})`}
          </button>
        )}
      </div>
      <div className="chat-principal-history" ref={setHistorySlot} hidden={!showPrincipal} />
      {thread?.kind === "task" && <p className="chat-recorded-note">Recorded messages about this task: what the agents recorded through the Runtime, not their native chats.</p>}
      <ol className="chat-messages" role="tabpanel" aria-label={thread?.label ?? "Everyone"}>
        {thread?.key === EVERYONE && props.openingCount > 0 && <li className="chat-opening">Opening {props.openingCount} sealed records…</li>}
        {messages.map((message) => {
          const name = props.names[message.sender] ?? (message.sender === "studio-operator" ? "You" : message.sender);
          return (
            <li key={message.id} id={`chat-msg-${message.sequence}`} className={`chat-message ${props.highlight === message.sequence ? "chat-message-highlight" : ""}`}
              style={{ "--bot-hue": String(hueOf(message.sender)) } as CSSProperties}>
              <span className="chat-avatar" aria-hidden="true">{name.slice(0, 1).toUpperCase()}</span>
              <div>
                <p className="chat-meta"><strong>{name}</strong>{message.to !== null && <> → {props.names[message.to] ?? (message.to === "studio-operator" ? "you" : message.to)}</>} · {ago(message.at)}</p>
                <p className="chat-text">{message.text}</p>
              </div>
            </li>
          );
        })}
      </ol>
      {props.cards}
      {(props.jev.loading || suggestion !== undefined || props.jev.issue !== null) && mode !== "none" && (
        <section className="jev-card" aria-label="Jev suggests">
          {props.jev.loading ? <p role="status">Jev is preparing a suggestion…</p> : suggestion ? <>
            <p className="jev-draft">{suggestion.draft}</p>
            <p className="jev-reason">{suggestion.reason}</p>
            {props.jev.older === true && <p className="jev-older">Based on the run as of a moment ago</p>}
            <button type="button" onClick={() => { setDraftFor(draftKey, suggestion.draft); props.onUseSuggestion(suggestion.draft); box.current?.focus(); }}>Use</button>
          </> : <>
            <p role="status" className="jev-issue">{props.jev.issue}</p>
            {props.jev.onRetry && <button type="button" onClick={props.jev.onRetry}>Retry</button>}
          </>}
        </section>
      )}
      {unseen && <button type="button" className="chat-new-pill" onClick={() => toBottom(true)}>New messages ↓</button>}
      </div>
      <div className="chat-dock">
      {(mode === "record" || mode === "record+principal") && (
        <div className="chat-composer">
          {props.answering !== null && (
            <p className="chat-answering">Answering {props.names[props.answering.asker] ?? props.answering.asker} <button type="button" onClick={props.onClearAnswer}>Clear</button></p>
          )}
          <label htmlFor="chat-message" className="sr-only">Message</label>
          <textarea id="chat-message" ref={box} value={draft} rows={3} maxLength={MAX_MESSAGE_LENGTH} onChange={(event) => setDraftFor(draftKey, event.target.value)}
            placeholder={thread?.kind === "direct" ? `Message ${thread.label}` : "Message everyone, or @name one bot"} />
          <button type="button" disabled={props.sending || draft.trim() === ""} onClick={() => void send()}>Send</button>
          {props.sendError && <p role="alert">{props.sendError}</p>}
        </div>
      )}
      <div className="chat-principal" hidden={!showPrincipal}>
        <MainChatHistorySlot.Provider value={historySlot}>{props.principal}</MainChatHistorySlot.Provider>
      </div>
      </div>
      <div role="separator" aria-label="Resize the chat column" aria-orientation="vertical" tabIndex={0}
        aria-valuenow={width} aria-valuemin={CHAT_MIN} aria-valuemax={CHAT_MAX}
        className={`chat-resize ${dragging ? "dragging" : ""}`}
        onPointerDown={(event) => { event.preventDefault(); setDragging(true); }}
        onDoubleClick={() => resizeBy(CHAT_DEFAULT)}
        onKeyDown={(event) => {
          const next = event.key === "ArrowLeft" ? width - 16 : event.key === "ArrowRight" ? width + 16
            : event.key === "Home" ? CHAT_MIN : event.key === "End" ? CHAT_MAX : null;
          if (next === null) return;
          event.preventDefault();
          resizeBy(next);
        }} />
      </div>
    </aside>
  );
}
