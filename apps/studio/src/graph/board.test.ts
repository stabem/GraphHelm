import { beforeEach, describe, expect, it } from "vitest";
import { MIN_FRAME_ZOOM, READABLE_ZOOM, chromeInsets, frameCards } from "./board";

import {
  CARD_HEIGHT,
  EMPTY_CARD_HEIGHT,
  OBJECTIVE_ALLOWANCE,
  cardHeight,
  clearBoards,
  defaultAgentPosition,
  defaultPosition,
  emptyBoard,
  fitCamera,
  gridPosition,
  gridRowStep,
  loadBoard,
  positionOf,
  saveBoard,
  type BoardState,
} from "./board";

const KEY = "graphhelm.studio.board.demo";

beforeEach(() => {
  localStorage.clear();
});

describe("board persistence", () => {
  it("round-trips positions, strokes and notes for one run", () => {
    const board: BoardState = {
      positions: { implementation: { x: 12, y: 34 } },
      agents: { "claude-code": { x: 90, y: 200 } },
      strokes: [{ id: "s1", tone: "flag", points: [{ x: 0, y: 0 }, { x: 5, y: 5 }] }],
      notes: [{ id: "n1", at: { x: 8, y: 9 }, text: "check this" }],
      graphFile: "flows/demo.json",
    };
    saveBoard("demo", board);
    expect(loadBoard("demo")).toEqual(board);
  });

  it("keeps one run's marks off another run's board", () => {
    saveBoard("demo", { ...emptyBoard(), positions: { a: { x: 1, y: 2 } } });
    expect(loadBoard("other")).toEqual(emptyBoard());
  });

  /** THE TOKEN RULE IS UNTOUCHED. This is the one thing this application writes to storage, and
   * a guard rather than a comment: nothing else may quietly start persisting alongside it. */
  it("writes under one namespaced key and nothing else", () => {
    saveBoard("demo", { ...emptyBoard(), notes: [{ id: "n", at: { x: 1, y: 1 }, text: "hi" }] });
    expect(Object.keys(localStorage)).toEqual([KEY]);
  });

  it("erases every board on disconnect, and leaves other origins' keys alone", () => {
    localStorage.setItem("unrelated.key", "keep me");
    saveBoard("demo", { ...emptyBoard(), positions: { a: { x: 1, y: 2 } } });
    saveBoard("other", { ...emptyBoard(), positions: { b: { x: 3, y: 4 } } });

    clearBoards();

    expect(loadBoard("demo")).toEqual(emptyBoard());
    expect(loadBoard("other")).toEqual(emptyBoard());
    expect(localStorage.getItem("unrelated.key")).toBe("keep me");
  });
});

describe("reading storage back is not trusting it", () => {
  it("returns an empty board rather than throwing on unparsable content", () => {
    localStorage.setItem(KEY, "{not json");
    expect(loadBoard("demo")).toEqual(emptyBoard());
  });

  /** The remembered path is re-sent to the verify endpoint, so a stored non-string or an absurdly
   * long value must degrade to "no path remembered", never reach a request. */
  it("drops a graph file that is not a sane string", () => {
    localStorage.setItem(KEY, JSON.stringify({ graphFile: 42 }));
    expect(loadBoard("demo").graphFile).toBe("");
    localStorage.setItem(KEY, JSON.stringify({ graphFile: "x".repeat(2000) }));
    expect(loadBoard("demo").graphFile).toBe("");
  });

  it("drops a position that is not two finite numbers", () => {
    localStorage.setItem(
      KEY,
      JSON.stringify({ positions: { a: { x: 1, y: "2" }, b: { x: Infinity, y: 0 }, c: { x: 3, y: 4 } } }),
    );
    expect(loadBoard("demo").positions).toEqual({ c: { x: 3, y: 4 } });
  });

  /** A tone read back from storage becomes a CSS class. An unrecognised one must fall back to a
   * known value rather than reach the DOM as whatever was written. */
  it("forces an unknown pen tone back into the closed set", () => {
    localStorage.setItem(
      KEY,
      JSON.stringify({
        strokes: [{ id: "s", tone: "url(javascript:1)", points: [{ x: 0, y: 0 }, { x: 1, y: 1 }] }],
      }),
    );
    expect(loadBoard("demo").strokes[0].tone).toBe("ink");
  });

  it("drops a stroke too short to be a stroke", () => {
    localStorage.setItem(KEY, JSON.stringify({ strokes: [{ id: "s", tone: "ink", points: [{ x: 0, y: 0 }] }] }));
    expect(loadBoard("demo").strokes).toEqual([]);
  });

  it("bounds what one board may hold", () => {
    const strokes = Array.from({ length: 900 }, (_, index) => ({
      id: `s${index}`,
      tone: "ink",
      points: [{ x: 0, y: 0 }, { x: 1, y: 1 }],
    }));
    localStorage.setItem(KEY, JSON.stringify({ strokes }));
    expect(loadBoard("demo").strokes.length).toBeLessThanOrEqual(400);
  });
});

