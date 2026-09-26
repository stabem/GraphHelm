/**
 * The window on the left: what is happening, and the thread about it.
 *
 * ONE PANEL, TWO SUBJECTS. Click a node on the board and it shows that node — its facts and the
 * events that named it. Click the run's name in the top strip and it shows the run — the verdict,
 * the sixteen lifecycle states, and the whole log. Two panels would have meant two layouts, two
 * scroll behaviours and two places to fix a rendering bug.
 *
 * THE THREAD IS THE LOG, NOT A SUMMARY OF IT. Each line is composed from the event's own fields;
 * an event this composer cannot state plainly falls back to its payload, verbatim and inert.
 * There is no model in this path and nothing is paraphrased: a console that rewrites its own
 * record is a console whose record cannot be trusted.
 *
 * Everything rendered here comes out of the Runtime and is UNTRUSTED CONTENT. It is text through
 * JSX, which escapes it. There is no `dangerouslySetInnerHTML` in this application.
 */

import React, { useEffect, useRef, useState } from "react";
import { NodeDeliveries, type DocumentReference } from "./deliveries";
import { LoaderCircle, Send, TriangleAlert, X } from "lucide-react";

import { MAX_MESSAGE_LENGTH, OPERATOR_ACTOR } from "../runtime/client";
import { sendsOnEnter } from "./keys";
import { AnswerNode, type AnswerOutcome } from "./answer";
import type { ClaimEvidence, EvidenceContent, ExecutionStatus, ReplySuggestion, ReplySuggestions, RuntimeEvent } from "../runtime/types";
import { moodOf, nodeResult, voiceOf, type GraphNode } from "../graph/model";
import { address_of, readable_content } from "../graph/ledger";
import {
  LIFECYCLE_STATES,
  clock,
  fullInstant,
  hueOf,
  initialOf,
  isAlarming,
  readable,
  verdictOf,
} from "./format";

/** The one-line reading of an event, or `null` when nothing can be said plainly about it. */
function describe(event: RuntimeEvent): string | null {
  const payload =
    event.payload !== null && typeof event.payload === "object"
      ? (event.payload as Record<string, unknown>)
      : {};
  const node = typeof payload.nodeId === "string" ? payload.nodeId : null;
  const nextState = typeof payload.nextState === "string" ? payload.nextState : null;
  const outcome = typeof payload.outcome === "string" ? payload.outcome : null;
  const mode = typeof payload.mode === "string" ? payload.mode : null;

  switch (event.kind) {
    case "execution_started":
      return mode === null ? "Started this run." : `Started this run in ${mode} mode.`;
    case "execution_form_declared": {
      const count = Array.isArray(payload.nodeIds) ? payload.nodeIds.length : null;
      return count === null
        ? "Declared the run's shape."
        : `Declared ${count} node${count === 1 ? "" : "s"} for this run.`;
    }
    case "node_outcome_recorded": {
      if (node === null) return null;
      const reported = outcome === null ? "an outcome" : readable(outcome);
      return nextState === null
        ? `Recorded ${reported} on ${node}.`
        : `Recorded ${reported} on ${node}, now ${readable(nextState)}.`;
    }
    case "execution_paused":
      return "Held the run. Nothing further is dispatched until it is resumed.";
    case "execution_resumed":
      return "Lifted the hold. The run continues.";
    case "execution_completed":
      return "This run reached its end.";
    case "execution_mode_changed":
      return mode === null ? "Changed the run's mode." : `Changed the run's mode to ${mode}.`;
    case "ghost_node_proposed":
      return node === null ? "Proposed a new node." : `Proposed ${node} as a new node.`;
    // Bookkeeping, said in words. These used to fall through to the raw-payload branch and a wake
    // lease rendered as `{"cursor":22,...}` between two human sentences (screenshot, 2026-08-30).
    // They stay in the thread - the log IS the thread - but in a voice, not a dump.
    case "wake_lease":
      return "Armed a wake doorbell: it will ring when this log moves.";
    case "wake_lease_consumed":
      return "The doorbell rang — whoever armed it is being woken.";
    case "signal_recorded": {
      // The payload carries the signal's SHAPE and never its words - `SignalRecorded` has
      // `kind`/`severity`/`sourceKind` and no `description`, because D-036 keeps free-form content
      // out of event payloads. The words arrive underneath, out of sealed evidence.
      const signalKind = typeof payload.kind === "string" ? payload.kind : null;
      const severity = typeof payload.severity === "string" ? payload.severity : null;
      const from = typeof payload.sourceKind === "string" ? payload.sourceKind : null;
      // `operator_note` is outside the recognized set on purpose, so the Governor records it and
      // never acts on it. To a reader that is not a signal at all - it is someone talking.
      if (signalKind === "operator_note") return from === "user" ? "Said:" : "Left a note:";
      // A persona's birth certificate: the envelope's `to` names who was chartered, and its
      // description is the charter. Unrecognized kind, like the notes - recorded, never steers.
      if (signalKind === "persona_created") return "A new persona joined:";
      if (signalKind === null) return "Recorded a signal against this run.";
      return severity === null
        ? `Raised ${readable(signalKind)}.`
        : `Raised ${readable(signalKind)} (${severity}).`;
    }
    case "sweep_performed":
      return "Checked every stage of the run.";
    case "overdue_exception":
      return node === null ? "Flagged an episode as overdue." : `Flagged ${node} as overdue.`;
    default:
      return null;
  }
}

/** A stage direction is the machine narrating itself; dialogue is exactly the signal family.
 * Mechanical, not textual: describe() already branches on `kind`, and nothing else may decide. */
export function isStageDirection(event: RuntimeEvent): boolean {
  return event.kind !== "signal_recorded";
}

/** Whether this event is the one machine line that answers "why does this run need me" - it
 * must never disappear into a fold. */
function isAlarmingTurn(event: RuntimeEvent): boolean {
  const payload =
    event.payload !== null && typeof event.payload === "object"
      ? (event.payload as Record<string, unknown>)
      : {};
  return typeof payload.nextState === "string" && isAlarming(payload.nextState);
}

export type TurnGroup =
  | { kind: "talk"; event: RuntimeEvent }
  | { kind: "stage"; events: RuntimeEvent[] };

/** Unsent composer drafts, per surface, so a remount cannot eat what a person typed. In this
 * module's memory only — never storage: a draft is not a record, and reload clears it. */
const DRAFTS = new Map<string, string>();

/** Test isolation only: module state outlives a test's render the same way it outlives a
 * remount — the feature in production, pollution in a suite. Sealed content is immutable on a
 * real Runtime, but fixtures reuse evidence ids with different bytes test to test. */
export function resetPanelCaches(): void {
  DRAFTS.clear();
  OPENED_EVIDENCE.clear();
}

/** Opened sealed content, shared by every hook instance and panel. An envelope is immutable
 * once sealed, so one open serves the page's lifetime — this map is what makes both the live
 * tail and a second panel affordable, and it is module-level on purpose: an instance-local
 * cache made the agent window re-fetch everything the app view already opened (round-3).
 *
 * It stores PROMISES, not values: two consumers racing the same envelope on the same render
 * (the hooks and the Said bubble mount together) must share one fetch, not both miss an empty
 * cache. A failed open is evicted so retry stays possible. Round 4 found Said reaching past
 * this map entirely — every eager envelope fetched twice — so the rule is now a function all
 * three consumers go through. */
const OPENED_EVIDENCE = new Map<string, Promise<EvidenceContent>>();

function openSealed(
  executionId: string,
  evidenceId: string,
  open: (executionId: string, evidenceId: string) => Promise<EvidenceContent>,
): Promise<EvidenceContent> {
  const key = `${executionId}/${evidenceId}`;
  const held = OPENED_EVIDENCE.get(key);
  if (held !== undefined) return held;
  const fetched = open(executionId, evidenceId);
  OPENED_EVIDENCE.set(key, fetched);
  fetched.catch(() => OPENED_EVIDENCE.delete(key));
  return fetched;
}

