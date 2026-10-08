/**
 * The sealed words of a message, opened into READING (#479 owner ask: "the text reads like code").
 *
 * Agents write plain text with the shapes people write: blank-line paragraphs, dash or numbered
 * lists, `backticked` identifiers, **bold**, links. They also sign every message with an identity
 * line (`Session: <lane> · Head: <sha>`) and cite issues, PRs, journeys, shas, paths and codes.
 * This renders those shapes for a person:
 * - the identity line becomes a small muted chip (who · short sha), not body text;
 * - the first line is the message's headline, in bold;
 * - a long body folds after six lines behind "Show more";
 * - `#123` / `PR #123` link to the repository on GitHub (when the run names one), and a journey
 *   id the Studio knows opens the Journey tab;
 * - shas, paths and dotted codes read as dimmed inline code.
 * Everything is built as React nodes: a message can never smuggle markup, and only http(s) links
 * are ever made.
 */
import React, { createContext, useContext, useState } from "react";

export interface SaidLinkContext {
  /** `https://github.com/<owner>/<repo>`, from the run's task records; null makes no GitHub links. */
  repoUrl: string | null;
  /** Journey (flow or contract) ids the Studio can open. */
  journeyIds: ReadonlySet<string>;
  onOpenJourney?: (journeyId: string) => void;
}

export const SaidLinks = createContext<SaidLinkContext>({ repoUrl: null, journeyIds: new Set() });

/** `Session: gh-claude-4 · Head: 3da974f6 — rest` → the signer, the head and what follows. The
 * identity is the first line, or (as some lanes sign) the last line, with or without a head. */
const IDENTITY = /^\s*Session:\s*([^·\n—–]+?)\s*(?:·\s*Head:\s*([^\s—–\n]+(?:\s+PR\s+\d+)?))?(?:\s+[—–-]\s+|\s*\n|\s*$)/;
const TRAILING_IDENTITY = /\n\s*Session:\s*([^·\n]+?)\s*(?:·\s*Head:\s*(\S+)\s*)?$/;

export function splitIdentity(text: string): { sender: string; head: string | null; rest: string } | null {
  const leading = IDENTITY.exec(text);
  if (leading !== null) return { sender: leading[1]!.trim(), head: leading[2]?.trim() ?? null, rest: text.slice(leading[0].length) };
  const trailing = TRAILING_IDENTITY.exec(text.trimEnd());
  if (trailing !== null) return { sender: trailing[1]!.trim(), head: trailing[2]?.trim() ?? null, rest: text.trimEnd().slice(0, trailing.index) };
  return null;
}

const SHA = /^[0-9a-f]{7,40}$/;
const shortHead = (head: string) => (SHA.test(head) ? head.slice(0, 8) : head);

/* One pass over a line, in priority order. Named groups keep the cases apart. */
const TOKEN = new RegExp(
  [
    "`(?<code>[^`\\n]{1,160})`",
    "\\*\\*(?<bold>[^*\\n]{1,200})\\*\\*",
    "\\[(?<label>[^\\]\\n]{1,120})\\]\\((?<href>https?:\\/\\/[^)\\s]{1,400})\\)",
    "(?<url>https?:\\/\\/[^\\s<>()\\]]{1,400}[^\\s<>()\\].,;:!?'\"])",
    "(?<pr>\\b(?:PRs?|pull request)\\s+#?(?<prnum>\\d{1,6})\\b)",
    "(?<issue>(?<![\\w&/])#(?<issuenum>\\d{1,6})\\b)",
    "(?<sha>\\b(?=[0-9a-f]*[a-f])(?=[0-9a-f]*\\d)[0-9a-f]{7,40}\\b)",
    "(?<path>\\b(?:[A-Za-z]:[\\\\/]|(?:apps|docs|tools|core|adapters|crates|schemas|extensions|\\.graphhelm|\\.claude)\\/)[^\\s`'\",;)]+)",
    "(?<dotted>\\b[a-z][a-z0-9-]{2,}(?:\\.[a-z][a-z0-9_-]*)*\\.[a-z][a-z0-9_-]{2,}\\b)",
    "(?<word>\\b[a-z0-9]+(?:-[a-z0-9]+)+(?:\\.[a-z0-9-]+)?\\b)",
  ].join("|"),
  "g",
);

function Inline({ text }: { text: string }) {
  const links = useContext(SaidLinks);
  const out: React.ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const match of text.matchAll(TOKEN)) {
    const groups = match.groups ?? {};
    const index = match.index ?? 0;
    const whole = match[0];
    let node: React.ReactNode | null = null;
    if (groups.code !== undefined) node = <code key={key++}>{groups.code}</code>;
    else if (groups.bold !== undefined) node = <strong key={key++}>{groups.bold}</strong>;
    else if (groups.href !== undefined) node = <a key={key++} href={groups.href} title={groups.href} target="_blank" rel="noreferrer noopener">{groups.label}</a>;
    else if (groups.url !== undefined) node = <a key={key++} href={groups.url} target="_blank" rel="noreferrer noopener">{groups.url}</a>;
    else if (groups.pr !== undefined && links.repoUrl !== null) node = <a key={key++} href={`${links.repoUrl}/pull/${groups.prnum}`} target="_blank" rel="noreferrer noopener">{whole}</a>;
    else if (groups.issue !== undefined && links.repoUrl !== null) node = <a key={key++} href={`${links.repoUrl}/issues/${groups.issuenum}`} target="_blank" rel="noreferrer noopener">{whole}</a>;
    else if (groups.sha !== undefined || groups.path !== undefined || groups.dotted !== undefined) node = <code key={key++} className="said-dim">{whole}</code>;
    else if (groups.word !== undefined && links.journeyIds.has(whole) && links.onOpenJourney) {
      const open = links.onOpenJourney;
      node = <button key={key++} type="button" className="said-journey" onClick={() => open(whole)}>{whole}</button>;
    }
    if (node === null) continue;
    if (index > last) out.push(text.slice(last, index));
    out.push(node);
    last = index + whole.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <>{out}</>;
}

