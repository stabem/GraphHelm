import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { WorkOverview, isFirstEntryNode } from "./work-overview";
import type { GraphModel } from "../graph/model";

const node = (id: string) => ({ id, state: "unknown", touches: 0, lastEventAt: null, history: [], reopened: null });
const model: GraphModel = { nodes: [node("triage"), node("work")], edges: [{id:"link",from:"triage",to:"work",type:"dependency"}], edgesKnown: false, entrypoints: [], rosterDeclared: true, lint: [] };
describe("organized work overview", () => {
  it("retains incomplete-roster and disagreement evidence in the default view", () => {
    render(<WorkOverview model={{...model,rosterDeclared:false,lint:[{kind:"done-without-evidence",detail:"Completion has no evidence",sequence:8}]}} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByText("2 nodes seen so far")).toBeInTheDocument();
    expect(screen.getByRole("region",{name:"Disagreements in the event log"})).toHaveTextContent("Completion has no evidence · event #8");
  });
  it("never presents unverified model edges as dependencies", () => {
    render(<WorkOverview model={model} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.queryByRole("button", {name:"to work"})).not.toBeInTheDocument();
    expect(screen.getAllByText("Awaiting event")).toHaveLength(2);
  });
  it("follows a verified dependency into the existing node panel", () => {
    const select=vi.fn();
    render(<WorkOverview model={{...model, edgesKnown:true}} selectedNode={null} onSelectNode={select} />);
    fireEvent.click(screen.getByRole("button",{name:"to work"}));
    expect(select).toHaveBeenCalledWith("work");
  });
  it("includes newly observed nodes without a template or lost selection", () => {
    const select=vi.fn();
    const {rerender}=render(<WorkOverview model={model} selectedNode="work" onSelectNode={select} />);
    rerender(<WorkOverview model={{...model,nodes:[...model.nodes,node("owner-defined-step")]}} selectedNode="work" onSelectNode={select} />);
    expect(screen.getByRole("button",{name:/owner-defined-step/})).toBeInTheDocument();
    expect(screen.getByRole("button",{name:/\bwork\b/})).toHaveAttribute("aria-pressed","true");
  });
  it("does not invent an assignment and opens the existing conversation", () => {
    const select=vi.fn();
    render(<WorkOverview model={model} crew={[{id:"codex",charter:null}]} talks={[{key:"pair",label:"codex + helper",participants:["codex","helper"],count:3,lastAt:null}]} selectedNode={null} onSelectNode={vi.fn()} onSelectTalk={select} />);
    expect(screen.getByText("No node activity yet")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button",{name:/codex \+ helper/}));
    expect(select).toHaveBeenCalledWith("pair");
  });
  it("shows recorded messages even while no graph node is active", () => {
    render(<WorkOverview model={model} crew={[{id:"codex",charter:null,lastAt:"2026-09-25T17:00:00Z"}]} talks={[{key:"room",label:"everyone",participants:["codex"],count:1,lastAt:"2026-09-25T17:00:00Z",preview:"Checking the failing flow"}]} activity={[{sequence:39,actorId:"codex",occurredAt:"2026-09-25T17:00:00Z",text:"Checking the failing flow"}]} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByText("0 active nodes")).toBeInTheDocument();
    const activity = screen.getByRole("region", {name:"Recent recorded activity"});
    expect(activity).toHaveTextContent("codex");
    expect(activity).toHaveTextContent("event #39");
    expect(activity).toHaveTextContent("Checking the failing flow");
    expect(screen.getByText("No node activity yet")).toBeInTheDocument();
    expect(screen.getByText("Last recorded message or event", {exact:false})).toBeInTheDocument();
    expect(screen.getByText("Checking the failing flow", {selector:".work-talk-preview"})).toBeInTheDocument();
  });
  it("keeps the report pending while its text has not opened", () => {
    render(<WorkOverview model={model} activity={[{sequence:40,actorId:"reviewer",occurredAt:null,text:null}]} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByRole("region", {name:"Recent recorded activity"})).toHaveTextContent("Report text has not opened yet");
  });
  it("shows where a one-node run stands and each agent's latest recorded report", () => {
    const selectAgent = vi.fn();
    render(<WorkOverview
      model={{ ...model, nodes: [node("start")] }}
      crew={[{ id: "builder", charter: null }, { id: "reviewer", charter: null }]}
      activity={[{ sequence: 20, actorId: "reviewer", occurredAt: "2026-09-25T17:02:00Z", text: "Checking the branch" }]}
      agentReports={{
        builder: { sequence: 11, occurredAt: "2026-09-25T17:00:00Z", text: "Implementing the issue" },
        reviewer: { sequence: 20, occurredAt: "2026-09-25T17:02:00Z", text: "Checking the branch" },
      }}
      runStatus="paused"
      selectedNode={null}
      onSelectNode={vi.fn()}
      onSelectAgent={selectAgent}
    />);
    const snapshot = screen.getByRole("region", { name: "Where this run stands" });
    expect(snapshot).toHaveTextContent("paused");
    expect(snapshot).toHaveTextContent("start · unknown");
    expect(snapshot).toHaveTextContent("reviewer");
    expect(snapshot).toHaveTextContent("This run declares one graph node");
    const builder = screen.getByText("builder", {selector: ".work-agent-id"}).closest("details")!;
    expect(builder).toHaveTextContent("event #11");
    expect(screen.getAllByText("Last recorded report", {selector: ".work-agent-report > span"})[0].closest("details")).toHaveTextContent("reviewer");
    fireEvent.click(within(builder).getByText("builder", {selector: ".work-agent-id"}).closest("summary")!);
    expect(builder).toHaveAttribute("open");
    expect(within(builder).getByText("Implementing the issue", {selector: ".work-agent-expanded p"})).toBeVisible();
    expect(selectAgent).not.toHaveBeenCalled();
    fireEvent.click(within(builder).getByRole("button", {name:"Open direct chat with builder"}));
    expect(selectAgent).toHaveBeenCalledWith("builder");
    expect(snapshot).not.toHaveTextContent("Running");
  });
  it("shows the next step before the activity and opens its action", () => {
    const act = vi.fn();
    render(<WorkOverview model={model} selectedNode={null} onSelectNode={vi.fn()} nextAction={{label:"Send direction",detail:"No question is visible yet."}} onNextAction={act} />);
    const action = screen.getByRole("region", {name:"Next action"});
    expect(action).toHaveTextContent("No question is visible yet.");
    fireEvent.click(within(action).getByRole("button", {name:"Send direction"}));
    expect(act).toHaveBeenCalledOnce();
  });
});

/** #1083 F9: a completed demonstration run carried `Evidence needs attention · 6 findings` in
 * amber - an alarm on a run that needs nothing. A demonstration run gets a neutral note with the
 * same findings readable; a real run with the same findings keeps the alarm. */
describe("log findings on a demonstration run", () => {
  const findings: GraphModel = { ...model, lint: [{ kind: "done-without-evidence", detail: "Completion has no evidence", sequence: 8 }, { kind: "done-without-evidence", detail: "Completion has no evidence", sequence: 9 }] };
  it("reads as a neutral note, never as the attention banner", () => {
    render(<WorkOverview model={findings} demonstration selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.queryByRole("region", { name: "Disagreements in the event log" })).not.toBeInTheDocument();
    expect(screen.queryByText(/needs attention/i)).not.toBeInTheDocument();
    const note = screen.getByRole("region", { name: "Log notes on a demonstration run" });
    expect(note).toHaveClass("work-note");
    expect(note).toHaveTextContent("Demonstration run · 2 log notes");
    expect(note).toHaveTextContent("Completion has no evidence · event #8");
  });
  /** Codex on PR #1091: only the fixture-explained kind is downgraded. A reopened settled node and
   * an orphan edge are real disagreements on a demonstration run too - they keep the attention
   * banner, and each section counts only its own findings. */
  it("keeps real disagreements amber on a demonstration run and counts only the explained note", () => {
    const mixed: GraphModel = {
      ...model,
      lint: [
        { kind: "done-without-evidence", detail: "docs settled as succeeded carrying no evidence", sequence: 8 },
        { kind: "reopened-after-done", detail: "tests was reopened after it settled by codex", sequence: 11 },
        { kind: "orphan-edge", detail: "the graph file draws plan → ghost, but ghost is not on this run's roster", sequence: null },
      ],
    };
    render(<WorkOverview model={mixed} demonstration selectedNode={null} onSelectNode={vi.fn()} />);
    const note = screen.getByRole("region", { name: "Log notes on a demonstration run" });
    expect(note).toHaveTextContent("Demonstration run · 1 log note");
    expect(note).toHaveTextContent("docs settled as succeeded carrying no evidence");
    expect(note).not.toHaveTextContent("reopened");
    expect(note).not.toHaveTextContent("ghost");
    const alarm = screen.getByRole("region", { name: "Disagreements in the event log" });
    expect(alarm).toHaveClass("work-caution");
    expect(alarm).toHaveTextContent("Evidence needs attention · 2 findings");
    expect(alarm).toHaveTextContent("tests was reopened after it settled by codex · event #11");
    expect(alarm).toHaveTextContent("ghost is not on this run's roster");
    expect(alarm).not.toHaveTextContent("carrying no evidence");
  });
  it("keeps the alarm for a real run with real findings", () => {
    render(<WorkOverview model={findings} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByRole("region", { name: "Disagreements in the event log" })).toHaveTextContent("Evidence needs attention · 2 findings");
    expect(screen.queryByRole("region", { name: "Log notes on a demonstration run" })).not.toBeInTheDocument();
  });
});

/** #1079 review (P2): the briefing's objective is the FIRST entrypoint's, in `spec.entrypoints`
 * order (#1071). A graph with two entrypoints shows it on one card, never on both. */
describe("the objective on the entry card", () => {
  const two: GraphModel = { ...model, entrypoints: ["work", "triage"] };
  it("is attached to the first entrypoint only", () => {
    render(<WorkOverview model={two} objective="Investigate slow login on mobile" selectedNode={null} onSelectNode={vi.fn()} />);
    const quotes = screen.getAllByText("Investigate slow login on mobile");
    expect(quotes).toHaveLength(1);
    expect(within(quotes[0].closest("article")!).getByText("work")).toBeInTheDocument();
    expect(isFirstEntryNode(two, "triage")).toBe(false);
    expect(isFirstEntryNode(two, "work")).toBe(true);
  });
  it("falls back to a declared one-node roster, and to nothing when the entrypoints are unknown", () => {
    expect(isFirstEntryNode({ ...model, nodes: [node("only")] }, "only")).toBe(true);
    expect(isFirstEntryNode(model, "triage")).toBe(false);
  });
});

/**
 * #1098 D5: on a COMPLETED run whose six nodes all succeeded, every card and the section heading
 * still read "Dependencies awaiting evidence" — a present participle promising something still on
 * its way, on a run where nothing more will ever arrive. On an ended run the wording must stop
 * promising an arrival. It says what is true of THE VIEW — the evidence is not available here —
 * rather than what this page cannot know about the run's past (see the P2 guard below). A run
 * still going keeps the awaiting wording, because for it the evidence really can still arrive.
 */
describe("a finished run does not read as still waiting for its dependencies", () => {
  const succeeded: GraphModel = {
    ...model,
    nodes: [
      { id: "triage", state: "succeeded", touches: 3, lastEventAt: null, history: [], reopened: null },
      { id: "work", state: "succeeded", touches: 3, lastEventAt: null, history: [], reopened: null },
    ],
  };

  it("states the absence in the present, in the heading and on every card", () => {
    render(<WorkOverview model={succeeded} ended selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.queryAllByText(/awaiting evidence/i)).toHaveLength(0);
    expect(screen.getByText("Dependency evidence is unavailable in this view")).toBeInTheDocument();
    expect(screen.getAllByText("Dependency evidence is unavailable in this view.")).toHaveLength(2);
  });

  it("keeps the awaiting wording while the run is still going", () => {
    render(<WorkOverview model={succeeded} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByText("Dependencies awaiting evidence")).toBeInTheDocument();
    expect(screen.getAllByText("Dependencies awaiting evidence.")).toHaveLength(2);
  });
});

/**
 * PR #1167 review, /root's BLOCKING P2. "Dependencies were never verified" is a claim about the
 * run's WHOLE HISTORY, inferred from one bit of the CURRENT view. `edgesKnown` is false whenever
 * this view holds no verified topology — which is also what a fresh selection leaves behind (the
 * page drops the proof with the board), and what a later graph-read failure leaves behind. A run
 * whose topology WAS verified a moment ago then reads as one that never was, and the sentence is
 * false about the only thing it talks about.
 *
 * The honest sentence is about the view, not about the past: the evidence is unavailable HERE.
 * A run still going keeps the "awaiting" wording, because for it the evidence really can arrive.
 */
describe("unavailable dependency evidence is not a claim about the run's past", () => {
  const succeeded: GraphModel = {
    ...model,
    nodes: [
      { id: "triage", state: "succeeded", touches: 3, lastEventAt: null, history: [], reopened: null },
      { id: "work", state: "succeeded", touches: 3, lastEventAt: null, history: [], reopened: null },
    ],
  };

  it("does not say the dependencies were never verified when this very view verified them a moment ago", () => {
    const { rerender } = render(
      <WorkOverview model={{ ...succeeded, edgesKnown: true }} ended selectedNode={null} onSelectNode={vi.fn()} />,
    );
    expect(screen.getByText("1 dependencies")).toBeInTheDocument();

    // The proof goes away — a re-selection, or a graph read that failed. The run's past did not.
    rerender(<WorkOverview model={succeeded} ended selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.queryAllByText(/never verified/i)).toHaveLength(0);
    expect(screen.getByText("Dependency evidence is unavailable in this view")).toBeInTheDocument();
    expect(screen.getAllByText("Dependency evidence is unavailable in this view.")).toHaveLength(2);
  });
});
