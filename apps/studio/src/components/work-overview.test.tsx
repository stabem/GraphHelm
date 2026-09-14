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
