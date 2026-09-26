/**
 * The board: the run's nodes as blocks you can arrange, connect, draw on and open.
 *
 * A NODE IS AN AGENT, and clicking one is the whole navigation model — the panel beside it swaps
 * to that node's facts and its thread. Nothing else on this surface navigates.
 *
 * AN EDGE IS DRAWN ONLY WHEN THE MODEL SAYS IT IS PROVEN. `model.edgesKnown` is true only after a
 * graph file's semantic hash matched the hash the run itself recorded; anything short of that —
 * no file read, a mismatched hash, a run that has not reported its hash yet — reaches this
 * component with an empty `edges` list, and the note beneath says which of the three it was. This
 * component never decides that question and must never be given a way to: a drawn arrow reads as
 * evidence.
 *
 * NODES ARE BUTTONS, STROKES ARE SVG. A node carries text an operator reads and an action they
 * take, so it is real DOM with a real accessible name and real keyboard focus. Ink is geometry, so
 * it is a path. Mixing the two would cost either the accessibility or the drawing.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Activity, ArrowUpRight, GitBranch, MessageSquare, Users, FileCode2, Hand, Highlighter, Minus, MousePointer2, Plus, RotateCcw, StickyNote, Waypoints } from "lucide-react";

import type { GraphModel, GraphNode } from "../graph/model";
import { moodOf, nodeResult, splitLint } from "../graph/model";
import type { AgentPresence } from "../runtime/session";
import { isAlarming } from "./format";
import { CARD_HEIGHT, CARD_WIDTH, agentPositionOf, cardHeight, chromeInsets, fitCamera, frameCards, gridPosition, markId, tidyBoard, type BoardBounds, type BoardState, type Camera, type Point, type Stroke } from "../graph/board";

/** The pieces of page chrome that float over the canvas sheet (#1083 F8): the toolbar, the
 * header rows, the lint strip, the notes and connect row, the HUD capsule, and the scene's docks.
 * Measured at framing time, so whatever the container queries laid out is what is framed around. */
const CANVAS_CHROME = ".tools, .canvas-story, .canvas-arrange, .board-navigator, .canvas-sections, .canvas-lint, .edge-note, .graph-file, .connection-legend, .canvas-hints, .work-view-switch";
function chromeRectsAround(sheet: HTMLElement | null): DOMRect[] {
  if (sheet === null) return [];
  const pieces = new Set<Element>();
  sheet.parentElement?.querySelectorAll(CANVAS_CHROME).forEach((piece) => pieces.add(piece));
  sheet.querySelectorAll(".run-capsule").forEach((piece) => pieces.add(piece));
  sheet.closest(".scene")?.querySelectorAll(":scope > .dock, :scope > .work-view-switch").forEach((piece) => pieces.add(piece));
  pieces.delete(sheet);
  return [...pieces].map((piece) => piece.getBoundingClientRect());
}
import { ago, hueOf, initialOf, readable } from "./format";
import { WorkOverview, isEntryNode, isFirstEntryNode } from "./work-overview";
import type { WorkOverviewProps } from "./work-overview";


type Tool = "select" | "pen" | "note" | "hand";

/** Minutes since an instant, or null when there is none - presence maths, kept honest. */
function minutesSince(value?: string | null): number | null {
  if (!value) return null;
  const then = new Date(value).valueOf();
  if (Number.isNaN(then)) return null;
  return (Date.now() - then) / 60000;
}

/** How present a face looks: full within 5 minutes, dimmed to 30, faded past that. Driven only
 * by the log's own timestamps - never a pulse the data cannot back. */
function presenceOf(minutes: number | null): number {
  if (minutes === null) return 1;
  if (minutes < 5) return 1;
  if (minutes < 30) return 0.7;
  return 0.45;
}
type Tone = Stroke["tone"];

const TONES: Array<{ value: Tone; label: string }> = [
  { value: "ink", label: "Grey" },
  { value: "flag", label: "Amber" },
  { value: "calm", label: "Green" },
];

/** An SVG path through the stroke's points, smoothed just enough to look drawn rather than
 * plotted. A polyline reads as a chart; this reads as a pen. */
function pathOf(points: Point[]): string {
  if (points.length === 0) return "";
  if (points.length === 1) return `M ${points[0].x} ${points[0].y}`;
  let path = `M ${points[0].x} ${points[0].y}`;
  for (let index = 1; index < points.length; index += 1) {
    const previous = points[index - 1];
    const current = points[index];
    const midpoint = { x: (previous.x + current.x) / 2, y: (previous.y + current.y) / 2 };
    path += ` Q ${previous.x} ${previous.y} ${midpoint.x} ${midpoint.y}`;
  }
  const last = points[points.length - 1];
  return `${path} L ${last.x} ${last.y}`;
}

/**
 * One edge, as an orthogonal run between the two block sides that face each other.
 *
 * Right angles rather than curves: this surface reads as a machine's own diagram, and a
 * schematic's lines are square. The elbow sits halfway between the blocks so two edges leaving one
 * node stay distinguishable.
 *
 * BOTH DIRECTIONS ARE REAL. The default layout follows the graph, but the operator can drag any
 * block anywhere, so a target ends up left of its source all the time. Always exiting right and
 * entering left then swings the run far past both blocks — measured on the real board before this
 * was fixed. Which side each end uses is decided per edge, from where the blocks actually are.
 */
function edgePath(from: Point, to: Point, fromHeight = CARD_HEIGHT, toHeight = CARD_HEIGHT): string {
  if (Math.abs(from.x - to.x) < CARD_WIDTH && Math.abs(from.y - to.y) >= CARD_HEIGHT) {
    const down = to.y > from.y;
    const x1 = from.x + CARD_WIDTH / 2;
    const x2 = to.x + CARD_WIDTH / 2;
    const y1 = from.y + (down ? fromHeight : 0);
    const y2 = to.y + (down ? 0 : toHeight);
    const middle = (y1 + y2) / 2;
    return `M ${x1} ${y1} C ${x1} ${middle}, ${x2} ${middle}, ${x2} ${y2}`;
  }
  const forwards = to.x >= from.x;
  const start = { x: forwards ? from.x + CARD_WIDTH : from.x, y: from.y + fromHeight / 2 };
  const end = { x: forwards ? to.x : to.x + CARD_WIDTH, y: to.y + toHeight / 2 };
  // A single gentle curve, not an elbow: the board reads as a whiteboard now, and a whiteboard
  // arrow is one stroke of the wrist. The reach scales with the gap so short hops stay shallow
  // and long ones do not flatten into straight lines. Direction is still decided per edge.
  const reach = Math.max(36, Math.abs(end.x - start.x) * 0.45) * (forwards ? 1 : -1);
  return `M ${start.x} ${start.y} C ${start.x + reach} ${start.y}, ${end.x - reach} ${end.y}, ${end.x} ${end.y}`;
}

