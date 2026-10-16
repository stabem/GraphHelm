import "./mission-view.css";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useNow } from "../runtime/use-now";
import type { JourneyFlowView, JourneyFlowsView, JourneyRunView, JourneyView } from "../runtime/types";
import type { TaskState } from "../runtime/team-tasks";
import type { TaskEventRecord } from "../runtime/team-tasks";
import type { Lane } from "../runtime/lane-bars";
import { RUNTIME_PREDATES_TASKS_ROUTE } from "../runtime/tasks-route";
import { buildMission, toMissionTask, unlinkedTasks, withCurrentReviewer, type Mission } from "../runtime/mission";
import { layoutMission } from "../runtime/mission-layout";
import { testFrames } from "../runtime/test-frames";
import { activity, ownerLane, ownerRole, stageHealth, stageProgress } from "../runtime/stage-health";
import { buildWorkGroups, type WorkGroup } from "../runtime/work-groups";
import { IssueGraph, MissionGraph, STATUS_LABEL, stageState } from "./mission-graph";
import { ProofTable } from "./proof-table";
import { TestCanvas } from "./test-canvas";
import { ProofRun } from "./proof-run";
import { journeyVerdict, runJourneys, type JourneyRunSource, type JourneyVerdict } from "./journey-flows";
import { LanesTimeline } from "./lanes-timeline";
import { WorkKanban, type KanbanItem } from "./work-kanban";
import type { Bot } from "../runtime/team";
import { realName } from "../runtime/lane-bars";
import { laneLiveness } from "../runtime/stage-health";
import { WORK_STAGES } from "../runtime/work-groups";
import { LaneActions, laneRoster, type LaneNote } from "./lane-actions";
import { laneRepliedAfterAsk } from "./lane-actions";
import { laneBudgets } from "../runtime/lane-bars";
import type { LivenessBudgets } from "../runtime/tasks-route";
import type { EnvelopeRecord } from "../graph/ledger";
import type { RuntimeEvent } from "../runtime/types";
import type { SlotView } from "../runtime/slots";
import type { GraphNode } from "../graph/model";
import { readable } from "./format";
import { approvalBlockReason } from "../runtime/journeys";

type Sub = "graph" | "proof" | "test" | "lanes";
const WINDOW_MS = 14 * 3_600_000;
const GONE_QUIET_MS = 24 * 3_600_000;

function timerTask(task: TaskState): TaskState {
  if (!task.blockedBy) return task;
  const blockedAt = task.rounds.at(-1)?.blockedAt;
  return blockedAt ? { ...task, clock: { ...task.clock, since: blockedAt } } : task;
}

interface Props {
  /** #1234: the budgets the Runtime judged liveness by (the tasks route's `liveness.budgets`). */
  livenessBudgets?: LivenessBudgets;
  events?: RuntimeEvent[];
  envelopes?: EnvelopeRecord;
  journeys: JourneyView[];
  tasks: TaskState[];
  taskRecords?: TaskEventRecord[];
  runFor(contractId: string): JourneyRunView | null;
  lanes: Lane[];
  now: number;
  frameUrl(stepId: string, contractId: string): string | null;
  /** The step and the journey it belongs to: the mark is recorded against that journey's flow. */
  onMarkSafe(stepId: string, contractId: string): void | Promise<void>;
  /** Absent: the Test canvas shows Send back as not available yet. */
  onSendBack?: (stepId: string, contractId: string) => void;
  /** #746: present, opening a journey's test runs it (the Journey tab's run source). */
  run?: JourneyRunSource;
  /** #746: each state of that run, so the Proof rows read it. */
  onRun?: (contractId: string, run: JourneyRunView) => void;
  /** #583 full page: the run named in the top bar's breadcrumb. */
  runName?: string;
  /** #583: when the newest record was written (ms); absent, the live indicator is omitted. */
  lastRecordAt?: number | null;
  /** #583: present, the view is the full page. The owner retired the Team tab (Lanes replaces it), so the nav no longer offers it. */
  onTeam?: () => void;
  initialView?: "graph" | "proof";
  focusJourneyId?: string | null;
  flows?: JourneyFlowsView | null;
  onApprove?: (flowId: string) => Promise<void>;
  onWatch?: (flowId: string) => Promise<void>;
  /** #591: "Agents right now" in the rail, from the team model. */
  agents?: Bot[];
  /** The selected run's initial task history read has not settled yet. */
  historyLoading?: boolean;
  /** The displayed task history is a recent page rather than the complete fold. */
  partial?: boolean;
  /** #1192: the Runtime answered the tasks route with "no such route" (an older binary); the
   * Lanes board and the kanban are replaced by one sentence saying so, never left empty. */
  tasksRouteMissing?: boolean;
  /** #1192: the Runtime's code and sentence when the tasks route failed (null when it answered). */
  tasksRouteFailure?: string | null;
  /** Recent task history is ready for Lanes while the complete history is still loading. */
  lanesHistoryLoading?: boolean;
  historyFailedCount?: number;
  onRetryHistory?: () => void;
  onSelectJourney?: (contractId: string) => void;
  /** #591: the "While you were away" gap, for the summary's `N h away · N shipped`; absent, omitted. */
  away?: { minutes: number; shipped: number } | null;
  /** #591: posts an owner note on the run (App: client.signal). Absent: no Nudge / Reassign. */
  onSignal?: (note: LaneNote) => Promise<unknown>;
  onReviewAssigned?: (task: TaskState, lane: string) => Promise<unknown>;
  /** #636: the build-slot queues (App polls them); empty: no slot roots, nothing shown. */
  slots?: SlotView[];
  /** Actors with an armed, unconsumed wake lease; absent on older Runtimes. */
  wakeListeners?: string[];
  seenActors?: string[];
  seenActorAgesMs?: Record<string, number>;
  seenActorSources?: Record<string, "claimed" | "verified">;
  onDemandLanes?: string[];
  /** #647: the run's graph nodes for the Run graph strip; empty, the strip is omitted. */
  runNodes?: GraphNode[];
  /** #647: node ids no bot holds a task for (App's unassignedSteps), listed as the Team canvas did. */
  unassignedNodeIds?: string[];
  /** #647: opens the existing node window (App: setFocus node). Absent: nodes are not buttons. */
  onOpenNode?: (nodeId: string) => void;
  /** #647: Lanes board "Details" (App: setFocus agent). */
  onOpenBotDetails?: (key: string) => void;
  /** #647: Lanes board "Name this bot" (App: nameBot). */
  onNameBot?: (actorId: string, displayName: string) => void | Promise<unknown>;
}

/** #647: the run's nodes and their state, each one opening the node window the Team canvas opened. */
/** `freeAfterMs` is the Runtime's free budget (#1234): an agent counts as recent within it; 0 (no
 * budget served) lists nobody. */
