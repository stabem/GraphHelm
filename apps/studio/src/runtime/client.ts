/**
 * The one way the Studio talks to the Runtime.
 *
 * Both surfaces - the human interface and the WebMCP adapter - hold the SAME instance of this
 * class. That is the architectural claim the whole feature rests on: WebMCP is not a second
 * operational path, it is a thin cover over this client. Nothing in `webmcp/` builds a request,
 * chooses an actor, or interprets a diagnostic; it only calls the methods below and hands the
 * result back.
 *
 * THE TOKEN NEVER LEAVES THIS OBJECT. It is a private field, set once at construction, and it is
 * never written to `localStorage`, `sessionStorage`, a cookie, a URL, a log line, or an error
 * message. `dispose()` overwrites it so a client kept alive by a stale closure cannot keep
 * authenticating after the operator disconnects. `toJSON` is overridden because
 * `JSON.stringify(client)` is exactly how a secret reaches a log by accident.
 */

import type {
  Actor,
  Diagnostic,
  Envelope,
  EventPage,
  ExecutionPage,
  EvidenceContent,
  ExecutionStatus,
  GraphTopology,
  ModelRouteSummary,
  MutationEvidence,
  RuntimeEvent,
} from "./types";

/** The actor id the human interface records. */
export const OPERATOR_ACTOR: Actor = { id: "studio-operator", type: "owner" };
/**
 * The actor id every WebMCP-initiated mutation records.
 *
 * DELIBERATELY NOT `owner`. The browser asks the person to confirm an agent's tool call, and it
 * is tempting to read that confirmation as "the owner did it". It is not: the owner consented to
 * an action the AGENT chose, and an audit log that cannot tell those apart cannot answer the one
 * question it exists for. The consent is real and is what lets the call proceed; the authorship
 * stays with the adapter.
 */
export const WEBMCP_ACTOR: Actor = { id: "studio-webmcp-adapter", type: "agent" };

/** The largest event page the Studio will ask for. The API's own ceiling is 1000; this is the
 * Studio's smaller working bound, so a page always fits one render pass. */
export const MAX_EVENT_LIMIT = 200;
/** The API refuses a list limit above 100. Mirrored here so a bad value is refused before it
 * costs a round trip, with the same wording the Runtime would have used. */
export const MAX_LIST_LIMIT = 100;

/** The longest message the Studio will send into a run.
 *
 * The signal schema puts no upper bound on `description`, so this is the Studio's own limit. It
 * lives here, beside the check that enforces it, so the box that stops typing and the client that
 * refuses cannot drift apart and start disagreeing about what is too long. */
export const MAX_MESSAGE_LENGTH = 4000;

/** An identifier bound the same way the Runtime bounds an `OpaqueId`. */
const MAX_ID_LENGTH = 128;
const MUTATION_KEY_SUFFIX: Record<MutationEvidence["action"], string> = {
  start: "started",
  pause: "paused",
  approve: "outcome",
  resume: "resumed",
  signal: "record",
};

const MUTATION_DECISION_KIND: Record<MutationEvidence["action"], string> = {
  start: "execution_started",
  pause: "execution_paused",
  approve: "node_outcome_recorded",
  resume: "execution_resumed",
  signal: "signal_recorded",
};

function isAttributableDecision(
  event: RuntimeEvent,
  action: MutationEvidence["action"],
  node: string | null,
  actor: Actor,
  keyPrefix: string,
): boolean {
  if (
    event.idempotencyKey?.startsWith(keyPrefix) !== true ||
    event.kind !== MUTATION_DECISION_KIND[action] ||
    event.actorId !== actor.id ||
    event.actorType !== actor.type
  ) {
    return false;
  }
  if (action !== "approve") return true;
  return event.payload !== null &&
    typeof event.payload === "object" &&
    (event.payload as { nodeId?: unknown }).nodeId === node;
}

