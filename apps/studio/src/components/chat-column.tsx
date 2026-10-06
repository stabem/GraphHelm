/**
 * The Chat column (spec §4.4): question cards on top, thread tabs, messages, the Jev card and the
 * composer for the selected tab's audience. Native sends stay in MainChat with its gating; this
 * column only decides which composer the tab gets. MainChat (the `principal` slot) is always
 * mounted and only hidden when the tab does not use it, so its gating and request ledger stay alive.
 */
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";

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
  jev: { suggestions: ReplySuggestion[]; loading: boolean; issue: string | null };
  nativeKeys: ReadonlySet<string>;
  principal: ReactNode;
  onSend: (text: string, to: string | null, replyTo: string | null) => void;
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
  if (thread.kind === "pair") return "none";
  return nativeKeys.has(thread.participants[0]) ? "native" : "record";
}

export function ChatColumn(props: ChatColumnProps) {
  const thread = props.threads.find((candidate) => candidate.key === props.selected) ?? props.threads[0];
  const mode = composerMode(thread, props.nativeKeys);
  const [draft, setDraft] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { if (props.composerFocus > 0) box.current?.focus(); }, [props.composerFocus]);
  useEffect(() => {
    if (props.highlight !== null) document.getElementById(`chat-msg-${props.highlight}`)?.scrollIntoView({ block: "center" });
  }, [props.highlight, props.selected]);

  const send = () => {
    const text = draft.trim();
    if (text === "" || props.sending) return;
    if (props.answering !== null) props.onSend(text, props.answering.asker, props.answering.signalId);
    else if (thread?.kind === "direct") props.onSend(text, thread.participants[0], null);
    else { const target = parseMention(text, props.bots); props.onSend(target.text, target.to, null); }
    setDraft("");
  };
  const suggestion = props.jev.suggestions[0];
  const showPrincipal = mode === "native" || mode === "record+principal";

  return (
    <aside className="chat-column" aria-label="Chat">
      {props.cards}
      <div className="chat-tabs" role="tablist" aria-label="Threads">
        {props.threads.map((candidate) => {
          const unread = props.unread[candidate.key] ?? 0;
          return (
            <button key={candidate.key} type="button" role="tab" aria-selected={candidate.key === thread?.key}
              aria-label={unread > 0 ? `${candidate.label}, ${unread} unread` : candidate.label} onClick={() => props.onSelect(candidate.key)}>
              {candidate.label}{unread > 0 && <span className="chat-unread" aria-hidden="true">{unread}</span>}
            </button>
          );
        })}
      </div>
      {thread?.kind === "pair" && <p className="chat-recorded-note">Recorded messages: what these agents recorded to each other through the Runtime, not their native chats.</p>}
      <ol className="chat-messages" role="tabpanel" aria-label={thread?.label ?? "Everyone"}>
        {thread?.key === EVERYONE && props.openingCount > 0 && <li className="chat-opening">Opening {props.openingCount} sealed records…</li>}
        {(thread?.messages ?? []).map((message) => {
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
      {(props.jev.loading || suggestion !== undefined) && mode !== "none" && (
        <section className="jev-card" aria-label="Jev suggests">
          {props.jev.loading ? <p role="status">Jev is preparing a suggestion…</p> : suggestion && <>
            <p>{suggestion.draft}</p>
            <p className="jev-reason">{suggestion.reason}</p>
            <button type="button" onClick={() => { setDraft(suggestion.draft); props.onUseSuggestion(suggestion.draft); box.current?.focus(); }}>Use</button>
          </>}
        </section>
      )}
      {(mode === "record" || mode === "record+principal") && (
        <div className="chat-composer">
          {props.answering !== null && (
            <p className="chat-answering">Answering {props.names[props.answering.asker] ?? props.answering.asker} <button type="button" onClick={props.onClearAnswer}>Clear</button></p>
          )}
          <label htmlFor="chat-message" className="sr-only">Message</label>
          <textarea id="chat-message" ref={box} value={draft} rows={3} maxLength={MAX_MESSAGE_LENGTH} onChange={(event) => setDraft(event.target.value)}
            placeholder={thread?.kind === "direct" ? `Message ${thread.label}` : "Message everyone, or @name one bot"} />
          <button type="button" disabled={props.sending || draft.trim() === ""} onClick={send}>Send</button>
          {props.sendError && <p role="alert">{props.sendError}</p>}
        </div>
      )}
      <div className="chat-principal" hidden={!showPrincipal}>{props.principal}</div>
    </aside>
  );
}