export function RunGraphStrip({ nodes, unassigned, agents = [], now, onOpenNode, freeAfterMs = 0 }: { nodes: GraphNode[]; unassigned: string[]; agents?: Bot[]; now: number; onOpenNode?: (nodeId: string) => void; freeAfterMs?: number }) {
  const [open, setOpen] = useState(true);
  if (nodes.length === 0) return null;
  const technical = new Set(["codex", "claude-code", "studio-operator"]);
  const recentAgents = new Map<string, { name: string; lastAt: number }>();
  for (const agent of agents) {
    const actorId = agent.actorId?.trim();
    const name = agent.name.trim();
    const lastAt = agent.lastRecordAt === null ? NaN : Date.parse(agent.lastRecordAt);
    const age = now - lastAt;
    if (!actorId || agent.shared || agent.native || technical.has(actorId.toLowerCase())
      || !name || name.toLowerCase() === "tbd" || !Number.isFinite(lastAt) || age < 0 || age > freeAfterMs) continue;
    if (!recentAgents.has(actorId)) recentAgents.set(actorId, { name, lastAt });
  }
  const free = new Set(unassigned);
  const parts = [
    { title: "Steps without a bot", nodes: nodes.filter((n) => free.has(n.id)) },
    { title: "Steps with a bot", nodes: nodes.filter((n) => !free.has(n.id)) },
  ].filter((p) => p.nodes.length > 0);
  const item = (n: GraphNode) => {
    const text = `${n.declaredName ?? n.id} · ${readable(n.state)}`;
    return <li key={n.id}>{onOpenNode ? <button type="button" className="mv-node" data-state={n.state} onClick={() => onOpenNode(n.id)}>{text}</button> : <span className="mv-node" data-state={n.state}>{text}</span>}</li>;
  };
  return (
    <section className="mv-run-graph mv-pad" aria-label="Run graph">
      <button type="button" className="mv-fold" aria-expanded={open} onClick={() => setOpen((o) => !o)}>{`Run graph · ${nodes.length} ${nodes.length === 1 ? "step" : "steps"}`}</button>
      {open && parts.map((p) => (
        <div key={p.title} className="mv-run-part">
          <h3>{p.title}</h3>
          <ol>{p.nodes.map(item)}</ol>
        </div>
      ))}
      {open && recentAgents.size > 0 && <div className="mv-run-part">
        <h3>Recently recorded agents</h3>
        <ul aria-label="Recently recorded agents">{[...recentAgents].map(([actorId, agent]) => (
          <li key={actorId}>{agent.name} · Last record {ago(now - agent.lastAt)}</li>
        ))}</ul>
      </div>}
    </section>
  );
}

export function agentTone(b: Bot): "work" | "stalled" | "idle" {
  return b.state === "working" ? "work" : b.state === "done" ? "idle" : "stalled";
}
export function agentActivity(b: Bot): string {
  if (b.state === "done") return b.quietMinutes !== null ? `idle ${b.quietMinutes} min` : "idle";
  if (b.state === "quiet") return `${b.doingNow || "quiet"}${b.quietMinutes !== null ? ` · silent ${b.quietMinutes} min` : ""}`;
  if (b.state === "waiting_for_you") return b.doingNow ? `${b.doingNow} · waits for you` : "waits for you";
  return b.doingNow || "working";
}
export const awayText = (a: { minutes: number; shipped: number }) => `${Math.max(1, Math.round(a.minutes / 60))} h away · ${a.shipped} shipped`;
/** #735: one stacked bar per work group, plus the same counts in words, instead of a spinner per item. */
const PROGRESS_ORDER = ["proven", "merged", "work", "stalled", "ready"] as const;
const PROGRESS_WORD: Record<(typeof PROGRESS_ORDER)[number], string> = { proven: "proven", merged: "merged", work: "in work", stalled: "blocked", ready: "Ready · unclaimed" };
/** `prs` is how many of the tasks have a PR (#1166): a claimed task without one is work, not a PR. */
export function GroupProgress({ states, prs = states.length }: { states: string[]; prs?: number }) {
  const counts = PROGRESS_ORDER.map((k) => [k, states.filter((s) => s === k).length] as const).filter(([, n]) => n > 0);
  const words = [prs === 0 ? "no PR yet" : `${prs} ${prs === 1 ? "PR" : "PRs"}`, ...counts.map(([k, n]) => `${n} ${PROGRESS_WORD[k]}`)].join(" · ");
  return (
    <span className="mv-progress">
      <span className="mv-progress-bar" role="img" aria-label={words}>
        {counts.map(([k, n]) => <span key={k} className="mv-progress-part" style={{ flexGrow: n, background: SEG_TONE[k] }} />)}
      </span>
      <span className="mv-progress-text">{words}</span>
    </span>
  );
}

/** #744: the same bar for a journey's steps on the Proof rail. */
function splitDraftTitle(title: string): { name: string; note: string | null } {
  const match = title.match(/^(.*?)\s*\(draft:\s*(.*?)\)\s*$/i);
  return match ? { name: match[1]!, note: match[2]! } : { name: title, note: null };
}
function flowForContract(flows: JourneyFlowView[], contractId: string): JourneyFlowView | undefined {
  return flows.find((flow) => flow.id === contractId)
    ?? flows.filter((flow) => contractId.startsWith(`${flow.id}.`)).sort((a, b) => b.id.length - a.id.length)[0];
}
/** #1153: an uncompiled flow's steps in its main path's order. The Runtime lists screens by id, and
 * the rail's staleness check compares against the path, so id order flagged every draft as stale. */
function flowStepOrder(flow: JourneyFlowView): JourneyFlowView["screens"] {
  const edges = flow.paths.main?.map((id) => flow.edges.find((edge) => edge.id === id));
  if (!edges?.length || edges.some((edge) => edge === undefined)) return flow.screens;
  const ids = [edges[0]!.from, ...edges.map((edge) => edge!.to)];
  // A path that revisits a screen is a validation finding (flow.path_revisits_screen); id order
  // keeps each step once instead of repeating one.
  if (new Set(ids).size !== ids.length) return flow.screens;
  const ordered = ids.map((id) => flow.screens.find((screen) => screen.id === id));
  if (ordered.some((screen) => screen === undefined)) return flow.screens;
  return [...ordered as JourneyFlowView["screens"], ...flow.screens.filter((screen) => !ids.includes(screen.id))];
}
const VERDICT_LABEL: Record<JourneyVerdict, string> = { passed: "✓ passed", failed: "✕ failed", stale: "! stale", needs_you: "! needs you", not_run: "○ not run", running: "… running" };
const VERDICT_HINT: Record<JourneyVerdict, string> = {
  passed: "Last replay passed and still matches the current flow.",
  failed: "Last replay failed. Open it to see where.",
  stale: "The flow changed since the last replay. Run it again.",
  needs_you: "The replay stopped at a step only you can mark safe.",
  not_run: "Not replayed yet.",
  running: "Replaying now.",
};
/** #1296: the Proof rail's journeys grouped by the flow's `feature` (business rule), groups and
 * journeys in name order so rows never jump while Run all lands verdicts. Without any feature the
 * rail stays one flat list in its old order. Ungrouped journeys go last, under "Other". */
