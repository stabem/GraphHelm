import "./mission-view.css";
import { useMemo, useState } from "react";
import type { JourneyRunView, JourneyView } from "../runtime/types";
import type { TaskState } from "../runtime/team-tasks";
import type { Lane } from "../runtime/lane-bars";
import { buildMission, toMissionTask, unlinkedTasks, type Mission } from "../runtime/mission";
import { layoutMission } from "../runtime/mission-layout";
import { testFrames } from "../runtime/test-frames";
import { buildWorkGroups, type WorkGroup } from "../runtime/work-groups";
import { IssueGraph, MissionGraph, STATUS_LABEL, stageState } from "./mission-graph";
import { ProofTable } from "./proof-table";
import { TestCanvas } from "./test-canvas";
import { LanesTimeline } from "./lanes-timeline";
import type { Bot } from "../runtime/team";

type Sub = "graph" | "proof" | "test" | "lanes";
const WINDOW_MS = 14 * 3_600_000;

interface Props {
  journeys: JourneyView[];
  tasks: TaskState[];
  runFor(contractId: string): JourneyRunView | null;
  lanes: Lane[];
  now: number;
  frameUrl(stepId: string, contractId: string): string | null;
  /** The step and the journey it belongs to: the mark is recorded against that journey's flow. */
  onMarkSafe(stepId: string, contractId: string): void | Promise<void>;
  /** Absent: the Test canvas shows Send back as not available yet. */
  onSendBack?: (stepId: string, contractId: string) => void;
  /** Absent: the Proof view's "Replay whole journey" is disabled and says where the replay starts. */
  onReplay?: (contractId: string) => void;
  /** #583 full page: the run named in the top bar's breadcrumb. */
  runName?: string;
  /** #583: when the newest record was written (ms); absent, the live indicator is omitted. */
  lastRecordAt?: number | null;
  /** #583: present, the top nav carries Team, which leaves the Graph page. */
  onTeam?: () => void;
  /** #591: "Agents right now" in the rail, from the team model. */
  agents?: Bot[];
  /** #591: the "While you were away" gap, for the summary's `N h away · N shipped`; absent, omitted. */
  away?: { minutes: number; shipped: number } | null;
}

/** #591: the rail's dot colour family for a bot: working amber, waiting or quiet red, done grey. */
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

