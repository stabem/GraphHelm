/**
 * The board's pointer behaviour, guarded because reading it was not enough to see the defect.
 *
 * THE BUG THESE EXIST FOR. The drag was held in a `ref`, and the effect that installed the window
 * `pointermove`/`pointerup` listeners named that ref in its condition. A ref does not re-render,
 * so beginning a drag installed nothing — and `pointerup` in particular was never wired, leaving
 * the hold armed after the button came up. The next press anywhere then moved the card that had
 * been clicked earlier, and the pen could not draw because its stroke was read as that stale hold.
 *
 * Found by driving a real browser, not by review. `a press after a release does not move the card
 * that was pressed before it` is the test that fails on the old shape.
 */

import { describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { Board } from "./board";
import { defaultPosition, emptyBoard, type BoardState } from "../graph/board";
import type { GraphModel } from "../graph/model";

const MODEL: GraphModel = {
  rosterDeclared: true,
  edgesKnown: false,
  edges: [],
  entrypoints: [],
  lint: [],
  nodes: [
    {
      id: "implementation",
      state: "blocked",
      touches: 3,
      lastEventAt: "2026-08-27T12:01:00Z",
      reopened: null,
      history: [
        {
          sequence: 7,
          kind: "node_outcome_recorded",
          nextState: "blocked",
          outcome: "retryable_failure",
          occurredAt: "2026-08-27T12:01:00Z",
          actorId: "system-cli",
          actorType: "system",
          evidence: 0,
        },
      ],
    },
    { id: "deploy", state: "ready", touches: 1, lastEventAt: null, history: [], reopened: null },
  ],
};

/**
 * The run capsule: the one card about the WHOLE run, with the only progress bar the log can
 * honestly back — nodes that need nothing more (succeeded, waived, skipped) over nodes declared.
 * Per-node progress does not exist in the log and is never invented.
 */
describe("the run capsule", () => {
  const PROGRESS_MODEL: GraphModel = {
    rosterDeclared: true,
    edgesKnown: false,
    edges: [],
    entrypoints: [],
    lint: [],
    nodes: [
      { id: "a", state: "succeeded", touches: 2, lastEventAt: null, history: [], reopened: null },
      { id: "b", state: "skipped", touches: 1, lastEventAt: null, history: [], reopened: null },
      { id: "c", state: "running", touches: 1, lastEventAt: "2026-08-27T12:03:00Z", history: [], reopened: null },
    ],
  };

  it("says how many nodes are done, out of how many the run declared", () => {
    render(
      <Board
        model={PROGRESS_MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        runId="demo-deploy"
        {...REST}
      />,
    );
    const capsule = screen.getByLabelText("This run's progress");
    expect(capsule).toHaveTextContent("demo-deploy");
    expect(capsule).toHaveTextContent("2 of 3 nodes done");
    const bar = capsule.querySelector(".run-progress i") as HTMLElement;
    expect(bar.style.width).toBe("67%");
  });

  it("carries the run's last activity, straight off the log's newest instant", () => {
    render(
      <Board
        model={PROGRESS_MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        runId="demo-deploy"
        {...REST}
      />,
    );
    expect(screen.getByLabelText("This run's progress")).toHaveTextContent(/last activity ·/);
  });

  it("stays off the canvas when no run is named", () => {
    render(
      <Board
        model={PROGRESS_MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        {...REST}
      />,
    );
    expect(screen.queryByLabelText("This run's progress")).not.toBeInTheDocument();
  });
});

describe("node evidence", () => {
  it("shows the latest observed actor and event address", () => {
    render(<Board model={{ ...MODEL, nodes: [...MODEL.nodes, { id: "draft", state: "unknown", touches: 0, lastEventAt: null, history: [], reopened: null }] }} board={emptyBoard()} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} {...REST} />);
    expect(screen.getByText("system-cli")).toBeInTheDocument();
    expect(screen.getByText(/3 events \/ #7/)).toBeInTheDocument();
    expect(screen.getByText(/Awaiting first work update/)).toBeInTheDocument();
  });

  it("exposes verified dependencies to assistive technology", () => {
    const model: GraphModel = {
      ...MODEL,
      edgesKnown: true,
      edges: [{ id: "implementation->deploy", from: "implementation", to: "deploy", type: "control" }],
    };
    render(<Board model={model} board={emptyBoard()} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} {...REST} />);
    expect(screen.getByRole("list", { name: "Verified dependencies" })).toHaveTextContent(
      "implementation connects to deploy (control)",
    );
  });
});

/** The props every render needs beyond the ones a given test is about. */
const REST = {
  connectionNote: "No graph file read yet.",
  connectionTone: "none" as const,
  graphFile: "",
  onGraphFileChange: () => {},
  onDrawConnections: () => {},
  busy: false,
};

/** Renders the board with live state, the way `App` holds it, so a change actually comes back as
 * the next `board` prop. A stub that swallowed the change would let the stale-hold bug pass. */
function mountBoard(initial: BoardState = emptyBoard()) {
  const changes: BoardState[] = [];
  let current = initial;
  const view = render(
    <Board model={MODEL} board={current} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} {...REST} />,
  );
  const rerender = (next: BoardState) => {
    current = next;
    changes.push(next);
    view.rerender(
      <Board model={MODEL} board={current} selectedNode={null} onSelectNode={() => {}} onChange={rerender} {...REST} />,
    );
  };
  view.rerender(
    <Board model={MODEL} board={current} selectedNode={null} onSelectNode={() => {}} onChange={rerender} {...REST} />,
  );
  return {
    changes,
    latest: () => current,
    sheet: () => document.querySelector(".sheet") as HTMLElement,
    card: (id: string) => screen.getByText(id).closest(".node") as HTMLElement,
  };
}

function press(target: Element, x: number, y: number) {
  fireEvent.pointerDown(target, { clientX: x, clientY: y, bubbles: true });
}
function drag(x: number, y: number) {
  fireEvent.pointerMove(window, { clientX: x, clientY: y, bubbles: true });
}
function release() {
  fireEvent.pointerUp(window, { bubbles: true });
}

describe("moving a card", () => {
  it("follows the pointer and keeps where it was left", () => {
    const board = mountBoard();
    press(board.card("implementation"), 10, 10);
    drag(120, 90);
    release();

    // Derived from `defaultPosition`, not from its numbers: the grid's spacing is a layout
    // decision that moves when a block's size changes, and a test that hard-codes it fails for a
    // reason that has nothing to do with dragging.
    const home = defaultPosition(0);
    expect(board.latest().positions.implementation).toEqual({
      x: 120 - 10 + home.x,
      y: 90 - 10 + home.y,
    });
  });

  /**
   * THE REGRESSION. After a release, the board must be holding nothing. On the old shape the
   * release listener had never been installed, so this second, unrelated press dragged the first
   * card across the sheet.
   */
  it("a press after a release does not move the card that was pressed before it", () => {
    const board = mountBoard();
    press(board.card("implementation"), 10, 10);
    drag(50, 50);
    release();
    const parked = board.latest().positions.implementation;

    // A press on the sheet itself, nowhere near the card, then a long move.
    press(board.sheet(), 400, 400);
    drag(900, 700);
    release();

    expect(board.latest().positions.implementation).toEqual(parked);
  });

  it("lets go when the browser cancels the pointer", () => {
    const board = mountBoard();
    press(board.card("deploy"), 5, 5);
    drag(60, 60);
    fireEvent.pointerCancel(window, { bubbles: true });
    const parked = board.latest().positions.deploy;

    drag(800, 800);
    expect(board.latest().positions.deploy).toEqual(parked);
  });
});

describe("drawing on the sheet", () => {
  it("records a stroke in the chosen tone", async () => {
    const board = mountBoard();
    await userEvent.click(screen.getByRole("button", { name: /^draw$/i }));

    press(board.sheet(), 20, 20);
    drag(60, 40);
    drag(100, 80);
    release();

    expect(board.latest().strokes).toHaveLength(1);
    expect(board.latest().strokes[0].tone).toBe("ink");
    expect(board.latest().strokes[0].points.length).toBeGreaterThan(1);
  });

  /** A tap is not a stroke. Without this, every click on the sheet with the pen selected would
   * leave a one-point mark that renders as nothing and still fills the board's budget. */
  it("does not record a stroke from a press with no movement", async () => {
    const board = mountBoard();
    await userEvent.click(screen.getByRole("button", { name: /^draw$/i }));
    press(board.sheet(), 20, 20);
    release();
    expect(board.latest().strokes).toEqual([]);
  });

  it("does not draw while the move tool is selected", () => {
    const board = mountBoard();
    press(board.sheet(), 20, 20);
    drag(60, 40);
    release();
    expect(board.latest().strokes).toEqual([]);
  });
});

describe("notes", () => {
  it("drops a note where the sheet was clicked and returns to moving", async () => {
    const board = mountBoard();
    await userEvent.click(screen.getByRole("button", { name: /^note$/i }));
    press(board.sheet(), 140, 160);

    expect(board.latest().notes).toHaveLength(1);
    expect(board.latest().notes[0].at).toEqual({ x: 140, y: 160 });
    expect(screen.getByRole("button", { name: /^move$/i })).toHaveAttribute("aria-pressed", "true");
  });
});

/** The disagreement signal, worn on the card: a node the log settled and then named again. */
describe("reopened after done", () => {
  const REOPENED_MODEL: GraphModel = {
    ...MODEL,
    nodes: [
      {
        ...MODEL.nodes[0],
        id: "deploy",
        state: "running",
        reopened: { settledAt: 3, reopenedAt: 9, by: "codex" },
      },
    ],
  };

  it("wears the chip with both coordinates when the log attests a reopening", () => {
    render(
      <Board
        model={REOPENED_MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        {...REST}
      />,
    );
    expect(screen.getByText("reopened after done · #3→#9")).toBeInTheDocument();
  });

  it("wears no chip on an undisturbed node", () => {
    render(
      <Board
        model={MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        {...REST}
      />,
    );
    expect(screen.queryByText(/reopened after done/)).not.toBeInTheDocument();
  });
});

/** The lint strip: the model's own accusations on the board's face, each citing the log. */
describe("the lint strip", () => {
  it("names each disagreement and offers the accused sequence to copy", () => {
    const model: GraphModel = {
      ...MODEL,
      lint: [
        { kind: "done-without-evidence", detail: "deploy settled as succeeded carrying no evidence", sequence: 7 },
        { kind: "orphan-edge", detail: "the graph file draws a → ghost, but ghost is not on this run's roster", sequence: null },
      ],
    };
    render(
      <Board
        model={model}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        runId="demo"
        {...REST}
      />,
    );
    const strip = screen.getByLabelText("Disagreements the log attests");
    expect(strip).toHaveTextContent("deploy settled as succeeded carrying no evidence");
    expect(strip).toHaveTextContent("ghost is not on this run's roster");
    // The accused event is a copyable coordinate; the file's edge has no sequence to cite.
    expect(screen.getByTitle("Copy demo#7")).toBeInTheDocument();
  });

  it("stays entirely off a clean board", () => {
    render(
      <Board
        model={MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        {...REST}
      />,
    );
    expect(screen.queryByLabelText("Disagreements the log attests")).not.toBeInTheDocument();
  });
});

describe("what the board refuses to imply", () => {
  it("keeps the directly opened node framed after a navigator choice and resize", async () => {
    let resize = () => {};
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: () => void) { resize = callback; }
      observe() {}
      disconnect() {}
    });
    const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 0, y: 0, top: 0, left: 0, right: 900, bottom: 700, width: 900, height: 700, toJSON() {} });
    try {
      const { container } = render(<Board model={MODEL} board={emptyBoard()} selectedNode={null} onSelectNode={vi.fn()} onChange={vi.fn()} runId="resize-test" {...REST} />);
      const transform = () => (container.querySelector(".world") as HTMLElement).style.transform;
      await userEvent.selectOptions(screen.getByRole("combobox", { name: "Find on board" }), "node:implementation");
      const expected = transform();
      await userEvent.selectOptions(screen.getByRole("combobox", { name: "Find on board" }), "node:deploy");
      expect(transform()).not.toBe(expected);
      fireEvent.click(container.querySelector(".node-open")!);
      act(() => resize());
      expect(transform()).toBe(expected);
    } finally {
      rect.mockRestore();
      vi.unstubAllGlobals();
    }
  });
  it("leaves Space activation available on focused controls", async () => {
    mountBoard();
    const history = screen.getAllByRole("button", { name: /show history/i })[0];
    history.focus();
    await userEvent.keyboard(" ");
    expect(history).toHaveAttribute("aria-expanded", "true");
  });

  it("opens the exact node picked in the board navigator", async () => {
    const selected = vi.fn();
    render(<Board model={MODEL} board={emptyBoard()} selectedNode={null} onSelectNode={selected} onChange={vi.fn()} {...REST} />);
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Find on board" }), "node:deploy");
    expect(selected).toHaveBeenCalledWith("deploy");
  });
  it("draws no connection between cards and says why", () => {
    const board = mountBoard();
    expect(board.sheet().querySelectorAll("path.edge")).toHaveLength(0);
    expect(screen.getByText(/work connections unverified/i)).toBeInTheDocument();
  });

  it("says the roster is incomplete when it has not been declared", () => {
    render(
      <Board
        model={{ ...MODEL, rosterDeclared: false }}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={vi.fn()}
        onChange={vi.fn()}
        {...REST}
      />,
    );
    expect(screen.getAllByText(/roster not read/i).length).toBeGreaterThan(0);
  });
});


describe("organizing a saved canvas", () => {
  it("restores the old positions without removing marks when the layout is undone", () => {
    const initial = { ...emptyBoard(), positions: { implementation: { x: 1900, y: 900 } }, agents: { "talk:room": { x: 1700, y: 20 } }, notes: [{ id: "note", at: { x: 3, y: 4 }, text: "Keep this" }] };
    const board = mountBoard(initial);
    fireEvent.click(screen.getByRole("button", { name: "Organize" }));
    expect(board.latest().positions).toEqual({});
    expect(board.latest().agents).toEqual({});
    expect(board.latest().notes).toEqual(initial.notes);
    fireEvent.click(screen.getByRole("button", { name: "Undo layout" }));
    expect(board.latest()).toEqual(initial);
  });
});


describe("canvas graph evidence boundaries", () => {
  it("does not crash or draw an edge to a node missing from the roster", () => {
    render(<Board model={{ ...MODEL, edgesKnown: true, edges: [{ id: "missing", from: "implementation", to: "ghost", type: "data" }] }} board={emptyBoard()} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} {...REST} />);
    expect(document.querySelectorAll("path.edge")).toHaveLength(0);
    expect(screen.getByText("implementation")).toBeInTheDocument();
  });
  it("offers graph verification as an accessible work-region action", () => {
    render(<Board model={MODEL} board={emptyBoard()} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} {...REST} />);
    fireEvent.click(screen.getByRole("button", { name: "Verify connections" }));
    expect(screen.getByRole("textbox", { name: "Graph file path on the Runtime host" })).toBeInTheDocument();
  });
});