const LIST_MARK = /^\s*(?:[-*]|\(\d{1,3}\)|\d{1,3}[.)])\s+/;

/* An inline enumeration mid-sentence: "... nesta ordem: (1) isto; (2) aquilo". Two or more of
 * these in one paragraph and the paragraph is a list that never got its line breaks. */
const INLINE_ENUM = /\s*\((\d{1,2})\)\s+/g;

/** Agents write without blank lines, so a paragraph has to be FOUND, not just split: a long
 * unbroken run is chunked at sentence ends into readable lengths. Presentation only - every
 * character of the message survives, in order. */
function sentencesOf(text: string): string[] {
  const parts = text.split(/(?<=[.!?:])\s+(?=[A-ZÀ-Ü(‘“"'`\d])/);
  const TARGET = 240;
  const chunks: string[] = [];
  let current = "";
  for (const part of parts) {
    if (current.length > 0 && current.length + part.length > TARGET) {
      chunks.push(current);
      current = part;
    } else {
      current = current.length > 0 ? current + " " + part : part;
    }
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}

function paragraphsOf(block: string, keyBase: string): React.ReactNode[] {
  // An inline (1) (2) (3) enumeration becomes the numbered list it always wanted to be.
  const enumMatches = [...block.matchAll(INLINE_ENUM)];
  if (enumMatches.length >= 2) {
    const first = enumMatches[0]!;
    const lead = block.slice(0, first.index).trim();
    const items: string[] = [];
    for (let index = 0; index < enumMatches.length; index += 1) {
      const from = enumMatches[index]!.index! + enumMatches[index]![0].length;
      const to = index + 1 < enumMatches.length ? enumMatches[index + 1]!.index! : block.length;
      items.push(block.slice(from, to).trim());
    }
    return [
      ...(lead.length > 0 ? [<p key={keyBase + "-lead"}><Inline text={lead} /></p>] : []),
      <ol key={keyBase + "-enum"}>
        {items.map((item, itemIndex) => (
          <li key={itemIndex}><Inline text={item} /></li>
        ))}
      </ol>,
    ];
  }
  return sentencesOf(block).map((chunk, chunkIndex) => (
    <p key={keyBase + "-" + chunkIndex}><Inline text={chunk} /></p>
  ));
}

function blocksOf(text: string): React.ReactNode[] {
  const blocks = text.replace(/\r\n/g, "\n").split(/\n{2,}/);
  return blocks.flatMap((block, blockIndex): React.ReactNode[] => {
    const lines = block.split("\n");
    const listy = lines.length > 1 && lines.filter((line) => LIST_MARK.test(line)).length >= 2;
    if (listy) {
      // Lines before the first marker stay a lead-in paragraph; marked lines become items, and
      // an unmarked continuation line belongs to the item above it.
      const lead: string[] = [];
      const items: string[] = [];
      for (const line of lines) {
        if (LIST_MARK.test(line)) items.push(line.replace(LIST_MARK, ""));
        else if (items.length === 0) lead.push(line);
        else items[items.length - 1] += "\n" + line;
      }
      return [
        <React.Fragment key={blockIndex}>
          {lead.length > 0 && <p><Inline text={lead.join("\n")} /></p>}
          <ul>
            {items.map((item, itemIndex) => (
              <li key={itemIndex}><Inline text={item} /></li>
            ))}
          </ul>
        </React.Fragment>,
      ];
    }
    return paragraphsOf(block, String(blockIndex));
  });
}

/** The headline: the message's first line. A short one-line message is all headline; a long one
 * has none and reads as paragraphs. */
function headlineOf(text: string): { headline: string; body: string } {
  const trimmed = text.replace(/^\s+/, "");
  const newline = trimmed.indexOf("\n");
  if (newline >= 0) return { headline: trimmed.slice(0, newline).trim(), body: trimmed.slice(newline + 1).replace(/^\n+/, "") };
  return trimmed.length <= 160 ? { headline: trimmed, body: "" } : { headline: "", body: trimmed };
}

const FOLD_LINES = 6;

/** One message, read: identity chip, bold headline, body folded after six lines. */
export function SaidText({ text, author }: { text: string; /** The message's own sender; its signature alone adds nothing. */ author?: string }) {
  const [open, setOpen] = useState(false);
  const identity = splitIdentity(text);
  const { headline, body } = headlineOf(identity === null ? text : identity.rest);
  const lines = body.split("\n");
  const long = lines.length > FOLD_LINES || body.length > 700;
  const shown = long && !open ? lines.slice(0, FOLD_LINES).join("\n").slice(0, 700) : body;
  return (
    <>
      {identity !== null && !(identity.head === null && identity.sender === author) && (
        <span className="said-identity" title={`Session ${identity.sender}${identity.head === null ? "" : ` · head ${identity.head}`}`}>
          {identity.sender}{identity.head !== null && <> · <code>{shortHead(identity.head)}</code></>}
        </span>
      )}
      {headline.length > 0 && <p className="said-headline"><strong><Inline text={headline} /></strong></p>}
      {shown.trim().length > 0 && blocksOf(shown)}
      {long && (
        <button type="button" className="show-more" aria-expanded={open} onClick={() => setOpen((value) => !value)}>
          {open ? "Show less" : "Show more"}
        </button>
      )}
    </>
  );
}