/**
 * A Runtime refusal, carrying the structured diagnostic rather than a flattened string.
 *
 * `message` is safe to render: it comes from the Runtime's own redaction-safe diagnostic vocabulary
 * (codes, JSON pointers, concise messages), which by contract carries no credential, backtrace, or
 * user-home path. Nothing from the request - the token above all - is ever folded into it here.
 */
export class RuntimeError extends Error {
  readonly httpStatus: number;
  readonly diagnostics: Diagnostic[];
  readonly code: string;

  constructor(message: string, httpStatus: number, diagnostics: Diagnostic[]) {
    super(message);
    this.name = "RuntimeError";
    this.httpStatus = httpStatus;
    this.diagnostics = diagnostics;
    this.code = diagnostics[0]?.code ?? "";
  }
}

/** Thrown when a caller keeps a reference to a disposed client. Its own class so the interface
 * can tell "you disconnected" apart from "the Runtime refused". */
export class DisconnectedError extends Error {
  constructor() {
    super("This Studio session was disconnected. Connect again to continue.");
    this.name = "DisconnectedError";
  }
}

export interface MutationOptions {
  /** The head sequence the caller believes it is racing against, sent as `If-Match`. */
  ifMatch?: number;
  /** Reused verbatim across retries of the SAME logical action. Minted per action when absent. */
  idempotencyKey?: string;
  actor?: Actor;
}

interface RequestOptions {
  method: "GET" | "POST";
  path: string;
  body?: unknown;
  headers?: Record<string, string>;
}

/** A caller-minted correlation id. `crypto.randomUUID` where available; a bounded random string
 * otherwise, because a Studio served over plain http on some browsers has no `randomUUID`. This
 * is NOT a security value - it correlates a retry with its first attempt - so a weaker source is
 * a correctness question, not a secrecy one. */
