import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { WorkOverview, isFirstEntryNode } from "./work-overview";
import type { GraphModel } from "../graph/model";
import type { SubagentReadModel } from "../runtime/subagents";
import type { ClaudeTaskReadModel } from "../runtime/team-tasks";
import type { RunTeamReadModel } from "../runtime/run-team";

const node = (id: string) => ({ id, state: "unknown", touches: 0, lastEventAt: null, history: [], reopened: null });
const model: GraphModel = { nodes: [node("triage"), node("work")], edges: [{id:"link",from:"triage",to:"work",type:"dependency"}], edgesKnown: false, entrypoints: [], rosterDeclared: true, lint: [] };
describe("organized work overview", () => {
  it("shows each recorded MCP transport's latest work, without claiming native chat membership", () => {
    const a = "aaaaaaaaaaaaaaaa", b = "bbbbbbbbbbbbbbbb", c = "cccccccccccccccc";
    const workMessages = [
      { id: "1", sequence: 1, sender: "codex", to: null, replyTo: null, text: "First transport's earlier note", at: null, provenance: "stored" as const, acknowledged: false, transportSession: a },
      { id: "2", sequence: 2, sender: "codex", to: null, replyTo: null, text: "Second transport's note", at: null, provenance: "stored" as const, acknowledged: false, transportSession: b },
      { id: "3", sequence: 3, sender: "codex", to: null, replyTo: null, text: "Third transport's note", at: null, provenance: "stored" as const, acknowledged: false, transportSession: c },
      { id: "4", sequence: 4, sender: "codex", to: null, replyTo: null, text: "First transport's latest note", at: null, provenance: "stored" as const, acknowledged: false, transportSession: a },
      { id: "5", sequence: 5, sender: "claude", to: null, replyTo: null, text: "Distinct HTTP actor note", at: null, provenance: "stored" as const, acknowledged: false },
    ];
    const onSelectAgent = vi.fn();
    render(<WorkOverview model={model} runId="run-a"
      crew={[{ id: "codex", charter: null }, { id: "claude", charter: null }]}
      recordedSessions={[{ actorId: "codex", session: a, sequences: [6] },
        { actorId: "codex", session: b, sequences: [7] }]}
      workMessages={workMessages} selectedNode={null} onSelectNode={vi.fn()} onSelectAgent={onSelectAgent} />);
    const people = screen.getByRole("region", { name: "People and reported work" });
    const cards = within(people).getAllByRole("article");
    expect(cards).toHaveLength(4);
    expect(people).toHaveTextContent("First transport's latest note");
    expect(people).not.toHaveTextContent("First transport's earlier note");
    for (const nonce of [a, b, c]) expect(people).toHaveTextContent(nonce);
    expect(people).toHaveTextContent("Distinct HTTP actor note");
    expect(people).toHaveTextContent("declaration not recorded");
    expect(people).not.toHaveTextContent("Explicitly joined sessions");
    fireEvent.click(within(cards[0]).getByRole("button", { name: "Open actor chat" }));
    expect(onSelectAgent).toHaveBeenCalledWith("claude");
  });

  it("shows typed transport sessions separately from actor identities and verified joins", () => {
    const recordedSessions = ["transport-a", "transport-b", "transport-c"].map((session, index) => ({
      actorId: "agent-a", session, sequences: [index + 1],
    }));
    recordedSessions[0].sequences = [1, 5, 7];
    recordedSessions.push({ actorId: "agent-b", session: "transport-a", sequences: [4] });
    const { rerender } = render(<WorkOverview model={model}
      crew={[{ id: "agent-a", charter: null }, { id: "agent-b", charter: null }]}
      recordedSessions={recordedSessions} selectedNode={null} onSelectNode={vi.fn()} />);
    const sessions = screen.getByRole("group", { name: "Recorded actor and transport sessions" });
    expect(screen.getByText("2 actor IDs")).toBeInTheDocument();
    expect(within(sessions).getByText("4 recorded actor/session pairs")).toBeInTheDocument();
    expect(within(sessions).getAllByText("agent-a")).toHaveLength(3);
    expect(within(sessions).getByText("transport-c")).toBeInTheDocument();
    expect(within(sessions).getByText("Declared at events #1, #5, #7")).toBeInTheDocument();
    expect(within(sessions).getByText(/not native chat identities or activity heartbeats/i)).toBeInTheDocument();
    expect(screen.queryByText("4 joined sessions")).not.toBeInTheDocument();
    rerender(<WorkOverview model={model} recordedSessions={[]} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(within(sessions).getByText(/No typed actor\/session declarations recorded/i)).toBeInTheDocument();
  });
  it("C5 puts explicitly joined sessions in the primary people list", () => {
    const members: RunTeamReadModel["members"] = ["c1", "c2", "c3"].map((sessionId) => ({
      actorId: `codex-session-${sessionId}`, host: "codex", sessionId, joinedAt: null,
      lastAt: null, task: `Task ${sessionId}`, activity: `Update ${sessionId}`,
      reportedState: "working", endedAt: null,
    }));
    members.push({ actorId: "claude-session-a1", host: "claude", sessionId: "a1", joinedAt: null,
      lastAt: null, task: "Review", activity: "Waiting for browser evidence", reportedState: "waiting", endedAt: null });
    const runTeam: RunTeamReadModel = { executionId: "run-1", members, rejected: 0, unavailable: false,
      messages: [{ id: "m1", sender: members[0].actorId, to: members[3].actorId, replyTo: null,
        text: "Please review the view", at: null, sequence: 1, acknowledged: false, acknowledgedAt: null }] };
    render(<WorkOverview model={model} runTeam={runTeam} selectedNode={null} onSelectNode={vi.fn()} />);
    const people = screen.getByRole("region", { name: "People and reported work" });
    expect(within(people).getByText(/Explicitly joined sessions/)).toBeInTheDocument();
    expect(within(people).getAllByText(/Task c[123]/)).toHaveLength(3);
    expect(within(people).getByText("Waiting for browser evidence")).toBeInTheDocument();
    expect(within(people).getAllByRole("button", { name: "Open direct chat" })).toHaveLength(4);
  });
  it("shows a joined actor's verified work update without implying inactivity", () => {
    const actorId = "codex-session-c1";
    const runTeam: RunTeamReadModel = { executionId: "run-1", rejected: 0, unavailable: false,
      messages: [], members: [{ actorId, host: "codex", sessionId: "c1", joinedAt: null,
        lastAt: "2026-09-29T12:00:00Z", task: "Review Studio", activity: "Checking the reply view",
        reportedState: "working", endedAt: null }] };
    render(<WorkOverview model={model} crew={[{ id: actorId, charter: null }]} runTeam={runTeam} selectedNode={null} onSelectNode={vi.fn()} />);
    const card = screen.getByText(actorId, { selector: ".work-primary-people strong" }).closest("article")!;
    expect(card).toHaveTextContent("Checking the reply view");
    expect(card).toHaveTextContent("Reported working");
    expect(card).not.toHaveTextContent("No direct chat report");
    expect(card).not.toHaveTextContent("No node activity yet");
  });
  it("keeps failed team evidence distinct from a verified empty team", () => {
    const empty: RunTeamReadModel = { executionId: "run-1", members: [], messages: [], rejected: 0, unavailable: false };
    const { rerender } = render(<WorkOverview model={model} runTeam={empty} selectedNode={null} onSelectNode={vi.fn()} />);
    const people = screen.getByRole("region", { name: "People and reported work" });
    expect(people).toHaveTextContent("No agent work message is readable yet");
    expect(people).not.toHaveTextContent("could not be verified");

    rerender(<WorkOverview model={model} runTeam={{ ...empty, rejected: 1 }} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(people).toHaveTextContent("1 team record could not be verified");
    expect(people).not.toHaveTextContent("No native sessions have explicitly joined this run");

    rerender(<WorkOverview model={model} runTeam={{ ...empty, members: [{ actorId: "codex-session-c1", host: "codex", sessionId: "c1", joinedAt: null, lastAt: null, task: null, activity: null, reportedState: null, endedAt: null }], rejected: 1 }} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(people).toHaveTextContent("codex-session-c1");
    expect(people).toHaveTextContent("1 team record could not be verified");
  });
  it("shows the declared step and actual model route separately from the recorder", () => {
    const review = {
      ...node("review_browser_evidence"),
      declaredName: "Review browser evidence",
      declaredRole: "evaluator",
      actualExecutor: { kind: "model", routeId: "review-route" },
      resultSource: "model_reply" as const,
      state: "succeeded",
      touches: 1,
      history: [{ sequence: 3, kind: "node_outcome_recorded", nextState: "succeeded", outcome: "succeeded", occurredAt: null, actorId: "system-runtime", actorType: "system", evidence: 1 }],
    };
    render(<WorkOverview model={{ ...model, nodes: [review] }} selectedNode={null} onSelectNode={vi.fn()} />);
    const card = screen.getByRole("button", { name: /Open node review_browser_evidence/ });
    expect(card).toHaveTextContent("Review browser evidence");
    expect(card).toHaveTextContent("Declared role · evaluator");
    expect(card).toHaveTextContent("Model · route review-route");
    expect(card).toHaveTextContent("Recorded by Runtime");
    expect(card).toHaveTextContent("review needed");
    expect(screen.getByText("review_browser_evidence · reply received · review needed")).toBeInTheDocument();
  });

  it("shows a finished model call as unverified and labels the system actor as recorder", () => {
    const review = {
      ...node("review_browser_evidence"),
      state: "succeeded",
      resultSource: "model_reply" as const,
      touches: 1,
      history: [{
        sequence: 17, kind: "node_outcome_recorded", nextState: "succeeded",
        outcome: "succeeded", occurredAt: "2026-09-26T02:45:00Z",
        actorId: "system-runtime", actorType: "system", evidence: 3,
      }],
    };
    render(<WorkOverview model={{ ...model, nodes: [review] }} selectedNode={null} onSelectNode={vi.fn()} />);
    const card = screen.getByRole("button", { name: /review_browser_evidence/ });
    expect(card).toHaveTextContent("Model call · model identity not recorded");
    expect(card).toHaveTextContent("Reply received · acceptance not verified");
    expect(card).toHaveTextContent("Recorded by Runtime");
    expect(card).toHaveTextContent("review needed");
    expect(screen.getByRole("note")).toHaveTextContent("A finished step does not prove its goal passed");
  });
  /** Observable contract: the operator can see the governed proposal's status, responsible actor,
   * and next action in the actual Work view. This catches the practical defect of exposing the
   * data only in the graph fold while leaving the primary task surface unusable. */
  it("shows a governed proposal owner and review action", () => {
    const ghost = {
      ...node("review"),
      state: "ghost",
      touches: 1,
      proposal: { draftId: "draft-1", digest: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", evidenceId: null, status: "proposed" as const, reason: null },
      assignedActor: { type: "agent", id: "reviewer" },
      history: [{ sequence: 4, kind: "ghost_node_proposed", nextState: "ghost", outcome: "proposed", occurredAt: null, actorId: "runtime", actorType: "system", evidence: 0 }],
    };
    render(<WorkOverview model={{ ...model, nodes: [ghost] }} selectedNode={null} onSelectNode={vi.fn()} />);
    const card = screen.getByRole("button", { name: /Open node review/ });
    expect(card).toHaveTextContent("Proposal proposed");
    expect(card).toHaveTextContent("Responsible actor");
    expect(card).toHaveTextContent("reviewer");
    expect(card).toHaveTextContent("assignment is separate from the Runtime recorder");
  });
  /** The technical node record keeps assignment separate from the Runtime recorder. */
  it("shows assigned node activity even when Runtime recorded the outcome", () => {
    const assigned = {
      ...node("implementation"),
      state: "succeeded" as const,
      assignedActor: { type: "agent", id: "builder" },
      touches: 1,
      history: [{ sequence: 12, kind: "node_outcome_recorded", nextState: "succeeded", outcome: "succeeded", occurredAt: "2026-09-28T12:00:00Z", actorId: "system-runtime", actorType: "system", evidence: 1 }],
    };
    render(<WorkOverview model={{ ...model, nodes: [assigned] }} crew={[{ id: "builder", charter: null }]} selectedNode={null} onSelectNode={vi.fn()} />);
    const step = screen.getByRole("button", { name: /Open node implementation/ });
    expect(step).toHaveTextContent("builder");
    expect(step).toHaveTextContent("Recorded by Runtime");
    expect(screen.getByRole("region", { name: "People and reported work" })).not.toHaveTextContent("builder");
  });
  it("keeps a typed verified step distinct from an unverified model reply", () => {
    const verified = {
      ...node("review_browser_evidence"),
      state: "succeeded",
      resultSource: "gate_verdict" as const,
      verificationEventSequence: 18,
    };
    render(<WorkOverview model={{ ...model, nodes: [verified] }} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByText("review_browser_evidence · succeeded")).toBeInTheDocument();
  });
  it("retains incomplete-roster and disagreement evidence in the default view", () => {
    render(<WorkOverview model={{...model,rosterDeclared:false,lint:[{kind:"done-without-evidence",detail:"Completion has no evidence",sequence:8}]}} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByText("2 graph nodes seen so far")).toBeInTheDocument();
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
    expect(screen.getByRole("region", { name: "People and reported work" })).toHaveTextContent("No agent work message is readable yet");
    fireEvent.click(screen.getByRole("button",{name:/codex \+ helper/}));
    expect(select).toHaveBeenCalledWith("pair");
  });
  it("shows recorded messages even while no graph node is active", () => {
    render(<WorkOverview model={model} crew={[{id:"codex",charter:null,lastAt:"2026-09-25T17:00:00Z"}]} talks={[{key:"room",label:"everyone",participants:["codex"],count:1,lastAt:"2026-09-25T17:00:00Z",preview:"Checking the failing flow"}]} activity={[{sequence:39,actorId:"codex",occurredAt:"2026-09-25T17:00:00Z",text:"Checking the failing flow"}]} workMessages={[{id:"m39",sequence:39,sender:"codex",to:null,replyTo:null,text:"Checking the failing flow",at:"2026-09-25T17:00:00Z",provenance:"stored",acknowledged:false}]} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByText("0 active graph nodes")).toBeInTheDocument();
    const activity = screen.getByRole("region", {name:"Recent recorded activity"});
    expect(activity).toHaveTextContent("codex");
    expect(activity).toHaveTextContent("event #39");
    expect(activity).toHaveTextContent("Checking the failing flow");
    expect(screen.getByRole("region", {name:"People and reported work"})).toHaveTextContent("Checking the failing flow");
    expect(screen.getByRole("region", {name:"People and reported work"})).toHaveTextContent("session unverified");
    expect(screen.getByText("Checking the failing flow", {selector:".work-talk-preview"})).toBeInTheDocument();
  });
  it("keeps the report pending while its text has not opened", () => {
    render(<WorkOverview model={model} activity={[{sequence:40,actorId:"reviewer",occurredAt:null,text:null}]} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByRole("region", {name:"Recent recorded activity"})).toHaveTextContent("Report text has not opened yet");
  });
  it("identifies the active project and chooses the newest recorded update by event sequence", () => {
    render(<WorkOverview
      model={model}
      projectName="GraphHelm"
      projectPath="fixtures/project"
      latestRecordedUpdate={{ sequence: 21, occurredAt: "2026-09-26T16:00:00Z" }}
      activity={[
        { sequence: 12, actorId: "older", occurredAt: "2026-09-26T17:00:00Z", text: "Older update" },
        { sequence: 14, actorId: "newer", occurredAt: "2026-09-26T16:00:00Z", text: "Newest journal update" },
      ]}
      selectedNode={null}
      onSelectNode={vi.fn()}
    />);
    const identity = screen.getByRole("region", { name: "Active workspace" });
    expect(identity).toHaveTextContent("GraphHelm");
    expect(identity).toHaveTextContent("fixtures/project");
    expect(identity).toHaveTextContent("Event #21 · recorded");
    expect(screen.getByRole("region", { name: "Where this run stands" })).toHaveTextContent("newer");
    expect(screen.getByText("Active workspace")).toBeInTheDocument();
  });
  it("states when the project folder and recorded timestamp are unavailable", () => {
    render(<WorkOverview model={model} latestRecordedUpdate={{ sequence: 9, occurredAt: null }} selectedNode={null} onSelectNode={vi.fn()} />);
    const identity = screen.getByRole("region", { name: "Active workspace" });
    expect(identity).toHaveTextContent("Project name unavailable");
    expect(identity).toHaveTextContent("Project folder unavailable");
    expect(identity).toHaveTextContent("Event #9 · timestamp unavailable");
    expect(screen.queryByText("Live workspace")).not.toBeInTheDocument();
  });
  it("shows where a one-node run stands and each agent's latest recorded report", () => {
    const selectAgent = vi.fn();
    render(<WorkOverview
      model={{ ...model, nodes: [node("start")] }}
      crew={[{ id: "builder", charter: null }, { id: "reviewer", charter: null }]}
      activity={[{ sequence: 20, actorId: "reviewer", occurredAt: "2026-09-25T17:02:00Z", text: "Checking the branch" }]}
      workMessages={[{id:"m11",sequence:11,sender:"builder",to:null,replyTo:null,text:"Implementing the issue",at:"2026-09-25T17:00:00Z",provenance:"stored",acknowledged:false},
        {id:"m20",sequence:20,sender:"reviewer",to:null,replyTo:null,text:"Checking the branch",at:"2026-09-25T17:02:00Z",provenance:"stored",acknowledged:false}]}
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
    const builder = screen.getByText("builder", {selector: ".work-primary-people strong"}).closest("article")!;
    expect(builder).toHaveTextContent("Implementing the issue");
    expect(selectAgent).not.toHaveBeenCalled();
    fireEvent.click(within(builder).getByRole("button", {name:"Open actor chat"}));
    expect(selectAgent).toHaveBeenCalledWith("builder");
    expect(snapshot).not.toHaveTextContent("Running");
  });
  it("shows the next step before the activity and opens its action", () => {
    const act = vi.fn();
    render(<WorkOverview model={model} selectedNode={null} onSelectNode={vi.fn()} nextAction={{label:"Send direction",detail:"No question is visible yet."}} onNextAction={act} />);
    const action = screen.getByRole("region", {name:"Owner decision"});
    expect(action).toHaveTextContent("No question is visible yet.");
    fireEvent.click(within(action).getByRole("button", {name:"Send direction"}));
    expect(act).toHaveBeenCalledOnce();
  });
});

/** New issue #86 observers. Each test checks a visible operator contract that the prior view did
 * not cover: state-first Now, event/report separation, honest flat team fallback, and completion
 * that remains unverified. */
describe("compact run handoff", () => {
  it("puts recorded running and queued nodes first and keeps them selectable", () => {
    const select = vi.fn();
    const queued = { ...node("queued-step"), state: "queued" as const };
    const done = { ...node("finished-step"), state: "succeeded" as const };
    render(<WorkOverview model={{ ...model, nodes: [done, queued] }} runStatus="running" selectedNode={null} onSelectNode={select} />);
    const now = screen.getByRole("region", { name: "Now, last, and next" });
    expect(now).toHaveTextContent("running");
    expect(now).toHaveTextContent("queued-step");
    const cards = screen.getAllByRole("button", { name: /Open node/ });
    expect(cards[0]).toHaveTextContent("queued-step");
    fireEvent.click(cards[0]);
    expect(select).toHaveBeenCalledWith("queued-step");
  });

  it("shows the latest event by sequence and keeps the latest chat report separate", () => {
    render(<WorkOverview
      model={model}
      latestEvent={{ sequence: 44, kind: "node_outcome_recorded", actorId: "system-runtime", actorType: "system", occurredAt: "2026-09-29T12:00:00Z" }}
      activity={[{ sequence: 41, actorId: "agent-old", occurredAt: null, text: "Older" }, { sequence: 43, actorId: "agent-new", occurredAt: null, text: "Latest report" }]}
      selectedNode={null}
      onSelectNode={vi.fn()}
    />);
    const now = screen.getByRole("region", { name: "Now, last, and next" });
    expect(now).toHaveTextContent("Event #44");
    expect(now).toHaveTextContent("system-runtime");
    expect(screen.getByRole("region", { name: "Where this run stands" })).toHaveTextContent("agent-new");
    expect(screen.getByRole("region", { name: "Where this run stands" })).toHaveTextContent("Latest report");
  });

  it("states that session relationships are unavailable without inventing parentage", () => {
    render(<WorkOverview model={model} crew={[{ id: "builder", charter: null }]} selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByRole("heading", { name: "Team" })).toBeInTheDocument();
    expect(screen.getByText("Checking recorded session links · showing a flat team list.")).toBeInTheDocument();
    expect(screen.queryByText(/parent|child|reports to/i)).not.toBeInTheDocument();
  });

  it("keeps a completed node visibly unverified", () => {
    const completed = { ...node("finished"), state: "succeeded" as const, resultSource: "model_reply" as const, touches: 1 };
    render(<WorkOverview model={{ ...model, nodes: [completed] }} runStatus="completed" selectedNode={null} onSelectNode={vi.fn()} />);
    expect(screen.getByRole("button", { name: /Open node finished/ })).toHaveTextContent("review needed");
    expect(screen.getByRole("note")).toHaveTextContent("does not prove its goal passed");
  });
  it("opens the first unverified result from the suggested next action", () => {
    const select = vi.fn();
    const completed = { ...node("finished"), state: "succeeded" as const, resultSource: "model_reply" as const };
    render(<WorkOverview model={{ ...model, nodes: [completed] }} selectedNode={null} onSelectNode={select} />);
    fireEvent.click(screen.getByRole("button", { name: "Review first unverified node" }));
    expect(select).toHaveBeenCalledWith("finished");
    expect(screen.getByRole("region", { name: "Now, last, and next" })).toHaveTextContent("Suggested: review node results");
  });
  it("shows only observed host session membership and no invented result", () => {
    const subagents: SubagentReadModel = {
      executionId: "run-one", rejected: 0, latestByChild: {}, relationships: [{
        executionId: "run-one", parentSessionId: "parent-1", childAgentId: "child-1",
        agentType: "worker", declaredNodeId: null, sourceId: "codex-session-parent-1",
        sourceActorId: "codex-session-parent-1", sourceActorType: "agent",
        startedSequence: 10, startedAt: "2026-09-29T12:00:00Z", startedEvidenceId: "ev-start",
        stoppedSequence: null, stoppedAt: null, stoppedEvidenceId: null, lastChildEvent: null, phase: "started",
      }],
    };
    render(<WorkOverview model={model} subagents={subagents} selectedNode={null} onSelectNode={vi.fn()} />);
    const tree = screen.getByRole("group", { name: "Observed host sessions" });
    expect(tree).toHaveTextContent("Recorded host session parent-1 · 1 subagent");
    expect(tree).toHaveTextContent("child-1");
    expect(tree).toHaveTextContent("stop not recorded");
    fireEvent.click(within(tree).getByText("child-1"));
    expect(tree).toHaveTextContent("Start: event #10");
    expect(tree).toHaveTextContent("No direct child action recorded");
    expect(tree).toHaveTextContent("Task result and acceptance not verified here");
    expect(tree).not.toHaveTextContent("completed");
  });
  it("shows Claude task lifecycle separately and never calls completion acceptance", () => {
    const claudeTasks: ClaudeTaskReadModel = { executionId: "run-one", rejected: 0, tasks: [{
      executionId: "run-one", nativeTaskId: "task-7", taskSubject: "Review the checkout flow", createdByTeammateName: "planner", completedByTeammateName: "reviewer",
      sourceId: "claude-session-session-1", parentSessionId: "session-1", createdSequence: 11, createdAt: null, createdEvidenceId: "ev-created",
      completedSequence: 15, completedAt: null, completedEvidenceId: "ev-completed",
    }] };
    render(<WorkOverview model={model} claudeTasks={claudeTasks} selectedNode={null} onSelectNode={vi.fn()} />);
    const region = screen.getByRole("group", { name: "Observed host sessions" });
    expect(region).toHaveTextContent("Review the checkout flow");
    expect(region).toHaveTextContent("created by teammate: planner");
    expect(region).toHaveTextContent("completed by teammate: reviewer · output review not observed");
    expect(region).toHaveTextContent("not proof of assignment");
    expect(region).toHaveTextContent("Recorded host session session-1 · 1 task");
    expect(region).toHaveTextContent("not linked to a particular subagent");
    expect(region).not.toHaveTextContent("Output accepted");
  });
  it("labels missing task creator and completer names as unknown", () => {
    const claudeTasks: ClaudeTaskReadModel = { executionId: "run-one", rejected: 0, tasks: [{
      executionId: "run-one", nativeTaskId: "task-8", taskSubject: "Check the release", createdByTeammateName: null, completedByTeammateName: null,
      sourceId: "claude-session-session-1", parentSessionId: "session-1", createdSequence: 21, createdAt: null, createdEvidenceId: "ev-created",
      completedSequence: 23, completedAt: null, completedEvidenceId: "ev-completed",
    }] };
    render(<WorkOverview model={model} claudeTasks={claudeTasks} selectedNode={null} onSelectNode={vi.fn()} />);
    const region = screen.getByRole("group", { name: "Observed host sessions" });
    expect(region).toHaveTextContent("created by teammate: unknown");
    expect(region).toHaveTextContent("completed by teammate: unknown");
  });
  it("groups task records only with the matching host identity and keeps task-only sessions visible", () => {
    const task = {
      executionId: "run-one", nativeTaskId: "task-9", taskSubject: "Check docs", createdByTeammateName: "planner", completedByTeammateName: null,
      sourceId: "claude-session-session-1", parentSessionId: "session-1", createdSequence: 31, createdAt: null, createdEvidenceId: "ev-created",
      completedSequence: null, completedAt: null, completedEvidenceId: null,
    };
    const subagents: SubagentReadModel = { executionId: "run-one", rejected: 0, latestByChild: {}, relationships: [
      { executionId: "run-one", parentSessionId: "session-1", childAgentId: "claude-child", agentType: "worker", declaredNodeId: null, sourceId: "claude-session-session-1", sourceActorId: "claude-session-session-1", sourceActorType: "agent", startedSequence: 10, startedAt: null, startedEvidenceId: "ev-start", stoppedSequence: null, stoppedAt: null, stoppedEvidenceId: null, lastChildEvent: null, phase: "started" },
      { executionId: "run-one", parentSessionId: "session-1", childAgentId: "codex-child", agentType: "worker", declaredNodeId: null, sourceId: "codex-session-session-1", sourceActorId: "codex-session-session-1", sourceActorType: "agent", startedSequence: 11, startedAt: null, startedEvidenceId: "ev-codex", stoppedSequence: null, stoppedAt: null, stoppedEvidenceId: null, lastChildEvent: null, phase: "started" },
    ] };
    render(<WorkOverview model={model} subagents={subagents} claudeTasks={{ executionId: "run-one", rejected: 0, tasks: [task] }} selectedNode={null} onSelectNode={vi.fn()} />);
    const sessions = screen.getByRole("group", { name: "Observed host sessions" });
    const summaries = within(sessions).getAllByText(/Recorded host session session-1 · 1 subagent/);
    const claude = summaries.find((summary) => summary.textContent?.includes("1 task"))!.closest("details")!;
    expect(claude).toHaveTextContent("claude-child");
    expect(claude).toHaveTextContent("Check docs");
    expect(claude).not.toHaveTextContent("codex-child");
    const codex = summaries.find((summary) => !summary.textContent?.includes("1 task"))!.closest("details")!;
    expect(codex).toHaveTextContent("codex-child");
    expect(codex).not.toHaveTextContent("Check docs");
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
