import { beforeEach, describe, expect, it } from "vitest";

import {
  clearBoards,
  defaultPosition,
  emptyBoard,
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
  });

  it("prefers a moved position over the default", () => {
    const board = { ...emptyBoard(), positions: { a: { x: 500, y: 600 } } };
    expect(positionOf(board, "a", 0)).toEqual({ x: 500, y: 600 });
    expect(positionOf(board, "b", 0)).toEqual(defaultPosition(0));
  });
});