/**
 * The dialogue and the stage directions.
 *
 * The log IS the thread, and machine narration interleaved at equal weight made human words the
 * minority in their own conversation (four "Recorded … on start" lines and two doorbells between
 * two sentences, on the owner's real screen). Consecutive stage directions fold into one strip a
 * reader can open; nothing is hidden or paraphrased - only folded.
 *
 * Three lines never fold: a spoken one, an alarming transition (it answers "why does this run
 * need me"), and the TRAILING machine events - they are what is happening now. A pure pre-pass
 * over the events, because the scroll pinning underneath has two regressions on record and must
 * never learn folding exists.
 */
export function groupTurns(events: RuntimeEvent[]): TurnGroup[] {
  // The trailing run of stage directions stays loose, whatever its length.
  let tail = events.length;
  while (tail > 0 && isStageDirection(events[tail - 1])) tail -= 1;

  const groups: TurnGroup[] = [];
  let strip: RuntimeEvent[] = [];
  const flush = () => {
    if (strip.length >= 2) groups.push({ kind: "stage", events: strip });
    else for (const lone of strip) groups.push({ kind: "talk", event: lone });
    strip = [];
  };
  events.forEach((event, index) => {
    if (index < tail && isStageDirection(event) && !isAlarmingTurn(event)) {
      strip.push(event);
      return;
    }
    flush();
    groups.push({ kind: "talk", event });
  });
  flush();
  return groups;
}

/** How many of a page's sealed items this opens without being asked.
 *
 * The words are the point - a thread that says "recorded a signal" and makes you click to find out
 * what was said is a log, not a conversation - so the default is to open them. It is bounded
 * anyway: each open is one small GET to a Runtime that is local by construction (D-040), and the
 * event page itself is capped well below this. Anything past the cap gets a button rather than
 * being dropped, because a thread that quietly stops opening things reads exactly like a thread
 * where nothing was said. */
const AUTO_OPEN_LIMIT = 40;

/** The readable part of a sealed item, or the item itself when it has no readable part.
 *
 * WHAT IS SEALED IS THE WHOLE ENVELOPE, not the sentence inside it. A signal seals its entire JSON
 * document — id, source, severity, `emittedAt` and all — so rendering the sealed bytes verbatim
 * puts a wall of JSON where somebody's sentence should be. That is what this surface looked like
 * the first time it was run against a real Runtime, and both tests were green: the fixture returned
 * plain text, and the end-to-end assertion was `contains`, which a JSON document carrying the
 * sentence satisfies perfectly.
 *
 * `readable_content` and `address_of` moved to `graph/ledger.ts` when the WebMCP attention tool
 * started reading envelopes too - one parsing oracle, two surfaces, and they cannot drift. */

/** Whether the reader is close enough to the newest message that the thread should follow it.
 *
 * Pure so it can be tested where jsdom cannot: jsdom lays nothing out, every height is zero, and
 * an "is it scrolled" assertion there measures the test double, not the behavior. The 80px slack
 * is the difference between "reading the latest" and "deliberately scrolled up into history" -
 * a thread that yanks someone out of history to show a new message loses them the place they
 * chose.
 */
export function pinnedToLatest(scrollHeight: number, scrollTop: number, clientHeight: number): boolean {
  return scrollHeight - scrollTop - clientHeight < 80;
}

/** The sealed words behind one evidence reference.
 *
 * Failure is rendered, never swallowed. An envelope that cannot be opened is a real state with real
 * causes - the Runtime holds no key for it, the store refused it, the reference outlived its
 * content - and every one of them matters more to the person reading than an empty line would.
 */
/**
 * The sealed words, opened into READING. Agents write plain text with the shapes people write:
 * blank-line paragraphs, dash or numbered lists, backticked identifiers. This renders exactly
 * those three shapes and nothing else - built as React nodes, so a message can never smuggle
 * markup, and a text with none of the shapes falls through as the single paragraph it is.
 */