export function newIdempotencyKey(): string {
  const uuid = globalThis.crypto?.randomUUID?.();
  if (uuid) return uuid;
  const bytes = new Uint8Array(16);
  globalThis.crypto?.getRandomValues?.(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** The first `emittedAt` minted under an idempotency key, remembered so a retry under the same
 * key sends the identical envelope bytes - the Runtime folds the body into its derived key, and
 * a shifted timestamp turns a retry into a 409 divergent reuse.
 *
 * Bounded at 4,096 - a session-length retry window, not a ledger. Each entry is a key and a
 * timestamp (~100 bytes), so the cap costs nothing and a page would have to send four thousand
 * DISTINCT messages before its oldest retry identity ages out (PR #467 review flagged the old
 * 256 bound). Past the boundary the failure mode is deliberately LOUD, never silent: an evicted
 * key's retry mints a fresh timestamp, the Runtime refuses it as divergent reuse (409), and the
 * caller sees an error to act on - a duplicate append is the outcome this map exists to prevent,
 * and eviction can never produce one. */
const EMITTED_AT_BY_KEY = new Map<string, string>();
function stableEmittedAt(idempotencyKey: string): string {
  const held = EMITTED_AT_BY_KEY.get(idempotencyKey);
  if (held !== undefined) return held;
  const minted = new Date().toISOString();
  EMITTED_AT_BY_KEY.set(idempotencyKey, minted);
  if (EMITTED_AT_BY_KEY.size > 4096) {
    const oldest = EMITTED_AT_BY_KEY.keys().next().value;
    if (oldest !== undefined) EMITTED_AT_BY_KEY.delete(oldest);
  }
  return minted;
}

/** Refuses an id the Runtime would refuse anyway, before it costs a request - and before it is
 * interpolated into a path. */
function checkedId(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_ID_LENGTH) {
    throw new RuntimeError(`${field} must be a non-empty identifier of at most ${MAX_ID_LENGTH} characters.`, 0, []);
  }
  return value;
}

function normaliseEvent(raw: Record<string, unknown>): RuntimeEvent {
  const kind = raw.kind;
  const tagged = kind !== null && typeof kind === "object" ? (kind as { type?: unknown; data?: unknown }) : null;
  const actor = raw.actor !== null && typeof raw.actor === "object" ? (raw.actor as { id?: unknown; type?: unknown }) : null;
  return {
    sequence: typeof raw.sequence === "number" ? raw.sequence : 0,
    kind: tagged && typeof tagged.type === "string" ? tagged.type : typeof kind === "string" ? kind : "unknown",
    payload: tagged ? (tagged.data ?? null) : (raw.payload ?? null),
    occurredAt: typeof raw.occurredAt === "string" ? raw.occurredAt : null,
    actorId: actor && typeof actor.id === "string" ? actor.id : null,
    actorType: actor && typeof actor.type === "string" ? actor.type : null,
    idempotencyKey: typeof raw.idempotencyKey === "string" ? raw.idempotencyKey : null,
    eventId: typeof raw.eventId === "string" ? raw.eventId : null,
    evidenceRefs: normaliseEvidenceRefs(raw.evidenceRefs),
  };
}

/** The `evidenceId`s out of an event's `evidenceRefs`, dropping anything shaped otherwise.
 *
 * A reference carries more than its id, but the id is the only field this client acts on, so it is
 * the only one read. Silently skipping a malformed entry rather than failing the whole page is
 * deliberate: an unreadable reference costs one unopenable message, while a throw costs the
 * operator the entire log - including the events that are fine. */
function normaliseEvidenceRefs(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  const ids: string[] = [];
  for (const entry of raw) {
    if (entry === null || typeof entry !== "object") continue;
    const id = (entry as { evidenceId?: unknown }).evidenceId;
    if (typeof id === "string" && id.length > 0 && id.length <= 256) ids.push(id);
  }
  return ids;
}

export class RuntimeClient {
  #token: string | null;
  readonly #baseUrl: string;
  readonly #fetch: typeof fetch;

  constructor(token: string, options: { baseUrl?: string; fetch?: typeof fetch } = {}) {
    this.#token = token;
    // Empty by default: the dev server and the built bundle are both served from the same origin
    // as the Runtime (Vite proxies `/v1` in development), so a relative path keeps the token off
    // any cross-origin request.
    this.#baseUrl = options.baseUrl ?? "";
    this.#fetch = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  /** Forgets the token. Every later call refuses with `DisconnectedError` rather than silently
   * sending `Bearer null`. */
  dispose(): void {
    this.#token = null;
  }

  get connected(): boolean {
    return this.#token !== null;
  }

  /** Overridden so an accidental `JSON.stringify(client)` - in a log, a React devtools dump, an
   * error report - cannot serialise the token. */
  toJSON(): Record<string, string> {
    return { runtimeClient: this.connected ? "connected" : "disconnected" };
  }

  toString(): string {
    return "[RuntimeClient]";
  }

  async #request<T>({ method, path, body, headers = {} }: RequestOptions): Promise<T> {
    const token = this.#token;
    if (token === null) throw new DisconnectedError();

    let response: Response;
    try {
      response = await this.#fetch(`${this.#baseUrl}${path}`, {
        method,
        headers: {
          Authorization: `Bearer ${token}`,
          ...(body === undefined ? {} : { "Content-Type": "application/json" }),
          ...headers,
        },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch {
      // The transport error's own text is discarded: browsers put the request URL in it, and the
      // Studio must not turn a network hiccup into a place where a request detail is surfaced.
      throw new RuntimeError("The Runtime could not be reached at this address.", 0, []);
    }

    let envelope: Envelope<T>;
    try {
      envelope = (await response.json()) as Envelope<T>;
    } catch {
      throw new RuntimeError(
        response.status === 401
          ? "The bearer token was refused."
          : `The Runtime replied ${response.status} with a body this client could not read.`,
        response.status,
        [],
      );
    }

    if (!response.ok || envelope.ok !== true || envelope.data === null) {
      const diagnostics = Array.isArray(envelope.diagnostics) ? envelope.diagnostics : [];
      const message =
        diagnostics[0]?.message ??
        (response.status === 401 ? "The bearer token was refused." : `The Runtime replied ${response.status}.`);
      throw new RuntimeError(message, response.status, diagnostics);
    }
    return envelope.data;
  }

  /**
   * `GET /health` - a REACHABILITY probe, and only that.
   *
   * The Runtime exempts this path from its bearer check before it ever looks at the header
   * (`require_token` in `apps/cli/src/commands/serve/mod.rs` returns early on `/health`), so a
   * success here says the server is listening and says NOTHING about the token. The header is
   * sent anyway for uniformity, not as evidence.
   *
   * Authorisation is decided by the first real read the caller makes afterwards - in the page,
   * `listExecutions` - which is why the connect flow runs both and reports their failures
   * differently: "could not be reached" and "the token was refused" are separate facts, and a
   * single probe cannot tell them apart.
   */
  async health(): Promise<void> {
    await this.#request<unknown>({ method: "GET", path: "/health" });
  }

  async listExecutions(options: { after?: string; limit?: number } = {}): Promise<ExecutionPage> {
    const query = new URLSearchParams();
    if (options.after !== undefined) query.set("after", checkedId(options.after, "after"));
    if (options.limit !== undefined) {
      if (!Number.isInteger(options.limit) || options.limit < 1 || options.limit > MAX_LIST_LIMIT) {
        throw new RuntimeError(`limit must be between 1 and ${MAX_LIST_LIMIT}.`, 0, []);
      }
      query.set("limit", String(options.limit));
    }
    const suffix = query.toString();
    return this.#request<ExecutionPage>({
      method: "GET",
      path: suffix ? `/v1/executions?${suffix}` : "/v1/executions",
    });
  }

  async getStatus(executionId: string): Promise<ExecutionStatus> {
    const id = checkedId(executionId, "executionId");
    return this.#request<ExecutionStatus>({
      method: "GET",
      path: `/v1/executions/${encodeURIComponent(id)}`,
    });
  }

  /** `after` is EXCLUSIVE, matching the API: a caller that pages with the last sequence it saw
   * never re-reads an event and never skips one. */
  async getEvents(
    executionId: string,
    options: { after?: number; limit?: number } = {},
  ): Promise<EventPage> {
    const id = checkedId(executionId, "executionId");
    const after = options.after ?? 0;
    const limit = options.limit ?? 50;
    if (!Number.isInteger(after) || after < 0) {
      throw new RuntimeError("after must be a non-negative integer.", 0, []);
    }
    if (!Number.isInteger(limit) || limit < 1 || limit > MAX_EVENT_LIMIT) {
      throw new RuntimeError(`limit must be between 1 and ${MAX_EVENT_LIMIT}.`, 0, []);
    }
    const page = await this.#request<{ events: Record<string, unknown>[]; head: number }>({
      method: "GET",
      path: `/v1/executions/${encodeURIComponent(id)}/events?after=${after}&limit=${limit}`,
    });
    return {
      head: typeof page.head === "number" ? page.head : 0,
      events: (page.events ?? []).map(normaliseEvent),
    };
  }

  /**
   * `POST /v1/graph/topology` - a graph file's shape and the hash that says which graph it is.
   *
   * `file` is a path on the RUNTIME's machine, not the browser's, and it is treated as untrusted
   * local input exactly as `resume`'s is: bounded and relayed verbatim, never read, never opened,
   * never echoed back with its contents. A POST rather than a GET because a filesystem path in a
   * URL lands in request logs, browser history and any `Referer` the page sends onward.
   */
  async getTopology(file: string): Promise<GraphTopology> {
    if (typeof file !== "string" || file.length === 0 || file.length > 512) {
      throw new RuntimeError("file must be a non-empty path of at most 512 characters.", 0, []);
    }
    return this.#request<GraphTopology>({
      method: "POST",
      path: "/v1/graph/topology",
      body: { file },
    });
  }

  /**
   * Every mutation goes through here, and every mutation therefore produces the same evidence.
   *
   * READ THE HEAD, MUTATE AGAINST IT, READ BACK. The middle step alone answers "did the server
   * accept my request", which is not the question. What the caller needs to know is whether the
   * STORE moved and what it now says - so the head before, the head after, the status after, and
   * the directly attributable decision event come back together. Concurrent and downstream
   * events remain in the stream without being claimed as part of this action. An HTTP 200 on its
   * own is never reported as success here.
   *
   * A verification read that itself fails yields `result: "unknown"`, NOT `"succeeded"`. The
   * mutation may well have landed; this client simply cannot prove it, and saying so is the only
   * honest answer. Reporting the optimistic case would make the one state an operator must act
   * on look exactly like the state they can ignore.
   */
  async #verifiedMutation(
    action: MutationEvidence["action"],
    executionId: string,
    path: string,
    body: unknown,
    node: string | null,
    options: MutationOptions,
  ): Promise<MutationEvidence> {
    const id = checkedId(executionId, "executionId");
    const actor = options.actor ?? OPERATOR_ACTOR;
    const idempotencyKey = options.idempotencyKey ?? newIdempotencyKey();
    const attributableKeyPrefix = `${idempotencyKey}-${MUTATION_KEY_SUFFIX[action]}-`;

    let headBefore = options.ifMatch ?? -1;
    if (headBefore < 0) {
      const before = await this.getStatus(id);
      headBefore = before.headSequence;
    }

    const headers: Record<string, string> = {
      "Idempotency-Key": idempotencyKey,
      "X-GraphHelm-Actor": actor.id,
      "X-GraphHelm-Actor-Type": actor.type,
    };
    // Only pinned when there is a head to pin. `If-Match: 0` on a stream that has never been
    // written is a claim about a sequence streams do not issue.
    if (headBefore > 0) headers["If-Match"] = String(headBefore);

    let diagnostics: Diagnostic[] = [];
    let refused = false;
    let postAmbiguous = false;
    let confirmedHead: number | null = null;
    let recognizedRetry = false;
    try {
      const reply = await this.#request<{
        headSequence?: unknown;
        idempotency?: { recognizedRetry?: unknown };
      }>({ method: "POST", path, body, headers });
      recognizedRetry = reply.idempotency?.recognizedRetry === true;
      if (
        typeof reply.headSequence === "number" &&
        Number.isSafeInteger(reply.headSequence) &&
        reply.headSequence >= headBefore
      ) {
        confirmedHead = reply.headSequence;
      } else {
        postAmbiguous = true;
        diagnostics = [{
          code: "",
          severity: "error",
          message: "The Runtime accepted the mutation but returned no usable headSequence.",
          path: "/headSequence",
          source: "studio",
        }];
      }
    } catch (error) {
      if (error instanceof DisconnectedError) throw error;
      if (!(error instanceof RuntimeError)) throw error;
      // A 4xx response proves the Runtime rejected the request. Transport failures, unreadable
      // responses and server errors may arrive after the mutation committed, so they stay
      // ambiguous even when a later read observes head movement.
      refused = error.httpStatus >= 400 && error.httpStatus < 500;
      postAmbiguous = !refused;
      diagnostics = error.diagnostics.length > 0
        ? error.diagnostics
        : [{ code: "", severity: "error", message: error.message, path: "/", source: "studio" }];
    }

    let statusAfter: ExecutionStatus | null = null;
    let newEvents: RuntimeEvent[] = [];
    let verified = true;
    try {
      statusAfter = await this.getStatus(id);
      if (confirmedHead !== null && statusAfter.headSequence < confirmedHead) {
        verified = false;
      }
      const targetHead = confirmedHead ?? statusAfter.headSequence;
      if (!refused && verified && targetHead > headBefore) {
        let cursor = headBefore;
        while (cursor < targetHead) {
          const page = await this.getEvents(id, { after: cursor, limit: MAX_EVENT_LIMIT });
          const eventsThroughTarget = page.events.filter((event) => event.sequence <= targetHead);
          const nextCursor = eventsThroughTarget.at(-1)?.sequence;
          if (page.head < targetHead || nextCursor === undefined || nextCursor <= cursor) {
            verified = false;
            break;
          }
          newEvents.push(...eventsThroughTarget.filter(
            (event) => isAttributableDecision(event, action, node, actor, attributableKeyPrefix),
          ));
          cursor = nextCursor;
        }
      }
      if (confirmedHead !== null && confirmedHead > headBefore && verified && newEvents.length === 0) {
        verified = false;
      }
      // A HEAD THAT DID NOT MOVE proves nothing by itself. When the pre-read already saw the
      // committed head (a retry after the original landed), the confirmed head equals it, no
      // event scan runs, and the old shape reported `succeeded` on zero evidence (PR #467
      // review). Success without movement is claimed only when the Runtime itself says
      // `recognizedRetry` - its own statement that this exact identity already committed.
      if (confirmedHead !== null && confirmedHead === headBefore && !recognizedRetry) {
        verified = false;
      }
    } catch (error) {
      if (error instanceof DisconnectedError) throw error;
      verified = false;
    }

    const result: MutationEvidence["result"] = refused
      ? "refused"
      : verified && !postAmbiguous
        ? "succeeded"
        : "unknown";

    return {
      action,
      executionId: id,
      node,
      actor,
      idempotencyKey,
      headBefore,
      headAfter: statusAfter?.headSequence ?? null,
      result,
      statusAfter,
      newEvents,
      diagnostics,
    };
  }

  /**
   * `POST /v1/executions/{id}/pause`.
   *
   * The Studio exposes only the cooperative hold. The Runtime's immediate-stop path does not
   * currently preserve this request's actor and idempotency identity, so offering it here would
   * produce evidence the client cannot attribute to the caller.
   */
  async pause(
    executionId: string,
    options: MutationOptions = {},
  ): Promise<MutationEvidence> {
    return this.#verifiedMutation(
      "pause",
      executionId,
      `/v1/executions/${encodeURIComponent(checkedId(executionId, "executionId"))}/pause`,
      {},
      null,
      options,
    );
  }

  /** `POST /v1/executions/{id}/approve` - readies a blocked or ghost node. */
  async approve(executionId: string, node: string, options: MutationOptions = {}): Promise<MutationEvidence> {
    const nodeId = checkedId(node, "node");
    return this.#verifiedMutation(
      "approve",
      executionId,
      `/v1/executions/${encodeURIComponent(checkedId(executionId, "executionId"))}/approve`,
      { node: nodeId },
      nodeId,
      options,
    );
  }

  /**
   * `POST /v1/executions/{id}/resume`.
   *
   * `file` is a path on the RUNTIME's filesystem, not the browser's, and it is treated as
   * untrusted local input: it is bounded and relayed verbatim, never read, never opened, never
   * echoed back with its contents. The Runtime is the only thing that resolves it, and its own
   * refusal is what the caller sees when the path is wrong.
   */
  async resume(
    executionId: string,
    file: string,
    options: MutationOptions & { fixtures?: string } = {},
  ): Promise<MutationEvidence> {
    if (typeof file !== "string" || file.length === 0 || file.length > 512) {
      throw new RuntimeError("file must be a non-empty path of at most 512 characters.", 0, []);
    }
    const body: Record<string, string> = { file };
    if (options.fixtures !== undefined) {
      if (options.fixtures.length === 0 || options.fixtures.length > 512) {
        throw new RuntimeError("fixtures must be a path of at most 512 characters.", 0, []);
      }
      body.fixtures = options.fixtures;
    }
    return this.#verifiedMutation(
      "resume",
      executionId,
      `/v1/executions/${encodeURIComponent(checkedId(executionId, "executionId"))}/resume`,
      body,
      null,
      options,
    );
  }
  /**
   * The models this Runtime can actually reach, and whether it can reach any.
   *
   * TWO ANSWERS, NOT ONE, because "no models" has two very different causes and an operator has to
   * act differently on each. A Runtime started without `--manifest` is FIXTURE-ONLY: it will accept
   * a task, park the first node in `waiting_input`, and never call a model - the diagnostic for
   * that (`GHCLI021_FIXTURE_ONLY_WAITING_INPUT`) exists in the Runtime and reaches nobody until
   * somebody sends work and waits. A Runtime WITH a manifest that lists nothing enabled is a
   * different problem with a different fix. Collapsing them into an empty list would send an
   * operator to look at their manifest when they never wrote one.
   *
   * ONLY the "no configured manifest" refusal is read as fixture-only. Every other failure is
   * rethrown: a surface that swallowed all errors here would report a Runtime that is down as one
   * that is merely unwired.
   */
  async listRoutes(): Promise<{ configured: boolean; routes: ModelRouteSummary[] }> {
    try {
      const reply = await this.#request<{ routes: ModelRouteSummary[] }>({
        method: "GET",
        path: "/v1/gateway/routes",
      });
      return { configured: true, routes: reply.routes };
    } catch (error) {
      if (
        error instanceof RuntimeError &&
        error.httpStatus === 400 &&
        // The CODE too, not just the path: a manifest that exists but broke after startup also
        // fails at path /manifest (GHCLI009_GATEWAY_INVALID), and reading that as "fixture-only"
        // sends the operator away from a configuration error they can actually fix
        // (PR #467 review). Only the server's own "no configured manifest" refusal counts.
        error.diagnostics.some(
          (diagnostic) =>
            diagnostic.path === "/manifest" && diagnostic.code === "GHCLI001_ARGUMENT_INVALID",
        )
      ) {
        return { configured: false, routes: [] };
      }
      throw error;
    }
  }

  /**
   * Starts a task from a graph the browser composed, on a model the operator chose.
   *
   * The document travels in the request body. There is no path because there is no file: a browser
   * has no filesystem on the Runtime's machine, and writing one there just to name it would put a
   * document on disk that nothing afterwards reads.
   *
   * `route` is sent ONLY when chosen. Sending `null` would be a present field with an unusable
   * value, which the Runtime refuses - correctly, since it cannot tell that apart from a client
   * that meant something by it. Absent means "the server's own default", which is what an operator
   * who has not picked is asking for.
   */
  async startTask(
    executionId: string,
    graph: Record<string, unknown>,
    options: MutationOptions & { mode?: string; route?: string | null } = {},
  ): Promise<MutationEvidence> {
    const id = checkedId(executionId, "executionId");
    const body: Record<string, unknown> = {
      graph,
      mode: options.mode ?? "autopilot",
    };
    if (typeof options.route === "string" && options.route.length > 0) {
      if (options.route.length > 128) {
        throw new RuntimeError("route must be at most 128 characters.", 0, []);
      }
      body.route = options.route;
    }
    return this.#verifiedMutation(
      "start",
      executionId,
      `/v1/executions/${encodeURIComponent(id)}/start`,
      body,
      null,
      options,
    );
  }

  /**
   * Says something into an execution's log, from the person watching it.
   *
   * This is the operator's half of the loop: the agent leaves records as it works, and this is the
   * way back in. It is a signal because signals are the one thing in this system that carry
   * free-form words from outside and are still durable - the envelope seals into the Evidence store
   * BEFORE the event appends, so a message that is recorded is a message that can be read back.
   *
   * `type` is deliberately OUTSIDE the recognized set (`HARNESS_SPEC.md` §19). `signal.rs` records
   * an unrecognized kind as evidence and never lets it reach the Governor, which is exactly the
   * semantics a human note needs: it must be visible to whoever is watching and must never steer
   * the graph. Sending a recognized kind like `no_progress` to get a message across would make the
   * Studio's chat box a control surface by accident.
   *
   * No `evidenceOut` is sent. A browser has no path on the Runtime's host, and since 05d Task 9 a
   * keyring-backed Runtime seals the envelope itself. On a Runtime started WITHOUT a keyring there
   * is no durable copy a browser can name, and the Runtime refuses with a diagnostic saying so -
   * which is the honest outcome, not a gap: the alternative is a message that looks sent and was
   * never preserved.
   */
  async signal(
    executionId: string,
    message: string,
    options: MutationOptions & { emittedAt?: string; to?: string; replyTo?: string } = {},
  ): Promise<MutationEvidence> {
    const id = checkedId(executionId, "executionId");
    if (typeof message !== "string" || message.trim().length === 0) {
      throw new RuntimeError("A message cannot be empty.", 0, []);
    }
    if (message.length > MAX_MESSAGE_LENGTH) {
      throw new RuntimeError(`A message must be at most ${MAX_MESSAGE_LENGTH} characters.`, 0, []);
    }
    const actor = options.actor ?? OPERATOR_ACTOR;
    // THE BODY IS PART OF THE IDENTITY. The Runtime folds the canonical request body into its
    // derived idempotency key, so a retry under the same key must send byte-identical bytes -
    // a fresh envelope id or timestamp turns "the same logical action" into a 409 divergent
    // reuse (PR #467 review). The id derives from the key, and the first emittedAt minted for a
    // key is remembered and re-sent, so retrying with the caller's same key reconciles.
    const idempotencyKey = options.idempotencyKey ?? newIdempotencyKey();
    const emittedAt = options.emittedAt ?? stableEmittedAt(idempotencyKey);
    return this.#verifiedMutation(
      "signal",
      executionId,
      `/v1/executions/${encodeURIComponent(id)}/signal`,
      {
        signal: {
          id: `studio-message-${idempotencyKey}`,
          // The source kind follows the AUTHOR: an agent's message recorded as user speech
          // would forge provenance into immutable history (PR #467 review). The schema's enum
          // has no "agent", so the tool surface speaks as "tool".
          source: { type: actor.type === "owner" ? "user" : "tool", id: actor.id },
          type: "operator_note",
          severity: "low",
          description: message,
          // `minItems: 1`, and the execution being spoken about is the one thing a message from
          // the Studio always has to point at.
          evidence: [id],
          emittedAt,
          // Addressing (schema 1.1.0): present only when the caller addressed someone. Absent is
          // "said to the room"; a null would be a present field a 1.0.0 consumer refuses.
          ...(typeof options.to === "string" && options.to.length > 0 ? { to: options.to } : {}),
          ...(typeof options.replyTo === "string" && options.replyTo.length > 0
            ? { replyTo: options.replyTo }
            : {}),
        },
      },
      null,
      // The key the envelope id was derived from, so header and body agree even when the key
      // was minted just above.
      { ...options, idempotencyKey },
    );
  }

  /**
   * The content behind an evidence reference - a model's reply, a tool's output - opened.
   *
   * The event stream carries the reference and the token counts; the words themselves are sealed
   * (D-036). This is how a thread shows what a node actually said rather than only that it said
   * something.
   */
  async readEvidence(executionId: string, evidenceId: string): Promise<EvidenceContent> {
    const id = checkedId(executionId, "executionId");
    if (typeof evidenceId !== "string" || evidenceId.length === 0 || evidenceId.length > 256) {
      throw new RuntimeError("evidenceId must be a non-empty id of at most 256 characters.", 0, []);
    }
    return this.#request<EvidenceContent>({
      method: "GET",
      path: `/v1/executions/${encodeURIComponent(id)}/evidence/${encodeURIComponent(evidenceId)}`,
    });
  }
}
