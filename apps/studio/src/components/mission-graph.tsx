import "./mission-graph.css";
import { useState, type CSSProperties, type ReactNode } from "react";
import { custodyRows, sha8, TRUST_LABELS, type Mission, type MissionStep, type MissionTask, type StepStatus } from "../runtime/mission";
import { layoutMission, type PlacedTask } from "../runtime/mission-layout";
import { liveDuration, stageDuration, type Activity, type StageHealth, type StageProgress } from "../runtime/stage-health";
import { GRAPH_STAGES, graphCol, graphRow, type PrRow, type WorkGroup, type WorkStage } from "../runtime/work-groups";

export const STATUS_LABEL: Record<StepStatus, string> = {
  proven: "Proven", failed: "Failed", needs_you: "Needs you", preview_only: "Preview only", not_run: "Not run",
};

/** The colour family a node is drawn in (mission-view.css defines the tokens). */
export type NodeState = "proven" | "merged" | "work" | "stalled" | "ready";
export function nodeState(t: MissionTask, step: MissionStep | undefined): NodeState {
  if (t.blocked) return "stalled";
  if (t.lane === null) return "ready";
  if (t.step === "merged") return step?.status === "proven" ? "proven" : "merged";
  return "work";
}
export function nodeLabel(t: MissionTask, step: MissionStep | undefined): string {
  if (t.blocked) return "Blocked";
  if (t.lane === null) return "Ready · unclaimed";
  if (t.step === "merged") return step?.status === "proven" ? "Proven" : "Merged · not proven";
  // #480: a task plans (and a design plan is critiqued) before it is written.
  return t.step === "plan" ? "Planning" : t.step === "critic" ? "Design in review"
    : t.step === "implement" ? "Writing" : t.step === "review" ? "In review" : "Merging";
}
export const prText = (t: MissionTask) => (t.pr ? `#${t.pr}` : "no PR");

// Geometry of the design's artboard: 150px columns 18px apart, 64px of column head, 104px a row.
export const COL_W = 196, PITCH = 216, HEAD = 64, NODE_H = 88, ROW = 104;

interface Seg { style: CSSProperties; dir: "h" | "v" | "right" | "down" | "up"; done: boolean }
function line(x1: number, y1: number, x2: number, y2: number, done: boolean): Seg {
  return y1 === y2
    ? { dir: "h", done, style: { left: Math.min(x1, x2), top: y1 - 1, width: Math.abs(x2 - x1), height: 2 } }
    : { dir: "v", done, style: { left: x1 - 1, top: Math.min(y1, y2), width: 2, height: Math.abs(y2 - y1) } };
}

/** Orthogonal route from a to b: a forward edge elbows through the gap between columns; within one
 * column it drops straight down; a backward edge loops under both nodes. Each route ends in an arrow. */
export function route(a: PlacedTask, b: PlacedTask, done: boolean): Seg[] {
  const ax = a.col * PITCH, ay = HEAD + a.row * ROW, bx = b.col * PITCH, by = HEAD + b.row * ROW;
  if (b.col > a.col) {
    const sx = ax + COL_W, sy = ay + NODE_H / 2, dx = bx - 6, dy = by + NODE_H / 2, ex = Math.round((sx + dx) / 2);
    const segs = [line(sx, sy, ex, sy, done)];
    if (sy !== dy) segs.push(line(ex, sy, ex, dy, done));
    segs.push(line(ex, dy, dx, dy, done), { dir: "right", done, style: { left: dx, top: dy - 5 } });
    return segs;
  }
  const acx = ax + COL_W / 2, bcx = bx + COL_W / 2;
  if (b.col === a.col) return [line(acx, ay + NODE_H, acx, by - 6, done), { dir: "down", done, style: { left: acx - 5, top: by - 6 } }];
  const low = Math.max(ay, by) + NODE_H + 10;
  return [line(acx, ay + NODE_H, acx, low, done), line(bcx, low, acx, low, done), line(bcx, by + NODE_H + 6, bcx, low, done),
    { dir: "up", done, style: { left: bcx - 5, top: by + NODE_H } }];
}

