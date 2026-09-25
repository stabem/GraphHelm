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
import { fastUserEvent } from "../test/user-event";
// Its own instance: see the helper for why this is not a shared const.
const userEvent = fastUserEvent();

import { Board } from "./board";
import { defaultPosition, emptyBoard, type BoardState } from "../graph/board";
import type { GraphModel } from "../graph/model";
import studioStyles from "../styles.css?raw";
import { newestPresenceByActor, type AgentPresence } from "../runtime/session";
import type { RuntimeEvent } from "../runtime/types";

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

/**
 * The ink layer covers the drawn content (#1072). The sheet-ink SVG was fixed at 2600x1700, so a
 * verified connection into a generated column or row past that box was clipped while the DOM card
 * it points at stayed visible. The SVG must be at least as wide and tall as the farthest card.
 */
describe("the ink layer", () => {
  it("is at least as wide and tall as the farthest card when 16 nodes carry a verified edge into the last column", () => {
    const nodes = Array.from({ length: 16 }, (_, index) => ({ id: `n${index}`, state: "ready" as const, touches: 0, lastEventAt: null, history: [], reopened: null }));
    const model: GraphModel = {
      ...MODEL,
      nodes,
      edgesKnown: true,
      edges: [{ id: "n0->n15", from: "n0", to: "n15", type: "control" }],
    };
    const board: BoardState = { ...emptyBoard(), positions: { n15: { x: 3400, y: 2200 } } };
    const { container } = render(<Board model={model} board={board} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} {...REST} />);
    const ink = container.querySelector("svg.sheet-ink");
    expect(ink).not.toBeNull();
    expect(container.querySelectorAll("svg.sheet-ink path.edge")).toHaveLength(1);
    const width = Number(ink!.getAttribute("width"));
    const height = Number(ink!.getAttribute("height"));
    expect(width).toBeGreaterThanOrEqual(3400 + 320);
    expect(height).toBeGreaterThanOrEqual(2200 + 100);
  });

  it("renders at the ink extent under the real stylesheet, where the farthest card sets both width and height", () => {
    /* The global `svg { width: 14px; height: 14px }` rule in styles.css outranks presentation
       attributes, so the attributes alone do not size the box (#1072 review). This cell applies the
       real stylesheet and reads the COMPUTED size. The farthest card is a touched one (288px tall) at
       (3400, 2400), past every grid slot, so dropping the card height from the extent changes the
       computed height from 2768 to 2480 (measured by that sabotage) and reddens this cell. */
    const style = document.createElement("style");
    style.textContent = studioStyles;
    document.head.appendChild(style);
    try {
      const nodes = Array.from({ length: 16 }, (_, index) => ({ id: `n${index}`, state: "ready" as const, touches: index === 15 ? 1 : 0, lastEventAt: null, history: [], reopened: null }));
      const model: GraphModel = { ...MODEL, nodes, edgesKnown: true, edges: [{ id: "n0->n15", from: "n0", to: "n15", type: "control" }] };
      const board: BoardState = { ...emptyBoard(), positions: { n15: { x: 3400, y: 2400 } } };
      const { container } = render(<Board model={model} board={board} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} {...REST} />);
      const ink = container.querySelector("svg.sheet-ink") as SVGSVGElement;
      const computed = getComputedStyle(ink);
      expect({ width: computed.width, height: computed.height }).toEqual({ width: `${3400 + 320 + 80}px`, height: `${2400 + 288 + 80}px` });
      expect(computed.overflow).toBe("visible");
    } finally {
      style.remove();
    }
  });});

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

  /** #1083: on the same completed demonstration run the canvas said an orange "6 log
   * disagreements" while the overview said the neutral "Demonstration run · 6 log notes". The
   * canvas now speaks as the overview does; a real run keeps the amber strip. */
  it("uses the overview's neutral note on a demonstration run, and amber on a real run", () => {
    const model: GraphModel = { ...MODEL, lint: [{ kind: "done-without-evidence", detail: "deploy settled with no evidence", sequence: 7 }, { kind: "done-without-evidence", detail: "tests settled with no evidence", sequence: 8 }] };
    const demo = render(<Board model={model} board={emptyBoard()} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} runId="demo" demonstration {...REST} />);
    const note = demo.container.querySelector(".canvas-lint") as HTMLElement;
    expect(note).toHaveClass("canvas-note");
    expect(note.querySelector("summary")).toHaveTextContent("Demonstration run · 2 log notes");
    expect(screen.queryByText(/log disagreements/)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Log notes on a demonstration run")).toHaveTextContent("deploy settled with no evidence");
    demo.unmount();

    const real = render(<Board model={model} board={emptyBoard()} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} runId="real" {...REST} />);
    const strip = real.container.querySelector(".canvas-lint") as HTMLElement;
    expect(strip).not.toHaveClass("canvas-note");
    expect(strip.querySelector("summary")).toHaveTextContent("2 log disagreements");
    expect(screen.getByLabelText("Disagreements the log attests")).toBeInTheDocument();
  });

  /** Codex on PR #1091: the canvas downgrade applies to the fixture-explained kind only. With one
   * of each kind on a demonstration run the strip stays amber, counts the two real disagreements,
   * lists them first, and counts the explained one separately. */
  it("keeps the amber strip for real disagreements on a demonstration run", () => {
    const model: GraphModel = {
      ...MODEL,
      lint: [
        { kind: "done-without-evidence", detail: "docs settled as succeeded carrying no evidence", sequence: 8 },
        { kind: "reopened-after-done", detail: "tests was reopened after it settled by codex", sequence: 11 },
        { kind: "orphan-edge", detail: "the graph file draws plan → ghost, but ghost is not on this run's roster", sequence: null },
      ],
    };
    const { container } = render(<Board model={model} board={emptyBoard()} selectedNode={null} onSelectNode={() => {}} onChange={() => {}} runId="demo" demonstration {...REST} />);
    const strip = container.querySelector(".canvas-lint") as HTMLElement;
    expect(strip).not.toHaveClass("canvas-note");
    expect(strip.querySelector("summary")).toHaveTextContent("2 log disagreements · 1 demonstration note");
    const list = screen.getByLabelText("Disagreements the log attests");
    expect(screen.queryByLabelText("Log notes on a demonstration run")).not.toBeInTheDocument();
    const items = [...list.querySelectorAll("li")].map((item) => item.textContent ?? "");
    expect(items).toHaveLength(3);
    expect(items[0]).toContain("reopened after it settled");
    expect(items[1]).toContain("ghost is not on this run's roster");
    expect(items[2]).toContain("carrying no evidence");
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

/**
 * The declared model and effort, worn beside an agent's name.
 *
 * `withPresence`, `withoutPresence` and `withPresenceHistory` build a CREW ROSTER (the shape
 * `Board`'s own `crew` prop takes), each one's `presence` field carried through
 * `newestPresenceByActor` - the same reduction the runtime module exposes - so these tests
 * exercise the real newest-per-actor logic, not a fixture that already knows the answer.
 */
describe("the declared model and effort", () => {
  const presenceEvent = (sequence: number, actorId: string, model: string, effort?: AgentPresence["effort"]): RuntimeEvent => ({
    sequence,
    kind: "agent_presence_declared",
    payload: { actorId, actorType: "agent", model, effort },
    occurredAt: null,
    actorId,
    actorType: "agent",
    idempotencyKey: null,
    eventId: null,
    evidenceRefs: [],
  });

  const crewWith = (actorId: string, events: RuntimeEvent[]) => [
    { id: actorId, charter: null, lastAt: null, presence: newestPresenceByActor(events)[actorId] ?? null },
  ];

  function withPresence(actorId: string, model: string, effort?: AgentPresence["effort"]) {
    return crewWith(actorId, [presenceEvent(1, actorId, model, effort)]);
  }

  function withoutPresence(actorId: string) {
    return crewWith(actorId, []);
  }

  function withPresenceHistory(actorId: string, declarations: Array<[string, AgentPresence["effort"]]>) {
    return crewWith(
      actorId,
      declarations.map(([model, effort], index) => presenceEvent(index + 1, actorId, model, effort)),
    );
  }

  it("shows the model and effort beside the name once declared", () => {
    render(
      <Board
        model={MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        crew={withPresence("codex", "gpt-6-astra", "low")}
        {...REST}
      />,
    );
    expect(screen.getByText("gpt-6-astra · low")).toBeInTheDocument();
  });

  it("shows NOTHING beside a name that never declared", () => {
    render(
      <Board
        model={MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        crew={withoutPresence("codex")}
        {...REST}
      />,
    );
    expect(screen.queryByText(/unknown|default|n\/a/i)).not.toBeInTheDocument();
  });

  it("shows the model alone when effort was not declared", () => {
    render(
      <Board
        model={MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        crew={withPresence("codex", "gpt-6-astra", undefined)}
        {...REST}
      />,
    );
    expect(screen.getByText("gpt-6-astra")).toBeInTheDocument();
    expect(screen.queryByText("·")).not.toBeInTheDocument();
  });

  it("shows the NEWEST declaration when a session changed model mid-run", () => {
    render(
      <Board
        model={MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        crew={withPresenceHistory("codex", [
          ["a", "low"],
          ["b", "high"],
        ])}
        {...REST}
      />,
    );
    expect(screen.getByText("b · high")).toBeInTheDocument();
    expect(screen.queryByText("a · low")).not.toBeInTheDocument();
  });

  /**
   * #1057: a LATER SESSION that declares nothing does not wear the previous session's model.
   *
   * An actor id is stable across sessions, so the board used to show a model belonging to a
   * session that had ended as the live session's own - and kept refreshing `lastAt` from the new
   * session's mutations while it did. The Runtime now records a model-less
   * `agent_presence_declared` on an undeclared session's first write, and this is what that has to
   * mean on screen: no badge, not a stale one.
   */
  it("drops the badge when a later session declared nothing", () => {
    const boundary = (sequence: number, actorId: string, session: string): RuntimeEvent => ({
      ...presenceEvent(sequence, actorId, "unused"),
      payload: { actorId, actorType: "agent", session },
    });
    const { container } = render(
      <Board
        model={MODEL}
        board={emptyBoard()}
        selectedNode={null}
        onSelectNode={() => {}}
        onChange={() => {}}
        crew={crewWith("codex", [
          presenceEvent(1, "codex", "gpt-6-astra", "low"),
          boundary(2, "codex", "session-two"),
        ])}
        {...REST}
      />,
    );
    expect(container.querySelector(".agent-badge")).toBeNull();
    expect(screen.queryByText("gpt-6-astra · low")).not.toBeInTheDocument();
    expect(screen.queryByText(/gpt-6-astra/)).not.toBeInTheDocument();
  });

  /**
   * #1057 Codex P2: `constructor` is a VALID actor id, and on a plain object literal
   * `presence["constructor"]` answers the inherited function rather than `undefined` - the board
   * then rendered a badge for an actor that had declared nothing at all. The CONTROL is the
   * declaring half: the same id really does wear a badge once it declares, so the absence below is
   * about the prototype and not about the id being rejected somewhere.
   */
  it.each(["constructor", "toString", "valueOf"])(
    "shows nothing for the undeclared actor %s, whose id names a prototype member",
    (actorId) => {
      const { container, unmount } = render(
        <Board
          model={MODEL}
          board={emptyBoard()}
          selectedNode={null}
          onSelectNode={() => {}}
          onChange={() => {}}
          crew={withoutPresence(actorId)}
          {...REST}
        />,
      );
      // The ELEMENT, not its text: a prototype member read as presence renders an EMPTY badge, so
      // a text assertion would pass over exactly the defect this cell is about.
      expect(container.querySelector(".agent-badge")).toBeNull();
      unmount();

      render(
        <Board
          model={MODEL}
          board={emptyBoard()}
          selectedNode={null}
          onSelectNode={() => {}}
          onChange={() => {}}
          crew={withPresence(actorId, "gpt-6-astra", "low")}
          {...REST}
        />,
      );
      expect(screen.getByText("gpt-6-astra · low")).toBeInTheDocument();
    },
  );
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
/**
 * #1083 F8: a six-node run with the People and Conversations lanes opened on the free canvas at
 * 60% with two of six cards on screen, and `fit` dropped to 15% - the lanes' chrome was being
 * framed as content. First framing and `fit` frame the CARDS at a readable zoom; when all of them
 * cannot fit at that zoom, the first ranks are on screen and a pan reveals the rest.
 */
describe("framing a run with lanes", () => {
  const SIX: GraphModel = {
    ...MODEL,
    nodes: ["docs", "implement", "map_repository", "plan", "review", "tests"].map((id) => ({ id, state: "succeeded" as const, touches: 0, lastEventAt: null, history: [], reopened: null })),
  };
  const CREW = [{ id: "codex", charter: null }, { id: "reviewer", charter: null }];
  const TALKS = [
    { key: "room", label: "Everyone", participants: [], count: 3, lastAt: null },
    { key: "codex+reviewer", label: "codex + reviewer", participants: ["codex", "reviewer"], count: 2, lastAt: null },
  ];
  const zoomOf = (container: HTMLElement) => {
    const match = /scale\(([\d.]+)\)/.exec((container.querySelector(".world") as HTMLElement).style.transform);
    return Number(match?.[1]);
  };
  function onScreen(container: HTMLElement, viewport: { width: number; height: number }) {
    const match = /translate\(([-\d.]+)px, ([-\d.]+)px\) scale\(([\d.]+)\)/.exec((container.querySelector(".world") as HTMLElement).style.transform)!;
    const [, x, y, zoom] = match.map(Number);
    return [...container.querySelectorAll<HTMLElement>("article.node")].map((card) => {
      const left = parseFloat(card.style.left) * zoom + x;
      const top = parseFloat(card.style.top) * zoom + y;
      return { top, inside: left >= 0 && top >= 0 && left + 320 * zoom <= viewport.width && top + 164 * zoom <= viewport.height };
    });
  }

  it("opens at a readable zoom with the first ranks on screen, and fit does not shrink it to thumbnails", () => {
    const viewport = { width: 900, height: 560 };
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 0, y: 0, top: 0, left: 0, right: viewport.width, bottom: viewport.height, width: viewport.width, height: viewport.height, toJSON() {} });
    try {
      const { container } = render(<Board initialLayout="canvas" model={SIX} board={emptyBoard()} selectedNode={null} onSelectNode={vi.fn()} onChange={vi.fn()} runId="exec_feature" {...REST} crew={CREW} talks={TALKS} />);
      expect(container.querySelectorAll(".canvas-region")).toHaveLength(3);
      const first = zoomOf(container);
      expect(first).toBeGreaterThanOrEqual(0.75);
      const cards = onScreen(container, viewport);
      expect(cards).toHaveLength(6);
      const firstRank = Math.min(...cards.map((card) => card.top));
      // Every card of the first two ranks is wholly on screen; nothing is lost to the lanes.
      const ranked = cards.filter((card) => card.top < firstRank + 1 + 320 * first);
      expect(ranked.length).toBeGreaterThanOrEqual(4);
      expect(ranked.every((card) => card.inside)).toBe(true);

      fireEvent.click(screen.getByRole("button", { name: "fit" }));
      expect(zoomOf(container)).toBeGreaterThanOrEqual(0.75);
      expect(screen.getByRole("button", { name: /%$/ })).not.toHaveTextContent("15%");
    } finally {
      rect.mockRestore();
      vi.unstubAllGlobals();
    }
  });

  it("frames all six when the viewport can hold them at a readable zoom", () => {
    const viewport = { width: 1400, height: 1100 };
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 0, y: 0, top: 0, left: 0, right: viewport.width, bottom: viewport.height, width: viewport.width, height: viewport.height, toJSON() {} });
    try {
      const { container } = render(<Board initialLayout="canvas" model={SIX} board={emptyBoard()} selectedNode={null} onSelectNode={vi.fn()} onChange={vi.fn()} runId="exec_feature" {...REST} crew={CREW} talks={TALKS} />);
      expect(zoomOf(container)).toBeGreaterThanOrEqual(0.75);
      expect(onScreen(container, viewport).every((card) => card.inside)).toBe(true);
    } finally {
      rect.mockRestore();
      vi.unstubAllGlobals();
    }
  });
});

/**
 * #1083 F8, second pass (orchestrator verification at 1280x720): first framing and `fit` held 75%,
 * but the cards' lower third sat under the canvas toolbar row and the header chrome ate the top.
 * Framing now measures the chrome that floats over the sheet and frames inside the band between.
 * Here the sheet is 683x360; a lint strip covers its top 40px and the toolbar its bottom 60px, as
 * laid out on the narrow scene. No framed card may cross into either, and the first rank is whole.
 */
describe("framing around the canvas chrome", () => {
  const SIX: GraphModel = {
    ...MODEL,
    lint: [{ kind: "done-without-evidence", detail: "no evidence", sequence: 3 }],
    nodes: ["docs", "implement", "map_repository", "plan", "review", "tests"].map((id) => ({ id, state: "succeeded" as const, touches: 0, lastEventAt: null, history: [], reopened: null })),
  };
  const SHEET = { top: 300, bottom: 660, left: 0, right: 683 };
  const LINT = { top: 300, bottom: 340, left: 16, right: 260 };
  const TOOLS = { top: 600, bottom: 652, left: 12, right: 640 };
  const rectOf = (r: { top: number; bottom: number; left: number; right: number }) => ({ ...r, x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top, toJSON() {} });
  function mockLayout() {
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    return vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.classList.contains("sheet")) return rectOf(SHEET) as DOMRect;
      if (this.classList.contains("canvas-lint")) return rectOf(LINT) as DOMRect;
      if (this.classList.contains("tools")) return rectOf(TOOLS) as DOMRect;
      return rectOf({ top: 0, bottom: 0, left: 0, right: 0 }) as DOMRect;
    });
  }
  function framed(container: HTMLElement) {
    const match = /translate\(([-\d.]+)px, ([-\d.]+)px\) scale\(([\d.]+)\)/.exec((container.querySelector(".world") as HTMLElement).style.transform)!;
    const [, x, y, zoom] = match.map(Number);
    const cards = [...container.querySelectorAll<HTMLElement>("article.node")].map((card) => {
      const top = SHEET.top + parseFloat(card.style.top) * zoom + y;
      const left = SHEET.left + parseFloat(card.style.left) * zoom + x;
      return { top, bottom: top + 164 * zoom, left, right: left + 320 * zoom };
    });
    return { zoom, cards };
  }
  const crosses = (card: { top: number; bottom: number; left: number; right: number }, piece: typeof LINT) =>
    card.left < piece.right && piece.left < card.right && card.top < piece.bottom && piece.top < card.bottom;
  const onSheet = (card: { top: number; bottom: number }) => card.bottom > SHEET.top && card.top < SHEET.bottom;

  it("frames the cards inside the band the lint strip and the toolbar leave, on first framing and on fit", () => {
    const rect = mockLayout();
    try {
      const { container } = render(<Board initialLayout="canvas" model={SIX} board={emptyBoard()} selectedNode={null} onSelectNode={vi.fn()} onChange={vi.fn()} runId="exec_feature" {...REST} crew={[{ id: "codex", charter: null }]} talks={[{ key: "room", label: "Everyone", participants: [], count: 1, lastAt: null }]} />);
      for (const pass of ["first framing", "fit"]) {
        if (pass === "fit") fireEvent.click(screen.getByRole("button", { name: "fit" }));
        const { zoom, cards } = framed(container);
        expect(zoom, pass).toBeGreaterThanOrEqual(0.6);
        // THE FRAMED RANK: the first rank is what framing promises whole. Every card of it is
        // on the sheet, inside the band, and crosses neither piece of chrome - the defect was
        // exactly this rank's lower third under the toolbar.
        const firstTop = Math.min(...cards.map((card) => card.top));
        const firstRank = cards.filter((card) => Math.abs(card.top - firstTop) < 1);
        expect(firstRank.length, pass).toBeGreaterThanOrEqual(2);
        for (const card of firstRank) {
          expect(onSheet(card), pass).toBe(true);
          expect(crosses(card, TOOLS), `${pass}: a framed card sits under the toolbar`).toBe(false);
          expect(crosses(card, LINT), `${pass}: a framed card sits under the lint strip`).toBe(false);
          expect(card.top, pass).toBeGreaterThanOrEqual(LINT.bottom);
          expect(card.bottom, pass).toBeLessThanOrEqual(TOOLS.top);
        }
        // Nothing sits under the lint strip at the top: later ranks only continue downward.
        for (const card of cards.filter(onSheet)) {
          expect(crosses(card, LINT), `${pass}: a card sits under the lint strip`).toBe(false);
        }
      }
    } finally {
      rect.mockRestore();
      vi.unstubAllGlobals();
    }
  });
});

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
