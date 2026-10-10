import "./proof-table.css";
import { useState } from "react";
import { EVIDENCE_SYM, type ProofRowData } from "../runtime/proof-rows";

interface Props {
  /** `#<issue> <title>` (the group label). */
  label: string;
  /** The mono head line above the question. */
  cap: string;
  rows: ProofRowData[];
  /** Opens the test canvas on the row's journey step. */
  onOpenTest(row: ProofRowData): void;
  /** Absent: the Studio cannot send, so the ask button is disabled. */
  onAsk?: (row: ProofRowData) => Promise<unknown>;
}

/** The artboard's mini browser, drawn when no frame was recorded. */
export function MiniBrowser({ tone }: { tone: string }) {
  return (
    <span className="proof-mini" aria-hidden="true">
      <span className="proof-mini-bar"><i /><i /><i /></span>
      <span className="proof-mini-body">
        <span className="proof-mini-side"><i /><i /><i /></span>
        <span className="proof-mini-main"><span className="proof-mini-hl" data-state={tone} /><span className="proof-mini-line" /><span className="proof-mini-line" data-short="true" /></span>
      </span>
    </span>
  );
}

function Row({ r, onOpenTest, onAsk }: { r: ProofRowData; onOpenTest: Props["onOpenTest"]; onAsk: Props["onAsk"] }) {
  const [asked, setAsked] = useState<"idle" | "busy" | "done" | "failed">("idle");
  const hasTest = r.frame.contractId !== null;
  const ask = () => {
    if (!onAsk) return;
    setAsked("busy");
    onAsk(r).then(() => setAsked("done"), () => setAsked("failed"));
  };
  const frameBody = (
    <>
      {r.frame.src ? <img src={r.frame.src} alt={`Replay frame for ${r.pr !== null ? `#${r.pr}` : r.title}`} /> : <MiniBrowser tone={r.tone} />}
      <span className="proof-frame-txt"><span>{r.frame.caption}</span>{hasTest && <span className="proof-frame-go">open test →</span>}</span>
    </>
  );
  return (
    <li className="proof-row" data-state={r.tone} data-alarm={r.alarm} aria-label={`Row ${r.n}: ${r.title}`}>
      <div className="proof-n"><span>{r.n}</span><span className="proof-dot" data-state={r.tone} /></div>
      <div className="proof-promise">
        <span className="proof-name" title={r.title}>{r.title}</span>
        {r.promise && <span className="proof-sub proof-clamp" title={r.promise}>{r.promise}</span>}
        <span className="proof-badge" data-state={r.tone}>{r.status}</span>
      </div>
      {hasTest
        ? <button type="button" className="proof-frame" data-state={r.tone} data-alarm={r.alarm} aria-label={`Open the test canvas for ${r.title}`} onClick={() => onOpenTest(r)}>{frameBody}</button>
        : <div className="proof-frame" data-state={r.tone} data-alarm={r.alarm} data-static="true">{frameBody}</div>}
      <div className="proof-chain">
        <div className="proof-chiprow">
          {r.chips.map((c, i) => <span key={i} className="proof-chip" data-tone={c.tone}><span className="proof-stage">{c.stage}</span>{` ${c.who}`}</span>)}
        </div>
        {r.evidence.map((e, i) => (
          <div key={i} className="proof-ev">
            <span className="proof-ev-mark" data-mark={e.mark}>{EVIDENCE_SYM[e.mark]}</span>
            {e.href ? <a className="proof-ev-txt" href={e.href} target="_blank" rel="noreferrer" title={e.text}>{e.text}</a>
              : <span className="proof-ev-txt" title={e.text}>{e.text}</span>}
          </div>
        ))}
      </div>
      <div className="proof-call">
        {r.call.kind === "ask" ? (
          <button type="button" className="proof-primary" disabled={!onAsk || asked === "busy" || asked === "done"} onClick={ask}>
            {asked === "done" ? "Asked" : r.call.label}
          </button>
        ) : r.call.kind === "test" ? (
          <button type="button" className="proof-primary" onClick={() => onOpenTest(r)}>{r.call.label}</button>
        ) : r.prUrl ? (
          <a className={r.call.primary ? "proof-primary" : "proof-secondary"} href={r.prUrl} target="_blank" rel="noreferrer">{r.call.label}</a>
        ) : <button type="button" className="proof-secondary" disabled>{r.call.label}</button>}
        <span className="proof-hint">{asked === "failed" ? "Could not send the ask. Try again." : r.call.hint}</span>
      </div>
    </li>
  );
}

/** #735: Proof for issue work, one row per PR in the Proof artboard's grid; merged rows fold. */
export function IssueProof({ label, cap, rows, onOpenTest, onAsk }: Props) {
  const open = rows.filter((r) => r.open), merged = rows.filter((r) => !r.open);
  // With nothing open, the merged rows are the whole story: they start unfolded.
  const [showMerged, setShowMerged] = useState(open.length === 0);
  const cols = <div className="proof-cols" aria-hidden="true"><span>PR</span><span>Promise</span><span>Replay saw</span><span>Chain of custody · evidence</span><span>Your call</span></div>;
  return (
    <section className="proof" aria-label="Proof">
      <div className="proof-head">
        <div className="proof-title">
          <span className="proof-cap">{cap}</span>
          <h2>{`Can I trust “${label}”?`}</h2>
          <span className="proof-sub">Every PR is a promise. Each row shows who built it, what checked it, and the frame the replay saw.</span>
        </div>
      </div>
      {open.length > 0 && cols}
      {rows.length === 0 && <p className="proof-sub">No PR recorded for this work yet.</p>}
      {open.length > 0 && <ol className="proof-rows" aria-label="PRs">
        {open.map((r) => <Row key={r.key} r={r} onOpenTest={onOpenTest} onAsk={onAsk} />)}
      </ol>}
      {merged.length > 0 && (
        <>
          <button type="button" className="proof-fold" aria-expanded={showMerged} onClick={() => setShowMerged((v) => !v)}>
            {`${showMerged ? "▾" : "▸"} Merged · ${merged.length}`}
          </button>
          {showMerged && open.length === 0 && cols}
          {showMerged && <ol className="proof-rows" aria-label="Merged PRs">{merged.map((r) => <Row key={r.key} r={r} onOpenTest={onOpenTest} onAsk={onAsk} />)}</ol>}
        </>
      )}
    </section>
  );
}
