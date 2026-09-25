/**
 * The first node's chat: pick a model, say what the task is, and start.
 *
 * A NEW TASK IS A DRAFT, NOT A RUN, until this is sent. Nothing exists in the event store while
 * the operator is typing - no execution id is reserved, no graph is published, no node is waiting.
 * That is why this surface is a composer rather than a node panel: a panel describes something
 * that happened, and nothing has.
 *
 * THE MODEL PICKER IS THE RUNTIME'S OWN LIST. It is `GET /v1/gateway/routes`, not a hard-coded set
 * of model names: a list this page invented would offer models the Runtime cannot reach and hide
 * the ones it can. Routes the manifest marks disabled are shown and unselectable rather than
 * hidden, because "the model I want is missing" and "the model I want is turned off" send an
 * operator to two different places.
 */

import { useState, type FormEvent } from "react";
import { Send, TriangleAlert } from "lucide-react";

import { MAX_OBJECTIVE_LENGTH } from "../graph/draft";
import type { ModelRouteSummary } from "../runtime/types";
import { sendsOnEnter } from "./keys";

export interface RouteChoice {
  /** `false` when the Runtime was started without a gateway manifest. See `listRoutes`. */
  configured: boolean;
  routes: ModelRouteSummary[];
}

/** What to call a route in the picker: the model when the manifest declares one, the route id
 * otherwise. Never a guess - a route with no model gets its id, which is at least true. */
function labelOf(route: ModelRouteSummary): string {
  return route.model === null ? route.id : `${route.model} · ${route.id}`;
}

export function Composer({
  choice,
  busy,
  error,
  onSend,
  onCancel,
  objective,
  onObjectiveChange,
}: {
  choice: RouteChoice | null;
  busy: boolean;
  error: string;
  onSend: (objective: string, route: string | null) => void;
  onCancel: () => void;
  /** #1098 D1: the half-typed objective is owned by the PAGE, not by this component. Selecting a
   * run unmounts the composer, and a sentence that lived only in local state died with it. The
   * page keeps it so the next open restores it; `discard` is what erases it. */
  objective: string;
  onObjectiveChange: (objective: string) => void;
}) {
  const [route, setRoute] = useState("");

  const usable = choice?.routes.filter((entry) => entry.enabled) ?? [];
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (objective.trim().length === 0 || busy) return;
    onSend(objective, route === "" ? null : route);
  };

  return (
    <section className="panel" aria-label="New task">
      <header className="panel-head calm">
        <i aria-hidden="true" />
        <div style={{ minWidth: 0 }}>
          <h2>New task</h2>
          <p className="lbl">a draft — nothing is saved until you send</p>
        </div>
        {/* Gated on `busy` like its three siblings below - it was the one control that was not
          * (PR #467 review), and an ungated discard while startTask is in flight throws away a
          * draft whose pending call can still land state for it. */}
        <button
          type="button"
          className="ghost close"
          disabled={busy}
          onClick={onCancel}
          aria-label="Discard this task"
        >
          discard
        </button>
      </header>

      <form className="composer" onSubmit={submit}>
        <label className="lbl" htmlFor="composer-route">
          Model
        </label>
        <select
          id="composer-route"
          value={route}
          disabled={busy || usable.length === 0}
          onChange={(event) => setRoute(event.target.value)}
        >
          <option value="">
            {usable.length === 0 ? "none wired yet" : "the Runtime's default"}
          </option>
          {choice?.routes.map((entry) => (
            <option key={entry.id} value={entry.id} disabled={!entry.enabled}>
              {labelOf(entry)}
              {entry.enabled ? "" : " (disabled)"}
            </option>
          ))}
        </select>

        {choice !== null && !choice.configured && (
          /* UNDER THE MODEL FIELD, because that is what it is about, and quiet, because the task
           * is startable either way: it publishes the graph and creates the run, and the first
           * node waits. Only the model call is missing.
           *
           * An earlier version put this above everything as a full-width amber block, before the
           * operator had typed a word - a configuration lecture standing where the work goes. The
           * fact is worth saying (the Runtime only ever says it AFTER a send, in a diagnostic
           * nobody reads) and it was not worth shouting. */
          <p className="hint" role="status">
            No model is wired to this Runtime, so the first node will wait instead of thinking.
            Restart <code>graphhelm serve</code> with a gateway manifest to give it one.
          </p>
        )}

        <label className="lbl" htmlFor="composer-objective">
          What should this task do?
        </label>
        <textarea
          id="composer-objective"
          value={objective}
          rows={6}
          maxLength={MAX_OBJECTIVE_LENGTH}
          disabled={busy}
          placeholder="Describe the work in your own words. It becomes the first node's objective, verbatim."
          onChange={(event) => onObjectiveChange(event.target.value)}
          // Enter sends, like every chat box on this page; Shift+Enter breaks the line. The
          // judge pressed Enter and nothing happened and nothing said why (#1077, MINOR).
          // #1083 F5: `sendsOnEnter` is shared with the run's message box (keys.ts).
          onKeyDown={(event) => {
            if (!sendsOnEnter(event)) return;
            event.preventDefault();
            if (objective.trim().length === 0 || busy) return;
            onSend(objective, route === "" ? null : route);
          }}
        />
        <p className="lbl composer-hint">Enter sends · Shift+Enter for a new line</p>

        {error !== "" && (
          <p className="notice bad" role="alert">
            <TriangleAlert aria-hidden="true" />
            <span>{error}</span>
          </p>
        )}

        <button type="submit" className="send" disabled={busy || objective.trim().length === 0}>
          <Send aria-hidden="true" />
          {busy ? "starting" : "start this task"}
        </button>
      </form>

      <p className="panel-foot">
        Sending publishes a one-node graph and starts it. What comes after the first node is
        proposed by the agent and published by the Graph Governor - this surface does not write
        topology, and neither does the model.
      </p>
    </section>
  );
}
