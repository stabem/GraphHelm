import { useState } from "react";

import { draftGraph, MAX_OBJECTIVE_LENGTH, newExecutionId } from "./draft-graph";
import type { Runtime } from "../runtime/client";

/**
 * Start a team run: the owner writes the objective; the run starts as the one-node graph of
 * draft-graph.ts. The run id is minted once per form, so a resend after an unclear outcome
 * carries the same idempotency key and cannot start a second run.
 */
export function StartRun({ runtime, onStarted, onCancel }: { runtime: Runtime; onStarted: (run: string) => void; onCancel: () => void }) {
  const [executionId] = useState(newExecutionId);
  const [objective, setObjective] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <form
      aria-label="New team run"
      className="start"
      onSubmit={(event) => {
        event.preventDefault();
        if (busy) return;
        setBusy(true);
        setError(null);
        void (async () => {
          try {
            await runtime.startRun(executionId, draftGraph(executionId, objective));
            onStarted(executionId);
          } catch (cause) {
            setError(cause instanceof Error ? cause.message : "The run did not start.");
            setBusy(false);
          }
        })();
      }}
    >
      <label htmlFor="objective">What should the team do?</label>
      <textarea id="objective" maxLength={MAX_OBJECTIVE_LENGTH} value={objective} onChange={(event) => setObjective(event.target.value)} autoFocus />
      {error !== null && <p className="error" role="alert">{error}</p>}
      <div className="start-actions">
        <button type="button" className="b ghost" onClick={onCancel}>Cancel</button>
        <button type="submit" className="b act" disabled={busy || objective.trim().length === 0}>{busy ? "Starting…" : "Start"}</button>
      </div>
    </form>
  );
}
