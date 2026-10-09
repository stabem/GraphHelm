/**
 * A journey as a flowchart (#519, the owner's sketch): one card per screen, joined by arrows, a
 * second way through the app as a side branch. A card shows the screen as the journey's run
 * rendered it, how that step fared, and what the owner Does and Sees there. Clicking a card
 * expands it over the Studio (Esc or a click outside closes it); nothing opens a window outside.
 *
 * The geometry is fixed (every card the same size on a grid), so the arrows are computed from the
 * layout alone and never measured from the page.
 */
import { useEffect, useRef, useState } from "react";

import type { JourneyFlowEdge, JourneyFlowScreen, JourneyFlowView, JourneyRunEdge, JourneyRunScreen, JourneyRunView } from "../runtime/types";

const CARD_W = 232;
const CARD_H = 252;
const FRAME_H = 145;
const GAP_X = 56;
const GAP_Y = 40;

const VERB: Record<string, string> = {
  activate: "Clicks",
  submit: "Submits with",
  navigate: "Follows",
  enter_text: "Fills in",
  wait_for: "Waits for",
  inspect: "Looks at",
};

const ROLE: Record<string, string> = { textbox: "field", heading: "heading", button: "button", link: "link" };

/** Why a step did not pass, in words; the Runtime's closed list of codes picks them (#519). */
const STEP_REASON: Record<string, string> = {
  expect_missing: "something this screen should show was not there",
  act_failed: "the control to use was not found",
  guard_refused: "the test stopped before an action that would delete or cancel something",
  not_reached: "an earlier step failed, so the test never got here",
  page_error: "the page did not load",
  timeout: "the page took too long",
};

export function actWords(act: { kind: string; name: string }): string {
  return `${VERB[act.kind] ?? act.kind.replace(/_/g, " ")} “${act.name}”`;
}

/** What the test checks on a screen, from its selectors: the detail behind the plain title. */
export function expectWords(screen: JourneyFlowScreen): string {
  return (screen.expect ?? []).map((item) => `the “${item.name}” ${ROLE[item.role] ?? item.role}`).join(", ");
}

/** The flow's paths, `main` first: Approve approves every one of them, so every one is drawn. */
export function pathsOf(flow: JourneyFlowView): string[] {
  const names = Object.keys(flow.paths);
  return [...names.filter((name) => name === "main"), ...names.filter((name) => name !== "main").sort()];
}

export interface ChartNode { screen: JourneyFlowScreen; number: number; col: number; row: number; arrivedBy: JourneyFlowEdge | null }
export interface ChartLayout { nodes: ChartNode[]; arrows: JourneyFlowEdge[]; cols: number; rows: number }

/** Where each screen sits: the main path along the top row, every other path branching into the
 * rows below from the screen it leaves. A screen is placed once, by the first path that reaches
 * it; a flow without paths lists its screens in a row. */
export function layoutFlow(flow: JourneyFlowView): ChartLayout {
  const screens = new Map(flow.screens.map((screen) => [screen.id, screen]));
  const edges = new Map(flow.edges.map((edge) => [edge.id, edge]));
  const at = new Map<string, ChartNode>();
  const taken = new Set<string>();
  const arrows = new Map<string, JourneyFlowEdge>();
  const place = (id: string, col: number, fromRow: number, arrivedBy: JourneyFlowEdge | null): void => {
    const screen = screens.get(id);
    if (screen === undefined || at.has(id)) return;
    let row = fromRow;
    while (taken.has(`${col}:${row}`)) row += 1;
    taken.add(`${col}:${row}`);
    at.set(id, { screen, number: at.size + 1, col, row, arrivedBy });
  };
  for (const path of pathsOf(flow)) {
    const steps = (flow.paths[path] ?? []).map((id) => edges.get(id)).filter((edge): edge is JourneyFlowEdge => edge !== undefined);
    if (steps.length === 0) continue;
    const side = path === "main" ? 0 : 1;
    place(steps[0]!.from, 0, side, null);
    for (const edge of steps) {
      const from = at.get(edge.from);
      if (from === undefined) continue;
      place(edge.to, from.col + 1, Math.max(side, from.row), edge);
      if (at.has(edge.to)) arrows.set(edge.id, edge);
    }
  }
  let nextCol = Math.max(-1, ...[...at.values()].filter((node) => node.row === 0).map((node) => node.col)) + 1;
  for (const screen of flow.screens) {
    if (!at.has(screen.id)) place(screen.id, nextCol++, 0, null);
  }
  const nodes = [...at.values()];
  return { nodes, arrows: [...arrows.values()], cols: Math.max(1, ...nodes.map((node) => node.col + 1)), rows: Math.max(1, ...nodes.map((node) => node.row + 1)) };
}

