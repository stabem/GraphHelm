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
import { ago, hueOf, initialOf, readable } from "./format";

type CrewMember = { id: string; charter: string | null; lastAt?: string | null };
type Talk = { key: string; label: string; participants: string[]; count: number; lastAt: string | null; preview?: string | null };
type RecordedActivity = { sequence: number; actorId: string | null; occurredAt: string | null; text: string | null };
type AgentReport = { sequence: number; occurredAt: string | null; text: string | null };

export interface WorkOverviewProps {
  model: GraphModel;
  crew?: CrewMember[];
  talks?: Talk[];
  activity?: RecordedActivity[];
  agentReports?: Record<string, AgentReport>;
  runStatus?: string | null;
  attention?: string | null;
  nextAction?: { label: string; detail: string } | null;
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

function latestObservationForAgent(model: GraphModel, agentId: string): { node: GraphNode; at: string | null } | null {
  let latest: { node: GraphNode; at: string | null; timestamp: number } | null = null;
  for (const node of model.nodes) {
    for (const event of node.history) {
      if (event.actorId !== agentId) continue;
      const timestamp = event.sequence;
      if (latest === null || timestamp >= latest.timestamp) {
        latest = { node, at: event.occurredAt, timestamp };
      }
    }
  }
  return latest === null ? null : { node: latest.node, at: latest.at };
}

function statusClass(node: GraphNode): string {
  if (nodeStatusLabel(node) !== null) return "work-status work-status-review";
  return `work-status work-status-${moodOf(node.state)}`;
}

export function WorkOverview({
  model,
  crew = [],
  talks = [],
  activity = [],
  agentReports = {},
  runStatus = null,
  attention = null,
  nextAction = null,
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
  const nodeIds = new Set(model.nodes.map((node) => node.id));
  const lint = splitLint(model.lint, demonstration);
  const latestReport = activity[0] ?? null;
  const orderedCrew = [...crew].sort((left, right) =>
    (agentReports[right.id]?.sequence ?? 0) - (agentReports[left.id]?.sequence ?? 0),
  );

  return (
    <main className="work-overview" aria-label="Work overview">
      <header className="work-header">
        <div className="work-heading">
          <span className="work-kicker"><Activity aria-hidden="true" size={16} /> Live workspace</span>
          <h1>Work overview</h1>
          {runId && <span className="work-run">Run {runId}</span>}
        </div>
        <div className="work-counts" aria-label="Workspace counts">
          <span><CircleDot aria-hidden="true" size={15} /> {model.nodes.length} node{model.nodes.length === 1 ? "" : "s"}{!model.rosterDeclared && " seen so far"}</span>
          <span><Activity aria-hidden="true" size={15} /> {activeNodes} active node{activeNodes === 1 ? "" : "s"}</span>
          <span><Users aria-hidden="true" size={15} /> {crew.length} agents</span>
          <span><MessageCircle aria-hidden="true" size={15} /> {talks.length} conversations</span>
          {attentionNodes > 0 && <span className="work-count-attention">{attentionNodes} needs attention</span>}
        </div>
      </header>

      {nextAction && <section className="work-next-action" aria-label="Next action">
        <div><span>Needs your attention{attention ? ` · ${readable(attention)}` : ""}</span><strong>{nextAction.label}</strong><p>{nextAction.detail}</p></div>
        <button type="button" onClick={onNextAction}>{nextAction.label}</button>
      </section>}

      <section className="work-snapshot" aria-label="Where this run stands">
        <div className="work-section-heading"><div><Activity aria-hidden="true" size={17} /><h2>Where this run stands</h2></div><span>From the Runtime record</span></div>
        <div className="work-snapshot-grid">
          <div><span>Run state</span><strong>{runStatus ? readable(runStatus) : "State unavailable"}</strong>{runStatus === "completed" && model.nodes.some((node) => nodeStatusLabel(node) === "review needed") && <small>Node results still need review</small>}</div>
          <div><span>Graph step</span><strong>{model.nodes.length === 1 ? `${model.nodes[0].id} · ${readable(model.nodes[0].state)}` : `${model.nodes.length} declared nodes · ${activeNodes} active`}</strong></div>
          <div><span>Last chat report</span><strong>{latestReport ? `${latestReport.actorId ?? "Unknown actor"} · ${ago(latestReport.occurredAt)}` : "No chat report · open nodes for replies"}</strong></div>
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
          <section className="work-section">
            <div className="work-section-heading">
              <div><Users aria-hidden="true" size={17} /><h2>Collaboration</h2></div>
              <span>{crew.length}</span>
            </div>
            {crew.length === 0 ? (
              <p className="work-empty">No agents have been observed in this run.</p>
            ) : (
              <div className="work-agent-list">
                {orderedCrew.map((agent) => {
                  const observation = latestObservationForAgent(model, agent.id);
                  const report = agentReports[agent.id];
                  const isSelected = selectedAgent === agent.id;
                  return (
                    <details className="work-agent-details" key={agent.id}>
                    <summary className="work-agent-row">
                      <span className="work-avatar" style={{ background: `hsl(${hueOf(agent.id)} 52% 46%)` }} aria-hidden="true">{initialOf(agent.id)}</span>
                      <span className="work-agent-copy">
                        <span className="work-agent-id">{agent.id}</span>
                        <span className="work-agent-report"><span>Last direct chat report</span><strong>{report?.text ?? (report ? "Report text has not opened yet" : "No direct chat report · see Work nodes")}</strong>{report && <small>{ago(report.occurredAt)} · event #{report.sequence}</small>}</span>
                        <span className="work-observed">
                          <span>Last node update</span>
                          {observation ? (
                            <><strong>{observation.node.id}</strong><small>{ago(observation.at)}</small></>
                          ) : <strong className="work-muted">No node activity yet</strong>}
                        </span>
                        {agent.lastAt && <span className="work-observed">Last recorded message or event · {ago(agent.lastAt)}</span>}
                      </span>
                    </summary>
                    <div className="work-agent-expanded">
                      <p>{report?.text ?? (report ? "Report text has not opened yet" : "No direct chat report · see Work nodes")}</p>
                      <button type="button" aria-pressed={isSelected} onClick={() => onSelectAgent?.(isSelected ? null : agent.id)}>{isSelected ? "Close direct chat" : `Open direct chat with ${agent.id}`}</button>
                    </div>
                    </details>
                  );
                })}
              </div>
            )}
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
              {model.nodes.map((node) => {
                const latest = latestNodeEvent(node);
                const result = nodeResult(node);
                const incoming = model.edgesKnown ? model.edges.filter((edge) => edge.to === node.id && nodeIds.has(edge.from)) : [];
                const outgoing = model.edgesKnown ? model.edges.filter((edge) => edge.from === node.id && nodeIds.has(edge.to)) : [];
                const isSelected = selectedNode === node.id;
                return (
                  <article className={`work-node-card${isSelected ? " work-selected" : ""}${selectedAgent && node.history.some(event => event.actorId === selectedAgent) ? " work-related" : ""}`} key={node.id}>
                    <button type="button" className="work-node-open" aria-label={`Open node ${node.id}`} aria-pressed={isSelected} onClick={() => onSelectNode(isSelected ? null : node.id)}>
                      <span className="work-node-topline"><span className={statusClass(node)} title={nodeStatusLabel(node) === "review needed" ? "Runtime state: succeeded; acceptance not verified" : undefined}>{nodeStatusLabel(node) ?? (node.state === "unknown" ? "Awaiting event" : readable(node.state))}</span><span>{node.touches} event{node.touches === 1 ? "" : "s"}</span></span>
                      <strong className="work-node-id">{node.declaredName ?? node.id}</strong>
                      {node.declaredName && <span className="work-muted">{node.id}</span>}
                      {node.declaredRole && <span className="work-muted">Declared role · {node.declaredRole}</span>}
                      {objective !== null && objective.trim().length > 0 && isFirstEntryNode(model, node.id) && (
                        <q className="work-node-objective" title={objective}>{objective}</q>
                      )}
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
    </main>
  );
}

