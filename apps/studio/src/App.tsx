import { recordReviewAssigned } from "./runtime/review-assign";
/**
 * GraphHelm Local Studio.
 *
 * The rail lists projects and runs. The canvas holds Team, Graph, and Proof; the right panel
 * shows recent activity when space allows. The top bar carries the run name and needs-you
 * beacon. A node's window and a bot's details open beside the canvas.
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
  MAX_NODE_TIMEOUT_SECONDS,
  MIN_NODE_TIMEOUT_SECONDS,
  RuntimeClient,
  RuntimeError,
  OPERATOR_ACTOR,
  newIdempotencyKey,
} from "./runtime/client";
import { devSession, type DevSession } from "./runtime/session";
import type { NativeChatRequest } from "./runtime/types";
import { workConversation } from "./runtime/work-conversation";
import { isSubagentLifecycleSignal } from "./runtime/subagents";
import { isRunTeamSignal } from "./runtime/run-team";
import { isClaudeTaskSignal, isTaskEventSignal } from "./runtime/team-tasks";
import { RUNTIME_PREDATES_TASKS_ROUTE } from "./runtime/tasks-route";
import { SlotPoll } from "./components/slot-poll";
import { MissionView, withFlowContracts } from "./components/mission-view";
import { applySessionEnds, laneBars, type TimedTaskEvent } from "./runtime/lane-bars";
import { skippedEdgeInto } from "./runtime/mission";
import { TaskGraphs } from "./components/task-graphs";
import { JEV_ABSENT_NOTE, jevAvailability } from "./runtime/jev";
import type {
  RuntimePreviousExit,
  Briefing,
  ClaimEvidence,
  EventPage,
  EvidenceContent,
  ExecutionStatus,
  ExecutionSummary,
  ReplySuggestions,
  MutationEvidence,
  NativeChatSummary,
} from "./runtime/types";
import { claimVerdict, clearVerdict, digestOf, openWaitSequence } from "./runtime/customs";
import type { AnswerOutcome } from "./components/answer";
import type { ModelContextLike, ToolActivity } from "./webmcp/adapter";
import { useWebMcp } from "./webmcp/use-webmcp";
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
import { AgentPanel, NodePanel, RunPanel, Thread, resetPanelCaches, useActorAliases, useEnvelopes, useNativePersonaLinks, usePersonas } from "./components/panel";
import type { DocumentReference } from "./components/deliveries";
import { DocumentEditor, type DocumentSaveRequest } from "./components/document-editor";
import { NativeChats } from "./components/native-chats";
import { TeamCanvas } from "./components/team-canvas";
import { DelegationTree } from "./components/delegation-tree";
import { GraphFileRow } from "./components/graph-file-row";
import { Beacon, NEEDS_YOU_ID, QuestionCards } from "./components/beacon";
import { HandoverCard } from "./components/handover-card";
import { RightPanel } from "./components/right-panel";
import { approvalBlockReason, beforeAfterPairs, captureDocuments, type BeforeAfterPair } from "./runtime/journeys";
import type { JourneyRunView } from "./runtime/types";
import { SaidLinks, type SaidLinkContext } from "./components/said";
import { teamLinks, teamModel } from "./runtime/team";
import { needsYou, type DraftItem, type StepItem } from "./runtime/needs-you";
import { buildHandover, hiddenThisSession, hideThisSession, readLastSeen, shouldShowHandover, writeLastSeen } from "./runtime/handover";
import { EVERYONE, describeActivity, namesOf } from "./runtime/threads";
import { ProjectRail } from "./components/rail";
import { Composer } from "./components/compose";
import { Models } from "./components/models";
import { AddProject } from "./components/addproject";
import { RAIL_MAX, RAIL_MIN, loadRailWidth, saveRailWidth } from "./rail-width";
import { useNow } from "./runtime/use-now";
import { sameValue } from "./runtime/same-value";
import { useExecutionPoll } from "./runtime/use-execution-poll";
import { useJourneys } from "./runtime/use-journeys";
import { messageOf } from "./runtime/message-of";
import { DRAFT_NODE_ID, draftGraph, newExecutionId } from "./graph/draft";
import { isGeneratedRunId, readable, runLabel, verdictOf } from "./components/format";
import { dockReserve } from "./dock-reserve";
import { actionLegality, hasEnded } from "./components/legality";
import { useProjectPrefs } from "./runtime/use-project-prefs";
import { useSay } from "./runtime/use-say";
import { useTeamReads } from "./runtime/use-team-reads";
import { useModelsRoutes } from "./runtime/use-models-routes";


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

type CanvasTab = "team" | "journey" | "graph";
const CANVAS_TAB_KEY = "graphhelm.studio.canvas-tab";
/** #583: the canvas tab the owner last chose this browser session, else Graph. */
function savedCanvasTab(): CanvasTab {
  if (journeyFromUrl()) return "journey";
  try {
    const saved = sessionStorage.getItem(CANVAS_TAB_KEY);
    if (saved === "team" || saved === "journey" || saved === "graph") return saved;
  } catch { /* storage refused: fall back to the default */ }
  return "graph";
}