interface Props {
  mission: Mission;
  selectedStepId: string | null;
  selectedTaskKey: string | null;
  onSelectStep(stepId: string): void;
  onSelectTask(key: string): void;
  onOpenTest(stepId: string): void;
}

export function MissionGraph({ mission, selectedStepId, selectedTaskKey, onSelectStep, onSelectTask, onOpenTest }: Props) {
  const { summary, steps } = mission;
  const layout = layoutMission(mission);
  const placed = layout.placed.find((p) => p.task.key === selectedTaskKey) ?? null;
  const at = new Map(layout.placed.map((p) => [p.task.key, p]));
  const selCol = steps.findIndex((s) => s.stepId === selectedStepId);
  const width = Math.max(1, steps.length) * PITCH - 18;
  const height = HEAD + layout.rows * ROW + 16;
  const step = steps.find((s) => s.stepId === selectedStepId) ?? null;
  return (
    <div className="mg">
      <p className="mg-summary">
        <span className="mg-sum" data-tone="proven"><b>{`${summary.proven}/${summary.total}`}</b> proven</span>
        <span className="mg-sep"> · </span>
        <span className="mg-sum" data-tone="work"><b>{summary.inFlight}</b> in flight</span>
        <span className="mg-sep"> · </span>
        <span className="mg-sum" data-tone="ready"><b>{summary.readyUnclaimed}</b> ready, unclaimed</span>
        <span className="mg-sep"> · </span>
        <span className="mg-sum" data-tone="stalled"><b>{summary.needYou}</b> need you</span>
      </p>
      <div className="mg-body">
        <section className="mg-graph" aria-label="Work graph">
          <div className="mg-scroll">
            <div className="mg-canvas" style={{ width, height }}>
              {selCol >= 0 && <div className="mg-colhi" style={{ left: selCol * PITCH - 8, height: height + 12 }} />}
              <div className="mg-cols" style={{ gridTemplateColumns: `repeat(${steps.length}, ${COL_W}px)` }}>
                {steps.map((s) => (
                  <button key={s.stepId} type="button" className="mg-col" data-status={s.status} data-selected={s.stepId === selectedStepId}
                    aria-label={`Column ${s.index + 1}: ${s.title}`} onClick={() => onSelectStep(s.stepId)}>
                    <span className="mg-col-head">{`STEP ${s.index + 1} · ${STATUS_LABEL[s.status]}`}</span>
                    <span className="mg-col-title">{s.title}</span>
                  </button>
                ))}
              </div>
              {layout.edges.flatMap((e) => {
                const a = at.get(e.from), b = at.get(e.to);
                return a && b ? route(a, b, e.done).map((sg, i) => (
                  <div key={`${e.from}-${e.to}-${i}`} className="mg-seg" data-dir={sg.dir} data-done={sg.done} style={sg.style} aria-hidden="true" />
                )) : [];
              })}
              {layout.placed.map(({ task: t, col, row }) => {
                const st = nodeState(t, steps[col]);
                return (
                  <button key={t.key} type="button" className="mg-node" data-state={st} data-step={t.step} data-blocked={t.blocked}
                    aria-pressed={t.key === selectedTaskKey} style={{ left: col * PITCH, top: HEAD + row * ROW }} onClick={() => onSelectTask(t.key)}>
                    <span className="mg-node-head">
                      <span className="mg-dot" data-state={st} />
                      <span className="mg-node-label">{nodeLabel(t, steps[col])}</span>
                      <span className="mg-node-pr">{prText(t)}</span>
                    </span>
                    <span className="mg-node-title">{t.title}</span>
                    <span className="mg-node-who">{t.lane ? [t.lane, t.reviewers.join(", ")].filter(Boolean).join(" → ") : "nobody yet"}</span>
                  </button>
                );
              })}
            </div>
          </div>
          {mission.tasks.length === 0 && <p className="mg-empty">No work linked to this journey yet</p>}
          <div className="mg-legend">
            <span className="mg-key" data-state="ready">Ready</span>
            <span className="mg-key" data-state="work">In work</span>
            <span className="mg-key" data-state="stalled">Stalled</span>
            <span className="mg-key" data-state="merged">Merged, not proven</span>
            <span className="mg-key" data-state="proven">Proven by the journey</span>
            <span className="mg-legend-note">Merged is not done. Done = the replay proves the step.</span>
          </div>
        </section>
        {placed ? <TaskInspector task={placed.task} step={mission.steps[placed.col]} onOpenTest={onOpenTest} />
          : step ? (
            <aside className="mg-inspector" aria-label="Selected step">
              <span className="mg-kick"><span className="mg-dot" data-status={step.status} />{`STEP ${step.index + 1} · ${STATUS_LABEL[step.status]}`}</span>
              <h3>{step.title}</h3>
              {step.promise && <p className="mg-muted">{step.promise}</p>}
              {step.reason && <div className="mg-note">{step.reason}</div>}
              <div className="mg-actions"><button type="button" className="mg-primary" onClick={() => onOpenTest(step.stepId)}>Open test</button></div>
            </aside>
          ) : (
            <aside className="mg-inspector" aria-label="Selected work"><p className="mg-muted">Pick a step or a task to see who touched it.</p></aside>
          )}
      </div>
    </div>
  );
}