const left = (col: number): number => col * (CARD_W + GAP_X);
const top = (row: number): number => row * (CARD_H + GAP_Y);

/** An arrow between two cards, at the height of the picture: straight along a row, an elbow to a
 * branch, and a loop under the cards when it goes back. A branch that REJOINS a row above it (the
 * owner's sketch: the lower card feeds back into the main row) leaves its card from the top,
 * crosses in the empty band between the two rows, and enters the card it rejoins from below: it
 * never runs behind a card or merges into the arrow already entering that card from the left. */
function arrowPath(from: ChartNode, to: ChartNode): string {
  if (to.row < from.row) {
    const band = top(to.row) + CARD_H + GAP_Y / 2;
    return `M ${left(from.col) + CARD_W / 2} ${top(from.row)} V ${band} H ${left(to.col) + CARD_W / 2} V ${top(to.row) + CARD_H}`;
  }
  const x1 = left(from.col) + CARD_W;
  const y1 = top(from.row) + FRAME_H / 2;
  const x2 = left(to.col);
  const y2 = top(to.row) + FRAME_H / 2;
  if (to.col > from.col) {
    if (to.row === from.row) return `M ${x1} ${y1} H ${x2}`;
    const mid = x2 - GAP_X / 2;
    return `M ${x1} ${y1} H ${mid} V ${y2} H ${x2}`;
  }
  const under = top(Math.max(from.row, to.row)) + CARD_H + GAP_Y / 2;
  return `M ${x1} ${y1} h ${GAP_X / 4} V ${under} H ${x2 - GAP_X / 4} V ${y2} H ${x2}`;
}

type StepState = "pass" | "fail" | "drift" | "skipped" | "running" | "not_reached" | null;

/** How a screen's step fared: its own result, or the arriving edge's when the act itself failed. */
function stepState(node: ChartNode, run: JourneyRunView | null): { state: StepState; from: JourneyRunScreen | JourneyRunEdge | null } {
  if (run === null || run.state === "none") return { state: null, from: null };
  const screen = run.screens?.[node.screen.id];
  const edge = node.arrivedBy ? run.edges?.[node.arrivedBy.id] : undefined;
  if (edge?.result === "fail" || edge?.result === "drift") return { state: edge.result, from: edge.reason !== undefined || screen === undefined ? edge : screen };
  if (screen?.result) return { state: screen.result, from: screen };
  // The guard stopped the act that leads here: the step was skipped on purpose, not lost.
  if (edge?.result === "skipped") return { state: "skipped", from: edge };
  if (run.state === "running") return { state: run.current === node.screen.id ? "running" : null, from: null };
  if (screen !== undefined && !screen.frame) return { state: screen.reason === "not_reached" || screen.reason === undefined ? "not_reached" : "fail", from: screen };
  return { state: null, from: null };
}

const STATE_WORDS: Record<Exclude<StepState, null>, string> = {
  pass: "Passed",
  fail: "Failed",
  drift: "Changed",
  skipped: "Skipped",
  running: "Running…",
  not_reached: "Not reached",
};

/** The reason a step did not pass, as a sentence: the code's words, then what the page showed. */
function whyWords(from: JourneyRunScreen | JourneyRunEdge | null): string | null {
  if (from === null || from.reason === undefined) return null;
  const words = STEP_REASON[from.reason] ?? "the step did not go as recorded";
  return from.seen ? `${words} (the page showed: ${from.seen})` : words;
}

export interface JourneyFlowchartProps {
  flow: JourneyFlowView;
  /** The journey's run; null before the Runtime has answered, or when no run is offered. */
  run?: JourneyRunView | null;
  /** Object URLs of the frames the run kept, by screen id. */
  frames?: Record<string, string>;
  /** The screen a Watch is on, and the page it is playing right now. */
  current?: string | null;
  liveFrame?: string | null;
}