describe("where a card sits before anyone moves it", () => {
  /** Deterministic, so the same run opens to the same board. A layout that moved between reads
   * would make the operator's own annotations point at the wrong card. */
  it("gives the same node the same default place every time", () => {
    expect(defaultPosition(4)).toEqual(defaultPosition(4));
    expect(defaultPosition(0)).not.toEqual(defaultPosition(1));
    expect(defaultPosition(0).x).toBeLessThan(1000);
  });

  it("prefers a moved position over the default", () => {
    const board = { ...emptyBoard(), positions: { a: { x: 500, y: 600 } } };
    expect(positionOf(board, "a", 0)).toEqual({ x: 500, y: 600 });
    expect(positionOf(board, "b", 0)).toEqual(defaultPosition(0));
  });

  it("places nodes in three rows before starting the next column", () => {
    expect(defaultPosition(0)).toEqual({ x: 680, y: 100 });
    expect(defaultPosition(2)).toEqual({ x: 680, y: 740 });
    expect(defaultPosition(3)).toEqual({ x: 1060, y: 100 });
  });

  it("keeps agents in one vertical lane", () => {
    expect(defaultAgentPosition(0)).toEqual({ x: 100, y: 140 });
    expect(defaultAgentPosition(3)).toEqual({ x: 100, y: 560 });
  });
});

/** #1079 review: the entry card gained the objective (two clamped lines, ~49px with margins)
 * but its measured box stayed 164px, so a draft-started run's card overflowed it. The bounds
 * the camera and edges use must be as tall as the box the CSS draws (.node-with-objective). */
describe("the default grid's row step", () => {
  /** #1077: the step was 320 while an entry card with an objective is 340, so the row below
   * overlapped it by 20px and took its pointer band. The step follows the tallest card. */
  it("is as tall as the tallest card it can hold, plus the gap", () => {
    expect(gridRowStep(false)).toBe(CARD_HEIGHT + 32);
    expect(gridRowStep(true)).toBe(CARD_HEIGHT + OBJECTIVE_ALLOWANCE + 32);
  });

  it("never lets the card below overlap a first-entry card with an objective at default positions", () => {
    const entry = { touches: 3, reopened: null };
    const columns = 2;
    const top = gridPosition(0, columns, true);
    const below = gridPosition(columns, columns, true);
    expect(below.x).toBe(top.x);
    expect(below.y).toBeGreaterThanOrEqual(top.y + cardHeight(entry, true));
    expect(below.y - (top.y + cardHeight(entry, true))).toBe(32);
  });

  it("keeps the tighter step for a run without an objective", () => {
    const columns = 3;
    expect(gridPosition(columns, columns, false).y - gridPosition(0, columns, false).y).toBe(CARD_HEIGHT + 32);
    expect(gridPosition(1, columns, false)).toEqual({ x: 680 + 380, y: 100 });
  });
});

describe("the card's measured height", () => {
  const untouched = { touches: 0, reopened: null };
  const touched = { touches: 3, reopened: null };
  it("is the short box for an untouched card without an objective", () => {
    expect(cardHeight(untouched, false)).toBe(EMPTY_CARD_HEIGHT);
    expect(cardHeight(touched, false)).toBe(CARD_HEIGHT);
  });
  it("is taller by the objective's allowance when the card carries one", () => {
    expect(cardHeight(untouched, true)).toBe(EMPTY_CARD_HEIGHT + OBJECTIVE_ALLOWANCE);
    expect(cardHeight(touched, true)).toBe(CARD_HEIGHT + OBJECTIVE_ALLOWANCE);
    // Two clamped lines of 12px/1.45 plus 4px + 10px margins is 48.8px; the box never undercuts it.
    expect(OBJECTIVE_ALLOWANCE).toBeGreaterThanOrEqual(Math.ceil(12 * 1.45 * 2 + 14));
  });
  it("counts a reopened card as a full card even with no touches", () => {
    expect(cardHeight({ touches: 0, reopened: { settledAt: 1, reopenedAt: 2, by: null } }, false)).toBe(CARD_HEIGHT);
  });
});