export interface JourneyGroup<T> { feature: string | null; items: T[] }
export function groupJourneys<T>(items: readonly T[], featureOf: (item: T) => string | null | undefined, nameOf: (item: T) => string): JourneyGroup<T>[] {
  const feature = (item: T) => featureOf(item)?.trim() || null;
  if (!items.some((item) => feature(item) !== null)) return items.length ? [{ feature: null, items: [...items] }] : [];
  const byName = (a: T, b: T) => nameOf(a).localeCompare(nameOf(b), undefined, { sensitivity: "base", numeric: true });
  const groups = new Map<string | null, T[]>();
  for (const item of items) groups.set(feature(item), [...groups.get(feature(item)) ?? [], item]);
  return [...groups.entries()]
    .sort(([a], [b]) => a === null ? 1 : b === null ? -1 : a.localeCompare(b, undefined, { sensitivity: "base", numeric: true }))
    .map(([key, list]) => ({ feature: key, items: list.sort(byName) }));
}
function JourneyRailButton({ mission, flow, duplicateTitle, selected, onSelect }: { mission: Mission; flow?: JourneyFlowView; duplicateTitle: boolean; selected: boolean; onSelect(): void }) {
  const { name, note } = splitDraftTitle(mission.title);
  const pathName = flow === undefined ? undefined : mission.contractId === flow.id ? "main" : mission.contractId.slice(flow.id.length + 1);
  const path = pathName === undefined ? undefined : flow?.paths[pathName];
  const edges = path?.map((id) => flow?.edges.find((edge) => edge.id === id));
  const stepIds = edges?.length && edges.every((edge) => edge !== undefined)
    ? [edges[0]!.from, ...edges.map((edge) => edge!.to)] : flow?.screens.map((screen) => screen.id);
  const stale = flow !== undefined && flow.title !== null && (flow.title !== mission.title
    || path === undefined
    || stepIds?.length !== mission.steps.length || stepIds.some((id, index) => id !== mission.steps[index]?.stepId)
    || flow.findings.some((finding) => finding.code === "flow.contract_stale" && finding.pointer === `/journeys/${mission.contractId}.json`));
  const title = stale && flow.title ? flow.title : name;
  // #1296: a short name reads like a menu item; the promise (or the long title) becomes its description.
  const short = flow?.name?.trim() ? (pathName && pathName !== "main" ? `${flow.name.trim()} · ${pathName}` : flow.name.trim()) : null;
  const description = short === null ? null : flow?.purpose?.trim() || title;
  return <button type="button" className="mv-journey mv-proof-journey" aria-pressed={selected} title={note ? `Draft: ${note}` : description ?? title} onClick={onSelect}>
    <span className="mv-journey-title">{short ?? title}</span>
    {description && <span className="mv-journey-desc">{description}</span>}
    {(duplicateTitle || stale) && <span className="mv-journey-id">{mission.contractId}</span>}
    {note && <span className="mv-draft-tag" title={note}>draft</span>}
    {stale && <span className="mv-draft-tag" title="The compiled contract does not match the current flow">contract out of date</span>}
    <span className="mv-journey-count">{`${mission.summary.proven}/${mission.summary.total}`}</span>
  </button>;
}
export function JourneyProgress({ statuses, caption = true }: { statuses: string[]; caption?: boolean }) {
  const total = statuses.length, proven = statuses.filter((s) => s === "proven").length;
  const work = statuses.filter((s) => s === "work").length;
  const merged = statuses.filter((s) => s === "merged").length;
  const remaining = total - proven - work - merged;
  const words = `${total}\u00a0${total === 1 ? "step" : "steps"} · ${proven}\u00a0proven · ${work}\u00a0in\u00a0work · ${merged}\u00a0merged,\u00a0not\u00a0proven · ${remaining}\u00a0not\u00a0proven`;
  const accessibleWords = `${total} ${total === 1 ? "step" : "steps"} · ${proven} proven · ${work} in work · ${merged} merged, not proven · ${remaining} not proven`;
  return (
    <span className="mv-progress">
      <span className="mv-progress-bar" role="img" aria-label={accessibleWords}>
        <span className="mv-progress-part" data-progress="proven" title="Proven" style={{ flexGrow: proven, background: SEG_TONE.proven }} />
        <span className="mv-progress-part" data-progress="work" title="In work (outlined)" style={{ flexGrow: work }} />
        <span className="mv-progress-part" data-progress="merged" title="Merged, not proven (hatched)" style={{ flexGrow: merged }} />
        <span className="mv-progress-part" data-progress="remaining" title="Not proven" style={{ flexGrow: remaining, background: SEG_TONE.ready }} />
      </span>
      {caption && <span className="mv-progress-text">{words}</span>}
    </span>
  );
}

function journeyStepProgress(mission: Mission): string[] {
  const placed = layoutMission(mission).placed;
  return mission.steps.map((step) => {
    if (step.status === "proven") return "proven";
    if (step.status === "preview_only" || step.status === "stale") return "remaining";
    const tasks = placed.filter((p) => p.col === step.index).map((p) => p.task);
    if (tasks.some((t) => t.step !== "merged")) return "work";
    if (tasks.some((t) => t.step === "merged")) return "merged";
    return "remaining";
  });
}

const SEG_TONE: Record<string, string> = { proven: "#4ADE9B", merged: "#8FB3D9", work: "#F5A524", stalled: "#FF6B5E", ready: "#24272E" };

type Selection = { kind: "group"; key: string } | { kind: "journey"; id: string };

export function ago(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86_400)} d ago`;
}

/** The PR ids of the tasks the graph places in each step's column. */
function stepIds(m: Mission): string[] {
  const placed = layoutMission(m).placed;
  return m.steps.map((s) => {
    const prs = placed.filter((p) => p.col === s.index).map((p) => (p.task.pr ? `#${p.task.pr}` : "no PR"));
    return prs.length ? prs.join(" ") : "";
  });
}