it("does not intercept Space on a disclosure", () => {
  render(<Board model={{ ...MODEL, lint: [{ kind: "orphan-edge", detail: "Missing node", sequence: null }] }} board={emptyBoard()} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} {...REST} />);
  const summary = document.querySelector(".canvas-lint summary") as HTMLElement;
  summary.focus();
  expect(fireEvent.keyDown(summary, { key: " " })).toBe(true);
  expect(screen.getByRole("button", { name: "pan" })).toHaveAttribute("aria-pressed", "false");
});


it("pans from a section title without native text selection or moving cards", () => {
  const board = mountBoard();
  fireEvent.click(screen.getByRole("button", { name: "pan" }));
  const title = document.querySelector(".canvas-region strong") as HTMLElement;
  const range = document.createRange();
  range.selectNodeContents(title);
  window.getSelection()?.addRange(range);
  const initial = document.querySelector(".world")?.getAttribute("style");
  expect(fireEvent.pointerDown(title, { button: 0, clientX: 100, clientY: 100, bubbles: true })).toBe(false);
  drag(180, 150);
  release();
  expect(window.getSelection()?.toString()).toBe("");
  expect(document.querySelector(".world")?.getAttribute("style")).not.toBe(initial);
  expect(board.changes).toHaveLength(0);
});

