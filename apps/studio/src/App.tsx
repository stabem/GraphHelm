/**
 * GraphHelm Local Studio.
 *
 * FOUR COLUMNS. The rail on the left is PROJECTS - folders, with their runs inside. A selected run
 * opens three more: CHAT (question cards, threads, composer), the CANVAS (the live Team tab and a
 * Journey tab) and a RIGHT PANEL with the run's recent activity. The top bar carries the run name,
 * the needs-you beacon and the run's details; below 768 px one tab at a time shows a column. A
 * node's window and a bot's details open beside them, never over the canvas.
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

import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { AlertTriangle, LoaderCircle, LogOut, Menu, MessageSquare, RefreshCw } from "lucide-react";

import {
  DisconnectedError,
  MAX_NODE_TIMEOUT_SECONDS,
  MIN_NODE_TIMEOUT_SECONDS,
  RuntimeClient,
  RuntimeError,
  OPERATOR_ACTOR,
  newIdempotencyKey,
} from "./runtime/client";
import { devSession, type DevSession } from "./runtime/session";
import { workConversation } from "./runtime/work-conversation";
import { isSubagentLifecycleSignal } from "./runtime/subagents";
import { isRunTeamSignal, readRunTeam, type RunTeamReadModel } from "./runtime/run-team";
import { isClaudeTaskSignal, readClaudeTasks, type ClaudeTaskReadModel } from "./runtime/team-tasks";
import type {
  Briefing,
  ClaimEvidence,
  EventPage,
  EvidenceContent,
  ExecutionStatus,
  ExecutionSummary,
  ReplySuggestions,
  MutationEvidence,
  NativeChatSummary,
  NativeChatRequest,
} from "./runtime/types";
import { claimVerdict, clearVerdict, digestOf, openWaitSequence } from "./runtime/customs";
import type { AnswerOutcome } from "./components/answer";
import {
  registerStudioTools,
  type ModelContextLike,
  type RegisteredTools,
  type ToolActivity,
  type WebMcpAvailability,
} from "./webmcp/adapter";
import { buildGraphModel, conversationFor, pendingAcceptanceCount } from "./graph/model";
import { openQuestions } from "./graph/ledger";
import { topologyFromJournal, topologyNote, verifyTopology, type VerifiedTopology } from "./graph/topology";
import {
  clearBoards,
  emptyBoard,
  loadBoard,
  saveBoard,
  type BoardState,
} from "./graph/board";
import { Connect } from "./components/Connect";
import { Board } from "./components/board";
import { AgentPanel, NodePanel, RunPanel, resetPanelCaches, useEnvelopes, useNativePersonaLinks, usePersonas } from "./components/panel";
import type { DocumentReference } from "./components/deliveries";
import { DocumentEditor, type DocumentSaveRequest } from "./components/document-editor";
import { NativeChats } from "./components/native-chats";
import { MainChat } from "./components/main-chat";
import { TeamCanvas } from "./components/team-canvas";
import { GraphFileRow } from "./components/graph-file-row";
import { Beacon, NEEDS_YOU_ID, QuestionCards } from "./components/beacon";
import { HandoverCard } from "./components/handover-card";
import { RightPanel } from "./components/right-panel";
import { ChatColumn } from "./components/chat-column";
import { botKeyOf, teamLinks, teamModel } from "./runtime/team";
import { needsYou, type DraftItem, type StepItem } from "./runtime/needs-you";
import { buildHandover, readLastSeen, shouldShowHandover, writeLastSeen } from "./runtime/handover";
import { EVERYONE, chatThreads, describeActivity, namesOf, sealedNotesPending, unreadCounts } from "./runtime/threads";
import { ProjectRail } from "./components/rail";
import { Composer, type RouteChoice } from "./components/compose";
import { Models, type KeyDraft, type ProbeState, type RouteDraft, type SaveOutcome } from "./components/models";
import { AddProject } from "./components/addproject";
import { RAIL_MAX, RAIL_MIN, loadRailWidth, saveRailWidth } from "./rail-width";
import { DRAFT_NODE_ID, draftGraph, newExecutionId } from "./graph/draft";
import { isGeneratedRunId, readable, runLabel, verdictOf } from "./components/format";
import { dockReserve } from "./dock-reserve";
import { actionLegality, hasEnded } from "./components/legality";
import { loadProjectName, loadRemovedRuns, saveProjectName, saveRemovedRuns, validProjectName } from "./studio-preferences";

/** Phase 2 fills this from actor_alias records; until then no alias exists. */
const NO_ALIASES: Record<string, string> = {};

function rawSha256(digest: string): string {
  return digest.startsWith("sha256:") ? digest.slice("sha256:".length) : digest;
}

function nodeIdFromDraftPath(path: unknown): string | null {
  if (typeof path !== "string" || !path.startsWith("/spec/nodes/") || path.length <= "/spec/nodes/".length) return null;
  const encoded = path.slice("/spec/nodes/".length);
  if (encoded.includes("/")) return null;
  let decoded: string;
  try {
    decoded = encoded.replace(/~1/g, "/").replace(/~0/g, "~");
  } catch { return null; }
  if (decoded.includes("/") || !/^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/.test(decoded)) return null;
  return decoded;
}

/** Whether a typed budget is one the client (and the envelope schema behind it) will accept. */
function budgetSecondsLegal(typed: string | undefined): boolean {
  if (typed === undefined || typed.trim() === "") return false;
  const seconds = Number(typed);
  return Number.isSafeInteger(seconds) && seconds >= MIN_NODE_TIMEOUT_SECONDS && seconds <= MAX_NODE_TIMEOUT_SECONDS;
}

/** Big enough that the board sees the whole roster on a normal run, and still one page. */
/** The Runtime's own page ceiling (`MAX_EVENTS_LIMIT` in `serve/routes.rs`). Every page costs the
 * Runtime a fresh store open plus a full stream replay, so fewer, larger pages are what make a
 * big run open fast: 200 per page made a 5,000-event run cost 25 serial full replays. */