function inlineOf(text: string): React.ReactNode[] {
  const parts = text.split(/`([^`\n]{1,120})`/g);
  return parts.map((part, index) =>
    index % 2 === 1 ? <code key={index}>{part}</code> : part,
  );
}

const LIST_MARK = /^\s*(?:[-*]|\(\d{1,3}\)|\d{1,3}[.)])\s+/;

/* An inline enumeration mid-sentence: "... nesta ordem: (1) isto; (2) aquilo". Two or more of
 * these in one paragraph and the paragraph is a list that never got its line breaks. */
const INLINE_ENUM = /\s*\((\d{1,2})\)\s+/g;

/** Agents write without blank lines, so a paragraph has to be FOUND, not just split: a long
 * unbroken run is chunked at sentence ends into readable lengths. Presentation only - every
 * character of the message survives, in order. */
function sentencesOf(text: string): string[] {
  const parts = text.split(/(?<=[.!?:])\s+(?=[A-Z\u00c0-\u00dc(\u2018\u201c"'\`\d])/);
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
    const first = enumMatches[0];
    const lead = block.slice(0, first.index).trim();
    const items: string[] = [];
    for (let index = 0; index < enumMatches.length; index += 1) {
      const from = enumMatches[index].index! + enumMatches[index][0].length;
      const to = index + 1 < enumMatches.length ? enumMatches[index + 1].index! : block.length;
      items.push(block.slice(from, to).trim());
    }
    return [
      ...(lead.length > 0 ? [<p key={keyBase + "-lead"}>{inlineOf(lead)}</p>] : []),
      <ol key={keyBase + "-enum"}>
        {items.map((item, itemIndex) => (
          <li key={itemIndex}>{inlineOf(item)}</li>
        ))}
      </ol>,
    ];
  }
  return sentencesOf(block).map((chunk, chunkIndex) => (
    <p key={keyBase + "-" + chunkIndex}>{inlineOf(chunk)}</p>
  ));
}

function formatSaid(text: string): React.ReactNode {
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
          {lead.length > 0 && <p>{inlineOf(lead.join("\n"))}</p>}
          <ul>
            {items.map((item, itemIndex) => (
              <li key={itemIndex}>{inlineOf(item)}</li>
            ))}
          </ul>
        </React.Fragment>,
      ];
    }
    return paragraphsOf(block, String(blockIndex));
  });
}

function Said({
  executionId,
  evidenceId,
  open,
  eager,
}: {
  executionId: string;
  evidenceId: string;
  open: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
  eager: boolean;
}) {
  const [state, setState] = useState<
    | { at: "idle" }
    | { at: "loading" }
    | { at: "open"; text: string; to: string | null; replyTo: string | null }
    | { at: "shut"; why: string }
  >(eager ? { at: "loading" } : { at: "idle" });

  useEffect(() => {
    if (!eager) return;
    let live = true;
    setState({ at: "loading" });
    openSealed(executionId, evidenceId, open).then(
      (content) => {
        if (live) {
          setState({
            at: "open",
            text: readable_content(content.content, content.mediaType),
            ...address_of(content.content, content.mediaType),
          });
        }
      },
      (error: unknown) => {
        if (live) setState({ at: "shut", why: error instanceof Error ? error.message : "unreadable" });
      },
    );
    return () => {
      live = false;
    };
  }, [executionId, evidenceId, eager, open]);

  const fetchNow = () => {
    setState({ at: "loading" });
    openSealed(executionId, evidenceId, open).then(
      (content) =>
        setState({
          at: "open",
          text: readable_content(content.content, content.mediaType),
          ...address_of(content.content, content.mediaType),
        }),
      (error: unknown) =>
        setState({ at: "shut", why: error instanceof Error ? error.message : "unreadable" }),
    );
  };
  if (state.at === "idle") {
    const label = evidenceId.endsWith("-context-provenance") ? "show context sources"
      : evidenceId.endsWith("-accounting-receipt") ? "show token accounting"
      : evidenceId.endsWith("-record") ? "show tool record"
      : evidenceId.endsWith("-stderr") ? "show error output"
      : "show what was said";
    return (
      <button type="button" className="show-more" onClick={fetchNow}>
        {label}
      </button>
    );
  }
  if (state.at === "loading") return <p className="turn-said loading">opening…</p>;
  if (state.at === "shut") {
    // An alert with a way back in, not a dead end: the words exist, the failure was one call,
    // and the only retry used to be remounting the whole panel.
    return (
      <p className="turn-said shut" role="alert">
        Sealed, and this Runtime cannot open it: {state.why}{" "}
        <button type="button" className="show-more" onClick={fetchNow}>
          try again
        </button>
      </p>
    );
  }
  return (
    <>
      {(state.to !== null || state.replyTo !== null) && (
        <p className="turn-addr">
          {state.to !== null ? `→ ${state.to}` : ""}
          {state.to !== null && state.replyTo !== null ? " · " : ""}
          {state.replyTo !== null ? `answers ${state.replyTo}` : ""}
        </p>
      )}
      {/* A record that OPENED and holds nothing readable is the third elision, and the one the
        * live screen actually had: two operator turns rendered as blank bubbles, ambiguous
        * between "the log holds nothing" and "the UI swallowed it". Marked, like the others. */}
      {state.text.trim().length === 0 ? (
        <p className="turn-said shut">opened — the sealed record is empty</p>
      ) : (
        <div className="turn-said">{formatSaid(state.text)}</div>
      )}
    </>
  );
}

function Thread({
  events,
  executionId,
  openEvidence,
}: {
  events: RuntimeEvent[];
  executionId?: string;
  openEvidence?: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
}) {
  // THE CHAT OPENS AT THE END, because a conversation is read from where it is happening. The
  // first version opened at the top and the person had to scroll past the whole history to find
  // the message that made the panel worth opening. It stays pinned to the newest message unless
  // the reader deliberately scrolled up into history (`pinnedToLatest`), and re-pins when they
  // come back down.
  const listRef = useRef<HTMLOListElement | null>(null);
  const pinned = useRef(true);
  // Only a scroll the READER made may unpin. The browser fires scroll events of its own while
  // the turns load - scroll anchoring nudges scrollTop as content grows above it - and reading
  // those as intent unpinned the thread mid-load: the person opened a chat and landed mid-way
  // (reported twice). A wheel, touch or key marks the next scroll events as human for a beat.
  const humanScroll = useRef(false);
  const humanUntil = useRef(0);
  useEffect(() => {
    const list = listRef.current;
    if (list === null) return;
    if (pinned.current) list.scrollTop = list.scrollHeight;
    // The turns keep growing after this first scroll - evidence opens asynchronously and each
    // bubble adds height - so pin to the end on every size change, not only on new events.
    // jsdom has no ResizeObserver; the first scroll above is the whole behaviour there.
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (pinned.current) list.scrollTop = list.scrollHeight;
    });
    observer.observe(list);
    for (const child of list.children) observer.observe(child);
    return () => observer.disconnect();
  }, [events]);

  if (events.length === 0) {
    return <p className="panel-empty">Nothing has been said about this yet.</p>;
  }
  // Newest first is what gets opened, because that is what someone watching is here to read.
  const eager = new Set(
    events
      .filter((event) => event.evidenceRefs.length > 0)
      .slice(-AUTO_OPEN_LIMIT)
      .map((event) => event.sequence),
  );
  return (
    <ol
      className="thread"
      ref={listRef}
      onScroll={(scrolled) => {
        if (!humanScroll.current && Date.now() > humanUntil.current) return;
        humanScroll.current = false;
        humanUntil.current = Date.now() + 150;
        const list = scrolled.currentTarget;
        pinned.current = pinnedToLatest(list.scrollHeight, list.scrollTop, list.clientHeight);
      }}
      onWheel={() => {
        humanScroll.current = true;
      }}
      onTouchMove={() => {
        humanScroll.current = true;
      }}
      onKeyDown={() => {
        humanScroll.current = true;
      }}
    >
      {(() => {
        // A continuation only follows another SPOKEN turn by the same actor - a strip between
        // two turns re-earns the header.
        let previousTalk: RuntimeEvent | null = null;
        return groupTurns(events).map((group) => {
          if (group.kind === "stage") {
            previousTalk = null;
            const first = group.events[0];
            const last = group.events[group.events.length - 1];
            return (
              <li className="turn system stage" key={`stage-${first.sequence}`}>
                {/* The log IS the thread: nothing here is dropped, only folded. The lines
                  * inside are the exact turns they would have been, one click away. */}
                <details>
                  <summary>
                    {group.events.length} lifecycle events ·{" "}
                    <time dateTime={first.occurredAt ?? undefined}>{clock(first.occurredAt)}</time>
                    {"–"}
                    <time dateTime={last.occurredAt ?? undefined}>{clock(last.occurredAt)}</time>
                  </summary>
                  <ol className="stage-lines">
                    {group.events.map((entry) => {
                      const said = describe(entry);
                      return (
                        <li key={entry.eventId ?? entry.sequence}>
                          <p className="turn-who">
                            {entry.actorId ?? "system"} · {clock(entry.occurredAt)}
                          </p>
                          {said === null ? (
                            <>
                              <p className="turn-text">{readable(entry.kind)}</p>
                              <code>{JSON.stringify(entry.payload)}</code>
                            </>
                          ) : (
                            <p className="turn-text">{said}</p>
                          )}
                        </li>
                      );
                    })}
                  </ol>
                </details>
              </li>
            );
          }
          const event = group.event;
          const voice = voiceOf(event.actorType);
          const line = describe(event);
          const who = event.actorId ?? voice;
          const payload =
            event.payload !== null && typeof event.payload === "object"
              ? (event.payload as Record<string, unknown>)
              : {};
          // A spoken error wears the danger surface: chat and lifecycle share one vocabulary.
          const grave = payload.severity === "error" || payload.severity === "critical";
          // A LONE stage direction between speech cannot fold (nothing to fold with), so it
          // murmurs instead: present, in the log, at reduced weight. Never an alarming one.
          const murmur = isStageDirection(event) && !isAlarmingTurn(event);
          // A continuation: same speaker as the previous turn, so the header would only repeat
          // itself. The avatar column is held by a spacer so the words stay aligned.
          const previous = previousTalk;
          previousTalk = event;
          const grouped =
            previous !== null &&
            voice !== "system" &&
            voiceOf(previous.actorType) === voice &&
            (previous.actorId ?? voiceOf(previous.actorType)) === who;
          return (
            <li
              className={`turn ${voice} ${grouped ? "cont" : ""} ${grave ? "grave" : ""} ${murmur ? "murmur" : ""}`}
              key={event.eventId ?? event.sequence}
            >
              {/* One colour per actor, everywhere, derived from the id - the messenger identity
                * the owner pointed at. System narration goes bare: lifecycle is not a contact. */}
              {voice !== "system" &&
                (grouped ? (
                  <span className="avatar-hold" aria-hidden="true" />
                ) : (
                  <span
                    className="avatar"
                    aria-hidden="true"
                    style={{ background: `hsl(${hueOf(who)} 52% 46%)` }}
                  >
                    {initialOf(who)}
                  </span>
                ))}
              <div className="turn-body">
              {!grouped && (
              <p className="turn-who">
                {voice === "system" ? (
                  who
                ) : (
                  <span className="turn-name" style={{ color: `hsl(${hueOf(who)} 60% 70%)` }}>
                    {who}
                  </span>
                )}{" "}
                ·{" "}
                <time dateTime={event.occurredAt ?? undefined} title={fullInstant(event.occurredAt)}>
                  {clock(event.occurredAt)}
                </time>{" "}
                {/* The log's own address for this line, click-to-copy. An append-only log's
                  * best property is immutable positions; this makes it a thing a person can
                  * paste to another person - or to an agent. */}
                {executionId !== undefined && (
                  <button
                    type="button"
                    className="turn-seq"
                    title={`Copy ${executionId}#${event.sequence}`}
                    onClick={() =>
                      void navigator.clipboard?.writeText(`${executionId}#${event.sequence}`)
                    }
                  >
                    #{event.sequence}
                  </button>
                )}
              </p>
              )}
              {line === null ? (
                <>
                  <p className="turn-text">{readable(event.kind)}</p>
                  <code>{JSON.stringify(event.payload)}</code>
                </>
              ) : (
                <p className="turn-text">{line}</p>
              )}
              {/* A line that promises words ("Said:") and shows none is ambiguous between "the
                * log holds nothing" and "the UI swallowed it". Elision is marked as elision. */}
              {line !== null && line.endsWith(":") && event.evidenceRefs.length === 0 && (
                <p className="turn-said shut">no words attached to this record</p>
              )}
              {line !== null &&
                line.endsWith(":") &&
                event.evidenceRefs.length > 0 &&
                (executionId === undefined || openEvidence === undefined) && (
                  <p className="turn-said shut">sealed — cannot be opened from this view</p>
                )}
              {executionId !== undefined &&
                openEvidence !== undefined &&
                event.evidenceRefs.map((evidenceId) => (
                  <Said
                    key={evidenceId}
                    executionId={executionId}
                    evidenceId={evidenceId}
                    open={openEvidence}
                    eager={eager.has(event.sequence) && (event.kind !== "node_outcome_recorded"
                      || evidenceId.endsWith("-reply") || evidenceId.endsWith("-stdout"))}
                  />
                ))}
              </div>
            </li>
          );
        });
      })()}
    </ol>
  );
}

