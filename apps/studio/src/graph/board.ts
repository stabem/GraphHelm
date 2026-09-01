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
  const COLUMNS = 2;
  const COLUMN_WIDTH = 310;
  const ROW_HEIGHT = 180;
  const column = index % COLUMNS;
  const row = Math.floor(index / COLUMNS);
  // The END of the funnel, which empties to the RIGHT: the work stands past the web, where
  // the owner drew it.
  return { x: 1560 + column * COLUMN_WIDTH, y: 180 + row * ROW_HEIGHT };
}

export function positionOf(board: BoardState, nodeId: string, index: number): Point {
  return board.positions[nodeId] ?? defaultPosition(index);
}

/** Where an agent stands before anyone moves it. `index` is a RANK, not a roster position: the
 * Board ranks the crew by conversation traffic and the busiest agent takes the centre slot; the
 * rest alternate outward - right, left, further right - on two staggered rows. Between two
 * staggered agents there is always clear air, which is exactly where their conversation's
 * bubble will stand: the layout itself draws who talks to whom. */
export function defaultAgentPosition(index: number): Point {
  // Two rows far apart, hub at the bottom centre, the rest fanning out - modelled on the
  // arrangement the owner drew by hand (2026-08-30). The distances are the point: with this
  // much air between agents, every conversation's midpoint is clear ground, so bubbles land
  // where the relationship IS instead of being shoved into a collision stack.
  // The quiet agents take the EDGES and the centre of the top row stays open - that gap is
  // where a top-row pair's own bubble sits inline, exactly as the owner drew it.
  const SLOTS: Point[] = [
    { x: 1040, y: 560 },
    { x: 480, y: 150 },
    { x: 1060, y: 150 },
    { x: 380, y: 560 },
    { x: 180, y: 150 },
    { x: 1460, y: 560 },
    { x: 1480, y: 150 },
    { x: 1900, y: 560 },
  ];
  return SLOTS[index] ?? { x: 220 + (index - SLOTS.length) * 280, y: 920 };
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