/** #1083 F8: the chrome that floats over the sheet, and the zoom floor a short band gets. */
describe("framing inside the chrome's band", () => {
  const SHEET = { top: 100, bottom: 500, left: 0, right: 700 };

  it("counts a top strip and a bottom toolbar, and ignores what does not cover the sheet", () => {
    expect(
      chromeInsets(SHEET, [
        { top: 100, bottom: 130, left: 16, right: 260 }, // lint strip over the top
        { top: 440, bottom: 492, left: 12, right: 640 }, // toolbar over the bottom
        { top: 20, bottom: 60, left: 0, right: 700 }, // header above the sheet: not over it
        { top: 520, bottom: 560, left: 0, right: 700 }, // dock below the sheet: not over it
        { top: 200, bottom: 240, left: 720, right: 900 }, // beside the sheet
        { top: 0, bottom: 0, left: 0, right: 0 }, // not laid out
      ]),
    ).toEqual({ top: 30, bottom: 60 });
  });

  it("ignores an overlay as tall as the sheet, and chrome that would leave no band", () => {
    expect(chromeInsets(SHEET, [{ top: 90, bottom: 510, left: 0, right: 700 }])).toEqual({ top: 0, bottom: 0 });
    expect(
      chromeInsets(SHEET, [
        { top: 100, bottom: 290, left: 0, right: 700 },
        { top: 280, bottom: 500, left: 0, right: 700 },
      ]),
    ).toEqual({ top: 0, bottom: 0 });
  });

  it("keeps the readable zoom when the first rank fits, and goes no lower than the floor to fit it whole", () => {
    const bounds = { x: 680, y: 100, w: 700, h: 900 };
    expect(frameCards(bounds, { w: 700, h: 400 }, 24, 288).zoom).toBe(READABLE_ZOOM);
    // A 260px band holds 212px of card: 288px at 0.736 fits, so the rank is framed whole.
    const tight = frameCards(bounds, { w: 700, h: 260 }, 24, 288);
    expect(tight.zoom).toBeCloseTo(212 / 288, 5);
    expect(288 * tight.zoom).toBeLessThanOrEqual(212 + 1e-9);
    // Anchored at the cards' top, so the first rank starts at the padding.
    expect(tight.y + bounds.y * tight.zoom).toBeCloseTo(24, 5);
    // A band that cannot hold it even at the floor stops at the floor.
    expect(frameCards(bounds, { w: 700, h: 120 }, 24, 288).zoom).toBe(MIN_FRAME_ZOOM);
  });
});

describe("camera framing", () => {
  it("fits content inside the viewport with bounded zoom", () => {
    expect(fitCamera({ x: 100, y: 50, w: 600, h: 400 }, { w: 1000, h: 800 }, 40)).toEqual({
      x: -113.33333333333337,
      y: 16.66666666666663,
      zoom: 1.5333333333333334,
    });
    const bounds = { x: -200, y: 100, w: 4000, h: 3000 };
    const camera = fitCamera(bounds, { w: 1000, h: 800 }, 40);
    expect(camera.zoom).toBeLessThan(0.3);
    expect(camera.x + bounds.x * camera.zoom).toBeGreaterThanOrEqual(40);
    expect(camera.y + bounds.y * camera.zoom).toBeGreaterThanOrEqual(40);
    expect(camera.x + (bounds.x + bounds.w) * camera.zoom).toBeLessThanOrEqual(960);
    expect(camera.y + (bounds.y + bounds.h) * camera.zoom).toBeLessThanOrEqual(760);
  });

  it("accepts a tighter maximum zoom for dense layouts", () => {
    expect(fitCamera({ x: 0, y: 0, w: 100, h: 100 }, { w: 1000, h: 800 }, 40, 1).zoom).toBe(1);
  });
});