/**
 * Whether the person's last message is still unanswered.
 *
 * The comparison is between the newest OWNER message and the newest message from anyone else:
 * strictly by sequence, because the log is the clock here. It deliberately reads only
 * `signal_recorded` events - lifecycle events after your message are the run moving, not anyone
 * answering you.
 */
export function awaitingReply(
  events: RuntimeEvent[],
  envelopes: Record<number, { to: string | null; replyTo: string | null; text?: string | null }>,
): boolean {
  let lastOwnerSeq = -1;
  let lastOwnerSignalId: string | null = null;
  for (const event of events) {
    if (event.kind !== "signal_recorded") continue;
    if (event.actorType !== "owner" || event.sequence <= lastOwnerSeq) continue;
    lastOwnerSeq = event.sequence;
    const payload =
      event.payload !== null && typeof event.payload === "object"
        ? (event.payload as { signalId?: unknown })
        : {};
    lastOwnerSignalId = typeof payload.signalId === "string" ? payload.signalId : null;
  }
  if (lastOwnerSeq === -1) return false;
  for (const event of events) {
    if (event.kind !== "signal_recorded") continue;
    if (event.actorType === "owner" || event.sequence <= lastOwnerSeq) continue;
    const envelope = envelopes[event.sequence];
    // No envelope: the words are sealed shut to this view, and "still waiting" would be an
    // absence asserted through an erasing filter. The receipt stands down.
    if (envelope === undefined) return false;
    // Settled only by an answer the operator can actually READ AS THEIRS: addressed to them,
    // spoken to the whole room (which includes them), or replying to their own signal. What
    // never settles is two agents talking in their own addressed pair - that was clearing the
    // receipt while the operator's question sat unanswered (three round-3 reviewers, blind).
    if (envelope.to === null || envelope.to === OPERATOR_ACTOR.id) return false;
    if (envelope.replyTo !== null && envelope.replyTo === lastOwnerSignalId) return false;
  }
  return true;
}

/**
 * The room's roster, derived from the log itself.
 *
 * A persona exists because a `persona_created` signal chartered it - the envelope's `to` is the
 * persona's id and its description the charter. Reading the roster out of those signals means
 * nobody maintains a second list that could drift from the journal; kill the page and reopen it
 * and the same personas stand, because the log is the registry.
 */
export function usePersonas(
  events: RuntimeEvent[],
  executionId: string | undefined,
  openEvidence: ((executionId: string, evidenceId: string) => Promise<EvidenceContent>) | undefined,
): Record<string, string> {
  const [personas, setPersonas] = useState<Record<string, string>>({});
  useEffect(() => {
    if (executionId === undefined || openEvidence === undefined) return;
    const births = events.filter((event) => {
      const payload =
        event.payload !== null && typeof event.payload === "object"
          ? (event.payload as Record<string, unknown>)
          : {};
      return (
        event.kind === "signal_recorded" &&
        payload.kind === "persona_created" &&
        event.evidenceRefs.length > 0
      );
    });
    let live = true;
    void (async () => {
      const found: Record<string, string> = {};
      for (const birth of births) {
        try {
          const content = await openSealed(executionId, birth.evidenceRefs[0], openEvidence);
          const envelope = JSON.parse(content.content) as { to?: unknown; description?: unknown };
          if (typeof envelope.to === "string" && envelope.to.length > 0) {
            // Two immune checks the persona-host also enforces, repeated here because this
            // roster renders whatever the LOG holds, including births an older host let
            // through: the operator is the person AT the screen, never a blob on it - and
            // the FIRST charter wins, so a later persona_created cannot silently re-charter
            // an existing persona with new words under the same name (round-4, biology).
            if (envelope.to === OPERATOR_ACTOR.id) continue;
            if (found[envelope.to] !== undefined) continue;
            found[envelope.to] =
              typeof envelope.description === "string" ? envelope.description : "";
          }
        } catch {
          // An unopenable charter costs one chip, never the roster.
        }
      }
      // Same commit-only-change rule as useEnvelopes below, same measured loop.
      if (live) {
        setPersonas((previous) => {
          const nextKeys = Object.keys(found);
          const same =
            Object.keys(previous).length === nextKeys.length &&
            nextKeys.every((key) => previous[key] === found[key]);
          return same ? previous : found;
        });
      }
    })();
    return () => {
      live = false;
    };
  }, [events, executionId, openEvidence]);
  return personas;
}

/** The envelope behind each signal event, opened once and shared: the agent panel filters by
 * `to`, the thread shows addresses, and neither should fetch what the other already has. */