/** The selected work: who touched it and the evidence on its head. `step` is the journey step the
 * task serves, when it serves one; without it the inspector offers no test. */
export function TaskInspector({ task: t, step, onOpenTest, health = null, pace = null, paceLabel = "", actions = null }: {
  task: MissionTask; step: MissionStep | undefined; onOpenTest(stepId: string): void; health?: StageHealth | null; pace?: Pace | null; paceLabel?: string;
  /** #591: the owner's Nudge / Reassign controls, under "Who touched it". */
  actions?: ReactNode;
}) {
  const st = nodeState(t, step);
  const note = t.blockedBy ? `BLOCK by ${t.blockedBy.reviewer || "a reviewer"} at ${sha8(t.blockedBy.headSha) ?? "an unrecorded head"}`
    : t.step === "merged" && step?.status !== "proven" ? "Merged, not proven yet" : null;
  const open = () => { if (step) onOpenTest(step.stepId); };
  return (
    <aside className="mg-inspector" aria-label="Selected work">
      <div className="mg-ins-head">
        <span className="mg-kick"><span className="mg-dot" data-state={st} />{`${nodeLabel(t, step)} · ${prText(t)}`}</span>
        <h3>{t.title}</h3>
        {health && <HealthFlag health={health} />}
        {pace && <PaceBlock pace={pace} label={paceLabel} stuck={health?.flag === "stalled"} size="large" />}
        {step && <span className="mg-muted">{`Proves step ${step.index + 1} · `}<button type="button" className="mg-link" onClick={open}>open its test</button></span>}
      </div>
      <div className="mg-section">
        <span className="mg-cap">How far it got</span>
        <ul className="mg-ladder" aria-label="How far it got" data-state={st}>
          {TRUST_LABELS.map((label, i) => (
            <li key={label} data-lit={i < t.trust}><span className="mg-rung" />{label}</li>
          ))}
        </ul>
      </div>
      {note && <div className="mg-note">{note}</div>}
      <div className="mg-section">
        <span className="mg-cap">Who touched it</span>
        <ul className="mg-custody" aria-label="Who touched it">
          {custodyRows(t).map((r, i) => (
            <li key={i} className="mg-custody-row"><span className="mg-stage">{r.stage}</span><span className="mg-who">{r.who}</span><span className="mg-verdict" data-tone={r.tone}>{r.verdict}</span></li>
          ))}
        </ul>
        {t.reviewerHistory && t.reviewerHistory.length > 0 && <p className="mg-muted">{`Earlier reviewers: ${t.reviewerHistory.join(", ")}`}</p>}
      </div>
      {actions}
      <div className="mg-section">
        <span className="mg-cap">Evidence on this head</span>
        {step && step.status !== "not_run" ? (
          <div className="mg-evidence">
            <span className="mg-ev-cmd">{`Journey replay · step ${step.index + 1}`}</span>
            <span className="mg-ev-result" data-status={step.status}>{`${STATUS_LABEL[step.status]}${step.reason ? ` · ${step.reason}` : ""}`}</span>
          </div>
        ) : <p className="mg-muted">No evidence recorded on this head</p>}
      </div>
      <div className="mg-actions">
        {t.repoUrl && t.pr ? <a className="mg-primary" href={`${t.repoUrl}/pull/${t.pr}`} target="_blank" rel="noreferrer">Open PR</a> : null}
        {step && <button type="button" className="mg-secondary" onClick={open}>Open its test</button>}
      </div>
    </aside>
  );
}

