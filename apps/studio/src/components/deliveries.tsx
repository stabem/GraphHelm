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
  work?: DeliveryWork;
}
export interface DeliveryWork { version: 1; sessionId: string; stage: string; revision: string; skills: DeliverySkill[]; checks: DeliveryCheck[]; journeyVerification?: Record<string, unknown>; }
interface DeliverySkill { id: string; version: string; digest: string; status: "requested" | "reported"; }
interface DeliveryCheck { id: string; command: string; observer: string; outcome: "passed" | "failed" | "skipped" | "unobserved"; attemptId: string; previousAttemptId?: string; evidence?: { evidenceId: string; contentHash: string; size: number }; }
export interface DocumentReference {
  evidenceId: string;
  index: number;
  path: string;
  title: string;
  projectId: string;
}

/** Evidence is untrusted. Never convert its prose or paths into HTML or URLs. */
const text = (value: unknown, max = 256): value is string => typeof value === "string" && value.trim().length > 0 && new TextEncoder().encode(value).byteLength <= max && !/[\u0000-\u001f\u007f]/.test(value);
const digest = (value: unknown): value is string => typeof value === "string" && /^sha256:[a-f0-9]{64}$/.test(value);
const boundedArray = (value: unknown, max: number): value is unknown[] => Array.isArray(value) && value.length <= max;
const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);

function parseRecord(content: string): DeliveryRecord {
  if (new TextEncoder().encode(content).byteLength > 262144) throw new Error("Delivery envelope is too large.");
  const envelope: unknown = JSON.parse(content);
  const inner = object(envelope) && "description" in envelope ? envelope.description : content;
  if (typeof inner !== "string" || new TextEncoder().encode(inner).byteLength > 16384) throw new Error("Delivery record is too large.");
  const value: unknown = JSON.parse(inner);
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
  if (record.work !== undefined) validateWork(record.work);
  return record;
}

function validateWork(work: unknown): asserts work is DeliveryWork {
  if (!work || typeof work !== "object") throw new Error("Invalid delivery work.");
  const value = work as Record<string, unknown>;
  if (value.version !== 1 || !text(value.sessionId, 256) || !text(value.stage, 128) || typeof value.revision !== "string" || !/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(value.revision) || !boundedArray(value.skills, 16) || !boundedArray(value.checks, 32)) throw new Error("Invalid delivery work.");
  for (const skill of value.skills) {
    if (!skill || typeof skill !== "object") throw new Error("Invalid delivery skill.");
    const item = skill as Record<string, unknown>;
    if (!text(item.id, 256) || !text(item.version, 64) || !digest(item.digest) || !["requested", "reported"].includes(item.status as string)) throw new Error("Invalid delivery skill.");
  }
  for (const check of value.checks) {
    if (!check || typeof check !== "object") throw new Error("Invalid delivery check.");
    const item = check as Record<string, unknown>;
    if (!text(item.id, 256) || !text(item.command, 1024) || !text(item.observer, 256) || !text(item.attemptId, 256) || !["passed", "failed", "skipped", "unobserved"].includes(item.outcome as string)) throw new Error("Invalid delivery check.");
    if (item.previousAttemptId !== undefined && !text(item.previousAttemptId, 256)) throw new Error("Invalid delivery check.");
    if (item.evidence !== undefined) {
      const evidence = item.evidence as Record<string, unknown> | undefined;
      if (!evidence || !text(evidence.evidenceId, 256) || !digest(evidence.contentHash) || typeof evidence.size !== "number" || !Number.isSafeInteger(evidence.size) || evidence.size < 0 || evidence.size > 16777216) throw new Error("Invalid check evidence.");
    } else if (item.outcome === "passed" || item.outcome === "failed") throw new Error("Checks with outcomes require evidence.");
  }
  if (value.journeyVerification !== undefined) validateJourney(value.journeyVerification, value.revision);
}