export function useEnvelopes(
  events: RuntimeEvent[],
  executionId: string | undefined,
  openEvidence: ((executionId: string, evidenceId: string) => Promise<EvidenceContent>) | undefined,
): Record<number, { to: string | null; replyTo: string | null; text: string }> {
  // TAGGED BY EXECUTION, not just keyed by sequence: sequences restart in every run, so run B's
  // signal at sequence 5 would wear run A's plaintext envelope from sequence 5 for as long as
  // B's evidence reads take - someone else's words, recipient and replyTo rendered under the
  // wrong run (PR #467 review). Until the map belongs to the CURRENT run, the hook answers
  // "nothing opened yet", which is true.
  const [envelopes, setEnvelopes] = useState<{
    forId: string | undefined;
    map: Record<number, { to: string | null; replyTo: string | null; text: string }>;
  }>({ forId: undefined, map: {} });
  // The shared OPENED_EVIDENCE cache is what lets the live tail exist: without it, every pass
  // re-fetched ~20 envelopes, and a pass slower than the poll interval was cancelled before it
  // could commit — bubbles, ledger and agent filters permanently blind (found live, round 2).
  useEffect(() => {
    if (executionId === undefined || openEvidence === undefined) return;
    const carriers = events.filter(
      (event) => event.kind === "signal_recorded" && event.evidenceRefs.length > 0,
    ).reverse();
    let live = true;
    void (async () => {
      for (const carrier of carriers) {
        try {
          const content = await openSealed(executionId, carrier.evidenceRefs[0], openEvidence);
          if (!live) return;
          const opened = {
            ...address_of(content.content, content.mediaType),
            // The words too: the attention block quotes the pending question out of the same
            // fetch the addressing already made.
            text: readable_content(content.content, content.mediaType),
          };
          setEnvelopes((previous) => {
            const before = previous.forId === executionId ? previous.map[carrier.sequence] : undefined;
            if (before?.to === opened.to && before.replyTo === opened.replyTo && before.text === opened.text) return previous;
            return {
              forId: executionId,
              map: { ...(previous.forId === executionId ? previous.map : {}), [carrier.sequence]: opened },
            };
          });
        } catch {
          // An unopenable envelope filters as unaddressed, never as an error.
        }
      }
    })();
    return () => {
      live = false;
    };
  }, [events, executionId, openEvidence]);
  // A stable empty answer, not a fresh literal: downstream memos key off this identity.
  return envelopes.forId === executionId ? envelopes.map : NO_ENVELOPES;
}

const NO_ENVELOPES: Record<number, { to: string | null; replyTo: string | null; text: string }> = {};

/** Actors the thread has actually heard from, for the roster's plain half. System actors stay
 * out - the room lists who CONVERSES, and `system-runtime` narrating lifecycle is not that. */
export function actorsInRoom(events: RuntimeEvent[], personas: Record<string, string>): string[] {
  const seen = new Set<string>();
  for (const event of events) {
    if (event.actorId === null) continue;
    if (event.actorType !== "agent" && event.actorType !== "owner") continue;
    if (personas[event.actorId] !== undefined) continue;
    seen.add(event.actorId);
  }
  return [...seen].sort();
}

/**
 * The way back in: say something into a run that is already going.
 *
 * This is the operator half of an asynchronous exchange. The agent leaves records as it works and
 * this is how a person answers them, in the same log, on the same terms - a message here is an
 * event with sealed words, exactly like the ones it is replying to.
 *
 * It does NOT steer the run. The envelope goes in as an unrecognized signal kind, which the
 * Governor records and never acts on. Saying so on the surface matters: a box that looks like a
 * command line and behaves like a comment thread would be read as the former.
 */
function SayBox({
  busy,
  error,
  onSay,
  suggestions = [],
  draftSuggestions = [],
  suggestionsClaim = "Talk to someone",
  onPickSuggestion,
  onPickDraft,
  focusNonce = 0,
  recipient = null,
  recipientLocked = false,
  onClearRecipient,
  suggestionsVerb = "answer",
  draftId,
}: {
  busy: boolean;
  error: string;
  onSay: (message: string, to: string | null) => void;
  /** Who to offer answering, newest first. Cards above the box; picking one addresses it. */
  suggestions?: Array<{ id: string; at: string | null }>;
  /** Jev-ranked draft text for this exact run head, if available. */
  draftSuggestions?: ReplySuggestion[];
  /** What the cards CLAIM about the people on them. "Who is waiting for an answer" is a claim
   * only the unanswered-question ledger may back; mere recent speakers get an honest label. */
  suggestionsClaim?: string;
  /** The verb on each card. "answer" asserts a debt — only the ledger claim may carry it. */
  suggestionsVerb?: string;
  onPickSuggestion?: (agentId: string) => void;
  onPickDraft?: (recipient: string | null) => void;
  /** Bumped by the attention area's "answer in the thread" action. A nonce, not a boolean, so a
   * second click focuses again even though the panel never remounted. */
  focusNonce?: number;
  /** The persona or agent the next message is addressed to. `null` speaks to the room, which is
   * what the placeholder already says. */
  recipient?: string | null;
  /** True in an agent's own window: the address IS the window, so there is nothing to clear. */
  recipientLocked?: boolean;
  onClearRecipient?: () => void;
  /** Where this box's unsent words survive a remount. A WebMCP agent selecting another run — or
   * the person closing the panel — unmounted the composer and silently discarded a half-typed
   * answer (round-3). In-memory only, per surface, gone on reload: a draft is not a record. */
  draftId?: string;
}) {
  const [message, setMessageState] = useState(
    () => (draftId !== undefined ? DRAFTS.get(draftId) : undefined) ?? "",
  );
  // THE STATE FOLLOWS THE SURFACE, not the mount. The draftId carries the execution, but a
  // useState initializer runs once - and the run panel keeps its tree position across a run
  // switch, so run A's half-typed message sat in the box with run B selected, one Enter away
  // from landing in the wrong append-only log (PR #467 review, P1). When the surface changes,
  // the box swaps to THAT surface's own unsent words (or empty), never carrying text across.
  const boxOwner = useRef(draftId);
  useEffect(() => {
    if (boxOwner.current === draftId) return;
    boxOwner.current = draftId;
    setMessageState((draftId !== undefined ? DRAFTS.get(draftId) : undefined) ?? "");
  }, [draftId]);
  const setMessage = (value: string) => {
    setMessageState(value);
    if (draftId !== undefined) {
      if (value === "") DRAFTS.delete(draftId);
      else DRAFTS.set(draftId, value);
    }
  };
  const boxRef = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => {
    if (focusNonce > 0) boxRef.current?.focus();
  }, [focusNonce]);
  useEffect(() => {
    // Picking someone to talk to IS the intent to talk: the cursor follows the choice.
    if (recipient !== null) boxRef.current?.focus();
  }, [recipient]);
  // Delivered — and only delivered — clears the box. `busy` is per surface, so the transition
  // this watches belongs to this box's own send. The surface-change effect above also resets
  // this ref: a busy-to-idle edge observed ACROSS a run switch is another run's completion
  // arriving under this box, and reading it as delivery deleted the new run's half-written
  // draft (PR #467 review, P1). Only a send that began on THIS surface may clear it.
  const wasBusy = useRef(false);
  // Its own ref rather than reading `boxOwner`: that effect is declared earlier, so it has
  // already caught up with the new surface by the time this one runs, and the edge would
  // look same-surface again.
  const busySurface = useRef(draftId);
  useEffect(() => {
    if (busySurface.current !== draftId) {
      busySurface.current = draftId;
      wasBusy.current = busy;
      return;
    }
    if (wasBusy.current && !busy && error === "") setMessage("");
    wasBusy.current = busy;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [busy, error, draftId]);
  const send = () => {
    if (message.trim().length === 0 || busy) return;
    // The words are NOT cleared here: a refused or failed send must hand them back, not
    // show "could not be sent" beside an emptied box. The effect above clears on the
    // busy→idle transition only when no error stood.
    onSay(message, recipient);
  };
  return (
    <form
      className="saybox"
      onSubmit={(event) => {
        event.preventDefault();
        send();
      }}
    >
      {draftSuggestions.length > 0 && (
        <div className="reply-drafts" role="group" aria-label="Recommended replies">
          {draftSuggestions.map((suggestion, index) => (
            <button
              key={`${index}:${suggestion.draft}`}
              type="button"
              className="reply-draft"
              onClick={() => {
                setMessage(suggestion.draft);
                onPickDraft?.(suggestion.to);
                boxRef.current?.focus();
              }}
            >
              <strong>Option {index + 1}</strong>
              <span>{suggestion.draft}</span>
              <small>{suggestion.reason}</small>
              {suggestion.sourceSequences.length > 0 && (
                <small>Based on event {suggestion.sourceSequences.map((sequence) => `#${sequence}`).join(", ")}</small>
              )}
            </button>
          ))}
          <p>Choosing fills the box. You can edit it before sending.</p>
        </div>
      )}
      {draftSuggestions.length === 0 && suggestions.length > 0 && recipient === null && (
        <div className="reply-hints" role="group" aria-label={suggestionsClaim}>
          {suggestions.map((suggestion) => (
            <button
              key={suggestion.id}
              type="button"
              className="reply-hint"
              onClick={() => onPickSuggestion?.(suggestion.id)}
            >
              <span
                className="avatar mini"
                aria-hidden="true"
                style={{ background: `hsl(${hueOf(suggestion.id)} 52% 46%)` }}
              >
                {initialOf(suggestion.id)}
              </span>
              <span className="reply-hint-who">{suggestionsVerb} {suggestion.id}</span>
              <span className="reply-hint-when">{clock(suggestion.at)}</span>
            </button>
          ))}
        </div>
      )}
      {recipient !== null && (
        <p className="to-chip">
          → {recipient}
          {!recipientLocked && (
            <button type="button" onClick={() => onClearRecipient?.()} aria-label="Speak to the room instead">
              ×
            </button>
          )}
        </p>
      )}
      <div className="say-capsule">
        <textarea
          ref={boxRef}
          aria-label="Say something into this run"
          value={message}
          rows={2}
          maxLength={MAX_MESSAGE_LENGTH}
          disabled={busy}
          placeholder="Say something to whoever is working on this…"
          onChange={(event) => setMessage(event.target.value)}
          // #1083 F5: Enter sends here too, exactly as in the new-task composer - one predicate
          // for both boxes (keys.ts). Shift+Enter breaks the line; an IME's commit never sends.
          onKeyDown={(event) => {
            if (!sendsOnEnter(event)) return;
            event.preventDefault();
            send();
          }}
        />
        <button
          type="submit"
          className="send"
          aria-label="send"
          disabled={busy || message.trim().length === 0}
        >
          {busy ? <LoaderCircle className="spin" aria-hidden="true" /> : <Send aria-hidden="true" />}
        </button>
      </div>
      {/* #1098 D3: the same sentence the composer prints, because this box has had the same key
        * contract since #1091 and said nothing about it. Two boxes, one page, one rule — the hint
        * is the only place that rule is visible without pressing the key and finding out. */}
      <p className="lbl composer-hint">Enter sends · Shift+Enter for a new line</p>
      {error !== "" && (
        <p className="notice bad" role="alert">
          <TriangleAlert aria-hidden="true" />
          <span>{error}</span>
        </p>
      )}
      <p className="hint">Agents read this as a message, not a command.</p>
    </form>
  );
}

