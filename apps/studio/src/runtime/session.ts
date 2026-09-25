/**
 * Getting a session without asking anyone to type a credential.
 *
 * In the ordinary loop the operator runs `graphhelm serve` and `npm run dev` on one machine. The
 * Runtime has already written the bearer token to disk; the dev server reads that same file and
 * hands it to the page (see `devSession` in `vite.config.ts`). So the Studio opens connected and
 * nobody copies a hex string out of a terminal.
 *
 * IT IS A CONVENIENCE, NOT A GUARANTEE, and the difference is the whole design here. A production
 * bundle served from anywhere else has no such endpoint, so this returns `null` and the page asks
 * for the token as before. The absence is expected, not an error, and it is never reported as
 * one — a page that shouted "session unavailable" at every non-dev deployment would be crying
 * about its own correct behaviour.
 *
 * The token still never leaves memory once it arrives. This module hands it to `RuntimeClient`
 * and keeps no copy.
 */

import type { RuntimeEvent } from "./types";

/** Where the dev server offers it. Same-origin by construction: a relative path cannot be pointed
 * at another host by configuration or by a stray environment variable. */
const SESSION_PATH = "/__studio/session";

/** A bound on what will be accepted as a token, so a misconfigured endpoint returning a page of
 * HTML cannot become an `Authorization` header. */
const MAX_TOKEN_LENGTH = 512;

export interface DevSession {
  token: string;
  /** What to call the folder this Runtime serves, when the operator named it
   * (`GRAPHHELM_PROJECT`). `null` leaves the rail saying what it can honestly say. */
  project: string | null;
  /** The absolute source folder, when the local launcher can identify it. */
  projectPath?: string | null;
}

export async function devSession(
  fetchImpl: typeof fetch = globalThis.fetch.bind(globalThis),
  search: string = globalThis.location?.search ?? "",
): Promise<DevSession | null> {
  // The page presents the nonce it was OPENED with (`?session=<nonce>`, from the URL the dev
  // server printed to its own terminal). Without one there is nothing to present, so the ask is
  // skipped entirely and the connect gate is the answer - the endpoint would refuse anyway, and
  // it refuses precisely so that a request another local user can forge earns nothing.
  const nonce = new URLSearchParams(search).get("session");
  if (nonce === null || nonce.length === 0) return null;
  let response: Response;
  try {
    response = await fetchImpl(`${SESSION_PATH}?nonce=${encodeURIComponent(nonce)}`, {
      headers: { Accept: "application/json" },
    });
  } catch {
    return null;
  }
  if (!response.ok) return null;

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    return null;
  }
  if (payload === null || typeof payload !== "object") return null;

  const token = (payload as { token?: unknown }).token;
  if (typeof token !== "string") return null;
  const trimmed = token.trim();
  if (trimmed.length === 0 || trimmed.length > MAX_TOKEN_LENGTH) return null;

  const project = (payload as { project?: unknown }).project;
  const named = typeof project === "string" ? project.trim() : "";
  const projectPath = (payload as { projectPath?: unknown }).projectPath;
  const folder = typeof projectPath === "string" ? projectPath.trim() : "";
  return {
    token: trimmed,
    // Bounded and rendered as text: it is a label from the operator's own environment, but it
    // reaches the DOM and nothing else validates it.
    project: named.length > 0 && named.length <= 120 ? named : null,
    projectPath: folder.length > 0 && folder.length <= 4096 && !/[\u0000-\u001f\u007f]/.test(folder) ? folder : null,
  };
}

/**
 * What one session told the room about its own model and effort.
 *
 * `effort` is the closed vocabulary the Runtime enforces (`low | medium | high`); `model` is an
 * opaque string, because the set of models changes faster than this repository ships and an enum
 * here would refuse a real declaration for no reason the operator could see.
 */
export interface AgentPresence {
  model: string;
  effort?: "low" | "medium" | "high";
}

/**
 * The newest `agent_presence_declared` for each actor, across a run's whole event stream.
 *
 * NEWEST-WINS, PER ACTOR - not the stream's newest event overall. The Runtime records a fresh
 * declaration only when it differs from that ACTOR's own previous one (a model switch mid-run),
 * so one actor can own several of these events; this keeps the one with the highest `sequence`
 * for each actor and drops the rest. An actor this stream never heard declare anything is simply
 * absent from the result - there is no placeholder entry to render, because there is nothing the
 * log can honestly say about that actor's model.
 */
export function newestPresenceByActor(events: RuntimeEvent[]): Record<string, AgentPresence> {
  const newestSequence = new Map<string, number>();
  // PROTOTYPE-FREE, and not as a precaution (#1057, Codex P2). `actorId` is a caller-chosen
  // identifier, so `constructor` and `toString` are valid actor ids, and on a plain object literal
  // `presence[id]` answers the inherited member for both - a truthy value the board renders as a
  // badge for an actor that declared nothing. A null-prototype object has ordinary key semantics
  // for every string, and keeps the `Record` shape every caller already indexes into.
  const byActor: Record<string, AgentPresence> = Object.create(null) as Record<string, AgentPresence>;
  for (const event of events) {
    if (event.kind !== "agent_presence_declared") continue;
    const payload =
      event.payload !== null && typeof event.payload === "object"
        ? (event.payload as Record<string, unknown>)
        : {};
    const actorId = typeof payload.actorId === "string" ? payload.actorId : null;
    if (actorId === null) continue;
    const model = typeof payload.model === "string" ? payload.model : null;
    const seenAt = newestSequence.get(actorId);
    if (seenAt !== undefined && seenAt >= event.sequence) continue;
    newestSequence.set(actorId, event.sequence);
    // A NEWEST RECORD WITH NO MODEL IS ABSENCE, SAID OUT LOUD (#1057). The Runtime writes one when
    // a session names itself and declares no model, which is how a new session sharing a stable
    // actor id supersedes the previous session's declaration. Skipping it here would leave the old
    // model newest and the board would keep showing a dead session's model as the live one's - the
    // exact defect the event exists to close. So it counts for NEWEST and then REMOVES the entry.
    if (model === null) {
      delete byActor[actorId];
      continue;
    }
    const effort = payload.effort;
    byActor[actorId] =
      effort === "low" || effort === "medium" || effort === "high" ? { model, effort } : { model };
  }
  return byActor;
}
