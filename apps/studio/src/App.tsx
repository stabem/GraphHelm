/**
 * GraphHelm Local Studio.
 *
 * TWO COLUMNS AND ONE FLOATING WINDOW. The rail on the left is PROJECTS — folders, with their
 * runs inside. The stage on the right is the run's graph: blocks you arrange, connect, draw on and
 * open. Opening one floats a window over the stage with that node's facts and its thread; the run
 * name in the top strip opens the same window for the whole run.
 *
 * ONE SCROLL REGION. The thread scrolls, because a log is unbounded. The stage pans. Nothing else
 * scrolls inside anything else — the previous shape nested three scroll containers and every
 * gesture landed in the wrong one.
 *
 * IT OPENS CONNECTED. The dev server hands the page the token the Runtime already wrote to disk
 * (`runtime/session.ts`), so nobody copies a hex string out of a terminal. Where that endpoint
 * does not exist — a built bundle served elsewhere — the gate appears. The token itself still
 * never leaves memory: not storage, not a cookie, not a URL, not a log line.
 *
 * Board marks (positions, ink, notes) ARE kept, per run, in this browser only — see
 * `graph/board.ts` for why that exception is the operator's own notes and not a widening of the
 * token rule. Disconnecting erases them.
 */

import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { AlertTriangle, LayoutGrid, LoaderCircle, LogOut, RefreshCw } from "lucide-react";

import {
  DisconnectedError,
  RuntimeClient,
  RuntimeError,
  OPERATOR_ACTOR,
  newIdempotencyKey,
} from "./runtime/client";
import { devSession, type DevSession } from "./runtime/session";
import type {
  EventPage,
  EvidenceContent,
  ExecutionStatus,
  ExecutionSummary,
  MutationEvidence,
} from "./runtime/types";
import {
  registerStudioTools,
  type ModelContextLike,
  type RegisteredTools,
  type ToolActivity,
  type WebMcpAvailability,
} from "./webmcp/adapter";
import { buildGraphModel, conversationFor } from "./graph/model";
import { openQuestions } from "./graph/ledger";
import { topologyNote, verifyTopology, type VerifiedTopology } from "./graph/topology";
import {
  clearBoards,
  emptyBoard,
  loadBoard,
  saveBoard,
  tidyBoard,
  type BoardState,
} from "./graph/board";
import { Connect } from "./components/Connect";
import { Board } from "./components/board";
import { AgentPanel, NodePanel, RunPanel, TalkPanel, actorsInRoom, resetPanelCaches, useEnvelopes, usePersonas } from "./components/panel";
import { ProjectRail } from "./components/rail";
import { Composer, type RouteChoice } from "./components/compose";
import { AddProject } from "./components/addproject";
import { RAIL_MAX, RAIL_MIN, loadRailWidth, saveRailWidth } from "./rail-width";
import { DRAFT_NODE_ID, draftGraph, newExecutionId } from "./graph/draft";
import { readable, verdictOf } from "./components/format";

/** Big enough that the board sees the whole roster on a normal run, and still one page. */
const EVENT_PAGE_SIZE = 200;
const LIST_PAGE_SIZE = 20;

/** What the window is showing, or `none` — which is the DEFAULT and the point: the board is what
 * you came to look at, and a panel that opens over it on arrival hides the thing it describes.
 * The window is a deliberate act: click a block, or click the run's name. */
type Focus =
  | { kind: "run" }
  | { kind: "node"; id: string }
  | { kind: "agent"; id: string }
  /** One conversation bubble on the board: "room", or a sorted "a + b" pair key. */
  | { kind: "talk"; id: string }
  | { kind: "none" };

export interface AppProps {
  /** Injected in tests. In the browser the client is built from the session token. */
  createClient?: (token: string) => RuntimeClient;
  /** Injected in tests so both the "WebMCP present" and "WebMCP absent" paths are reachable
   * without a browser that has either. `undefined` means "probe the real browser". */
  modelContext?: ModelContextLike | null;
  /** Injected in tests. `undefined` probes the dev server; resolving `null` shows the gate. */
  session?: () => Promise<DevSession | null>;
  /** How often the selected run is re-read so replies and progress appear on their own. A prop so
   * tests do not wait wall-clock seconds; the default is the product value. */
  pollIntervalMs?: number;
}

function messageOf(error: unknown, fallback: string): string {
  if (error instanceof RuntimeError || error instanceof DisconnectedError) return error.message;
  return fallback;
}