export function NodePanel({
  node,
  events,
  onClose,
  executionId,
  openEvidence,
  onOpenDocument,
  answer,
}: {
  node: GraphNode;
  events: RuntimeEvent[];
  onClose: () => void;
  executionId?: string;
  openEvidence?: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
  onOpenDocument?: (document: DocumentReference) => void;
  /** How this panel answers a node that is waiting for a person (#1186). Optional, and its
   * absence is not a claim that the node is not waiting: a panel opened without a connected
   * Runtime, or without the graph file the claim has to carry, has no honest way to answer and
   * renders nothing rather than a button that cannot work. */
  answer?: {
    waitSeq: number | null;
    onAnswer: (evidence: ClaimEvidence[]) => Promise<AnswerOutcome>;
    hash: (file: File) => Promise<{ contentHash: string; size: number }>;
  };
}) {
  const mood = moodOf(node.state);
  const result = nodeResult(node);
  // A dispatch records Started twice (ready -> queued -> running); only the running
  // transition is one actual attempt. Counting outcomes doubled every attempt on real runs.
  const attempts = node.history.filter((entry) => entry.outcome === "started" && entry.nextState === "running").length;
  const deliveryView = Boolean(executionId && openEvidence && onOpenDocument);
  const historyEvents = deliveryView ? events.filter((event) => {
    const payload = event.payload as Record<string, unknown> | null;
    return !(event.kind === "signal_recorded" && payload?.kind === "node_delivery" &&
      payload.sourceKind === "node" && payload.sourceId === node.id);
  }) : events;
  return (
    <section className="panel" aria-label={`Node ${node.id}`}>
      <header className={`panel-head ${mood}${node.resultSource === "model_reply" && node.state === "succeeded" ? " verification-unchecked" : ""}`}>
        <i aria-hidden="true" />
        <div style={{ minWidth: 0 }}>
          <h2>{node.declaredName ?? node.id}</h2>
          {node.declaredName && <p className="lbl">Node · {node.id}</p>}
          {node.declaredRole && <p className="lbl">Declared role · {node.declaredRole}</p>}
          <p className="lbl">{node.resultSource === "model_reply" && node.state === "succeeded" ? "Reply received · review needed" : readable(node.state)}</p>
        </div>
        <button type="button" className="ghost close" onClick={onClose} aria-label="Close this node">
          <X aria-hidden="true" />
        </button>
      </header>

      <div className="facts">
        <div>
          <p className="lbl">Events</p>
          <strong>{node.touches}</strong>
        </div>
        <div>
          <p className="lbl">Attempts</p>
          <strong>{attempts}</strong>
        </div>
        <div>
          <p className="lbl">Last</p>
          <strong>{clock(node.lastEventAt)}</strong>
        </div>
      </div>
      {result && (
        <section className="node-result-summary" aria-label="Node result">
          <strong>{result.verification}</strong>
          <span>Executed by {result.executor}</span>
          <span>Lifecycle recorded by {node.history.at(-1)?.actorType === "system" ? "Runtime" : node.history.at(-1)?.actorId ?? "an unknown actor"}.</span>
          <span>Read the sealed reply or tool record below for what this step actually reported.</span>
        </section>
      )}
      {!result && node.actualExecutor && (
        <section className="node-result-summary" aria-label="Last attempt">
          <strong>Last attempt · {readable(node.state)}</strong>
          <span>Executed by {node.actualExecutor.kind === "model" ? `Model · route ${node.actualExecutor.routeId ?? "not recorded"}` : node.actualExecutor.kind}</span>
          <span>Lifecycle recorded by {node.history.at(-1)?.actorType === "system" ? "Runtime" : node.history.at(-1)?.actorId ?? "an unknown actor"}.</span>
        </section>
      )}

      {/* THE STATE DECIDES, not the presence of the prop: a node that is not parked must not be
        * offered an answer, because a claim against it is refused (`not_waiting`) and the button
        * would exist only to produce that refusal. */}
      {node.state === "waiting_input" && answer !== undefined && (
        <AnswerNode
          node={node.id}
          waitSeq={answer.waitSeq}
          onAnswer={answer.onAnswer}
          hash={answer.hash}
        />
      )}

      {executionId && openEvidence && onOpenDocument && <NodeDeliveries nodeId={node.id} executionId={executionId} events={events} openEvidence={openEvidence} onOpenDocument={onOpenDocument} />}
      {(!deliveryView || historyEvents.length > 0) && <Thread events={historyEvents} executionId={executionId} openEvidence={openEvidence} />}

    </section>
  );
}

/**
 * The individual conversation with one agent or persona - the window that opens when a member of
 * the crew card is clicked on the canvas.
 *
 * The thread is FILTERED, not restyled: what this agent said, and what was addressed to it (by
 * envelope `to`, never by guessing from prose). The say box is locked to the agent - a window
 * named after someone that quietly posts to the room would put words where nobody sent them.
 */