export function MissionView({ journeys, tasks, runFor, lanes, now, frameUrl, onMarkSafe, onSendBack, onReplay, runName, lastRecordAt, onTeam, agents = [], away = null }: Props) {
  const [chosen, setChosen] = useState<Selection | null>(null);
  const [stepId, setStepId] = useState<string | null>(null);
  // undefined: the group's default (the task that most needs the owner); null: none, a column is chosen.
  const [taskKey, setTaskKey] = useState<string | null | undefined>(undefined);
  const [stageCol, setStageCol] = useState<number | null>(null);
  const [sub, setSub] = useState<Sub>("graph");
  const [frame, setFrame] = useState(0);
  const [unlinkedOpen, setUnlinkedOpen] = useState(false);
  const missions = useMemo(() => journeys.map((j) => buildMission(j, runFor(j.contractId), tasks)), [journeys, tasks, runFor]);
  const groups = useMemo(() => buildWorkGroups(tasks, journeys, runFor), [tasks, journeys, runFor]);
  const orphans = useMemo(() => unlinkedTasks(journeys, tasks), [journeys, tasks]);
  const allTasks = useMemo(() => tasks.map(toMissionTask), [tasks]);
  // Default: the first group (open work sorts first, then the newest), else the first journey.
  const fallback: Selection | null = groups[0] ? { kind: "group", key: groups[0].key }
    : journeys[0] ? { kind: "journey", id: journeys[0].contractId } : null;
  const valid = (s: Selection | null) => s !== null && (s.kind === "group" ? groups.some((g) => g.key === s.key) : journeys.some((j) => j.contractId === s.id));
  const sel = valid(chosen) ? chosen! : fallback;
  const tabs: { id: Sub | "team"; label: string }[] = [
    { id: "graph", label: "Graph" }, { id: "lanes", label: "Lanes" }, { id: "proof", label: "Proof" },
    ...(onTeam ? [{ id: "team" as const, label: "Team" }] : []),
  ];
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
        <div role="tablist" aria-label="Mission views" className="mv-tabs">
          {tabs.map((t) => (
            <button key={t.id} role="tab" type="button" className="mv-tab" aria-selected={sub === t.id}
              onClick={() => (t.id === "team" ? onTeam?.() : setSub(t.id))}>{t.label}</button>
          ))}
        </div>
        {sub === "test" && <button type="button" className="mv-back" onClick={() => setSub("proof")}>‹ Back to proof</button>}
        {lastRecordAt != null && Number.isFinite(lastRecordAt) && (
          <span className="mv-live"><span className="mv-live-dot" aria-hidden="true" />{`live · last record ${ago(now - lastRecordAt)}`}</span>
        )}
      </header>
  );
  const page = onTeam ? "full" : undefined;
  if (sel === null) return <div className="mv" data-wide="true" data-page={page}>{header}
    {sub === "lanes" ? <div className="mv-pad"><LanesTimeline lanes={lanes} now={now} windowMs={WINDOW_MS} tasks={allTasks} /></div>
      : <p className="mv-none mv-pad">No journeys in this project yet</p>}</div>;
  const group: WorkGroup | null = sel.kind === "group" ? groups.find((g) => g.key === sel.key)! : null;
  const knownIds = new Set(journeys.map((j) => j.contractId));
  // A group's Proof and Test follow its first linked journey; a journey selection is its own.
  const journey = sel.kind === "journey" ? journeys.find((j) => j.contractId === sel.id)!
    : journeys.find((j) => j.contractId === group!.journeyIds.find((id) => knownIds.has(id))) ?? null;
  const contractId = journey?.contractId ?? null;
  const mission = contractId ? missions.find((m) => m.contractId === contractId)! : null;
  const openTest = (id: string) => {
    if (!journey) return;
    setStepId(id);
    setFrame(Math.max(0, journey.steps.findIndex((s) => s.stepId === id)));
    setSub("test");
  };
  const reset = () => { setStepId(null); setTaskKey(undefined); setStageCol(null); if (sub === "test") setSub("graph"); };
  const pickJourney = (id: string) => { setChosen({ kind: "journey", id }); reset(); };
  const pickGroup = (key: string, task: string | null = null) => {
    setChosen({ kind: "group", key }); reset(); setTaskKey(task ?? undefined);
    if (sub === "test") setSub("graph");
  };
  const pickStep = (jid: string, sid: string) => {
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
  const shownTask = taskKey === undefined ? group?.focus ?? null : taskKey;
  const ids = mission ? stepIds(mission) : [];
  const gsum = group ? {
    proven: group.tasks.filter((t) => group.stages[t.key] === "proven").length,
    inFlight: group.tasks.filter((t) => t.step !== "merged").length,
    ready: group.tasks.filter((t) => t.lane === null).length,
    needYou: group.tasks.filter((t) => t.blocked).length,
  } : null;
  return (
    // data-wide: the Studio layout gives the mission view the whole main area (mission-view.css).
    <div className="mv" data-wide="true" data-page={page}>
      {header}
      {gsum && sub !== "lanes" && (
        <p className="mg-summary mv-summary" aria-label="Summary">
          <span className="mg-sum" data-tone="proven"><b>{`${gsum.proven}/${group!.tasks.length}`}</b> proven</span>
          <span className="mg-sum" data-tone="work"><b>{gsum.inFlight}</b> in flight</span>
          <span className="mg-sum" data-tone="ready"><b>{gsum.ready}</b> ready, unclaimed</span>
          <span className="mg-sum" data-tone="stalled"><b>{gsum.needYou}</b> need you</span>
          {away && <><span className="mv-sum-fill" /><span className="mv-away">{awayText(away)}</span></>}
        </p>
      )}
      <div className="mv-body">
        {sub !== "lanes" && (
          <nav aria-label="Journeys" className="mv-journeys">
            {groups.length > 0 && (
              <section aria-label="Work by issue" className="mv-groups">
                <span className="mv-cap">Work</span>
                {groups.map((g) => {
                  const on = group?.key === g.key;
                  const prs = g.tasks.filter((t) => t.pr !== null).length;
                  const proven = g.tasks.filter((t) => g.stages[t.key] === "proven").length;
                  const count = `${proven} / ${g.tasks.length}`;
                  if (!on) return (
                    <div key={g.key} className="mv-jcard" data-selected={false}>
                      <button type="button" className="mv-journey mv-group" aria-pressed={false} title={g.label} onClick={() => pickGroup(g.key)}>
                        <span className="mv-journey-title">{g.label}</span>
                        <span className="mv-journey-count">{count}</span>
                      </button>
                      <span className="mv-segs" aria-hidden="true">
                        {g.tasks.map((t) => <span key={t.key} className="mv-seg" style={{ background: SEG_TONE[stageState(g.stages[t.key]!, t)] }} />)}
                      </span>
                    </div>
                  );
                  return (
                    <div key={g.key} className="mv-jcard" data-selected={true}>
                      <button type="button" className="mv-journey mv-group" aria-pressed={true} title={g.label} onClick={() => pickGroup(g.key)}>
                        <span className="mv-journey-title">{g.label}</span>
                        <span className="mv-journey-count">{count}</span>
                      </button>
                      <span className="mv-journey-meta">{`${g.issue !== null ? `issue #${g.issue}` : "no issue"} · ${prs} ${prs === 1 ? "PR" : "PRs"}`}</span>
                      <div className="mv-chips">
                        {g.tasks.map((t, i) => {
                          const stage = g.stages[t.key]!;
                          return (
                            <button key={t.key} type="button" className="mv-chip mv-pr-chip" data-state={stageState(stage, t)} data-stage={stage}
                              aria-pressed={t.key === shownTask} aria-label={`${t.pr ? `PR #${t.pr}` : "No PR"}: ${t.title}`}
                              onClick={() => pickGroup(g.key, t.key)}>{i + 1}</button>
                          );
                        })}
                      </div>
                      <div className="mv-steps">
                        {g.tasks.map((t, i) => (
                          <button key={t.key} type="button" className="mv-step" data-state={stageState(g.stages[t.key]!, t)} aria-pressed={t.key === shownTask}
                            onClick={() => pickGroup(g.key, t.key)}>
                            <span className="mv-tick" />
                            <span className="mv-step-n">{i + 1}</span>
                            <span className="mv-step-title">{t.title}</span>
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
                  <button type="button" className="mv-journey" aria-pressed={on} title={m.title} onClick={() => pickJourney(m.contractId)}>
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
                          <span className="mv-step-title">{s.title}</span>
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
            {agents.length > 0 && (
              <section aria-label="Agents right now" className="mv-agents">
                <span className="mv-cap">Agents right now</span>
                {agents.map((b) => (
                  <div key={b.key} className="mv-agent">
                    <span className="mv-agent-dot" data-tone={agentTone(b)} />
                    <span className="mv-agent-name">{b.name}</span>
                    <span className="mv-agent-doing">{agentActivity(b)}</span>
                  </div>
                ))}
              </section>
            )}
          </nav>
        )}
        <div className="mv-content">
          {sub === "graph" && group && <IssueGraph group={group} stepFor={stepFor} selectedTaskKey={shownTask} selectedCol={stageCol}
            onSelectTask={(key) => { setTaskKey(key); setStageCol(null); }} onSelectCol={(c) => { setStageCol(c); setTaskKey(null); }} onOpenTest={openTest} />}
          {sub === "graph" && !group && mission && <MissionGraph mission={mission} selectedStepId={stepId} selectedTaskKey={taskKey ?? null}
            onSelectStep={(id) => { setStepId(id); setTaskKey(null); }} onSelectTask={pickTask} onOpenTest={openTest} />}
          {sub === "proof" && (mission && contractId ? <ProofTable mission={mission} onOpenTest={openTest} frameUrl={(id) => frameUrl(id, contractId)}
            {...(onReplay ? { onReplay: () => onReplay(contractId) } : {})} />
            : <p className="mv-empty">This work names no journey yet — agents pass --journeys when they claim.</p>)}
          {sub === "test" && journey && contractId && <TestCanvas frames={testFrames(journey, runFor(contractId))} selected={frame} onSelect={setFrame}
            frameUrl={(id) => frameUrl(id, contractId)} onMarkSafe={(id) => onMarkSafe(id, contractId)}
            {...(onSendBack ? { onSendBack: (id: string) => onSendBack(id, contractId) } : {})} />}
          {sub === "lanes" && <LanesTimeline lanes={lanes} now={now} windowMs={WINDOW_MS} tasks={allTasks} />}
        </div>
      </div>
    </div>
  );
}