function validateJourney(value: unknown, revision: string): asserts value is Record<string, unknown> {
  if (!value || typeof value !== "object") throw new Error("Invalid journey verification.");
  const jpd = value as Record<string, unknown>;
  if (!text(jpd.verificationId, 256) || !text(jpd.journeyRunId, 256) || !text(jpd.contractId, 256) || !digest(jpd.contractDigest) || !text(jpd.verifiedAt, 64) || !["proven", "accepted_with_waiver", "unresolved"].includes(jpd.proposedResultStatus as string) || !boundedArray(jpd.obligations, 128) || !boundedArray(jpd.disagreements, 32) || !jpd.retry || typeof jpd.retry !== "object") throw new Error("Invalid journey verification.");
  const bindings = jpd.bindings as Record<string, unknown> | undefined;
  const code = bindings?.code as Record<string, unknown> | undefined;
  const graph = bindings?.graph as Record<string, unknown> | undefined;
  if (!bindings || !code || !text(code.repository, 256) || code.revision !== revision || !graph || !text(graph.graphId, 256) || typeof graph.version !== "number" || !Number.isSafeInteger(graph.version) || graph.version < 1 || !digest(graph.semanticHash)) throw new Error("Journey bindings do not match work revision.");
  if (!object(bindings.configuration) || !text(bindings.configuration.environment, 128) || !digest(bindings.configuration.digest) || !boundedArray(bindings.fixtures, 64) || bindings.fixtures.some((fixture) => !object(fixture) || !text(fixture.fixtureId) || !digest(fixture.digest))) throw new Error("Invalid journey environment bindings.");
  const authority = jpd.authority as Record<string, unknown> | undefined;
  if (!authority || authority.status !== "candidate" || !authority.validation || typeof authority.validation !== "object") throw new Error("Journey authority validation is missing.");
  const validation = authority.validation as Record<string, unknown>;
  if (validation.status !== "capability_missing" || validation.code !== "JPD_VALIDATOR_MISSING" || validation.missingCapability !== "jpd.registered-deterministic-validator" || !text(validation.reason, 1024)) throw new Error("Invalid journey authority.");
  const gate = jpd.gate as Record<string, unknown> | undefined;
  if (!gate || !["evaluated", "capability_missing"].includes(String(gate.status))) throw new Error("Journey gate is missing or invalid.");
  if (gate.status === "evaluated" ? !["accepted", "rejected"].includes(gate.result as string) :
      !object(gate.refusal) || !text(gate.refusal.missingCapability) || !text(gate.refusal.reason, 1024)) throw new Error("Invalid journey gate result.");
  for (const obligation of jpd.obligations) {
    if (!obligation || typeof obligation !== "object" || !text((obligation as Record<string, unknown>).obligationId, 256) || !["satisfied", "failed", "observer_missing"].includes((obligation as Record<string, unknown>).status as string)) throw new Error("Invalid journey obligation.");
    const item = obligation as Record<string, unknown>;
    if (item.status === "observer_missing" && (!object(item.refusal) || !text(item.refusal.missingCapability) || !text(item.refusal.reason, 1024))) throw new Error("Invalid observer refusal.");
    if (item.status === "failed" && !text(item.failure, 1024)) throw new Error("Invalid obligation failure.");
  }
  for (const disagreement of jpd.disagreements) {
    if (!object(disagreement) || !text(disagreement.disagreementId) || !text(disagreement.resolution, 1024) || !["resolved", "unresolved", "observer_missing"].includes(disagreement.status as string)) throw new Error("Invalid journey disagreement.");
  }
  const retry = jpd.retry as Record<string, unknown>;
  if ((retry.outcomeClassification !== null && !["first_pass_success", "recovered_success", "flaky_pass", "unresolved_failure"].includes(retry.outcomeClassification as string)) || typeof retry.firstFailurePreserved !== "boolean" || !object(retry.classification) || !object(retry.lineageValidation) || !["evaluated", "capability_missing"].includes(retry.classification.status as string) || !["evaluated", "capability_missing"].includes(retry.lineageValidation.status as string)) throw new Error("Invalid journey retry.");
  const observers = bindings.observers;
  if (!boundedArray(observers, 64) || observers.some((observer) => !object(observer) || !text(observer.observerId) || !text(observer.version, 32) || !digest(observer.configurationDigest) || !digest(observer.environmentDigest) || typeof observer.evidenceFresh !== "boolean")) throw new Error("Invalid journey observers.");
}