export default function App({
  createClient,
  modelContext,
  session,
  pollIntervalMs = 4000,
}: AppProps = {}) {
  const clientRef = useRef<RuntimeClient | null>(null);
  const toolsRef = useRef<RegisteredTools | null>(null);

  const [connected, setConnected] = useState(false);
  /** What the rail calls this folder. Named by the operator; absent, the rail says what it can. */
  const [project, setProject] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  /** Which composer is mid-send / holds the failure — "run", "talk" or "agent" — so an error
   * lands only in the box that produced it. `null` means idle / no failure. */
  const [saying, setSaying] = useState<string | null>(null);
  const [sayError, setSayError] = useState<{ via: string; text: string } | null>(null);
  /** Bumped when an attention action asks the message box to take the cursor. A nonce rather
   * than a boolean: the second click must focus again even though the panel is already open. */
  const [sayFocusNonce, setSayFocusNonce] = useState(0);
  /** Who that action wants the message addressed to — the asker of the pending question, or
   * `null` for the room. Set BEFORE the nonce bumps, so the composer retargets every time:
   * five independent reviewers found the answer shipping as a DM to a stale recipient. */
  const [sayTo, setSayTo] = useState<string | null>(null);
  /** The question the next message ANSWERS, when the attention block chose the target. The
   * ledger retires a debt only via a reply whose replyTo names the question's signalId — and
   * the banner's own button used to send none, leaving the debt immortal from this UI (two
   * blind round-4 reviewers). Rides only while the recipient is still that asker. */
  const [sayAnswer, setSayAnswer] = useState<{ asker: string; signalId: string | null } | null>(
    null,
  );
  /** Disconnect asks twice: it erases the token and every stored board, and it sits one slip
   * away from Refresh. First click arms; the arm decays on its own. */
  const [leaving, setLeaving] = useState(false);
  /** True while a key is auto-repeating on the disconnect button — key-repeat clicks carry
   * detail 0 and would sail through the double-click guard (round-3 speedrunner). */
  const keyHeld = useRef(false);
  /** Bumped when the dock's resume button finds no graph file path: the board opens the box and
   * puts the cursor there instead of resume sitting disabled with its excuse in a tooltip. */
  const [fileFocusNonce, setFileFocusNonce] = useState(0);
  /** Whether the run's conversation column is open. Closing the panel closes the PANEL — the
   * run, its board and its crew stay; the run's name in the top strip brings it back. The old
   * wiring deselected the whole run and left "This board is empty" over 19 messages. */
  const [talkOpen, setTalkOpen] = useState(true);

  const [executions, setExecutions] = useState<ExecutionSummary[]>([]);
  /** The rows currently on the rail, readable from the poll timer without joining its
   * dependency list - the refresh reads as many pages as the operator has loaded. */
  const executionsRef = useRef<ExecutionSummary[]>([]);
  useEffect(() => {
    executionsRef.current = executions;
  }, [executions]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [selected, setSelected] = useState("");
  const [status, setStatus] = useState<ExecutionStatus | null>(null);
  const [events, setEvents] = useState<EventPage | null>(null);
  const [evidence, setEvidence] = useState<MutationEvidence | null>(null);

  const [graphFile, setGraphFile] = useState("");
  const [topology, setTopology] = useState<VerifiedTopology | null>(null);
  const [topologyError, setTopologyError] = useState("");

  const [focus, setFocus] = useState<Focus>({ kind: "none" });
  /** A task being composed: an execution id reserved locally and nothing else. `null` means the
   * page is showing a real run. Held here rather than in the rail because the board and the panel
   * both render from it. */
  const [draft, setDraft] = useState<{ executionId: string } | null>(null);
  /** How wide the operator dragged the rail. Read once at mount, written on release. */
  const [railWidth, setRailWidth] = useState(loadRailWidth);
  const [addingProject, setAddingProject] = useState(false);
  /** The Runtime's own model list, read once per draft. `null` while it is being read. */
  const [routes, setRoutes] = useState<RouteChoice | null>(null);
  const [composeError, setComposeError] = useState("");
  const [board, setBoard] = useState<BoardState>(emptyBoard);

  const [webmcp, setWebmcp] = useState<WebMcpAvailability>("unavailable");
  const [tools, setTools] = useState<string[]>([]);
  const [activity, setActivity] = useState<ToolActivity | null>(null);

  /** ONE identity for "the events, or none yet". Building `events?.events ?? []` inline handed
   * every consumer a FRESH empty array per render while a run loaded; the envelope hooks then
   * committed fresh state per pass, and the pair looped through microtasks — 70,201 renders
   * measured before anything else ran. The hooks now also bail on equal commits; this memo
   * removes the other half and stops rebuilding every derivation per render. */
  const eventList = useMemo(() => events?.events ?? [], [events]);
  const model = useMemo(() => buildGraphModel(eventList, topology), [events, topology]);
  const focusedNode = focus.kind === "node" ? focus.id : null;
  const node = useMemo(
    () =>
      focusedNode === null
        ? null
        : (model.nodes.find((candidate) => candidate.id === focusedNode) ?? null),
    [model, focusedNode],
  );
  const nodeThread = useMemo(
    () => (focusedNode === null ? [] : conversationFor(eventList, focusedNode)),
    [events, focusedNode],
  );

  /** The current events page, readable from timers without joining their dependency lists. */
  const eventsRef = useRef<EventPage | null>(null);
  useEffect(() => {
    eventsRef.current = events;
  }, [events]);

  /** Every event after `after`, across as many pages as it takes.
   *
   * The old single read of `{ after: 0, limit: 200 }` froze the tail at event 200: the poll kept
   * succeeding on the same first page while new replies stayed invisible, and the page went on
   * looking live. The page bound is a runaway-loop guard, not a truncation anyone reaches: 600
   * pages of 200 covers 120,000 events, above the store's own 100,000-event ceiling — the earlier
   * 50-page cap could stop an initial rebuild at 10,000 events with the head reported current
   * (PR #467 review), which is a partial projection wearing a live face. */
  const readEvents = useCallback(
    async (client: RuntimeClient, id: string, after: number): Promise<EventPage> => {
      const collected: EventPage = { head: 0, events: [] };
      let cursor = after;
      for (let page = 0; page < 600; page += 1) {
        const next = await client.getEvents(id, { after: cursor, limit: EVENT_PAGE_SIZE });
        collected.head = next.head;
        collected.events.push(...next.events);
        if (next.events.length < EVENT_PAGE_SIZE) break;
        cursor = collected.events[collected.events.length - 1].sequence;
      }
      return collected;
    },
    [],
  );

  /** The run the operator has selected RIGHT NOW, readable after an await. A slow read for a
   * previously selected run must not commit over the newly selected one: two fast rail clicks
   * inside one network round-trip left run A's data under run B's name (round-3 speedrunner). */
  const selectedRef = useRef("");

  const loadExecution = useCallback(
    async (id: string) => {
      const client = clientRef.current;
      if (!client || !id) return;
      setBusy(true);
      try {
        const [nextStatus, nextEvents] = await Promise.all([
          client.getStatus(id),
          readEvents(client, id, 0),
        ]);
        // THE CONNECTION IS A GUARD AXIS TOO (PR #467 review, P1): dispose() cannot cancel a
        // fetch already in flight, and Runtime B can hold the SAME execution id as Runtime A -
        // so a run-id guard alone lets A's late completion land under B. The client object this
        // call captured IS the connection generation; a disconnect or reconnect changes it.
        if (clientRef.current !== client || selectedRef.current !== id) return;
        setStatus(nextStatus);
        // KEEP THE OLD IDENTITY WHEN NOTHING CHANGED. Everything derived from the events array
        // (envelopes, personas, bubbles) keys off its identity; a fresh-but-equal array made the
        // poll re-open every sealed envelope each tick, and could cancel the pass forever.
        setEvents((previous) =>
          previous !== null &&
          previous.head === nextEvents.head &&
          previous.events.length === nextEvents.events.length
            ? previous
            : nextEvents,
        );
        setError("");
      } catch (reason) {
        // Guarded like the success path: a superseded run's FAILURE must not banner the run the
        // operator moved to, and its finally must not clear that run's busy flag (PR #467 review
        // — the success path was guarded in round 3 and this completion was not).
        if (clientRef.current !== client || selectedRef.current !== id) return;
        setError(messageOf(reason, "This execution could not be read."));
      } finally {
        if (clientRef.current === client && selectedRef.current === id) setBusy(false);
      }
    },
    [readEvents],
  );

  /**
   * The live tail: the selected run is re-read on an interval so the conversation MOVES.
   *
   * A person sent a message from this page and the reply - already durable in the log - stayed
   * invisible until they pressed refresh (2026-08-30). A chat that needs manual refresh is a log
   * viewer wearing a chat costume. Quiet by design: no busy flag (a spinner every four seconds
   * reads as the page being broken) and read failures are swallowed rather than surfaced (a
   * transient miss on a background read is not something the operator can act on; the next tick
   * either heals it or the explicit actions surface the real error).
   */
  /** Consecutive background reads that failed; three in a row flips the rail to "stale" —
   * a dead Runtime was indistinguishable from a quiet room under a badge stuck on "live". */
  const pollMisses = useRef(0);
  const [stale, setStale] = useState(false);
  /** One tick at a time: a Runtime slower than the interval stacked concurrent reads whose
   * commits could land out of order and paint stale status for a whole interval. */
  const tickBusy = useRef(false);
  useEffect(() => {
    if (!connected || selected === "") return;
    const timer = setInterval(() => {
      const client = clientRef.current;
      if (!client || tickBusy.current) return;
      tickBusy.current = true;
      void (async () => {
        try {
          // Incremental: only what the log grew since the last read. A quiet tick leaves the
          // events identity UNTOUCHED, so nothing downstream re-derives or re-fetches. The
          // RAIL is re-read on the same tick: it claims to be a live triage surface, and a
          // non-selected run flipping to needs-you stayed frozen until the next click
          // (round-3, two reviewers).
          const lastSeen = eventsRef.current?.events.at(-1)?.sequence ?? 0;
          // The rail refresh covers EVERY row the operator has paged in, not just page one: a
          // kept-but-stale row let a page-two run flip to needs_you invisibly under the "live"
          // badge (PR #467 review). Pages follow the loaded count, bounded at 50 hops.
          const loadedRows = executionsRef.current.length;
          const readList = async () => {
            const first = await client.listExecutions({ limit: LIST_PAGE_SIZE });
            const rows = [...first.executions];
            let cursor = first.hasMore ? first.nextCursor : null;
            for (let hops = 1; cursor !== null && rows.length < loadedRows && hops < 50; hops += 1) {
              const next = await client.listExecutions({ after: cursor, limit: LIST_PAGE_SIZE });
              rows.push(...next.executions);
              cursor = next.hasMore ? next.nextCursor : null;
            }
            return rows;
          };
          const [nextStatus, fresh, listRows] = await Promise.all([
            client.getStatus(selected),
            readEvents(client, selected, lastSeen),
            readList(),
          ]);
          // The connection too, not just the run: Runtime B can hold the same execution id, and
          // a tick that started against A must not paint B (PR #467 review, P1).
          if (clientRef.current !== client || selectedRef.current !== selected) return;
          setStatus(nextStatus);
          // The refresh covers what it read and prepends what is new; rows beyond the read
          // range (a store that GREW past the operator's paging mid-poll) are kept, not
          // truncated.
          setExecutions((previous) => {
            const refreshed = new Map(listRows.map((run) => [run.executionId, run]));
            const kept = previous.map((run) => refreshed.get(run.executionId) ?? run);
            const known = new Set(previous.map((run) => run.executionId));
            const added = listRows.filter((run) => !known.has(run.executionId));
            return added.length === 0 &&
              kept.every((run, index) => run === previous[index])
              ? previous
              : [...added, ...kept];
          });
          // The duplicate filter runs INSIDE the updater, against the array it appends to: a
          // full reload landing mid-tick already contains this tick's events, and filtering
          // against the tick-start cursor alone appended them twice (round-3 speedrunner).
          setEvents((previous) => {
            const base = previous?.events.at(-1)?.sequence ?? 0;
            const novel = fresh.events.filter((event) => event.sequence > base);
            if (novel.length === 0) return previous;
            return previous === null
              ? { head: fresh.head, events: novel }
              : { head: fresh.head, events: [...previous.events, ...novel] };
          });
          pollMisses.current = 0;
          setStale(false);
        } catch {
          // A single transient miss stays quiet on purpose - but persistent failure must not:
          // the screen aging under a "live" badge happened for real (server died mid-session).
          if (clientRef.current !== client) return;
          pollMisses.current += 1;
          if (pollMisses.current >= 3) setStale(true);
        } finally {
          tickBusy.current = false;
        }
      })();
    }, pollIntervalMs);
    return () => clearInterval(timer);
  }, [connected, selected, pollIntervalMs, readEvents]);

  const loadList = useCallback(
    async (options: { append?: boolean; cursor?: string | null } = {}) => {
      const client = clientRef.current;
      if (!client) return [] as ExecutionSummary[];
      const page = await client.listExecutions({
        limit: LIST_PAGE_SIZE,
        after: options.cursor ?? undefined,
      });
      if (clientRef.current !== client) return [] as ExecutionSummary[];
      setExecutions((previous) => (options.append ? [...previous, ...page.executions] : page.executions));
      setNextCursor(page.hasMore ? page.nextCursor : null);
      return page.executions;
    },
    [],
  );

  /** Selecting a run swaps the board and drops the proof with it. A verified topology is a claim
   * about ONE run; carrying it across would draw the previous run's shape over this one's nodes,
   * with its proof still showing. */
  const select = useCallback(
    (id: string) => {
      selectedRef.current = id;
      setSelected(id);
      setStatus(null);
      setEvents(null);
      setFocus({ kind: "none" });
      setTopology(null);
      setTopologyError("");
      // A recipient chosen in one run must not address a message in another — nor may a send
      // failure, an in-flight send's busy display, an act-note, or a pending answer's signalId
      // follow the operator across runs. `saying` in particular: cleared HERE, and the old
      // send's guarded finally then leaves it alone, so the new run's box never witnesses a
      // busy-to-idle edge that was not its own.
      setSayTo(null);
      setSayAnswer(null);
      setSaying(null);
      setSayError(null);
      setEvidence(null);
      setTalkOpen(true);
      const stored = loadBoard(id);
      setBoard(stored);
      // The path this board was pointed at last time comes back with the board, so the shape
      // check below can re-run without anyone re-typing a Runtime-host path. The check is armed
      // HERE, from the REMEMBERED path only — arming it from the live field made the first
      // typed character fire a verify on a one-letter path.
      setGraphFile(stored.graphFile);
      autoVerify.current =
        stored.graphFile.trim().length > 0 ? { id, path: stored.graphFile.trim() } : null;
      void loadExecution(id);
    },
    [loadExecution],
  );

  /**
   * Opens a new task: an execution id reserved locally, an empty board, and the Runtime's model
   * list read so the picker can offer what this server can actually reach.
   *
   * NOTHING IS WRITTEN. No graph is published and no execution exists until the composer is sent,
   * so abandoning a draft leaves no orphan in the store for someone to find later and wonder about.
   */
  const startDraft = useCallback(() => {
    const client = clientRef.current;
    if (!client) return;
    const executionId = newExecutionId();
    setDraft({ executionId });
    selectedRef.current = "";
    setSelected("");
    setStatus(null);
    setEvents(null);
    setTopology(null);
    setTopologyError("");
    setComposeError("");
    setRoutes(null);
    setBoard(emptyBoard());
    setFocus({ kind: "node", id: DRAFT_NODE_ID });
    void (async () => {
      try {
        const routes = await client.listRoutes();
        if (clientRef.current !== client) return;
        setRoutes(routes);
      } catch (reason) {
        // The picker degrades to "the Runtime's default" rather than blocking the task. A model
        // list that could not be read is not a reason to refuse to start work the server may well
        // be able to do.
        if (clientRef.current !== client) return;
        setRoutes({ configured: true, routes: [] });
        setComposeError(messageOf(reason, "The model list could not be read."));
      }
    })();
  }, []);

  const discardDraft = useCallback(() => {
    setDraft(null);
    setFocus({ kind: "none" });
    setComposeError("");
  }, []);

  /**
   * Publishes the one-node graph and starts it, then switches the page to the run it created.
   *
   * The draft is cleared only AFTER the start is verified. A failed start leaves the operator's
   * text on screen where they can fix and resend it - clearing first would lose what they wrote to
   * a refusal they did not cause.
   */
  const sendDraft = useCallback(
    async (objective: string, route: string | null) => {
      const client = clientRef.current;
      if (!client || draft === null) return;
      const startedId = draft.executionId;
      setBusy(true);
      setComposeError("");
      try {
        const evidenceOfStart = await client.startTask(
          startedId,
          draftGraph(startedId, objective),
          // The key derives from the DRAFT, not from the attempt: a resend of the same draft
          // after an ambiguous outcome carries the same identity, so the Runtime reconciles it
          // as a retry instead of starting a second run (PR #467 review).
          { route, actor: OPERATOR_ACTOR, idempotencyKey: `start-${startedId}` },
        );
        // The connection generation, before anything commits: a start fired at Runtime A must
        // not clear a draft, select a run, or banner an error once the page is on Runtime B
        // (PR #467 review, P1 - the same class as the run guards, on the connection axis).
        if (clientRef.current !== client) return;
        if (evidenceOfStart.result === "refused") {
          setComposeError(
            evidenceOfStart.diagnostics[0]?.message ?? "The Runtime refused to start this task.",
          );
          return;
        }
        // UNKNOWN keeps the draft. "Not proven" is not "done": clearing here would throw away
        // the objective and the retry identity for an action the client itself classified as
        // unconfirmed. The words stay on screen and resend is safe by the stable key above.
        if (evidenceOfStart.result === "unknown") {
          setComposeError(
            "The start could not be verified. Nothing was lost - send again; the retry carries the same identity, so it cannot start a second run.",
          );
          return;
        }
        setEvidence(evidenceOfStart);
        // Only the draft this send belongs to is cleared and selected: the completion of a
        // superseded send must not unmount whatever the operator is composing now, nor drag
        // the page away from it.
        let stillMine = false;
        setDraft((current) => {
          stillMine = current !== null && current.executionId === startedId;
          return stillMine ? null : current;
        });
        if (stillMine) select(startedId);
        await loadList();
      } catch (reason) {
        if (clientRef.current !== client) return;
        setComposeError(messageOf(reason, "The task could not be started."));
      } finally {
        if (clientRef.current === client) setBusy(false);
      }
    },
    [draft, loadList, select],
  );

  /**
   * Dragging the rail's edge.
   *
   * The listeners are attached ONCE and always, and the drag flag is read INSIDE them. Installing
   * them from the pointerdown handler is the shape that shipped a stale-hold bug on this very
   * board: the flag lives in a ref, a ref does not re-render, so an effect naming it never re-runs
   * and pointerup is never wired - leaving the hold armed after the button came up.
   */
  const gripping = useRef(false);
  const [grip, setGrip] = useState(false);
  useEffect(() => {
    const move = (event: PointerEvent) => {
      if (!gripping.current) return;
      // Clamped here as well as in CSS: a pointer dragged past either end must not leave the
      // stored width somewhere the next session cannot recover from.
      setRailWidth(Math.max(RAIL_MIN, Math.min(RAIL_MAX, Math.round(event.clientX))));
    };
    const release = () => {
      if (!gripping.current) return;
      gripping.current = false;
      setGrip(false);
      setRailWidth((width) => {
        saveRailWidth(width);
        return width;
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
    };
  }, []);

  const updateBoard = useCallback(
    (next: BoardState) => {
      setBoard(next);
      if (selected) saveBoard(selected, next);
    },
    [selected],
  );

  /** One shape check against one EXPLICIT path. The auto-verify passes the remembered path
   * directly instead of going through the live field: arming was already blind to typing, but
   * firing read `graphFile` back out of state — so editing the box during the run's first load
   * verified a half-typed path (round-3 speedrunner). */
  const verifyPath = useCallback(
    async (file: string) => {
      const client = clientRef.current;
      if (!client || file.length === 0) return;
      // The run this verification belongs to, captured before the await: a slow read armed for
      // run A (or for a path since retyped) must not install its proof over run B's board — a
      // drawn edge reads as evidence, so a stale one is the worst thing this could commit
      // (PR #467 review, same family as the selectedRef guard on loadExecution).
      const forRun = selectedRef.current;
      setBusy(true);
      setTopologyError("");
      try {
        const read = await client.getTopology(file);
        if (clientRef.current !== client || selectedRef.current !== forRun) return;
        setTopology(verifyTopology(eventList, read, file));
      } catch (reason) {
        if (clientRef.current !== client || selectedRef.current !== forRun) return;
        setTopology(null);
        setTopologyError(messageOf(reason, "That graph file could not be read."));
      } finally {
        if (clientRef.current === client && selectedRef.current === forRun) setBusy(false);
      }
    },
    [eventList],
  );
  const drawConnections = useCallback(() => verifyPath(graphFile.trim()), [verifyPath, graphFile]);

  /** The run (and remembered path) still owed one automatic verify. */
  const autoVerify = useRef<{ id: string; path: string } | null>(null);
  /** Re-verifies the REMEMBERED path, once, as soon as the events it checks against have
   * arrived. Armed by select(); both arming and firing are blind to the live field. The
   * verification itself is UNCHANGED — verifyTopology still refuses an unproven edge. */
  useEffect(() => {
    const want = autoVerify.current;
    if (want === null || want.id !== selected || events === null) return;
    autoVerify.current = null;
    void verifyPath(want.path);
  }, [selected, events, verifyPath]);

  // The armed disconnect stands down by itself: an armed destructive control forgotten on
  // screen is a trap for the next misclick.
  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(() => setLeaving(false), 5000);
    return () => clearTimeout(timer);
  }, [leaving]);

  /** Kept in a ref so the WebMCP hooks — registered once per connection — always call the CURRENT
   * closure. Passing the callbacks directly would freeze the first render's copies into the tool
   * table, and the page would stop updating after the first agent call. */
  const hooksRef = useRef({
    onSelect: (_id: string) => {},
    onMutation: (_evidence: MutationEvidence) => {},
    onActivity: (_activity: ToolActivity) => {},
  });

  hooksRef.current = {
    onSelect: (id: string) => {
      if (id !== selected) select(id);
    },
    onMutation: (next: MutationEvidence) => {
      // Only for the run ON SCREEN. A successful WebMCP write selects its run first, so this
      // matches; a REFUSED write skips the select, and installing run A's refused status,
      // evidence and focus under run B's name was exactly the cross-run smear the selection
      // ordering fix left behind (PR #467 review, P1). The activity chip still reports the
      // refusal - onActivity is unconditional - so the agent's act is never invisible.
      if (next.executionId !== selectedRef.current) return;
      setEvidence(next);
      if (next.statusAfter) setStatus(next.statusAfter);
      // An agent that acted on a node brings that node's window up, so the person sees the same
      // detail the agent was working on rather than a whole-run view they have to search.
      setFocus(next.node === null ? { kind: "run" } : { kind: "node", id: next.node });
      void loadExecution(next.executionId);
      void loadList().catch(() => {});
    },
    onActivity: (next: ToolActivity) => setActivity(next),
  };

  const openWith = useCallback(
    async (client: RuntimeClient) => {
      clientRef.current = client;
      const page = await client.listExecutions({ limit: LIST_PAGE_SIZE });
      // A disconnect while the opening read was in flight must not re-light the page: this
      // completion belongs to the connection it started, like every other (PR #467 review, P1).
      if (clientRef.current !== client) return;
      setExecutions(page.executions);
      setNextCursor(page.hasMore ? page.nextCursor : null);
      setConnected(true);

      const registration = registerStudioTools(
        client,
        {
          onSelect: (id) => hooksRef.current.onSelect(id),
          onMutation: (next) => hooksRef.current.onMutation(next),
          onActivity: (next) => hooksRef.current.onActivity(next),
        },
        modelContext === undefined ? {} : { modelContext },
      );
      toolsRef.current = registration;
      setWebmcp(registration.availability);
      setTools(registration.names);
      // A host whose registerTool settles ASYNCHRONOUSLY confirms its names only when `ready`
      // settles - the snapshot above would then say "no site tools" forever on such a host
      // (PR #467 review). Refreshed once settled, and only if this registration is still the
      // page's current one: a disconnect in between must not resurrect dead chips.
      void registration.ready.then(() => {
        if (toolsRef.current !== registration) return;
        setWebmcp(registration.availability);
        setTools(registration.names);
      });

      // Read at connect, not only when a draft opens: the attention area needs to know whether a
      // model is wired to SAY why a waiting node will never move on its own. Unreadable stays
      // null and the area says nothing about models rather than guessing.
      void client
        .listRoutes()
        .then((routes) => {
          if (clientRef.current === client) setRoutes(routes);
        })
        .catch(() => {});

      // The opening pick pages PAST the first 20: "open on the run that needs you" is the
      // page's first promise, and a store whose only blocked run sorts onto page three would
      // otherwise open calm with the debt hidden behind a "more" button (PR #467 review).
      // Bounded at 50 hops (1,000 runs) - a triage pick, not a full index scan; the rail still
      // shows page one and the selected run loads by id regardless of which page held it.
      let candidate = page.executions.find((row) => row.attention === "needs_you");
      let cursor = page.hasMore ? page.nextCursor : null;
      for (let hops = 0; candidate === undefined && cursor !== null && hops < 50; hops += 1) {
        const next = await client.listExecutions({ after: cursor, limit: LIST_PAGE_SIZE });
        if (clientRef.current !== client) return;
        candidate = next.executions.find((row) => row.attention === "needs_you");
        cursor = next.hasMore ? next.nextCursor : null;
      }
      const first = candidate ?? page.executions[0];
      if (first) {
        selectedRef.current = first.executionId;
        setSelected(first.executionId);
        const stored = loadBoard(first.executionId);
        setBoard(stored);
        setGraphFile(stored.graphFile);
        autoVerify.current =
          stored.graphFile.trim().length > 0
            ? { id: first.executionId, path: stored.graphFile.trim() }
            : null;
        await loadExecution(first.executionId);
      }
    },
    [loadExecution, modelContext],
  );

  /** True once ANY connection attempt has begun this page-life - the boot auto-connect yields
   * to it and never fires afterwards, deliberately including after a manual disconnect: a boot
   * continuation re-opening a session the operator just closed is the exact behavior the boot
   * effect's own comment forbids. */
  const connectionAttempted = useRef(false);

  const connect = useCallback(
    async (token: string) => {
      connectionAttempted.current = true;
      setConnecting(true);
      setError("");
      const client = createClient ? createClient(token) : new RuntimeClient(token);
      try {
        await client.health();
        await openWith(client);
      } catch (reason) {
        // The tools too, not just the client: a failure AFTER registration (the first load can
        // throw) would otherwise leave the host holding registrations whose only removal handle
        // this overwrite discards - and the next connect's registerTool calls then collide with
        // them (PR #467 review). unregister() is idempotent and safe when nothing registered.
        toolsRef.current?.unregister();
        toolsRef.current = null;
        setWebmcp("unavailable");
        setTools([]);
        client.dispose();
        clientRef.current = null;
        setConnected(false);
        setError(messageOf(reason, "The Runtime could not be reached."));
      } finally {
        setConnecting(false);
      }
    },
    [createClient, openWith],
  );

  const connectRef = useRef(connect);
  connectRef.current = connect;

  /**
   * The automatic session, tried once at boot.
   *
   * A `null` here is the ordinary outcome for a built bundle and is NOT an error: the gate simply
   * appears. Only a token that exists and is then refused produces a message, and it comes from
   * `connect` like any other refusal.
   *
   * Boot ONLY — the effect has no dependencies and reaches `connect` through a ref. Listing
   * `connect` would re-run this whenever its identity changed and re-open the session behind the
   * operator's back after they deliberately disconnected.
   */
  useEffect(() => {
    let cancelled = false;
    const read = session ?? devSession;
    void (async () => {
      const opened = await read();
      // The boot is an async completion that CREATES a client, so it carries the same
      // connection-generation discipline as the completions that commit under one (PR #467
      // review, P1): if any connection attempt began while session() was in flight - the
      // operator typed a token faster than the dev endpoint answered - this continuation
      // discards itself instead of starting a second connection over the manual one.
      if (cancelled || opened === null || connectionAttempted.current) return;
      setProject(opened.project);
      await connectRef.current(opened.token);
    })();
    return () => {
      cancelled = true;
    };
  }, [session]);

  const disconnect = useCallback(() => {
    toolsRef.current?.unregister();
    toolsRef.current = null;
    clientRef.current?.dispose();
    clientRef.current = null;
    clearBoards();
    // The button PROMISES erasure — and the module maps (drafts, opened sealed words) were
    // surviving it, restoring a half-typed draft from the "erased" session and able to serve
    // one store's words as another's (three round-4 reviewers).
    resetPanelCaches();
    setConnected(false);
    setExecutions([]);
    setNextCursor(null);
    setSelected("");
    setStatus(null);
    setEvents(null);
    setEvidence(null);
    setActivity(null);
    setTools([]);
    setWebmcp("unavailable");
    setGraphFile("");
    setTopology(null);
    setTopologyError("");
    setFocus({ kind: "none" });
    setBoard(emptyBoard());
    setError("");
  }, []);

  useEffect(
    () => () => {
      toolsRef.current?.unregister();
      clientRef.current?.dispose();
    },
    [],
  );

  // ESCAPE CLOSES THE TOP WINDOW. The only global key the shell owns: a node's, agent's or
  // conversation's window closes; the run itself never deselects from a key.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setFocus((current) =>
        current.kind === "node" || current.kind === "agent" || current.kind === "talk"
          ? { kind: "none" }
          : current,
      );
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const runMutation = useCallback(
    async (perform: (client: RuntimeClient) => Promise<MutationEvidence>) => {
      const client = clientRef.current;
      if (!client) return;
      // The run this mutation was fired FROM, so the catch and finally can tell a superseded
      // completion apart too - the success guard's early return still fell through to an
      // unconditional finally that cleared the NEW run's busy flag, and a rejection bannered
      // its error under the new run (PR #467 review, the fresh-evidence half).
      const forRun = selectedRef.current;
      setBusy(true);
      setError("");
      try {
        const next = await perform(client);
        // Committed only if the run the mutation belongs to is STILL the selected one - AND the
        // connection is still the one it was fired over: rail clicks stay live while a mutation
        // flies, Runtime B can hold the same execution id as Runtime A, and installing run A's
        // statusAfter (and its approval target) under run B's name is how a press meant for A
        // appends to B (PR #467 review — the same guards loadExecution carries).
        if (clientRef.current !== client || selectedRef.current !== next.executionId) return;
        setEvidence(next);
        if (next.statusAfter) setStatus(next.statusAfter);
        await loadExecution(next.executionId);
        await loadList();
      } catch (reason) {
        if (clientRef.current !== client || selectedRef.current !== forRun) return;
        setError(messageOf(reason, "The action could not be completed."));
      } finally {
        if (clientRef.current === client && selectedRef.current === forRun) setBusy(false);
      }
    },
    [loadExecution, loadList],
  );

  /** Opens one sealed item. Stable across renders because the thread's turns depend on it: a fresh
   * identity every render would re-open every envelope on every render. */
  const openEvidence = useCallback(
    async (executionId: string, evidenceId: string): Promise<EvidenceContent> => {
      const client = clientRef.current;
      if (!client) throw new Error("This Studio session is not connected.");
      return client.readEvidence(executionId, evidenceId);
    },
    [],
  );

  /** Says something into the selected run. Its own busy/error pair rather than the shared ones —
   * and TAGGED BY SURFACE: with the run panel and a chat column open side by side, one shared
   * error painted "could not be sent" into every composer at once, so the operator could not tell
   * which of their messages actually failed (round-3 markets). */
  const say = useCallback(
    async (
      message: string,
      to: string | null = null,
      via: string = "run",
      replyTo: string | null = null,
    ) => {
      const client = clientRef.current;
      if (!client || selected === "") return;
      // The run this send belongs to. Its completion may only touch the say state while that
      // run is still on screen: the rail stays selectable during a send, and an unconditional
      // finally cleared `saying` under the NEXT run - whose box then read the busy-to-idle
      // edge as ITS delivery and deleted a draft that was never sent (PR #467 review, P1).
      const forRun = selected;
      setSaying(via);
      setSayError(null);
      const fail = (text: string) => {
        if (clientRef.current === client && selectedRef.current === forRun) {
          setSayError({ via, text });
        }
      };
      try {
        const evidence = await client.signal(selected, message, {
          ...(to === null ? {} : { to }),
          // The receipt the ledger settles by. Without it, an answer sent through the banner's
          // own button never retired the question it answered.
          ...(replyTo === null ? {} : { replyTo }),
        });
        if (evidence.result === "refused") {
          fail(evidence.diagnostics[0]?.message ?? "The Runtime refused the message.");
          return;
        }
        if (evidence.result === "unknown") {
          // Deliberately not reported as sent. The append may well have landed, and saying "sent"
          // about something unverified is the one error that cannot be walked back later.
          fail(
            "The Runtime accepted this but it could not be confirmed in the log. Reload before sending it again.",
          );
        }
        await loadExecution(selected);
      } catch (reason) {
        fail(messageOf(reason, "The message could not be sent."));
      } finally {
        if (clientRef.current === client && selectedRef.current === forRun) setSaying(null);
      }
    },
    [loadExecution, selected],
  );

  // The crew: personas chartered in this run's log plus the agents the thread has heard from.
  // Derived AT THIS LEVEL because the crew lives on the canvas, not inside the chat panel.
  const personas = usePersonas(eventList, selected === "" ? undefined : selected, openEvidence);
  // When each actor was last heard from - the blobs dim with silence, honestly. MEMOIZED, like
  // every O(events) derivation below: these run inside the component body and App re-renders at
  // pointer-move frequency during a rail drag - rebuilding five full-array scans per frame was
  // main-thread work for data that only changes when the events identity does (round-4).
  const crew = useMemo<Array<{ id: string; charter: string | null; lastAt: string | null }>>(() => {
    const lastHeard = new Map<string, string>();
    for (const event of eventList) {
      if (event.actorId !== null && event.occurredAt !== null) lastHeard.set(event.actorId, event.occurredAt);
    }
    return [
      ...Object.entries(personas).map(([id, charter]) => ({
        id,
        charter: charter || null,
        lastAt: lastHeard.get(id) ?? null,
      })),
      ...actorsInRoom(eventList, personas)
        // The operator is the person AT this screen, not a blob on it.
        .filter((id) => id !== OPERATOR_ACTOR.id)
        .map((id) => ({ id, charter: null, lastAt: lastHeard.get(id) ?? null })),
    ];
  }, [eventList, personas]);

  // THE CONVERSATIONS, SORTED BY WHO IS TALKING TO WHOM. Three kinds, kept apart because reading
  // them mixed is what made the room illegible: the ROOM (said to nobody in particular - you are
  // in it), each PAIR of agents (their addressed exchange - you can read it, you are not in it),
  // and your DIRECT LINE with one agent (behind that agent's own blob, not here). Each room and
  // pair becomes a bubble standing on the board.
  const envelopes = useEnvelopes(eventList, selected === "" ? undefined : selected, openEvidence);
  const { roomEvents, pairTalks, talks } = useMemo(() => {
    const spoken = eventList.filter(
      (event) =>
        event.kind === "signal_recorded" &&
        (event.actorType === "agent" || event.actorType === "owner"),
    );
    const room = spoken.filter((event) => (envelopes[event.sequence]?.to ?? null) === null);
    const pairs = new Map<string, { participants: string[]; events: typeof spoken }>();
    for (const event of spoken) {
      const to = envelopes[event.sequence]?.to ?? null;
      const from = event.actorId;
      if (to === null || from === null) continue;
      // An exchange with the operator is the direct line, which lives behind the agent's blob.
      if (from === OPERATOR_ACTOR.id || to === OPERATOR_ACTOR.id) continue;
      const key = [from, to].sort().join(" + ");
      const entry = pairs.get(key) ?? { participants: [from, to].sort(), events: [] };
      entry.events.push(event);
      pairs.set(key, entry);
    }
    return {
      roomEvents: room,
      pairTalks: pairs,
      talks: [
        ...(room.length > 0
          ? [
              {
                key: "room",
                label: "everyone",
                participants: [
                  ...new Set(
                    room
                      .map((event) => event.actorId)
                      .filter((id): id is string => id !== null && id !== OPERATOR_ACTOR.id),
                  ),
                ],
                count: room.length,
                lastAt: room.at(-1)?.occurredAt ?? null,
              },
            ]
          : []),
        ...[...pairs.entries()].map(([key, entry]) => ({
          key,
          label: key,
          participants: entry.participants,
          count: entry.events.length,
          lastAt: entry.events.at(-1)?.occurredAt ?? null,
        })),
      ],
    };
  }, [eventList, envelopes]);
  const talkThread =
    focus.kind === "talk"
      ? focus.id === "room"
        ? roomEvents
        : (pairTalks.get(focus.id)?.events ?? [])
      : [];

  // THE LEDGER OF UNANSWERED QUESTIONS. The attention block used to name the debtor ("start is
  // waiting for input") and never the debt — the question itself was nowhere on screen, and the
  // owner's verbatim reaction was "needs me FOR WHAT?". A question is a recorded fact: an agent's
  // signal addressed to the operator by envelope. Derived, never guessed — an unaddressed room
  // message is not a question, because quoting the WRONG message is worse than quoting none.
  //
  // Two rules a second review round added, both about honesty of settlement:
  // - A debt to the OPERATOR dies only when the OPERATOR answers. answeredIds was author-blind
  //   and another agent's side-reply erased a question the thread still showed unanswered.
  // - A reply to the operator's OWN signal is a return receipt, not a new question: quoting a
  //   delivered report as "X asked you" would invert a settled exchange into a fresh demand.
  // Extracted to `graph/ledger.ts` so the WebMCP attention tool reads the SAME debt this
  // banner shows - two derivations of "who is owed" is how the surfaces start disagreeing.
  const { pendingQuestion, owedCards } = useMemo(() => {
    const owed = openQuestions(eventList, envelopes, OPERATOR_ACTOR.id);
    // One card per creditor, newest first — this same derivation feeds the composer's reply
    // hints, so the banner and the box below it can never again disagree about who is owed.
    const cards: Array<{ id: string; at: string | null }> = [];
    for (const debt of owed) {
      if (cards.some((card) => card.id === debt.asker)) continue;
      cards.push({ id: debt.asker, at: debt.at });
      if (cards.length === 2) break;
    }
    return {
      pendingQuestion:
        owed.length > 0
          ? { asker: owed[0].asker, text: owed[0].text, signalId: owed[0].signalId }
          : null,
      owedCards: cards,
    };
  }, [eventList, envelopes]);

  if (!connected) {
    return <Connect onConnect={(token) => void connect(token)} busy={connecting} error={error} />;
  }

  const verdict = status ? verdictOf(status.attention) : null;
  const blocking =
    status?.attentionReasons
      .map((reason) => (typeof reason.node === "string" ? reason.node : null))
      .filter((candidate): candidate is string => candidate !== null) ?? [];
  const approvable =
    status?.attentionReasons
      .filter((reason) => reason.kind === "blocked_node")
      .map((reason) => (typeof reason.node === "string" ? reason.node : null))
      .filter((candidate): candidate is string => candidate !== null) ?? [];
  const approveTarget =
    focusedNode !== null && approvable.includes(focusedNode) ? focusedNode : (approvable[0] ?? "");
  const waitingInstead = approveTarget === "" && blocking.length > 0;
  /** The board a draft shows: the start node alone, with no history, because none exists. Built
   * here rather than through `buildGraphModel` - that reads events, and a draft has none, so
   * asking it would return an empty board and lose the one node the operator is about to fill. */
  const draftModel = {
    nodes: [
      { id: DRAFT_NODE_ID, state: "ready" as const, touches: 0, lastEventAt: null, history: [], reopened: null },
    ],
    edges: [],
    entrypoints: [DRAFT_NODE_ID],
    rosterDeclared: true,
    edgesKnown: true,
    lint: [],
  };
  const connectionTone: "proven" | "refused" | "none" =
    topologyError !== "" || topology?.match === "mismatched"
      ? "refused"
      : topology?.match === "matched"
        ? "proven"
        : "none";

  return (
    <div className="app" style={{ "--rail": `${railWidth}px` } as CSSProperties}>
      <ProjectRail
        projects={[{ name: project ?? "this runtime", runs: executions }]}
        selected={selected}
        connected={connected}
        stale={stale}
        hasMore={nextCursor !== null}
        busy={busy}
        onSelect={select}
        onLoadMore={() => void loadList({ append: true, cursor: nextCursor })}
        onNewTask={startDraft}
        onAddProject={() => {
          setAddingProject(true);
          setFocus({ kind: "none" });
        }}
      />

      {/* The grip is a real control, so it is focusable and the keyboard can move it: a pointer
          is not the only way somebody sizes a pane. */}
      <div
        role="separator"
        aria-label="Resize the projects rail"
        aria-orientation="vertical"
        aria-valuenow={railWidth}
        aria-valuemin={RAIL_MIN}
        aria-valuemax={RAIL_MAX}
        tabIndex={0}
        className={`grip ${grip ? "dragging" : ""}`}
        onPointerDown={(event) => {
          event.preventDefault();
          gripping.current = true;
          setGrip(true);
        }}
        onKeyDown={(event) => {
          const step = event.key === "ArrowLeft" ? -16 : event.key === "ArrowRight" ? 16 : 0;
          if (step === 0) return;
          event.preventDefault();
          const next = Math.max(RAIL_MIN, Math.min(RAIL_MAX, railWidth + step));
          setRailWidth(next);
          saveRailWidth(next);
        }}
      />

      <div className="stage">
        <div className="topstrip">
          <div className="strip-card">
            {selected ? (
              <button
                type="button"
                className="run-name"
                onClick={() => {
                  // Every nonce bump carries its own target: this button reopens the panel and
                  // speaks to the ROOM. Bumping without setting sayTo replayed a long-dead
                  // "answer X" choice — the round-1 bug back through a side door, and all five
                  // round-2 reviewers caught it.
                  setSayTo(null);
                  setSayAnswer(null);
                  setTalkOpen(true);
                  setFocus({ kind: "run" });
                  setSayFocusNonce((nonce) => nonce + 1);
                }}
              >
                {selected}
              </button>
            ) : (
              <span className="meta">pick a run, or start one</span>
            )}
            {verdict && <span className={`tag ${verdict.key}`}>{verdict.label}</span>}
          </div>

          <div className="strip-card right">
            {webmcp === "available" ? (
              <span className="toolchips">
                {tools.map((name) => {
                  const mine = activity?.tool === name;
                  const refused = mine && activity.outcome === "refused";
                  // A refused agent call must LOOK refused: the chip lit identically for
                  // success and refusal, and the adapter's own detail reached nobody.
                  return (
                    <span
                      key={name}
                      className={mine ? (refused ? "on refused" : "on") : ""}
                      style={refused ? { color: "var(--danger)" } : undefined}
                      title={mine ? activity.detail : undefined}
                    >
                      {name.replace("graphhelm_", "")}
                      {refused ? " ✕" : ""}
                    </span>
                  );
                })}
              </span>
            ) : null}
            {/* Blocks keep the position they were dragged to, per run, across reloads. Stored
                positions outlive the layout that made them, and a graph that gains a node lands
                it on one already placed - so there has to be a way back to the grid. */}
            <button
              type="button"
              className="ghost"
              onClick={() => updateBoard(tidyBoard(board))}
              disabled={!selected}
              aria-label="Tidy the board"
              title="Put every block back on the grid"
            >
              <LayoutGrid aria-hidden="true" />
            </button>
            <button
              type="button"
              className="ghost"
              onClick={() => void loadExecution(selected)}
              disabled={busy || !selected}
              aria-label="Refresh"
            >
              <RefreshCw className={busy ? "spin" : ""} aria-hidden="true" />
            </button>
            {/* One slip away from Refresh, and it erases the token AND every stored board.
              * First click arms, second erases; the arm decays after 5s on its own. */}
            <button
              type="button"
              className={`ghost ${leaving ? "arming" : ""}`}
              onClick={(click) => {
                if (leaving) {
                  // The second half of an accidental double-click arrives with detail > 1, and
                  // a HELD Enter key delivers a stream of detail-0 clicks flagged as repeats —
                  // both are one continuous gesture, the very slip the arm exists for. Only a
                  // separate, deliberate activation fires.
                  if (click.detail > 1 || keyHeld.current) return;
                  setLeaving(false);
                  disconnect();
                } else {
                  setLeaving(true);
                }
              }}
              onKeyDown={(key) => {
                if (key.repeat) keyHeld.current = true;
              }}
              onKeyUp={() => {
                keyHeld.current = false;
              }}
              aria-label={leaving ? "Click again to disconnect and clear the boards" : "Disconnect"}
              title={
                leaving
                  ? "Click again to disconnect and clear the boards"
                  : "Disconnect erases this session and every board drawing"
              }
            >
              <LogOut aria-hidden="true" />
            </button>
          </div>
        </div>

        {error && (
          <div className="banner" role="alert">
            <AlertTriangle aria-hidden="true" /> {error}
          </div>
        )}

        {/* THE CONVERSATION AND THE BOARD ARE PEERS. The first layout floated the panel over the
          * canvas, and a real screenshot showed it burying node cards, the attention line and the
          * dock. `.split` puts the task's group chat (`.talk`) beside the canvas (`.scene`); only
          * a node's own window layers, and it layers over the canvas it describes. */}
        {addingProject ? (
          <AddProject onClose={() => setAddingProject(false)} />
        ) : draft !== null ? (
          <div className="split">
            <aside className="talk">
              <Composer
                choice={routes}
                busy={busy}
                error={composeError}
                onSend={(objective, route) => void sendDraft(objective, route)}
                onCancel={discardDraft}
              />
            </aside>
            <div className="scene">
              <Board
                model={draftModel}
                board={board}
                selectedNode={focus.kind === "node" ? focus.id : null}
                onSelectNode={(id) =>
                  setFocus(id === null ? { kind: "none" } : { kind: "node", id })
                }
                onChange={setBoard}
                connectionNote="Nothing has run yet. This node starts the task."
                connectionTone="none"
                graphFile=""
                onGraphFileChange={() => {}}
                onDrawConnections={() => {}}
                busy={busy}
              />
            </div>
          </div>
        ) : selected === "" ? (
          <div className="board-empty">
            <p>This board is empty.</p>
            <button type="button" className="act" onClick={startDraft}>
              start a task
            </button>
          </div>
        ) : status === null ? (
          <p className="loading">
            <LoaderCircle className="spin" aria-hidden="true" /> opening this run…
          </p>
        ) : (
          <div className="split">
            {talkOpen && (
            <aside className="talk">
              {/* WHY IT NEEDS YOU, where you answer it. This block lived in the top strip -
                * a header narrating a panel it did not belong to. Each reason still carries
                * the one action that is legal for it. */}
              {verdict?.key === "needs" && status && status.attentionReasons.length > 0 && (
                <div className="attention" aria-label="Why this run needs you">
                  {status.attentionReasons.map((reason, reasonIndex) => {
                    const node = typeof reason.node === "string" ? reason.node : null;
                    const kind = typeof reason.kind === "string" ? reason.kind : "unknown";
                    const key = `${kind}:${node ?? ""}:${reasonIndex}`;
                    // The question is quoted ONCE. The ledger cannot say WHICH waiting node a
                    // question belongs to (the wire carries no link), and repeating the newest
                    // quote under every waiting reason attributed it to nodes it never came
                    // from - one of the two banners was necessarily wrong (round-4, 3am).
                    const firstWaiting = status.attentionReasons.findIndex(
                      (candidate) => candidate.kind === "waiting_input_node",
                    );
                    if (kind === "blocked_node" && node !== null) {
                      return (
                        <span key={key}>
                          {node} is blocked
                          <button
                            type="button"
                            disabled={busy}
                            onClick={() =>
                              void runMutation((client) =>
                                client.approve(selected, node, {
                                  actor: OPERATOR_ACTOR,
                                  idempotencyKey: newIdempotencyKey(),
                                }),
                              )
                            }
                          >
                            approve {node}
                          </button>
                        </span>
                      );
                    }
                    if (kind === "waiting_input_node" && node !== null) {
                      // A second waiting node says only what the record backs about IT.
                      if (reasonIndex !== firstWaiting) {
                        return <span key={key}>{node} is also waiting</span>;
                      }
                      // The debt, not just the debtor: quote the actual unanswered question
                      // when one is on the record, and confess when none is - demanding "an
                      // answer" to nothing sent a real person hunting the thread for a
                      // question that did not exist.
                      const asker = pendingQuestion?.asker ?? null;
                      const answers = pendingQuestion?.signalId ?? null;
                      return (
                        <span key={key}>
                          {pendingQuestion !== null && asker !== null ? (
                            <>
                              {asker} asked you:{" "}
                              <q className="why-quote">
                                {pendingQuestion.text.length > 240
                                  ? `${pendingQuestion.text.slice(0, 240)}…`
                                  : pendingQuestion.text}
                              </q>
                              <button
                                type="button"
                                onClick={() => {
                                  setSayTo(asker);
                                  // Carrying the question's id is what lets the reply SETTLE
                                  // the ledger: it retires debts only by replyTo.
                                  setSayAnswer({ asker, signalId: answers });
                                  setFocus({ kind: "run" });
                                  setSayFocusNonce((nonce) => nonce + 1);
                                }}
                              >
                                answer {asker}
                              </button>
                            </>
                          ) : (
                            <>
                              {node} is waiting, but nothing has asked you anything yet
                              <button
                                type="button"
                                onClick={() => {
                                  setSayTo(null);
                                  setSayAnswer(null);
                                  setFocus({ kind: "run" });
                                  setSayFocusNonce((nonce) => nonce + 1);
                                }}
                              >
                                answer in the thread
                              </button>
                            </>
                          )}
                        </span>
                      );
                    }
                    return (
                      <span key={key}>
                        {readable(kind)}
                        {node !== null ? ` · ${node}` : ""}
                      </span>
                    );
                  })}
                  {routes !== null &&
                    (!routes.configured || routes.routes.every((route) => !route.enabled)) && (
                      <span className="why-context">
                        {pendingQuestion === null
                          ? "Chat-only room: no model is wired here, so steps wait for people instead of thinking. Talking still works."
                          : "No model is wired to this Runtime, so agent nodes wait instead of thinking. The thread still works."}
                      </span>
                    )}
                </div>
              )}
              <RunPanel
                status={status}
                events={eventList}
                onClose={() => setTalkOpen(false)}
                openEvidence={openEvidence}
                onSay={(message, to) =>
                  void say(
                    message,
                    to,
                    "run",
                    // The receipt rides only while the message still goes to the asker the
                    // banner chose - a hand-retargeted message answers nothing by accident.
                    to !== null && sayAnswer !== null && to === sayAnswer.asker
                      ? sayAnswer.signalId
                      : null,
                  )
                }
                saying={saying === "run"}
                sayError={sayError?.via === "run" ? sayError.text : ""}
                sayFocus={sayFocusNonce}
                sayRecipient={sayTo}
                owed={owedCards}
              />
            </aside>
            )}
            {/* IN FLOW, NOT FLOATED: a conversation opens as its own column and the canvas
              * yields width. A window layered over the board buried blobs and bubbles - the
              * one thing this screen promised not to do. */}
            {(focus.kind === "talk" || focus.kind === "agent") && (
              <aside className="talk chat-col">
              {focus.kind === "talk" && (
                <TalkPanel
                  key={`talk-${focus.id}`}
                  talkId={focus.id}
                  label={talks.find((talk) => talk.key === focus.id)?.label ?? focus.id}
                  participants={talks.find((talk) => talk.key === focus.id)?.participants ?? []}
                  events={talkThread}
                  executionId={selected === "" ? undefined : selected}
                  openEvidence={openEvidence}
                  onClose={() => setFocus({ kind: "none" })}
                  onSay={
                    focus.id === "room"
                      ? (message, to) => void say(message, to, "talk")
                      : undefined
                  }
                  saying={saying === "talk"}
                  sayError={sayError?.via === "talk" ? sayError.text : ""}
                />
              )}

              {focus.kind === "agent" && (
                <AgentPanel
                  // KEYED BY WHO IT BELONGS TO: a prop change re-addressed a LIVE composer
                  // without remounting - agent A's half-typed draft stood one Enter from
                  // shipping to agent B (round-4).
                  key={`agent-${focus.id}`}
                  agentId={focus.id}
                  charter={personas[focus.id] ?? null}
                  events={eventList}
                  executionId={selected === "" ? undefined : selected}
                  openEvidence={openEvidence}
                  onClose={() => setFocus({ kind: "none" })}
                  onSay={(message, to) => void say(message, to, "agent")}
                  saying={saying === "agent"}
                  sayError={sayError?.via === "agent" ? sayError.text : ""}
                />
              )}
              </aside>
            )}

            <div className="scene">
            <Board
              model={model}
              board={board}
              selectedNode={focusedNode}
              onSelectNode={(id) => setFocus(id === null ? { kind: "none" } : { kind: "node", id })}
              onChange={updateBoard}
              connectionNote={topologyError || topologyNote(topology)}
              connectionTone={connectionTone}
              graphFile={graphFile}
              onGraphFileChange={(value) => {
                setGraphFile(value);
                // The path is the operator's own note about this run, remembered with the
                // board marks so re-opening the run re-verifies without re-typing.
                updateBoard({ ...board, graphFile: value });
              }}
              onDrawConnections={() => void drawConnections()}
              focusGraphFile={fileFocusNonce}
              busy={busy}
              runId={selected === "" ? undefined : selected}
              crew={crew}
              selectedAgent={focus.kind === "agent" ? focus.id : null}
              onSelectAgent={(id) => setFocus(id === null ? { kind: "none" } : { kind: "agent", id })}
              talks={talks}
              selectedTalk={focus.kind === "talk" ? focus.id : null}
              onSelectTalk={(id) => setFocus(id === null ? { kind: "none" } : { kind: "talk", id })}
            />

            {/* THE CREW STANDS ON THE BOARD ITSELF - draggable blobs the Board renders, so
              * agents and nodes share one scene. Selection still lives here. */}

            <div className="dock">
              <button
                type="button"
                onClick={() =>
                  void runMutation((client) =>
                    client.pause(selected, {
                      actor: OPERATOR_ACTOR,
                      idempotencyKey: newIdempotencyKey(),
                    }),
                  )
                }
                disabled={busy}
              >
                pause
              </button>
              <button
                type="button"
                className="act"
                onClick={() =>
                  void runMutation((client) =>
                    client.approve(selected, approveTarget, {
                      actor: OPERATOR_ACTOR,
                      idempotencyKey: newIdempotencyKey(),
                    }),
                  )
                }
                disabled={busy || approveTarget === ""}
              >
                {approveTarget === "" ? "nothing to approve" : `approve ${approveTarget}`}
              </button>
              {/* In plain dock text, not a tooltip: a disabled button never shows its title —
                * the exact channel the resume fix three lines down already condemned. */}
              {waitingInstead && (
                <p className="hint">
                  Nothing is blocked. A waiting node wants an answer in the thread, not an
                  approval.
                </p>
              )}
              <button
                type="button"
                onClick={() => {
                  // A control that names its own missing ingredient goes and fetches it: with
                  // no path, resume walks the person to the box instead of sitting disabled
                  // with its excuse in a tooltip a disabled button never shows.
                  if (graphFile.trim().length === 0) {
                    setFileFocusNonce((nonce) => nonce + 1);
                    return;
                  }
                  void runMutation((client) =>
                    client.resume(selected, graphFile.trim(), {
                      actor: OPERATOR_ACTOR,
                      idempotencyKey: newIdempotencyKey(),
                    }),
                  );
                }}
                disabled={busy}
                title={
                  graphFile.trim().length === 0
                    ? "resume re-reads the graph file — click to point at it"
                    : undefined
                }
              >
                resume
              </button>
            </div>

            {evidence && (
              <p className={`act-note ${evidence.result === "succeeded" ? "proven" : "refused"}`}>
                {({ approve: "Approved", pause: "Paused", resume: "Resumed", start: "Started" } as Record<string, string>)[
                  evidence.action
                ] ?? evidence.action}
                {evidence.node ? ` ${evidence.node}` : ""}{" — "}
                {evidence.result === "succeeded" ? "done" : evidence.result} (log {evidence.headBefore}{" "}
                → {evidence.headAfter ?? "?"})
                {evidence.actor.type !== "owner" ? ` · by ${evidence.actor.type}:${evidence.actor.id}` : ""}
                {evidence.result === "unknown"
                  ? " · the Runtime accepted it but the verification read failed — do not treat this as done"
                  : ""}
              </p>
            )}
            </div>

            {node !== null && (
              <aside className="talk node-col">
              {node !== null && (
                <NodePanel
                  node={node}
                  events={nodeThread}
                  onClose={() => setFocus({ kind: "none" })}
                  executionId={selected === "" ? undefined : selected}
                  openEvidence={openEvidence}
                />
              )}
              </aside>
            )}
          </div>
        )}
      </div>
    </div>
  );
}