/** #583: the colour family and the label of a PR node on an issue's graph, read off its stage. */
export function stageState(stage: WorkStage, t: MissionTask): NodeState {
  if (stage === "proven") return "proven";
  if (stage === "merged") return "merged";
  if (t.blocked) return "stalled";
  if (t.lane === null) return "ready";
  return "work";
}
const STAGE_LABEL: Record<WorkStage, string> = {
  plan: "Planning", implement: "Writing", review: "In review", fix: "Fixing", merge: "Merging", merged: "Merged · not proven", proven: "Proven",
};
export const stageLabel = (stage: WorkStage, t: MissionTask) => (t.step === "critic" && stage === "plan" ? "Design in review" : STAGE_LABEL[stage]);

/** #706 geometry: five stage columns sized from the graph box (CSS grid `repeat(5, minmax(140px, 1fr))`
 * with GAP between them). Arrows are placed with `calc()` against the same track width, so they follow
 * the columns at any box width without measuring. Inside a row: the current card's top at CARD_TOP, the
 * done cells' at CELL_TOP (their middle on the card's middle, MID); the loop rides at LOOP_Y above both. */
export const GRAPH_COLS = GRAPH_STAGES.length, GAP = 12, CARD_TOP = 12, CELL_TOP = 36, MID = CARD_TOP + NODE_H / 2, LOOP_Y = 4;
/** x of `units` column widths plus `px`, in the row's own width. */
export const at = (units: number, px: number) => `calc((100% - ${(GRAPH_COLS - 1) * GAP}px) / ${GRAPH_COLS} * ${units} + ${px}px)`;
const colLeft = (col: number, px = 0) => at(col, col * GAP + px);
const colMid = (col: number, px = 0) => at(col + 0.5, col * GAP + px);

/** The arrows inside one row, positioned in the row's cell strip: forward ones run straight along the
 * row's middle; the loop from Fix back to the re-review rides above the row's cells, so no arrow leaves its row. */
export function rowSegs(row: Pick<PrRow, "cells" | "edges">): Seg[] {
  const cellTop = (col: number) => (row.cells.find((c) => c.col === col)?.state === "current" ? CARD_TOP : CELL_TOP);
  return row.edges.flatMap((e): Seg[] => {
    const done = e.kind === "done";
    if (e.to > e.from) {
      const n = e.to - e.from;
      return [{ dir: "h", done, style: { left: colLeft(e.from + 1, -GAP), width: at(n - 1, n * GAP - 6), top: MID - 1, height: 2 } },
        { dir: "right", done, style: { left: colLeft(e.to, -6), top: MID - 5 } }];
    }
    const n = e.from - e.to, a = cellTop(e.from), b = cellTop(e.to);
    return [
      { dir: "v", done, style: { left: colMid(e.from, -1), top: LOOP_Y, width: 2, height: a - LOOP_Y } },
      { dir: "h", done, style: { left: colMid(e.to), top: LOOP_Y, width: at(n, n * GAP), height: 2 } },
      { dir: "v", done, style: { left: colMid(e.to, -1), top: LOOP_Y, width: 2, height: b - 6 - LOOP_Y } },
      { dir: "down", done, style: { left: colMid(e.to, -5), top: b - 6 } },
    ];
  });
}