const EVENT_PAGE_SIZE = 1000;
/** How many tail pages are asked for at once once the first page has told us the head. */
const EVENT_PAGE_PARALLEL = 4;
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

  const [runActionsOpen, setRunActionsOpen] = useState(false);
  const [connected, setConnected] = useState(false);
  /** What the rail calls this folder. Named by the operator; absent, the rail says what it can. */
  const [project, setProject] = useState<string | null>(null);
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [projectIdentity, setProjectIdentity] = useState<string | null>(null);
  const [removedRuns, setRemovedRuns] = useState<string[]>([]);
  const removedRunsRef = useRef<string[]>([]);
  removedRunsRef.current = removedRuns;
  const [projectPreferenceNotice, setProjectPreferenceNotice] = useState("");
  const [openDocument, setOpenDocument] = useState<{executionId: string; reference: DocumentReference} | null>(null);
  const documentAttention = useRef<"clean" | "draft" | "pending_notice" | "uncertain_save">("clean");
  const documentDraftDirty = useRef(false);
  const documentSaving = useRef(false);
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
  const [talkOpen, setTalkOpen] = useState(false);
  const conversationToggleRef = useRef<HTMLButtonElement | null>(null);
  const [projectsOpen, setProjectsOpen] = useState(() => typeof window.matchMedia !== "function" || window.matchMedia("(min-width: 901px)").matches);

  const [executions, setExecutions] = useState<ExecutionSummary[]>([]);
  /** The rows currently on the rail, readable from the poll timer without joining its
   * dependency list - the refresh reads as many pages as the operator has loaded. */
  const executionsRef = useRef<ExecutionSummary[]>([]);
  /** Restored rows outside the explicitly paged prefix must not expand background paging. */
  const restoredOutsidePage = useRef(new Set<string>());
  useEffect(() => {
    executionsRef.current = executions;
  }, [executions]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [selected, setSelected] = useState("");
  const [governedActorByNode, setGovernedActorByNode] = useState<Record<string, string>>({});
  const [status, setStatus] = useState<ExecutionStatus | null>(null);
  // Loads, polls and verified mutations can settle out of order within one selection. A later
  // arrival is not necessarily a newer observation; identity guards alone do not protect it.
  const observeStatus = useCallback((next: ExecutionStatus) => {
    setStatus((previous) => previous !== null && previous.executionId === next.executionId &&
      previous.headSequence > next.headSequence ? previous : next);
  }, []);
  const [replySuggestions, setReplySuggestions] = useState<ReplySuggestions | null>(null);
  const [replyLoading, setReplyLoading] = useState(false);
  const [replyIssue, setReplyIssue] = useState<{ executionId: string; headSequence: number; text: string } | null>(null);
  const [judgeRoute, setJudgeRoute] = useState("");
  const [events, setEvents] = useState<EventPage | null>(null);
  const [evidence, setEvidence] = useState<MutationEvidence | null>(null);
  /** The selected run's briefing (#1063/#1077): its NAME and OBJECTIVE live here and nowhere
   * else public - both are sealed out of the event payloads. `null` while unread, when the
   * Runtime has no briefing route (older server, 404), or when the read failed: the page then
   * names the run by its id, as it always did, and raises no banner for an optional read. */
  const [briefing, setBriefing] = useState<Briefing | null>(null);
  /** Every briefing read this connection, by run id, so the rail can name a generated
   * `run-<uuid>` by its objective. Read once per row - the objective is declared at start and
   * never changes - and never re-read on the poll. */
  const [briefings, setBriefings] = useState<Record<string, Briefing | null>>({});
  /** Every briefing read this connection, settled or still in flight, keyed by run id. ONE
   * map for both: a selection that lands while the rail's prefetch of the same run is in
   * flight AWAITS that read instead of concluding "no briefing" (PR #1079 review, P2) - and a
   * re-selection is served from what was already read. */
  const briefingReads = useRef(new Map<string, Promise<Briefing | null>>());
  /** Set on the first 404: an older Runtime has no route, and asking it twenty more times per
   * page of the rail would be twenty more ways to learn the same thing. */
  const briefingRouteMissing = useRef(false);

  const [graphFile, setGraphFile] = useState("");
  const graphFileRef = useRef("");
  const verificationGeneration = useRef(0);
  /** #1083 F2: a fixture file on the Runtime host for a demonstration run's `resume`. Never
   * remembered with the board: it is a one-off input to one verb, not a note about the run. */
  const [fixtureFile, setFixtureFile] = useState("");
  /** #1083 F9: the height the docks cover at the scene's bottom (see `dockRef`). */
  const [dockReservePx, setDockReservePx] = useState<number | null>(null);
  const [topology, setTopology] = useState<VerifiedTopology | null>(null);
  const [topologyError, setTopologyError] = useState("");

  const [focus, setFocus] = useState<Focus>({ kind: "none" });
  const nodeFocusOrigin = useRef<HTMLElement | null>(null);
  /** A task being composed: an execution id reserved locally and nothing else. `null` means the
   * page is showing a real run. Held here rather than in the rail because the board and the panel
   * both render from it. */
  const [draft, setDraft] = useState<{ executionId: string } | null>(null);
  /** The draft, readable after an await WITHOUT a setState updater: the old sendDraft decided
   * "is this still my draft" inside `setDraft((current) => ...)`, and React runs that updater
   * during the next render, not at the call - so the flag it set was read before it was
   * written, the draft was cleared and NOTHING was selected: "This board is empty." right
   * after `start this task` (#1077, the judge's third MAJOR). */
  const draftRef = useRef<{ executionId: string } | null>(null);
  draftRef.current = draft;
  /**
   * #1098 D1: the draft's half-typed objective, owned HERE rather than inside the Composer.
   *
   * Selecting a run now leaves the draft (see `select`), which unmounts the composer. The text
   * the operator typed is the one thing on that surface that cannot be recovered from anywhere
   * else, so it survives the unmount and comes back the next time the composer opens. Only the
   * two controls that PROMISE to end the draft clear it: `discard`, and a verified send.
   */
  const [draftObjective, setDraftObjective] = useState("");
  /**
   * THE REST OF THE PARKED DRAFT (PR #1167 review, P1). A draft is not its text: it is an
   * execution id reserved locally, and `start-<executionId>` is the idempotency key every attempt
   * on that draft carries. Parking only the words meant the composer that reopened MINTED A NEW
   * ID, so the retry of an UNKNOWN start was a second start under a second identity - exactly the
   * duplicate the stable key exists to prevent, and the opposite of what the banner the operator
   * is reading at that moment promises. The composer's error travels with it for the same reason:
   * the sentence explaining why to resend must not be lost by looking away.
   *
   * A ref, not state: nothing renders from it. It is read at the next `startDraft` and by the
   * guards in `sendDraft`, both of which need the value AT THE CALL, not at the last render.
   */
  const parkedDraft = useRef<{
    executionId: string;
    error: string;
    pending: boolean;
    busyToken: symbol | null;
  } | null>(null);
  /** Busy belongs to one draft send. A late completion from a parked draft must not clear the
   * busy state of a newer composer or selected run. */
  const draftBusyToken = useRef<symbol | null>(null);
  /** Whether that draft operation currently owns the visible composer. Parking transfers the
   * pending operation, but its completion must not touch a different run's controls. */
  const draftBusyVisible = useRef(false);
  /** How wide the operator dragged the rail. Read once at mount, written on release. */
  const [railWidth, setRailWidth] = useState(loadRailWidth);
  const [addingProject, setAddingProject] = useState(false);
  /** The Runtime's own model list, read once per draft. `null` while it is being read. */
  const [routes, setRoutes] = useState<RouteChoice | null>(null);
  const judgeRoutes = routes?.routes.filter((route) =>
    route.enabled && route.provider === "typesafe" && route.transport === "direct_api",
  ) ?? [];
  const activeJudgeRoute = judgeRoutes.some((route) => route.id === judgeRoute)
    ? judgeRoute
    : judgeRoutes.length === 1 ? judgeRoutes[0].id : null;
  useEffect(() => {
    let superseded = false;
    setReplySuggestions(null);
    setReplyLoading(false);
    setReplyIssue(null);
    const id = status?.executionId;
    const head = status?.headSequence;
    if (!connected || !id || head === undefined || status?.attention !== "needs_you" || selected !== id) return;
    const setCurrentIssue = (text: string) => setReplyIssue({ executionId: id, headSequence: head, text });
    if (routes === null) {
      setCurrentIssue("Checking the available model routes…");
      return;
    }
    if (activeJudgeRoute === null) {
      setCurrentIssue(judgeRoutes.length === 0
        ? "Suggested replies aren't available because this Runtime has no Jev model set up. You can still send your own message."
        : "Choose a Jev model to prepare suggested replies.");
      return;
    }
    const client = clientRef.current;
    if (client === null) return;
    setReplyLoading(true);
    void client.getReplySuggestions(id, activeJudgeRoute).then((reply) => {
      if (superseded || clientRef.current !== client) return;
      if (reply === null) {
        setCurrentIssue("This Runtime does not offer recommended replies yet.");
      } else if (reply.executionId !== id || reply.headSequence !== head) {
        setCurrentIssue("The run changed while its replies were prepared. Waiting for a fresh reading.");
      } else {
        setReplySuggestions(reply);
      }
    }).catch((reason: unknown) => {
      if (!superseded && clientRef.current === client) {
        const detail = messageOf(reason, "Recommended replies could not be prepared.");
        setCurrentIssue(detail.includes("GRAPHHELM_GATEWAY_KEY")
          ? "Recommended replies are unavailable: the Runtime needs its model gateway key. You can still write your own message."
          : `Recommended replies are unavailable: ${detail}`);
      }
    }).finally(() => {
      if (!superseded && clientRef.current === client) setReplyLoading(false);
    });
    return () => { superseded = true; };
  }, [connected, selected, status?.executionId, status?.headSequence, status?.attention, routes, activeJudgeRoute]);
  // #1171: the models screen. `probes` starts empty on purpose - a route nobody has checked is
  // rendered "not checked", never green, because a dot that started green would be a claim
  // nobody measured.
  const [modelsOpen, setModelsOpen] = useState(false);
  const [modelsBusy, setModelsBusy] = useState(false);
  const [modelsError, setModelsError] = useState("");
  const [probes, setProbes] = useState<Record<string, ProbeState>>({});
  const modelsSaveGeneration = useRef(0);
  const probeGeneration = useRef(new Map<string, number>());
  const [composeError, setComposeError] = useState("");
  /** The composer's error readable at a call (see `parkedDraft`; `draftRef` for the shape). */
  const composeErrorRef = useRef("");
  composeErrorRef.current = composeError;
  /** The cancel button's in-place confirmation: destructive on an append-only log means no
   * undo, so the first press only asks. Per run - `select()` withdraws the question. */
  const [confirmCancel, setConfirmCancel] = useState(false);
  /** The operator's typed seconds per remedy node - a decision being composed, not a default. */
  const [remedySeconds, setRemedySeconds] = useState<Record<string, string>>({});
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
  const journalTopology = useMemo(() => topologyFromJournal(eventList), [eventList]);
  // A recorded snapshot is the run's own claim. If it fails verification, a
  // remembered file must not make the same run look connected anyway.
  const visibleTopology = journalTopology ?? topology;
  const model = useMemo(() => buildGraphModel(eventList, visibleTopology), [eventList, visibleTopology]);
  // A blocked node can mean several things. Only a recorded retryable failure supports calling
  // the owner's approval a retry; an older or incomplete journal keeps the generic wording.
  const retryFailures = new Map<string, { reason: string; executor: string | null }>();
  for (const candidate of model.nodes) {
    const latest = candidate.history.at(-1);
    if (candidate.state === "blocked" && latest?.outcome === "retryable_failure" && latest.reason) {
      retryFailures.set(candidate.id, { reason: latest.reason, executor: candidate.actualExecutor?.kind ?? null });
    }
  }
  const unverifiedResults = pendingAcceptanceCount(model.nodes, status?.nodeStateCounts.succeeded ?? 0);
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
      // FIRST PAGE ALONE: it tells us the head. A stream's sequences are contiguous, so every
      // remaining page's cursor is known up front and those pages are read in parallel batches
      // instead of one round-trip (and one full Runtime replay) after another.
      const first = await client.getEvents(id, { after: cursor, limit: EVENT_PAGE_SIZE });
      collected.head = first.head;
      collected.events.push(...first.events);
      if (first.events.length < EVENT_PAGE_SIZE) return collected;
      cursor = collected.events[collected.events.length - 1].sequence;
      for (let round = 0; round < 600 && cursor < collected.head; round += 1) {
        const cursors: number[] = [];
        for (let next = cursor; next < collected.head && cursors.length < EVENT_PAGE_PARALLEL; next += EVENT_PAGE_SIZE) {
          cursors.push(next);
        }
        const pages = await Promise.all(
          cursors.map((pageAfter) => client.getEvents(id, { after: pageAfter, limit: EVENT_PAGE_SIZE })),
        );
        let short = false;
        for (const [index, page] of pages.entries()) {
          // A page asked from a cursor PAST the collected tail would leave a hole (an earlier
          // page in the batch came back short or the stream is not contiguous): drop it and let
          // the next round resume from the real tail, so the result never has a gap in it.
          const tail = collected.events[collected.events.length - 1].sequence;
          if (cursors[index] > tail) break;
          collected.head = Math.max(collected.head, page.head);
          collected.events.push(...page.events.filter((event) => event.sequence > tail));
          if (page.events.length < EVENT_PAGE_SIZE) { short = true; break; }
        }
        const tail = collected.events[collected.events.length - 1].sequence;
        if (short || tail === cursor) break;
        cursor = tail;
      }
      return collected;
    },
    [],
  );

  /** The run the operator has selected RIGHT NOW, readable after an await. A slow read for a
   * previously selected run must not commit over the newly selected one: two fast rail clicks
   * inside one network round-trip left run A's data under run B's name (round-3 speedrunner). */
  const selectedRef = useRef("");

  /**
   * One briefing read, remembered. Returns `null` for "nothing to name it by": an older
   * Runtime (404 - and the route is then marked missing for this connection), a read that
   * failed, or a briefing with no objective. A failure here is swallowed on purpose: the
   * name is a nicety over the id, and a banner for it would be louder than the thing it
   * decorates. The connection guard is the same as every other completion's.
   */
  const readBriefing = useCallback((client: RuntimeClient, id: string): Promise<Briefing | null> => {
    if (briefingRouteMissing.current) return Promise.resolve(null);
    const pending = briefingReads.current.get(id);
    if (pending !== undefined) return pending;
    const read = (async () => {
      let value: Briefing | null = null;
      try {
        value = await client.getBriefing(id);
        if (value === null) briefingRouteMissing.current = true;
      } catch {
        value = null;
      }
      if (clientRef.current !== client) return null;
      setBriefings((previous) => (previous[id] === value ? previous : { ...previous, [id]: value }));
      return value;
    })();
    briefingReads.current.set(id, read);
    return read;
  }, []);

  const loadExecution = useCallback(
    async (id: string) => {
      const client = clientRef.current;
      if (!client || !id) return;
      setBusy(true);
      // Naming is optional. Keep its cached in-flight read, but never let its completion
      // publish a status/events snapshot captured before a newer poll or mutation.
      void readBriefing(client, id).then((nextBriefing) => {
        if (clientRef.current === client && selectedRef.current === id && nextBriefing !== null) {
          setBriefing(nextBriefing);
        }
      });
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
        observeStatus(nextStatus);
        // KEEP THE OLD IDENTITY WHEN NOTHING CHANGED. Everything derived from the events array
        // (envelopes, personas, bubbles) keys off its identity; a fresh-but-equal array made the
        // poll re-open every sealed envelope each tick, and could cancel the pass forever.
        setEvents((previous) =>
          previous !== null &&
          (previous.head > nextEvents.head ||
            (previous.head === nextEvents.head && previous.events.length === nextEvents.events.length))
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
    [readEvents, readBriefing, observeStatus],
  );

  /**
   * THE RAIL'S NAMES (#1077). A generated `run-<uuid>` whose index row carries no objective (an
   * older Runtime) is asked for its briefing once, in order, one at a time - cached by id, never
   * re-asked on a render. Since #1083 F7 the index row carries the declared objective itself, so
   * against a current Runtime this asks for nothing: every row, hand-named or generated, is named
   * from the one index read.
   */
  useEffect(() => {
    const client = clientRef.current;
    if (!connected || !client) return;
    const wanted = executions
      .filter((run) => run.objective === undefined && isGeneratedRunId(run.executionId))
      .map((run) => run.executionId)
      .filter((id) => !briefingReads.current.has(id));
    if (wanted.length === 0) return;
    void (async () => {
      for (const id of wanted) {
        if (clientRef.current !== client || briefingRouteMissing.current) return;
        await readBriefing(client, id);
      }
    })();
  }, [connected, executions, readBriefing]);

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
  const pollGeneration = useRef(0);
  useEffect(() => {
    if (!connected || selected === "") return;
    const generation = ++pollGeneration.current;
    let tickBusy = false;
    pollMisses.current = 0;
    setStale(false);
    const timer = setInterval(() => {
      const client = clientRef.current;
      if (!client || tickBusy) return;
      tickBusy = true;
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
          const loadedRows = executionsRef.current.filter((run) => !restoredOutsidePage.current.has(run.executionId)).length;
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
          if (pollGeneration.current !== generation || clientRef.current !== client || selectedRef.current !== selected) return;
          for (const run of listRows) restoredOutsidePage.current.delete(run.executionId);
          observeStatus(nextStatus);
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
          if (pollGeneration.current !== generation || clientRef.current !== client) return;
          pollMisses.current += 1;
          if (pollMisses.current >= 3) setStale(true);
        } finally {
          tickBusy = false;
        }
      })();
    }, pollIntervalMs);
    return () => {
      clearInterval(timer);
      tickBusy = false;
    };
  }, [connected, selected, pollIntervalMs, readEvents, observeStatus]);

  const loadList = useCallback(
    async (options: { append?: boolean; cursor?: string | null } = {}) => {
      const client = clientRef.current;
      if (!client) return [] as ExecutionSummary[];
      const page = await client.listExecutions({
        limit: LIST_PAGE_SIZE,
        after: options.cursor ?? undefined,
      });
      if (clientRef.current !== client) return [] as ExecutionSummary[];
      for (const run of page.executions) restoredOutsidePage.current.delete(run.executionId);
      setExecutions((previous) => {
        // A page-one refresh resets ordinary pagination, but an explicitly restored row
        // outside that prefix stays visible and excluded from background page expansion.
        if (!options.append) return [...page.executions, ...previous.filter((run) => restoredOutsidePage.current.has(run.executionId))];
        const known = new Set(previous.map((run) => run.executionId));
        return [...previous, ...page.executions.filter((run) => {
          if (known.has(run.executionId)) return false;
          known.add(run.executionId);
          return true;
        })];
      });
      setNextCursor(page.hasMore ? page.nextCursor : null);
      return page.executions;
    },
    [],
  );

  const closeProjectDocument = useCallback(() => {
    if (documentSaving.current) return false;
    const draftWarning = documentDraftDirty.current ? " Any unsaved text will also be discarded." : "";
    const warning = documentAttention.current === "uncertain_save"
      ? `This save may have committed. Closing abandons its retry key and any pending run notifications.${draftWarning} Close this project document?`
      : documentAttention.current === "pending_notice"
        ? `Run notices are still pending. Closing abandons their retry key.${draftWarning} Close this project document?`
        : documentAttention.current === "draft"
          ? "Close this project document? Unsaved text will be discarded."
          : "";
    if (warning && !window.confirm(warning)) return false;
    setOpenDocument(null);
    documentAttention.current = "clean";
    documentDraftDirty.current = false;
    documentSaving.current = false;
    return true;
  }, []);

  /** Selecting a run swaps the board and drops the proof with it. A verified topology is a claim
   * about ONE run; carrying it across would draw the previous run's shape over this one's nodes,
   * with its proof still showing. */
  const select = useCallback(
    (id: string) => {
      if (!closeProjectDocument()) return;
      // #1098 D1: leaving the draft is PART of selecting a run. The composer is chosen ahead of
      // the selection (`draft !== null ?` in the render), so a draft left standing kept its fake
      // one-node overview on screen under the selected run's header — through a second run click,
      // and clearable only by `discard`. The draft is SET ASIDE WHOLE, not thrown away: its words
      // above, and its identity and error here (PR #1167 review, P1). A parked draft that came
      // back under a fresh execution id would turn its own retry into a duplicate run.
      if (draftRef.current !== null) {
        parkedDraft.current = {
          executionId: draftRef.current.executionId,
          error: composeErrorRef.current,
          pending: draftBusyToken.current !== null,
          busyToken: draftBusyToken.current,
        };
      }
      setDraft(null);
      // React updates draftRef on render, not at this call site. Clear it before selecting the
      // newly-created run, otherwise select() would park the already-consumed draft again.
      draftRef.current = null;
      setComposeError("");
      draftBusyVisible.current = false;
      setBusy(false);
      selectedRef.current = id;
      setSelected(id);
      // A deselection starts no replacement read whose finally could release this flag.
      if (id === "") setBusy(false);
      setStatus(null);
      setEvents(null);
      // The name arrives with the run, never carried over from the last one: a run's objective
      // under another run's id would be the one lie this feature could tell.
      setBriefing(briefings[id] ?? null);
      setFocus({ kind: "none" });
      setTopology(null);
      setTopologyError("");
      verificationGeneration.current += 1;
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
      // An armed cancel confirmation and half-typed budget seconds are questions about ONE
      // run; carrying either across a switch would aim them at the wrong one.
      setConfirmCancel(false);
      setRemedySeconds({});
      const stored = loadBoard(id);
      setBoard(stored);
      // The path this board was pointed at last time comes back with the board, so the shape
      // check below can re-run without anyone re-typing a Runtime-host path. The check is armed
      // HERE, from the REMEMBERED path only — arming it from the live field made the first
      // typed character fire a verify on a one-letter path.
      setGraphFile(stored.graphFile);
      graphFileRef.current = stored.graphFile;
      setFixtureFile("");
      autoVerify.current =
        stored.graphFile.trim().length > 0 ? { id, path: stored.graphFile.trim() } : null;
      void loadExecution(id);
    },
    [loadExecution, briefings, closeProjectDocument],
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
    // The rail button stays mounted while the composer is visible. A second click during an
    // start or an UNKNOWN result must not mint a new execution id or replace the retry key of
    // the active draft; the existing composer already owns this operation.
    if (draftRef.current !== null) return;
    if (!client || !closeProjectDocument()) return;
    // A parked draft is RESUMED, never re-minted: same execution id, so `start-<executionId>`
    // still names the attempt the Runtime may already have (PR #1167 review, P1).
    const resumed = parkedDraft.current;
    parkedDraft.current = null;
    const executionId = resumed?.executionId ?? newExecutionId();
    setDraft({ executionId });
    draftRef.current = { executionId };
    draftBusyToken.current = resumed?.pending ? resumed.busyToken : null;
    draftBusyVisible.current = resumed?.pending === true;
    setBusy(resumed?.pending === true);
    setTalkOpen(true);
    selectedRef.current = "";
    setSelected("");
    setStatus(null);
    setEvents(null);
    setTopology(null);
    setTopologyError("");
    setComposeError(resumed?.error ?? "");
    setRoutes(null);
    setProbes({});
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
  }, [closeProjectDocument]);

  const discardDraft = useCallback(() => {
    setDraft(null);
    // The one control that promises erasure erases (#1098 D1). A selection only SETS the draft
    // aside; `discard` is what ends it, and a draft that outlived its own discard would be the
    // lie this recovery could tell. Its identity goes with its words: a discarded draft must not
    // hand its execution id to the next one (PR #1167 review, P1).
    setDraftObjective("");
    parkedDraft.current = null;
    setFocus({ kind: "none" });
    setComposeError("");
  }, []);

  /**
   * Says why a send did not land, TO THE DRAFT IT BELONGS TO. On screen that is the composer's
   * banner; for a draft a selection parked mid-flight it is the parked record, so the sentence
   * comes back with the draft instead of dying with the composer (PR #1167 review, P1). A send
   * whose draft is gone entirely - discarded, or superseded - says nothing: its banner would land
   * on whatever the operator is composing now.
   */
  const reportToDraft = useCallback((startedId: string, message: string) => {
    if (draftRef.current !== null && draftRef.current.executionId === startedId) {
      setComposeError(message);
      draftBusyToken.current = null;
      draftBusyVisible.current = false;
      setBusy(false);
      return;
    }
    if (parkedDraft.current?.executionId === startedId) {
      parkedDraft.current = { ...parkedDraft.current, error: message, pending: false, busyToken: null };
    }
  }, []);

  /**
   * Publishes the one-node graph and starts it, then switches the page to the run it created.
   *
   * The draft is cleared only AFTER the start is verified. A failed start leaves the operator's
   * text on screen where they can fix and resend it - clearing first would lose what they wrote to
   * a refusal they did not cause.
   */
  // #1171: the three writes the models screen makes. Each one re-reads the listing afterwards
  // rather than patching local state from the reply: the manifest is a FILE the Runtime owns and
  // another operator (or a CLI in another window) can have moved it between two of these calls.
  const refreshRoutes = useCallback(async () => {
    const client = clientRef.current;
    if (!client) return;
    try {
      const listing = await client.listRoutes();
      if (clientRef.current === client) setRoutes(listing);
    } catch {
      // A listing that cannot be re-read leaves the previous one on screen. It is stale, and the
      // next successful read replaces it; inventing an empty list here would report that the
      // operator just deleted every provider they have.
    }
  }, []);

  const saveModels = useCallback(async (draft: RouteDraft, key: KeyDraft | null): Promise<SaveOutcome> => {
    const client = clientRef.current;
    if (!client) return "route_saved_key_failed";
    const generation = connectionGeneration.current;
    const saveGeneration = ++modelsSaveGeneration.current;
    const isCurrentSave = () =>
      clientRef.current === client &&
      connectionGeneration.current === generation &&
      modelsSaveGeneration.current === saveGeneration;
    probeGeneration.current.set(draft.id, (probeGeneration.current.get(draft.id) ?? 0) + 1);
    for (const routeId of key?.usableBy ?? []) {
      probeGeneration.current.set(routeId, (probeGeneration.current.get(routeId) ?? 0) + 1);
    }
    setModelsBusy(true);
    setModelsError("");
    try {
      await client.setRoute(draft);
      // A route may have landed before the connection changed, but a requested key that was not
      // sent is never a saved operation. Keep the draft retryable and do not let this completion
      // paint the replacement Runtime's Models panel.
      if (!isCurrentSave()) return "route_saved_key_failed";
      if (key !== null) {
        try {
          await client.setCredential(key);
        } catch {
          if (!isCurrentSave()) return "route_saved_key_failed";
          await refreshRoutes();
          if (!isCurrentSave()) return "route_saved_key_failed";
          setProbes({});
          setModelsError("The route was saved, but the API key was not stored. Retry to store it.");
          return "route_saved_key_failed";
        }
      }
      if (!isCurrentSave()) return "route_saved_key_failed";
      await refreshRoutes();
      if (!isCurrentSave()) return "route_saved_key_failed";
      setProbes({});
      return "saved";
    } catch (error) {
      if (isCurrentSave()) {
        setModelsError(messageOf(error, "The model could not be saved."));
      }
      throw error;
    } finally {
      if (isCurrentSave()) setModelsBusy(false);
    }
  }, [refreshRoutes]);

  const probeRoute = useCallback(async (routeId: string) => {
    const client = clientRef.current;
    if (!client) return;
    const generation = connectionGeneration.current;
    const requestGeneration = (probeGeneration.current.get(routeId) ?? 0) + 1;
    probeGeneration.current.set(routeId, requestGeneration);
    setProbes((current) => ({ ...current, [routeId]: { state: "checking" } }));
    try {
      const reply = await client.probeRoute(routeId);
      if (clientRef.current !== client || connectionGeneration.current !== generation || probeGeneration.current.get(routeId) !== requestGeneration) return;
      setProbes((current) => ({
        ...current,
        [routeId]:
          reply.health === "available"
            ? { state: "available" }
            : { state: "refused", message: reply.health },
      }));
    } catch (error) {
      if (clientRef.current !== client || connectionGeneration.current !== generation || probeGeneration.current.get(routeId) !== requestGeneration) return;
      setProbes((current) => ({
        ...current,
        [routeId]: { state: "refused", message: messageOf(error, "the check could not run") },
      }));
    }
  }, []);
  const sendDraft = useCallback(
    async (objective: string, route: string | null) => {
      const client = clientRef.current;
      if (!client || draft === null) return;
      const startedId = draft.executionId;
      setBusy(true);
      const busyToken = Symbol("draft-start");
      draftBusyToken.current = busyToken;
      draftBusyVisible.current = true;
      setComposeError("");
      // The Runtime records the run before its model call finishes. A slow model must not keep
      // that real run hidden behind the draft until the HTTP start response finally returns.
      let observingStart = false;
      const startObserver = window.setInterval(() => {
        if (observingStart || clientRef.current !== client || draftRef.current?.executionId !== startedId) return;
        observingStart = true;
        void (async () => {
          try {
            const listing = await client.listExecutions({ limit: LIST_PAGE_SIZE });
            if (clientRef.current !== client || draftRef.current?.executionId !== startedId) return;
            if (!listing.executions.some((run) => run.executionId === startedId)) return;
            await loadList();
            if (clientRef.current === client && draftRef.current?.executionId === startedId) select(startedId);
          } catch {
            // The start response still owns errors. An index read can briefly miss the commit.
          } finally {
            observingStart = false;
          }
        })();
      }, Math.min(pollIntervalMs, 2000));
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
          reportToDraft(
            startedId,
            evidenceOfStart.diagnostics[0]?.message ?? "The Runtime refused to start this task.",
          );
          return;
        }
        // UNKNOWN keeps the draft. "Not proven" is not "done": clearing here would throw away
        // the objective and the retry identity for an action the client itself classified as
        // unconfirmed. The words stay on screen and resend is safe by the stable key above.
        if (evidenceOfStart.result === "unknown") {
          reportToDraft(
            startedId,
            "The start could not be verified. Nothing was lost - send again; the retry carries the same identity, so it cannot start a second run.",
          );
          return;
        }
        // Only the draft this send belongs to is cleared and selected: the completion of a
        // superseded send must not unmount whatever the operator is composing now, nor drag
        // the page away from it. Decided from the REF, synchronously - see draftRef: deciding
        // it inside a setState updater left the board empty after every start (#1077).
        const onScreen = draftRef.current !== null && draftRef.current.executionId === startedId;
        // A verified start consumed the sentence: it is the run's objective now, and restoring it
        // into the next composer would offer to start the same task twice (#1098 D1). This holds
        // whether the draft is still on screen or was PARKED by a selection that landed while the
        // start was in flight - the old guard read only the live draft, so a selection mid-start
        // left the consumed text waiting in the next composer (PR #1167 review, P1).
        if (onScreen || parkedDraft.current?.executionId === startedId) {
          setDraftObjective("");
          parkedDraft.current = null;
        }
        if (onScreen) {
          setDraft(null);
          draftRef.current = null;
          // The run it just made is the run on screen: selected, its overview open, the rail
          // row lit - not "pick a run, or start one" over the run that was just started.
          select(startedId);
        }
        // The act-note is set AFTER the selection: `select` clears the previous run's evidence
        // (a note must never follow the operator across runs), and setting it first meant the
        // "Started — done" line was wiped by the very selection it announced (#1077 review).
        if (onScreen) setEvidence(evidenceOfStart);
        await loadList();
      } catch (reason) {
        if (clientRef.current !== client) return;
        reportToDraft(startedId, messageOf(reason, "The task could not be started."));
      } finally {
        window.clearInterval(startObserver);
        if (
          clientRef.current === client &&
          draftBusyToken.current === busyToken &&
          draftBusyVisible.current &&
          draftRef.current?.executionId === startedId
        ) {
          draftBusyToken.current = null;
          draftBusyVisible.current = false;
          setBusy(false);
        }
      }
    },
    [draft, loadList, select, reportToDraft, pollIntervalMs],
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
      const generation = ++verificationGeneration.current;
      const isCurrent = () => clientRef.current === client && selectedRef.current === forRun
        && verificationGeneration.current === generation && graphFileRef.current.trim() === file;
      setBusy(true);
      setTopologyError("");
      try {
        const read = await client.getTopology(file);
        if (!isCurrent()) return;
        setTopology(verifyTopology(eventList, read, file));
      } catch (reason) {
        if (!isCurrent()) return;
        setTopology(null);
        setTopologyError(messageOf(reason, "That graph file could not be read."));
      } finally {
        if (isCurrent()) setBusy(false);
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
      if (next.statusAfter) observeStatus(next.statusAfter);
      // An agent that acted on a node brings that node's window up, so the person sees the same
      // detail the agent was working on rather than a whole-run view they have to search.
      setFocus(next.node === null ? { kind: "run" } : { kind: "node", id: next.node });
      void loadExecution(next.executionId);
      void loadList().catch(() => {});
    },
    onActivity: (next: ToolActivity) => setActivity(next),
  };

  /**
   * Answering a parked node (#1186): claim, then clear, then re-read.
   *
   * TWO WRITES, AND THE SECOND ONE CAN FAIL AFTER THE FIRST LANDED. A claim is spent whether or
   * not its clearance arrives — the wait it answered is no longer open to a second claim — so a
   * clear that throws must NOT surface as "nothing happened". It returns `unknown` carrying the
   * claim's sequence, and the form tells the person to re-open the node rather than retry into a
   * `duplicate_completion` refusal.
   *
   * THE VERDICTS COME FROM THE JOURNAL, never from the HTTP result: both verbs answer 200 when
   * they refuse, because a refusal is a recorded decision in this design rather than a transport
   * error. `claimVerdict` and `clearVerdict` are where that rule lives.
   */
  const answerNode = useCallback(
    async (node: string, waitSeq: number, evidence: ClaimEvidence[]): Promise<AnswerOutcome> => {
      const client = clientRef.current;
      const execution = selectedRef.current;
      const file = graphFile.trim();
      // BOUND AT THE PRESS, not armed earlier: `graphFile` is a dependency, so the callback the
      // button holds is rebuilt whenever the box changes and a press always carries what the box
      // says now. That is the opposite arrangement from `verifyPath`, which must NOT read the
      // live box — its read is armed long before it fires, and a half-typed path drawing edges
      // was the defect there. Here the person presses after typing, and the path they can see is
      // the path that travels.
      // Thrown, not returned: nothing was sent, and every returned outcome describes something
      // that was. The form reports it as a Runtime it could not reach, which is what it is.
      if (!client || execution === "" || file === "") {
        throw new Error("no runtime, no run, or no graph file");
      }
      const claimed = await client.claimNode(execution, { file, node, waitSeq, evidence });
      const verdict = claimVerdict(claimed);
      if (verdict.outcome === "refused") {
        void loadExecution(execution);
        return { step: "claim-refused", reasonCode: verdict.reasonCode };
      }
      if (verdict.outcome === "unknown") {
        void loadExecution(execution);
        return { step: "unknown", claimSeq: null };
      }
      let cleared: MutationEvidence;
      try {
        cleared = await client.clearClaim(execution, {
          file,
          claimSeq: verdict.claimSeq,
          evidence,
          node,
        });
      } catch {
        void loadExecution(execution);
        return { step: "unknown", claimSeq: verdict.claimSeq };
      }
      const clearance = clearVerdict(cleared, verdict.claimSeq);
      // The re-read happens on EVERY path, including the refusals: the journal moved in all of
      // them, and a board still showing the state from before the claim would be a stale screen
      // asserting a world that no longer exists.
      void loadExecution(execution);
      if (clearance.outcome === "cleared") return { step: "claimed-and-cleared" };
      if (clearance.outcome === "refused") {
        return { step: "clearance-refused", reasonCode: clearance.reasonCode };
      }
      return { step: "unknown", claimSeq: verdict.claimSeq };
    },
    [graphFile, loadExecution],
  );

  const openWith = useCallback(
    async (client: RuntimeClient) => {
      clientRef.current = client;
      setProbes({});
      probeGeneration.current.clear();
      const page = await client.listExecutions({ limit: LIST_PAGE_SIZE });
      // A disconnect while the opening read was in flight must not re-light the page: this
      // completion belongs to the connection it started, like every other (PR #467 review, P1).
      if (clientRef.current !== client) return;
      restoredOutsidePage.current.clear();
      // A new connection may be a different Runtime with the same ids: nothing it was told
      // about the last one's routes or objectives carries over.
      briefingReads.current.clear();
      briefingRouteMissing.current = false;
      setBriefings({});
      setBriefing(null);
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
      const visible = (row: ExecutionSummary) => !removedRunsRef.current.includes(row.executionId);
      let firstVisible = page.executions.find(visible);
      // The first page may contain several runs needing attention. Open the one whose log
      // changed most recently, so a quiet old run does not hide the conversation in progress.
      const mostRecentNeedsYou = (rows: ExecutionSummary[]) => rows
        .filter((row) => visible(row) && row.attention === "needs_you")
        .reduce<ExecutionSummary | undefined>((best, row) => {
          if (best === undefined) return row;
          const at = row.lastEventAt ? Date.parse(row.lastEventAt) : NaN;
          const bestAt = best.lastEventAt ? Date.parse(best.lastEventAt) : NaN;
          return Number.isFinite(at) && (!Number.isFinite(bestAt) || at > bestAt) ? row : best;
        }, undefined);
      let candidate = mostRecentNeedsYou(page.executions);
      let cursor = page.hasMore ? page.nextCursor : null;
      for (let hops = 0; candidate === undefined && cursor !== null && hops < 50; hops += 1) {
        const next = await client.listExecutions({ after: cursor, limit: LIST_PAGE_SIZE });
        if (clientRef.current !== client) return;
        firstVisible ??= next.executions.find(visible);
        candidate = mostRecentNeedsYou(next.executions);
        cursor = next.hasMore ? next.nextCursor : null;
      }
      const first = candidate ?? firstVisible;
      if (first) {
        selectedRef.current = first.executionId;
        setSelected(first.executionId);
        // A connection-level selection bypasses select(); it carries select()'s own resets for
        // the state that must never outlive a run switch (PR #662 review, P1).
        setConfirmCancel(false);
        setRemedySeconds({});
        const stored = loadBoard(first.executionId);
        setBoard(stored);
        setBriefing(null);
        setGraphFile(stored.graphFile);
        graphFileRef.current = stored.graphFile;
        verificationGeneration.current += 1;
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
  const connectionGeneration = useRef(0);

  const connect = useCallback(
    async (token: string, knownProject?: string | null, knownProjectPath?: string | null) => {
      connectionAttempted.current = true;
      const generation = ++connectionGeneration.current;
      pollMisses.current = 0;
      setStale(false);
      setConnecting(true);
      setError("");
      const client = createClient ? createClient(token) : new RuntimeClient(token);
      try {
        await client.health();
        if (connectionGeneration.current !== generation) { client.dispose(); return; }
        // A manual reconnect must establish its public identity again. A different bearer
        // cannot inherit the previous Runtime's browser preferences merely by sharing a tab.
        let identity = knownProject;
        let folder = knownProjectPath ?? null;
        if (identity === undefined) {
          const opened = await (session ?? devSession)().catch(() => null);
          identity = opened?.token === token ? opened.project : null;
          folder = opened?.token === token ? opened.projectPath ?? null : null;
        }
        if (connectionGeneration.current !== generation) { client.dispose(); return; }
        setProjectIdentity(identity);
        setProject(identity === null ? null : loadProjectName(`${location.origin}:${identity}`, identity));
        setProjectPath(folder);
        const removed = identity === null ? [] : loadRemovedRuns(`${location.origin}:${identity}`);
        removedRunsRef.current = removed;
        setRemovedRuns(removed);
        setProjectPreferenceNotice("");
        await openWith(client);
      } catch (reason) {
        if (connectionGeneration.current !== generation) { client.dispose(); return; }
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
        if (connectionGeneration.current === generation) setConnecting(false);
      }
    },
    [createClient, openWith, session],
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
      await connectRef.current(opened.token, opened.project, opened.projectPath);
    })();
    return () => {
      cancelled = true;
    };
  }, [session]);

  const disconnect = useCallback(() => {
    if (!closeProjectDocument()) return;
    connectionGeneration.current += 1;
    pollMisses.current = 0;
    setStale(false);
    setConnecting(false);
    toolsRef.current?.unregister();
    toolsRef.current = null;
    clientRef.current?.dispose();
    clientRef.current = null;
    clearBoards();
    // The button PROMISES erasure — and the module maps (drafts, opened sealed words) were
    // surviving it, restoring a half-typed draft from the "erased" session and able to serve
    // one store's words as another's (three round-4 reviewers).
    resetPanelCaches();
    parkedDraft.current = null;
    draftRef.current = null;
    draftBusyToken.current = null;
    draftBusyVisible.current = false;
    setDraft(null);
    setDraftObjective("");
    setComposeError("");
    setRoutes(null);
    setModelsBusy(false);
    setModelsError("");
    setProbes({});
    probeGeneration.current.clear();
    setBusy(false);
    setConnected(false);
    setProject(null);
    setProjectPath(null);
    setProjectIdentity(null);
    setRemovedRuns([]);
    removedRunsRef.current = [];
    setProjectPreferenceNotice("");
    setExecutions([]);
    restoredOutsidePage.current.clear();
    setNextCursor(null);
    setSelected("");
    setStatus(null);
    setEvents(null);
    setEvidence(null);
    setBriefing(null);
    setBriefings({});
    briefingReads.current.clear();
    briefingRouteMissing.current = false;
    // An armed cancel and half-typed budget seconds die with the session (PR #662 review, P1):
    // the next connection - possibly another Runtime - must open with nothing armed.
    setConfirmCancel(false);
    setRemedySeconds({});
    setActivity(null);
    setTools([]);
    setWebmcp("unavailable");
    setGraphFile("");
    graphFileRef.current = "";
    verificationGeneration.current += 1;
    setFixtureFile("");
    setTopology(null);
    setTopologyError("");
    setFocus({ kind: "none" });
    setBoard(emptyBoard());
    setOpenDocument(null);
    documentAttention.current = "clean";
    documentDraftDirty.current = false;
    documentSaving.current = false;
    setError("");
  }, [closeProjectDocument]);

  useEffect(
    () => () => {
      connectionGeneration.current += 1;
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

  // #1083 F9: THE DOCK'S RESERVE IS MEASURED. The overview's scroll box pads its content by the
  // height the docks actually cover at the scene's bottom (`--dock-reserve`), because the dock
  // wraps to as many rows as the width forces and a fixed reserve left cards behind it.
  //
  // MEASURED ON RESIZE, NEVER ON RENDER. A stable callback ref on each dock element attaches ONE
  // ResizeObserver to the docks themselves; the observer fires when a dock's box changes (it
  // wraps, a remedy row appears) and is disconnected when the last dock unmounts. A render that
  // changes nothing about the docks measures nothing.
  const dockElements = useRef(new Set<HTMLElement>());
  const dockObserver = useRef<ResizeObserver | null>(null);
  const measureDocks = useCallback(() => {
    const next = dockReserve(
      [...dockElements.current].map((dock) => ({
        height: dock.getBoundingClientRect().height,
        bottom: Number.parseFloat(getComputedStyle(dock).bottom) || 0,
      })),
    );
    setDockReservePx((current) => (current === next ? current : next));
  }, []);
  const dockRef = useCallback(
    (element: HTMLDivElement | null) => {
      if (element === null) return;
      dockElements.current.add(element);
      if (typeof ResizeObserver !== "undefined") {
        dockObserver.current ??= new ResizeObserver(measureDocks);
        dockObserver.current.observe(element);
      }
      measureDocks();
      // THE EXACT ELEMENT LEAVES (Codex on PR #1091). React 19 calls a ref callback's returned
      // cleanup for the element it attached, instead of calling the callback with `null`. The
      // earlier `null` branch could not tell which dock detached and skipped docks still
      // connected - and a remedy dock is still connected while its ref detaches, so it stayed in
      // the set and `--dock-reserve` kept its 174px-high cover after the remedy was gone.
      return () => {
        dockObserver.current?.unobserve(element);
        dockElements.current.delete(element);
        if (dockElements.current.size === 0) {
          dockObserver.current?.disconnect();
          dockObserver.current = null;
        }
        measureDocks();
      };
    },
    [measureDocks],
  );

  const runMutation = useCallback(
    async (
      perform: (client: RuntimeClient) => Promise<MutationEvidence>,
      onError?: (reason: unknown) => string | undefined,
    ) => {
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
        if (next.statusAfter) observeStatus(next.statusAfter);
        await loadExecution(next.executionId);
        await loadList();
      } catch (reason) {
        if (clientRef.current !== client || selectedRef.current !== forRun) return;
        setError(onError?.(reason) ?? messageOf(reason, "The action could not be completed."));
      } finally {
        if (clientRef.current === client && selectedRef.current === forRun) setBusy(false);
      }
    },
    [loadExecution, loadList, observeStatus],
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
  const [runTeamRead, setRunTeamRead] = useState<RunTeamReadModel | null>(null);
  const [claudeTaskRead, setClaudeTaskRead] = useState<ClaudeTaskReadModel | null>(null);
  const subagentEvidenceCache = useRef(new Map<string, Promise<EvidenceContent>>());
  useEffect(() => { subagentEvidenceCache.current.clear(); }, [selected]);
  useEffect(() => {
    if (!selected || events === null) {
      setRunTeamRead(null);
      setClaudeTaskRead(null);
      return;
    }
    let cancelled = false;
    const run = selected;
    const readEvidence = (executionId: string, evidenceId: string) => {
      const key = `${executionId}\u0000${evidenceId}`;
      let pending = subagentEvidenceCache.current.get(key);
      if (!pending) {
        pending = openEvidence(executionId, evidenceId);
        subagentEvidenceCache.current.set(key, pending);
      }
      return pending;
    };
    void readRunTeam(run, eventList, readEvidence)
      .then((result) => { if (!cancelled) setRunTeamRead(result); })
      .catch(() => { if (!cancelled) setRunTeamRead({ executionId: run, members: [], messages: [], rejected: 0, unavailable: true }); });
    void readClaudeTasks({ executionId: run, events: eventList, readEvidence })
      .then((result) => { if (!cancelled) setClaudeTaskRead(result); })
      .catch(() => { if (!cancelled) setClaudeTaskRead({ executionId: run, tasks: [], rejected: 1 }); });
    return () => { cancelled = true; };
  }, [selected, events, eventList, openEvidence]);

  const readProjectDocument = useCallback(async (reference: DocumentReference) => {
    const client = clientRef.current;
    if (!client || !openDocument) throw new Error("The project document session is closed.");
    return client.readDocument(openDocument.executionId, reference);
  }, [openDocument]);
  const saveProjectDocument = useCallback(async (reference: DocumentReference, edit: DocumentSaveRequest) => {
    const client = clientRef.current;
    if (!client || !openDocument) throw new Error("The project document session is closed.");
    return client.saveDocument(openDocument.executionId, reference, edit);
  }, [openDocument]);
  const openProjectDocument = useCallback((reference: DocumentReference) => {
    if (openDocument?.executionId === selectedRef.current &&
        openDocument.reference.projectId === reference.projectId &&
        openDocument.reference.evidenceId === reference.evidenceId &&
        openDocument.reference.index === reference.index) return;
    if (!closeProjectDocument()) return;
    setOpenDocument({executionId: selectedRef.current, reference});
  }, [closeProjectDocument, openDocument]);
  const documentAttentionChanged = useCallback((attention: "clean" | "draft" | "pending_notice" | "uncertain_save", draftDirty: boolean) => {
    documentAttention.current = attention;
    documentDraftDirty.current = draftDirty;
  }, []);
  const documentSavingChanged = useCallback((saving: boolean) => { documentSaving.current = saving; }, []);

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
    ): Promise<boolean> => {
      const client = clientRef.current;
      if (!client || selected === "") return false;
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
          return false;
        }
        if (evidence.result === "unknown") {
          // Deliberately not reported as sent. The append may well have landed, and saying "sent"
          // about something unverified is the one error that cannot be walked back later.
          fail(
            "The Runtime accepted this but it could not be confirmed in the log. Reload before sending it again.",
          );
          await loadExecution(selected);
          return false;
        }
        await loadExecution(selected);
        return true;
      } catch (reason) {
        fail(messageOf(reason, "The message could not be sent."));
        return false;
      } finally {
        if (clientRef.current === client && selectedRef.current === forRun) setSaying(null);
      }
    },
    [loadExecution, selected],
  );

  const linkNativePersona = useCallback(async (nodeId: string, chat: NativeChatSummary, charter: string) => {
    const client = clientRef.current;
    const executionId = selected;
    if (!client || executionId === "" || selectedRef.current !== executionId) throw new Error("The project is not connected to a selected execution.");
    const record = JSON.stringify({
      protocol: "graphhelm-native-persona-v1",
      executionId,
      nodeId,
      threadId: chat.id,
      title: chat.title,
      sourceDirectory: chat.projectDirectory,
      charter,
    });
    const evidence = await client.signal(executionId, record, { to: chat.id, kind: "native_persona_linked" });
    if (evidence.result !== "succeeded") throw new Error(evidence.diagnostics[0]?.message ?? "The persona link was not confirmed in the activity log.");
    if (clientRef.current !== client || selectedRef.current !== executionId) return;
    await loadExecution(executionId);
  }, [loadExecution, selected]);

  // The crew: personas chartered in this run's log plus the agents the thread has heard from.
  // Derived AT THIS LEVEL because the crew lives on the canvas, not inside the chat panel.
  const personas = usePersonas(eventList, selected === "" ? undefined : selected, openEvidence);
  const nativePersonaLinks = useNativePersonaLinks(eventList, selected === "" ? undefined : selected, openEvidence);
  const mainChatPersonas = useMemo(() => Object.values(nativePersonaLinks), [nativePersonaLinks]);
  // THE CONVERSATIONS, SORTED BY WHO IS TALKING TO WHOM. Three kinds, kept apart because reading
  // them mixed is what made the room illegible: the ROOM (said to nobody in particular - you are
  // in it), each PAIR of agents (their addressed exchange - you can read it, you are not in it),
  // and your DIRECT LINE with one agent (behind that agent's own blob, not here). Each room and
  // pair becomes a bubble standing on the board.
  const envelopes = useEnvelopes(eventList, selected === "" ? undefined : selected, openEvidence);
  const workMessages = useMemo(() => workConversation(selected, eventList, envelopes,
    runTeamRead?.executionId === selected ? runTeamRead : null), [selected, eventList, envelopes, runTeamRead]);
  const hasRecordedAgentWork = useMemo(() => {
    const agentSequences = new Set(eventList.filter((event) =>
      event.kind === "signal_recorded" && event.actorType === "agent").map((event) => event.sequence));
    return workMessages.some((message) => message.provenance === "stored" && agentSequences.has(message.sequence));
  }, [eventList, workMessages]);
  const recentActivity = useMemo(() => eventList
    .filter((event) => event.kind === "signal_recorded" && !isSubagentLifecycleSignal(event) && !isClaudeTaskSignal(event) && !isRunTeamSignal(event))
    .slice(-5)
    .reverse()
    .map((event) => ({
      sequence: event.sequence,
      actorId: event.actorId,
      occurredAt: event.occurredAt,
      text: envelopes[event.sequence]?.text?.trim().slice(0, 220) || null,
    })), [eventList, envelopes]);
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

  // The armed cancel confirmation follows the RECOMPUTED legality: a poll tick or a WebMCP write
  // can finish the run while the question stands, and a "yes" that would only bounce off the
  // Runtime's refusal is withdrawn - the outer button then wears the reason (PR #662 review).
  // ABOVE the connect gate's early return, like every hook: a hook after a conditional return
  // took the whole page down (92 red cells in one run - measured, not imagined).
  const cancelIllegal = actionLegality(status).cancel !== undefined;
  useEffect(() => {
    if (cancelIllegal) setConfirmCancel(false);
  }, [cancelIllegal]);

  const durableProposals = useMemo(() => {
    const closedDraftIds = new Set(eventList.flatMap((event) => {
      if (event.kind !== "draft_rejected" && event.kind !== "draft_applied" && event.kind !== "mutation_accepted") return [];
      const payload = event.payload;
      if (payload === null || typeof payload !== "object") return [];
      const draftId = (payload as Record<string, unknown>).draftId;
      return typeof draftId === "string" ? [draftId] : [];
    }));
    return eventList.flatMap((event) => {
      if (event.kind !== "draft_proposed" || event.payload === null || typeof event.payload !== "object") return [];
      const value = event.payload as Record<string, unknown>;
      const draftId = typeof value.draftId === "string" ? value.draftId : null;
      const digest = typeof value.proposalSha256 === "string" ? value.proposalSha256 : null;
      if (draftId === null || digest === null || closedDraftIds.has(draftId)) return [];
      return [{ draftId, digest, evidenceId: event.evidenceRefs[0] ?? null }];
    });
  }, [eventList]);
  const [governedDraftId, setGovernedDraftId] = useState("");
  const selectedDraftProposal = durableProposals.find((proposal) => proposal.draftId === governedDraftId) ?? null;
  const [proposalReview, setProposalReview] = useState<{ draftId: string; state: "loading" | "ready" | "unavailable"; steps: string[]; edges: string[]; nodeIds: string[]; reason: string | null }>({ draftId: "", state: "unavailable", steps: [], edges: [], nodeIds: [], reason: null });
  useEffect(() => {
    const proposal = selectedDraftProposal;
    if (proposal === null || proposal.evidenceId === null) {
      setProposalReview({ draftId: proposal?.draftId ?? "", state: "unavailable", steps: [], edges: [], nodeIds: [], reason: "Select a sealed typed proposal to review." });
      return;
    }
    let cancelled = false;
    setProposalReview({ draftId: proposal.draftId, state: "loading", steps: [], edges: [], nodeIds: [], reason: null });
    void openEvidence(selected, proposal.evidenceId).then(async (evidence) => {
      if (cancelled) return;
      try {
        const openedDigest = await digestOf(new TextEncoder().encode(evidence.content).buffer, globalThis.crypto.subtle);
        if (rawSha256(openedDigest) !== rawSha256(proposal.digest)) throw new Error("the sealed proposal bytes do not match the recorded digest");
        const parsed: unknown = JSON.parse(evidence.content);
        const envelope = parsed !== null && typeof parsed === "object" ? parsed as Record<string, unknown> : null;
        const candidate = envelope?.proposal !== null && typeof envelope?.proposal === "object" ? envelope.proposal : envelope;
        const draft = candidate !== null && typeof candidate === "object" ? candidate as Record<string, unknown> : null;
        const operations = draft?.id === proposal.draftId && Array.isArray(draft.operations) ? draft.operations : null;
        if (operations === null || operations.length === 0 || operations.length > 64) throw new Error("typed proposal operations are unavailable");
        const steps: string[] = [];
        const edges: string[] = [];
        const nodeIds: string[] = [];
        for (const operation of operations) {
          if (operation === null || typeof operation !== "object") throw new Error("typed proposal operation is invalid");
          const value = operation as Record<string, unknown>;
          if (value.op === "addNode") {
            const id = nodeIdFromDraftPath(value.path) ?? (typeof value.id === "string" ? value.id : null);
            if (id === null) throw new Error("typed addNode path is invalid");
            nodeIds.push(id);
            const nodeValue = value.value !== null && typeof value.value === "object" ? value.value as Record<string, unknown> : null;
            const name = nodeValue !== null && typeof nodeValue.name === "string" ? nodeValue.name : null;
            const objective = nodeValue !== null && typeof nodeValue.objective === "string" ? nodeValue.objective : null;
            steps.push([id, name, objective].filter((part): part is string => part !== null && part.length > 0).join(" · "));
          }
          else if (value.op === "addEdge" && (value.edge !== null && typeof value.edge === "object" || value.value !== null && typeof value.value === "object")) {
            const edge = (value.edge ?? value.value) as Record<string, unknown>;
            if (typeof edge.from === "string" && typeof edge.to === "string") edges.push(`${edge.from} → ${edge.to}`);
          } else if (typeof value.op === "string") steps.push(value.op);
        }
        if (steps.length === 0 && edges.length === 0) throw new Error("typed proposal has no reviewable operations");
        setProposalReview({ draftId: proposal.draftId, state: "ready", steps, edges, nodeIds, reason: null });
      } catch (reason) {
        setProposalReview({ draftId: proposal.draftId, state: "unavailable", steps: [], edges: [], nodeIds: [], reason: reason instanceof Error ? reason.message : "The typed proposal descriptor is invalid." });
      }
    }).catch(() => {
      if (!cancelled) setProposalReview({ draftId: proposal.draftId, state: "unavailable", steps: [], edges: [], nodeIds: [], reason: "The sealed typed proposal descriptor could not be opened." });
    });
    return () => { cancelled = true; };
  }, [selectedDraftProposal?.draftId, selectedDraftProposal?.evidenceId, selected, openEvidence]);

  // THE LIVE TEAM (spec section 4): what needs the owner, who is on the team, the chat threads and
  // the "while you were away" card. All derived from the log; nothing here is a second source of truth.
  const [clock, setClock] = useState(() => Date.now());
  useEffect(() => { const timer = window.setInterval(() => setClock(Date.now()), 30_000); return () => window.clearInterval(timer); }, []);
  useEffect(() => { setClock(Date.now()); }, [eventList]);
  const [nativeRequests, setNativeRequests] = useState<NativeChatRequest[] | null>(null);
  useEffect(() => { setNativeRequests(null); }, [selected]);
  const nodeNames = useMemo(() => Object.fromEntries(model.nodes.flatMap((node) => node.declaredName ? [[node.id, node.declaredName] as const] : [])), [model]);
  const needs = useMemo(() => needsYou({ status, stale, events: eventList, envelopes, nativeRequests,
    pendingDraftIds: durableProposals.map((proposal) => proposal.draftId), nodeNames, operatorId: OPERATOR_ACTOR.id }),
    [status, stale, eventList, envelopes, nativeRequests, durableProposals, nodeNames]);
  const waitingAskers = useMemo(() => new Set(needs.items.flatMap((item) => item.kind === "question" ? [item.asker] : [])), [needs]);
  const team = useMemo(() => teamModel({ events: eventList, envelopes, personas, nativeLinks: nativePersonaLinks, aliases: NO_ALIASES, model,
    claudeTasks: claudeTaskRead?.executionId === selected ? claudeTaskRead : null, waitingAskers, now: clock }),
    [eventList, envelopes, personas, nativePersonaLinks, model, claudeTaskRead, selected, waitingAskers, clock]);
  const links = useMemo(() => teamLinks(eventList, envelopes, team.bots, clock), [eventList, envelopes, team, clock]);
  const botNames = useMemo(() => namesOf(team.bots), [team]);
  const threads = useMemo(() => chatThreads(workMessages, team.bots, OPERATOR_ACTOR.id), [workMessages, team]);
  const [thread, setThread] = useState(EVERYONE);
  const [threadOpened, setThreadOpened] = useState<Record<string, number>>({});
  useEffect(() => { setThread(EVERYONE); setThreadOpened({}); }, [selected]);
  useEffect(() => {
    const newest = threads.find((candidate) => candidate.key === thread)?.messages.at(-1)?.sequence ?? 0;
    setThreadOpened((current) => current[thread] === newest ? current : { ...current, [thread]: newest });
  }, [thread, threads]);
  const unread = useMemo(() => unreadCounts(threads, threadOpened), [threads, threadOpened]);
  const openingCount = useMemo(() => sealedNotesPending(eventList, envelopes), [eventList, envelopes]);
  const activityLines = useMemo(() => recentActivity.map((item) => describeActivity(item, botNames, envelopes, OPERATOR_ACTOR.id)), [recentActivity, botNames, envelopes]);
  const assignedNodeIds = useMemo(() => new Set(team.bots.flatMap((bot) => bot.tasks.flatMap((task) => task.nodeId === null ? [] : [task.nodeId]))), [team]);
  const unassignedSteps = useMemo(() => model.nodes.filter((node) => !assignedNodeIds.has(node.id)), [model, assignedNodeIds]);
  const nativeKeys = useMemo(() => new Set(Object.keys(nativePersonaLinks)), [nativePersonaLinks]);

  const [canvasTab, setCanvasTab] = useState<"team" | "journey">("team");
  const [mobileTab, setMobileTab] = useState<"chat" | "team" | "journeys">("team");
  const [graphFileOpen, setGraphFileOpen] = useState(false);
  const [answering, setAnswering] = useState<{ asker: string; signalId: string | null } | null>(null);
  const [composerFocus, setComposerFocus] = useState(0);
  const [highlight, setHighlight] = useState<number | null>(null);
  const [mainChatSeed, setMainChatSeed] = useState<{ text: string; nonce: number } | null>(null);
  const [nativeRefresh, setNativeRefresh] = useState(0);
  useEffect(() => { setAnswering(null); setHighlight(null); setMainChatSeed(null); setCanvasTab("team"); }, [selected]);
  const graphFileInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (fileFocusNonce > 0) {
      setCanvasTab("team");
      setMobileTab("team");
      setGraphFileOpen(true);
      window.setTimeout(() => graphFileInput.current?.focus(), 0);
    }
  }, [fileFocusNonce]);

  const projectKey = project ?? "this runtime";
  const [seenSeq, setSeenSeq] = useState<number | null>(null);
  useEffect(() => { setSeenSeq(selected === "" ? null : readLastSeen(projectKey, selected)); }, [projectKey, selected]);
  const head = status?.headSequence ?? 0;
  const handover = useMemo(() => status !== null && status.executionId === selected && shouldShowHandover(eventList, seenSeq, head)
    ? buildHandover({ events: eventList, bots: team.bots, model, claudeTasks: claudeTaskRead?.executionId === selected ? claudeTaskRead : null,
        openItems: needs.items, fromSeq: seenSeq!, toSeq: head })
    : null, [status, selected, eventList, seenSeq, head, team, model, claudeTaskRead, needs]);
  const markSeen = useCallback(() => {
    if (selected === "" || head === 0) return;
    writeLastSeen(projectKey, selected, head);
    setSeenSeq(head);
  }, [projectKey, selected, head]);
  useEffect(() => {
    // Ten visible seconds of the live view count as seen; an open handover waits for Got it.
    if (handover !== null || selected === "" || head === 0) return;
    const timer = window.setTimeout(() => { if (document.visibilityState === "visible") markSeen(); }, 10_000);
    return () => window.clearTimeout(timer);
  }, [handover, selected, head, markSeen]);

  const openRecords = (sequences: number[]) => {
    const first = sequences[0];
    if (first === undefined) return;
    const owner = threads.find((candidate) => candidate.messages.some((message) => message.sequence === first));
    if (owner !== undefined) { setThread(owner.key); setHighlight(first); setMobileTab("chat"); return; }
    setTalkOpen(true);
    setFocus({ kind: "run" });
  };
  const answerQuestion = (asker: string, signalId: string | null) => {
    const key = botKeyOf(team.bots, asker) ?? asker;
    setThread(`direct:${key}`);
    setAnswering({ asker, signalId });
    setComposerFocus((nonce) => nonce + 1);
    setMobileTab("chat");
  };
  const onRequestsChange = useCallback((requests: NativeChatRequest[]) => setNativeRequests(requests), []);

  if (!connected) {
    return <Connect onConnect={(token) => void connect(token)} busy={connecting} error={error} />;
  }

  const verdict = status ? verdictOf(status.attention) : null;
  const waitingForInput = status?.attentionReasons.some((reason) => reason.kind === "waiting_input_node") ?? false;
  const needsDirection = verdict?.key === "needs" && waitingForInput && pendingQuestion === null;
  const focusRunReply = () => {
    const asker = pendingQuestion?.asker ?? null;
    setTalkOpen(true);
    setSayTo(asker);
    setSayAnswer(asker === null ? null : { asker, signalId: pendingQuestion!.signalId });
    setFocus({ kind: "run" });
    setSayFocusNonce((nonce) => nonce + 1);
  };
  const pendingOwnerReview = durableProposals.length > 0;
  const currentReplyIssue = status !== null && replyIssue !== null && replyIssue.executionId === status.executionId && replyIssue.headSequence === status.headSequence ? replyIssue.text : null;
  const currentReplySuggestions = status !== null && replySuggestions !== null && replySuggestions.executionId === status.executionId && replySuggestions.headSequence === status.headSequence ? replySuggestions : null;
  const blockedAttentionNode = status?.attentionReasons.find((reason) =>
    (reason.kind === "blocked_node" || reason.kind === "untriaged_interruption") && typeof reason.node === "string")?.node ?? null;
  const effectiveAttention = pendingOwnerReview && status !== null ? "needs_you" : status?.attention;
  const effectiveVerdict = effectiveAttention === undefined ? null : verdictOf(effectiveAttention);
  const effectiveStatus = pendingOwnerReview && status !== null ? { ...status, attention: "needs_you" as const } : status;
  /** An entry graph's waiting bit is a technical fact, not an owner request or a claim that
   * external agents stopped. Keep other reasons, questions, and proposals prominent. */
  const recordedWorkBehindGraphWait = status?.status === "running" && status.attention === "needs_you"
    && waitingForInput && pendingQuestion === null && !pendingOwnerReview && blockedAttentionNode === null
    && retryFailures.size === 0
    && status.attentionReasons.every((reason) => reason.kind === "waiting_input_node")
    && hasRecordedAgentWork;
  const focusPendingProposal = () => {
    setGovernedDraftId((current) => current || durableProposals[0]?.draftId || "");
    setRunActionsOpen(true);
    setTalkOpen(false);
    setFocus({ kind: "run" });
  };
  /** What each dock verb may claim right now, and the reason for each it may not. */
  const legality = actionLegality(status);
  /** #1083: an ended run's dock drops pause, resume and cancel (see `hasEnded`). */
  const ended = hasEnded(status);
  /**
   * THE HEAD THE DOCK RENDERED rides as `If-Match` on every verb fired against the run on
   * screen (L's follow-up on #662, App.tsx:1615). The precondition exists so that "an interrupt
   * aimed at the run the operator SAW must not interrupt work that began after they looked"
   * (#681, #695) - and the operator's own button is exactly the caller that must supply the
   * head it displayed. Without it every click took the client's fallback, a fresher pre-read,
   * and the guard passed by construction. The value is the status THIS render painted for THIS
   * run - the observation the click is a judgement on - never a later poll's; the handlers
   * below close over it. Omitted (not fabricated) when nothing is rendered: those verbs are
   * disabled anyway. `start` (no stream exists yet) and `say` (a message answers by replyTo,
   * not by head; pinning it would refuse a reply whenever the run spoke since the last poll)
   * are the two verbs deliberately outside this rule.
   */
  const ifMatchRendered: { ifMatch?: number } =
    status !== null && status.executionId === selected && Number.isSafeInteger(status.headSequence)
      ? { ifMatch: status.headSequence }
      : {};
  /** The declareNodeBudget remedies the attention verdict itself offered. An entry whose
   * `computedAtSequence` is absent is NOT offered: a remedy that cannot be placed in the
   * history is refused by the API, and a button that walks into that refusal would pretend. */
  const budgetRemedies = (status?.silenceUnevaluated ?? []).flatMap((entry) => {
    if (entry === null || typeof entry !== "object") return [];
    const remedy = (entry as { remedy?: unknown }).remedy;
    if (remedy === null || typeof remedy !== "object") return [];
    const shaped = remedy as {
      remedy?: unknown;
      node?: unknown;
      observedSilenceSeconds?: unknown;
      computedAtSequence?: unknown;
    };
    if (shaped.remedy !== "declareNodeBudget") return [];
    if (typeof shaped.node !== "string" || shaped.node.length === 0) return [];
    if (typeof shaped.computedAtSequence !== "number") return [];
    return [{
      node: shaped.node,
      observedSilenceSeconds: typeof shaped.observedSilenceSeconds === "number" ? shaped.observedSilenceSeconds : 0,
      computedAtSequence: shaped.computedAtSequence,
    }];
  });
  const blocking =
    status?.attentionReasons
      .map((reason) => (typeof reason.node === "string" ? reason.node : null))
      .filter((candidate): candidate is string => candidate !== null) ?? [];
  // Two kinds are approvable, not one (PR #662 review, legality.ts:66): a node blocked for
  // cause, and a node an immediate pause INTERRUPTED (`untriaged_interruption`) - the Runtime's
  // `approve.rs:15` on the latter is the triage act `resume_preconditions` waits for, so it
  // is the one action that unblocks resume.
  const approvable =
    status?.attentionReasons
      .filter((reason) => reason.kind === "blocked_node" || reason.kind === "untriaged_interruption")
      .map((reason) => (typeof reason.node === "string" ? reason.node : null))
      .filter((candidate): candidate is string => candidate !== null) ?? [];
  const approveTarget =
    focusedNode !== null && approvable.includes(focusedNode) ? focusedNode : (approvable[0] ?? "");
  // A governed ghost must carry the exact proposal identity visible on its card. Do not fall back
  // to the latest draft or the legacy node-only approval body when the digest or assignment is
  // unavailable; that would approve a different proposal or hide the real blocker.
  const governedApprovalNode = (() => {
    const candidates = model.nodes.filter((candidate) => candidate.state === "ghost" && candidate.proposal?.status === "proposed");
    return focusedNode !== null && candidates.some((candidate) => candidate.id === focusedNode)
      ? candidates.find((candidate) => candidate.id === focusedNode) ?? null
      : candidates[0] ?? null;
  })();
  const governedActorChoices = [...new Set([
    ...eventList.filter((event) => event.actorType === "agent" && event.actorId !== null).map((event) => event.actorId as string),
    ...model.nodes.flatMap((candidate) => candidate.assignedActor?.type === "agent" ? [candidate.assignedActor.id] : []),
  ])].sort((left, right) => left.localeCompare(right));
  const governedApproval = (() => {
    const proposal = selectedDraftProposal;
    if (proposal === null || proposalReview.state !== "ready" || proposalReview.nodeIds.length === 0) return null;
    const assignments: Record<string, string> = {};
    for (const nodeId of proposalReview.nodeIds) {
      const candidate = model.nodes.find((node) => node.id === nodeId && node.proposal?.draftId === proposal.draftId);
      const actor = governedActorByNode[nodeId] ?? candidate?.assignedActor?.id ?? governedActorChoices[0];
      if (actor === undefined || actor.length === 0) return null;
      assignments[nodeId] = actor;
    }
    const selectedNode = governedApprovalNode?.id ?? proposalReview.nodeIds[0];
    return { node: { id: selectedNode }, draftId: proposal.draftId, proposalDigest: proposal.digest, assignments };
  })();
  const retryTarget = approveTarget !== "" && retryFailures.has(approveTarget);
  /** A running retry failure must be paused before its owner can allow another attempt. */
  const approvalNeedsPause = status?.status === "running" && approveTarget !== "";
  /** Governed drafts may be inspected at any time, but approval is only legal against a paused
   * Runtime observation with no node still running. The Runtime owns this state; Studio must not
   * pause or retry implicitly from the governed proposal button. */
  const observedNodeStates = status === null
    ? []
    : Object.values((status as ExecutionStatus & { nodeStates?: Record<string, unknown> }).nodeStates ?? {});
  const runningNodeCount = Math.max(
    status?.nodeStateCounts.running ?? 0,
    observedNodeStates.filter((state) => state === "running").length,
  );
  const governedApprovalAllowed = status?.status === "paused" && runningNodeCount === 0;
  const governedApprovalWaitReason = governedApprovalAllowed
    ? null
    : status?.status !== "paused"
      ? "Pause the run before approving this proposal."
      : runningNodeCount > 0
        ? "Approval is waiting for the running node to stop."
        : "Wait for the Runtime to report a paused state before approving this proposal.";
  const stalledAfterFailure = status?.status === "running" && retryFailures.size > 0 &&
    ["ready", "queued", "linting", "running"].every((state) => (status.nodeStateCounts[state] ?? 0) === 0);
  const waitingInstead = approveTarget === "" && blocking.length > 0;
  /** Reconstruct only Studio's generated one-node run. File-backed runs keep requiring their
   * original Runtime-host path, because objective text cannot safely stand in for topology. */
  const selectedSummary = executions.find((run) => run.executionId === selected);
  const resumeGraph =
    !graphFile.trim() &&
    isGeneratedRunId(selected) &&
    typeof (briefing?.objective ?? selectedSummary?.objective) === "string" &&
    (briefing?.objective ?? selectedSummary?.objective)?.trim().length !== 0
      ? draftGraph(selected, (briefing?.objective ?? selectedSummary?.objective) as string)
      : null;
  const snapshotResumeAvailable = (() => {
    if (graphFile.trim() !== "" || resumeGraph !== null || journalTopology?.match !== "matched") return false;
    const published = eventList
      .filter((event) => event.kind === "graph_version_published" && event.payload !== null && typeof event.payload === "object")
      .map((event) => (event.payload as { version?: { number?: unknown; topology?: { executionId?: unknown } } }).version)
      .filter((version): version is { number: number; topology?: { executionId?: unknown } } =>
        version !== undefined && typeof version.number === "number" && version.topology?.executionId === selected)
      .sort((left, right) => right.number - left.number);
    const current = published[0];
    return current !== undefined && eventList.some((event) => {
      if (event.kind !== "graph_authoring_snapshot_stored" || event.evidenceRefs.length === 0 || event.payload === null || typeof event.payload !== "object") return false;
      const payload = event.payload as Record<string, unknown>;
      return payload.executionId === selected && payload.graphVersion === current.number;
    });
  })();
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
    visibleTopology?.match === "matched"
      ? "proven"
      : topologyError !== "" || visibleTopology?.match === "mismatched"
        ? "refused"
        : "none";
  /** What the page says about the graph file check: how many nodes, whether any connection is
   * proven, and the check's own note. Shown under the Graph file field on the Team tab. */
  const connectionNote = [
    model.rosterDeclared
      ? `${model.nodes.length} node${model.nodes.length === 1 ? "" : "s"}`
      : `${model.nodes.length} node${model.nodes.length === 1 ? "" : "s"} seen so far, roster not read`,
    model.edgesKnown
      ? `${model.edges.length} connection${model.edges.length === 1 ? "" : "s"} drawn`
      : "work connections unverified",
    model.edgesKnown || connectionTone === "refused"
      ? (journalTopology ? topologyNote(journalTopology) : topologyError || topologyNote(visibleTopology))
      : "Verify the graph to connect work nodes.",
  ].join(" · ");
  const runtimePreferenceKey = projectIdentity === null ? null : `${location.origin}:${projectIdentity}`;
  const visibleExecutions = executions.filter((run) => !removedRuns.includes(run.executionId));
  const renameProject = (name: string): boolean => {
    const valid = validProjectName(name);
    if (valid === null) return false;
    if (runtimePreferenceKey === null) {
      setProject(valid);
      setProjectPreferenceNotice("Project name changed for this browser view only; this Runtime has no public project identity.");
      return true;
    }
    const saved = saveProjectName(runtimePreferenceKey, valid);
    if (!saved) {
      setProjectPreferenceNotice("Project name changed for this view only; browser storage is unavailable.");
      setProject(valid);
      return true;
    }
    setProject(valid);
    setProjectPreferenceNotice("");
    return true;
  };
  const removeRun = (id: string) => {
    const next = [...new Set([...removedRuns, id])];
    setRemovedRuns(next);
    if (runtimePreferenceKey === null) {
      setProjectPreferenceNotice("Removed from this browser view only; this Runtime has no public project identity.");
    } else if (!saveRemovedRuns(runtimePreferenceKey, next)) {
      setProjectPreferenceNotice("Removed from this view only; browser storage is unavailable or full.");
    }
    if (selected === id) select("");
  };
  const restoreRun = async (id: string) => {
    const client = clientRef.current;
    if (!client) return;
    if (!executionsRef.current.some((run) => run.executionId === id)) {
      try {
        // Read a bounded prefix to find the row. Keep the restore
        // entry until the Runtime has actually returned that run; an absent row is not success.
        let cursor: string | null = null;
        let restored: ExecutionSummary | undefined;
        for (let pageNumber = 0; pageNumber < 50; pageNumber += 1) {
          const page = await client.listExecutions({ limit: LIST_PAGE_SIZE, after: cursor ?? undefined });
          if (clientRef.current !== client) return;
          cursor = page.hasMore ? page.nextCursor : null;
          restored = page.executions.find((run) => run.executionId === id);
          if (restored || cursor === null) break;
        }
        if (!restored) {
          setProjectPreferenceNotice(cursor === null
            ? "This run is not in the Runtime list. It remains in the removed list; try restoring it again later."
            : "This run was not found in the first 50 pages. It remains in the removed list; try restoring it again later.");
          return;
        }
        // Restoring one run must not page every traversed row into the live rail: polling
        // follows its loaded row count. Preserve concurrent rows and the Show more cursor.
        const target = restored;
        if (!executionsRef.current.some((run) => run.executionId === id)) restoredOutsidePage.current.add(id);
        setExecutions((previous) => previous.some((run) => run.executionId === id)
          ? previous
          : [...previous, target]);
      } catch {
        if (clientRef.current === client) setProjectPreferenceNotice("The run could not be loaded. It remains in the removed list; try restoring it again.");
        return;
      }
    }
    const next = removedRunsRef.current.filter((runId) => runId !== id);
    removedRunsRef.current = next;
    setRemovedRuns(next);
    if (runtimePreferenceKey === null) {
      setProjectPreferenceNotice("Restored for this browser view only; this Runtime has no public project identity.");
    } else if (!saveRemovedRuns(runtimePreferenceKey, next)) {
      setProjectPreferenceNotice("Restored for this view only; browser storage is unavailable or full.");
    }
  };

  const threadsWithAnswer = answering !== null && !threads.some((candidate) => candidate.key === thread)
    ? [...threads, { key: thread, kind: "direct" as const, label: botNames[answering.asker] ?? answering.asker,
      participants: [botKeyOf(team.bots, answering.asker) ?? answering.asker], messages: [] }]
    : threads;

  const conversationVisible = talkOpen && (draft !== null || !["node", "agent", "talk"].includes(focus.kind));

  const onGraphFileChange = (value: string) => {
    graphFileRef.current = value;
    verificationGeneration.current += 1;
    setGraphFile(value);
    setTopology(null);
    setTopologyError("");
    setBusy(false);
    // The path is the operator's own note about this run, remembered with the
    // board marks so re-opening the run re-verifies without re-typing.
    updateBoard({ ...board, graphFile: value });
  };
  const chooseCanvas = (tab: "team" | "journey") => {
    setCanvasTab(tab);
    setMobileTab(tab === "team" ? "team" : "journeys");
  };
  /** The one legal action for a blocked or waiting step, or for a pending proposal. */
  const stepActions = (item: StepItem | DraftItem): ReactNode => {
    if (item.kind === "draft") return <button type="button" onClick={focusPendingProposal}>Review proposal</button>;
    if (item.kind === "waiting_step") {
      return <button type="button" onClick={pendingQuestion !== null ? () => answerQuestion(pendingQuestion.asker, pendingQuestion.signalId) : focusRunReply}>Answer in chat</button>;
    }
    const node = item.nodeId;
    const failure = item.reason === "blocked_node" ? retryFailures.get(node) : undefined;
    return <>
      {failure && <span className="question-failure">{`Failed after retries: ${readable(failure.reason)}. Check ${failure.executor === "model" ? "the model route" : "the failed step"} before allowing another attempt.`}</span>}
      <button
        type="button"
        disabled={busy}
        onClick={() =>
          void runMutation((client) =>
            status?.status === "running"
              ? client.pause(selected, {
                  actor: OPERATOR_ACTOR,
                  idempotencyKey: newIdempotencyKey(),
                  ...ifMatchRendered,
                })
              : client.approve(selected, node, {
                  actor: OPERATOR_ACTOR,
                  idempotencyKey: newIdempotencyKey(),
                  ...ifMatchRendered,
                }),
          )
        }
      >
        {status?.status === "running"
          ? `pause before ${failure ? "allowing retry" : "approving"} ${node}`
          : `${failure ? "allow retry" : "approve"} ${node}`}
      </button>
      {status?.status === "running" && (
        <small>Pause the running task first. Then approve this node.</small>
      )}
    </>;
  };

  return (
    <div className={`app ${projectsOpen ? "projects-open" : ""} ${conversationVisible ? "conversation-open" : ""}`} data-document-open={openDocument !== null} style={{ "--rail": `${railWidth}px` } as CSSProperties}>
      <ProjectRail
        projects={[{ name: project ?? "this runtime", path: projectPath, runs: visibleExecutions.map((run) => run.executionId === selected && pendingOwnerReview ? { ...run, attention: "needs_you" as const } : run) }]}
        selectedPresentation={recordedWorkBehindGraphWait ? { key: "calm", status: "agent work recorded · graph step waiting" } : null}
        selected={selected}
        connected={connected}
        stale={stale}
        hasMore={nextCursor !== null}
        busy={busy}
        onSelect={(id) => {
          setModelsOpen(false);
          select(id);
          if (typeof window.matchMedia === "function" && !window.matchMedia("(min-width: 901px)").matches) {
            setProjectsOpen(false);
            setTalkOpen(false);
          }
        }}
        onLoadMore={() => void loadList({ append: true, cursor: nextCursor })}
        onNewTask={() => {
          setModelsOpen(false);
          startDraft();
        }}
        onOpenModels={() => {
          if (!closeProjectDocument()) return;
          setModelsOpen(true);
        }}
        onAddProject={() => {
          if (!closeProjectDocument()) return;
          setModelsOpen(false);
          setAddingProject(true);
          setFocus({ kind: "none" });
        }}
        projectName={project ?? undefined}
        onRenameProject={renameProject}
        removedRuns={removedRuns}
        onRemoveRun={removeRun}
        onRestoreRun={restoreRun}
        briefings={briefings}
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
        <header className="topbar">
          <button type="button" className="ghost mobile-toggle projects-toggle" aria-expanded={projectsOpen} aria-controls="projects-rail" onClick={() => setProjectsOpen((open) => !open)} aria-label="Toggle projects"><Menu aria-hidden="true" /></button>
          <button type="button" className="ghost mobile-toggle conversation-toggle" ref={conversationToggleRef} disabled={addingProject || (!draft && selected === "")} aria-expanded={conversationVisible} aria-controls="conversation-panel" onClick={() => { setTalkOpen(!conversationVisible); if (draft === null && ["node", "agent", "talk"].includes(focus.kind)) setFocus({ kind: "none" }); }} aria-label="Toggle conversation"><MessageSquare aria-hidden="true" /></button>
          <div className="topbar-mission">
            {selected ? (<>
              {project && <span className="run-project">{project} {"\u00b7"} </span>}
              <button
                type="button"
                className="run-name"
                onClick={() => {
                  // Every nonce bump carries its own target: this button reopens the panel and
                  // speaks to the ROOM. Bumping without setting sayTo replayed a long-dead
                  // "answer X" choice - the round-1 bug back through a side door.
                  setSayTo(null);
                  setSayAnswer(null);
                  setTalkOpen(true);
                  setFocus({ kind: "run" });
                  setSayFocusNonce((nonce) => nonce + 1);
                }}
                // The objective as the name, the id on hover (#1077).
                title={selected}
              >
                {runLabel(selected, briefing)}
              </button>
            </>) : (
              <span className="meta">pick a run, or start one</span>
            )}
            {selected && <p className="run-selection-identity" aria-label="Selected run identity">Project: {project ?? "this runtime"} / Run: {selected}</p>}
          </div>
          {selected && status !== null && (
            <Beacon
              state={needs.state}
              onOpen={() => {
                setMobileTab("chat");
                window.setTimeout(() => {
                  const target = document.getElementById(NEEDS_YOU_ID);
                  target?.scrollIntoView?.({ block: "start" });
                  target?.focus();
                }, 0);
              }}
            />
          )}
          <details className="topbar-menu">
            <summary>Run details</summary>
            <div className="topbar-menu-body">
            {effectiveVerdict && <span className={`tag ${recordedWorkBehindGraphWait || needsDirection && !pendingOwnerReview && !blockedAttentionNode ? "calm" : status?.status === "completed" && (unverifiedResults > 0 || status.executor === "fixture") ? "needs" : effectiveVerdict.key}`}>{recordedWorkBehindGraphWait ? "agent work recorded · graph step waiting" : status?.status === "completed" && status.executor === "fixture" ? "demonstration completed · scripted outcomes" : unverifiedResults > 0 && status?.status === "completed" ? `execution completed · ${unverifiedResults} results need review` : needsDirection && !pendingOwnerReview && !blockedAttentionNode ? "graph waiting; no request recorded" : stalledAfterFailure ? "blocked · retry decision" : <>{status?.status && `${readable(status.status)} · `}{effectiveVerdict.label}</>}</span>}
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
            </div>
          </details>
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
        </header>

        {error && (
          <div className="banner" role="alert">
            <AlertTriangle aria-hidden="true" /> {error}
          </div>
        )}
        {projectPreferenceNotice !== "" && <p className="hint" role="status">{projectPreferenceNotice}</p>}

        {/* THE CONVERSATION AND THE BOARD ARE PEERS. The first layout floated the panel over the
          * canvas, and a real screenshot showed it burying node cards, the attention line and the
          * dock. `.split` puts the task's group chat (`.talk`) beside the canvas (`.scene`); only
          * a node's own window layers, and it layers over the canvas it describes. */}
        {modelsOpen ? (
          <Models
            choice={routes}
            busy={modelsBusy}
            error={modelsError}
            probes={probes}
            onApply={saveModels}
            onProbe={(routeId) => void probeRoute(routeId)}
            onClose={() => {
              setModelsOpen(false);
              setModelsError("");
            }}
          />
        ) : addingProject ? (
          <AddProject onClose={() => setAddingProject(false)} />
        ) : draft !== null ? (
          <div className="split">
            <aside id="conversation-panel" className="talk" hidden={!talkOpen}>
              <Composer
                choice={routes}
                busy={busy}
                error={composeError}
                onSend={(objective, route) => void sendDraft(objective, route)}
                onCancel={discardDraft}
                objective={draftObjective}
                onObjectiveChange={setDraftObjective}
              />
            </aside>
            <div className="scene">
              <Board
                model={draftModel}
                board={board}
                selectedNode={focus.kind === "node" ? focus.id : null}
                onSelectNode={(id) => {
                  if (id !== null) nodeFocusOrigin.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
                  setFocus(id === null ? { kind: "none" } : { kind: "node", id });
                }}
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
          <section className="loading" role="status">
            {error && !busy ? (
              <><strong>This run could not be opened.</strong><p>{error}</p><button type="button" onClick={() => void loadExecution(selected)}>Try again</button></>
            ) : (
              <><LoaderCircle className="spin" aria-hidden="true" /><strong>Opening this run…</strong><p>Reading its state and event history. Large runs may take a moment.</p></>
            )}
          </section>
        ) : (
          <div className="studio-columns" data-mobile-tab={mobileTab}>
            <div className="mobile-tabs" role="tablist" aria-label="Studio columns">
              {(["chat", "team", "journeys"] as const).map((tab) => (
                <button key={tab} type="button" role="tab" aria-selected={mobileTab === tab}
                  onClick={() => { setMobileTab(tab); if (tab !== "chat") setCanvasTab(tab === "team" ? "team" : "journey"); }}>
                  {tab === "chat" ? "Chat" : tab === "team" ? "Team" : "Journeys"}
                </button>
              ))}
            </div>
            <ChatColumn
              threads={threadsWithAnswer} selected={thread} onSelect={(key) => { setThread(key); setAnswering(null); setHighlight(null); }} unread={unread}
              bots={team.bots} names={botNames} openingCount={openingCount}
              cards={<QuestionCards items={needs.items} names={botNames} busy={busy || saying === "chat"}
                onChoose={(item, choice) => void say(choice, item.asker, "chat", item.signalId)}
                onAnswer={(item) => answerQuestion(item.asker, item.signalId)}
                onCheck={() => setNativeRefresh((nonce) => nonce + 1)}
                stepActions={stepActions} />}
              jev={{ suggestions: currentReplySuggestions?.state === "ready" ? currentReplySuggestions.suggestions : [], loading: replyLoading, issue: currentReplyIssue }}
              nativeKeys={nativeKeys}
              principal={<aside className="main-chat-rail" aria-label="Principal conversation">
                <MainChat key={`main-${selected}`} client={clientRef.current} executionId={selected} personas={mainChatPersonas}
                  refreshSequence={status.headSequence ?? 0} recipientId={thread.startsWith("direct:") ? thread.slice("direct:".length) : null}
                  seed={mainChatSeed} refreshNonce={nativeRefresh} onRequestsChange={onRequestsChange}
                  onConnect={() => {
                    const first = model.nodes[0];
                    if (first) setFocus({ kind: "node", id: first.id });
                  }} />
              </aside>}
              onSend={async (text, to, replyTo) => {
                // The Answering chip stays until the answer is confirmed, so a refused one can be retried.
                const ok = await say(text, to, "chat", replyTo);
                if (ok) setAnswering(null);
                return ok;
              }}
              sending={saying === "chat"} sendError={sayError?.via === "chat" ? sayError.text : ""}
              answering={answering} onClearAnswer={() => setAnswering(null)} composerFocus={composerFocus} highlight={highlight}
              onUseSuggestion={(text) => setMainChatSeed((current) => ({ text, nonce: (current?.nonce ?? 0) + 1 }))}
            />
            {talkOpen && (
            <aside id="conversation-panel" className="talk" hidden={!conversationVisible}>
              {status.attention === "needs_you" && judgeRoutes.length > 1 && (
                <label className="judge-route-choice">
                  Jev route
                  <select value={activeJudgeRoute ?? ""} onChange={(event) => setJudgeRoute(event.target.value)}>
                    <option value="">Choose a route</option>
                    {judgeRoutes.map((route) => <option key={route.id} value={route.id}>{route.model ?? route.id} · {route.id}</option>)}
                  </select>
                </label>
              )}
              <RunPanel
                status={effectiveStatus!}
                events={eventList}
                workMessages={workMessages}
                unverifiedResults={unverifiedResults}
                onClose={() => {
                  setTalkOpen(false);
                  window.setTimeout(() => conversationToggleRef.current?.focus(), 0);
                }}
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
                objective={briefing?.objective ?? null}
                replySuggestions={replySuggestions}
                replyLoading={replyLoading}
                replyIssue={currentReplyIssue}
                needsDirection={needsDirection}
                retryFailureNodes={[...retryFailures.keys()]}
                nodeNames={Object.fromEntries(model.nodes.flatMap((node) => node.declaredName ? [[node.id, node.declaredName] as const] : []))}
                proposalPending={pendingOwnerReview}
              />
            </aside>
            )}
            {/* IN FLOW, NOT FLOATED: a conversation opens as its own column and the canvas
              * yields width. A window layered over the board buried blobs and bubbles - the
              * one thing this screen promised not to do. */}
            {focus.kind === "agent" && (
              <aside className="talk chat-col">
              <AgentPanel
                  // KEYED BY WHO IT BELONGS TO: a prop change re-addressed a LIVE composer
                  // without remounting - agent A's half-typed draft stood one Enter from
                  // shipping to agent B (round-4).
                  key={`agent-${selected}-${focus.id}`}
                  agentId={focus.id}
                  charter={nativePersonaLinks[focus.id]?.charter ?? personas[focus.id] ?? null}
                  name={nativePersonaLinks[focus.id]?.chat.title}
                  nativeChats={nativePersonaLinks[focus.id] === undefined ? undefined : (
                    <NativeChats
                      client={clientRef.current}
                      executionId={selected}
                      nodeId={nativePersonaLinks[focus.id].nodeId}
                      boundChat={nativePersonaLinks[focus.id].chat}
                      personaCharter={nativePersonaLinks[focus.id].charter}
                      refreshSequence={status?.headSequence ?? 0}
                    />
                  )}
                  events={eventList}
                  executionId={selected === "" ? undefined : selected}
                  openEvidence={openEvidence}
                  onClose={() => setFocus({ kind: "none" })}
                  onSay={nativePersonaLinks[focus.id] === undefined ? (message, to) => void say(message, to, "agent") : undefined}
                  saying={saying === "agent"}
                  sayError={sayError?.via === "agent" ? sayError.text : ""}
              />
              </aside>
            )}

            <section className="canvas-column" aria-label="Canvas">
            <div className="canvas-tabs" role="tablist" aria-label="Canvas views">
              <button type="button" role="tab" aria-selected={canvasTab === "team"} onClick={() => chooseCanvas("team")}>Team (live)</button>
              <button type="button" role="tab" aria-selected={canvasTab === "journey"} onClick={() => chooseCanvas("journey")}>Journey</button>
            </div>
            <div
              className="scene"
              style={dockReservePx === null ? undefined : ({ "--dock-reserve": `${dockReservePx}px` } as CSSProperties)}
            >
            {canvasTab === "team" ? (
              <TeamCanvas storageKey={`graphhelm.team-positions:${projectKey}:${selected}`} bots={team.bots} otherRecorders={team.otherRecorders}
                links={links} unassignedSteps={unassignedSteps} selectedBot={thread.startsWith("direct:") ? thread.slice(7) : null}
                onSelectBot={(key) => { setThread(`direct:${key}`); setAnswering(null); }}
                onOpenBotDetails={(key) => setFocus({ kind: "agent", id: key })}
                onOpenNode={(id) => { nodeFocusOrigin.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; setFocus({ kind: "node", id }); }}
                onOpenTask={(bot, task) => task.nodeId !== null ? setFocus({ kind: "node", id: task.nodeId }) : openRecords([task.sequence])}
                graphFileOpen={graphFileOpen} onGraphFileOpenChange={setGraphFileOpen}
                graphFileRow={<><GraphFileRow graphFile={graphFile} onGraphFileChange={onGraphFileChange} onDrawConnections={() => void drawConnections()} busy={busy}
                  demonstration={status.executor === "fixture"} fixtureFile={fixtureFile} onFixtureFileChange={setFixtureFile} inputRef={graphFileInput} />
                  <p className={`connection-note ${connectionTone}`} role="status">{connectionNote}</p></>} />
            ) : (
              <section className="journey-empty" aria-label="Journey">
                <p>No journeys mapped yet. A journey appears here once its screens are captured.</p>
              </section>
            )}
            {handover !== null && <HandoverCard handover={handover} onOpen={openRecords} onDismiss={markSeen} />}

            {/* THE CREW STANDS ON THE BOARD ITSELF - draggable blobs the Board renders, so
              * agents and nodes share one scene. Selection still lives here. */}

            {/* THE DOCK (Phase 2, #105): every execution verb the API exposes that belongs on
              * this screen. A verb the current state makes illegal renders DISABLED with its
              * reason - the reason rides a wrapping span's title, because a disabled button
              * never shows its own (the channel the resume fix below already condemned), and
              * `actionLegality` is the one place that judgement lives. */}
            <div className="dock" ref={dockRef}>
              <span className="canvas-execution-state">Execution / {status.status ?? "State unavailable"}</span>
              <details className="execution-actions" open={runActionsOpen} onToggle={(event) => setRunActionsOpen(event.currentTarget.open)}>
              <summary>Run actions</summary>
              <div className="execution-menu">
              {!ended && (<>
              <span title={legality.pause ?? "Nothing new starts; work already in flight finishes and is joined — the run exits by quiescence."}>
                <button
                  type="button"
                  onClick={() =>
                    void runMutation((client) =>
                      client.pause(selected, {
                        actor: OPERATOR_ACTOR,
                        idempotencyKey: newIdempotencyKey(),
                        ...ifMatchRendered,
                      }),
                    )
                  }
                  disabled={busy || legality.pause !== undefined}
                >
                  pause · finish in-flight
                </button>
              </span>
              <span title={legality.pause ?? "Interrupts work in flight; every interrupted node is recorded before the pause folds."}>
                <button
                  type="button"
                  onClick={() =>
                    void runMutation((client) =>
                      client.pauseImmediately(selected, {
                        actor: OPERATOR_ACTOR,
                        idempotencyKey: newIdempotencyKey(),
                        ...ifMatchRendered,
                      }),
                    )
                  }
                  disabled={busy || legality.pause !== undefined}
                >
                  pause now · interrupt
                </button>
              </span>
              </>)}
              {durableProposals.length > 0 && (
                <label className="action-choice">
                  Draft to review
                  <select value={governedDraftId} onChange={(event) => setGovernedDraftId(event.target.value)} disabled={busy}>
                    <option value="">Select a sealed draft</option>
                    {durableProposals.map((proposal) => <option value={proposal.draftId} key={proposal.draftId}>{proposal.draftId} · {proposal.digest}</option>)}
                  </select>
                </label>
              )}
              {selectedDraftProposal !== null && proposalReview.state === "ready" && proposalReview.nodeIds.map((nodeId) => (
                  <label className="action-choice" key={`assignment-${nodeId}`}>
                    Responsible actor for {nodeId}
                    <select
                      value={governedActorByNode[nodeId] ?? model.nodes.find((candidate) => candidate.id === nodeId)?.assignedActor?.id ?? governedActorChoices[0] ?? ""}
                      onChange={(event) => setGovernedActorByNode((current) => ({ ...current, [nodeId]: event.target.value }))}
                      disabled={busy || governedActorChoices.length === 0}
                    >
                      {governedActorChoices.length === 0
                        ? <option value="">No observed agent is available</option>
                        : governedActorChoices.map((actorId) => <option value={actorId} key={actorId}>{actorId}</option>)}
                    </select>
                  </label>
                ))}
              {selectedDraftProposal !== null && (
                <section className="proposal-review" aria-label="Typed proposal review">
                  <strong>Review draft {selectedDraftProposal.draftId}</strong>
                  <span>Proposal digest · {selectedDraftProposal.digest}</span>
                  {proposalReview.state === "loading" && <span>Opening the sealed typed proposal…</span>}
                  {proposalReview.state === "unavailable" && <span>{proposalReview.reason ?? "The typed proposal descriptor is unavailable."}</span>}
                  {proposalReview.state === "ready" && <>
                    <span>Proposed nodes · {proposalReview.nodeIds.length} (shown separately from published work)</span>
                    <span>Proposed steps · {proposalReview.steps.join(", ") || "none"}</span>
                    <span>Proposed dependencies · {proposalReview.edges.join(", ") || "none"}</span>
                  </>}
                </section>
              )}
              <button
                type="button"
                className="act"
                onClick={() =>
                  void runMutation((client) =>
                    governedApproval !== null && proposalReview.state === "ready" && governedApprovalAllowed
                      ? client.approve(selected, governedApproval.node.id, {
                          governed: { draftId: governedApproval.draftId, proposalDigest: governedApproval.proposalDigest, assignments: governedApproval.assignments },
                          actor: OPERATOR_ACTOR,
                          idempotencyKey: newIdempotencyKey(),
                          ...ifMatchRendered,
                        })
                    : governedApproval !== null || durableProposals.length > 0
                      ? Promise.reject(new Error("Review the sealed typed proposal before approving it."))
                    : approvalNeedsPause
                      ? client.pause(selected, {
                          actor: OPERATOR_ACTOR,
                          idempotencyKey: newIdempotencyKey(),
                          ...ifMatchRendered,
                        })
                      : client.approve(selected, approveTarget, {
                          actor: OPERATOR_ACTOR,
                          idempotencyKey: newIdempotencyKey(),
                          ...ifMatchRendered,
                        }),
                  )
                }
                disabled={busy || (approveTarget === "" && governedApproval === null) || durableProposals.length > 0 && governedApproval === null || governedApproval !== null && !governedApprovalAllowed}
              >
                {governedApproval !== null
                  ? `approve proposal for ${governedApproval.node.id}`
                  : durableProposals.length > 0
                  ? "review selected proposal"
                  : approveTarget === ""
                  ? "nothing to approve"
                  : approvalNeedsPause
                    ? `pause before ${retryTarget ? "allowing retry" : "approving"} ${approveTarget}`
                    : `${retryTarget ? "allow retry" : "approve"} ${approveTarget}`}
              </button>
              {durableProposals.length > 0 && governedApproval === null && (
                <p className="hint">Select a sealed draft, review its typed nodes and dependencies, then choose a responsible actor for each node.</p>
              )}
              {governedApproval !== null && governedApprovalWaitReason !== null && (
                <p className="hint">{governedApprovalWaitReason}</p>
              )}
              {approvalNeedsPause && (
                <p className="hint">Pause the running task first. After it is paused, approve the node.</p>
              )}
              {/* In plain dock text, not a tooltip: a disabled button never shows its title —
                * the exact channel the resume fix three lines down already condemned. */}
              {waitingInstead && (
                <p className="hint">
                  Approval is not needed. This waiting node needs {needsDirection ? "a direction" : "an answer"} in the thread.
                </p>
              )}
              {!ended && (
              <span title={legality.resume}>
                <button
                  type="button"
                  onClick={() => {
                    // A control that names its own missing ingredient goes and fetches it: with
                    // no path, resume walks the person to the box instead of sitting disabled
                    // with its excuse in a tooltip a disabled button never shows.
                    if (graphFile.trim().length === 0 && resumeGraph === null && !snapshotResumeAvailable) {
                      setCanvasTab("team");
                      setMobileTab("team");
                      setGraphFileOpen(true);
                      setFileFocusNonce((nonce) => nonce + 1);
                      return;
                    }
                    void runMutation((client) =>
                      client.resume(selected, snapshotResumeAvailable ? undefined : resumeGraph ?? graphFile.trim(), {
                        actor: OPERATOR_ACTOR,
                        idempotencyKey: newIdempotencyKey(),
                        ...ifMatchRendered,
                        // #1083 F2: the API's existing `fixtures` field, the same path-on-the-
                        // Runtime-host trust the graph file already has - sent only for a
                        // demonstration run and only when the operator named a file. Without it a
                        // resumed fixture node has no outcome and parks `waiting_input`.
                        ...(status?.executor === "fixture" && fixtureFile.trim().length > 0
                          ? { fixtures: fixtureFile.trim() }
                          : {}),
                      }),
                      (reason) => {
                        if (
                          resumeGraph !== null &&
                          reason instanceof RuntimeError &&
                          /mismatch|version/i.test(`${reason.code} ${reason.message}`)
                        ) {
                          setFileFocusNonce((nonce) => nonce + 1);
                          return "The saved graph no longer matches this run. Point to the original graph file to resume safely.";
                        }
                        return undefined;
                      },
                    );
                  }}
                  disabled={busy || legality.resume !== undefined}
                  title={
                    graphFile.trim().length === 0 && resumeGraph === null
                      ? "resume re-reads the graph file — click to point at it"
                      : undefined
                  }
                >
                  resume
                </button>
              </span>
              )}
              <span title={legality.sweep ?? "Evaluate this run's customs stages now and journal the result."}>
                <button
                  type="button"
                  onClick={() =>
                    void runMutation((client) =>
                      client.sweep(selected, {
                        actor: OPERATOR_ACTOR,
                        idempotencyKey: newIdempotencyKey(),
                        ...ifMatchRendered,
                      }),
                    )
                  }
                  disabled={busy || legality.sweep !== undefined}
                >
                  sweep
                </button>
              </span>
              {/* CANCEL CONFIRMS IN PLACE. Destructive on an append-only log means no undo, so
                * the first press only ASKS - the verb fires from a second, explicit press, and
                * "keep running" backs out. No browser confirm(): the question and its answer
                * belong to the page, where a test can walk them. */}
              {ended ? (
                <p className="hint" role="status">
                  This run is {status.status}: nothing is left to pause, resume or cancel. Sweep
                  and messages are still accepted.
                </p>
              ) : confirmCancel ? (
                <span className="confirm-cancel">
                  <span>cancel this run? every unfinished node is recorded Cancelled — no undo.</span>
                  <button
                    type="button"
                    className="danger"
                    onClick={() => {
                      setConfirmCancel(false);
                      void runMutation((client) =>
                        client.cancel(selected, {
                          actor: OPERATOR_ACTOR,
                          idempotencyKey: newIdempotencyKey(),
                          ...ifMatchRendered,
                        }),
                      );
                    }}
                    disabled={busy || legality.cancel !== undefined}
                    title={legality.cancel}
                  >
                    yes, cancel it
                  </button>
                  <button type="button" onClick={() => setConfirmCancel(false)}>
                    keep running
                  </button>
                </span>
              ) : (
                <span title={legality.cancel ?? "Cancel this run — asks first; cancelling records every unfinished node as Cancelled."}>
                  <button
                    type="button"
                    className="danger"
                    onClick={() => setConfirmCancel(true)}
                    disabled={busy || legality.cancel !== undefined}
                  >
                    Cancel execution
                  </button>
                </span>
              )}
              </div>
              </details>
            </div>

            {/* THE VERDICT'S OWN REMEDY, offered where the verdict stands: each silence the
              * attention read could not judge for want of a declared budget arrives with a
              * declareNodeBudget remedy, and this is its socket. Seconds has NO default -
              * the API refuses to invent one and so does this surface. */}
            {budgetRemedies.length > 0 && (
              <div className="dock remedies" ref={dockRef}>
                {budgetRemedies.map((remedy) => (
                  <span key={remedy.node} className="remedy">
                    <span>
                      {remedy.node} has been silent {remedy.observedSilenceSeconds}s with no declared budget —
                    </span>
                    <input
                      type="number"
                      min={MIN_NODE_TIMEOUT_SECONDS}
                      max={MAX_NODE_TIMEOUT_SECONDS}
                      placeholder="seconds"
                      aria-label={`Silence budget for ${remedy.node}, in seconds`}
                      value={remedySeconds[remedy.node] ?? ""}
                      onChange={(event) =>
                        setRemedySeconds((previous) => ({ ...previous, [remedy.node]: event.target.value }))
                      }
                    />
                    {/* Input bounds AND the button derive from the schema's own bound (imported):
                      * a value the client would refuse must not be offered as a live action
                      * (PR #662 review, App.tsx:1743). The reason rides the button's title. */}
                    <button
                      type="button"
                      disabled={busy || !budgetSecondsLegal(remedySeconds[remedy.node])}
                      title={
                        budgetSecondsLegal(remedySeconds[remedy.node])
                          ? undefined
                          : `seconds must be an integer between ${MIN_NODE_TIMEOUT_SECONDS} and ${MAX_NODE_TIMEOUT_SECONDS} (the envelope schema's bound)`
                      }
                      onClick={() =>
                        void runMutation((client) =>
                          client.amendBudget(
                            selected,
                            {
                              node: remedy.node,
                              seconds: Number(remedySeconds[remedy.node]),
                              computedAtSequence: remedy.computedAtSequence,
                            },
                            { actor: OPERATOR_ACTOR, idempotencyKey: newIdempotencyKey(), ...ifMatchRendered },
                          ),
                        )
                      }
                    >
                      declare budget for {remedy.node}
                    </button>
                  </span>
                ))}
              </div>
            )}

            {evidence && (
              <p className={`act-note ${evidence.result === "succeeded" ? "proven" : "refused"}`}>
                {({ approve: "Approved", pause: "Paused", resume: "Resumed", start: "Started", cancel: "Cancelled", sweep: "Swept", amendBudget: "Budget declared for" } as Record<string, string>)[
                  evidence.action
                ] ?? evidence.action}
                {evidence.node ? ` ${evidence.node}` : ""}{" — "}
                {evidence.result === "succeeded" ? "done" : evidence.result} (log {evidence.headBefore}{" "}
                → {evidence.headAfter ?? "?"})
                {evidence.actor.type !== "owner" ? ` · by ${evidence.actor.type}:${evidence.actor.id}` : ""}
                {/* A refusal says WHY, in the Runtime's words: a bare "refused" next to a button
                  * the operator just pressed is a question, not an answer - and the If-Match
                  * conflict is the refusal a person can only act on if they are told the run moved. */}
                {evidence.result === "refused" && evidence.diagnostics[0] !== undefined
                  ? ` · ${evidence.diagnostics[0].message}`
                  : ""}
                {evidence.result === "unknown"
                  ? " · the Runtime accepted it but the verification read failed — do not treat this as done"
                  : ""}
              </p>
            )}
            </div>
            </section>

            {node !== null && (
              <aside className="talk node-col">
              {node !== null && (
                <>
                <NodePanel
                  node={node}
                  events={nodeThread}
                  onClose={() => {
                    setFocus({ kind: "none" });
                    window.setTimeout(() => {
                      if (nodeFocusOrigin.current?.isConnected) nodeFocusOrigin.current.focus();
                      nodeFocusOrigin.current = null;
                    }, 0);
                  }}
                  executionId={selected === "" ? undefined : selected}
                  openEvidence={openEvidence}
                  onOpenDocument={openProjectDocument}
                  answer={
                    // WITHHELD WHEN IT CANNOT WORK, rather than rendered and then failing: a
                    // claim must carry the graph (the Runtime reads the node's declared proof
                    // kinds from it), so with no path in the box there is nothing to offer. The
                    // panel renders nothing here and says nothing about whether the node waits.
                    graphFile.trim() === "" || selected === ""
                      ? undefined
                      : {
                          waitSeq: openWaitSequence(status, node.id),
                          onAnswer: (evidence) => {
                            const waitSeq = openWaitSequence(status, node.id);
                            // CORRECTED (#1187 review, lane B). This read was described as
                            // making the press "carry the wait that is open NOW, not the one that
                            // was open when the panel rendered". It cannot: it reads the same
                            // `status` from the same render closure as the `waitSeq` prop above,
                            // so the two are the same value by construction and neither is
                            // fresher than the render. A ref would not help either — React state
                            // cannot change without a re-render, and a re-render rebuilds this
                            // closure.
                            //
                            // WHAT IT ACTUALLY BUYS is that the sequence travelling to the
                            // Runtime is computed at this call site rather than threaded from
                            // elsewhere, so a future caller cannot pass a number from a different
                            // source without editing this line.
                            //
                            // WHERE A SUPERSEDED RENDEZVOUS IS ACTUALLY CAUGHT is the fold, not
                            // this screen: a claim naming a wait that is no longer open is
                            // refused as `stale_rendezvous`, and the node stays parked. The
                            // guarantee is the Runtime's; this line never had it.
                            if (waitSeq === null) {
                              return Promise.reject(new Error("no open wait"));
                            }
                            return answerNode(node.id, waitSeq, evidence);
                          },
                          hash: async (file: File) => ({
                            contentHash: await digestOf(
                              await file.arrayBuffer(),
                              globalThis.crypto.subtle,
                            ),
                            size: file.size,
                          }),
                        }
                  }
                  nativeChats={
                    <NativeChats
                      key={`native-${selected}-${node.id}`}
                      client={clientRef.current}
                      executionId={selected}
                      nodeId={node.id}
                      refreshSequence={status?.headSequence ?? 0}
                      onLinkPersona={(chat, charter) => linkNativePersona(node.id, chat, charter)}
                    />
                  }
                />
                </>
              )}
              </aside>
            )}
            <RightPanel activity={activityLines} onOpenActivity={(sequence) => openRecords([sequence])} />
            {openDocument && <DocumentEditor document={openDocument.reference}
              readDocument={readProjectDocument} saveDocument={saveProjectDocument}
              onAttentionChange={documentAttentionChanged}
              onSavingChange={documentSavingChanged}
              onClose={() => {
                if (documentSaving.current) return;
                setOpenDocument(null);
                documentAttention.current = "clean";
                documentDraftDirty.current = false;
                documentSaving.current = false;
              }} />}
          </div>
        )}
      </div>
    </div>
  );
}