export default function App({
  createClient,
  modelContext,
  session,
  pollIntervalMs = 4000,
}: AppProps = {}) {
  const clientRef = useRef<RuntimeClient | null>(null);
  /** Bumped by every connect and disconnect; a completion from an older connection discards itself. */
  const connectionGeneration = useRef(0);

  const [runActionsOpen, setRunActionsOpen] = useState(false);
  /** #1156: resume was pressed without a graph file; the field says what it is waiting for. */
  const [resumeNeedsFile, setResumeNeedsFile] = useState(false);
  // #1156: Escape closes the Run actions popover. A click elsewhere does not: a pending proposal
  // opens the menu on purpose, and selecting the run must not shut it again.
  useEffect(() => {
    if (!runActionsOpen) return undefined;
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") setRunActionsOpen(false); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [runActionsOpen]);
  /** #585: the step whose chat picker Main chat's "Connect existing chat" opened. */
  const [connected, setConnected] = useState(false);
  const [previousExit, setPreviousExit] = useState<RuntimePreviousExit | null>(null);
  const [startupTiming, setStartupTiming] = useState<{ journalVerifyMs: number; lifecycleStartMs: number } | null>(null);
  const [currentRun, setCurrentRun] = useState<Awaited<ReturnType<RuntimeClient["health"]>>["currentRun"]>(null);
  const [runtimeDetailsRefreshed, setRuntimeDetailsRefreshed] = useState(false);
  const [runtimeDetailsRefreshing, setRuntimeDetailsRefreshing] = useState(false);
  const [runtimeDetailsRefreshError, setRuntimeDetailsRefreshError] = useState("");
  const runtimeDetailsRefreshClient = useRef<RuntimeClient | null>(null);
  /** What the rail calls this folder. Named by the operator; absent, the rail says what it can. */
  const [openDocument, setOpenDocument] = useState<{executionId: string; reference: DocumentReference} | null>(null);
  const documentAttention = useRef<"clean" | "draft" | "pending_notice" | "uncertain_save">("clean");
  const documentDraftDirty = useRef(false);
  const documentSaving = useRef(false);
  const [connecting, setConnecting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
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
  const {
    project, projectPath, onDemandLanes, removedRuns, removedRunsRef, projectPreferenceNotice,
    openProject, resetProject, renameProject,
  } = useProjectPrefs({ clientRef, executionsRef, restoredOutsidePage, setExecutions, listPageSize: LIST_PAGE_SIZE });
  useEffect(() => {
    executionsRef.current = executions;
  }, [executions]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [selected, setSelected] = useState("");
  // #1169: the open run lives in the address (`?run=`), so a reload reopens it. An empty
  // selection before connect must not erase it; the draft path clears it on purpose.
  useEffect(() => { if (selected) rememberSelectedRun(selected); }, [selected]);
  // #1156: the resume prompt belongs to the run it was asked for.
  useEffect(() => { setResumeNeedsFile(false); }, [selected]);
  const [governedActorByNode, setGovernedActorByNode] = useState<Record<string, string>>({});
  const [status, setStatus] = useState<ExecutionStatus | null>(null);
  // Loads, polls and verified mutations can settle out of order within one selection. A later
  // arrival is not necessarily a newer observation; identity guards alone do not protect it.
  const observeStatus = useCallback((next: ExecutionStatus) => {
    // #503: a quiet tick answers with an equal status; keeping the old object keeps React still.
    setStatus((previous) => previous !== null && previous.executionId === next.executionId &&
      (previous.headSequence > next.headSequence || sameValue(previous, next)) ? previous : next);
  }, []);
  // #327: Jev's answer is keyed by the QUESTION it answers (execution + pending signal), not by the
  // head. A busy run moves its head every few seconds; keying by head restarted a ~30s request on
  // every tick, so "preparing" never finished.
  const [replySuggestions, setReplySuggestions] = useState<{ key: string; reply: ReplySuggestions } | null>(null);
  const [replyLoading, setReplyLoading] = useState(false);
  const [replyIssue, setReplyIssue] = useState<{ key: string; text: string } | null>(null);
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
  const {
    routes, setRoutes, judgeRoutes, activeJudgeRoute, setJudgeRoute,
    modelsOpen, setModelsOpen, modelsBusy, modelsError, setModelsError, probes, setProbes,
    clearProbes, resetModels, saveModels, probeRoute,
  } = useModelsRoutes({ clientRef, connectionGeneration });
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

  const { webmcp, tools, registerTools, unregisterTools } = useWebMcp(modelContext);
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
  const node = useMemo(() => {
    const current = model.nodes.find((candidate) => candidate.id === focusedNode);
    if (current === undefined) return null;
    // Clearance changes the Runtime projection without another node_outcome_recorded event.
    // Use its current state only when the status covers the journal shown in this panel.
    const state = status?.executionId === selected && status.headSequence >= (events?.head ?? 0)
      ? status.nodeStates?.[current.id]
      : undefined;
    return state === undefined || state === current.state ? current : { ...current, state };
  }, [model, focusedNode, status, selected, events?.head]);
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
          client.getStatus(id, true),
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

  const { stale, markFresh, readNativeRequests } = useExecutionPoll({
    connected, selected, pollIntervalMs, clientRef, selectedRef, eventsRef, executionsRef, restoredOutsidePage,
    listPageSize: LIST_PAGE_SIZE, readEvents, observeStatus, setEvents, setExecutions,
  });

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
      if (id === "") rememberSelectedRun("");
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
      resetSay();
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
    rememberSelectedRun("");
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
      const file = snapshotResumeAvailable ? undefined : graphFile.trim();
      // BOUND AT THE PRESS, not armed earlier: `graphFile` is a dependency, so the callback the
      // button holds is rebuilt whenever the box changes and a press always carries what the box
      // says now. That is the opposite arrangement from `verifyPath`, which must NOT read the
      // live box — its read is armed long before it fires, and a half-typed path drawing edges
      // was the defect there. Here the person presses after typing, and the path they can see is
      // the path that travels.
      // Thrown, not returned: nothing was sent, and every returned outcome describes something
      // that was. The form reports it as a Runtime it could not reach, which is what it is.
      if (!client || execution === "" || (!snapshotResumeAvailable && file === "")) {
        throw new Error("no runtime, no run, or no graph file");
      }
      const claimed = await client.claimNode(execution, { ...(file === undefined ? {} : { file }), node, waitSeq, evidence });
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
          ...(file === undefined ? {} : { file }),
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
    [graphFile, snapshotResumeAvailable, loadExecution],
  );

  const openWith = useCallback(
    async (client: RuntimeClient) => {
      clientRef.current = client;
      clearProbes();
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

      registerTools(client, {
        onSelect: (id) => hooksRef.current.onSelect(id),
        onMutation: (next) => hooksRef.current.onMutation(next),
        onActivity: (next) => hooksRef.current.onActivity(next),
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
      // #1696: a demonstration (fixture) run is never the project's view, so it is never the opening pick.
      const real = (row: ExecutionSummary) => visible(row) && row.executor !== "fixture";
      let firstVisible = page.executions.find(real);
      // The first page may contain several runs needing attention. Open the one whose log
      // changed most recently, so a quiet old run does not hide the conversation in progress.
      const mostRecentNeedsYou = (rows: ExecutionSummary[]) => rows
        .filter((row) => real(row) && row.attention === "needs_you")
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
        firstVisible ??= next.executions.find(real);
        candidate = mostRecentNeedsYou(next.executions);
        cursor = next.hasMore ? next.nextCursor : null;
      }
      // #1169: a reload reopens the run its address names, unless that run was removed here or
      // the Runtime does not know it; then the ordinary pick stands and the address forgets it.
      const wanted = selectedRunFromUrl();
      let first: { executionId: string } | undefined = candidate ?? firstVisible;
      if (wanted) {
        const known = visible({ executionId: wanted } as ExecutionSummary) &&
          (page.executions.some((row) => row.executionId === wanted) ||
            (await client.getStatus(wanted).then(() => true, () => false)));
        if (clientRef.current !== client) return;
        if (known) first = { executionId: wanted };
        else rememberSelectedRun("");
      }
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
    [loadExecution, registerTools],
  );

  /** True once ANY connection attempt has begun this page-life - the boot auto-connect yields
   * to it and never fires afterwards, deliberately including after a manual disconnect: a boot
   * continuation re-opening a session the operator just closed is the exact behavior the boot
   * effect's own comment forbids. */
  const connectionAttempted = useRef(false);

  const connect = useCallback(
    async (token: string, knownProject?: string | null, knownProjectPath?: string | null, knownOnDemand?: string[]) => {
      connectionAttempted.current = true;
      const generation = ++connectionGeneration.current;
      markFresh();
      setPreviousExit(null);
      setStartupTiming(null);
      setCurrentRun(null);
      setRuntimeDetailsRefreshed(false);
      setRuntimeDetailsRefreshing(false);
      setRuntimeDetailsRefreshError("");
      runtimeDetailsRefreshClient.current = null;
      setConnecting(true);
      setError("");
      const client = createClient ? createClient(token) : new RuntimeClient(token);
      try {
        const health = await client.health();
        if (connectionGeneration.current !== generation) { client.dispose(); return; }
        setPreviousExit(health?.previousExit ?? null);
        setStartupTiming(health?.startup ?? null);
        setCurrentRun(health?.currentRun ?? null);
        // A manual reconnect must establish its public identity again. A different bearer
        // cannot inherit the previous Runtime's browser preferences merely by sharing a tab.
        let identity = knownProject;
        let folder = knownProjectPath ?? null;
        let roster = knownOnDemand ?? [];
        if (identity === undefined) {
          const opened = await (session ?? devSession)().catch(() => null);
          identity = opened?.token === token ? opened.project : null;
          folder = opened?.token === token ? opened.projectPath ?? null : null;
          roster = opened?.token === token ? opened.onDemandLanes ?? [] : [];
        }
        if (connectionGeneration.current !== generation) { client.dispose(); return; }
        openProject(identity, folder, roster);
        await openWith(client);
      } catch (reason) {
        if (connectionGeneration.current !== generation) { client.dispose(); return; }
        // The tools too, not just the client: a failure AFTER registration (the first load can
        // throw) would otherwise leave the host holding registrations whose only removal handle
        // this overwrite discards - and the next connect's registerTool calls then collide with
        // them (PR #467 review). unregister() is idempotent and safe when nothing registered.
        unregisterTools();
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
      await connectRef.current(opened.token, opened.project, opened.projectPath, opened.onDemandLanes);
    })();
    return () => {
      cancelled = true;
    };
  }, [session]);

  const disconnect = useCallback(() => {
    if (!closeProjectDocument()) return;
    connectionGeneration.current += 1;
    markFresh();
    setConnecting(false);
    unregisterTools();
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
    resetModels();
    setBusy(false);
    setConnected(false);
    setPreviousExit(null);
    setStartupTiming(null);
    setCurrentRun(null);
    setRuntimeDetailsRefreshed(false);
    setRuntimeDetailsRefreshing(false);
    setRuntimeDetailsRefreshError("");
    runtimeDetailsRefreshClient.current = null;
    resetProject();
    setExecutions([]);
    restoredOutsidePage.current.clear();
    setNextCursor(null);
    setSelected("");
    // #1169: another token may open another project; its runs must not inherit this `?run=`.
    rememberSelectedRun("");
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

  const refreshRuntimeDetails = useCallback(async () => {
    const client = clientRef.current;
    const generation = connectionGeneration.current;
    if (!connected || !client || runtimeDetailsRefreshClient.current === client) return;
    runtimeDetailsRefreshClient.current = client;
    setRuntimeDetailsRefreshing(true);
    setRuntimeDetailsRefreshError("");
    try {
      const health = await client.health();
      if (clientRef.current !== client || connectionGeneration.current !== generation) return;
      setPreviousExit(health?.previousExit ?? null);
      setStartupTiming(health?.startup ?? null);
      setCurrentRun(health?.currentRun ?? null);
      setRuntimeDetailsRefreshed(true);
    } catch {
      if (clientRef.current !== client || connectionGeneration.current !== generation) return;
      setRuntimeDetailsRefreshError("Runtime details refresh failed. Details shown above were not updated.");
    } finally {
      if (clientRef.current === client && connectionGeneration.current === generation) {
        runtimeDetailsRefreshClient.current = null;
        setRuntimeDetailsRefreshing(false);
      }
    }
  }, [connected]);

  useEffect(
    () => () => {
      connectionGeneration.current += 1;
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

  /** One immutable evidence read per connection and id, shared by the task board and conversation:
   * the client keeps it (bounded, see `RuntimeClient.readEvidence`), so a new connection starts empty. */
  const openEvidence = useCallback(
    async (executionId: string, evidenceId: string): Promise<EvidenceContent> => {
      const client = clientRef.current;
      if (!client) throw new Error("This Studio session is not connected.");
      return client.readEvidence(executionId, evidenceId);
    },
    [],
  );
  const {
    runTeamRead, claudeTaskRead, taskGraphs, historyFailure, setHistoryRetry, tasksRouteMissing, tasksRouteFailure,
  } = useTeamReads({ clientRef, selected, events, eventList, openEvidence });

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

  const {
    saying, sayError, sayFocusNonce, setSayFocusNonce, sayTo, setSayTo, sayAnswer, setSayAnswer,
    resetSay, say, nameBot, refuse,
  } = useSay({ clientRef, selected, selectedRef, loadExecution });

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
  const actorAliases = useActorAliases(eventList, selected === "" ? undefined : selected, openEvidence);
  // THE CONVERSATIONS, SORTED BY WHO IS TALKING TO WHOM. Three kinds, kept apart because reading
  // them mixed is what made the room illegible: the ROOM (said to nobody in particular - you are
  // in it), each PAIR of agents (their addressed exchange - you can read it, you are not in it),
  // and your DIRECT LINE with one agent (behind that agent's own blob, not here). Each room and
  // pair becomes a bubble standing on the board.
  const envelopes = useEnvelopes(eventList, selected === "" ? undefined : selected, openEvidence);
  const workMessages = useMemo(() => workConversation(selected, eventList, envelopes,
    runTeamRead?.executionId === selected ? runTeamRead : null), [selected, eventList, envelopes, runTeamRead]);
  const hasRecordedAgentWork = useMemo(() => eventList.some((event) =>
    event.kind === "signal_recorded" && event.actorType === "agent"), [eventList]);
  const agentWorkPending = events === null;
  const recentActivity = useMemo(() => eventList
    .filter((event) => event.kind === "signal_recorded" && !isSubagentLifecycleSignal(event) && !isClaudeTaskSignal(event) && !isTaskEventSignal(event) && !isRunTeamSignal(event))
    .slice(-5)
    .reverse()
    .map((event) => ({
      sequence: event.sequence,
      actorId: event.actorId,
      occurredAt: event.occurredAt,
      // #442: never cut a document before it is parsed; describeActivity shortens what it prints.
      text: envelopes[event.sequence]?.text?.trim() || null,
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

  // #327: one request per pending question. The head is deliberately NOT a dependency: unrelated
  // appends must neither restart the request nor discard its answer. Failure is shown with a
  // Retry button; nothing retries on its own.
  const replyKey = status?.executionId ? `${status.executionId}\0${pendingQuestion?.signalId ?? "room"}` : "";
  const replyHead = useRef<number | undefined>(undefined);
  replyHead.current = status?.headSequence;
  useEffect(() => {
    let superseded = false;
    setReplySuggestions(null);
    setReplyLoading(false);
    setReplyIssue(null);
    const id = status?.executionId;
    if (!connected || !id || replyHead.current === undefined || status?.attention !== "needs_you" || selected !== id) return;
    const key = replyKey;
    const setCurrentIssue = (text: string) => setReplyIssue({ key, text });
    if (routes === null) {
      setCurrentIssue("Checking the available model routes…");
      return;
    }
    // #396: no judge route is a state, not an error. The chat shows no Jev card (there is
    // nothing to retry); run details carries JEV_ABSENT_NOTE instead.
    if (jevAvailability(judgeRoutes.length) === "absent") return;
    if (activeJudgeRoute === null) {
      setCurrentIssue("Choose a Jev model to prepare suggested replies.");
      return;
    }
    const client = clientRef.current;
    if (client === null) return;
    setReplyLoading(true);
    void client.getReplySuggestions(id, activeJudgeRoute).then((reply) => {
      if (superseded || clientRef.current !== client) return;
      if (reply === null) {
        setCurrentIssue("This Runtime does not offer recommended replies yet.");
      } else if (reply.executionId !== id) {
        setCurrentIssue("Jev answered for a different run.");
      } else if (reply.state === "unavailable") {
        setCurrentIssue(`Jev could not prepare a suggestion${reply.reason ? `: ${reply.reason}` : "."}`);
      } else {
        setReplySuggestions({ key, reply });
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
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the head is read through a ref on purpose (#327)
  }, [connected, selected, replyKey, status?.attention, routes, activeJudgeRoute]);

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
  const ticking = useNow(30_000);
  const grown = useMemo(() => Date.now(), [eventList]);
  const clock = Math.max(ticking, grown);
  const nodeNames = useMemo(() => Object.fromEntries(model.nodes.flatMap((node) => node.declaredName ? [[node.id, node.declaredName] as const] : [])), [model]);
  const [nativeRequests, setNativeRequests] = useState<NativeChatRequest[] | null>(null);
  useEffect(() => {
    const client = clientRef.current;
    if (!connected || !client || selected === "") { setNativeRequests(null); return; }
    let active = true;
    setNativeRequests(null);
    const read = () => { void client.listNativeChatRequests(selected).then(
      (page) => { if (active && clientRef.current === client && selectedRef.current === selected) setNativeRequests((previous) => previous !== null && sameValue(previous, page.requests) ? previous : page.requests); },
      () => { if (active && clientRef.current === client && selectedRef.current === selected) setNativeRequests(null); },
    ); };
    read();
    readNativeRequests.current = read;
    return () => { active = false; if (readNativeRequests.current === read) readNativeRequests.current = null; };
  }, [connected, selected]);
  const needs = useMemo(() => needsYou({ status, stale, events: eventList, envelopes, pendingEnvelopeSequences: envelopes.__pendingEnvelopeSequences, nativeRequests,
    pendingDraftIds: durableProposals.map((proposal) => proposal.draftId), nodeNames, operatorId: OPERATOR_ACTOR.id,
    agentWorkRecorded: hasRecordedAgentWork, agentWorkPending }),
    [status, stale, eventList, envelopes, nativeRequests, durableProposals, nodeNames, hasRecordedAgentWork, agentWorkPending]);
  const waitingAskers = useMemo(() => new Set(needs.items.flatMap((item) => item.kind === "question" ? [item.asker] : [])), [needs]);
  const team = useMemo(() => teamModel({ events: eventList, envelopes, personas, nativeLinks: nativePersonaLinks, aliases: actorAliases, model,
    claudeTasks: claudeTaskRead?.executionId === selected ? claudeTaskRead : null, waitingAskers, now: clock,
    taskStates: taskGraphs?.executionId === selected ? taskGraphs.tasks : null,
    taskRecords: taskGraphs?.executionId === selected ? taskGraphs.records : null }),
    [eventList, envelopes, personas, nativePersonaLinks, actorAliases, model, claudeTaskRead, selected, waitingAskers, clock, taskGraphs]);
  const links = useMemo(() => teamLinks(eventList, envelopes, team.bots, clock), [eventList, envelopes, team, clock]);
  const botNames = useMemo(() => namesOf(team.bots), [team]);
  // Journeys (phase 5, T5): read on run select and again whenever a new `jpd.*` signal lands in the
  // tail. No interval of its own. A failed read keeps the last view; with none, the tab says so.
  const lastJpdSequence = useMemo(() => {
    let last = 0;
    for (const event of eventList) {
      if (event.kind !== "signal_recorded") continue;
      const kind = event.payload !== null && typeof event.payload === "object" ? (event.payload as Record<string, unknown>).kind : undefined;
      if (typeof kind === "string" && kind.startsWith("jpd.") && event.sequence > last) last = event.sequence;
    }
    return last;
  }, [eventList]);
  const { flowsRead, journeysView, journeysFailed, approveFlow, rejectFlow, markFlowEdgeSafe, watchFlow, journeyRun } =
    useJourneys({ clientRef, selectedRef, selected, lastJpdSequence });
  const beforeAfter = useMemo(() => beforeAfterPairs(captureDocuments(eventList, envelopes)), [eventList, envelopes]);
  // #446: captures whose envelopes are not opened yet; the before/after list is not empty, it is opening.
  const capturesOpening = useMemo(() => eventList.filter((event) => event.kind === "signal_recorded" && event.evidenceRefs.length > 0
    && (event.payload as { kind?: unknown } | null)?.kind === "jpd.screen_captured" && envelopes[event.sequence] === undefined).length,
  [eventList, envelopes]);
  const [journeyContract, setJourneyContract] = useState<string | null>(() => journeyFromUrl() || null);
  const botNameOf = useCallback((id: string) => botNames[id] ?? id, [botNames]);
  // #393: one thread per task, opened by the run's task.* records.
  const currentTaskGraphs = useMemo(() => (taskGraphs?.executionId === selected ? taskGraphs : null), [taskGraphs, selected]);
  const runTasks = currentTaskGraphs?.tasks ?? null;
  // A run seen carrying task records is team work: named by its execution, and its demonstration
  // form's placeholder steps are not its progress (runLabel).
  const [teamRuns, setTeamRuns] = useState<ReadonlySet<string>>(() => new Set());
  useEffect(() => {
    if (!selected || !runTasks || runTasks.length === 0) return;
    setTeamRuns((seen) => (seen.has(selected) ? seen : new Set(seen).add(selected)));
  }, [selected, runTasks]);
  // #1696: the project card opens the team work, so the run carrying task records must be known
  // BEFORE anything is selected: ask the public tasks route once for each of the newest runs.
  const probedForTeam = useRef(new Set<string>());
  const probedClient = useRef<RuntimeClient | null>(null);
  useEffect(() => {
    const client = clientRef.current;
    if (!connected || !client) return;
    if (probedClient.current !== client) {
      probedClient.current = client;
      probedForTeam.current.clear();
      setTeamRuns(new Set());
    }
    const newest = [...executions].sort((x, y) => (Date.parse(y.lastEventAt ?? "") || 0) - (Date.parse(x.lastEventAt ?? "") || 0)).slice(0, 12);
    for (const run of newest) {
      if (probedForTeam.current.has(run.executionId)) continue;
      probedForTeam.current.add(run.executionId);
      void client.getTasks(run.executionId).then((reply) => {
        if (clientRef.current !== client || !reply || reply.tasks.length === 0) return;
        setTeamRuns((seen) => (seen.has(run.executionId) ? seen : new Set(seen).add(run.executionId)));
      }).catch(() => { probedForTeam.current.delete(run.executionId); });
    }
  }, [connected, executions, clientRef]);
  const teamRun = !!selected && teamRuns.has(selected);
  // #479: a message's issue/PR numbers link to the run's GitHub repository (read off its task
  // records) and a journey id it names opens the Journey tab on that journey.
  const saidLinks = useMemo<SaidLinkContext>(() => ({
    repoUrl: runTasks?.find((task) => task.repoUrl !== null)?.repoUrl ?? null,
    journeyIds: new Set([...(flowsRead.view?.flows.map((flow) => flow.id) ?? []), ...(journeysView?.journeys.map((journey) => journey.contractId) ?? [])]),
    onOpenJourney: (journeyId: string) => { setJourneyContract(journeyId); setCanvasTab("journey"); },
  }), [runTasks, flowsRead.view, journeysView]);
  const [thread, setThread] = useState(EVERYONE);
  useEffect(() => setThread(EVERYONE), [selected]);
  const activityLines = useMemo(() => {
    const stepTitle = (contractId: string, stepId: string) => journeysView?.journeys.find((journey) => journey.contractId === contractId)
      ?.steps.find((step) => step.stepId === stepId)?.screen?.title ?? stepId;
    return recentActivity.map((item) => describeActivity(item, botNames, envelopes, OPERATOR_ACTOR.id, stepTitle));
  }, [recentActivity, botNames, envelopes, journeysView]);
  const assignedNodeIds = useMemo(() => new Set(team.bots.flatMap((bot) => bot.tasks.flatMap((task) => task.nodeId === null ? [] : [task.nodeId]))), [team]);
  const unassignedSteps = useMemo(() => model.nodes.filter((node) => !assignedNodeIds.has(node.id)), [model, assignedNodeIds]);

  // #583: Graph is the default canvas tab; a tab the owner chose this browser session is kept.
  const [canvasTab, setCanvasTab] = useState<CanvasTab>(savedCanvasTab);
  // Graph tab: each journey's run, read through the same `journeyRun` the Journey tab's flowchart
  // uses (no new endpoint), and its screen frames through `journeyScreenFrame`.
  const [missionRuns, setMissionRuns] = useState<Record<string, JourneyRunView | null>>({});
  const [missionRunsRevision, setMissionRunsRevision] = useState(0);
  const [missionFrames, setMissionFrames] = useState<Record<string, string>>({});
  const [selectedJourney, setSelectedJourney] = useState<string | null>(null);
  useEffect(() => setSelectedJourney(null), [selected]);
  const activeJourney = selectedJourney ?? journeyContract
    ?? (canvasTab === "journey" ? journeysView?.journeys[0]?.contractId ?? flowsRead.view?.flows[0]?.id ?? null : null);
  useEffect(() => {
    const client = clientRef.current;
    if (client === null || (canvasTab !== "graph" && canvasTab !== "journey") || journeysView === null || !activeJourney) return undefined;
    let cancelled = false;
    client.journeyRun(activeJourney).then(
      (run) => { if (!cancelled) setMissionRuns((before) => ({ ...before, [activeJourney]: run })); },
      () => { if (!cancelled) setMissionRuns((before) => ({ ...before, [activeJourney]: null })); });
    return () => { cancelled = true; };
  }, [canvasTab, activeJourney, missionRunsRevision]);
  // Frames are fetched once per (journey, screen, run state); a URL is revoked only when its frame is
  // replaced or the Studio unmounts, never while it may still be on screen.
  const missionFrameKeys = useRef(new Map<string, { key: string; url: string | null }>());
  useEffect(() => {
    const client = clientRef.current;
    if (client === null || (canvasTab !== "graph" && canvasTab !== "journey") || !activeJourney) return;
    for (const [contractId, run] of Object.entries(missionRuns)) {
      if (contractId !== activeJourney) continue;
      for (const [screenId, screen] of Object.entries(run?.screens ?? {})) {
        if (!screen.frame) continue;
        const slot = `${contractId} ${screenId}`;
        const key = `${run?.digest ?? ""}:${run?.commit ?? ""}:${screen.result ?? ""}`;
        const held = missionFrameKeys.current.get(slot);
        if (held?.key === key) continue;
        missionFrameKeys.current.set(slot, { key, url: held?.url ?? null });
        client.journeyScreenFrame(contractId, screenId).then((frame) => {
          const entry = missionFrameKeys.current.get(slot);
          if (frame === null || entry === undefined || entry.key !== key) return;
          const url = URL.createObjectURL(frame.blob);
          if (entry.url !== null) URL.revokeObjectURL(entry.url);
          entry.url = url;
          setMissionFrames((before) => ({ ...before, [slot]: url }));
        }, () => { missionFrameKeys.current.delete(slot); });
      }
    }
  }, [canvasTab, missionRuns, activeJourney]);
  useEffect(() => {
    const kept = missionFrameKeys.current;
    return () => { for (const entry of kept.values()) if (entry.url !== null) URL.revokeObjectURL(entry.url); kept.clear(); };
  }, []);
  // #746: the test canvas runs a journey; each state it reads updates that journey's Proof rows.
  const recordMissionRun = useCallback((contractId: string, run: JourneyRunView) => setMissionRuns((before) => ({ ...before, [contractId]: run })), []);
  const missionRunFor = useCallback((contractId: string) => missionRuns[contractId] ?? null, [missionRuns]);
  const missionFrameUrl = useCallback((stepId: string, contractId: string) => {
    const step = journeysView?.journeys.find((j) => j.contractId === contractId)?.steps.find((s) => s.stepId === stepId);
    return missionFrames[`${contractId} ${step?.screen?.screenId ?? stepId}`] ?? null;
  }, [journeysView, missionFrames]);
  // The test canvas's "mark safe" is #518's mark: the skipped edge into that step, on that flow.
  const markMissionStepSafe = useCallback(async (stepId: string, contractId: string) => {
    const journey = journeysView?.journeys.find((j) => j.contractId === contractId);
    const edge = journey === undefined ? undefined : skippedEdgeInto(missionRuns[contractId] ?? null, stepId, journey.steps.map((s) => s.stepId));
    if (edge === undefined) throw new Error("No skipped step to mark here.");
    try {
      await markFlowEdgeSafe(contractId, edge[0]);
    } finally {
      setMissionRunsRevision((r) => r + 1);
    }
  }, [journeysView, missionRuns, markFlowEdgeSafe]);
  // No Runtime route sends a journey step back to its author, so the Graph tab passes no onSendBack
  // and the Test canvas shows that button disabled with the reason.
  const missionLanes = useMemo(() => {
    const records = currentTaskGraphs?.records ?? [];
    const timed: TimedTaskEvent[] = records.flatMap((r) => (r.occurredAt ? [{ ...r, at: r.occurredAt }] : []));
    return applySessionEnds(laneBars(timed, { slices: currentTaskGraphs?.slices ?? [], routes: currentTaskGraphs?.routes ?? [], liveness: currentTaskGraphs?.liveness, leases: currentTaskGraphs?.leases }, clock, 14 * 3_600_000), eventList);
  }, [currentTaskGraphs, clock, eventList]);
  const [graphFileOpen, setGraphFileOpen] = useState(false);
  useEffect(() => { setCanvasTab(savedCanvasTab()); }, [selected]);
  const graphFileInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (fileFocusNonce > 0) {
      setCanvasTab("team");
      setGraphFileOpen(true);
      window.setTimeout(() => graphFileInput.current?.focus(), 0);
    }
  }, [fileFocusNonce]);

  const projectKey = project ?? "this runtime";
  const [seenSeq, setSeenSeq] = useState<ReturnType<typeof readLastSeen>>(null);
  useEffect(() => { setSeenSeq(selected === "" ? null : readLastSeen(projectKey, selected)); }, [projectKey, selected]);
  const head = status?.headSequence ?? 0;
  const handover = useMemo(() => status !== null && status.executionId === selected && shouldShowHandover(eventList, seenSeq, head)
    ? buildHandover({ events: eventList, bots: team.bots, model, taskRecords: currentTaskGraphs?.records ?? [], claudeTasks: claudeTaskRead?.executionId === selected ? claudeTaskRead : null,
        openItems: needs.items, envelopes, fromSeq: typeof seenSeq === "number" ? seenSeq : seenSeq!.sequence,
        seenAt: typeof seenSeq === "object" ? seenSeq?.at : undefined, toSeq: head })
    : null, [status, selected, eventList, seenSeq, head, team, model, claudeTaskRead, needs, envelopes]);
  // Hide puts the bar away for this browser session and this gap; it does not mark anything seen.
  const [hiddenGap, setHiddenGap] = useState<string | null>(null);
  const handoverHidden = handover !== null && (hiddenGap === `${selected}:${handover.fromSeq}`
    || hiddenThisSession(projectKey, selected, handover.fromSeq));
  const hideHandover = useCallback(() => {
    if (handover === null) return;
    hideThisSession(projectKey, selected, handover.fromSeq);
    setHiddenGap(`${selected}:${handover.fromSeq}`);
  }, [handover, projectKey, selected]);
  const markSeen = useCallback(() => {
    if (selected === "" || head === 0) return;
    writeLastSeen(projectKey, selected, head);
    setSeenSeq({ sequence: head, at: Date.now() });
  }, [projectKey, selected, head]);
  useEffect(() => {
    // Ten visible seconds of the live view count as seen; an open handover waits for Got it.
    if (handover !== null || selected === "" || head === 0) return;
    const timer = window.setTimeout(() => { if (document.visibilityState === "visible") markSeen(); }, 10_000);
    return () => window.clearTimeout(timer);
  }, [handover, selected, head, markSeen]);

  const [citedRecords, setCitedRecords] = useState<{ executionId: string; sequences: number[] } | null>(null);
  const citationOrigin = useRef<HTMLElement | null>(null);
  const citationPanel = useRef<HTMLElement | null>(null);
  useEffect(() => { setCitedRecords(null); }, [selected]);
  useEffect(() => { if (citedRecords !== null) citationPanel.current?.focus(); }, [citedRecords]);
  // The cited records, filtered once per citation or log change through a Set, not per render by
  // a linear `includes` over every event.
  const citedEvents = useMemo(() => {
    if (citedRecords === null) return [];
    const cited = new Set(citedRecords.sequences);
    return eventList.filter((event) => cited.has(event.sequence));
  }, [citedRecords, eventList]);
  const closeCitedRecords = () => { setCitedRecords(null); citationOrigin.current?.focus(); };
  const openRecords = (sequences: number[]) => {
    const first = sequences[0];
    if (first === undefined) return;
    citationOrigin.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setCitedRecords({ executionId: selected, sequences });
  };
  const answerQuestion = (asker: string, signalId: string | null) => {
    setTalkOpen(true);
    setSayTo(asker);
    setSayAnswer({ asker, signalId });
    setFocus({ kind: "run" });
    setSayFocusNonce((nonce) => nonce + 1);
  };
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
  const currentReplyIssue = replyIssue !== null && replyIssue.key === replyKey ? replyIssue.text : null;
  const currentReplySuggestions = replySuggestions !== null && replySuggestions.key === replyKey ? replySuggestions.reply : null;
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
  const visibleExecutions = executions.filter((run) => !removedRuns.includes(run.executionId));
  // #1654: one card per project. The rail shows the project; its runs are history in the stage.
  const railProject = { name: project ?? "this runtime", path: projectPath, runs: visibleExecutions.map((run) => run.executionId === selected && pendingOwnerReview ? { ...run, attention: "needs_you" as const } : run) };

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
  const chooseCanvas = (tab: CanvasTab) => {
    setCanvasTab(tab);
    // The deep link chose the first tab only; the owner's own choice wins from here on.
    if (journeyFromUrl() && typeof window.history?.replaceState === "function") {
      const url = new URL(window.location.href);
      url.searchParams.delete("journey");
      window.history.replaceState(window.history.state, "", url);
    }
    try { sessionStorage.setItem(CANVAS_TAB_KEY, tab); } catch { /* storage refused: the choice lasts this page only */ }
  };
  const canvasViews = <div className="canvas-tabs" role="tablist" aria-label="Canvas views">
    <button type="button" role="tab" aria-selected={canvasTab === "team"} onClick={() => chooseCanvas("team")}>Team (live)</button>
    <button type="button" role="tab" aria-selected={canvasTab === "journey"} onClick={() => chooseCanvas("journey")}>Proof</button>
    <button type="button" role="tab" aria-selected={canvasTab === "graph"} onClick={() => chooseCanvas("graph")}>Graph</button>
  </div>;
  /** The one legal action for a blocked or waiting step, or for a pending proposal. */
  const stepActions = (item: StepItem | DraftItem): ReactNode => {
    if (item.kind === "draft") return <button type="button" onClick={focusPendingProposal}>Review proposal</button>;
    if (item.kind === "waiting_step") {
      return <button type="button" onClick={pendingQuestion !== null ? () => answerQuestion(pendingQuestion.asker, pendingQuestion.signalId) : focusRunReply}>Open run conversation</button>;
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

  const pendingJourneyApprovals = (flowsRead.view?.flows ?? []).filter((flow) => {
    if (!flow.approvable || (flow.status !== "draft" && flow.status !== "approval_stale")) return false;
    const contract = withFlowContracts(journeysView?.journeys ?? [], [flow]).find((journey) => journey.contractId === flow.id);
    return contract !== undefined && approvalBlockReason(flow, contract, missionRunFor(contract.contractId)) === null;
  });
  const beaconState = needs.state.kind === "unknown" || pendingJourneyApprovals.length === 0 ? needs.state
    : { kind: "lit" as const, count: (needs.state.kind === "lit" ? needs.state.count : 0) + pendingJourneyApprovals.length };
  return (
    <SaidLinks.Provider value={saidLinks}>
    <div className={`app ${projectsOpen ? "projects-open" : ""} ${conversationVisible ? "conversation-open" : ""}`} data-document-open={openDocument !== null} style={{ "--rail": `${railWidth}px` } as CSSProperties}>
      <ProjectRail
        projects={[railProject]}
        selectedPresentation={recordedWorkBehindGraphWait ? { key: "calm", status: "agent work recorded · graph step waiting" } : null}
        onClose={() => setProjectsOpen(false)}
        selected={selected}
        connected={connected}
        previousExit={previousExit}
        startupTiming={startupTiming}
        currentRun={currentRun}
        runtimeDetailsRefreshed={runtimeDetailsRefreshed}
        runtimeDetailsRefreshing={runtimeDetailsRefreshing}
        runtimeDetailsRefreshError={runtimeDetailsRefreshError}
        onRefreshRuntimeDetails={() => void refreshRuntimeDetails()}
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
        briefings={briefings}
        teamRuns={teamRuns}
      />

      {projectsOpen && <button type="button" className="projects-backdrop" aria-label="Close projects" onClick={() => setProjectsOpen(false)} />}
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
                {runLabel(selected, teamRun ? { objective: null, teamWork: true } : briefing)}
              </button>
            </>) : (
              <span className="meta">pick a run, or start one</span>
            )}
            {selected && <p className="run-selection-identity" aria-label="Selected run identity">Project: {project ?? "this runtime"} / Run: {selected}</p>}
          </div>
          {selected && status !== null && (
            <Beacon
              state={beaconState}
              onOpen={() => {
                if (pendingJourneyApprovals.length > 0) {
                  setJourneyContract(pendingJourneyApprovals[0]!.id);
                  setCanvasTab("journey");
                  setTalkOpen(false);
                  return;
                }
                // #402: the cards live in the Needs you thread; the beacon opens it.
                setTalkOpen(true);
                setFocus({ kind: "run" });
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
            {routes !== null && jevAvailability(judgeRoutes.length) === "absent" && <p className="jev-absent-note">{JEV_ABSENT_NOTE}</p>}
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
              <button type="button" className="ghost" onClick={() => select("")}>Back to project</button>
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
          <section className="canvas-column" aria-label="Canvas">
            <div className="mv-pad">
              <h2>Start your team's work</h2>
              <p>No run is selected. Start a run, then connect your agents to record their tasks and proof.</p>
              <button type="button" className="act" onClick={startDraft}>Start a team run</button>
            </div>
            {canvasViews}
            {canvasTab === "team" ? (
              <div role="tabpanel" aria-label="Team" className="mv-pad">
                <p>No team activity yet. Agents and their tasks appear here when they record work in a run.</p>
              </div>
            ) : (
              <div role="tabpanel" aria-label={canvasTab === "journey" ? "Proof" : "Graph"}>
                <MissionView key={canvasTab} journeys={[]} tasks={[]} lanes={[]} now={clock}
                  runFor={missionRunFor} frameUrl={missionFrameUrl} onMarkSafe={markMissionStepSafe}
                  initialView={canvasTab === "journey" ? "proof" : "graph"} />
              </div>
            )}
          </section>
        ) : status === null ? (
          <section className="loading" role="status">
            {error && !busy ? (
              <><strong>This run could not be opened.</strong><p>{error}</p><button type="button" onClick={() => void loadExecution(selected)}>Try again</button></>
            ) : (
              <><LoaderCircle className="spin" aria-hidden="true" /><strong>Opening this run…</strong><p>Reading its state and event history. Large runs may take a moment.</p></>
            )}
          </section>
        ) : (
          <div className="studio-columns">
            {talkOpen && (
            <aside id="conversation-panel" className="talk" hidden={!conversationVisible}>
              <QuestionCards items={needs.items} names={botNames} busy={busy || saying === "run"}
                onChoose={(item, choice) => void say(choice, item.asker, "run", item.signalId, "operator_note", item.task ?? null)}
                onAnswer={(item) => answerQuestion(item.asker, item.signalId)}
                onRefuse={(item) => void refuse(item)}
                onCheck={(item) => setFocus({ kind: "node", id: item.nodeId })}
                stepActions={stepActions} />
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
                replySuggestions={currentReplySuggestions}
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
                  name={actorAliases[focus.id] ?? nativePersonaLinks[focus.id]?.chat.title}
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
            {/* #1636: on every canvas. No control reaches "team" since #617, so a team-only card was
                unreachable. It shows only while an undismissed handover exists, and Got it / Hide remove it. */}
            {handover !== null && !handoverHidden && (
              <HandoverCard handover={handover} onOpen={openRecords} onDismiss={markSeen} onHide={hideHandover} />
            )}
            {/* #583: the Graph tab is the whole page: its own top bar replaces the banner and this strip. */}
            {canvasTab === "team" && canvasViews}
            <div
              className="scene"
              style={dockReservePx === null ? undefined : ({ "--dock-reserve": `${dockReservePx}px` } as CSSProperties)}
            >
            <div id="studio-panel-team" role="tabpanel" aria-label="Team" hidden={canvasTab !== "team"}>
            {canvasTab === "team" && tasksRouteMissing === selected && (
              <p className="mv-none mv-pad" role="alert">{RUNTIME_PREDATES_TASKS_ROUTE}</p>
            )}
            {canvasTab === "team" && tasksRouteFailure?.executionId === selected && (
              <div className="mv-pad" role="alert">Tasks route failed · {tasksRouteFailure.text} · <button type="button" onClick={() => setHistoryRetry((value) => value + 1)}>Retry</button></div>
            )}
            {canvasTab === "team" && taskGraphs?.executionId === selected && tasksRouteMissing !== selected && (
              <DelegationTree tasks={taskGraphs.tasks} />
            )}
            {canvasTab === "team" && taskGraphs?.executionId === selected && tasksRouteMissing !== selected && (
              <TaskGraphs tasks={taskGraphs.tasks}
                  onOpenJourney={(contractId) => { setJourneyContract(contractId); chooseCanvas("journey"); }} />
            )}
            {canvasTab === "team" && (
              <TeamCanvas storageKey={`graphhelm.team-positions:${projectKey}:${selected}`} bots={team.bots} otherRecorders={team.otherRecorders}
                links={links} unassignedSteps={unassignedSteps} selectedBot={thread.startsWith("direct:") ? thread.slice(7) : null}
                onSelectBot={(key) => { setThread(`direct:${key}`); }}
                onOpenBotDetails={(key) => setFocus({ kind: "agent", id: key })}
                onNameBot={nameBot}
                onOpenNode={(id) => { nodeFocusOrigin.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; setFocus({ kind: "node", id }); }}
                onOpenTask={(bot, task) => task.nodeId !== null ? setFocus({ kind: "node", id: task.nodeId }) : openRecords([task.sequence])}
                graphFileOpen={graphFileOpen} onGraphFileOpenChange={setGraphFileOpen}
                graphFileRow={<><GraphFileRow graphFile={graphFile} onGraphFileChange={onGraphFileChange} onDrawConnections={() => void drawConnections()} busy={busy}
                  demonstration={status.executor === "fixture"} fixtureFile={fixtureFile} onFixtureFileChange={setFixtureFile} inputRef={graphFileInput} />
                  <p className={`connection-note ${resumeNeedsFile && graphFile.trim().length === 0 ? "none" : connectionTone}`} role="status">{resumeNeedsFile && graphFile.trim().length === 0
                    ? "Resume needs this run's graph file. Type its path on the Runtime host in the Graph file field, then press resume again."
                    : connectionNote}</p></>} />
            )}
            </div>
            {(canvasTab === "graph" || canvasTab === "journey") && (
              <div id="studio-panel-graph" role="tabpanel" aria-label={canvasTab === "journey" ? "Proof" : "Graph"}>
                <SlotPoll client={clientRef.current}>{(slots) => <MissionView key={canvasTab} events={eventList} envelopes={envelopes} wakeListeners={status?.wakeListeners ?? []} seenActors={status?.seenActors ?? []} seenActorAgesMs={status?.seenActorAgesMs ?? {}} seenActorSources={status?.seenActorSources ?? {}} onDemandLanes={onDemandLanes} slots={slots} journeys={journeysView?.journeys ?? []} tasks={runTasks ?? []} taskRecords={currentTaskGraphs?.records ?? []} backlog={currentTaskGraphs?.backlog ?? []} {...(currentTaskGraphs?.durations ? { durations: currentTaskGraphs.durations } : {})} {...(currentTaskGraphs?.liveness ? { livenessBudgets: currentTaskGraphs.liveness.budgets } : {})} tasksRouteMissing={tasksRouteMissing === selected} tasksRouteFailure={tasksRouteFailure?.executionId === selected ? tasksRouteFailure.text : null} historyLoading={selected !== null && taskGraphs?.executionId !== selected} lanesHistoryLoading={selected !== null && currentTaskGraphs === null} historyFailedCount={historyFailure?.executionId === selected ? historyFailure.count : 0} onRetryHistory={() => setHistoryRetry((value) => value + 1)} onSelectJourney={setSelectedJourney} runFor={missionRunFor}
                  initialView={canvasTab === "journey" ? "proof" : "graph"} focusJourneyId={journeyContract}
                  flows={flowsRead.view} onApprove={approveFlow} onReject={rejectFlow} onWatch={watchFlow}

                  run={journeyRun} onRun={recordMissionRun}
                  lanes={missionLanes} now={clock} frameUrl={missionFrameUrl} onMarkSafe={markMissionStepSafe}
                  runName={selected} lastRecordAt={Date.parse(eventList[eventList.length - 1]?.occurredAt ?? "")} onTeam={() => chooseCanvas("team")}
                  agents={team.bots} away={handover ? { minutes: handover.gapMinutes, shipped: handover.shipped.length } : null}
                  runNodes={teamRun && status?.executor === "fixture" ? [] : model.nodes} unassignedNodeIds={unassignedSteps.map((node) => node.id)}
                  onOpenNode={(id) => { nodeFocusOrigin.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; setFocus({ kind: "node", id }); }}
                  onOpenBotDetails={(key) => setFocus({ kind: "agent", id: key })} onNameBot={nameBot}
                  onReviewAssigned={async (task, lane) => {
                    const client = clientRef.current;
                    if (!client || selected === "") throw new Error("No Runtime is connected.");
                    await recordReviewAssigned(client, selected, task, lane);
                  }}
                  onSignal={async (note) => {
                    // #591 Nudge / Reassign: an owner note on this run, the same POST /signal the chat box uses.
                    const client = clientRef.current;
                    if (!client || selected === "") throw new Error("No Runtime is connected.");
                    const evidence = await client.signal(selected, note.description, { to: note.to, kind: note.type });
                    if (evidence.result === "unknown") throw new Error("The Runtime may have received the note, but the result is unconfirmed. Check the activity log before trying again.");
                    if (evidence.result === "refused") throw new Error(evidence.diagnostics[0]?.message ?? "The Runtime refused the note.");
                  }} />}</SlotPoll>
              </div>
            )}
            {citedRecords !== null && citedRecords.executionId === selected && (
              <section ref={citationPanel} className="journey-detail cited-records" role="dialog" aria-modal="false" aria-label="Cited records" tabIndex={-1}
                onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); closeCitedRecords(); } }}>
                <div className="journey-detail-head"><h2>Cited records</h2><button type="button" onClick={closeCitedRecords}>Close cited records</button></div>
                <Thread events={citedEvents} executionId={selected} openEvidence={openEvidence}
                  emptyMessage="The cited records are not in the loaded history." />
              </section>
            )}

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
              {/* #585: a bare <summary> reads as an unnamed group, so no journey could target it by
                * role and name; it is the button that opens the run's actions. */}
              <summary role="button" aria-expanded={runActionsOpen}>Run actions</summary>
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
                      setRunActionsOpen(false);
                      setResumeNeedsFile(true);
                      setCanvasTab("team");
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
                  ? " · the Studio could not confirm whether the Runtime applied it — check the run before treating it as done"
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
                  delegation={status?.delegation?.[node.id]}
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
                    // A claim needs either a host file or the matched sealed snapshot.
                    (graphFile.trim() === "" && !snapshotResumeAvailable) || selected === ""
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
            <RightPanel activity={activityLines} onOpenActivity={(sequence) => openRecords([sequence])}
              journeys={journeysView?.journeys ?? []} beforeAfter={beforeAfter} botName={botNameOf}
              journeysLoading={journeysView === null && !journeysFailed && selected !== null}
              pairsLoading={capturesOpening > 0}
              onOpenJourney={(contractId) => { setJourneyContract(contractId); chooseCanvas("journey"); }}
              onOpenPair={(pair: BeforeAfterPair) => {
                const known = journeysView?.journeys.some((j) => j.contractId === pair.contractId && j.steps.some((step) => step.stepId === pair.stepId)) ?? false;
                if (!known) { openRecords([pair.before.sequence, pair.after.sequence]); return; }
                setJourneyContract(pair.contractId); chooseCanvas("journey");
              }} />
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
    </SaidLinks.Provider>
  );
}

/** #1169: the run named by `?run=` in this tab's address, or none. */
export function selectedRunFromUrl(): string {
  if (typeof window === "undefined") return "";
  return new URLSearchParams(window.location.search).get("run") ?? "";
}

/** The journey named by `?journey=` (`graphhelm studio open --journey`): the Proof view opens on it. */
export function journeyFromUrl(): string {
  if (typeof window === "undefined") return "";
  return new URLSearchParams(window.location.search).get("journey") ?? "";
}

/** #1169: keep `?run=` in step with the open run, leaving every other parameter alone. */
export function rememberSelectedRun(run: string): void {
  if (typeof window === "undefined" || typeof window.history?.replaceState !== "function") return;
  const url = new URL(window.location.href);
  if ((url.searchParams.get("run") ?? "") === run) return;
  if (run) url.searchParams.set("run", run);
  else url.searchParams.delete("run");
  window.history.replaceState(window.history.state, "", url);
}
