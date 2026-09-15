/**
 * The board's own state: where each node card sits, and what the operator has drawn on top.
 *
 * TWO KINDS OF STATE, KEPT APART ON PURPOSE. The nodes and their states belong to the Runtime and
 * are re-read from it. Positions, pen strokes and notes belong to the PERSON, exist nowhere but
 * this browser, and are never sent anywhere. Mixing them would make a drag look like a mutation.
 *
 * WHY THIS PERSISTS WHEN THE TOKEN DOES NOT. The token is a credential and its rule is absolute.
 * A board layout is the operator's own notes about their own screen: losing it on every reload
 * makes annotation pointless, and it carries no secret and no execution data beyond node ids the
 * holder of this browser could already read. Stored per execution under one namespaced key,
 * documented here so the exception is a decision rather than a drift. Nothing else in this
 * application writes to storage.
 */

const STORAGE_PREFIX = "graphhelm.studio.board.";
/** A bound on what one board may hold, so a runaway pen cannot fill the origin's quota. */
const MAX_STROKES = 400;
const MAX_NOTES = 60;

export interface Point {
  x: number;
  y: number;
}

export interface BoardBounds {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * The node card's own size, needed to anchor an edge to its sides rather than its corner and to
 * frame the camera by a box the card really fills. Kept beside the CSS that sets it
 * (`.free-canvas-content .node*` in styles.css); a drift here misses by a few pixels rather than
 * breaking anything - except overflow, which is why the objective allowance is measured too.
 */
export const CARD_WIDTH = 320;
/** A card with history: chips, crew, the last line. */
export const CARD_HEIGHT = 288;
/** A card nothing has touched yet: eyebrow, title, the waiting line. */
export const EMPTY_CARD_HEIGHT = 164;
/** The room `.node-objective` takes on the entry node's card: two clamped lines of 12px/1.45
 * (34.8px) plus its 4px + 10px margins, rounded up so the box is never the shorter one. */
export const OBJECTIVE_ALLOWANCE = 52;

/** The height the card's CSS box is given, so bounds and edges measure what is drawn. A card
 * carrying the run's objective (#1077: the first entry node) is taller by the objective's
 * allowance; without it the entry card of a draft-started run overflowed its 164px box. */
export function cardHeight(node: { touches: number; reopened: unknown | null }, withObjective: boolean): number {
  const base = node.touches === 0 && node.reopened === null ? EMPTY_CARD_HEIGHT : CARD_HEIGHT;
  return withObjective ? base + OBJECTIVE_ALLOWANCE : base;
}

/** The gap the default grid leaves between two rows of cards. */
export const GRID_ROW_GAP = 32;

/**
 * The default grid's row step: the tallest card the layout can hold, plus the gap (#1077).
 *
 * The step was `CARD_HEIGHT + 32 = 320` while a first-entry card carrying the objective is
 * `CARD_HEIGHT + OBJECTIVE_ALLOWANCE = 340`, so the row below overlapped the entry card by 20px and
 * intercepted its pointer band. Derived from the same constants the card is drawn with, so a
 * taller card moves the rows with it.
 */
export function gridRowStep(withObjective: boolean): number {
  return (withObjective ? CARD_HEIGHT + OBJECTIVE_ALLOWANCE : CARD_HEIGHT) + GRID_ROW_GAP;
}

/** Where a card sits on the width-following default grid (board.tsx) before anyone moves it:
 * `columns` across, then the next row, each row one `gridRowStep` down. */
export function gridPosition(index: number, columns: number, withObjective: boolean): Point {
  return {
    x: 680 + (index % columns) * 380,
    y: 100 + Math.floor(index / columns) * gridRowStep(withObjective),
  };
}

export interface Camera {
  x: number;
  y: number;
  zoom: number;
}

/** Return a camera that centers a board-space rectangle in a viewport. */
export function fitCamera(
  bounds: BoardBounds,
  viewport: { w: number; h: number },
  padding = 48,
  maxZoom = 2.5,
): Camera {
  const width = Math.max(1, bounds.w);
  const height = Math.max(1, bounds.h);
  const availableWidth = Math.max(1, viewport.w - padding * 2);
  const availableHeight = Math.max(1, viewport.h - padding * 2);
  // Whole-map fitting must include every item, even below the manual zoom floor.
  // Automatic framing applies its separate readability floor in the component.
  const zoom = Math.min(maxZoom, availableWidth / width, availableHeight / height);
  return {
    x: viewport.w / 2 - (bounds.x + width / 2) * zoom,
    y: viewport.h / 2 - (bounds.y + height / 2) * zoom,
    zoom,
  };
}

/** The smallest zoom automatic framing will choose (#1083 F8): a card's 12px body text reads at
 * 9px here. Below it the frame is a thumbnail sheet, which is what `fit` produced at 15%. */
export const READABLE_ZOOM = 0.75;

/** The floor for a band too short to hold the first rank whole at `READABLE_ZOOM` (#1083 F8): a
 * whole card at 60% beats a card whose lower third sits under the toolbar at 75%. */
export const MIN_FRAME_ZOOM = 0.6;

/** A screen rectangle, as `getBoundingClientRect` reports it. */
export interface ScreenRect {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

/**
 * How much of the sheet's top and bottom the floating chrome covers (#1083 F8).
 *
 * A piece counts when it overlaps the sheet and is SHORTER than the sheet: it covers the top when
 * its middle is above the sheet's middle, the bottom otherwise. A piece as tall as the sheet (or
 * taller) is an overlay no band can frame around and is ignored; so is an unlaid-out (0-size) one.
 * Chrome that would leave no band at all is ignored too, rather than framing into nothing.
 */
export function chromeInsets(sheet: ScreenRect, chrome: ReadonlyArray<ScreenRect>): { top: number; bottom: number } {
  const height = sheet.bottom - sheet.top;
  const middle = (sheet.top + sheet.bottom) / 2;
  let top = 0;
  let bottom = 0;
  for (const piece of chrome) {
    const pieceHeight = piece.bottom - piece.top;
    if (pieceHeight <= 0 || piece.right - piece.left <= 0 || pieceHeight >= height) continue;
    if (piece.right <= sheet.left || piece.left >= sheet.right) continue;
    if (piece.bottom <= sheet.top || piece.top >= sheet.bottom) continue;
    if ((piece.top + piece.bottom) / 2 < middle) top = Math.max(top, piece.bottom - sheet.top);
    else bottom = Math.max(bottom, sheet.bottom - piece.top);
  }
  if (top + bottom >= height) return { top: 0, bottom: 0 };
  return { top, bottom };
}

/**
 * Frame the work cards (#1083 F8): the whole set when it fits at a readable zoom, otherwise the
 * first ranks at `READABLE_ZOOM`, anchored at the cards' top (and left, when the columns are
 * wider than the viewport) so panning reveals the rest.
 *
 * `bounds` must be the CARDS' bounds - not lane chrome and not the dock. Framing the People and
 * Conversations lanes as content is what left a six-card run showing two cards at 60%.
 */
export function frameCards(
  bounds: BoardBounds,
  viewport: { w: number; h: number },
  padding = 24,
  firstRank: number = bounds.h,
): Camera {
  const whole = fitCamera(bounds, viewport, padding, 1);
  if (whole.zoom >= READABLE_ZOOM) return whole;
  // A band too short to hold the first rank whole at the readable zoom (the toolbar and header
  // chrome ate it) goes down toward `MIN_FRAME_ZOOM` so no framed card is cut by the chrome.
  const room = Math.max(1, viewport.h - padding * 2);
  const zoom = Math.max(MIN_FRAME_ZOOM, Math.min(READABLE_ZOOM, room / Math.max(1, firstRank)));
  const fitsAcross = bounds.w * zoom <= Math.max(1, viewport.w - padding * 2);
  const fitsDown = bounds.h * zoom <= Math.max(1, viewport.h - padding * 2);
  return {
    x: fitsAcross ? viewport.w / 2 - (bounds.x + bounds.w / 2) * zoom : padding - bounds.x * zoom,
    y: fitsDown ? viewport.h / 2 - (bounds.y + bounds.h / 2) * zoom : padding - bounds.y * zoom,
    zoom,
  };
}

export interface Stroke {
  id: string;
  /** The pen's colour, chosen from the board's own small set - never free-form input. */
  tone: "ink" | "flag" | "calm";
  points: Point[];
}

export interface Note {
  id: string;
  at: Point;
  text: string;
}

export interface BoardState {
  positions: Record<string, Point>;
  /* Where each agent blob stands on the canvas. The same contract as node positions: the
     operator's own arrangement, never sent anywhere, never a fact about the run. */
  agents: Record<string, Point>;
  strokes: Stroke[];
  notes: Note[];
  /* The graph file the operator pointed this run's board at, so re-opening the run re-verifies
     without re-typing a Runtime-host path. The operator's own note, same contract as the marks:
     browser-only, and only ever sent to the verify call they already chose to make. The
     VERDICT is untouched - verifyTopology still refuses to draw an unproven edge. */
  graphFile: string;
}

export function emptyBoard(): BoardState {
  return { positions: {}, agents: {}, strokes: [], notes: [], graphFile: "" };
}

/**
 * Where a node card sits before anyone moves it.
 *
 * A deterministic grid, ordered by the model's own node order, so the same execution opens to the
 * same board every time. Not a force-directed layout: with no edges to pull against, a physics
 * simulation would produce a different arrangement on every load and imply relationships that
 * were never measured.
 */
export function defaultPosition(index: number): Point {
  // Wider than the block (254px) by a clear margin, so an edge has somewhere to BE. At the old
  // spacing two blocks in a row were six pixels apart and the connection between them had no
  // room to read as a connection.
  const ROWS = 3;
  const COLUMN_WIDTH = 380;
  const ROW_HEIGHT = 320;
  const column = Math.floor(index / ROWS);
  const row = index % ROWS;
  return { x: 680 + column * COLUMN_WIDTH, y: 100 + row * ROW_HEIGHT };
}

export function positionOf(board: BoardState, nodeId: string, index: number): Point {
  return board.positions[nodeId] ?? defaultPosition(index);
}

/** Where an agent stands before anyone moves it. `index` is a rank in the left lane. */
export function defaultAgentPosition(index: number): Point {
  return { x: 100, y: 140 + index * 140 };
}

export function agentPositionOf(board: BoardState, agentId: string, index: number): Point {
  return board.agents[agentId] ?? defaultAgentPosition(index);
}

/**
 * Puts every block back on the default grid, and keeps everything a person drew.
 *
 * WHY THIS IS A BUTTON AND NOT AN AUTOMATIC REPAIR. Positions are stored per run and survive a
 * reload, which is the point of them - a board someone arranged is theirs. But stored positions
 * outlive the layout that produced them, and a graph that gains a node lands it on top of one
 * that was already placed. Both leave blocks buried under each other with no way out, because
 * dragging the top one is the only way to learn the bottom one is there.
 *
 * Dropping the overrides rather than nudging them: `positionOf` already falls back to
 * `defaultPosition`, so the grid is the one arrangement in the code and tidy cannot drift from
 * it. Strokes and notes are untouched - they are not positions, and an operator's annotation is
 * not a layout mistake to clean up.
 */
export function tidyBoard(board: BoardState): BoardState {
  return { ...board, positions: {}, agents: {} };
}

function storageKey(executionId: string): string {
  return `${STORAGE_PREFIX}${executionId}`;
}

/**
 * Reads one execution's board back.
 *
 * Every field is re-validated rather than trusted: this comes out of storage, which a page on the
 * same origin can write, and a malformed entry must degrade to an empty board rather than throw
 * on render. `try`/`catch` because a private window can make the accessor itself throw.
 */
export function loadBoard(executionId: string): BoardState {
  try {
    const raw = globalThis.localStorage?.getItem(storageKey(executionId));
    if (!raw) return emptyBoard();
    const parsed: unknown = JSON.parse(raw);
    if (parsed === null || typeof parsed !== "object") return emptyBoard();
    const value = parsed as Partial<BoardState>;
    return {
      positions: sanitisePositions(value.positions),
      agents: sanitisePositions(value.agents),
      strokes: sanitiseStrokes(value.strokes),
      notes: sanitiseNotes(value.notes),
      // Re-sent to the verify endpoint, so a non-string or an absurd length degrades to
      // "no path remembered" rather than reaching a request.
      graphFile:
        typeof value.graphFile === "string" && value.graphFile.length <= 1024
          ? value.graphFile
          : "",
    };
  } catch {
    return emptyBoard();
  }
}

export function saveBoard(executionId: string, board: BoardState): void {
  try {
    globalThis.localStorage?.setItem(storageKey(executionId), JSON.stringify(board));
  } catch {
    // A full or blocked quota must not break the page. The board stays live in memory; only its
    // persistence is lost, and that is the right thing to lose.
  }
}

/** Forgets every board this Studio has stored. Called on disconnect, so "disconnect" means the
 * screen is clean, not merely logged out. */
export function clearBoards(): void {
  try {
    const storage = globalThis.localStorage;
    if (!storage) return;
    const doomed: string[] = [];
    for (let index = 0; index < storage.length; index += 1) {
      const key = storage.key(index);
      if (key !== null && key.startsWith(STORAGE_PREFIX)) doomed.push(key);
    }
    for (const key of doomed) storage.removeItem(key);
  } catch {
    // Same reason as above: storage that refuses to answer is not a reason to fail a disconnect.
  }
}

function finitePoint(value: unknown): Point | null {
  if (value === null || typeof value !== "object") return null;
  const { x, y } = value as { x?: unknown; y?: unknown };
  if (typeof x !== "number" || typeof y !== "number") return null;
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x, y };
}

function sanitisePositions(value: unknown): Record<string, Point> {
  if (value === null || typeof value !== "object") return {};
  const out: Record<string, Point> = {};
  for (const [key, raw] of Object.entries(value as Record<string, unknown>)) {
    if (key.length === 0 || key.length > 128) continue;
    const point = finitePoint(raw);
    if (point) out[key] = point;
  }
  return out;
}

function sanitiseStrokes(value: unknown): Stroke[] {
  if (!Array.isArray(value)) return [];
  const out: Stroke[] = [];
  for (const raw of value.slice(0, MAX_STROKES)) {
    if (raw === null || typeof raw !== "object") continue;
    const { id, tone, points } = raw as { id?: unknown; tone?: unknown; points?: unknown };
    if (typeof id !== "string" || !Array.isArray(points)) continue;
    const cleaned = points.map(finitePoint).filter((point): point is Point => point !== null);
    if (cleaned.length < 2) continue;
    out.push({
      id,
      // A closed vocabulary, not the stored string: a tone read back from storage becomes a CSS
      // class, and an unrecognised one falls back rather than reaching the DOM.
      tone: tone === "flag" ? "flag" : tone === "calm" ? "calm" : "ink",
      points: cleaned.slice(0, 1000),
    });
  }
  return out;
}

function sanitiseNotes(value: unknown): Note[] {
  if (!Array.isArray(value)) return [];
  const out: Note[] = [];
  for (const raw of value.slice(0, MAX_NOTES)) {
    if (raw === null || typeof raw !== "object") continue;
    const { id, at, text } = raw as { id?: unknown; at?: unknown; text?: unknown };
    const point = finitePoint(at);
    if (typeof id !== "string" || point === null || typeof text !== "string") continue;
    out.push({ id, at: point, text: text.slice(0, 400) });
  }
  return out;
}

/** A board-local id. Not a security value: it distinguishes one stroke from another. */
export function markId(): string {
  const uuid = globalThis.crypto?.randomUUID?.();
  if (uuid) return uuid;
  const bytes = new Uint8Array(8);
  globalThis.crypto?.getRandomValues?.(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}



