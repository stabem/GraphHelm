import "./mission-graph.css";
import { useState, type CSSProperties } from "react";
import { custodyRows, sha8, TRUST_LABELS, type Mission, type MissionStep, type MissionTask, type StepStatus } from "../runtime/mission";
import { layoutMission, type PlacedTask } from "../runtime/mission-layout";
import { WORK_STAGES, type PrRow, type WorkGroup, type WorkStage } from "../runtime/work-groups";

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
export const COL_W = 150, PITCH = 168, HEAD = 64, NODE_H = 88, ROW = 104;

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
export function TaskInspector({ task: t, step, onOpenTest }: { task: MissionTask; step: MissionStep | undefined; onOpenTest(stepId: string): void }) {
  const st = nodeState(t, step);
  const note = t.blockedBy ? `BLOCK by ${t.blockedBy.reviewer || "a reviewer"} at ${sha8(t.blockedBy.headSha) ?? "an unrecorded head"}`
    : t.step === "merged" && step?.status !== "proven" ? "Merged, not proven yet" : null;
  const open = () => { if (step) onOpenTest(step.stepId); };
  return (
    <aside className="mg-inspector" aria-label="Selected work">
      <div className="mg-ins-head">
        <span className="mg-kick"><span className="mg-dot" data-state={st} />{`${nodeLabel(t, step)} · ${prText(t)}`}</span>
        <h3>{t.title}</h3>
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

/** #583: the colour family and the label of a PR node on an issue's graph, read off its stage. */
export function stageState(stage: WorkStage, t: MissionTask): NodeState {
  if (stage === "proven") return "proven";
  if (stage === "merged") return "merged";
  if (t.blocked) return "stalled";
  if (t.lane === null) return "ready";
  return "work";
}
const STAGE_LABEL: Record<WorkStage, string> = {
  plan: "Planning", implement: "Writing", review: "In review", fix: "Fix pushed · re-review", merge: "Merging", merged: "Merged · not proven", proven: "Proven",
};
export const stageLabel = (stage: WorkStage, t: MissionTask) => (stage === "fix" && t.blocked ? "Blocked" : t.step === "critic" ? "Design in review" : STAGE_LABEL[stage]);

/** #591 geometry: a 120px row header, then the seven stage columns; each PR owns one 112px band. */
export const ROW_HEAD = 120, X0 = ROW_HEAD + 12, BAND = 112, CARD_TOP = 12, CELL_TOP = 36, MID = CARD_TOP + NODE_H / 2, FOLD_H = 44;
const colX = (col: number) => X0 + col * PITCH;

/** The arrows inside one row: forward ones run straight along the row's middle; the loop from Fix
 * back to the re-review rides above the row's cells, inside its band, so no arrow leaves its row. */
export function rowSegs(row: PrRow, top: number): Seg[] {
  const cellTop = (col: number) => top + (row.cells.find((c) => c.col === col)?.state === "current" ? CARD_TOP : CELL_TOP);
  return row.edges.flatMap((e): Seg[] => {
    const done = e.kind === "done";
    if (e.to > e.from) {
      const sx = colX(e.from) + COL_W, dx = colX(e.to) - 6, y = top + MID;
      return [line(sx, y, dx, y, done), { dir: "right", done, style: { left: dx, top: y - 5 } }];
    }
    const ax = colX(e.from) + COL_W / 2, bx = colX(e.to) + COL_W / 2, high = top + 4;
    return [line(ax, cellTop(e.from), ax, high, done), line(bx, high, ax, high, done), line(bx, high, bx, cellTop(e.to) - 6, done),
      { dir: "down", done, style: { left: bx - 5, top: cellTop(e.to) - 6 } }];
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
}

/** #591: an issue's graph, one row per PR along the stage columns: the stages it passed as small
 * cells, the stage it is in as the full card, arrows only inside its own row. Merged rows fold below. */
export function IssueGraph({ group, stepFor, selectedTaskKey, selectedCol, onSelectTask, onSelectCol, onOpenTest }: IssueProps) {
  const [showMerged, setShowMerged] = useState(false);
  const open = group.rows.filter((r) => r.open), merged = group.rows.filter((r) => !r.open);
  const shown = showMerged ? [...open, ...merged] : open;
  const tops = new Map<string, number>();
  let y = HEAD;
  for (const r of open) { tops.set(r.key, y); y += BAND; }
  const foldTop = y;
  if (merged.length) y += FOLD_H;
  if (showMerged) for (const r of merged) { tops.set(r.key, y); y += BAND; }
  const selected = group.rows.find((r) => r.key === selectedTaskKey) ?? null;
  const col = selected ? WORK_STAGES.findIndex((s) => s.id === group.stages[selected.key]) : selectedCol;
  const width = colX(WORK_STAGES.length) - 18;
  const height = y + 8;
  return (
    <div className="mg">
      <div className="mg-title"><h1>{group.label}</h1>{group.summary && <span className="mg-desc">{group.summary}</span>}</div>
      <div className="mg-body">
        <section className="mg-graph" aria-label="Work graph">
          <div className="mg-scroll">
            <div className="mg-canvas" style={{ width, height }}>
              {col !== null && col >= 0 && <div className="mg-colhi" style={{ left: colX(col) - 8, height: height + 12 }} />}
              <div className="mg-cols" style={{ left: X0, gridTemplateColumns: `repeat(${WORK_STAGES.length}, ${COL_W}px)` }}>
                {WORK_STAGES.map((s, i) => {
                  const n = group.tasks.filter((t) => group.stages[t.key] === s.id).length;
                  return (
                    <button key={s.id} type="button" className="mg-col" data-stage={s.id} data-selected={i === col}
                      aria-label={`Column ${s.label}: ${n}`} onClick={() => onSelectCol(i)}>
                      <span className="mg-col-head">{`${s.label.toUpperCase()} · ${n}`}</span>
                    </button>
                  );
                })}
              </div>
              {merged.length > 0 && (
                <button type="button" className="mg-fold" aria-expanded={showMerged} style={{ top: foldTop + 8 }} onClick={() => setShowMerged((v) => !v)}>
                  {`Merged · ${merged.length}`}
                </button>
              )}
              {shown.map((r) => {
                const top = tops.get(r.key)!;
                const t = r.task, stage = group.stages[t.key]!, st = stageState(stage, t);
                const pr = t.pr ? `PR #${t.pr}` : "No PR";
                return (
                  <div key={r.key} className="mg-row" data-row={r.key} data-open={r.open}>
                    <button type="button" className="mg-rowhead" aria-pressed={t.key === selectedTaskKey} style={{ top: top + CARD_TOP }}
                      aria-label={`Row ${pr}: ${t.title}`} title={t.title} onClick={() => onSelectTask(t.key)}>
                      <span className="mg-rowhead-pr">{pr}</span>
                      <span className="mg-rowhead-title">{t.title}</span>
                    </button>
                    {rowSegs(r, top).map((sg, i) => (
                      <div key={i} className="mg-seg" data-dir={sg.dir} data-done={sg.done} style={sg.style} aria-hidden="true" />
                    ))}
                    {r.cells.map((c) => c.state === "current" ? (
                      <button key={c.stage} type="button" className="mg-node" data-state={st} data-stage={stage} data-blocked={t.blocked} data-current="true"
                        aria-pressed={t.key === selectedTaskKey} style={{ left: colX(c.col), top: top + CARD_TOP }} onClick={() => onSelectTask(t.key)}>
                        <span className="mg-node-head">
                          <span className="mg-dot" data-state={st} />
                          <span className="mg-node-label">{c.count > 1 ? `${stageLabel(stage, t)} ×${c.count}` : stageLabel(stage, t)}</span>
                          <span className="mg-node-pr">{prText(t)}</span>
                        </span>
                        <span className="mg-node-title">{t.title}</span>
                        <span className="mg-node-who">{t.lane ? [t.lane, t.reviewers.join(", ")].filter(Boolean).join(" → ") : "nobody yet"}</span>
                      </button>
                    ) : (
                      <button key={c.stage} type="button" className="mg-cell" data-cell={c.state} data-stage={c.stage} tabIndex={c.state === "ahead" ? -1 : 0}
                        aria-label={`${pr} ${[c.label, c.who, c.mark, c.time].filter(Boolean).join(" · ")}`} style={{ left: colX(c.col), top: top + CELL_TOP }}
                        onClick={() => onSelectTask(t.key)}>
                        {c.state !== "ahead" && <>
                          <span className="mg-cell-line">
                            <span className="mg-cell-label">{c.label}</span>
                            {c.mark && <span className="mg-cell-mark" data-tone={c.mark === "BLOCK" ? "block" : c.mark === "✓" ? "ok" : "skip"}>{c.mark}</span>}
                          </span>
                          <span className="mg-cell-line mg-cell-sub">{[c.who, c.time].filter(Boolean).join(" · ")}</span>
                        </>}
                      </button>
                    ))}
                  </div>
                );
              })}
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
        {selected ? <TaskInspector task={selected.task} step={stepFor(selected.key)} onOpenTest={onOpenTest} />
          : <aside className="mg-inspector" aria-label="Selected work"><p className="mg-muted">Pick a PR to see who touched it.</p></aside>}
      </div>
    </div>
  );
}