export function JourneyFlowchart({ flow, run = null, frames = {}, current = null, liveFrame = null }: JourneyFlowchartProps) {
  const layout = layoutFlow(flow);
  const byId = new Map(layout.nodes.map((node) => [node.screen.id, node]));
  const [open, setOpen] = useState<string | null>(null);
  const close = useRef<HTMLButtonElement>(null);
  const opened = open === null ? undefined : byId.get(open);
  useEffect(() => {
    if (opened === undefined) return undefined;
    close.current?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") setOpen(null); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [opened === undefined]);
  const frameOf = (id: string): string | null => (id === current && liveFrame !== null ? liveFrame : frames[id] ?? null);
  const width = layout.cols * CARD_W + (layout.cols - 1) * GAP_X;
  const height = layout.rows * (CARD_H + GAP_Y);
  return (
    <div className="journey-chart-scroll">
      <div className="journey-chart" style={{ width, height }}>
        <svg className="journey-chart-arrows" style={{ width, height }} aria-hidden="true">
          <defs>
            <marker id="journey-chart-head" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M 0 0 L 8 4 L 0 8 z" />
            </marker>
          </defs>
          {layout.arrows.map((edge) => (
            <path key={edge.id} d={arrowPath(byId.get(edge.from)!, byId.get(edge.to)!)} data-edge={edge.id} data-result={run?.edges?.[edge.id]?.result} markerEnd="url(#journey-chart-head)" />
          ))}
        </svg>
        <ol className="journey-flow-steps" aria-label="Steps">
          {layout.nodes.map((node) => {
            const { state } = stepState(node, run);
            const frame = frameOf(node.screen.id);
            const acts = node.arrivedBy?.acts ?? [];
            return (
              <li key={node.screen.id} className="journey-flow-step" aria-current={node.screen.id === current ? "step" : undefined}
                style={{ left: left(node.col), top: top(node.row), width: CARD_W, height: CARD_H }}>
                <button type="button" className="journey-card" data-state={state ?? undefined} aria-haspopup="dialog" onClick={() => setOpen(node.screen.id)}>
                  <span className="journey-card-frame" style={{ height: FRAME_H }}>
                    {frame !== null && <img src={frame} alt="" />}
                  </span>
                  <span className="journey-card-head">
                    <span className="journey-flow-step-number">{node.number}</span>
                    {state !== null && <span className="journey-card-result" data-state={state}>{STATE_WORDS[state]}</span>}
                  </span>
                  <span className="journey-flow-step-text">
                    {acts.length > 0 && <span className="journey-does"><span className="journey-label">Does</span> {acts.map(actWords).join(", then ")}</span>}
                    <span className="journey-sees"><span className="journey-label">Sees</span> {node.screen.title ?? (expectWords(node.screen) || node.screen.id)}</span>
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
      </div>
      {opened !== undefined && (() => {
        const { state, from } = stepState(opened, run);
        const frame = frameOf(opened.screen.id);
        const acts = opened.arrivedBy?.acts ?? [];
        const why = whyWords(from);
        const name = opened.screen.title ?? opened.screen.id;
        return (
          <div className="journey-zoom-backdrop" onClick={(event) => { if (event.target === event.currentTarget) setOpen(null); }}>
            <div className="journey-zoom" role="dialog" aria-modal="true" aria-label={`Step ${opened.number}: ${name}`}>
              <div className="journey-zoom-head">
                <h4>Step {opened.number}: {name}</h4>
                {state !== null && <span className="journey-card-result" data-state={state}>{STATE_WORDS[state]}</span>}
                <button ref={close} type="button" onClick={() => setOpen(null)}>Close</button>
              </div>
              {why !== null && <p className="journey-zoom-why">Why: {why}.</p>}
              <div className="journey-zoom-frame">
                {frame !== null ? <img src={frame} alt={`The screen at step ${opened.number}`} /> : <p>No picture of this screen yet.</p>}
              </div>
              {acts.length > 0 && <p className="journey-does"><span className="journey-label">Does</span> {acts.map(actWords).join(", then ")}</p>}
              <p className="journey-sees"><span className="journey-label">Sees</span> {opened.screen.title ?? (expectWords(opened.screen) || opened.screen.id)}</p>
              {opened.screen.title !== undefined && expectWords(opened.screen) !== "" && (
                <details className="journey-sees-details"><summary>details</summary>{expectWords(opened.screen)}</details>
              )}
            </div>
          </div>
        );
      })()}
    </div>
  );
}