export function MissionView({ events = [], envelopes = {}, journeys: compiledJourneys, tasks, taskRecords = [], tasksRouteMissing = false, tasksRouteFailure = null, runFor, lanes, now, frameUrl, onMarkSafe, onSendBack, run, onRun, runName, lastRecordAt, onTeam, initialView = "graph", focusJourneyId = null, flows, onApprove, onWatch, agents = [], historyLoading = false, partial = false, lanesHistoryLoading = historyLoading, historyFailedCount = 0, onRetryHistory, onSelectJourney, away = null, onSignal, onReviewAssigned, wakeListeners, seenActors, seenActorAgesMs, seenActorSources, onDemandLanes = [], slots = [], runNodes = [], unassignedNodeIds, onOpenNode, onOpenBotDetails, onNameBot, livenessBudgets }: Props) {
  const freeAfterMs = livenessBudgets ? livenessBudgets.freeAfterSeconds * 1000 : laneBudgets(lanes).freeAfterMs;
  const journeys = useMemo(() => {
    const known = new Set(compiledJourneys.map((journey) => journey.contractId));
    return [...compiledJourneys, ...(flows?.flows ?? []).filter((flow) => !known.has(flow.id)).map((flow): JourneyView => ({
      contractId: flow.id, title: flow.title ?? flow.id, arrows: [],
      steps: flowStepOrder(flow).map((screen) => ({ stepId: screen.id, screen: { screenId: screen.id, title: screen.title ?? screen.id, scopePaths: [] }, promises: [] })),
    }))];
  }, [compiledJourneys, flows]);

  const [chosen, setChosen] = useState<Selection | null>(null);
  const [stepId, setStepId] = useState<string | null>(null);
  // undefined: the group's default (the task that most needs the owner); null: none, a column is chosen.
  const [taskKey, setTaskKey] = useState<string | null | undefined>(undefined);
  const [stageCol, setStageCol] = useState<number | null>(null);
  // #591: when the owner asked a lane for status, by PR + lane; kept here so it survives selecting another PR.
  const [askedAt, setAskedAt] = useState<Record<string, { at: number; listening: boolean | null }>>({});
  const [sub, setSub] = useState<Sub>(initialView);
  const [frame, setFrame] = useState(0);
  // #744: the journey the Proof rail shows; null: preselected from the Graph selection.
  const [proofJid, setProofJid] = useState<string | null>(focusJourneyId);
  useEffect(() => { if (focusJourneyId !== null) { setProofJid(focusJourneyId); setSub("proof"); } }, [focusJourneyId]);
  const [flowError, setFlowError] = useState<string | null>(null);
  const [flowBusy, setFlowBusy] = useState(false);
  // #591: one one-second clock for the live card timers (the Studio's shared one, #1380), anchored
  // on the `now` the parent gave: the cards advance by the time since that `now` arrived.
  const anchor = useMemo(() => Date.now(), [now]);
  const clock = useNow(1000);
  const live = now + Math.max(0, clock - anchor);
  const [unlinkedOpen, setUnlinkedOpen] = useState(false);
  const missions = useMemo(() => journeys.map((j) => {
    const flow = flowForContract(flows?.flows ?? [], j.contractId);
    const digest = flow === undefined ? undefined : flow.status === "approved" ? flow.approved?.digest ?? null : null;
    return buildMission(j, runFor(j.contractId), tasks, taskRecords, digest);
  }), [journeys, tasks, taskRecords, runFor, flows]);
  const [goneQuietOpen, setGoneQuietOpen] = useState(false);
  const proofTitles = missions.map((m) => {
    const flow = flowForContract(flows?.flows ?? [], m.contractId);
    return flow?.title && flow.title !== m.title ? flow.title : splitDraftTitle(m.title).name;
  });
  const railGroups = useMemo(() => groupJourneys(missions, (m) => flowForContract(flows?.flows ?? [], m.contractId)?.feature,
    (m) => flowForContract(flows?.flows ?? [], m.contractId)?.name?.trim() || proofTitles[missions.indexOf(m)] || m.contractId),
  [missions, flows]);
  const railOrder = railGroups.flatMap((g) => g.items);
  const groups = useMemo(() => buildWorkGroups(tasks, journeys, runFor, partial), [tasks, journeys, runFor, partial]);
  const orphans = useMemo(() => unlinkedTasks(journeys, tasks), [journeys, tasks]);
  const allTasks = useMemo(() => tasks.map(toMissionTask), [tasks]);
  // Default: the first group (open work sorts first, then the newest), else the first journey.
  const fallback: Selection | null = groups[0] ? { kind: "group", key: groups[0].key }
    : journeys[0] ? { kind: "journey", id: journeys[0].contractId } : null;
  const valid = (s: Selection | null) => s !== null && (s.kind === "group" ? groups.some((g) => g.key === s.key) : journeys.some((j) => j.contractId === s.id));
  const sel = valid(chosen) ? chosen! : fallback;
  const tabs: { id: Sub | "team"; label: string }[] = [
    { id: "graph", label: "Graph" }, { id: "lanes", label: "Lanes" }, { id: "proof", label: "Proof" },
  ];
  // A step test is Proof's content, so Proof stays the open tab.
  const openTab = sub === "test" ? "proof" : sub;
  const tabsRef = useRef<HTMLDivElement>(null);
  // Focus follows the open tab while it is in the strip: a click that does not move focus (scripted
  // or assistive) otherwise leaves the focus ring on the old tab, which reads as that tab being open.
  useEffect(() => {
    const strip = tabsRef.current;
    if (!strip || !strip.contains(document.activeElement)) return;
    strip.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus();
  }, [openTab]);
  const header = (
      <header className="mv-head">
        {runName !== undefined && (
          <div className="mv-crumbs">
            <span className="mv-logo" aria-hidden="true">G</span>
            <span className="mv-brand">GraphHelm</span><span className="mv-slash">/</span>
            <span className="mv-dim">{`Run ${runName}`}</span><span className="mv-slash">/</span>
            <span className="mv-brand">Mission graph</span>
          </div>
        )}
        <div role="tablist" aria-label="Mission views" className="mv-tabs" ref={tabsRef}>
          {tabs.map((t) => (
            <button key={t.id} role="tab" type="button" className="mv-tab" aria-selected={openTab === t.id} tabIndex={openTab === t.id ? 0 : -1}
              onClick={() => (t.id === "team" ? onTeam?.() : setSub(t.id))}>{t.label}</button>
          ))}
        </div>
        {sub === "test" && <button type="button" className="mv-back" onClick={() => setSub("proof")}>‹ Back to proof</button>}
        {lastRecordAt != null && Number.isFinite(lastRecordAt) && (
          <span className="mv-live" aria-label={`live · last record ${ago(now - lastRecordAt)}`}><span className="mv-live-dot" aria-hidden="true" />live<span className="mv-live-age">{` · last record ${ago(now - lastRecordAt)}`}</span></span>
        )}
      </header>
  );
  const cardRecords = events.filter((event) => {
    if (event.kind !== "signal_recorded" || !event.payload || typeof event.payload !== "object") return false;
    const payload = event.payload as { kind?: unknown; sourceKind?: unknown };
    return payload.kind === "keel.card" && payload.sourceKind === "node";
  });
  const keelCards = <section className="mv-pad" aria-label="Keel cards">
    <h2>Keel cards</h2>
    {historyLoading && <p>Card history: still reading the history</p>}
    {cardRecords.length > 0 && !historyLoading && partial && <p>Showing partial history</p>}
    {cardRecords.length === 0 && <p>No Keel card in the loaded history</p>}
    {cardRecords.map((event) => {
      const source = (event.payload as { sourceId?: unknown }).sourceId;
      const text = envelopes[event.sequence]?.text;
      const reading = envelopes as EnvelopeRecord & { __readingEnvelopeSequences?: number[]; __envelopeFailures?: Record<number, string> };
      const failure = reading.__envelopeFailures?.[event.sequence];
      return <article key={event.sequence}>
        <h3>{`${typeof source === "string" && source ? source : "Unknown node"} · record #${event.sequence}`}</h3>
        <p style={{ whiteSpace: "pre-wrap" }}>{text?.trim() || (failure !== undefined ? `Keel card recorded; its content could not be read: ${failure}`
          : reading.__readingEnvelopeSequences?.includes(event.sequence) ? "Keel card recorded; reading its content…" : "Keel card recorded; content unavailable")}</p>
      </article>;
    })}
  </section>;
  const page = onTeam ? "full" : undefined;
  // #668: every open PR as its Graph node, by stage, above the Agents board; a click opens it on the Graph.
  const kanbanItems: KanbanItem[] = groups.flatMap((g) => {
    const rows = tasks.filter((t) => g.rows.some((r) => r.key === t.key));
    return g.rows.filter((r) => r.open).map((r): KanbanItem => {
      const raw = rows.find((t) => t.key === r.key);
      return {
        groupKey: g.key, row: r, stage: g.stages[r.key]!, task: withCurrentReviewer(r.task, lanes),
        health: raw ? stageHealth(timerTask(raw), lanes, rows, live, false, slots) : null,
        pace: raw ? { progress: stageProgress(timerTask(raw), rows, live, lanes), activity: activity(ownerLane(raw), lanes, live, rows, ownerRole(raw), slots) } : null,
      };
    });
  });
  const openOnGraph = (groupKey: string, key: string) => {
    setChosen({ kind: "group", key: groupKey }); setStepId(null); setStageCol(null); setTaskKey(key); setSub("graph");
  };
  const routeMissing = <p className="mv-none mv-pad" role="alert">{RUNTIME_PREDATES_TASKS_ROUTE}</p>;
  const routeFailed = tasksRouteFailure === null ? null
    : <div role="alert">Tasks route failed · {tasksRouteFailure} · <button type="button" onClick={onRetryHistory}>Retry</button></div>;
  const lanesView = tasksRouteMissing ? routeMissing : <>
    <WorkKanban items={kanbanItems} onOpen={openOnGraph} historyLoading={lanesHistoryLoading} />
    <LanesTimeline lanes={lanes} freeAfterMs={freeAfterMs} now={now} windowMs={WINDOW_MS} tasks={allTasks} agents={agents} slots={slots} onDemandLanes={onDemandLanes} liveLanes={wakeListeners} seenActors={seenActors} seenActorAgesMs={seenActorAgesMs} seenActorSources={seenActorSources} historyLoading={lanesHistoryLoading}
      {...(onOpenBotDetails ? { onOpenBotDetails } : {})} {...(onNameBot ? { onNameBot } : {})} />
  </>;
  const runGraph = <RunGraphStrip nodes={runNodes} unassigned={unassignedNodeIds ?? []} agents={agents} now={now} freeAfterMs={freeAfterMs} {...(onOpenNode ? { onOpenNode } : {})} />;
  const contentRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (sub === "test" && stepId !== null) {
      contentRef.current?.querySelector(".tc-head")?.scrollIntoView?.({ block: "start" });
    }
  }, [sub, stepId]);
  // #run-all: like a test explorer. Each journey's last run is read from the Runtime's cache once
  // and kept until something it depends on changes (the flow digest: stale). Run all replays
  // every journey in one click, one after another; ▶ Run replays one.
  const runnable = useMemo(() => (run && (sub === "proof" || sub === "test") ? railOrder.filter((m) => flowForContract(flows?.flows ?? [], m.contractId) !== undefined).map((m) => m.contractId) : []), [run, sub, missions, flows]);
  const [batch, setBatch] = useState<{ ids: string[]; done: number; current: string | null } | null>(null);
  const [batchNote, setBatchNote] = useState<string | null>(null);
  const [batchRevision, setBatchRevision] = useState(0);
  const readCached = useRef(new Set<string>());
  useEffect(() => {
    if (!run || !onRun) return;
    // Every journey, on every tab: the rail's x/y and verdict must come from the cache on load,
    // not only after the journey is clicked.
    for (const id of journeys.map((j) => j.contractId)) {
      if (readCached.current.has(id) || runFor(id) !== null) continue;
      readCached.current.add(id);
      run.read(id).then((value) => onRun(id, value), () => {});
    }
  }, [run, onRun, journeys, runFor]);
  const verdictOf = (m: Mission): JourneyVerdict => (batch?.current === m.contractId ? "running" : journeyVerdict(m.steps.map((step) => step.status), runFor(m.contractId)));
  const verdictCount = (v: JourneyVerdict) => missions.filter((m) => runnable.includes(m.contractId) && verdictOf(m) === v).length;
  const startBatch = (ids: string[]) => {
    if (!run || batch !== null) return;
    setBatchNote(null);
    setBatch({ ids, done: 0, current: null });
    void runJourneys(ids, run, (id, value) => onRun?.(id, value), (id) => setBatch((before) => before && ({ ...before, current: id, done: id === null ? before.done : Math.max(0, before.ids.indexOf(id)) }))).then((outcome) => {
      setBatch(null);
      setBatchRevision((n) => n + 1);
      if (outcome.busy) setBatchNote("Another journey is still running in this tab. Try again when it ends.");
      else if (Object.keys(outcome.errors).length > 0) setBatchNote(`Couldn't start ${Object.keys(outcome.errors).length} journey(s): ${Object.values(outcome.errors)[0]}`);
    });
  };
  if (sel === null) return <div className="mv" data-wide="true" data-page={page}>
    {routeFailed}{historyFailedCount > 0 && <div role="alert">History partly loaded · {historyFailedCount} records could not be read · <button type="button" onClick={onRetryHistory}>Retry</button></div>}{header}
    {sub === "lanes" ? <div className="mv-pad">{lanesView}</div>
      : <>{sub === "graph" && <>{runGraph}{keelCards}</>}{tasksRouteMissing ? routeMissing : <p className="mv-none mv-pad" role={historyLoading ? "status" : undefined}>{historyLoading ? "Showing recent work · still reading the history" : "No journeys in this project yet"}</p>}</>}</div>;
  const rawGroup: WorkGroup | null = sel.kind === "group" ? groups.find((g) => g.key === sel.key)! : null;
  // #591: the live Review row names only the current reviewer (latest assignment on the current head).
  const group: WorkGroup | null = rawGroup ? { ...rawGroup, rows: rawGroup.graphRows.map((r) => ({ ...r, task: withCurrentReviewer(r.task, lanes) })) } : null;
  // #591: time in stage and the health flag of each PR card, from the same records the lanes read.
  const groupRows = group ? tasks.filter((t) => group.rows.some((r) => r.key === t.key)) : [];
  const health = Object.fromEntries(groupRows.map((t) => [t.key, stageHealth(timerTask(t), lanes, groupRows, live, false, slots)]));
  // #591: progress against the usual time, and the last record of whoever is on the card's stage.
  const pace = Object.fromEntries(groupRows.map((t) => [t.key, {
    progress: stageProgress(timerTask(t), groupRows, live, lanes),
    activity: activity(ownerLane(t), lanes, live, groupRows, ownerRole(t), slots),
  }]));
  const knownIds = new Set(journeys.map((j) => j.contractId));
  // #744: Proof and Test list journeys only. Preselect the Graph selection's journey, else the first
  // journey with a recorded run, else the first.
  const proofMode = sub === "proof" || sub === "test";
  const linkedId = sel.kind === "journey" ? sel.id : group!.journeyIds.find((id) => knownIds.has(id)) ?? null;
  const proofId = proofJid !== null && knownIds.has(proofJid) ? proofJid
    : linkedId ?? journeys.find((j) => { const r = runFor(j.contractId); return r !== null && r.state !== "none"; })?.contractId ?? journeys[0]?.contractId ?? null;
  const journey = journeys.find((j) => j.contractId === (proofMode ? proofId : linkedId)) ?? null;
  const contractId = journey?.contractId ?? null;
  const flow: JourneyFlowView | undefined = contractId ? flowForContract(flows?.flows ?? [], contractId) : undefined;
  const pendingFlows = flows?.flows.filter((item) => item.status === "draft" || item.status === "approval_stale" || item.status === "weak") ?? [];
  const approvableCount = pendingFlows.filter((item) => {
    const contract = journeys.find((candidate) => candidate.contractId === item.id);
    return item.approvable && contract !== undefined && approvalBlockReason(item, contract, runFor(contract.contractId)) === null;
  }).length;
  const mission = contractId ? missions.find((m) => m.contractId === contractId)! : null;
  const openTest = (id: string) => {
    if (!journey) return;
    onSelectJourney?.(journey.contractId);
    setStepId(id);
    setFrame(Math.max(0, journey.steps.findIndex((s) => s.stepId === id)));
    if (contentRef.current) contentRef.current.scrollTop = 0;
    setSub("test");
  };
  const reset = () => { setProofJid(null); setStepId(null); setTaskKey(undefined); setStageCol(null); if (sub === "test") setSub("graph"); };
  const pickJourney = (id: string) => { onSelectJourney?.(id); setChosen({ kind: "journey", id }); reset(); };
  const pickProof = (id: string) => { onSelectJourney?.(id); setProofJid(id); setStepId(null); setFlowError(null); setSub("proof"); };
  const pickGroup = (key: string, task: string | null = null) => {
    setChosen({ kind: "group", key }); reset(); setTaskKey(task ?? undefined);
    if (sub === "test") setSub("graph");
  };
  const pickStep = (jid: string, sid: string) => {
    onSelectJourney?.(jid);
    if (sel.kind !== "journey" || jid !== contractId) pickJourney(jid);
    setStepId(sid); setTaskKey(null);
    if (sub !== "graph" && sub !== "proof") setSub("graph");
  };
  const pickTask = (key: string) => {
    setTaskKey(key);
    if (!mission) return;
    const placed = layoutMission(mission).placed.find((p) => p.task.key === key);
    if (placed) setStepId(mission.steps[placed.col]?.stepId ?? null);
  };
  const stepFor = (key: string) => {
    if (!mission) return undefined;
    const placed = layoutMission(mission).placed.find((p) => p.task.key === key);
    return placed ? mission.steps[placed.col] : undefined;
  };
  // #591: Nudge / Reassign the lane that owns the selected PR's current step.
  const roster = laneRoster(agents.map((b) => b.name), lanes);
  const goneQuiet = agents.filter((b) => {
    const at = b.lastRecordAt === null ? null : Date.parse(b.lastRecordAt);
    return at !== null && !Number.isNaN(at) && now - at > GONE_QUIET_MS;
  });
  const goneQuietKeys = new Set(goneQuiet.map((b) => b.key));
  const activeAgents = agents.filter((b) => !goneQuietKeys.has(b.key));
  const inspectorActions = onSignal && group ? (key: string) => {
    const t = groupRows.find((r) => r.key === key);
    const lane = t ? realName(ownerLane(t)) : null;
    if (!t || !lane) return null;
    const seen = laneLiveness(lane, lanes, tasks, live, slots).sinceMs;
    const stage = WORK_STAGES.find((s) => s.id === group.stages[key])?.label ?? t.step;
    const id = `${t.pr ?? key}:${lane}`;
    return <LaneActions key={`${key}:${lane}`} lane={lane} step={stage} pr={t.pr} roster={roster} now={live}
      lastSeenAt={seen === null ? null : live - seen} send={onSignal}
      assignReview={async (reviewer) => {
        const task = tasks.find((row) => row.key === key);
        if (!task || !onReviewAssigned) throw new Error("Review assignment is unavailable.");
        await onReviewAssigned(task, reviewer);
      }}
      askedAt={askedAt[id]?.at ?? null} listening={wakeListeners?.includes(lane) ?? null}
      answered={askedAt[id] !== undefined && laneRepliedAfterAsk(events, envelopes, lane, askedAt[id].at)}
      askedListening={askedAt[id]?.listening} onAsked={(at, listening) => setAskedAt((m) => ({ ...m, [id]: { at, listening } }))} />;
  } : undefined;
  const shownTask = taskKey === undefined ? group?.focus ?? null : taskKey;
  const ids = mission ? stepIds(mission) : [];
  const gsum = group ? {
    proven: group.rows.filter((r) => group.stages[r.key] === "proven").length,
    inFlight: group.rows.filter((r) => r.task.step !== "merged").length,
    ready: group.rows.filter((r) => r.task.lane === null).length,
    needYou: group.rows.filter((r) => r.task.blocked).length,
  } : null;
  const summary = gsum && sub === "graph" ? (
    <p className="mg-summary mv-summary" aria-label="Summary">
      <span className="mg-sum" data-tone="proven"><b>{`${gsum.proven}/${group!.tasks.length}`}</b> proven</span>
      <span className="mg-sum" data-tone="work"><b>{gsum.inFlight}</b> in flight</span>
      <span className="mg-sum" data-tone="ready"><b>{gsum.ready}</b> Ready · unclaimed</span>
      <span className="mg-sum" data-tone="stalled"><b>{gsum.needYou}</b> need you</span>
      {away && <><span className="mv-sum-fill" /><span className="mv-away">{awayText(away)}</span></>}
    </p>
  ) : null;
  return (
    // data-wide: the Studio layout gives the mission view the whole main area (mission-view.css).
    <div className="mv" data-wide="true" data-page={page}>
      {routeFailed}
      {historyFailedCount > 0 && <div role="alert">History partly loaded · {historyFailedCount} records could not be read · <button type="button" onClick={onRetryHistory}>Retry</button></div>}
      {header}
      {page !== "full" && summary}
      <div className="mv-body">
        {page === "full" && summary}
        {page === "full" && sub === "graph" && runGraph}
        {sub === "graph" && tasksRouteMissing && routeMissing}
        {sub === "graph" && !tasksRouteMissing && historyLoading && <p className="mv-none" role="status">Showing recent work · still reading the history</p>}
        {proofMode && (
          <nav aria-label="Journeys" className="mv-journeys">
            <span className="mv-cap">Journeys</span>
            {pendingFlows.length > 0 && <span className="mv-cap">{approvableCount > 0 ? `${approvableCount} to approve` : `${pendingFlows.length} awaiting proof`}</span>}
            {runnable.length > 0 && <div className="mv-runall">
              <button type="button" className="mv-runall-btn" disabled={batch !== null} onClick={() => startBatch(runnable)}>
                {batch !== null && batch.ids.length > 1 ? `Running ${batch.done + 1}/${batch.ids.length}…` : "▶ Run all"}
              </button>
              <span className="mv-runall-sum">{`${verdictCount("passed")} passed · ${verdictCount("failed")} failed · ${verdictCount("stale")} stale · ${verdictCount("not_run")} not run`}</span>
              {batchNote && <p role="alert" className="mv-runall-note">{batchNote}</p>}
            </div>}
            {missions.length === 0 && <p className="mv-none">No journeys in this project yet</p>}
            {railGroups.map((g) => {
              const cards = g.items.map((m) => {
              const on = m.contractId === contractId;
              return (
                <div key={m.contractId} className="mv-jcard" data-selected={on}>
                  <JourneyRailButton mission={m} flow={flowForContract(flows?.flows ?? [], m.contractId)} duplicateTitle={proofTitles.filter((title) => title === proofTitles[missions.indexOf(m)]).length > 1} selected={on} onSelect={() => pickProof(m.contractId)} />
                  {runnable.includes(m.contractId) && <span className="mv-jrun">
                    <span className="mv-verdict" data-verdict={verdictOf(m)} title={VERDICT_HINT[verdictOf(m)]}>{VERDICT_LABEL[verdictOf(m)]}</span>
                    <button type="button" className="mv-jrun-btn" aria-label={`Run journey ${m.title}`} disabled={batch !== null} onClick={() => startBatch([m.contractId])}>▶ Run</button>
                  </span>}
                  {historyLoading ? <span className="mv-progress-text" role="status">… loading work</span> : <JourneyProgress statuses={journeyStepProgress(m)} caption={false} />}
                </div>
              );
              });
              if (railGroups.length === 1 && g.feature === null) return cards;
              const passed = g.items.filter((m) => verdictOf(m) === "passed").length;
              const failed = g.items.filter((m) => verdictOf(m) === "failed").length;
              return (
                <details key={g.feature ?? "\u0000other"} className="mv-jgroup" open>
                  <summary className="mv-jgroup-head">
                    <span className="mv-jgroup-name">{g.feature ?? "Other"}</span>
                    {failed > 0 && <span className="mv-verdict" data-verdict="failed">{`${failed} failed`}</span>}
                    <span className="mv-journey-count">{runnable.length > 0 ? `${passed}/${g.items.length}` : g.items.length}</span>
                  </summary>
                  {cards}
                </details>
              );
            })}
          </nav>
        )}
        {sub === "graph" && (
          <nav aria-label="Journeys" className="mv-journeys">
            {groups.length > 0 && (
              <section aria-label="Work by issue" className="mv-groups">
                <span className="mv-cap">Work</span>
                {groups.map((g) => {
                  const on = group?.key === g.key;
                  const visibleTasks = g.graphRows.map((r) => r.task);
                  const prs = visibleTasks.filter((t) => t.pr !== null).length;
                  if (!on) return (
                    <div key={g.key} className="mv-jcard" data-selected={false}>
                      <button type="button" className="mv-journey mv-group" aria-pressed={false} title={g.label} onClick={() => pickGroup(g.key)}>
                        <span className="mv-journey-title">{g.label}</span>
                      </button>
                      <GroupProgress states={visibleTasks.map((t) => stageState(g.stages[t.key]!, t))} prs={prs} />
                      {g.contested.length > 0 && <span className="mv-journey-meta mv-contested" data-tone="stalled" title="Two lanes hold open claims on this issue: one of them is duplicate work">{`${g.contested.length} claims: ${g.contested.join(", ")}`}</span>}
                      {visibleTasks.length === 0 && g.releasedKeys.length > 0 && <span className="mv-journey-meta">released</span>}
                    </div>
                  );
                  return (
                    <div key={g.key} className="mv-jcard" data-selected={true}>
                      <button type="button" className="mv-journey mv-group" aria-pressed={true} title={g.label} onClick={() => pickGroup(g.key)}>
                        <span className="mv-journey-title">{g.label}</span>
                      </button>
                      <GroupProgress states={visibleTasks.map((t) => stageState(g.stages[t.key]!, t))} prs={prs} />
                      {g.contested.length > 0 && <span className="mv-journey-meta mv-contested" data-tone="stalled" title="Two lanes hold open claims on this issue: one of them is duplicate work">{`${g.contested.length} claims: ${g.contested.join(", ")}`}</span>}
                      {visibleTasks.length === 0 && g.releasedKeys.length > 0 && <span className="mv-journey-meta">released</span>}
                      <span className="mv-journey-meta">{`${g.issue !== null ? `issue #${g.issue}` : partial ? "issue not read yet" : "no issue"} · ${prs} ${prs === 1 ? "PR" : "PRs"}`}</span>
                      <div className="mv-chips">
                        {visibleTasks.map((t, i) => {
                          const stage = g.stages[t.key]!;
                          const busy = t.step !== "merged", alert = t.blocked || health[t.key]?.flag === "stalled";
                          return (
                            <button key={t.key} type="button" className="mv-chip mv-pr-chip" data-state={stageState(stage, t)} data-stage={stage}
                              aria-pressed={t.key === shownTask} aria-label={`${t.pr ? `PR #${t.pr}` : "No PR"}: ${t.title}`}
                              data-alert={alert && busy} onClick={() => pickGroup(g.key, t.key)}>{i + 1}</button>
                          );
                        })}
                      </div>
                      <div className="mv-steps">
                        {visibleTasks.map((t, i) => (
                          <button key={t.key} type="button" className="mv-step" data-state={stageState(g.stages[t.key]!, t)} aria-pressed={t.key === shownTask}
                            onClick={() => pickGroup(g.key, t.key)}>
                            <span className="mv-tick" />
                            <span className="mv-step-n">{i + 1}</span>
                            <span className="mv-step-title" title={t.title}>{t.title}</span>
                            <span className="mv-step-ids">{t.pr ? `PR #${t.pr}` : "no PR"}</span>
                          </button>
                        ))}
                      </div>
                    </div>
                  );
                })}
              </section>
            )}
            <span className="mv-cap">Journeys</span>
            {missions.length === 0 && <p className="mv-none">No journeys in this project yet</p>}
            {missions.map((m) => {
              const on = sel.kind === "journey" && m.contractId === contractId;
              const issue = m.tasks.find((t) => t.issue !== null)?.issue ?? null;
              return (
                <div key={m.contractId} className="mv-jcard" data-selected={on}>
                  <button type="button" className="mv-journey" aria-pressed={on} title={m.title} onClick={() => pickProof(m.contractId)}>
                    <span className="mv-journey-title">{m.title}</span>
                    <span className="mv-journey-count">{`${m.summary.proven}/${m.summary.total}`}</span>
                  </button>
                  {issue !== null && <span className="mv-journey-meta">{`issue #${issue} · ${m.tasks.length} ${m.tasks.length === 1 ? "task" : "tasks"}`}</span>}
                  <div className="mv-chips">
                    {m.steps.map((s) => (
                      <button key={s.stepId} type="button" className="mv-chip" data-status={s.status}
                        aria-pressed={on && s.stepId === stepId} aria-label={`Step ${s.index + 1}: ${s.title}, ${STATUS_LABEL[s.status]}`}
                        onClick={() => pickStep(m.contractId, s.stepId)}>{s.index + 1}</button>
                    ))}
                  </div>
                  {on && (
                    <div className="mv-steps">
                      {m.steps.map((s) => (
                        <button key={s.stepId} type="button" className="mv-step" data-status={s.status} aria-pressed={s.stepId === stepId}
                          onClick={() => pickStep(m.contractId, s.stepId)}>
                          <span className="mv-tick" data-status={s.status} />
                          <span className="mv-step-n">{s.index + 1}</span>
                          <span className="mv-step-title" title={s.title}>{s.title}</span>
                          <span className="mv-step-ids">{ids[s.index] || "—"}</span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
            {orphans.length > 0 && (
              <section aria-label="Unlinked work" className="mv-unlinked">
                <button type="button" className="mv-fold" aria-expanded={unlinkedOpen} onClick={() => setUnlinkedOpen((o) => !o)}>
                  {`Unlinked work · ${orphans.length}`}
                </button>
                {unlinkedOpen && <ul>{orphans.map((t) => <li key={t.key}>{`${t.pr ? `#${t.pr}` : "no PR"} ${t.title}`}</li>)}</ul>}
              </section>
            )}
            {!historyLoading && agents.length > 0 && (
              <section aria-label="Agents right now" className="mv-agents">
                <span className="mv-cap">Agents right now</span>
                {activeAgents.map((b) => (
                  <div key={b.key} className="mv-agent">
                    <span className="mv-agent-dot" data-tone={agentTone(b)} />
                    <span className="mv-agent-name">{b.name}</span>
                    <span className="mv-agent-doing">{agentActivity(b)}</span>
                  </div>
                ))}
                {goneQuiet.length > 0 && (
                  <div className="mv-gone-quiet">
                    <button type="button" className="mv-fold" aria-expanded={goneQuietOpen} onClick={() => setGoneQuietOpen((open) => !open)}>
                      {`Gone quiet · ${goneQuiet.length}`}
                    </button>
                    {goneQuietOpen && goneQuiet.map((b) => (
                      <div key={b.key} className="mv-agent">
                        <span className="mv-agent-dot" data-tone={agentTone(b)} />
                        <span className="mv-agent-name">{b.name}</span>
                        <span className="mv-agent-doing">{agentActivity(b)}</span>
                      </div>
                    ))}
                  </div>
                )}
              </section>
            )}
          </nav>
        )}
        <div className="mv-content" ref={contentRef}>
          {page !== "full" && sub === "graph" && runGraph}
          {sub === "graph" && keelCards}
          {sub === "graph" && group && <IssueGraph group={group} stepFor={stepFor} selectedTaskKey={shownTask} selectedCol={stageCol}
            onSelectTask={(key) => { setTaskKey(key); setStageCol(null); }} onSelectCol={(c) => { setStageCol(c); setTaskKey(null); }} onOpenTest={openTest} health={health} pace={pace}
            {...(inspectorActions ? { inspectorActions } : {})} />}
          {sub === "graph" && !group && mission && (!historyLoading || partial) && <MissionGraph mission={mission} selectedStepId={stepId} selectedTaskKey={taskKey ?? null}
            onSelectStep={(id) => { setStepId(id); setTaskKey(null); }} onSelectTask={pickTask} onOpenTest={openTest} />}
          {sub === "proof" && (mission && contractId ? <ProofTable key={`${contractId}:${batchRevision}`} mission={mission} flow={flow} onOpenTest={openTest} frameUrl={(id) => frameUrl(id, contractId)}
            run={run} onApprove={onApprove} onWatch={onWatch} busy={flowBusy} onBusy={setFlowBusy} error={flowError} onActionError={setFlowError}
            {...(onRun ? { onRun: (value: JourneyRunView) => onRun(contractId, value) } : {})}
            />
            : <p className="mv-empty">No journeys in this project yet</p>)}
          {sub === "test" && journey && contractId && (run
            ? <ProofRun key={contractId} journey={journey} flow={flow} source={run} selected={frame} onSelect={setFrame}
              frameUrl={(id) => frameUrl(id, contractId)} onMarkSafe={(id) => onMarkSafe(id, contractId)}
              {...(onRun ? { onRun: (r: JourneyRunView) => onRun(contractId, r) } : {})}
              {...(onSendBack ? { onSendBack: (id: string) => onSendBack(id, contractId) } : {})} />
            : <TestCanvas frames={testFrames(journey, runFor(contractId), null, flow)} purpose={flow?.purpose ?? null} selected={frame} onSelect={setFrame}
              frameUrl={(id) => frameUrl(id, contractId)} onMarkSafe={(id) => onMarkSafe(id, contractId)}
              {...(onSendBack ? { onSendBack: (id: string) => onSendBack(id, contractId) } : {})} />)}
          {sub === "lanes" && lanesView}
        </div>
      </div>
    </div>
  );
}
