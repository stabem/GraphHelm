import { useState, type FormEvent } from "react";
import { ArrowRight, KeyRound, LoaderCircle } from "lucide-react";

/**
 * The fallback gate.
 *
 * NOBODY SEES THIS IN THE ORDINARY LOOP. The dev server reads the token the Runtime already wrote
 * and hands the page a session, so the Studio opens connected. This screen exists for the case
 * that cannot: a built bundle served from somewhere with no such endpoint.
 *
 * The field is a password input and `autoComplete="off"`: the browser must not be invited to
 * persist a Runtime credential in a password manager keyed to this origin, because the whole point
 * of the in-memory rule is that closing the tab ends the session.
 */
export function Connect({
  onConnect,
  busy,
  error,
}: {
  onConnect: (token: string) => void;
  busy: boolean;
  error: string;
}) {
  const [value, setValue] = useState("");
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const token = value.trim();
    if (token) onConnect(token);
  };

  return (
    <main className="gate">
      <section className="gate-card" aria-labelledby="gate-title">
        <p className="lbl">graphhelm local studio</p>
        <h1 id="gate-title">Your agents are one token away</h1>
        <p className="dim">
          They work on a whiteboard: you watch, answer, and approve. Paste the bearer token{" "}
          <code>graphhelm serve</code> wrote beside your events directory.
        </p>
        <form onSubmit={submit}>
          <label htmlFor="token">Bearer token</label>
          <div className="field">
            <KeyRound aria-hidden="true" />
            <input
              id="token"
              type="password"
              value={value}
              onChange={(event) => setValue(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              placeholder="Paste the token…"
            />
          </div>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <button className="act" type="submit" disabled={busy || !value.trim()}>
            {busy ? <LoaderCircle className="spin" aria-hidden="true" /> : <ArrowRight aria-hidden="true" />}
            {busy ? "connecting" : "connect"}
          </button>
        </form>
        <p className="gate-note">
          You are seeing this because the page was not served by the Studio&apos;s own dev server —
          run <code>npm run dev</code> with <code>GRAPHHELM_EVENTS</code> set and it connects
          without asking. The token stays in this tab&apos;s memory either way: never storage,
          never a cookie, never a URL, never a log line.
        </p>
      </section>
    </main>
  );
}