interface IssueProps {
  group: WorkGroup;
  /** The journey step a task serves, when one of the group's journeys places it. */
  stepFor(taskKey: string): MissionStep | undefined;
  selectedTaskKey: string | null;
  selectedCol: number | null;
  onSelectTask(key: string): void;
  onSelectCol(col: number): void;
  onOpenTest(stepId: string): void;
  /** #591: time in stage and the health flag of each open PR, by task key. */
  health?: Record<string, StageHealth | null>;
  /** #591: live progress against the usual time and the last activity, by task key. */
  pace?: Record<string, Pace>;
  /** #591: the owner's controls for the selected PR's current lane, when the Studio can send. */
  inspectorActions?: (taskKey: string) => ReactNode;
}
export interface Pace { progress: StageProgress | null; activity: Activity }

/** #591: the live timer, the progress bar against the usual stage time, and the last-activity line. */
export function PaceBlock({ pace, label, stuck, size = "card" }: { pace: Pace; label: string; stuck: boolean; size?: "card" | "large" }) {
  const p = pace.progress, a = pace.activity;
  const tone = p ? (stuck ? "red" : p.tone) : null;
  const act = a.sinceMs === null ? "no activity recorded" : a.tone === "red" ? `no activity ${stageDuration(a.sinceMs)}`
    : `${a.role ? `${a.role} active` : "last activity"} ${stageDuration(a.sinceMs)} ago`;
  const x = p ? p.pace.toFixed(1) : "";
  // #591: the time and the activity note each get their own line; nothing overlaps or cuts them.
  return (
    <span className="mg-pace" data-size={size}>
      {p && (
        <span className="mg-pace-time">
          <span className="mg-timer">{liveDuration(p.elapsedMs)}</span>
          {tone && <>{" · "}<span className="mg-pace-x">{`${x}× typical`}</span></>}
        </span>
      )}
      <span className="mg-pace-line">
        <span className="mg-act-dot" data-tone={a.tone} aria-hidden="true" />
        <span className="mg-act" data-tone={a.tone}>{act}</span>
      </span>
      {p && tone && (
        <span className="mg-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(p.ratio * 100)}
          aria-label={`${label} for ${stageDuration(p.elapsedMs)}, ${x} times the usual time`} data-tone={tone}>
          <span className="mg-bar-fill" style={{ width: `${Math.round(p.ratio * 100)}%` }} />
        </span>
      )}
    </span>
  );
}

/** #591: the plain health flag plus the time in stage, on the card and in the inspector header. */
export function HealthFlag({ health, time = true }: { health: StageHealth; time?: boolean }) {
  return (
    <span className="mg-health" data-flag={health.flag} data-tone={health.tone}>
      <span className="mg-health-text" title={health.text}>{health.text}</span>
      {time && health.elapsed && <span className="mg-health-time">{health.elapsed}</span>}
    </span>
  );
}

/** #591: the current card's "author → reviewer" line. */
function cardWho(c: PrRow["cells"][number], stage: WorkStage, t: MissionTask): string {
  if (stage === "fix" && t.blocked) return t.lane ?? "nobody yet";
  if (stage === "review" && Boolean(c.title?.startsWith("Re-review")) && c.who) return c.who;
  return t.lane ? [t.lane, t.reviewers.join(", ")].filter(Boolean).join(" → ") : "nobody yet";
}

/** #706: "author → reviewer" with each lane kept whole: a name never breaks inside itself. */
function WhoLine({ who }: { who: string }) {
  const parts = who.split(/( → |, )/);
  return <span className="mg-node-who">{parts.map((p, i) => (p === " → " || p === ", " ? <span key={i}>{p}</span> : <span key={i} className="mg-lane">{p}</span>))}</span>;
}