export function Board({
  model,
  board,
  selectedNode,
  onSelectNode,
  onChange,
  connectionNote,
  connectionTone,
  graphFile,
  onGraphFileChange,
  onDrawConnections,
  busy,
  crew = [],
  selectedAgent = null,
  onSelectAgent,
  talks = [],
  activity = [],
  agentReports = {},
  runStatus = null,
  attention = null,
  nextAction = null,
  onNextAction,
  selectedTalk = null,
  onSelectTalk,
  focusGraphFile = 0,
  runId,
  initialLayout = "canvas",
  onCanvasChange,
  objective = null,
  demonstration = false,
  ended = false,
  fixtureFile = "",
  onFixtureFileChange,
}: {
  model: GraphModel;
  board: BoardState;
  selectedNode: string | null;
  onSelectNode: (nodeId: string | null) => void;
  onChange: (next: BoardState) => void;
  /** What the page has to say about whether the connections could be trusted. Composed by
   * `graph/topology.ts`, never by this component. */
  connectionNote: string;
  connectionTone: "proven" | "refused" | "none";
  /** The graph file path on the RUNTIME's host. One field, two uses: it is what the topology read
   * needs to draw connections and what `resume` needs to lift a hold, and asking for it twice
   * would invite two answers. */
  graphFile: string;
  onGraphFileChange: (value: string) => void;
  onDrawConnections: () => void;
  busy: boolean;
  /** The room's roster, standing on the canvas as draggable blobs. Derived by App from the
   * log; this component only places and moves them. `presence` is the newest
   * `agent_presence_declared` this actor has made, or absent when the actor never declared one -
   * absence renders no badge at all, never a placeholder. */
  crew?: Array<{ id: string; charter: string | null; lastAt?: string | null; presence?: AgentPresence | null }>;
  selectedAgent?: string | null;
  onSelectAgent?: (agentId: string | null) => void;
  /** The room's conversations, each one a bubble standing on the board. Derived by App from
   * the envelopes; this component only places and moves them. */
  talks?: Array<{ key: string; label: string; participants: string[]; count: number; lastAt: string | null; preview?: string | null }>;
  activity?: WorkOverviewProps["activity"];
  agentReports?: WorkOverviewProps["agentReports"];
  runStatus?: string | null;
  attention?: WorkOverviewProps["attention"];
  nextAction?: WorkOverviewProps["nextAction"];
  onNextAction?: WorkOverviewProps["onNextAction"];
  selectedTalk?: string | null;
  onSelectTalk?: (talkKey: string | null) => void;
  /** Bumped when another control (the dock's resume) needs the person AT the graph-file box:
   * opens the row and puts the cursor in it. A nonce so a second walk works. */
  focusGraphFile?: number;
  /** The selected run's id, for the HUD capsule. Absent (a draft, no selection) renders none. */
  runId?: string;
  initialLayout?: "overview" | "canvas";
  onCanvasChange?: (canvas: boolean) => void;
  /** The run's objective from its briefing (#1077), for the entry node's card. */
  objective?: string | null;
  /** The run was started under the fixture executor (#1064). */
  demonstration?: boolean;
  /** #1098 D5: the run reached a terminal status; passed straight through to the overview, which
   * states an unverified topology in the tense that is true for a run nothing will add to. */
  ended?: boolean;
  /** #1083 F2: a fixture file path on the RUNTIME's host, sent with `resume` as the API's
   * existing `fixtures` field. Offered only on a demonstration run - the fixture stands in for
   * the model there, and without it a resumed node has no outcome and parks `waiting_input`. */
  fixtureFile?: string;
  onFixtureFileChange?: (value: string) => void;
}) {
  const [organized, setOrganized] = useState(initialLayout === "overview");
  useEffect(() => { onCanvasChange?.(!organized); }, [organized, onCanvasChange]);
  const [layoutUndo, setLayoutUndo] = useState<{ run: string | undefined; positions: BoardState["positions"]; agents: BoardState["agents"] } | null>(null);
  const arrangePending = useRef(false);
  const surface = useRef<HTMLDivElement | null>(null);
  const [tool, setTool] = useState<Tool>("select");
  const [tone, setTone] = useState<Tone>("ink");
  const [drawing, setDrawing] = useState<Point[] | null>(null);
  /* The graph-file row is chrome nobody needs until they want connections drawn: shown when
     asked, or whenever the field already holds something worth seeing. */
  const [connectOpen, setConnectOpen] = useState(false);
  /* THE CAMERA. Pan and zoom are the viewer's, not the board's: they live in this tab only and
     never write to the stored board - a camera move is not an annotation. */
  const [view, setView] = useState({ x: 0, y: 0, zoom: 1 });
  const viewRef = useRef(view);
  viewRef.current = view;
  /* THE MULTI-SELECTION: prefixed ids ("agent:x" / "talk:k" / "node:n"), chosen by marquee or
     Ctrl+A, moved as one hand of pieces. Separate from the focus - picking many opens nothing. */
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const pickedRef = useRef(picked);
  pickedRef.current = picked;
  const [marquee, setMarquee] = useState<{ a: Point; b: Point } | null>(null);
  const marqueeRef = useRef<{ a: Point; b: Point } | null>(null);
  const itemRectsRef = useRef<Array<{ id: string; x: number; y: number; w: number; h: number }>>([]);
  const groupStartRef = useRef<Map<string, Point>>(new Map());
  const panStartRef = useRef<{ vx: number; vy: number; sx: number; sy: number } | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);
  // The dock's resume button walks the person here when the path is missing: a control that
  // names its own missing ingredient should go fetch it, not sit as a labelled excuse.
  useEffect(() => {
    if ((focusGraphFile ?? 0) > 0) { setOrganized(false); setConnectOpen(true); }
  }, [focusGraphFile]);
  useEffect(() => {
    if (connectOpen && (focusGraphFile ?? 0) > 0) fileRef.current?.focus();
  }, [connectOpen, focusGraphFile]);

  /**
   * Live state for the window listeners below.
   *
   * The listeners are installed ONCE and never re-installed, so they cannot close over a stale
   * render. Everything they read has to be in a ref. An earlier version made installation
   * conditional on a ref — which does not re-render — so `pointerup` was simply never wired, and a
   * drag stayed armed after the button came up.
   */
  const dragging = useRef<{ kind: "node" | "agent" | "note" | "pan" | "group"; id: string; grab: Point; origin: Point; moved: boolean } | null>(null);
  /* Set when a real drag ends, read by the click that follows it: the click is the drag's
     echo, not an intent, and must not open or toggle anything. */
  const swallowClick = useRef(false);
  const strokeRef = useRef<Point[] | null>(null);
  const boardRef = useRef(board);
  const toneRef = useRef(tone);
  const changeRef = useRef(onChange);
  boardRef.current = board;
  toneRef.current = tone;
  changeRef.current = onChange;

  const pointAt = useCallback((event: { clientX: number; clientY: number }): Point => {
    const element = surface.current;
    if (!element) return { x: 0, y: 0 };
    const box = element.getBoundingClientRect();
    const camera = viewRef.current;
    return {
      x: (event.clientX - box.left - camera.x) / camera.zoom,
      y: (event.clientY - box.top - camera.y) / camera.zoom,
    };
  }, []);

  useEffect(() => {
    const move = (event: PointerEvent) => {
      const held = dragging.current;
      if (held !== null && held.kind === "pan") {
        const start = panStartRef.current;
        if (start !== null) {
          held.moved = true;
          surface.current?.classList.add("dragging");
          setView((current) => ({
            ...current,
            x: start.vx + (event.clientX - start.sx),
            y: start.vy + (event.clientY - start.sy),
          }));
        }
        return;
      }
      if (held !== null && held.kind === "group") {
        const at = pointAt(event);
        if (!held.moved) {
          if (Math.hypot(at.x - held.origin.x, at.y - held.origin.y) < 4) return;
          held.moved = true;
          surface.current?.classList.add("dragging");
        }
        const dx = at.x - held.origin.x;
        const dy = at.y - held.origin.y;
        const positions = { ...boardRef.current.positions };
        const agents = { ...boardRef.current.agents };
        for (const [prefixed, start] of groupStartRef.current) {
          const to = { x: start.x + dx, y: start.y + dy };
          if (prefixed.startsWith("node:")) positions[prefixed.slice(5)] = to;
          else if (prefixed.startsWith("agent:")) agents[prefixed.slice(6)] = to;
          else if (prefixed.startsWith("talk:")) agents[prefixed] = to;
        }
        changeRef.current({ ...boardRef.current, positions, agents });
        return;
      }
      if (marqueeRef.current !== null) {
        marqueeRef.current = { a: marqueeRef.current.a, b: pointAt(event) };
        setMarquee(marqueeRef.current);
        return;
      }
      if (held !== null) {
        const at = pointAt(event);
        // Below the threshold nothing moves and the coming click stays a click. Past it, the
        // hold IS a drag: the card follows 1:1 and the click it releases into is swallowed.
        if (!held.moved) {
          if (Math.hypot(at.x - held.origin.x, at.y - held.origin.y) < 4) return;
          held.moved = true;
          surface.current?.classList.add("dragging");
        }
        const to = { x: at.x - held.grab.x, y: at.y - held.grab.y };
        changeRef.current(
          held.kind === "node"
            ? { ...boardRef.current, positions: { ...boardRef.current.positions, [held.id]: to } }
            : held.kind === "note"
              ? {
                  ...boardRef.current,
                  notes: boardRef.current.notes.map((candidate) =>
                    candidate.id === held.id ? { ...candidate, at: to } : candidate,
                  ),
                }
              : { ...boardRef.current, agents: { ...boardRef.current.agents, [held.id]: to } },
        );
        return;
      }
      if (strokeRef.current === null) return;
      strokeRef.current = [...strokeRef.current, pointAt(event)];
      setDrawing(strokeRef.current);
    };

    const release = () => {
      const box = marqueeRef.current;
      if (box !== null) {
        marqueeRef.current = null;
        setMarquee(null);
        const x1 = Math.min(box.a.x, box.b.x);
        const x2 = Math.max(box.a.x, box.b.x);
        const y1 = Math.min(box.a.y, box.b.y);
        const y2 = Math.max(box.a.y, box.b.y);
        if (x2 - x1 > 6 || y2 - y1 > 6) {
          const hits = itemRectsRef.current
            .filter((item) => item.x < x2 && x1 < item.x + item.w && item.y < y2 && y1 < item.y + item.h)
            .map((item) => item.id);
          setPicked(new Set(hits));
          swallowClick.current = true;
          setTimeout(() => {
            swallowClick.current = false;
          }, 0);
        }
      }
      panStartRef.current = null;
      if (dragging.current?.moved) {
        swallowClick.current = true;
        setTimeout(() => {
          swallowClick.current = false;
        }, 0);
      }
      surface.current?.classList.remove("dragging");
      dragging.current = null;
      const points = strokeRef.current;
      strokeRef.current = null;
      setDrawing(null);
      if (points !== null && points.length > 1) {
        changeRef.current({
          ...boardRef.current,
          strokes: [...boardRef.current.strokes, { id: markId(), tone: toneRef.current, points }],
        });
      }
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", release);
    // `pointercancel` too: a pointer the browser takes away otherwise leaves a drag armed, which
    // is the same stale hold the old shape produced.
    window.addEventListener("pointercancel", release);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
    };
  }, [pointAt]);

  /* THE DEFAULT GRID USES THE WIDTH IT HAS (#1077). The judge opened a seven-card run and saw
     two: cards stacked in a column, so a frame that held them all was a miniature. Columns
     follow the surface's own width - floor(width / card width), never fewer than two - and the
     rows follow. Measured, not assumed: the surface is re-read on mount and on every resize.
     Only DEFAULT positions move with the width; a position the operator saved still wins. */
  const [surfaceWidth, setSurfaceWidth] = useState(0);
  const columns = Math.max(2, Math.floor(surfaceWidth / CARD_WIDTH));
  /** The run carries an objective, so its first entry card is the tallest card on the board
   * (graph/board.ts): bounds, edges, the camera AND the row step measure that card as drawn.
   * At the old fixed step the row below overlapped the entry card by 20px (#1077). */
  const hasObjective = objective !== null && objective.trim().length > 0;
  const nodePosition = (nodeId: string, index: number): Point =>
    board.positions[nodeId] ?? gridPosition(index, columns, hasObjective);
  const places = new Map<string, Point>(
    model.nodes.map((node, index) => [node.id, nodePosition(node.id, index)]),
  );
  /** The card's measured height (graph/board.ts): the first entry node carries the objective
   * and is taller by its allowance, so bounds, edges and the camera frame what is drawn. */
  const nodeHeight = (node: GraphNode) => cardHeight(node, hasObjective && isFirstEntryNode(model, node.id));
  /* THE INK COVERS WHAT IS DRAWN (#1072). The sheet-ink SVG was a fixed 2600x1700 box, so a
     verified connection into a generated column or row past it was clipped while the card it
     points at stayed visible. The box grows to the farthest card plus a margin; the old size
     stays the floor so strokes and talk lines drawn inside it keep their room. */
  const inkWidth = Math.max(2600, ...model.nodes.map((node, index) => nodePosition(node.id, index).x + CARD_WIDTH + 80));
  const inkHeight = Math.max(1700, ...model.nodes.map((node, index) => nodePosition(node.id, index).y + nodeHeight(node) + 80));
  const edgeGeometry = model.edges.flatMap((edge) => {
    const from = places.get(edge.from);
    const to = places.get(edge.to);
    if (!from || !to) return [];
    return [{ id: edge.id, type: edge.type, d: edgePath(from, to, nodeHeight(model.nodes.find(node => node.id === edge.from)!), nodeHeight(model.nodes.find(node => node.id === edge.to)!)) }];
  });

  // The crew is placed by RANK, not roster order: total pair-talk traffic decides who holds
  // the centre of the web. Deterministic - same log, same board.
  const agentTraffic = new Map<string, number>();
  for (const talk of talks) {
    if (talk.key === "room") continue;
    for (const id of talk.participants) {
      agentTraffic.set(id, (agentTraffic.get(id) ?? 0) + talk.count);
    }
  }
  const rankOf = new Map<string, number>();
  [...crew]
    .sort(
      (a, b) =>
        (agentTraffic.get(b.id) ?? 0) - (agentTraffic.get(a.id) ?? 0) || a.id.localeCompare(b.id),
    )
    .forEach((member, rank) => rankOf.set(member.id, rank));
  // Agents within reach of each other form a huddle: a single-link clustering over the blob
  // anchors, recomputed on every render and stored nowhere - drag apart and the ring is gone.
  const agentPlaces = crew.map((member, index) => ({
    id: member.id,
    at: agentPositionOf(board, member.id, rankOf.get(member.id) ?? index),
  }));
  const HUDDLE_REACH = 110;
  const clusterOf = new Map<string, number>();
  agentPlaces.forEach((agent, index) => clusterOf.set(agent.id, index));
  for (const a of agentPlaces) {
    for (const b of agentPlaces) {
      if (a.id === b.id) continue;
      if (Math.hypot(a.at.x - b.at.x, a.at.y - b.at.y) > HUDDLE_REACH) continue;
      const from = clusterOf.get(b.id)!;
      const to = clusterOf.get(a.id)!;
      if (from === to) continue;
      for (const [id, cluster] of clusterOf) if (cluster === from) clusterOf.set(id, to);
    }
  }
  const grouped = new Map<number, typeof agentPlaces>();
  for (const agent of agentPlaces) {
    const cluster = clusterOf.get(agent.id)!;
    grouped.set(cluster, [...(grouped.get(cluster) ?? []), agent]);
  }
  const huddles = [...grouped.values()]
    .filter((members) => members.length >= 2)
    .map((members) => {
      const xs = members.map((member) => member.at.x);
      const ys = members.map((member) => member.at.y);
      const minX = Math.min(...xs) - 58;
      const maxX = Math.max(...xs) + 58;
      const minY = Math.min(...ys) - 52;
      const maxY = Math.max(...ys) + 64;
      return {
        key: members.map((member) => member.id).sort().join("+"),
        cx: (minX + maxX) / 2,
        cy: (minY + maxY) / 2,
        rx: (maxX - minX) / 2,
        ry: (maxY - minY) / 2,
      };
    });

  // Separate lanes keep conversation links short and leave the work graph unobstructed.
  // Stored positions always win; only an explicit Organize resets the operator's placement.
  const BUBBLE_W = 280;
  const BUBBLE_H = 148;
  const occupied: BoardBounds[] = [
    ...model.nodes.map((node, index) => ({ ...nodePosition(node.id, index), w: CARD_WIDTH, h: nodeHeight(node) })),
    ...agentPlaces.map(({ at }) => ({ x: at.x - 88, y: at.y - 26, w: 176, h: 96 })),
    ...talks.flatMap(talk => {
      const at = board.agents[`talk:${talk.key}`];
      return at ? [{ ...at, w: BUBBLE_W, h: BUBBLE_H }] : [];
    }),
  ];
  const talkPlaces = [...talks].sort((a, b) => {
    if (a.key === b.key) return 0;
    if (a.key === "room") return -1;
    if (b.key === "room") return 1;
    return a.key.localeCompare(b.key);
  }).map((talk, rank) => {
    const stored = board.agents[`talk:${talk.key}`];
    if (stored) return { talk, at: stored };
    const at = { x: 280, y: 100 + rank * 180 };
    // Every downward move clears at least one finite obstacle; no unchecked fallback.
    for (let attempt = 0; attempt <= occupied.length; attempt += 1) {
      const collisions = occupied.filter(zone => at.x < zone.x + zone.w + 18 && zone.x < at.x + BUBBLE_W + 18 && at.y < zone.y + zone.h + 18 && zone.y < at.y + BUBBLE_H + 18);
      if (collisions.length === 0) break;
      at.y = Math.max(...collisions.map(zone => zone.y + zone.h + 18));
    }
    occupied.push({ ...at, w: BUBBLE_W, h: BUBBLE_H });
    return { talk, at };
  });

  const contentBounds = (() => {
    const items: BoardBounds[] = [
      ...model.nodes.map((node, index) => {
        const at = nodePosition(node.id, index);
        return { x: at.x, y: at.y, w: CARD_WIDTH, h: nodeHeight(node) };
      }),
      ...agentPlaces.map((agent) => ({ x: agent.at.x - 88, y: agent.at.y - 26, w: 176, h: 96 })),
      ...talkPlaces.map(({ at }) => ({ x: at.x, y: at.y, w: BUBBLE_W, h: BUBBLE_H })),
    ];
    if (items.length === 0) return null;
    items.push({ x: -8, y: 8, w: 1038, h: Math.max(330, crew.length * 140 + 104, talks.length * 180 + 104) });
    const left = Math.min(...items.map((item) => item.x));
    const top = Math.min(...items.map((item) => item.y));
    const right = Math.max(...items.map((item) => item.x + item.w));
    const bottom = Math.max(...items.map((item) => item.y + item.h));
    return { x: left, y: top, w: right - left, h: bottom - top };
  })();
  /** The work cards' own bounds (#1083 F8) - what first framing and `fit` frame. The lanes'
   * chrome, the agents and the conversations are not the work: counting them made a six-card
   * run open at 60% with two cards on screen, and `fit` drop to 15%. */
  const cardBounds = (() => {
    if (model.nodes.length === 0) return null;
    const cards = model.nodes.map((node, index) => ({ ...nodePosition(node.id, index), w: CARD_WIDTH, h: nodeHeight(node) }));
    const left = Math.min(...cards.map((card) => card.x));
    const top = Math.min(...cards.map((card) => card.y));
    const right = Math.max(...cards.map((card) => card.x + card.w));
    const bottom = Math.max(...cards.map((card) => card.y + card.h));
    // The first rank's tallest card: the least a short band must hold whole (#1083 F8).
    const rank = Math.max(...cards.filter((card) => card.y === top).map((card) => card.h));
    return { x: left, y: top, w: right - left, h: bottom - top, rank };
  })();
  const focusedBoundsRef = useRef<BoardBounds | null>(null);
  const fit = (selection?: BoardBounds, wholeMap = false) => {
    const box = surface.current?.getBoundingClientRect();
    const first = model.nodes[0];
    // THE PHONE RULE READS THE WINDOW, NOT THE SHEET (#1083 F8). Keyed on the sheet's width, it
    // fired on a 1280px desktop whenever the conversation column left the canvas under 700px
    // (measured: 683px), framed ONE card at 60% and showed two of six - the finding itself - and
    // the resize observer put it back after every `fit`.
    const phone = typeof window !== "undefined" && window.innerWidth < 700;
    const mobileFocus = !wholeMap && !selection && box && phone && first ? { ...nodePosition(first.id, 0), w: CARD_WIDTH, h: nodeHeight(first) } : null;
    const bounds = selection ?? mobileFocus ?? cardBounds ?? contentBounds;
    if (!box || box.width <= 0 || box.height <= 0 || !bounds) return;
    focusedBoundsRef.current = selection ?? null;
    // FRAME INSIDE WHAT IS NOT COVERED (#1083 F8, orchestrator verification at 1280x720). The
    // canvas toolbar, the header chrome, the lint strip and the docks float over the sheet, and a
    // frame computed for the whole sheet put the cards' lower third under the toolbar row. The
    // band left between the chrome above and below is the viewport; the camera is shifted down
    // by what covers the top.
    const insets = chromeInsets(box, chromeRectsAround(surface.current));
    const viewport = { w: box.width, h: Math.max(1, box.height - insets.top - insets.bottom) };
    const below = (camera: Camera): Camera => ({ ...camera, y: camera.y + insets.top });
    // A selection or the mobile single-card focus fits exactly what was asked for.
    if (selection || mobileFocus || cardBounds === null) {
      setView(below(fitCamera(bounds, viewport, 24, selection ? 1.25 : 1)));
      return;
    }
    // Automatic framing and `fit` frame THE CARDS at a readable zoom (#1077, #1083 F8): all of
    // them when they fit at `READABLE_ZOOM` or better, otherwise the first rank whole, and a pan
    // reveals the rest. `wholeMap` is kept as the explicit-fit signature.
    void wholeMap;
    setView(below(frameCards(bounds, viewport, 24, cardBounds.rank)));
  };
  const fitRef = useRef(fit);
  fitRef.current = fit;
  useEffect(() => {
    if (!organized) fitRef.current();
  }, [organized]);
  useEffect(() => {
    if (arrangePending.current) {
      arrangePending.current = false;
      fitRef.current();
    }
  }, [board.positions, board.agents]);
  const framedRunRef = useRef<string | null>(null);
  useEffect(() => {
    // Keyed by run AND by column count: a width change re-lays the default grid, and a frame
    // computed for the old grid would show the new one half off screen.
    const frameKey = `${runId ?? "draft"}@${columns}`;
    if (framedRunRef.current !== frameKey && contentBounds !== null) {
      fit(undefined, true);
      framedRunRef.current = frameKey;
    }
  }, [runId, contentBounds, columns]);
  const measure = useCallback(() => {
    const box = surface.current?.getBoundingClientRect();
    if (box && box.width > 0) setSurfaceWidth((current) => (current === box.width ? current : box.width));
  }, []);
  useEffect(() => {
    measure();
    const element = surface.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      measure();
      if (framedRunRef.current !== null) fitRef.current(focusedBoundsRef.current ?? undefined);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [measure]);
  // The surface is `hidden` while the overview is up, and a hidden box measures zero: re-read
  // it when the free canvas is shown, so the grid follows the width the cards actually get.
  useEffect(() => {
    if (!organized) measure();
  }, [organized, measure]);

  const onSurfacePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    if (tool === "hand") {
      event.preventDefault();
      window.getSelection()?.removeAllRanges();
      panStartRef.current = { vx: view.x, vy: view.y, sx: event.clientX, sy: event.clientY };
      dragging.current = { kind: "pan", id: "", grab: { x: 0, y: 0 }, origin: pointAt(event), moved: false };
      return;
    }
    if (event.target !== event.currentTarget && !(event.target as HTMLElement).closest(".sheet-ink")) {
      return;
    }
    event.preventDefault();
    if (tool === "pen") {
      const first = [pointAt(event)];
      strokeRef.current = first;
      setDrawing(first);
      return;
    }
    if (tool === "note") {
      onChange({ ...board, notes: [...board.notes, { id: markId(), at: pointAt(event), text: "" }] });
      setTool("select");
      return;
    }
    // Select tool on empty board: a drag is a marquee. The click that ends an empty marquee is
    // swallowed; a plain click still deselects via onSurfaceClick.
    marqueeRef.current = { a: pointAt(event), b: pointAt(event) };
  };

  const showConnect = connectOpen || graphFile.trim().length > 0 || connectionTone !== "none";

  itemRectsRef.current = [
    ...agentPlaces.map((agent) => ({ id: `agent:${agent.id}`, x: agent.at.x - 88, y: agent.at.y - 26, w: 176, h: 96 })),
    ...talkPlaces.map(({ talk, at }) => ({ id: `talk:${talk.key}`, x: at.x, y: at.y, w: BUBBLE_W, h: BUBBLE_H })),
    ...model.nodes.map((node, index) => {
      const at = nodePosition(node.id, index);
      return { id: `node:${node.id}`, x: at.x, y: at.y, w: CARD_WIDTH, h: nodeHeight(node) };
    }),
  ];

  /** Arms a GROUP drag when the grabbed piece is part of the multi-selection: every picked
   * piece's start position is captured once, and the move handler slides them all by one delta. */
  const armGroup = (prefixed: string, here: Point): boolean => {
    if (!pickedRef.current.has(prefixed) || pickedRef.current.size < 2) return false;
    const starts = new Map<string, Point>();
    for (const id of pickedRef.current) {
      if (id.startsWith("agent:")) {
        const at = agentPlaces.find((agent) => `agent:${agent.id}` === id)?.at;
        if (at) starts.set(id, at);
      } else if (id.startsWith("talk:")) {
        const at = talkPlaces.find(({ talk }) => `talk:${talk.key}` === id)?.at;
        if (at) starts.set(id, at);
      } else if (id.startsWith("node:")) {
        const index = model.nodes.findIndex((node) => `node:${node.id}` === id);
        if (index >= 0) starts.set(id, nodePosition(model.nodes[index].id, index));
      }
    }
    groupStartRef.current = starts;
    dragging.current = { kind: "group", id: prefixed, grab: { x: 0, y: 0 }, origin: here, moved: false };
    return true;
  };

  const onSurfaceClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget && !(event.target as HTMLElement).closest(".sheet-ink")) {
      return;
    }
    if (swallowClick.current) return;
    if (tool === "select") {
      onSelectNode(null);
      setPicked(new Set());
    }
  };

  const spaceHeld = useRef<Tool | null>(null);
  useEffect(() => {
    if (organized) return;
    const typing = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      return Boolean(target?.closest("input, textarea, select, button, summary, a, [contenteditable='true'], [role='button']"));
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setPicked(new Set());
        setTool("select");
        return;
      }
      if (typing(event)) return;
      // Space holds the hand, the way every whiteboard does it; releasing gives the tool back.
      if (event.key === " " && spaceHeld.current === null) {
        event.preventDefault();
        setTool((current) => {
          spaceHeld.current = current;
          return "hand";
        });
        return;
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") {
        event.preventDefault();
        setPicked(new Set(itemRectsRef.current.map((item) => item.id)));
      }
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (event.key === " " && spaceHeld.current !== null) {
        const back = spaceHeld.current;
        spaceHeld.current = null;
        setTool(back);
      }
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [organized]);

  // Ctrl+wheel zooms at the cursor; a plain wheel pans. Native listener because React's wheel
  // is passive and the browser's own page-zoom must be preempted.
  useEffect(() => {
    const element = surface.current;
    if (element === null) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        const box = element.getBoundingClientRect();
        const cx = event.clientX - box.left;
        const cy = event.clientY - box.top;
        setView((current) => {
          const zoom = Math.min(2.5, Math.max(0.3, current.zoom * (1 - event.deltaY * 0.0015)));
          const scale = zoom / current.zoom;
          return { zoom, x: cx - (cx - current.x) * scale, y: cy - (cy - current.y) * scale };
        });
      } else {
        setView((current) => ({ ...current, x: current.x - event.deltaX, y: current.y - event.deltaY }));
      }
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, []);

  const zoomBy = (factor: number) => {
    const element = surface.current;
    const box = element?.getBoundingClientRect();
    const cx = box ? box.width / 2 : 0;
    const cy = box ? box.height / 2 : 0;
    setView((current) => {
      const zoom = Math.min(2.5, Math.max(0.3, current.zoom * factor));
      const scale = zoom / current.zoom;
      return { zoom, x: cx - (cx - current.x) * scale, y: cy - (cy - current.y) * scale };
    });
  };

  /** What the lint strip says, by kind (#1083): see `splitLint`. */
  const lintSplit = splitLint(model.lint, demonstration);

  const undo = () => {
    if (board.strokes.length > 0) {
      onChange({ ...board, strokes: board.strokes.slice(0, -1) });
      return;
    }
    if (board.notes.length > 0) onChange({ ...board, notes: board.notes.slice(0, -1) });
  };

  return (
    <>
      <div className="work-view-switch" role="group" aria-label="Workspace view">
        <button type="button" aria-pressed={organized} onClick={() => setOrganized(true)}>Overview</button>
        <button type="button" aria-pressed={!organized} onClick={() => setOrganized(false)}>Free canvas</button>
      </div>
      {organized && <div className="work-overview-scroll">
        <WorkOverview model={model} crew={crew} talks={talks} activity={activity} agentReports={agentReports} runStatus={runStatus} attention={attention} nextAction={nextAction} onNextAction={onNextAction} selectedNode={selectedNode} onSelectNode={onSelectNode} selectedAgent={selectedAgent} onSelectAgent={onSelectAgent} selectedTalk={selectedTalk} onSelectTalk={onSelectTalk} runId={runId} objective={objective} demonstration={demonstration} ended={ended} />
        <div className="work-verification"><span>{model.edgesKnown ? connectionNote : "Connect the run’s graph to see verified dependencies."}</span><button type="button" onClick={() => { setOrganized(false); setConnectOpen(true); }}>Verify connections</button></div>
      </div>}
      <div className="free-canvas-content" hidden={organized}>
      <div className="canvas-story">
        <span><Activity aria-hidden="true" /> Execution map</span>
        <h2>{model.nodes.length === 0 ? "Waiting for work to appear" : model.nodes.every(node => node.touches === 0) ? "No node activity yet" : `${model.nodes.filter(node => node.state === "running").length} running / ${model.nodes.filter(node => node.state === "blocked").length} blocked`}</h2>
        <p>{model.nodes.every(node => node.touches === 0) ? "Follow the conversations. Node updates will appear as work is reported." : "Follow the people, their conversations, and the latest reported work."}</p>
      </div>
      <div className="canvas-arrange" role="group" aria-label="Canvas layout">
        <button type="button" onClick={() => {
          setLayoutUndo({ run: runId, positions: board.positions, agents: board.agents });
          arrangePending.current = true;
          onChange(tidyBoard(board));
        }}>Organize</button>
        {layoutUndo?.run === runId && layoutUndo !== null && <button type="button" onClick={() => {
          arrangePending.current = true;
          onChange({ ...board, positions: layoutUndo.positions, agents: layoutUndo.agents });
          setLayoutUndo(null);
        }}>Undo layout</button>}
        <span>Drag to arrange / select to inspect</span>
      </div>
      <div
        className={`sheet tool-${tool}`}
        ref={surface}
        onPointerDown={onSurfacePointerDown}
        onClick={onSurfaceClick}
        aria-label="Execution board"
      >
        {/* THE RUN CAPSULE - a HUD, not a citizen of the canvas: it sits OUTSIDE `.world`, so it
          * neither pans nor scales with the camera. The one card about the whole run, and the
          * ONLY progress bar the log can honestly back: nodes that need nothing more (succeeded,
          * waived, skipped) over nodes declared. Per-node progress does not exist in the log. */}
        {runId !== undefined && model.nodes.length > 0 && (() => {
          const done = model.nodes.filter(
            (node) => node.state === "succeeded" || node.state === "waived" || node.state === "skipped",
          ).length;
          const total = model.nodes.length;
          // The run's own pulse line: the newest instant any node reported, straight off the
          // log's timestamps - never a liveness the data cannot back.
          const lastActivity = model.nodes.reduce<string | null>(
            (newest, node) =>
              node.lastEventAt !== null && (newest === null || node.lastEventAt > newest)
                ? node.lastEventAt
                : newest,
            null,
          );
          return (
            <article className="run-capsule" aria-label="This run's progress">
              <p className="capsule-eyebrow">#run</p>
              <h3 className="run-capsule-title">{runId}</h3>
              <p className="run-capsule-line">
                {done} of {total} nodes done
              </p>
              <span className="run-progress" aria-hidden="true">
                <i style={{ width: `${Math.round((done / total) * 100)}%` }} />
              </span>
              {lastActivity !== null && (
                <p className="run-capsule-line node-fresh">last activity · {ago(lastActivity)}</p>
              )}
            </article>
          );
        })()}
        <div
          className="world"
          style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})` }}
        >
        <div className="canvas-region" style={{ left: -8, top: 8, width: 216, height: Math.max(420, crew.length * 140 + 104) }} aria-hidden="true">
          <header><Users /><strong>People</strong><span>{crew.length}</span></header>
          <p>Agents seen in this run</p>
        </div>
        <div className="canvas-region" style={{ left: 250, top: 8, width: 340, height: Math.max(330, talks.length * 180 + 104) }} aria-hidden="true">
          <header><MessageSquare /><strong>Conversations</strong><span>{talks.length}</span></header>
          <p>Who is talking to whom</p>
        </div>
        <div className="canvas-region work-region" style={{ left: 650, top: 8, width: Math.max(380, Math.min(columns, model.nodes.length) * 380), height: Math.max(330, ...model.nodes.map((node, index) => nodePosition(node.id, index).y + nodeHeight(node) + 24)) }}>
          <header><GitBranch /><strong>Work</strong><span>{model.nodes.length}{model.rosterDeclared ? "" : "+"}</span></header>
          <p>{model.edgesKnown ? "Verified dependencies connect these nodes" : "Connections have not been verified"}</p>
          {!model.edgesKnown && <button type="button" onClick={() => setConnectOpen(true)}>Verify connections</button>}
        </div>

        <svg className="sheet-ink" aria-hidden="true" width={inkWidth} height={inkHeight} style={{ width: `${inkWidth}px`, height: `${inkHeight}px` }}>
          <defs>
            <filter id="roughen" x="-5%" y="-5%" width="110%" height="110%">
              <feTurbulence type="fractalNoise" baseFrequency="0.035" numOctaves="2" seed="7" result="noise" />
              <feDisplacementMap in="SourceGraphic" in2="noise" scale="2.6" />
            </filter>
            <marker
              id="edge-tip"
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="6"
              markerHeight="6"
              orient="auto-start-reverse"
            >
              <path className="edge-tip" d="M 0 1 L 9 5 L 0 9" />
            </marker>
          </defs>
          {talkPlaces.flatMap(({ talk, at }) =>
            talk.participants.flatMap((id) => {
              const anchor = agentPlaces.find((agent) => agent.id === id)?.at;
              if (anchor === undefined) return [];
              return [
                <path
                  key={`${talk.key}->${id}`}
                  className={`talk-tie ${selectedTalk === talk.key ? "on" : ""}`}
                  d={`M ${anchor.x + 88} ${anchor.y} C ${anchor.x + 132} ${anchor.y}, ${at.x - 64} ${at.y + 40}, ${at.x} ${at.y + 40}`}
                />,
              ];
            }),
          )}
          {huddles.map((huddle) => (
            <ellipse
              key={huddle.key}
              className="huddle"
              cx={huddle.cx}
              cy={huddle.cy}
              rx={huddle.rx}
              ry={huddle.ry}
            />
          ))}
          {edgeGeometry.map((edge) => (
            <path key={edge.id} className={`edge ${edge.type}`} d={edge.d} markerEnd="url(#edge-tip)" />
          ))}
          {board.strokes.map((stroke) => (
            <path key={stroke.id} className={`stroke ${stroke.tone}`} d={pathOf(stroke.points)} />
          ))}
          {drawing && <path className={`stroke ${tone} live`} d={pathOf(drawing)} />}
        </svg>
        {model.edgesKnown && model.edges.length > 0 && (
          <ul className="sr-only" aria-label="Verified dependencies">
            {model.edges.map((edge) => (
              <li key={edge.id}>{edge.from} connects to {edge.to} ({edge.type})</li>
            ))}
          </ul>
        )}

        {board.notes.map((note) => (
          <label
            className="sheet-note"
            key={note.id}
            style={{ left: note.at.x, top: note.at.y }}
            onPointerDown={(event) => {
              // The textarea keeps the pointer for text selection; the border is the handle.
              if (tool !== "select") return;
              if ((event.target as HTMLElement).tagName === "TEXTAREA") return;
              const here = pointAt(event);
              dragging.current = {
                kind: "note",
                id: note.id,
                grab: { x: here.x - note.at.x, y: here.y - note.at.y },
                origin: here,
                moved: false,
              };
            }}
          >
            <span className="sr-only">Note</span>
            <textarea
              value={note.text}
              placeholder={"Write it down" + "\u2026"}
              onChange={(event) =>
                onChange({
                  ...board,
                  notes: board.notes.map((candidate) =>
                    candidate.id === note.id ? { ...candidate, text: event.target.value } : candidate,
                  ),
                })
              }
              onBlur={() => {
                // An empty note left behind is litter, not an annotation.
                if (note.text.trim().length === 0) {
                  onChange({ ...board, notes: board.notes.filter((candidate) => candidate.id !== note.id) });
                }
              }}
            />
          </label>
        ))}

        {model.nodes.map((node, index) => (
          <NodeBlock
            connections={model.edgesKnown ? model.edges.filter(edge => edge.from === node.id || edge.to === node.id).length : null}
            key={node.id}
            node={node}
            at={nodePosition(node.id, index)}
            entry={isEntryNode(model, node.id)}
            objective={isFirstEntryNode(model, node.id) ? objective : null}
            selected={node.id === selectedNode}
            multi={picked.has(`node:${node.id}`)}
            onOpen={() => {
              if (swallowClick.current) return;
              focusedBoundsRef.current = itemRectsRef.current.find((item) => item.id === `node:${node.id}`) ?? null;
              onSelectNode(node.id);
            }}
            highlight={
              selectedAgent !== null ? `hsl(${hueOf(selectedAgent)} 52% 60%)` : null
            }
            highlightAgent={selectedAgent}
            onGrab={(event) => {
              if (tool !== "select") return;
              const here = pointAt(event);
              if (armGroup(`node:${node.id}`, here)) return;
              const at = nodePosition(node.id, index);
              dragging.current = {
                kind: "node",
                id: node.id,
                grab: { x: here.x - at.x, y: here.y - at.y },
                origin: here,
                moved: false,
              };
            }}
          />
        ))}

        <div className="cast" aria-label="Agents in this room">
        {agentPlaces.map((agent) => {
        const presence = crew.find((member) => member.id === agent.id)?.presence ?? null;
        return (
          <article
            key={agent.id}
            className={`agent-blob ${selectedAgent === agent.id ? "picked" : ""} ${picked.has(`agent:${agent.id}`) ? "multi" : ""}`}
            style={{ left: agent.at.x, top: agent.at.y }}
            onPointerDown={(event) => {
              if (tool !== "select") return;
              const here = pointAt(event);
              if (armGroup(`agent:${agent.id}`, here)) return;
              dragging.current = {
                kind: "agent",
                id: agent.id,
                grab: { x: here.x - agent.at.x, y: here.y - agent.at.y },
                origin: here,
                moved: false,
              };
            }}
          >
            <button
              type="button"
              className="agent-open"
              onClick={() => {
                if (swallowClick.current) return;
                focusedBoundsRef.current = selectedAgent === agent.id ? null : itemRectsRef.current.find((item) => item.id === `agent:${agent.id}`) ?? null;
                onSelectAgent?.(selectedAgent === agent.id ? null : agent.id);
              }}
            >
              <span
                className="avatar"
                aria-hidden="true"
                style={{
                  background: `hsl(${hueOf(agent.id)} 52% 46%)`,
                  opacity: presenceOf(minutesSince(crew.find((member) => member.id === agent.id)?.lastAt)),
                }}
              >
                {initialOf(agent.id)}
              </span>
              <span className="agent-name" title={agent.id}>{agent.id}</span>
              {/* THE DECLARED CAPABILITY - never inferred from a route or a default. A session
                * that declared nothing produced no `agent_presence_declared` event at all, so
                * `presence` is null and this renders NOTHING: no "unknown", no placeholder. A
                * declaration with no effort renders the model alone, with no dangling separator. */}
              {presence &&
                (() => {
                  const declaration = presence.effort
                    ? `${presence.model} · ${presence.effort}`
                    : presence.model;
                  return (
                    // The TITLE carries the declaration itself, not only the explanation: the
                    // badge is clipped to the card's width (#1057, Codex P2), and a 128-character
                    // model name would otherwise be unreadable with no way to see the rest.
                    <span
                      className="agent-badge"
                      aria-hidden="true"
                      title={`${declaration} — declared by the session, never inferred`}
                    >
                      {declaration}
                    </span>
                  );
                })()}
              {/* Presentation only: the blob's accessible name stays the agent's id alone. */}
              <span className="agent-when" aria-hidden="true">
                Seen {ago(crew.find((member) => member.id === agent.id)?.lastAt)} ago
              </span>
              {crew.find((member) => member.id === agent.id)?.charter != null && (
                <span className="crew-role">persona</span>
              )}
            </button>
          </article>
        );
        })}
        </div>

        {talkPlaces.map(({ talk, at }) => (
          <article
            key={talk.key}
            className={`talk-bubble ${talk.key === "room" ? "room" : ""} ${selectedTalk === talk.key ? "picked" : ""} ${picked.has(`talk:${talk.key}`) ? "multi" : ""}`}
            style={{ left: at.x, top: at.y }}
            onPointerDown={(event) => {
              if (tool !== "select") return;
              const here = pointAt(event);
              if (armGroup(`talk:${talk.key}`, here)) return;
              dragging.current = {
                kind: "agent",
                id: `talk:${talk.key}`,
                grab: { x: here.x - at.x, y: here.y - at.y },
                origin: here,
                moved: false,
              };
            }}
          >
            <button
              type="button"
              className="talk-open"
              style={{
                borderColor: talk.count >= 8 ? "var(--faint)" : undefined,
                ...(selectedAgent !== null &&
                selectedTalk !== talk.key &&
                talk.participants.includes(selectedAgent)
                  ? { boxShadow: `0 0 0 1.5px hsl(${hueOf(selectedAgent)} 52% 60%), var(--lift)` }
                  : {}),
              }}
              onClick={() => {
                if (swallowClick.current) return;
                focusedBoundsRef.current = selectedTalk === talk.key ? null : itemRectsRef.current.find((item) => item.id === `talk:${talk.key}`) ?? null;
                onSelectTalk?.(selectedTalk === talk.key ? null : talk.key);
              }}
            >
              <span className="talk-line">
                <span className="talk-faces" aria-hidden="true">
                  {talk.participants.slice(0, 3).map((id) => (
                    <span
                      key={id}
                      className="avatar mini"
                      style={{ background: `hsl(${hueOf(id)} 52% 46%)` }}
                    >
                      {initialOf(id)}
                    </span>
                  ))}
                  {talk.participants.length > 3 && (
                    <span className="talk-more">+{talk.participants.length - 3}</span>
                  )}
                </span>
                <span className="talk-name">
                  <span className="talk-title">{talk.key === "room" ? "everyone" : talk.label}</span>
                  {talk.key === "room" && <span className="talk-kind">the whole room</span>}
                </span>
              </span>
              <span className="talk-preview">{talk.preview || "Message preview unavailable"}</span>
              <span className="talk-meta">
                <span>{talk.count} msg{talk.count === 1 ? "" : "s"}</span>
                <span>{ago(talk.lastAt)}</span>
              </span>
            </button>
          </article>
        ))}

        {model.nodes.length === 0 && <p className="sheet-empty">no work on this board yet</p>}

        {marquee !== null && (
          <div
            className="marquee"
            style={{
              left: Math.min(marquee.a.x, marquee.b.x),
              top: Math.min(marquee.a.y, marquee.b.y),
              width: Math.abs(marquee.b.x - marquee.a.x),
              height: Math.abs(marquee.b.y - marquee.a.y),
            }}
          />
        )}
        </div>
      </div>

      <div className="connection-legend" aria-label="Connection types">
        <span><i className="conversation-line" aria-hidden="true" />Conversation</span>
        {model.edgesKnown && <span><i className="dependency-line" aria-hidden="true" />Verified dependency</span>}
      </div>
      <div className="canvas-sections" role="group" aria-label="Explore canvas sections">
        <button type="button" disabled={crew.length === 0} onClick={() => { const item = itemRectsRef.current.find(item => item.id.startsWith("agent:")); if (item) fit(item); }}>People</button>
        <button type="button" disabled={talks.length === 0} onClick={() => { const item = itemRectsRef.current.find(item => item.id.startsWith("talk:")); if (item) fit(item); }}>Chats</button>
        <button type="button" disabled={model.nodes.length === 0} onClick={() => { const item = itemRectsRef.current.find(item => item.id.startsWith("node:")); if (item) fit(item); }}>Work</button>
        {!model.edgesKnown && <button type="button" aria-label="Verify work dependencies" onClick={() => setConnectOpen(true)}>Verify graph</button>}
      </div>
      <label className="board-navigator">
        <span className="sr-only">Find on board</span>
        <select value="" onChange={(event) => {
          const value = event.target.value;
          const item = itemRectsRef.current.find((candidate) => candidate.id === value);
          if (item) fit(item);
          if (value.startsWith("node:")) onSelectNode(value.slice(5));
          else if (value.startsWith("agent:")) onSelectAgent?.(value.slice(6));
          else if (value.startsWith("talk:")) onSelectTalk?.(value.slice(5));
        }}>
          <option value="" disabled>Find people, chats or nodes</option>
          <optgroup label="Nodes">
            {model.nodes.map((node) => <option key={node.id} value={`node:${node.id}`}>{node.id} · {readable(node.state)}</option>)}
          </optgroup>
          {crew.length > 0 && <optgroup label="Agents">
            {crew.map((agent) => <option key={agent.id} value={`agent:${agent.id}`}>{agent.id}</option>)}
          </optgroup>}
          {talks.length > 0 && <optgroup label="Conversations">
            {talks.map((talk) => <option key={talk.key} value={`talk:${talk.key}`}>{talk.key === "room" ? "Everyone" : talk.label}</option>)}
          </optgroup>}
        </select>
      </label>
      <p className={`edge-note ${connectionTone === "none" ? "" : connectionTone}`}>
        {model.rosterDeclared
          ? `${model.nodes.length} node${model.nodes.length === 1 ? "" : "s"}`
          : `${model.nodes.length} node${model.nodes.length === 1 ? "" : "s"} seen so far, roster not read`}
        {" · "}
        {model.edgesKnown
          ? `${model.edges.length} connection${model.edges.length === 1 ? "" : "s"} drawn`
          : "work connections unverified"}
        {" · "}
        {model.edgesKnown || connectionTone === "refused" ? connectionNote : "Verify the graph to connect work nodes."}
        {!showConnect && (
          <button type="button" className="connect-toggle" onClick={() => setConnectOpen(true)}>
            Verify graph
          </button>
        )}
      </p>

      {/* THE LINT STRIP: what the model itself can accuse, on the board's face. The reference
        * project lints a PLAN file; ours lints the LOG - each line names the disagreement and,
        * when an event is accused, cites its #sequence click-to-copy, same as the thread's. */}
      {model.lint.length > 0 && (
        // #1083: the overview's rule, on the canvas. Only a demonstration run's fixture-explained
        // findings read as neutral notes; any real disagreement (a reopened settled node, an
        // orphan edge) keeps the amber strip and is listed first (Codex on PR #1091).
        <details className={`canvas-lint ${lintSplit.disagreements.length === 0 ? "canvas-note" : ""}`}><summary>{lintSplit.disagreements.length === 0 ? `Demonstration run · ${lintSplit.expected.length} log note${lintSplit.expected.length === 1 ? "" : "s"}` : `${lintSplit.disagreements.length} log disagreement${lintSplit.disagreements.length === 1 ? "" : "s"}${lintSplit.expected.length > 0 ? ` · ${lintSplit.expected.length} demonstration note${lintSplit.expected.length === 1 ? "" : "s"}` : ""}`}</summary>
        <ul className="lint-strip" aria-label={lintSplit.disagreements.length === 0 ? "Log notes on a demonstration run" : "Disagreements the log attests"}>
          {[...lintSplit.disagreements, ...lintSplit.expected].map((finding, index) => (
            <li key={`${finding.kind}-${finding.sequence ?? index}`}>
              <span>{finding.detail}</span>
              {finding.sequence !== null && runId !== undefined && (
                <button
                  type="button"
                  className="turn-seq"
                  title={`Copy ${runId}#${finding.sequence}`}
                  onClick={() => void navigator.clipboard?.writeText(`${runId}#${finding.sequence}`)}
                >
                  #{finding.sequence}
                </button>
              )}
            </li>
          ))}
        </ul>
        </details>
      )}

      {showConnect && (
      <div className="graph-file">
        <span className="wrap">
          <FileCode2 aria-hidden="true" />
          <label>
            <span className="sr-only">Graph file path on the Runtime host</span>
            <input
              ref={fileRef}
              value={graphFile}
              onChange={(event) => onGraphFileChange(event.target.value)}
              placeholder="Graph file on the Runtime host…"
            />
          </label>
        </span>
        <button
          type="button"
          onClick={onDrawConnections}
          disabled={busy || graphFile.trim().length === 0}
          title="Read this file's shape and check it against the hash this run recorded"
        >
          <Waypoints aria-hidden="true" />
          connect
        </button>
        {demonstration && onFixtureFileChange && (
          <span className="wrap">
            <FileCode2 aria-hidden="true" />
            <label>
              <span className="sr-only">Fixture file path on the Runtime host, sent with resume</span>
              <input
                value={fixtureFile}
                onChange={(event) => onFixtureFileChange(event.target.value)}
                placeholder="Fixture file for resume (optional)…"
                title="A demonstration run's outcomes come from a fixture file. Name one here and resume sends it; leave it empty and the resumed node waits for input."
              />
            </label>
          </span>
        )}
      </div>
      )}

      <p className="canvas-hints" aria-hidden="true">
        <span>click · opens</span>
        <span>drag · moves</span>
        <span>space · pans</span>
        <span>ctrl+scroll · zooms</span>
      </p>

      <div className="tools" role="toolbar" aria-label="Board tools">
        <button
          type="button"
          className={tool === "select" ? "on" : ""}
          aria-pressed={tool === "select"}
          onClick={() => setTool("select")}
        >
          <MousePointer2 aria-hidden="true" />
          move
        </button>
        <button
          type="button"
          className={tool === "hand" ? "on" : ""}
          aria-pressed={tool === "hand"}
          title="Drag the board around (hold Space for the same)"
          onClick={() => setTool("hand")}
        >
          <Hand aria-hidden="true" />
          pan
        </button>
        <button
          type="button"
          className={tool === "pen" ? "on" : ""}
          aria-pressed={tool === "pen"}
          onClick={() => setTool("pen")}
        >
          <Highlighter aria-hidden="true" />
          draw
        </button>
        <button
          type="button"
          className={tool === "note" ? "on" : ""}
          aria-pressed={tool === "note"}
          onClick={() => setTool("note")}
        >
          <StickyNote aria-hidden="true" />
          note
        </button>
        <span className="tones">
          {TONES.map((option) => (
            <button
              type="button"
              key={option.value}
              className={`${option.value} ${tone === option.value ? "on" : ""}`}
              aria-pressed={tone === option.value}
              onClick={() => {
                setTone(option.value);
                setTool("pen");
              }}
            >
              <span className="sr-only">{option.label} pen</span>
              <i aria-hidden="true" />
            </button>
          ))}
        </span>
        <button
          type="button"
          onClick={undo}
          disabled={board.strokes.length === 0 && board.notes.length === 0}
        >
          <RotateCcw aria-hidden="true" />
          undo
        </button>
        <span className="zoomer">
          <button type="button" onClick={() => fit(undefined, true)} title="Frame the work nodes at a readable size">
            fit
          </button>
          <button
            type="button"
            disabled={selectedNode === null}
            onClick={() => {
              if (selectedNode === null) return;
              const index = model.nodes.findIndex((node) => node.id === selectedNode);
              if (index < 0) return;
              const at = nodePosition(selectedNode, index);
              fit({ x: at.x, y: at.y, w: CARD_WIDTH, h: nodeHeight(model.nodes[index]) });
            }}
            title="Frame the selected node"
          >
            fit selected
          </button>
          <button type="button" aria-label="Zoom out" onClick={() => zoomBy(0.8)}>
            <Minus aria-hidden="true" />
          </button>
          <button
            type="button"
            className="zoom-label"
            title="Back to 100%"
            onClick={() => setView({ x: 0, y: 0, zoom: 1 })}
          >
            {Math.round(view.zoom * 100)}%
          </button>
          <button type="button" aria-label="Zoom in" onClick={() => zoomBy(1.25)}>
            <Plus aria-hidden="true" />
          </button>
        </span>
      </div>
      </div>
    </>
  );
}