function JourneyDetails({ journey }: { journey: Record<string, unknown> }) {
  const bindings = journey.bindings as Record<string, unknown>;
  const graph = bindings.graph as Record<string, unknown>;
  const code = bindings.code as Record<string, unknown>;
  const configuration = bindings.configuration as Record<string, unknown>;
  const fixtures = bindings.fixtures as Array<Record<string, unknown>>;
  const authority = journey.authority as Record<string, unknown>;
  const validation = authority.validation as Record<string, unknown>;
  const gate = journey.gate as Record<string, unknown>;
  const retry = journey.retry as Record<string, unknown> | undefined;
  const obligations = journey.obligations as Array<Record<string, unknown>>;
  const observers = Array.isArray(bindings.observers) ? bindings.observers as Array<Record<string, unknown>> : [];
  return <div className="delivery-jpd"><div className="delivery-jpd-heading"><h5>JPD candidate (not certification)</h5><span className="delivery-outcome delivery-outcome-unobserved">not certification</span></div>
    <p>Proposed result: <strong>{String(journey.proposedResultStatus)}</strong></p>
    <details className="delivery-details"><summary>Candidate provenance</summary><div className="delivery-details-body">
      <p>Graph: {String(graph.graphId)} v{String(graph.version)} · {String(graph.semanticHash)}</p>
      <p>Code: {String(code.repository)} @ {String(code.revision)}</p>
      <p>Environment: {String(configuration.environment)} · configuration {String(configuration.digest)}</p>
      <ul>{fixtures.map((fixture) => <li key={String(fixture.fixtureId)}>Fixture {String(fixture.fixtureId)}: {String(fixture.digest)}</li>)}</ul>
      {observers.map((observer) => <p key={String(observer.observerId)}>Observer {String(observer.observerId)} v{String(observer.version)}: reported evidence {observer.evidenceFresh ? "fresh" : "stale"} · configuration {String(observer.configurationDigest)} · environment {String(observer.environmentDigest)}</p>)}
    </div></details>
    <p>Authority: candidate · validation {String(validation.status)}{validation.missingCapability ? ` · missing ${String(validation.missingCapability)}` : ""}{validation.reason ? ` · ${String(validation.reason)}` : ""}</p>
    <p>Gate: {String(gate.status)}{gate.result ? ` · ${String(gate.result)}` : ""}{gate.refusal ? ` · missing ${String((gate.refusal as Record<string, unknown>).missingCapability)}` : ""}</p>
    <ul>{obligations.map((item) => <li key={String(item.obligationId)}>Obligation {String(item.obligationId)}: {String(item.status)}{item.status === "observer_missing" ? ` · missing ${String((item.refusal as Record<string, unknown>).missingCapability)}` : ""}{item.status === "failed" ? ` · ${String(item.failure)}` : ""}</li>)}</ul>
    {(journey.disagreements as Array<Record<string, unknown>>).map((item) => <p key={String(item.disagreementId)}>Disagreement {String(item.disagreementId)}: {String(item.status)} · {String(item.resolution)}</p>)}
    {retry && <p>Retry: {String(retry.outcomeClassification ?? "unclassified")} · classification {String((retry.classification as Record<string, unknown>).status)} · lineage {String((retry.lineageValidation as Record<string, unknown>).status)} · first failure preserved {retry.firstFailurePreserved ? "yes" : "no"}</p>}
    <p className="delivery-unavailable">Review and merge receipts: not observed in this delivery record.</p>
  </div>;
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
    {visible.records.map(({ id, record }) => {
      const event = events.find((item) => item.kind === "signal_recorded" && item.evidenceRefs.includes(id) && (() => { const payload = item.payload as Record<string, unknown> | null; return payload?.kind === "node_delivery" && payload.sourceKind === "node" && payload.sourceId === nodeId; })());
      return <article className="delivery-card" key={id}>
      <span className="delivery-eyebrow">Reported delivery</span>
      <h4>{record.summary}</h4><p>{record.reason}</p>
      <ul>{record.documents.map((document, index) => <li key={`${document.path}:${index}`}>
        {/\.(md|mdx|txt|rst|adoc|json|yaml|yml|toml|csv)$/i.test(document.path) ? <button className="delivery-document" onClick={() => onOpenDocument({ evidenceId: id, index, path: document.path, title: document.title, projectId: record.projectId })}>
          <FileText size={15} /><span><strong>{document.title || document.path}</strong><small>{document.path}</small></span><ArrowUpRight size={14} />
        </button> : <div className="delivery-document delivery-source">
          <FileText size={15} /><span><strong>{document.title || document.path}</strong><small>{document.path}</small><small>Source reference. Open this path in your code editor.</small></span>
        </div>}
        <small className="delivery-meta">{document.action} · {document.kind === "business_rule" ? "Business rule" : "File"}</small>
        {document.journeyIds?.length ? <p className="delivery-meta">Journeys: {document.journeyIds.join(", ")}</p> : null}
        {document.ruleIds?.length ? <p className="delivery-meta">Rules: {document.ruleIds.join(", ")}</p> : null}
      </li>)}</ul>
      {record.work ? <div className="delivery-work"><div className="delivery-work-heading"><h5>Reported work</h5><span className="delivery-stage">{record.work.stage}</span></div><details className="delivery-details"><summary>Session and revision</summary><div className="delivery-details-body"><p>Session reported by work: <code>{record.work.sessionId}</code></p><p>Recorded reporter: {event?.actorId ?? "unknown"} · {event?.occurredAt ?? "time unavailable"} · sequence {event?.sequence ?? "unknown"}</p><p>Revision: <code>{record.work.revision}</code></p></div></details>
        <h5>Skills</h5>{record.work.skills.length === 0 && <p>No skill provenance reported.</p>}<ul>{record.work.skills.map((skill) => <li className="delivery-skill-row" key={skill.id}><span><strong>{skill.id}</strong> v{skill.version}</span><span className="delivery-skill-status">{skill.status}</span><details className="delivery-details delivery-inline-details"><summary>Digest</summary><code>{skill.digest}</code></details></li>)}</ul>
        <h5>Checks</h5>{record.work.checks.length === 0 && <p>No check attempts reported.</p>}<ul>{record.work.checks.map((check) => <li className="delivery-check-row" key={`${check.id}:${check.attemptId}`}><span className="delivery-check-main"><strong>Check {check.id}</strong><span className="delivery-meta">{check.observer}</span></span><span className={`delivery-outcome delivery-outcome-${check.outcome}`}>{check.outcome}</span><details className="delivery-details delivery-inline-details"><summary>Details</summary><div className="delivery-details-body"><p><code>{check.command}</code></p><p>Attempt {check.attemptId}{check.previousAttemptId ? ` · after ${check.previousAttemptId}` : ""}{check.evidence ? ` · evidence ${check.evidence.evidenceId} (${check.evidence.contentHash}, ${check.evidence.size} bytes)` : ""}</p></div></details></li>)}</ul>
        {record.work.journeyVerification ? <JourneyDetails journey={record.work.journeyVerification} /> : <p className="delivery-unavailable">No JPD candidate recorded. Certification is unobserved.</p>}
      </div> : <p className="delivery-unavailable">Reported work is unavailable in this delivery record.</p>}
    </article>;
    })}
  </section>;
}
