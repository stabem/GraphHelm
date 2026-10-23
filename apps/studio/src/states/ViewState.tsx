import { useEffect, useState, type ReactNode } from "react";

/** #1934: the states every view can be in (docs/design/studio/States.dc.html). Graph, Lanes and
 * Proof show these instead of a bare sentence. */

const NOT_ANSWERING = "The Runtime is not answering.";
/** Seconds between automatic retries while the Runtime does not answer. */
export const RETRY_SECONDS = 5;

export type FailureKind = "offline" | "restarting" | "error";

/** Which state a failed read is: the Runtime unreachable, restarting, or an answer it could not use. */
export function failureKind(message: string): FailureKind {
  if (message === NOT_ANSWERING) return "offline";
  if (/restart/i.test(message)) return "restarting";
  return "error";
}

/** How long a first read may take before the view says the Runtime is not answering. */
export const FIRST_READ_MS = 8_000;

/** `read`, bounded: a read that has not answered within `ms` fails as "not answering", so a view
 * never waits on a silent Runtime with a spinner forever. */
export function bounded<T>(read: Promise<T>, ms = FIRST_READ_MS): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(NOT_ANSWERING)), ms);
    read.then((value) => { clearTimeout(timer); resolve(value); }, (error: unknown) => { clearTimeout(timer); reject(error); });
  });
}

/** What a view shows before its first answer: loading until it either answers or fails. */
export function firstRead<T>(value: T | null, failure: string | null): "loading" | "failed" | "ready" {
  return value !== null ? "ready" : failure !== null ? "failed" : "loading";
}

/** The next value of the retry countdown: one second less, and back to the top once it fires. */
export function countdown(left: number, seconds = RETRY_SECONDS): { left: number; fire: boolean } {
  return left <= 1 ? { left: seconds, fire: true } : { left: left - 1, fire: false };
}

function Panel({ label, kind, children, busy }: { label: string; kind: string; children: ReactNode; busy?: boolean }) {
  return (
    <section aria-label={label} aria-busy={busy || undefined} className="state-panel">
      <span className="mono state-kind">{kind}</span>
      {children}
    </section>
  );
}

/** Loading: a spinner, the sentence, and a skeleton of the board to come. */
export function Loading({ text }: { text: string }) {
  return (
    <Panel label="Loading" kind="Loading" busy>
      <div role="status" className="state-row"><span className="spin" aria-hidden="true" /><span>{text}</span></div>
      <div className="skeleton" aria-hidden="true">
        {[[72, 56, 72], [56, 72]].map((cards, column) => (
          <div key={column} className="skeleton-col">
            <div className="sk sk-head" />
            {cards.map((height, index) => <div key={index} className="sk" style={{ height }} />)}
          </div>
        ))}
      </div>
    </Panel>
  );
}

/** The Runtime is not answering: Retry now, and an automatic retry counting down. */
export function Offline({ target, onRetry }: { target: string; onRetry: () => void }) {
  const [left, setLeft] = useState(RETRY_SECONDS);
  useEffect(() => {
    const timer = setInterval(() => {
      setLeft((prior) => {
        const next = countdown(prior);
        if (next.fire) queueMicrotask(onRetry);
        return next.left;
      });
    }, 1000);
    return () => clearInterval(timer);
  }, [onRetry]);
  return (
    <Panel label="Runtime not answering" kind="Offline">
      <svg className="state-ico" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="#ff6b6b" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M9 2v5" /><path d="M15 2v5" /><path d="M6 7h12v4a6 6 0 0 1-12 0z" /><path d="M12 17v5" /><path d="M3 3l18 18" /></svg>
      <h2 className="state-title">The Runtime is not answering</h2>
      <span className="muted">GraphHelm couldn't reach {target}. It may be restarting.</span>
      <div className="state-row">
        <button type="button" className="b act" onClick={() => { setLeft(RETRY_SECONDS); onRetry(); }}>Retry</button>
        <span role="status" className="mono faint">Retrying in {left} s</span>
      </div>
    </Panel>
  );
}

export function Restarting() {
  return (
    <Panel label="Restarting" kind="Restarting">
      <svg className="state-ico" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="#4ea1ff" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M20 12a8 8 0 1 1-2.3-5.7" /><path d="M20 4v5h-5" /></svg>
      <h2 className="state-title">The Runtime is restarting on a new version</h2>
      <span role="status" className="muted">This clears by itself.</span>
    </Panel>
  );
}

/** Nothing to show yet, with "Start a team run" when the Studio can start one. */
export function Empty({ title, hint, onStart }: { title: string; hint: string; onStart?: (() => void) | null }) {
  return (
    <Panel label="Empty" kind="Empty">
      <svg className="state-ico" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="#a2a2ad" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 9h18" /><path d="M9 4v16" /></svg>
      <h2 className="state-title">{title}</h2>
      <span className="muted">{hint}</span>
      {onStart && <div><button type="button" className="b act" onClick={onStart}>Start a team run</button></div>}
    </Panel>
  );
}

/** An answer the Studio could not use: what failed, the technical detail folded, and Copy details. */
export function Failure({ title, message, detail }: { title: string; message: string; detail: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard?.writeText(detail).then(() => setCopied(true), () => setCopied(false));
  };
  return (
    <Panel label="Error" kind="Error">
      <svg className="state-ico" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="#ff8a3d" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M12 3l10 18H2z" /><path d="M12 10v5" /><path d="M12 18h.01" /></svg>
      <h2 className="state-title">{title}</h2>
      <span role="alert" className="muted">{message}</span>
      <details className="state-detail">
        <summary>Technical detail</summary>
        <pre className="mono">{detail}</pre>
      </details>
      <div className="state-row"><button type="button" className="b ghost" onClick={copy}>Copy details</button>{copied && <span role="status" className="sent">Copied</span>}</div>
    </Panel>
  );
}

/** A failed read as the state it is: offline, restarting, or an error naming what could not be read. */
export function ReadFailure({ what, route, message, onRetry }: { what: string; route: string; message: string; onRetry: () => void }) {
  const kind = failureKind(message);
  if (kind === "offline") return <Offline target="the Runtime" onRetry={onRetry} />;
  if (kind === "restarting") return <Restarting />;
  return <Failure title={`The ${what} could not be read`} message={message} detail={`GET ${route}\nerror: ${message}`} />;
}