export function AgentPanel({
  agentId,
  charter = null,
  events,
  executionId,
  openEvidence,
  onClose,
  onSay,
  saying = false,
  sayError = "",
}: {
  agentId: string;
  charter?: string | null;
  events: RuntimeEvent[];
  executionId?: string;
  openEvidence?: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
  onClose: () => void;
  onSay?: (message: string, to: string | null) => void;
  saying?: boolean;
  sayError?: string;
}) {
  const envelopes = useEnvelopes(events, executionId, openEvidence);
  // THE DIRECT LINE, and only that. This window used to show everything the agent said anywhere,
  // which meant opening a bot showed its group traffic - and the person could not tell what was
  // said TO THEM from what was said across the room. Group exchanges live in the talk bubbles on
  // the board; this window is you and the agent. A reply is direct when it is addressed to the
  // operator, or when it answers a signal the operator sent.
  const ownerSignalIds = new Set(
    events
      .filter((event) => event.actorType === "owner" && event.kind === "signal_recorded")
      .map((event) =>
        event.payload !== null && typeof event.payload === "object"
          ? (event.payload as { signalId?: unknown }).signalId
          : null,
      )
      .filter((id): id is string => typeof id === "string"),
  );
  const exchanges = events.filter((event) => {
    if (event.kind !== "signal_recorded") return false;
    const envelope = envelopes[event.sequence];
    const to = envelope?.to ?? null;
    if (event.actorType === "owner") return to === agentId;
    if (event.actorId !== agentId) return false;
    if (to === OPERATOR_ACTOR.id) return true;
    const replyTo = envelope?.replyTo ?? null;
    return replyTo !== null && ownerSignalIds.has(replyTo);
  });
  return (
    <section className="panel" aria-label={`Agent ${agentId}`}>
      <header className="panel-head calm">
        <span
          className="avatar"
          aria-hidden="true"
          style={{ background: `hsl(${hueOf(agentId)} 52% 46%)` }}
        >
          {initialOf(agentId)}
        </span>
        <div style={{ minWidth: 0 }}>
          <h2>{agentId}</h2>
          <p className={charter !== null ? "charter" : "lbl"}>{charter ?? "your direct conversation"}</p>
        </div>
        <button type="button" className="ghost close" onClick={onClose} aria-label={`Close ${agentId}`}>
          <X aria-hidden="true" />
        </button>
      </header>

      <Thread events={exchanges} executionId={executionId} openEvidence={openEvidence} />

      {onSay !== undefined && (
        <SayBox
          busy={saying}
          error={sayError}
          onSay={onSay}
          recipient={agentId}
          recipientLocked
          draftId={`agent:${executionId ?? ""}:${agentId}`}
        />
      )}

      <p className="panel-foot">
        Only what you and {agentId} said to each other. Group talk lives in the bubbles on the
        board.
      </p>
    </section>
  );
}

/**
 * One conversation's own window, opened from its bubble on the board.
 *
 * Three kinds of talk exist and they never mix here: the ROOM (everything said to nobody in
 * particular - you are part of it, so the say box speaks into it), a PAIR of agents (their
 * addressed exchange - you read it, you are not in it), and your own direct line with one agent,
 * which is not this component at all: that is the agent's own window, opened from its blob.
 */
export function TalkPanel({
  talkId,
  label,
  participants,
  events,
  executionId,
  openEvidence,
  onClose,
  onSay,
  saying = false,
  sayError = "",
}: {
  talkId: string;
  label: string;
  participants: string[];
  events: RuntimeEvent[];
  executionId?: string;
  openEvidence?: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
  onClose: () => void;
  /** Present only for the room: a pair of agents is theirs, and writing into it would put the
   * operator's words inside a conversation that never addressed them. */
  onSay?: (message: string, to: string | null) => void;
  saying?: boolean;
  sayError?: string;
}) {
  return (
    <section className="panel" aria-label={`Conversation ${label}`}>
      <header className="panel-head calm">
        <span className="talk-faces" aria-hidden="true">
          {participants.slice(0, 3).map((id) => (
            <span
              key={id}
              className="avatar mini"
              style={{ background: `hsl(${hueOf(id)} 52% 46%)` }}
            >
              {initialOf(id)}
            </span>
          ))}
        </span>
        <div style={{ minWidth: 0 }}>
          <h2>{talkId === "room" ? "Everyone" : label}</h2>
          <p className="lbl">{talkId === "room" ? "the whole room, you included" : "only between them"}</p>
        </div>
        <button type="button" className="ghost close" onClick={onClose} aria-label={`Close ${label}`}>
          <X aria-hidden="true" />
        </button>
      </header>

      <Thread events={events} executionId={executionId} openEvidence={openEvidence} />

      {onSay !== undefined && (
        <SayBox
          busy={saying}
          error={sayError}
          onSay={onSay}
          draftId={`talk:${executionId ?? ""}:${talkId}`}
        />
      )}

      <p className="panel-foot">
        {talkId === "room"
          ? "Everyone on this run sees what you send here."
          : "Their exchange. To talk to one of them, open its blob."}
      </p>
    </section>
  );
}

/** The words the monitor page prints for the same run (`apps/cli/src/commands/serve/monitor.rs`):
 * one sentence, plain, on every view of a fixture run. */
export const DEMONSTRATION_SENTENCE =
  "Demonstration run — started under the fixture executor: outcomes at start were supplied by a fixture file, not produced by a model or a tool.";

