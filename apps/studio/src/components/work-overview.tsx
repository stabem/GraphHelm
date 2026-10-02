import {
  Activity,
  ArrowDown,
  ArrowUp,
  CircleDot,
  Clock3,
  MessageCircle,
  Network,
  Users,
} from "lucide-react";

import type { GraphModel, GraphNode } from "../graph/model";
import { moodOf, nodeResult, nodeStatusLabel, splitLint } from "../graph/model";
import { ago, fullInstant, hueOf, initialOf, readable } from "./format";
import type { SubagentReadModel } from "../runtime/subagents";
import type { ClaudeTaskReadModel } from "../runtime/team-tasks";
import type { RunTeamReadModel } from "../runtime/run-team";
import type { RecordedActorSession } from "../runtime/session";
import type { WorkMessage } from "../runtime/work-conversation";

type CrewMember = { id: string; charter: string | null; lastAt?: string | null };
type Talk = { key: string; label: string; participants: string[]; count: number; lastAt: string | null; preview?: string | null };
type RecordedActivity = { sequence: number; actorId: string | null; occurredAt: string | null; text: string | null };
type AgentReport = { sequence: number; occurredAt: string | null; text: string | null };
type RecordedUpdate = { sequence: number; occurredAt: string | null };
type LatestEvent = { sequence: number; kind: string; actorId: string | null; actorType: string | null; occurredAt: string | null };

export interface WorkOverviewProps {
  model: GraphModel;
  projectName?: string | null;
  projectPath?: string | null;
  latestRecordedUpdate?: RecordedUpdate | null;
  latestEvent?: LatestEvent | null;
  subagents?: SubagentReadModel | null;
  runTeam?: RunTeamReadModel | null;
  claudeTasks?: ClaudeTaskReadModel | null;
  crew?: CrewMember[];
  recordedSessions?: RecordedActorSession[];
  workMessages?: WorkMessage[];
  talks?: Talk[];
  activity?: RecordedActivity[];
  agentReports?: Record<string, AgentReport>;
  runStatus?: string | null;
  attention?: string | null;
  nextAction?: { label: string; detail: string } | null;
  replyGuidance?: string | null;
  onNextAction?: () => void;
  selectedNode: string | null;
  onSelectNode: (nodeId: string | null) => void;
  selectedAgent?: string | null;
  onSelectAgent?: (agentId: string | null) => void;
  selectedTalk?: string | null;
  onSelectTalk?: (talkKey: string | null) => void;
  runId?: string;
  /** The run's objective from its briefing (#1077), shown on the ENTRY node's card - the node
   * whose objective it is. A node's own text is sealed content the log never carries, so this
   * is the only line that can say what the first node was asked to do. */
  objective?: string | null;
  /** The run was started under the fixture executor (#1064): its log findings are expected and
   * read as a neutral note, not as an alarm (#1083 F9). */
  demonstration?: boolean;
  /** #1098 D5: the run reached a terminal status. "Awaiting evidence" is a promise that something
   * is still on its way; on an ended run nothing is, and a finished run read as unfinished. The
   * same absence, stated so it stays true for an ended run — about THIS VIEW, never about the
   * run's past (PR #1167 review, P2). */
  ended?: boolean;
}

/** Whether a node is one of the run's entrypoints. Proven by the verified topology when there
 * is one; otherwise only the one case that is a fact without it: a declared roster of exactly
 * one node, which the graph schema requires to be an entrypoint. Never guessed from order. */
export function isEntryNode(model: GraphModel, nodeId: string): boolean {
  if (model.entrypoints.length > 0) return model.entrypoints.includes(nodeId);
  return model.rosterDeclared && model.nodes.length === 1 && model.nodes[0].id === nodeId;
}

/** Whether a node is the entrypoint the briefing's objective BELONGS to: #1071 records the
 * objective of the first node in `spec.entrypoints` order (schemas/CHANGELOG.md), and the
 * verified topology relays that order. A second entrypoint is an entry node and is NOT owed
 * the first one's request (PR #1079 review, P2). */
export function isFirstEntryNode(model: GraphModel, nodeId: string): boolean {
  if (model.entrypoints.length > 0) return model.entrypoints[0] === nodeId;
  return isEntryNode(model, nodeId);
}

function latestNodeEvent(node: GraphNode) {
  return node.history.at(-1);
}

