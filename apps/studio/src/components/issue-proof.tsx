import "./proof-table.css";

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