function NodeBlock({
  connections = null,
  node,
  at,
  entry,
  selected,
  onOpen,
  onGrab,
  multi = false,
  highlight = null,
  highlightAgent = null,
  objective = null,
}: {
  multi?: boolean;
  connections?: number | null;
  /** The run's objective, on the entry node only (#1077) - the words this node was given. */
  objective?: string | null;
  node: GraphNode;
  at: Point;
  entry: boolean;
  selected: boolean;
  onOpen: () => void;
  onGrab: (event: React.PointerEvent) => void;
  /** The picked agent's own colour: nodes this agent touched wear a thin ring of it. */
  highlight?: string | null;
  highlightAgent?: string | null;
}) {
  const mood = moodOf(node.state);
  const last = node.history.at(-1);
  const latestActor = last?.actorId ?? null;
  const [historyOpen, setHistoryOpen] = useState(false);
  // The actors whose hands touched this node, newest first. Owner and system stay off the card:
  // the question the chips answer is "which AGENT is on this", and lifecycle is not a contact.
  const crew: string[] = [];
  for (let index = node.history.length - 1; index >= 0 && crew.length < 3; index -= 1) {
    const entry = node.history[index];
    if (entry.actorId === null) continue;
    if (entry.actorType === "system" || entry.actorType === "owner") continue;
    if (!crew.includes(entry.actorId)) crew.push(entry.actorId);
  }
  /** The chip a history line wears: alarm states in the signal, motion and endings in mint,
   * the not-yet in faint - the same meanings the whole surface already speaks. */
  const chipToneOf = (state: string | null): string => {
    if (state === null) return "quiet";
    if (isAlarming(state)) return "alarm";
    if (state === "running" || state === "queued" || state === "linting") return "live pulse";
    if (state === "succeeded" || state === "waived") return "live";
    return "quiet";
  };
  return (
    <article
      className={`node ${node.touches === 0 && node.reopened === null ? "node-empty" : ""} ${objective !== null && objective.trim().length > 0 ? "node-with-objective" : ""} ${mood} ${selected ? "selected" : ""} ${multi ? "multi" : ""}`}
      style={{
        left: at.x,
        top: at.y,
        ...(highlight !== null && highlightAgent !== null && !selected && crew.includes(highlightAgent)
          ? { boxShadow: `0 0 0 1.5px ${highlight}, var(--lift)` }
          : {}),
      }}
      onPointerDown={onGrab}
    >
      <button type="button" className="node-open" onClick={onOpen}>
        {/* Eyebrow: the log's own address for the block, and - only while something moves or
            waits - the state word breathing on the right. */}
        <span className="node-eyebrow">
          <span className="node-tag"><GitBranch aria-hidden="true" />{node.declaredRole ? `${node.declaredRole} · ` : ""}{entry ? "Entry node" : "Work node"}</span>
          <span className={`hist-chip ${node.resultSource === "model_reply" && node.state === "succeeded" ? "alarm" : mood === "moving" ? "live pulse" : mood === "waiting" || mood === "dead" ? "alarm" : "quiet"}`} title={node.resultSource === "model_reply" && node.state === "succeeded" ? "Runtime state: succeeded; reply not verified" : undefined}>
            {node.resultSource === "model_reply" && node.state === "succeeded" ? "review needed" : node.state === "unknown" ? "Awaiting event" : readable(node.state)}
            {(mood === "moving" || mood === "waiting") && <i aria-hidden="true"> ✳</i>}
          </span>
        </span>
        <span className="node-title" title={node.declaredName ? `${node.declaredName} (${node.id})` : node.id}>{node.declaredName ?? node.id}</span>
        {/* Clamped: the card is a fixed box the camera frames by (cardHeight, graph/board.ts -
            taller by the objective's allowance, matched by .node-with-objective), and an
            objective may run to 2,000 characters. The whole text is one hover away on the title. */}
        {objective !== null && objective.trim().length > 0 && <q className="node-objective" title={objective}>{objective}</q>}
        {/* THE DISAGREEMENT CHIP - the reference project's best signal ("quietly reopened the
          * part it already called done"), derived here from the log alone: this node settled at
          * one sequence and a later event named it again. Both coordinates cited, author named. */}
        {node.reopened !== null && (
          <span
            className="hist-chip alarm node-reopened"
            title={`Settled at #${node.reopened.settledAt}, then named again at #${node.reopened.reopenedAt}${node.reopened.by !== null ? ` by ${node.reopened.by}` : ""}`}
          >
            reopened after done · #{node.reopened.settledAt}→#{node.reopened.reopenedAt}
          </span>
        )}
        <span className="node-story-status" title={nodeResult(node)?.verification}>
          {nodeResult(node)?.short ?? (node.touches === 0 ? "Awaiting first work update" : last?.outcome ? readable(last.outcome) : readable(node.state))}
        </span>
        {node.touches > 0 && <span className="node-facts">
          {node.actualExecutor ? <span><small>Executed by</small><strong>{nodeResult(node)?.executor ?? (node.actualExecutor.kind === "model" ? `Model · route ${node.actualExecutor.routeId ?? "not recorded"}` : node.actualExecutor.kind)}</strong></span> : null}
          <span><small>Recorded by</small><strong>{last?.actorType === "system" ? "Runtime" : latestActor ?? "Not reported"}</strong></span>
          {!node.actualExecutor && <span><small>Latest update</small><strong>{node.lastEventAt ? ago(node.lastEventAt) : "No updates yet"}</strong></span>}
        </span>}
        {node.touches > 0 && <span className="node-receipt">
          <span>{node.touches} event{node.touches === 1 ? "" : "s"}{last?.sequence !== undefined ? ` / #${last.sequence}` : ""}{node.actualExecutor && node.lastEventAt ? ` · ${ago(node.lastEventAt)}` : ""}</span>
          <span>{connections === null ? "Connections unverified" : `${connections} connection${connections === 1 ? "" : "s"}`}</span>
        </span>}
        <span className="node-inspect">Inspect node <ArrowUpRight aria-hidden="true" /></span>
      </button>

      {node.history.length > 0 && (
        <button
          type="button"
          className="node-history-toggle"
          aria-expanded={historyOpen}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            setHistoryOpen((open) => !open);
          }}
        >
          {historyOpen ? "hide history \u25b4" : "show history \u25be"}
        </button>
      )}
      {historyOpen && (
        <ol className="node-history">
          {[...node.history].reverse().map((line) => (
            <li key={line.sequence}>
              <span className={`hist-chip ${chipToneOf(line.nextState)}`}>
                {line.nextState !== null ? readable(line.nextState) : readable(line.kind)}
              </span>
              <span className="hist-what">{line.outcome !== null ? readable(line.outcome) : readable(line.kind)}</span>
              {/* Whose hand: the log's own actor, worn as the same face the rest of the surface
                * uses. System stays bare - lifecycle narration is not a contact. */}
              {line.actorId !== null && line.actorType !== "system" && (
                <span
                  className="avatar mini"
                  title={line.actorId}
                  aria-hidden="true"
                  style={{ background: `hsl(${hueOf(line.actorId)} 52% 46%)` }}
                >
                  {initialOf(line.actorId)}
                </span>
              )}
              <span className="hist-when">{ago(line.occurredAt)}</span>
            </li>
          ))}
        </ol>
      )}
    </article>
  );
}