/** #591: the current card. A BLOCK opens a Fixing card for the author (amber, red only when the
 * owner lane is silent past LIVENESS_MS: Stalled); the health flag gets its own line and the time in stage sits right on the who line. */
export function CurrentCard({ cell: c, stage, task: t, health, pace, selected, left, top, onSelect }: {
  cell: PrRow["cells"][number]; stage: WorkStage; task: MissionTask; health: StageHealth | null; pace: Pace | null; selected: boolean;
  /** Absolute position on the graph canvas; omitted (#668) when the card sits in a kanban column. */
  left?: number; top?: number; onSelect(): void;
}) {
  const stuck = health?.flag === "stalled";
  const fixing = stage === "fix" && t.blocked;
  const st: NodeState = stuck ? "stalled" : fixing ? "work" : stageState(stage, t);
  const base = stuck ? "Stalled" : c.title ?? stageLabel(stage, t);
  const who = cardWho(c, stage, t);
  // The Fixing card's sub-line already names the BLOCK; its "Blocked by" flag would say it twice.
  const flag = health && !(fixing && health.flag === "blocked") ? health : null;
  return (
    <button type="button" className="mg-node" data-state={st} data-stage={stage} data-blocked={t.blocked} data-current="true" data-dense={Boolean(c.sub || flag)}
      data-pace={Boolean(pace)} aria-pressed={selected} style={left === undefined ? undefined : { left, top }} onClick={onSelect}>
      <span className="mg-node-head">
        <span className="mg-dot" data-state={st} />
        <span className="mg-node-label">{c.count > 1 ? `${base} ×${c.count}` : base}</span>
        <span className="mg-node-pr">{prText(t)}</span>
      </span>
      <span className="mg-node-title">{t.title}</span>
      {c.sub && <span className="mg-node-sub" title={c.sub}>{c.sub}</span>}
      {flag && <HealthFlag health={flag} time={false} />}
      <span className="mg-node-foot">
        <WhoLine who={who} />
        {!pace && health?.elapsed && <span className="mg-node-time">{health.elapsed}</span>}
      </span>
      {pace && <PaceBlock pace={pace} label={stageLabel(stage, t)} stuck={stuck} />}
    </button>
  );
}

/** #591: an issue's graph, one row per PR along the stage columns: the stages it passed as small
 * cells, the stage it is in as the full card, arrows only inside its own row. Merged rows fold below.
 * #706: five columns sized from the box; each row's full-width title line sits above its cells. */