export function RunPanel({
  status,
  events,
  unverifiedReplies = 0,
  onClose,
  openEvidence,
  onSay,
  saying = false,
  sayError = "",
  sayFocus = 0,
  sayRecipient = null,
  owed,
  objective = null,
  replySuggestions = null,
  replyLoading = false,
  replyIssue = null,
  needsDirection = false,
}: {
  status: ExecutionStatus;
  events: RuntimeEvent[];
  /** Completed model calls whose replies still lack an acceptance verdict. */
  unverifiedReplies?: number;
  onClose: () => void;
  /** Absent when this Studio has no way to open sealed content; the thread then shows the events
   * alone rather than pretending the words are missing. */
  openEvidence?: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
  /** Absent when there is nothing to say into - no execution id to address. */
  onSay?: (message: string, to: string | null) => void;
  saying?: boolean;
  sayError?: string;
  sayFocus?: number;
  /** Who the attention block wants the next message addressed to when its action fires — the
   * asker of the pending question, or `null` for the room. Applied on each `sayFocus` bump. */
  sayRecipient?: string | null;
  /** Agents with UNANSWERED operator-addressed questions, from the same ledger the banner
   * reads. When present these are the reply cards — the banner and the box may never again
   * make opposite claims about who is owed an answer on the same screen. */
  owed?: Array<{ id: string; at: string | null }>;
  /** The operator's request in their own words, from the briefing (#1077). Quoted VERBATIM
   * under the heading: it is the one line that says what this run is for. `null` when the
   * Runtime has no briefing route or the declaration carried none. */
  objective?: string | null;
  replySuggestions?: ReplySuggestions | null;
  replyLoading?: boolean;
  replyIssue?: string | null;
  needsDirection?: boolean;
}) {
  const verdict = verdictOf(status.attention);
  // A run that is over is not "running by itself" (#1077, the judge's MINOR): a calm verdict
  // on a completed, failed or cancelled run reads as the wire status, the only true headline.
  const over = status.status === "completed" || status.status === "cancelled" || status.status === "failed";
  // What the run is owed, in words, derived from the same reasons the block below itemizes. The
  // wire state ("running") under a headline that says "needs you" answered the wrong question.
  const debts: string[] = [];
  for (const reason of status.attentionReasons) {
    const kind = typeof reason.kind === "string" ? reason.kind : "";
    if (kind === "waiting_input_node" && !debts.includes(needsDirection ? "your direction" : "your answer")) debts.push(needsDirection ? "your direction" : "your answer");
    if (kind === "blocked_node" && !debts.includes("your go-ahead")) debts.push("your go-ahead");
  }
  // Recent speakers are offered as people to TALK TO — never as people "waiting for an
  // answer": that claim belongs to the ledger alone (the `owed` prop). The old cards made the
  // opposite assertion to the banner one block above, on the same screen.
  // The receipt below the thread needs the envelopes to know who an answer was FOR.
  const runEnvelopes = useEnvelopes(events, status.executionId ?? undefined, openEvidence);
  const askers: Array<{ id: string; at: string | null }> = [];
  for (let index = events.length - 1; index >= 0 && askers.length < 2; index -= 1) {
    const event = events[index];
    if (event.kind !== "signal_recorded") continue;
    if (event.actorType !== "agent" || event.actorId === null) continue;
    if (askers.some((asker) => asker.id === event.actorId)) continue;
    askers.push({ id: event.actorId, at: event.occurredAt });
  }
  const cards = owed !== undefined && owed.length > 0 ? owed : askers;
  const cardsClaim =
    owed !== undefined && owed.length > 0 ? "Who is waiting for an answer" : "Talk to someone";
  const readyDrafts =
    verdict.key === "needs" &&
    replySuggestions?.state === "ready" &&
    replySuggestions.executionId === status.executionId &&
    replySuggestions.headSequence === status.headSequence &&
    replySuggestions.suggestions.length === 2
      ? replySuggestions.suggestions
      : [];
  const [recipient, setRecipient] = useState<string | null>(null);
  // THE CRUDE VERSION WAS GREPPABLE. Past the auto-open limit the words are not in the DOM, so
  // Ctrl-F finds nothing - but every spoken envelope is already in runEnvelopes. Search filters
  // over what the page already holds and opens nothing new; the narrowing is ANNOUNCED, because
  // a thread that silently hides turns reads exactly like a thread where nothing was said.
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const spokenCount = events.filter((event) => event.kind === "signal_recorded").length;
  const shown =
    needle === ""
      ? events
      : events.filter((event) => {
          if (event.kind !== "signal_recorded") return false;
          const envelope = runEnvelopes[event.sequence];
          return (
            (envelope?.text ?? "").toLowerCase().includes(needle) ||
            (event.actorId ?? "").toLowerCase().includes(needle)
          );
        });
  // THE FIVE-REVIEWER BUG. "answer in the thread" promised the answer lands where the run waits,
  // but the composer kept whatever recipient an earlier click locked in — the answer shipped as a
  // DM. The attention action now SETS the target (the asker, or the room) every time it fires.
  useEffect(() => {
    if (sayFocus > 0) setRecipient(sayRecipient);
  }, [sayFocus, sayRecipient]);
  return (
    <section className="panel" aria-label={`Run ${status.executionId ?? ""}`}>
      <header className={`panel-head ${verdict.key === "needs" ? "waiting" : unverifiedReplies > 0 || status.executor === "fixture" ? "verification-unchecked" : ""}`}>
        <i aria-hidden="true" />
        <div style={{ minWidth: 0 }}>
          <h2>
            {verdict.key === "needs"
              ? needsDirection ? "This run needs direction" : "This run needs you"
              : over
                 ? status.status === "completed" && status.executor === "fixture" ? "Demonstration finished · scripted outcomes" : status.status === "completed" && unverifiedReplies > 0 ? "Execution finished · review needed" : `This run is ${readable(status.status ?? "")}`
                : verdict.key === "calm"
                  ? "Running by itself"
                  : "Nothing to report yet"}
          </h2>
          <p className="lbl">
            {verdict.key === "needs" && debts.length > 0
              ? `waiting for ${debts.join(" and ")}`
              : status.status === null
                ? "no record yet"
                : readable(status.status)}
          </p>
          {/* A completed fixture run is byte-identical to a real one everywhere but here
            * (#1064): the executor was declared at start and this is the one sentence that
            * keeps a demonstration from being read as work a model or a tool did. */}
          {status.executor === "fixture" && (
            <p className="demonstration">{DEMONSTRATION_SENTENCE}</p>
          )}
        </div>
        {/* "Close this run" read as ENDING the run - the scariest possible misread on a header
          * that says "needs you". The verb names what actually happens: the panel hides. */}
        <button type="button" className="ghost close" onClick={onClose} aria-label="Close this panel">
          <X aria-hidden="true" />
        </button>
      </header>

      {objective !== null && objective.trim().length > 0 && (
        <p className="run-objective">
          <span className="lbl">Objective</span>
          <q>{objective}</q>
        </p>
      )}

      {/* Only the states that ARE something render, as chips; one muted line asserts every zero
        * collectively. The old sixteen-cell grid stated fifteen zeros a person had to read past
        * to find the 1 that mattered. The rule stands - an omitted bucket must not read as "no
        * such problem" - and the summary line is how silence stays a statement. */}
      <div className="states">
        {LIFECYCLE_STATES.filter((state) => (status.nodeStateCounts[state] ?? 0) > 0).map(
          (state) => (
            <span
              className={`state-chip ${isAlarming(state) ? "alarm" : ""}`}
              key={state}
              title={state}
            >
              {state === "succeeded" && status.executor === "fixture" ? "scripted steps" : state === "succeeded" && unverifiedReplies > 0 ? "steps finished" : readable(state)} <strong>{status.nodeStateCounts[state]}</strong>
            </span>
          ),
        )}
        <span className="states-rest">
          {LIFECYCLE_STATES.every((state) => (status.nodeStateCounts[state] ?? 0) === 0)
            ? "no nodes in any state yet"
            : `nothing in the other ${LIFECYCLE_STATES.filter((state) => (status.nodeStateCounts[state] ?? 0) === 0).length} states`}
        </span>
      </div>
      {unverifiedReplies > 0 && <p className="work-note" role="note">{unverifiedReplies} model repl{unverifiedReplies === 1 ? "y" : "ies"} returned. These calls finished, but their answers have not been checked against the task's acceptance criteria.</p>}

      <div className="thread-search">
        <input
          aria-label="Search this conversation"
          placeholder="search this conversation…"
          value={query}
          onChange={(typed) => setQuery(typed.target.value)}
        />
        {needle !== "" && (
          <span className="hint" role="status">
            {shown.length} of {spokenCount} match — the rest of the log is still there
            <button type="button" className="show-more" onClick={() => setQuery("")}>
              clear
            </button>
          </span>
        )}
      </div>

      <Thread
        events={shown}
        executionId={status.executionId ?? undefined}
        openEvidence={openEvidence}
      />

      {awaitingReply(events, runEnvelopes) && (
        <p className="turn-wait" role="status">
          <i aria-hidden="true" />
          {/* #1083: on an ended run nobody is working, so "waiting for a reply" promised an
            * answer that cannot come. The message is still recorded (signal accepts it). */}
          {over
            ? `delivered — this run is ${status.status}, so nobody is working on it to reply`
            : "delivered — waiting for a reply from whoever is working on this"}
        </p>
      )}

      {onSay !== undefined && status.executionId !== null && (
        <>
        {verdict.key === "needs" && (
          <p className="reply-state" role="status">
            {replyLoading
              ? "Preparing two replies for this run…"
              : readyDrafts.length === 2
                ? "Jev recommended two messages for the current run. Sending a message does not approve or unblock a node."
                : replyIssue ?? replySuggestions?.reason ?? "Two recommended replies are unavailable."}
          </p>
        )}
        <SayBox
          busy={saying}
          error={sayError}
          onSay={onSay}
          focusNonce={sayFocus}
          suggestions={cards}
          draftSuggestions={readyDrafts}
          suggestionsClaim={cardsClaim}
          suggestionsVerb={owed !== undefined && owed.length > 0 ? "answer" : "message"}
          onPickSuggestion={setRecipient}
          onPickDraft={setRecipient}
          recipient={recipient}
          onClearRecipient={() => setRecipient(null)}
          draftId={`run:${status.executionId ?? ""}`}
        />
        </>
      )}

      <p className="panel-foot">
        started {clock(status.startedAt)} · last event {clock(status.lastEventAt)}
      </p>
    </section>
  );
}