function statusClass(node: GraphNode): string {
  if (nodeStatusLabel(node) !== null) return "work-status work-status-review";
  return `work-status work-status-${moodOf(node.state)}`;
}

export function WorkOverview({
  model,
  projectName = null,
  projectPath = null,
  latestRecordedUpdate = null,
  latestEvent = null,
  subagents = null,
  runTeam = null,
  claudeTasks = null,
  crew = [],
  recordedSessions = [],
  workMessages = [],
  talks = [],
  activity = [],
  agentReports = {},
  runStatus = null,
  attention = null,
  nextAction = null,
  replyGuidance = null,
  onNextAction,
  selectedNode,
  onSelectNode,
  selectedAgent = null,
  onSelectAgent,
  selectedTalk = null,
  onSelectTalk,
  runId,
  objective = null,
  demonstration = false,
  ended = false,
}: WorkOverviewProps) {
  /**
   * The one sentence for "no verified topology is available here", written once so the heading
   * and every card cannot drift apart.
   *
   * It speaks about THE VIEW, not about the run's history. `edgesKnown` is a property of what
   * this page currently holds: it is false when no topology has been verified yet, AND when a
   * selection dropped the proof with the board, AND when a later graph read failed. A run whose
   * topology WAS verified moments ago has that same false bit, so "were never verified" would be
   * a claim about the past inferred from present evidence that cannot carry it (PR #1167 review,
   * P2). A run still going keeps "awaiting": for it the evidence really can still arrive.
   */
  const unverified = ended
    ? "Dependency evidence is unavailable in this view"
    : "Dependencies awaiting evidence";
  const activeNodes = model.nodes.filter((node) => ["running", "queued", "linting"].includes(node.state)).length;
  const attentionNodes = model.nodes.filter((node) => ["blocked", "failed", "waiting_input", "waiting_capacity"].includes(node.state)).length;
  const singleStep = model.nodes.length === 1 ? model.nodes[0] : null;
  const singleStepStatus = singleStep === null ? null : nodeStatusLabel(singleStep);
  const singleStepText = singleStep === null ? null : singleStepStatus === "review needed"
    ? singleStep.resultSource === "model_reply" ? "reply received · review needed" : "finished · review needed"
    : singleStepStatus ?? readable(singleStep.state);
  const nodeIds = new Set(model.nodes.map((node) => node.id));
  const lint = splitLint(model.lint, demonstration);
  const latestReport = activity.reduce<typeof activity[number] | null>(
    (latest, item) => latest === null || item.sequence > latest.sequence ? item : latest,
    null,
  );
  const recordedUpdate = latestRecordedUpdate === null
    ? "No recorded update yet"
    : latestRecordedUpdate.occurredAt === null
      ? `Event #${latestRecordedUpdate.sequence} · timestamp unavailable`
      : Number.isNaN(new Date(latestRecordedUpdate.occurredAt).valueOf())
        ? `Event #${latestRecordedUpdate.sequence} · timestamp invalid`
        : `Event #${latestRecordedUpdate.sequence} · recorded ${fullInstant(latestRecordedUpdate.occurredAt)}`;
  const joinedMembers = runTeam?.unavailable ? [] : runTeam?.members ?? [];
  const knownActors = new Set(crew.map((agent) => agent.id));
  const recentSpeakers = [...workMessages].reverse().reduce<string[]>((actors, message) => {
    if (knownActors.has(message.sender) && !actors.includes(message.sender)) actors.push(message.sender);
    return actors;
  }, []).slice(0, 6);
  const orderedNodes = [...model.nodes].sort((left, right) => {
    const leftActive = ["running", "queued", "linting"].includes(left.state) ? 0 : 1;
    const rightActive = ["running", "queued", "linting"].includes(right.state) ? 0 : 1;
    return leftActive - rightActive;
  });
  const activeNodeNames = model.nodes.filter((node) => ["running", "queued", "linting"].includes(node.state)).map((node) => node.id);
  const reviewNode = model.nodes.find((node) => nodeStatusLabel(node) === "review needed");
  const sessions = new Map<string, { sourceId: string; parentSessionId: string; children: SubagentReadModel["relationships"]; tasks: NonNullable<WorkOverviewProps["claudeTasks"]>["tasks"] }>();
  for (const child of subagents?.relationships ?? []) {
    const key = `${child.sourceId}\u0000${child.parentSessionId}`;
    const group = sessions.get(key) ?? { sourceId: child.sourceId, parentSessionId: child.parentSessionId, children: [], tasks: [] };
    group.children.push(child);
    sessions.set(key, group);
  }
  for (const task of claudeTasks?.tasks ?? []) {
    const key = `${task.sourceId}\u0000${task.parentSessionId}`;
    const group = sessions.get(key) ?? { sourceId: task.sourceId, parentSessionId: task.parentSessionId, children: [], tasks: [] };
    group.tasks.push(task);
    sessions.set(key, group);
  }
  const jumpToTeam = () => document.getElementById("work-team")?.scrollIntoView?.({ behavior: "smooth", block: "start" });

  return (
    <main className="work-overview" aria-label="Work overview">
      <header className="work-primary-header">
        <span>Selected run</span>
        <h1>{projectName ?? "Project"}</h1>
        {runId && <small>Run {runId}</small>}
      </header>
      {nextAction && <section className="work-next-action" aria-label="Owner decision">
        <div><span>Needs a decision</span><strong>{nextAction.label}</strong><p>{nextAction.detail}</p>{replyGuidance && <p>{replyGuidance}</p>}</div>
        <button type="button" onClick={onNextAction}>{nextAction.label}</button>
      </section>}
      <section className="work-primary-roster" aria-label="People and reported work">
        <div className="work-section-heading"><div><Users aria-hidden="true" size={17} /><h2>People</h2></div></div>
        {joinedMembers.length > 0 ? <>
          <p className="work-roster-note">Explicitly joined sessions. Work state is what each session last reported, not a live heartbeat.</p>
          <div className="work-primary-people">{joinedMembers.map((member) => <article key={member.actorId}>
            <strong>{member.actorId}</strong><small>{member.host} session</small>
            <span>{member.task ?? "Task not reported"}</span>
            <p>{member.activity ?? "No work update reported"}</p>
            <small>{member.reportedState ? `Reported ${member.reportedState}` : "No state reported"} · {ago(member.lastAt)}</small>
            <button type="button" onClick={() => onSelectAgent?.(member.actorId)}>Open direct chat</button>
          </article>)}</div>
        </> : recentSpeakers.length > 0 ? <>
          <p className="work-roster-note">Observed actor IDs from recorded messages. A single ID may cover several native chats; no native joins are verified here.</p>
          <div className="work-primary-people">{recentSpeakers.map((actorId) => {
            const latest = [...workMessages].reverse().find((message) => message.sender === actorId)!;
            return <article key={actorId}>
              <strong>{actorId}</strong><small>Observed actor · session unverified</small>
              <span>Task not reported</span><p>{latest.text}</p>
              <small>Message stored · {ago(latest.at)}</small>
              <button type="button" onClick={() => onSelectAgent?.(actorId)}>Open direct chat</button>
            </article>;
          })}</div>
        </> : <p className="work-empty">No agent work message is readable yet. Recorded identities are available in Details.</p>}
        {runTeam && runTeam.rejected > 0 && <p className="work-caution" role="note">{runTeam.rejected} team record{runTeam.rejected === 1 ? "" : "s"} could not be verified; membership may be incomplete.</p>}
      </section>
      <details className="work-technical-details"><summary>Details: graph, records, and transport IDs</summary>
      <header className="work-header">
        <div className="work-heading">
          <span className="work-kicker"><Activity aria-hidden="true" size={16} /> Active workspace</span>
          <h1>Work overview</h1>
          {runId && <span className="work-run">Run {runId}</span>}
        </div>
        <div className="work-counts" aria-label="Workspace counts">
          <span><CircleDot aria-hidden="true" size={15} /> {model.nodes.length} graph node{model.nodes.length === 1 ? "" : "s"}{!model.rosterDeclared && " seen so far"}</span>
          <span><Activity aria-hidden="true" size={15} /> {activeNodes} active graph node{activeNodes === 1 ? "" : "s"}</span>
          <span><Users aria-hidden="true" size={15} /> {runTeam && runTeam.members.length > 0 ? `${runTeam.members.length} joined sessions` : `${crew.length} agent identities`}</span>
          {subagents && subagents.relationships.length > 0 && <button type="button" className="work-count-link" onClick={jumpToTeam}><Users aria-hidden="true" size={15} /> {subagents.relationships.length} observed subagent{subagents.relationships.length === 1 ? "" : "s"} · open team</button>}
          <span><MessageCircle aria-hidden="true" size={15} /> {runTeam && runTeam.members.length > 0 ? `${runTeam.messages.length} team message${runTeam.messages.length === 1 ? "" : "s"}` : `${talks.length} conversations`}</span>
          {attentionNodes > 0 && <span className="work-count-attention">{attentionNodes} needs attention</span>}
        </div>
      </header>

      <section className="work-now-strip" aria-label="Now, last, and next">
        <div><span>Now</span><strong>{runStatus ? readable(runStatus) : "Run state unavailable"}</strong>{activeNodeNames.length > 0 ? <small><button type="button" className="work-inline-link" onClick={() => onSelectNode(activeNodeNames[0])}>Open {activeNodeNames[0]}</button>{activeNodeNames.length > 1 ? ` · ${activeNodeNames.length - 1} more active` : ""}</small> : <small>No active node recorded</small>}</div>
        <div><span>Last</span><strong>{latestEvent ? `Event #${latestEvent.sequence} · ${readable(latestEvent.kind)}` : "No latest event recorded"}</strong><small>{latestEvent ? `${latestEvent.actorId ?? "Unknown actor"} · ${ago(latestEvent.occurredAt)}` : "Last chat report is shown below"}</small></div>
        <div><span>Next</span><strong>{nextAction?.label ?? (reviewNode ? "Suggested: review node results" : "No next action recorded")}</strong><small>{nextAction?.detail ?? (reviewNode ? <button type="button" className="work-inline-link" aria-label="Review first unverified node" onClick={() => onSelectNode(reviewNode.id)}>Open {reviewNode.id} · acceptance unverified</button> : "The Runtime has recorded no owner action.")}</small></div>
      </section>

      <section className="work-identity" aria-label="Active workspace">
        <div><span>Project</span><strong>{projectName ?? "Project name unavailable"}</strong></div>
        <div><span>Project folder</span><strong>{projectPath ?? "Project folder unavailable"}</strong></div>
        <div><span>Latest recorded update</span><strong>{recordedUpdate}</strong></div>
      </section>

      <section className="work-snapshot" aria-label="Where this run stands">
        <div className="work-section-heading"><div><Activity aria-hidden="true" size={17} /><h2>Where this run stands</h2></div><span>From the Runtime record</span></div>
        <div className="work-snapshot-grid">
          <div><span>Run state</span><strong>{runStatus ? readable(runStatus) : "State unavailable"}</strong>{runStatus === "running" && activeNodes === 0 && attentionNodes > 0 && <small>No graph node is active; a step needs attention</small>}{runStatus === "completed" && model.nodes.some((node) => nodeStatusLabel(node) === "review needed") && <small>Node results still need review</small>}</div>
          <div><span>Graph step</span><strong>{singleStep ? `${singleStep.id} · ${singleStepText}` : `${model.nodes.length} declared nodes · ${activeNodes} active`}</strong></div>
          <div><span>Last direct chat report</span><strong>{latestReport ? `${latestReport.actorId ?? "Unknown actor"} · ${ago(latestReport.occurredAt)}` : "No direct chat report recorded"}</strong></div>
        </div>
        {latestReport?.text && <p className="work-snapshot-report">{latestReport.text}</p>}
        {model.rosterDeclared && model.nodes.length === 1 && <p className="work-snapshot-note">This run declares one graph node. Agent reports below show work inside the run; they are not extra graph steps or proof that the node is done.</p>}
      </section>

      {!model.rosterDeclared && <p className="work-caution">The complete node roster has not been read yet.</p>}
      {/* #1083 F9: on a DEMONSTRATION run (fixture executor) a settled node with no evidence is
        * expected - outcomes came from a fixture file - and an amber "needs attention" banner for
        * it was an alarm on a run that needs nothing. ONLY that kind is downgraded (Codex on PR
        * #1091): a reopened settled node or an orphan edge is a real disagreement on any run and
        * keeps the attention banner, counted on its own. */}
      {lint.expected.length > 0 && <section className="work-note" aria-label="Log notes on a demonstration run"><details><summary>Demonstration run · {lint.expected.length} log note{lint.expected.length === 1 ? "" : "s"}</summary><p>Outcomes on this run were supplied by a fixture file, not produced by a model or a tool, so the log holds no evidence for them. These notes are expected here.</p><ul>{lint.expected.map((finding, index) => <li key={`${finding.kind}-${finding.sequence}-${index}`}>{finding.detail}{finding.sequence !== null && <span> · event #{finding.sequence}</span>}</li>)}</ul></details></section>}
      {lint.disagreements.length > 0 && <section className="work-caution" aria-label="Disagreements in the event log"><details><summary>Evidence needs attention · {lint.disagreements.length} finding{lint.disagreements.length === 1 ? "" : "s"}</summary><ul>{lint.disagreements.map((finding, index) => <li key={`${finding.kind}-${finding.sequence}-${index}`}>{finding.detail}{finding.sequence !== null && <span> · event #{finding.sequence}</span>}</li>)}</ul></details></section>}

      <div className="work-layout">
        <aside className="work-sidebar" aria-label="Collaboration">
          <section className="work-section" id="work-team">
            <div className="work-section-heading">
              <div><Users aria-hidden="true" size={17} /><h2>Team</h2></div>
              <span>{crew.length} actor IDs</span>
            </div>
            <div className="work-actor-session-list" role="group" aria-label="Recorded actor and transport sessions">
              <strong>Recorded actor and transport sessions</strong>
              <span>{recordedSessions.length} recorded actor/session pairs</span>
              <p>These are transport declarations, not native chat identities or activity heartbeats.</p>
              {recordedSessions.length === 0
                ? <p>No typed actor/session declarations recorded for this run.</p>
                : <ul>{recordedSessions.map((entry) => <li key={JSON.stringify([entry.actorId, entry.session])}>
                  <span>{entry.actorId}</span><code>{entry.session}</code>
                  <small>Declared at {entry.sequences.length === 1 ? "event" : "events"} {entry.sequences.map((sequence) => `#${sequence}`).join(", ")}</small>
                </li>)}</ul>}
            </div>
            {sessions.size > 0 && <div className="work-session-list" role="group" aria-label="Observed host sessions">
              {[...sessions].map(([key, session]) => <details className="work-session" key={key} open>
                <summary>Recorded host session {session.parentSessionId}{session.children.length > 0 ? ` · ${session.children.length} subagent${session.children.length === 1 ? "" : "s"}` : ""}{session.tasks.length > 0 ? ` · ${session.tasks.length} task${session.tasks.length === 1 ? "" : "s"}` : ""}</summary>
                <small>Source: {session.sourceId}.{session.children.length > 0 ? " The record does not identify who directly delegated a nested task." : ""}</small>
                {session.children.length > 0 && <ul>{session.children.map((child) => <li key={child.childAgentId}><details className="work-child-details">
                  <summary><strong>{child.childAgentId}</strong><span>Role: {child.agentType} · {child.phase === "stopped" ? "stop observed" : "stop not recorded"}</span></summary>
                  <small>Start: event #{child.startedSequence} · {ago(child.startedAt)} · evidence {child.startedEvidenceId}</small>
                  {child.stoppedSequence !== null && <small>Stop: event #{child.stoppedSequence} · {ago(child.stoppedAt)} · evidence {child.stoppedEvidenceId}</small>}
                  <small>{child.lastChildEvent ? `Last direct child event: #${child.lastChildEvent.sequence} · ${readable(child.lastChildEvent.kind)} · ${ago(child.lastChildEvent.occurredAt)}` : "No direct child action recorded"}</small>
                  <small>{child.declaredNodeId ? `Declared node: ${child.declaredNodeId}; assignment not verified` : "Assigned task not recorded"}</small>
                  <small>Task result and acceptance not verified here.</small>
                </details></li>)}</ul>}
                {session.tasks.length > 0 && <div className="work-session-tasks" aria-label="Claude tasks observed in this session">
                  <p className="work-team-flat-note">Claude task records observed in this session. They are not linked to a particular subagent; teammate names are not proof of assignment. Marked complete does not mean reviewed or accepted.</p>
                  <ul>{session.tasks.map((task) => <li key={task.nativeTaskId}>
                    <strong>{task.taskSubject}</strong>
                    <small>Claude task {task.nativeTaskId}</small>
                    <small>{task.createdSequence === null ? "Creation not observed" : `Creation observed · event #${task.createdSequence} · created by teammate: ${task.createdByTeammateName ?? "unknown"}`}</small>
                    <small>{task.completedSequence === null ? "Completion not observed" : `Marked complete in Claude · event #${task.completedSequence} · completed by teammate: ${task.completedByTeammateName ?? "unknown"} · output review not observed`}</small>
                  </li>)}</ul>
                </div>}
              </details>)}
            </div>}
            {sessions.size === 0 && <p className="work-team-flat-note">{subagents === null ? "Checking recorded session links · showing a flat team list." : "No readable session links · showing a flat team list."}</p>}
            {subagents && subagents.rejected > 0 && <p className="work-caution" role="note">{subagents.rejected} session signal{subagents.rejected === 1 ? "" : "s"} could not be verified.</p>}
            {claudeTasks && claudeTasks.rejected > 0 && <p className="work-caution" role="note">{claudeTasks.rejected} Claude task signal{claudeTasks.rejected === 1 ? "" : "s"} could not be verified.</p>}
          </section>

          <section className="work-section">
            <div className="work-section-heading">
              <div><MessageCircle aria-hidden="true" size={17} /><h2>Conversations</h2></div>
              <span>{talks.length}</span>
            </div>
            {talks.length === 0 ? (
              <p className="work-empty">No conversations have been observed in this run.</p>
            ) : (
              <div className="work-talk-list">
                {talks.map((talk) => {
                  const isSelected = selectedTalk === talk.key;
                  return (
                    <button
                      type="button"
                      className={`work-talk-row${isSelected ? " work-selected" : ""}${selectedAgent && talk.participants.includes(selectedAgent) ? " work-related" : ""}`}
                      key={talk.key}
                      aria-pressed={isSelected}
                      onClick={() => onSelectTalk?.(isSelected ? null : talk.key)}
                    >
                      <span className="work-talk-faces" aria-hidden="true">{talk.participants.slice(0, 3).map(id => <span key={id} style={{ background: `hsl(${hueOf(id)} 52% 46%)` }}>{initialOf(id)}</span>)}</span>
                      <span className="work-talk-copy">
                        <strong>{talk.label}</strong>
                        <span>{talk.participants.length > 0 ? talk.participants.join(", ") : "Everyone"}</span>
                        {talk.preview && <span className="work-talk-preview">{talk.preview}</span>}
                        {talk.lastAt && <span>Last message · {ago(talk.lastAt)}</span>}
                      </span>
                      <span className="work-talk-count">{talk.count}</span>
                    </button>
                  );
                })}
              </div>
            )}
          </section>
        </aside>

        <section className="work-main" aria-label="Work nodes">
          <div className="work-section-heading work-node-heading">
            <div><Network aria-hidden="true" size={17} /><h2>Work nodes</h2></div>
            <span>{model.edgesKnown ? `${model.edges.length} dependencies` : unverified}</span>
          </div>
          {model.nodes.some((node) => nodeStatusLabel(node) === "review needed") && (
            <p className="work-note" role="note">
              A finished step does not prove its goal passed. Open the node to read its evidence and acceptance verdict.
            </p>
          )}
          {model.nodes.length === 0 ? (
            <div className="work-empty work-empty-panel"><CircleDot aria-hidden="true" size={22} /><p>No work nodes have been observed yet.</p></div>
          ) : (
            <div className="work-node-grid">
              {orderedNodes.map((node) => {
                const latest = latestNodeEvent(node);
                const result = nodeResult(node);
                const incoming = model.edgesKnown ? model.edges.filter((edge) => edge.to === node.id && nodeIds.has(edge.from)) : [];
                const outgoing = model.edgesKnown ? model.edges.filter((edge) => edge.from === node.id && nodeIds.has(edge.to)) : [];
                const isSelected = selectedNode === node.id;
                return (
                <article className={`work-node-card${isSelected ? " work-selected" : ""}${selectedAgent && (node.assignedActor?.id === selectedAgent || node.history.some(event => event.actorId === selectedAgent)) ? " work-related" : ""}`} key={node.id}>
                    <button type="button" className="work-node-open" aria-label={`Open node ${node.id}`} aria-pressed={isSelected} onClick={() => onSelectNode(isSelected ? null : node.id)}>
                      <span className="work-node-topline"><span className={statusClass(node)} title={nodeStatusLabel(node) === "review needed" ? "Runtime state: succeeded; acceptance not verified" : undefined}>{nodeStatusLabel(node) ?? (node.state === "unknown" ? "Awaiting event" : readable(node.state))}</span><span>{node.touches} event{node.touches === 1 ? "" : "s"}</span></span>
                      <strong className="work-node-id">{node.declaredName ?? node.id}</strong>
                      {node.declaredName && <span className="work-muted">{node.id}</span>}
                      {node.declaredRole && <span className="work-muted">Declared role · {node.declaredRole}</span>}
                      {objective !== null && objective.trim().length > 0 && isFirstEntryNode(model, node.id) && (
                        <q className="work-node-objective" title={objective}>{objective}</q>
                      )}
                      {node.proposal && <span className="work-node-latest"><span>Governance</span><strong>{node.proposal.status === "rejected" ? `Proposal rejected · ${node.proposal.reason ?? "reason unavailable"}` : node.proposal.status === "unavailable" ? `Proposal unavailable · ${node.proposal.reason ?? "reason unavailable"}` : `Proposal ${node.proposal.status}`}</strong><small>{node.proposal.status === "rejected" ? "Next action: review the recorded reason before proposing again." : node.proposal.status === "unavailable" ? "Next action: request the typed proposal descriptor again." : node.assignedActor ? `Responsible actor · ${node.assignedActor.id}` : "Next action: assign an actor and approve the governed draft."}</small></span>}
                      {node.assignedActor && <span className="work-node-latest"><span>Responsible actor</span><strong>{node.assignedActor.id}</strong><small>{node.assignedActor.type} · assignment is separate from the Runtime recorder</small></span>}
                      {latest ? (
                        <span className="work-node-latest"><span>Latest event</span><strong>{readable(latest.outcome ?? latest.kind)}</strong><small>{latest.actorType === "system" ? "Recorded by Runtime" : latest.actorId ? `Recorded by ${latest.actorId}` : "Recorder unknown"} · {ago(latest.occurredAt)}</small></span>
                      ) : <span className="work-node-latest"><span>Latest event</span><strong className="work-muted">Awaiting first event</strong></span>}
                      {(result || node.actualExecutor) && <span className="work-node-latest"><span>Executed by</span><strong>{result?.executor ?? (node.actualExecutor?.kind === "model" ? `Model · route ${node.actualExecutor.routeId ?? "not recorded"}` : node.actualExecutor?.kind ?? "Not recorded")}</strong><small>{result?.verification ?? "Latest attempt recorded"}</small></span>}
                    </button>
                    <div className="work-node-footer"><span><Clock3 aria-hidden="true" size={14} /> {node.history.length} history item{node.history.length === 1 ? "" : "s"}</span></div>
                    {model.edgesKnown ? (
                      <div className="work-dependencies">
                        {incoming.map((edge) => <button type="button" className="work-dependency" key={`in-${edge.id}`} onClick={() => onSelectNode(edge.from)}><ArrowDown aria-hidden="true" size={14} /><span>from {edge.from}</span></button>)}
                        {outgoing.map((edge) => <button type="button" className="work-dependency" key={`out-${edge.id}`} onClick={() => onSelectNode(edge.to)}><ArrowUp aria-hidden="true" size={14} /><span>to {edge.to}</span></button>)}
                        {incoming.length === 0 && outgoing.length === 0 && <span className="work-no-dependencies">No dependencies</span>}
                      </div>
                    ) : <p className="work-awaiting">{ended ? "Dependency evidence is unavailable in this view." : "Dependencies awaiting evidence."}</p>}
                  </article>
                );
              })}
            </div>
          )}
        </section>
      </div>
      <section className="work-activity" aria-label="Recent recorded activity">
        <div className="work-section-heading"><div><MessageCircle aria-hidden="true" size={17} /><h2>Recent recorded activity</h2></div><span>Messages in the event log</span></div>
        {activity.length === 0 ? <p className="work-empty">No messages recorded in this run yet.</p> : (
          <ol className="work-activity-list">{activity.map((item) => <li key={item.sequence}>
            <div className="work-activity-meta"><strong>{item.actorId ?? "Unknown actor"}</strong><span>{ago(item.occurredAt)} · event #{item.sequence}</span></div>
            <p>{item.text ?? "Report text has not opened yet"}</p>
          </li>)}</ol>
        )}
      </section>
      </details>
    </main>
  );
}
