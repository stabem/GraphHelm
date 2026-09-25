import { useEffect, useRef, useState } from "react";
import { FileText, X } from "lucide-react";
import type { DocumentReference } from "./deliveries";
import { newIdempotencyKey } from "../runtime/client";
import "./delivery.css";

export interface DocumentSnapshot { content: string; contentSha256: string }
export interface DocumentSaveRequest { content: string; expectedSha256: string; reason: string; idempotencyKey: string }
export interface DocumentSaveResult {
  contentSha256: string;
  notification: { status: "recorded" | "pending"; notifiedRuns: string[]; pendingRuns: string[] };
}
export function DocumentEditor(props: {
  document: DocumentReference;
  readDocument: (reference: DocumentReference) => Promise<DocumentSnapshot>;
  saveDocument: (reference: DocumentReference, request: DocumentSaveRequest) => Promise<DocumentSaveResult>;
  onClose: () => void;
  onAttentionChange?: (attention: "clean" | "draft" | "pending_notice" | "uncertain_save", draftDirty: boolean) => void;
  onSavingChange?: (saving: boolean) => void;
}) {
  // A selected document has its own state, including a private, in-memory draft.
  return <EditorSession key={`${props.document.projectId}:${props.document.evidenceId}:${props.document.index}`} {...props} />;
}
function EditorSession({ document, readDocument, saveDocument, onClose, onAttentionChange, onSavingChange }: Parameters<typeof DocumentEditor>[0]) {
  const [snapshot, setSnapshot] = useState<DocumentSnapshot | null>(null);
  const [content, setContent] = useState("");
  const [reason, setReason] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [result, setResult] = useState<DocumentSaveResult | null>(null);
  const [discard, setDiscard] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [pendingNotice, setPendingNotice] = useState<DocumentSaveRequest | null>(null);
  const [revisionConflict, setRevisionConflict] = useState(false);
  const [uncertainSave, setUncertainSave] = useState<DocumentSaveRequest | null>(null);
  const reader = useRef(readDocument);
  reader.current = readDocument;
  const busy = useRef(false);
  const live = useRef(true);
  const dirty = snapshot !== null && content !== snapshot.content;
  const attention = uncertainSave ? "uncertain_save" : pendingNotice ? "pending_notice" : dirty ? "draft" : "clean";
  const needsAttention = attention !== "clean";
  useEffect(() => { onAttentionChange?.(attention, dirty); }, [attention, dirty, onAttentionChange]);
  useEffect(() => { onSavingChange?.(saving); }, [saving, onSavingChange]);
  useEffect(() => () => { onAttentionChange?.("clean", false); onSavingChange?.(false); }, [onAttentionChange, onSavingChange]);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  useEffect(() => {
    let current = true;
    setError("");
    void reader.current(document).then((value) => {
      if (current) { setSnapshot(value); setContent(value.content); }
    }).catch(() => { if (current) setError("Could not open this project file. Try again."); });
    return () => { current = false; };
  }, [document.projectId, document.evidenceId, document.index, attempt]);
  useEffect(() => {
    if (!needsAttention) return;
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ""; };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, [needsAttention]);
  async function save(retryNotice?: DocumentSaveRequest) {
    if (!snapshot || busy.current || (!retryNotice && (!dirty || !reason.trim() || pendingNotice || uncertainSave || revisionConflict))) return;
    const retrying = retryNotice !== undefined;
    busy.current = true; setSaving(true); onSavingChange?.(true); setError(""); setResult(null);
    let request: DocumentSaveRequest | null = null;
    let saveResponseReceived = false;
    try {
      if (retryNotice) request = retryNotice;
      else {
        request = { content, expectedSha256: snapshot.contentSha256, reason: reason.trim(), idempotencyKey: newIdempotencyKey() };
      }
      const response = await saveDocument(document, request);
      saveResponseReceived = true;
      if (!live.current) return;
      if (retrying) {
        const current = await reader.current(document);
        if (!live.current) return;
        if (current.contentSha256 !== response.contentSha256) {
          setPendingNotice(response.notification.status === "pending" ? request : null);
          setRevisionConflict(true);
          setError("The file changed while confirming this save. Your draft is preserved. Copy your draft before reopening the latest version.");
          setUncertainSave(null);
          return;
        }
        setSnapshot(current);
      } else {
        setSnapshot({ content: request.content, contentSha256: response.contentSha256 });
      }
      setResult(response);
      setPendingNotice(response.notification.status === "pending" ? request : null);
      setUncertainSave(null);
    } catch (failure) {
      if (!live.current) return;
      const status = failure && typeof failure === "object" ? ("httpStatus" in failure ? failure.httpStatus : "status" in failure ? failure.status : null) : null;
      const diagnostics = failure && typeof failure === "object" && "diagnostics" in failure ? failure.diagnostics : null;
      const conflict = !saveResponseReceived && status === 409 && Array.isArray(diagnostics) && diagnostics.some((entry: unknown) => entry !== null && typeof entry === "object" && "path" in entry && entry.path === "/expectedSha256");
      const definitiveFailure = !saveResponseReceived && (status === 400 || conflict);
      setUncertainSave(definitiveFailure ? null : request);
      setRevisionConflict(conflict);
      setError(conflict
        ? "The file changed since you opened it. Your draft is preserved. Copy your draft before reopening the latest version."
        : definitiveFailure
          ? `${failure instanceof Error ? failure.message : "Save was rejected."} Your draft is preserved. Correct it and save again.`
          : "Save was not confirmed. Your draft is preserved. Retry to check the same save.");
    } finally { busy.current = false; onSavingChange?.(false); if (live.current) setSaving(false); }
  }
  return <aside className="document-editor" aria-label="Project document editor" onKeyDown={(event) => {
    if (event.key !== "Escape" || saving) return;
    event.stopPropagation();
    if (discard) setDiscard(false);
    else if (needsAttention) setDiscard(true);
    else onClose();
  }}>
    <header><div className="delivery-heading"><FileText size={18} /><h2>{document.title || "Project document"}</h2></div><button aria-label="Close document editor" disabled={saving} onClick={() => needsAttention ? setDiscard(true) : onClose()}><X size={18} /></button></header>
    <p className="document-path">{document.path}</p>
    <div className="document-scope"><strong>Main project file</strong><p>Saving changes the project’s main folder. Other runs linked to this project receive a change notice.</p></div>
    {!snapshot && !error && <p role="status">Opening project file…</p>}
    {error && <div role="alert" className="document-error"><p>{error}</p>{!snapshot && <button onClick={() => setAttempt((value) => value + 1)}>Retry opening file</button>}</div>}
    {snapshot && <>
      <div className="document-draft-status" role="status">{saving ? "Saving…" : dirty ? "Unsaved draft" : result ? "Saved to main project folder" : "Current project version"}</div>
      <label className="document-content-label">File content<textarea aria-label="File content" spellCheck={false} value={content} disabled={saving} onChange={(event) => { setContent(event.target.value); setResult(null); }} /></label>
      <label>Why are you changing this?<textarea aria-label="Why are you changing this?" className="document-reason" value={reason} disabled={saving} placeholder="Explain what changed so agents can reassess their work." onChange={(event) => { setReason(event.target.value); }} /></label>
      <button className="document-save" disabled={!dirty || !reason.trim() || saving || pendingNotice !== null || uncertainSave !== null || revisionConflict} onClick={() => void save()}>{saving ? "Saving…" : "Save project file"}</button>
      {uncertainSave && <div className="document-notice"><p>Save was not confirmed. Retry this save before starting another change.</p><button disabled={saving} onClick={() => void save(uncertainSave)}>Retry save</button></div>}
      {pendingNotice && <div className="document-notice"><p>Run notices are pending for the last save. Retry these before saving another change.</p><button disabled={saving} onClick={() => void save(pendingNotice)}>Retry run notices</button></div>}
      {result && <div role="status" className="document-notice">{result.notification.status === "pending" ? "File saved. Some run notifications are still pending." : `Change notice recorded for ${result.notification.notifiedRuns.length} run(s).`} Agents reading or changing their plans has not been confirmed.</div>}
    </>}
    {discard && <div className="document-discard" role="alertdialog" aria-label="Close project document"><p>{uncertainSave ? "This save may have committed. Closing abandons its retry key and any pending run notifications." : dirty ? "Discard this unsaved draft?" : "Run notices are still pending. Close without retrying?"}</p>{!uncertainSave && dirty && pendingNotice && <p>Run notices are also pending for the last save.</p>}<button disabled={saving} onClick={() => setDiscard(false)}>Keep editing</button><button disabled={saving} onClick={() => { if (!busy.current) onClose(); }}>{uncertainSave ? "Close and abandon save" : dirty ? "Discard draft" : "Close anyway"}</button></div>}
  </aside>;
}
