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
import { moodOf } from "../graph/model";
import { ago, hueOf, initialOf, readable } from "./format";

type CrewMember = { id: string; charter: string | null; lastAt?: string | null };
type Talk = { key: string; label: string; participants: string[]; count: number; lastAt: string | null };

export interface WorkOverviewProps {
  model: GraphModel;
  crew?: CrewMember[];
  talks?: Talk[];
  selectedNode: string | null;
  onSelectNode: (nodeId: string | null) => void;
  selectedAgent?: string | null;
  onSelectAgent?: (agentId: string | null) => void;
  selectedTalk?: string | null;
  onSelectTalk?: (talkKey: string | null) => void;
  runId?: string;
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
  return `work-status work-status-${moodOf(node.state)}`;
}

export function WorkOverview({
  model,
  crew = [],
  talks = [],
  selectedNode,
  onSelectNode,
  selectedAgent = null,
  onSelectAgent,
  selectedTalk = null,
  onSelectTalk,
  runId,
}: WorkOverviewProps) {
  const activeNodes = model.nodes.filter((node) => ["running", "queued", "linting"].includes(node.state)).length;
  const attentionNodes = model.nodes.filter((node) => ["blocked", "failed", "waiting_input", "waiting_capacity"].includes(node.state)).length;
  const nodeIds = new Set(model.nodes.map((node) => node.id));

  return (
    <main className="work-overview" aria-label="Work overview">
      <header className="work-header">
        <div className="work-heading">
          <span className="work-kicker"><Activity aria-hidden="true" size={16} /> Live workspace</span>
          <h1>Work overview</h1>
          {runId && <span className="work-run">Run {runId}</span>}
        </div>
        <div className="work-counts" aria-label="Workspace counts">
          <span><CircleDot aria-hidden="true" size={15} /> {model.nodes.length} nodes{!model.rosterDeclared && " seen so far"}</span>
          <span><Activity aria-hidden="true" size={15} /> {activeNodes} active</span>
          <span><Users aria-hidden="true" size={15} /> {crew.length} agents</span>
          <span><MessageCircle aria-hidden="true" size={15} /> {talks.length} conversations</span>
          {attentionNodes > 0 && <span className="work-count-attention">{attentionNodes} needs attention</span>}
        </div>
      </header>

      {!model.rosterDeclared && <p className="work-caution">The complete node roster has not been read yet.</p>}
      {model.lint.length > 0 && <section className="work-caution" aria-label="Disagreements in the event log"><details><summary>Evidence needs attention · {model.lint.length} finding{model.lint.length === 1 ? "" : "s"}</summary><ul>{model.lint.map((finding, index) => <li key={`${finding.kind}-${finding.sequence}-${index}`}>{finding.detail}{finding.sequence !== null && <span> · event #{finding.sequence}</span>}</li>)}</ul></details></section>}

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
                {crew.map((agent) => {
                  const observation = latestObservationForAgent(model, agent.id);
                  const isSelected = selectedAgent === agent.id;
                  return (
                    <button
                      type="button"
                      className={`work-agent-row${isSelected ? " work-selected" : ""}`}
                      key={agent.id}
                      aria-pressed={isSelected}
                      onClick={() => onSelectAgent?.(isSelected ? null : agent.id)}
                    >
                      <span className="work-avatar" style={{ background: `hsl(${hueOf(agent.id)} 52% 46%)` }} aria-hidden="true">{initialOf(agent.id)}</span>
                      <span className="work-agent-copy">
                        <span className="work-agent-id">{agent.id}</span>
                        <span className="work-observed">
                          <span>Last observed</span>
                          {observation ? (
                            <><strong>{observation.node.id}</strong><small>{ago(observation.at)}</small></>
                          ) : <strong className="work-muted">No node activity yet</strong>}
                        </span>
                      </span>
                    </button>
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
            <span>{model.edgesKnown ? `${model.edges.length} dependencies` : "Dependencies awaiting evidence"}</span>
          </div>
          {model.nodes.length === 0 ? (
            <div className="work-empty work-empty-panel"><CircleDot aria-hidden="true" size={22} /><p>No work nodes have been observed yet.</p></div>
          ) : (
            <div className="work-node-grid">
              {model.nodes.map((node) => {
                const latest = latestNodeEvent(node);
                const incoming = model.edgesKnown ? model.edges.filter((edge) => edge.to === node.id && nodeIds.has(edge.from)) : [];
                const outgoing = model.edgesKnown ? model.edges.filter((edge) => edge.from === node.id && nodeIds.has(edge.to)) : [];
                const isSelected = selectedNode === node.id;
                return (
                  <article className={`work-node-card${isSelected ? " work-selected" : ""}${selectedAgent && node.history.some(event => event.actorId === selectedAgent) ? " work-related" : ""}`} key={node.id}>
                    <button type="button" className="work-node-open" aria-label={`Open node ${node.id}`} aria-pressed={isSelected} onClick={() => onSelectNode(isSelected ? null : node.id)}>
                      <span className="work-node-topline"><span className={statusClass(node)}>{node.state === "unknown" ? "Awaiting event" : readable(node.state)}</span><span>{node.touches} event{node.touches === 1 ? "" : "s"}</span></span>
                      <strong className="work-node-id">{node.id}</strong>
                      {latest ? (
                        <span className="work-node-latest"><span>Latest event</span><strong>{readable(latest.outcome ?? latest.kind)}</strong><small>{latest.actorId ? `${latest.actorId} · ` : ""}{ago(latest.occurredAt)}</small></span>
                      ) : <span className="work-node-latest"><span>Latest event</span><strong className="work-muted">Awaiting first event</strong></span>}
                    </button>
                    <div className="work-node-footer"><span><Clock3 aria-hidden="true" size={14} /> {node.history.length} history item{node.history.length === 1 ? "" : "s"}</span></div>
                    {model.edgesKnown ? (
                      <div className="work-dependencies">
                        {incoming.map((edge) => <button type="button" className="work-dependency" key={`in-${edge.id}`} onClick={() => onSelectNode(edge.from)}><ArrowDown aria-hidden="true" size={14} /><span>from {edge.from}</span></button>)}
                        {outgoing.map((edge) => <button type="button" className="work-dependency" key={`out-${edge.id}`} onClick={() => onSelectNode(edge.to)}><ArrowUp aria-hidden="true" size={14} /><span>to {edge.to}</span></button>)}
                        {incoming.length === 0 && outgoing.length === 0 && <span className="work-no-dependencies">No dependencies</span>}
                      </div>
                    ) : <p className="work-awaiting">Dependencies awaiting evidence.</p>}
                  </article>
                );
              })}
            </div>
          )}
        </section>
      </div>
    </main>
  );
}
