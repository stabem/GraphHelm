import { useEffect, useMemo, useState } from "react";
import { ArrowUpRight, FileText } from "lucide-react";
import type { EvidenceContent, RuntimeEvent } from "../runtime/types";
import "./delivery.css";

export interface DeliveryDocument {
  path: string;
  title: string;
  kind: "file" | "business_rule";
  action: "created" | "updated" | "reviewed";
  journeyIds?: string[];
  ruleIds?: string[];
}
export interface DeliveryRecord {
  version: 1;
  projectId: string;
  summary: string;
  reason: string;
  documents: DeliveryDocument[];
}
export interface DocumentReference {
  evidenceId: string;
  index: number;
  path: string;
  title: string;
  projectId: string;
}

/** Evidence is untrusted. Never convert its prose or paths into HTML or URLs. */
function parseRecord(content: string): DeliveryRecord {
  if (content.length > 262144) throw new Error("Delivery record is too large.");
  const envelope: unknown = JSON.parse(content);
  const value: unknown = envelope !== null && typeof envelope === "object" && "description" in envelope
    ? JSON.parse(String(envelope.description)) : envelope;
  if (!value || typeof value !== "object") throw new Error("Invalid delivery record.");
  const record = value as DeliveryRecord;
  if (record.version !== 1 || typeof record.projectId !== "string" || !/^[a-f0-9]{64}$/i.test(record.projectId) ||
      typeof record.summary !== "string" || typeof record.reason !== "string" ||
      !Array.isArray(record.documents) || record.documents.length > 64 ||
      record.documents.some((document) => !document || typeof document.path !== "string" ||
        typeof document.title !== "string" || !["file", "business_rule"].includes(document.kind) ||
        !["created", "updated", "reviewed"].includes(document.action) ||
        [document.journeyIds, document.ruleIds].some((ids) => ids !== undefined &&
          (!Array.isArray(ids) || ids.length > 64 || ids.some((id) => typeof id !== "string"))))) {
    throw new Error("Invalid delivery record.");
  }
  return record;
}

export function NodeDeliveries({ nodeId, executionId, events, openEvidence, onOpenDocument }: {
  nodeId: string;
  executionId: string;
  events: RuntimeEvent[];
  openEvidence: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
  onOpenDocument: (reference: DocumentReference) => void;
}) {
  const ids = useMemo(() => [...new Set(events.filter((event) => {
    const payload = event.payload as Record<string, unknown> | null;
    return event.kind === "signal_recorded" && payload?.kind === "node_delivery" &&
      payload.sourceKind === "node" && payload.sourceId === nodeId;
  }).flatMap((event) => event.evidenceRefs))].reverse(), [events, nodeId]);
  const signature = JSON.stringify(ids.slice(0, 30));
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<{ key: string; records: { id: string; record: DeliveryRecord }[]; loading: boolean; errors: number }>({ key: "", records: [], loading: true, errors: 0 });
  const key = `${executionId}:${nodeId}:${signature}`;
  useEffect(() => {
    let current = true;
    setState({ key, records: [], loading: true, errors: 0 });
    void (async () => {
      const records: { id: string; record: DeliveryRecord }[] = [];
      let errors = 0;
      for (const id of JSON.parse(signature) as string[]) {
        if (!current) return;
        try { records.push({ id, record: parseRecord((await openEvidence(executionId, id)).content) }); }
        catch { errors++; }
      }
      if (current) setState({ key, records, loading: false, errors });
    })();
    return () => { current = false; };
  }, [executionId, key, signature, openEvidence, attempt]);
  const visible = state.key === key ? state : { records: [], loading: true, errors: 0 };
  return <section className="node-deliveries" aria-label="Deliveries">
    <div className="delivery-heading"><FileText size={16} /><h3>Deliveries</h3></div>
    {visible.loading && <p role="status">Opening delivery records…</p>}
    {!visible.loading && ids.length === 0 && <p>No deliveries recorded. A successful state alone does not describe the output.</p>}
    {visible.errors > 0 && <div role="alert"><p>Some delivery records could not be opened.</p><button onClick={() => setAttempt((value) => value + 1)}>Retry deliveries</button></div>}
    {ids.length > 30 && <p>Showing the latest 30 delivery records.</p>}
    {visible.records.map(({ id, record }) => <article className="delivery-card" key={id}>
      <span className="delivery-eyebrow">Reported delivery</span>
      <h4>{record.summary}</h4><p>{record.reason}</p>
      <ul>{record.documents.map((document, index) => <li key={`${document.path}:${index}`}>
        <button className="delivery-document" onClick={() => onOpenDocument({ evidenceId: id, index, path: document.path, title: document.title, projectId: record.projectId })}>
          <FileText size={15} /><span><strong>{document.title || document.path}</strong><small>{document.path}</small></span><ArrowUpRight size={14} />
        </button>
        <small className="delivery-meta">{document.action} · {document.kind === "business_rule" ? "Business rule" : "File"}</small>
        {document.journeyIds?.length ? <p className="delivery-meta">Journeys: {document.journeyIds.join(", ")}</p> : null}
        {document.ruleIds?.length ? <p className="delivery-meta">Rules: {document.ruleIds.join(", ")}</p> : null}
      </li>)}</ul>
    </article>)}
  </section>;
}