export function IssueGraph({ group, stepFor, selectedTaskKey, selectedCol, onSelectTask, onSelectCol, onOpenTest, health = {}, pace = {}, inspectorActions }: IssueProps) {
  const [showMerged, setShowMerged] = useState(false);
  const open = group.rows.filter((r) => r.open), merged = group.rows.filter((r) => !r.open);
  const selected = group.rows.find((r) => r.key === selectedTaskKey) ?? null;
  const col = selected ? graphCol(group.stages[selected.key]!) : selectedCol;
  const drawRow = (r: PrRow) => {
    const t = r.task, stage = group.stages[t.key]!, g = graphRow(r);
    const pr = t.pr ? `PR #${t.pr}` : "No PR";
    return (
      <div key={r.key} className="mg-row" data-row={r.key} data-open={r.open}>
        <button type="button" className="mg-rowtitle" aria-pressed={t.key === selectedTaskKey} aria-label={`Row ${pr}: ${t.title}`} title={t.title}
          onClick={() => onSelectTask(t.key)}>
          <span className="mg-rowtitle-pr">{pr}</span>{" · "}<span className="mg-rowtitle-text">{t.title}</span>
          {g.planTag && <span className="mg-tag" data-tone={g.planTag === "planning" ? "work" : "ok"}>{g.planTag}</span>}
        </button>
        <div className="mg-cells mg-grid">
          {rowSegs(g).map((sg, i) => (
            <div key={i} className="mg-seg" data-dir={sg.dir} data-done={sg.done} style={sg.style} aria-hidden="true" />
          ))}
          {g.cells.map((c) => c.state === "current" ? (
            <div key={c.stage} className="mg-slot" style={{ gridColumn: c.col + 1 }}>
              <CurrentCard cell={c} stage={stage} task={t} health={health[t.key] ?? null} pace={pace[t.key] ?? null} selected={t.key === selectedTaskKey}
                onSelect={() => onSelectTask(t.key)} />
            </div>
          ) : (
            <button key={c.stage} type="button" className="mg-cell" data-cell={c.state} data-stage={c.stage} tabIndex={c.state === "ahead" ? -1 : 0}
              aria-label={`${pr} ${[c.label, c.who, c.mark, c.badge, c.time].filter(Boolean).join(" · ")}`} style={{ gridColumn: c.col + 1 }}
              onClick={() => onSelectTask(t.key)}>
              {c.state !== "ahead" && <>
                <span className="mg-cell-line">
                  <span className="mg-cell-label">{c.label}</span>
                  {c.mark && <span className="mg-cell-mark" data-tone={c.mark === "BLOCK" ? "block" : c.mark === "✓" || c.mark.startsWith("fix pushed") ? "ok" : "skip"}>{c.mark}</span>}
                </span>
                <span className="mg-cell-line mg-cell-sub">
                  <span className="mg-cell-who">{[c.who, c.time].filter(Boolean).join(" · ")}</span>
                  {c.badge && <span className="mg-tag" data-tone="proven">{c.badge}</span>}
                </span>
              </>}
            </button>
          ))}
        </div>
      </div>
    );
  };
  return (
    <div className="mg">
      <div className="mg-title"><h1>{group.label}</h1>{group.summary && <span className="mg-desc">{group.summary}</span>}</div>
      <div className="mg-body">
        <section className="mg-graph" aria-label="Work graph">
          <div className="mg-scroll">
            <div className="mg-issue">
              {col !== null && col >= 0 && <div className="mg-colhi" style={{ left: colLeft(col, -8), width: at(1, 16) }} aria-hidden="true" />}
              <div className="mg-cols mg-grid">
                {GRAPH_STAGES.map((s, i) => {
                  const n = group.tasks.filter((t) => graphCol(group.stages[t.key]!) === i).length;
                  return (
                    <button key={s.id} type="button" className="mg-col" data-stage={s.id} data-selected={i === col}
                      aria-label={`Column ${s.label}: ${n}`} onClick={() => onSelectCol(i)}>
                      <span className="mg-col-head">{`${s.label.toUpperCase()} · ${n}`}</span>
                    </button>
                  );
                })}
              </div>
              <div className="mg-rows">
                {open.map(drawRow)}
                {merged.length > 0 && (
                  <button type="button" className="mg-fold" aria-expanded={showMerged} onClick={() => setShowMerged((v) => !v)}>
                    {`Merged · ${merged.length}`}
                  </button>
                )}
                {showMerged && merged.map(drawRow)}
              </div>
            </div>
          </div>
          <div className="mg-legend">
            <span className="mg-key" data-state="ready">Ready</span>
            <span className="mg-key" data-state="work">In work</span>
            <span className="mg-key" data-state="stalled">Stalled</span>
            <span className="mg-key" data-state="merged">Merged, not proven</span>
            <span className="mg-key" data-state="proven">Proven by the journey</span>
            <span className="mg-legend-note">Merged is not done. Done = the replay proves the step.</span>
          </div>
        </section>
        {selected ? <TaskInspector task={selected.task} step={stepFor(selected.key)} onOpenTest={onOpenTest} health={health[selected.key] ?? null}
            pace={selected.open ? pace[selected.key] ?? null : null} paceLabel={stageLabel(group.stages[selected.key]!, selected.task)}
            actions={selected.open ? inspectorActions?.(selected.key) ?? null : null} />
          : <aside className="mg-inspector" aria-label="Selected work"><p className="mg-muted">Pick a PR to see who touched it.</p></aside>}
      </div>
    </div>
  );
}