/**
 * #1077: the blind judge opened the free canvas and saw 2 of 7 cards - an 80% floor on the
 * first framing and a one-column stack. First framing and Organize FIT THE CONTENT, and the
 * default grid uses the width it has (columns = floor(width / card width), at least 2), so a
 * fit is not a miniature. Positions the operator saved still win (#1056's rule).
 */
describe("first framing", () => {
  const SEVEN: GraphModel = {
    ...MODEL,
    nodes: ["a", "b", "c", "d", "e", "f", "g"].map((id) => ({ id, state: "ready" as const, touches: 0, lastEventAt: null, history: [], reopened: null })),
  };
  const VIEWPORT = { width: 1200, height: 800 };
  function mockViewport() {
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    return vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 0, y: 0, top: 0, left: 0, right: VIEWPORT.width, bottom: VIEWPORT.height, width: VIEWPORT.width, height: VIEWPORT.height, toJSON() {} });
  }
  /** Every card's screen rectangle, through the world transform. */
  function cardsOnScreen(container: HTMLElement) {
    const transform = (container.querySelector(".world") as HTMLElement).style.transform;
    const match = /translate\(([-\d.]+)px, ([-\d.]+)px\) scale\(([\d.]+)\)/.exec(transform);
    if (match === null) throw new Error(`unexpected transform ${transform}`);
    const [, x, y, zoom] = match.map(Number);
    return {
      zoom,
      rects: [...container.querySelectorAll<HTMLElement>("article.node")].map((card) => ({
        left: parseFloat(card.style.left) * zoom + x,
        top: parseFloat(card.style.top) * zoom + y,
        right: (parseFloat(card.style.left) + 320) * zoom + x,
        bottom: (parseFloat(card.style.top) + 164) * zoom + y,
        x: parseFloat(card.style.left),
      })),
    };
  }
  const inside = (rect: { left: number; top: number; right: number; bottom: number }) =>
    rect.left >= 0 && rect.top >= 0 && rect.right <= VIEWPORT.width && rect.bottom <= VIEWPORT.height;

  it("opens with every card on screen, laid out across the width", () => {
    const rect = mockViewport();
    try {
      const { container } = render(<Board initialLayout="canvas" model={SEVEN} board={emptyBoard()} selectedNode={null} onSelectNode={vi.fn()} onChange={vi.fn()} runId="seven" {...REST} />);
      const { rects, zoom } = cardsOnScreen(container);
      expect(rects).toHaveLength(7);
      expect(rects.every(inside)).toBe(true);
      // floor(1200 / 320) = 3 columns, not one stacked column.
      expect(new Set(rects.map((card) => card.x)).size).toBe(3);
      expect(zoom).toBeLessThanOrEqual(1);
      expect(screen.getByRole("button", { name: /%$/ })).not.toHaveTextContent("80%");
    } finally {
      rect.mockRestore();
      vi.unstubAllGlobals();
    }
  });

  it("re-frames the content after Organize, and Undo layout is still offered", () => {
    const rect = mockViewport();
    try {
      let board: BoardState = { ...emptyBoard(), positions: { a: { x: 4000, y: 3000 } } };
      const view = render(<Board initialLayout="canvas" model={SEVEN} board={board} selectedNode={null} onSelectNode={vi.fn()} onChange={(next) => { board = next; }} runId="seven" {...REST} />);
      fireEvent.click(screen.getByRole("button", { name: "Organize" }));
      view.rerender(<Board initialLayout="canvas" model={SEVEN} board={board} selectedNode={null} onSelectNode={vi.fn()} onChange={(next) => { board = next; }} runId="seven" {...REST} />);
      expect(board.positions).toEqual({});
      const { rects } = cardsOnScreen(view.container);
      expect(rects.every(inside)).toBe(true);
      expect(screen.getByRole("button", { name: "Undo layout" })).toBeInTheDocument();
    } finally {
      rect.mockRestore();
      vi.unstubAllGlobals();
    }
  });

  it("still honours a position the operator saved", () => {
    const rect = mockViewport();
    try {
      const { container } = render(<Board initialLayout="canvas" model={SEVEN} board={{ ...emptyBoard(), positions: { a: { x: 4000, y: 3000 } } }} selectedNode={null} onSelectNode={vi.fn()} onChange={vi.fn()} runId="seven" {...REST} />);
      const moved = [...container.querySelectorAll<HTMLElement>("article.node")].find((card) => card.style.left === "4000px");
      expect(moved).toBeDefined();
    } finally {
      rect.mockRestore();
      vi.unstubAllGlobals();
    }
  });
});
