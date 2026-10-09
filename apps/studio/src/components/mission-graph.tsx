import "./mission-graph.css";
import type { CSSProperties } from "react";
import { custodyRows, sha8, TRUST_LABELS, type Mission, type MissionStep, type MissionTask, type StepStatus } from "../runtime/mission";
import { layoutMission, type PlacedTask } from "../runtime/mission-layout";

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
const prText = (t: MissionTask) => (t.pr ? `#${t.pr}` : "no PR");

// Geometry of the design's artboard: 150px columns 18px apart, 64px of column head, 104px a row.
const COL_W = 150, PITCH = 168, HEAD = 64, NODE_H = 88, ROW = 104;

interface Seg { style: CSSProperties; dir: "h" | "v" | "right" | "down" | "up"; done: boolean }
function line(x1: number, y1: number, x2: number, y2: number, done: boolean): Seg {
  return y1 === y2
    ? { dir: "h", done, style: { left: Math.min(x1, x2), top: y1 - 1, width: Math.abs(x2 - x1), height: 2 } }
    : { dir: "v", done, style: { left: x1 - 1, top: Math.min(y1, y2), width: 2, height: Math.abs(y2 - y1) } };
}

/** Orthogonal route from a to b: a forward edge elbows through the gap between columns; within one
 * column it drops straight down; a backward edge loops under both nodes. Each route ends in an arrow. */
function route(a: PlacedTask, b: PlacedTask, done: boolean): Seg[] {
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
        {placed ? <TaskInspector placed={placed} mission={mission} onOpenTest={onOpenTest} />
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

function TaskInspector({ placed, mission, onOpenTest }: { placed: PlacedTask; mission: Mission; onOpenTest(stepId: string): void }) {
  const t = placed.task;
  const step = mission.steps[placed.col];
  const st = nodeState(t, step);
  const note = t.blockedBy ? `BLOCK by ${t.blockedBy.reviewer || "a reviewer"} at ${sha8(t.blockedBy.headSha) ?? "an unrecorded head"}`
    : t.step === "merged" && step?.status !== "proven" ? "Merged, not proven yet" : null;
  const open = () => { if (step) onOpenTest(step.stepId); };
  return (
    <aside className="mg-inspector" aria-label="Selected work">
      <div className="mg-ins-head">
        <span className="mg-kick"><span className="mg-dot" data-state={st} />{`${nodeLabel(t, step)} · ${prText(t)}`}</span>
        <h3>{t.title}</h3>
        {step && <span className="mg-muted">{`Proves step ${placed.col + 1} · `}<button type="button" className="mg-link" onClick={open}>open its test</button></span>}
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
      </div>
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
