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
