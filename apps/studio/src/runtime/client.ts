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

import envelopeSchema from "../../../../schemas/event-envelope.schema.json";

import type {
  Actor,
  Briefing,
  ClaimEvidence,
  Diagnostic,
  Envelope,
  EventPage,
  ExecutionPage,
  EvidenceContent,
  ExecutionStatus,
  GraphTopology,
  ModelRouteSummary,
  MutationEvidence,
  RecordedActor,
  RuntimeEvent,
  WireActorType,
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
/** The bound `resume` has always applied to a graph path, now named so three verbs share it. */
const MAX_GRAPH_PATH_LENGTH = 512;
/** A claim presents a handful of artefacts, not a corpus. The Runtime bounds the bundle FILE at
 * 1 MiB; this is the same intent one layer up, where the items are still items. */
const MAX_EVIDENCE_ITEMS = 32;
/** The Runtime's refusal for a well-formed id that names no execution (#1083 F1). */
const EXECUTION_NOT_FOUND = "GHCLI028_EXECUTION_NOT_FOUND";

/**
 * The largest silence budget the persisted envelope admits, READ FROM THE SCHEMA rather than
 * copied: `executionFormAmended.nodeTimeoutSeconds` is bounded there (PR #662 review), and a
 * client that accepted any safe integer let a "valid" control turn into the store's refusal one
 * hop later. If the schema moves, this moves with it; a copied number would not.
 */
const AMENDED_TIMEOUT_BOUNDS = (
  envelopeSchema as {
    $defs: { executionFormAmended: { properties: { nodeTimeoutSeconds: { additionalProperties: { minimum: number; maximum: number } } } } };
  }
).$defs.executionFormAmended.properties.nodeTimeoutSeconds.additionalProperties;
export const MAX_NODE_TIMEOUT_SECONDS: number = AMENDED_TIMEOUT_BOUNDS.maximum;
export const MIN_NODE_TIMEOUT_SECONDS: number = AMENDED_TIMEOUT_BOUNDS.minimum;

/**
 * The wire's actor vocabulary, READ FROM THE SCHEMA (`$defs.actor.properties.type.enum`). An
 * actor read back from the ledger is accepted only if its type is in this set; anything else is
 * REFUSED - never narrowed by a cast into a union it does not belong to (PR #662 review,
 * client.ts:684: the driver's `system` was being cast to `owner | agent`, so the evidence's own
 * type lied at runtime). `WireActorType` in types.ts restates the same enum as literals; the
 * two are pinned together by a cell.
 */
export const ACTOR_TYPES: readonly string[] = (
  envelopeSchema as { $defs: { actor: { properties: { type: { enum: string[] } } } } }
).$defs.actor.properties.type.enum;

function isWireActorType(value: string | null): value is WireActorType {
  return value !== null && ACTOR_TYPES.includes(value);
}

/**
 * The Runtime's own timestamp form, READ FROM THE SCHEMA (`$defs.timestamp.pattern`): UTC only,
 * `Z`-terminated, at most nine fraction digits, and the leap second `23:59:60` admitted. The
 * first version hand-rolled an RFC 3339 regex plus `Date.parse`, which accepted offsets and
 * over-long fractions the wire contract refuses and rejected the leap second it permits -
 * validation looser AND stricter than the contract at once (PR #662 review, client.ts:83).
 * No `Date.parse` anywhere: it normalizes impossible dates instead of refusing them.
 */
export const TIMESTAMP_PATTERN: string = (
  envelopeSchema as { $defs: { timestamp: { pattern: string } } }
).$defs.timestamp.pattern;
const TIMESTAMP_REGEX = new RegExp(TIMESTAMP_PATTERN);

/** Days in a month, leap years included - the calendar check the pattern cannot express and
 * `PersistedTimestamp` enforces on the Runtime side (`2026-02-30` is refused there). */
function isCalendarDay(year: number, month: number, day: number): boolean {
  if (month < 1 || month > 12 || day < 1) return false;
  const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  const lengths = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return day <= lengths[month - 1];
}

/** An instant this client will relay as `asOf`: present means it matches the persisted timestamp
 * contract exactly, or it is REFUSED - never dropped, never normalized. Dropping turned "sweep as
 * of X" into "sweep now"; normalizing would turn an impossible date into a real one. */
export function isPersistedTimestamp(value: string): boolean {
  if (!TIMESTAMP_REGEX.test(value)) return false;
  return isCalendarDay(Number(value.slice(0, 4)), Number(value.slice(5, 7)), Number(value.slice(8, 10)));
}
const MUTATION_KEY_SUFFIX: Record<MutationEvidence["action"], string> = {
  start: "started",
  pause: "paused",
  approve: "outcome",
  resume: "resumed",
  signal: "record",
  // Phase 2 verbs, each suffix copied from the serve route's own `parse_mutation_headers` call
  // (routes.rs) rather than guessed - the derived key is `{key}-{suffix}-{digest}` and a wrong
  // word here would make every verification read zero attributable events.
  cancel: "cancelled",
  sweep: "sweep-performed",
  amendBudget: "outcome",
  claim: "claim",
  clear: "clear",
};

/**
 * The kind (or kinds) the Runtime appends for each verb, as the attribution scan looks for.
 *
 * A LIST, NOT A NAME, because two verbs decide in the journal rather than in the status code.
 * `claim` appends `completion_claimed` OR `completion_refused` — the route says so in as many
 * words, and a refused claim is a 200 whose decision is the event. A map holding one name per
 * action would make every refused claim read as "no attributable event", which is the reading
 * reserved for a mutation that never landed: the caller would be told nothing happened while
 * the journal holds the refusal that did.
 */
const MUTATION_DECISION_KIND: Record<MutationEvidence["action"], readonly string[]> = {
  start: ["execution_started"],
  pause: ["execution_paused"],
  approve: ["node_outcome_recorded"],
  resume: ["execution_resumed"],
  signal: ["signal_recorded"],
  cancel: ["execution_completed"],
  sweep: ["sweep_performed"],
  amendBudget: ["execution_form_amended"],
  claim: ["completion_claimed", "completion_refused"],
  clear: ["completion_cleared"],
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
    !MUTATION_DECISION_KIND[action].includes(event.kind) ||
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
  method: "GET" | "POST" | "PUT";
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

/** The bound `resume` already applies to a graph path, named once so the customs verbs cannot
 * drift from it. A path on the RUNTIME's filesystem: relayed verbatim, never opened here. */
function checkedGraphPath(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_GRAPH_PATH_LENGTH) {
    throw new RuntimeError(
      `file must be a non-empty path of at most ${MAX_GRAPH_PATH_LENGTH} characters.`,
      0,
      [],
    );
  }
  return value;
}

/** A journal sequence, which is a non-negative INTEGER.
 *
 * Zero is legal and is not an absence: an empty stream answers `headSequence: 0`, and the same
 * reasoning that made `If-Match: 0` load-bearing applies to any sequence this client sends. A
 * guard written as `!value` would drop it. */
function checkedSequence(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new RuntimeError(`${field} must be a non-negative integer.`, 0, []);
  }
  return value;
}

/**
 * The evidence bundle, checked to the shape the Runtime's own parser accepts
 * (`{kind, contentHash, size}`), before it costs a request.
 *
 * NOT A CREDENTIAL AND NOT A FILE. Every item is a DIGEST of an artefact plus its size; the
 * artefact itself never leaves the caller. That is the whole reason this surface can hash in the
 * browser and send the result: what travels is a fingerprint of a test report, not the report.
 */
function checkedEvidence(value: unknown): ClaimEvidence[] {
  if (!Array.isArray(value) || value.length > MAX_EVIDENCE_ITEMS) {
    throw new RuntimeError(`evidence must be an array of at most ${MAX_EVIDENCE_ITEMS} items.`, 0, []);
  }
  return value.map((item, index) => {
    const entry = item as Partial<ClaimEvidence> | null;
    if (
      entry === null ||
      typeof entry !== "object" ||
      typeof entry.kind !== "string" ||
      entry.kind.length === 0 ||
      entry.kind.length > MAX_ID_LENGTH ||
      typeof entry.contentHash !== "string" ||
      !/^sha256:[0-9a-f]{64}$/.test(entry.contentHash) ||
      typeof entry.size !== "number" ||
      !Number.isSafeInteger(entry.size) ||
      entry.size < 0
    ) {
      throw new RuntimeError(
        `evidence[${index}] must be {kind, contentHash: "sha256:<64 hex>", size}.`,
        0,
        [],
      );
    }
    return { kind: entry.kind, contentHash: entry.contentHash, size: entry.size };
  });
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
  /** Set once this Runtime has refused the route listing as fixture-only. See `listRoutes`. */
  #fixtureOnly = false;

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

  /**
   * `GET /v1/executions/{id}/briefing` (#1063) - the run's name and objective, and the harness
   * digest around them. Same bearer as `getStatus`, read-only.
   *
   * `null` on a 404 and ONLY a 404: that is what an older Runtime without the route answers
   * (its fallback is `serve.not_found`), and the page then degrades to naming runs by id with
   * no banner - a missing optional read is not an error the operator can act on. Every other
   * failure still throws, so a broken store is not mistaken for an old server.
   */
  async getBriefing(executionId: string): Promise<Briefing | null> {
    const id = checkedId(executionId, "executionId");
    try {
      return await this.#request<Briefing>({
        method: "GET",
        path: `/v1/executions/${encodeURIComponent(id)}/briefing`,
      });
    } catch (reason) {
      // A 404 CARRYING `GHCLI028_EXECUTION_NOT_FOUND` is a Runtime that HAS the route telling us
      // this id names no execution (#1083 F1) - rethrown, so the page never concludes the whole
      // server lacks briefings from one unknown id.
      if (
        reason instanceof RuntimeError &&
        reason.httpStatus === 404 &&
        !reason.diagnostics.some((diagnostic) => diagnostic.code === EXECUTION_NOT_FOUND)
      ) {
        return null;
      }
      throw reason;
    }
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
      try {
        const before = await this.getStatus(id);
        headBefore = before.headSequence;
      } catch (error) {
        // A START is the one verb whose target is SUPPOSED not to exist yet. Since #1083 F1 the
        // Runtime answers a well-formed unknown id with 404 `GHCLI028_EXECUTION_NOT_FOUND` where
        // it used to answer 200 with `headSequence: 0`; both say "an empty stream", so the start
        // proceeds guarded at 0 exactly as before. Every other verb keeps the refusal: acting on a
        // run that does not exist is the caller's mistake, and the read already said so.
        if (
          action === "start" &&
          error instanceof RuntimeError &&
          error.httpStatus === 404 &&
          error.diagnostics.some((diagnostic) => diagnostic.code === EXECUTION_NOT_FOUND)
        ) {
          headBefore = 0;
        } else {
          throw error;
        }
      }
    }

    const headers: Record<string, string> = {
      "Idempotency-Key": idempotencyKey,
      "X-GraphHelm-Actor": actor.id,
      "X-GraphHelm-Actor-Type": actor.type,
    };
    // ZERO IS AN OBSERVATION, NOT AN ABSENCE (PR #662 review, adapter.ts:157). The predecessor
    // sent the header only when the head was above zero, on the belief that `If-Match: 0` claims
    // a sequence streams never issue. The Runtime says otherwise: `current_head` is
    // `history.last().map_or(0, ..)`, so a stream that resolves with no events answers exactly
    // `Some(0)`, and it answers `None` only when the store cannot be read. A caller that observed
    // an empty run and asks to be refused if anything appeared was therefore having its
    // precondition dropped -- and a WebMCP caller could pass `ifMatch: 0` through a schema whose
    // `minimum` is 0, be told the verb is guarded, and have it run unguarded against a stream
    // created between the read and the write. A guard whose absence is invisible is worse than no
    // guard, because its presence gets cited.
    //
    // `-1` above is the sentinel for "the caller named no head", and it is unreachable from
    // outside: `ifMatchOf` refuses anything negative before it becomes an option. So a
    // non-negative value here is always a head someone actually observed, and every one of them
    // is sent.
    if (headBefore >= 0) headers["If-Match"] = String(headBefore);

    let diagnostics: Diagnostic[] = [];
    let refused = false;
    let postAmbiguous = false;
    let confirmedHead: number | null = null;
    let recognizedRetry = false;
    // Where the Runtime says this identity's decision already landed. Null means it said nothing
    // usable, which is refused as proof rather than rounded down to "no retry" -- the same rule
    // the immediate-pause path applies to the same field.
    let recognizedRetryAt: number | null = null;
    try {
      const reply = await this.#request<{
        headSequence?: unknown;
        idempotency?: { recognizedRetry?: unknown; originalDecisionSequence?: unknown };
      }>({ method: "POST", path, body, headers });
      recognizedRetry = reply.idempotency?.recognizedRetry === true;
      if (recognizedRetry) {
        const sequence = reply.idempotency?.originalDecisionSequence;
        if (typeof sequence === "number" && Number.isSafeInteger(sequence) && sequence > 0) {
          recognizedRetryAt = sequence;
        }
      }
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
      // committed head (a retry after the original landed), the confirmed head equals it and no
      // event scan runs, so the old shape reported `succeeded` on zero evidence (PR #467 review).
      if (confirmedHead !== null && confirmedHead === headBefore && !recognizedRetry) {
        verified = false;
      }
      // A RECOGNIZED RETRY IS A COORDINATE, NOT A VERDICT (PR #662 review, adapter.ts:178).
      // `recognizedRetry` alone was accepted as the missing evidence, which made these seven
      // verbs answer `succeeded` with an EMPTY `newEvents` while the response advertised
      // verification against the original decision. The immediate-pause path was already fixed to
      // read the decision back at `originalDecisionSequence`; this is the same rule for the rest,
      // through the same attribution predicate the forward scan uses, so a retry cannot claim an
      // event that belongs to another actor or another key.
      //
      // A retry whose coordinate is missing, unusable, or points at something this caller did not
      // author degrades to `unknown`. That is not the same as failure: the mutation may well have
      // committed. It is the honest report that this client could not read the proof, which is
      // what `unknown` is for.
      if (recognizedRetry && verified) {
        const proof = recognizedRetryAt === null ? null : await this.#eventAt(id, recognizedRetryAt);
        if (
          proof !== null &&
          isAttributableDecision(proof, action, node, actor, attributableKeyPrefix)
        ) {
          if (!newEvents.some((event) => event.sequence === proof.sequence)) {
            newEvents.push(proof);
          }
        } else {
          verified = false;
          if (diagnostics.length === 0) {
            diagnostics = [{
              code: "",
              severity: "error",
              message: recognizedRetryAt === null
                ? "The Runtime recognized this retry but returned no usable originalDecisionSequence; the proof is not accepted."
                : "The Runtime recognized this retry, but no decision attributable to this actor and key was readable at the sequence it named.",
              path: "/idempotency/originalDecisionSequence",
              source: "studio",
            }];
          }
        }
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

  /**
   * `POST /v1/executions/{id}/pause` with `{"mode": "immediate"}` - the runtime's OTHER pause.
   *
   * The graceful `pause` above lets in-flight work finish and exits by quiescence; this one
   * interrupts it. The two are different promises to the operator and are two methods here so no
   * call site can reach the interrupting one by a default.
   *
   * VERIFIED BY STATE, NOT BY EVENT - deliberately different from every other mutation: the
   * immediate handler appends nothing itself (the DRIVER appends `execution_paused` under its own
   * key once every in-flight node is recorded Interrupted, routes.rs), so there is no attributable
   * decision event for this caller to find, and the handler's reply is the polled STATUS. The
   * honest proof is therefore the state: `status === "paused"` on a fresh read is `succeeded`;
   * anything else the poll window could not confirm is `unknown`, never optimism.
   */
  async pauseImmediately(
    executionId: string,
    options: MutationOptions = {},
  ): Promise<MutationEvidence> {
    const id = checkedId(executionId, "executionId");
    const actor = options.actor ?? OPERATOR_ACTOR;
    const idempotencyKey = options.idempotencyKey ?? newIdempotencyKey();
    // THE IDENTITY OF A MUTATION IS (KEY, ACTOR), NEVER THE SHAPE OF AN EVENT (PR #662 review,
    // client.ts:903). The Runtime records both on the pause it appends for this request - the
    // derived key `{key}-paused-{digest}` and the actor the headers named - so "my pause" is the
    // event carrying this prefix AND this actor, the same test #verifiedMutation applies to every
    // other verb. A pause that merely IS a pause (another actor's, landed in the same interval,
    // or the payload that won a race this request lost) is not this request's and is never
    // reported as its evidence.
    const keyPrefix = `${idempotencyKey}-${MUTATION_KEY_SUFFIX.pause}-`;
    const isMine = (event: RuntimeEvent): boolean =>
      event.kind === "execution_paused" &&
      event.idempotencyKey?.startsWith(keyPrefix) === true &&
      event.actorId === actor.id &&
      event.actorType === actor.type;
    const before = await this.getStatus(id);
    let diagnostics: Diagnostic[] = [];
    let refused = false;
    // SUCCESS NEEDS TWO FACTS, not one (PR #662 review): the POST was CONFIRMED (a 2xx read
    // back, not a transport error or a 5xx that a later read happens to follow), and the run was
    // NOT ALREADY PAUSED before this call - otherwise `paused` afterwards proves nothing about
    // this mutation. Either fact missing degrades to `unknown` with the reason as a diagnostic.
    let posted = false;
    const alreadyPaused = before.status === "paused";
    // THE CALLER'S OBSERVED HEAD RIDES AS If-Match, like every other Studio mutation: an
    // interrupt aimed at the run the operator SAW must not interrupt work that began after they
    // looked. The head is the CALLER'S when they passed one (`options.ifMatch`) - the pre-read
    // above is for verification (was the run already paused?), never a substitute for the
    // precondition: replacing the head the caller observed with a fresher one the client just
    // read turns the guard into one that always passes, which is worse than none because its
    // presence gets cited (PR #662 review, client.ts:664; the #verifiedMutation rule). Only
    // Since #681 the immediate branch runs through the same `run_idempotent_mutation` wrapper as
    // the graceful one (routes.rs `pause` doc comment), so the precondition is honoured on both
    // paths - the bypass this comment once recorded is gone. Zero is sent like any other observed
    // head, for the reason written at the `#verifiedMutation` site: the Runtime reports an empty
    // stream as head 0 and an unreadable one as no head at all, so pinning 0 fails CLOSED on a
    // store it cannot read and refuses correctly on a stream something has since written.
    const headBefore = options.ifMatch ?? before.headSequence;
    const headers: Record<string, string> = {
      "Idempotency-Key": idempotencyKey,
      "X-GraphHelm-Actor": actor.id,
      "X-GraphHelm-Actor-Type": actor.type,
    };
    if (headBefore >= 0) headers["If-Match"] = String(headBefore);
    // A RECOGNIZED RETRY IS THE RUNTIME'S OWN PROOF (PR #662 review, client.ts:667): when the
    // first POST committed and its reply was lost, the retry under the same key arrives after
    // the run already reads paused - `alreadyPaused` is true and a scan after the pre-read head
    // finds nothing new, so the old shape could only answer `unknown`. The wrapper replies
    // `idempotency.recognizedRetry` with `originalDecisionSequence`; that sequence is where the
    // pause event is read back instead (the regular path's rule, #verifiedMutation), and success
    // no longer needs the run to have been running before THIS call.
    let recognizedRetryAt: number | null = null;
    try {
      const reply = await this.#request<{
        idempotency?: { recognizedRetry?: unknown; originalDecisionSequence?: unknown };
      }>({
        method: "POST",
        path: `/v1/executions/${encodeURIComponent(id)}/pause`,
        body: { mode: "immediate" },
        headers,
      });
      posted = true;
      if (reply.idempotency?.recognizedRetry === true) {
        const sequence = reply.idempotency.originalDecisionSequence;
        if (typeof sequence === "number" && Number.isSafeInteger(sequence) && sequence > 0) {
          recognizedRetryAt = sequence;
        } else {
          // A proof without a usable coordinate is refused as proof, not rounded to "no retry".
          diagnostics = [{
            code: "",
            severity: "error",
            message: "The Runtime recognized this retry but returned no usable originalDecisionSequence; the proof is not accepted.",
            path: "/idempotency/originalDecisionSequence",
            source: "studio",
          }];
        }
      }
    } catch (error) {
      if (error instanceof DisconnectedError) throw error;
      if (!(error instanceof RuntimeError)) throw error;
      refused = error.httpStatus >= 400 && error.httpStatus < 500;
      diagnostics = error.diagnostics.length > 0
        ? error.diagnostics
        : [{ code: "", severity: "error", message: error.message, path: "/", source: "studio" }];
    }
    let statusAfter: ExecutionStatus | null = null;
    try {
      statusAfter = await this.getStatus(id);
    } catch (error) {
      if (error instanceof DisconnectedError) throw error;
    }
    const pausedNow = statusAfter?.status === "paused";
    // THE ACTOR IS READ BACK FROM THE LEDGER, NOT REPEATED FROM THE REQUEST (PR #662 review,
    // adapter.ts:389, then client.ts:760). The immediate branch signals the cancel channel and
    // the driver appends `execution_paused` later, asynchronously; since #681 the request's
    // actor and idempotency key ride that channel (`ImmediateCancelRequest`, routes.rs), so the
    // record names the caller. The evidence does not take that on faith: the pause event is
    // READ BACK and its actor is what the evidence carries - the caller when the Runtime did
    // its part, and whoever the record names if it ever stops. Success requires having found it.
    // The scan runs until the head the re-read OBSERVED is exhausted - the bound is the model's
    // (the head itself), not a page literal: a legal 1,024-node ready set can record 1,024
    // Interrupted outcomes before the pause folds (bounds.rs MAX_READY_SET; driver.rs
    // write_outcome loop), and a five-page cap turned a confirmed pause into `unknown`.
    const observedHead = statusAfter?.headSequence ?? null;
    let foundEvent: RuntimeEvent | null = null;
    // A pause that was read but is NOT this request's - reported as such, never as evidence.
    let foreignPause: RuntimeEvent | null = null;
    if (recognizedRetryAt !== null) {
      const at = await this.#pausedEventAt(id, recognizedRetryAt);
      if (at === null) {
        diagnostics = [
          ...diagnostics,
          {
            code: "",
            severity: "error",
            message: `The Runtime named sequence ${recognizedRetryAt} as the original pause decision, but no execution_paused event was read there; the retry is not accepted as proof.`,
            path: "/idempotency/originalDecisionSequence",
            source: "studio",
          },
        ];
      } else if (isMine(at)) {
        foundEvent = at;
      } else {
        foreignPause = at;
      }
    } else if (posted && !refused) {
      // The scan starts at the head the precondition named - the caller's when given - the
      // same coordinate #verifiedMutation scans from; the pre-read is not consulted here either.
      const scan = await this.#pausedEventAfter(id, headBefore, observedHead, isMine);
      foundEvent = scan.mine;
      foreignPause = scan.mine === null ? scan.foreign : null;
    }
    if (foundEvent === null && foreignPause !== null) {
      diagnostics = [
        ...diagnostics,
        {
          code: "",
          severity: "error",
          message: `a pause was found in the log but it is not this request's: sequence ${foreignPause.sequence} carries actor ${String(foreignPause.actorType)}:${String(foreignPause.actorId)} under key ${String(foreignPause.idempotencyKey)}; this request is ${actor.type}:${actor.id} under key prefix ${keyPrefix}`,
          path: "/idempotency",
          source: "studio",
        },
      ];
    }
    // THE LEDGER'S ACTOR, IN THE WIRE'S VOCABULARY. An event whose actor type is outside the
    // schema's enum is not accepted as proof - refused, never narrowed by a cast into a union it
    // does not belong to. Only a found event with a legal actor can carry `succeeded`.
    const ledgerActor: RecordedActor | null =
      foundEvent !== null && foundEvent.actorId !== null && isWireActorType(foundEvent.actorType)
        ? { id: foundEvent.actorId, type: foundEvent.actorType }
        : null;
    const pausedEvent = ledgerActor !== null ? foundEvent : null;
    if (foundEvent !== null && pausedEvent === null) {
      diagnostics = [
        ...diagnostics,
        {
          code: "",
          severity: "error",
          message: `The ledger's pause event carries an actor type outside the wire vocabulary (${String(foundEvent.actorType)}); it is not accepted as proof.`,
          path: "/actor",
          source: "studio",
        },
      ];
    }
    // A RECOGNIZED RETRY SUCCEEDS ON THE RECORD, NOT ON THE AGGREGATE (PR #662 review,
    // client.ts:774): the pause the Runtime recognized was committed at N whatever the run did
    // afterwards - another actor may have resumed it since, and `running` now says nothing
    // against a pause proven at N. `pausedNow` decides only the path with no retry proof. When
    // the run has moved on, the evidence says so: the resume that followed, by whom, where.
    const provenRetry = recognizedRetryAt !== null && pausedEvent !== null ? recognizedRetryAt : null;
    if (provenRetry !== null && statusAfter !== null && statusAfter.status !== "paused") {
      const resumed = await this.#firstEventAfter(id, "execution_resumed", provenRetry, observedHead);
      diagnostics = [
        ...diagnostics,
        {
          code: "",
          severity: "warning",
          message:
            resumed !== null
              ? `your pause was committed at ${provenRetry}; the run was resumed afterwards by ${String(resumed.actorType)}:${String(resumed.actorId)} at ${resumed.sequence} (status now ${String(statusAfter.status)}, head ${String(observedHead)})`
              : `your pause was committed at ${provenRetry}; the run has since moved on (status now ${String(statusAfter.status)}, head ${String(observedHead)}) - no resume event was read in between`,
          path: "/execution",
          source: "studio",
        },
      ];
    }
    let result: MutationEvidence["result"];
    if (refused) {
      result = "refused";
    } else if (posted && pausedEvent !== null && (provenRetry !== null || (!alreadyPaused && pausedNow))) {
      result = "succeeded";
    } else {
      result = "unknown";
      if (alreadyPaused && recognizedRetryAt === null) {
        diagnostics = [
          ...diagnostics,
          {
            code: "",
            severity: "error",
            message: "The run was already paused before this call; the paused state proves nothing about it.",
            path: "/execution",
            source: "studio",
          },
        ];
      } else if (!posted) {
        diagnostics = [
          ...diagnostics,
          {
            code: "",
            severity: "error",
            message: "The immediate pause was not confirmed by the Runtime; the later paused state may belong to someone else.",
            path: "/execution",
            source: "studio",
          },
        ];
      }
    }
    if (ledgerActor !== null) {
      diagnostics = [
        ...diagnostics,
        {
          code: "",
          severity: "warning",
          message:
            "immediate pause is signalled on the cancel channel and the runtime appends the pause event asynchronously - the actor reported here is the one read back from the append-only record, not repeated from the request.",
          path: "/actor",
          source: "studio",
        },
      ];
    }
    return {
      action: "pause",
      executionId: id,
      node: null,
      // Refused means nothing was appended: the identity that was SENT is the only truthful
      // one. Otherwise the ledger's actor, and never the caller's, on this path.
      actor: refused ? actor : (ledgerActor ?? actor),
      idempotencyKey,
      // A proven retry reports the ORIGINAL pause's heads: the record held N-1 just before the
      // pause event and N once it landed. (N-1 is the head before the pause, not the original
      // request's precondition - the record does not carry that.) Otherwise this call's own.
      headBefore: provenRetry !== null ? provenRetry - 1 : headBefore,
      headAfter: provenRetry !== null ? provenRetry : (statusAfter?.headSequence ?? null),
      result,
      statusAfter,
      newEvents: pausedEvent === null ? [] : [pausedEvent],
      diagnostics,
    };
  }

  /** The first event of `kind` strictly after `after`, scanning until `until` is exhausted (or
   * a short page ends the log when no head is known). `null` when none, or nothing readable. */
  async #firstEventAfter(executionId: string, kind: string, after: number, until: number | null): Promise<RuntimeEvent | null> {
    try {
      let cursor = after;
      while (until === null || cursor < until) {
        const read = await this.getEvents(executionId, { after: cursor, limit: MAX_EVENT_LIMIT });
        const found = read.events.find((event) => event.kind === kind);
        if (found !== undefined) return found;
        if (read.events.length === 0) return null;
        const last = read.events[read.events.length - 1].sequence;
        if (last <= cursor) return null;
        cursor = last;
        if (read.events.length < MAX_EVENT_LIMIT && until === null) return null;
      }
      return null;
    } catch (error) {
      if (error instanceof DisconnectedError) throw error;
      return null;
    }
  }

  /** The `execution_paused` at exactly `sequence` - the coordinate a recognized retry names as
   * the original decision. Anything else at that sequence (or nothing readable) is `null`: the
   * Runtime's proof is checked against the record, not repeated. This reads the SHAPE at the
   * coordinate; whether the pause is this request's (key prefix + actor) is the caller's test. */
  /** The event AT `sequence`, or null when the log does not have one there or cannot be read. */
  async #eventAt(executionId: string, sequence: number): Promise<RuntimeEvent | null> {
    try {
      const page = await this.getEvents(executionId, { after: sequence - 1, limit: 1 });
      const event = page.events[0];
      return event !== undefined && event.sequence === sequence ? event : null;
    } catch (error) {
      if (error instanceof DisconnectedError) throw error;
      return null;
    }
  }

  async #pausedEventAt(executionId: string, sequence: number): Promise<RuntimeEvent | null> {
    const event = await this.#eventAt(executionId, sequence);
    return event !== null && event.kind === "execution_paused" ? event : null;
  }

  /** THIS request's `execution_paused` after `after` - the one `isMine` accepts (key prefix +
   * actor, PR #662 review, client.ts:903) - scanning until `until` (the head the re-read
   * observed) is exhausted, or until a short page says the log ends when no head is known. No
   * page literal: the observed head IS the bound, so a pause folded behind any number of
   * Interrupted records is found. `foreign` is the last pause read that is NOT this request's,
   * reported only when no own pause was found, so the caller can say so instead of "nothing". */
  async #pausedEventAfter(
    executionId: string,
    after: number,
    until: number | null,
    isMine: (event: RuntimeEvent) => boolean,
  ): Promise<{ mine: RuntimeEvent | null; foreign: RuntimeEvent | null }> {
    let foreign: RuntimeEvent | null = null;
    try {
      let cursor = after;
      while (until === null || cursor < until) {
        const read = await this.getEvents(executionId, { after: cursor, limit: MAX_EVENT_LIMIT });
        for (const event of read.events) {
          if (event.kind !== "execution_paused") continue;
          if (isMine(event)) return { mine: event, foreign: null };
          foreign = event;
        }
        if (read.events.length === 0) break;
        const last = read.events[read.events.length - 1].sequence;
        if (last <= cursor) break;
        cursor = last;
        if (read.events.length < MAX_EVENT_LIMIT && until === null) break;
      }
      return { mine: null, foreign };
    } catch (error) {
      if (error instanceof DisconnectedError) throw error;
      return { mine: null, foreign };
    }
  }

  /**
   * `POST /v1/executions/{id}/cancel` - the destructive verb: every non-terminal node is recorded
   * Cancelled and the execution completes as `cancelled`. No request body, mirroring the CLI.
   * The CONFIRMATION lives on the surfaces (the page asks; a WebMCP host confirms tool calls) -
   * this client only carries the verb.
   */
  async cancel(executionId: string, options: MutationOptions = {}): Promise<MutationEvidence> {
    return this.#verifiedMutation(
      "cancel",
      executionId,
      `/v1/executions/${encodeURIComponent(checkedId(executionId, "executionId"))}/cancel`,
      {},
      null,
      options,
    );
  }

  /** `POST /v1/executions/{id}/sweep` - evaluates the stream's customs stages and journals the
   * result. `asOf` absent means now, read from the STORE's clock; the future is refused by the
   * verb itself, so this surface deliberately re-checks nothing. */
  async sweep(
    executionId: string,
    options: MutationOptions & { asOf?: string } = {},
  ): Promise<MutationEvidence> {
    const body: Record<string, string> = {};
    // PRESENT MEANS VALID OR REFUSED - never silently absent. An empty or unparseable `asOf`
    // used to be dropped, which turned "sweep as of X" into "sweep now": a different operation,
    // one that journals and can spend overdue-exception episodes (PR #662 review).
    if (options.asOf !== undefined) {
      if (typeof options.asOf !== "string" || !isPersistedTimestamp(options.asOf)) {
        throw new RuntimeError(
          "asOf must be a UTC instant in the Runtime's own form, Z-terminated (for example 2026-09-02T10:00:00Z); offsets, impossible dates and fractions past nine digits are refused.",
          0,
          [],
        );
      }
      body.asOf = options.asOf;
    }
    return this.#verifiedMutation(
      "sweep",
      executionId,
      `/v1/executions/${encodeURIComponent(checkedId(executionId, "executionId"))}/sweep`,
      body,
      null,
      options,
    );
  }

  /**
   * `POST /v1/executions/{id}/amend-budget` - the attention verdict's own remedy socket: declares
   * a silence bound for one node. `seconds` is the operator's decision and has NO default here
   * for the same reason the API gives it none - "absent means unknown" must not become "absent
   * means 300s" through a client. `computedAtSequence` is where the remedy was computed, copied
   * from the verdict itself, never invented.
   */
  async amendBudget(
    executionId: string,
    remedy: { node: string; seconds: number; computedAtSequence: number },
    options: MutationOptions = {},
  ): Promise<MutationEvidence> {
    const nodeId = checkedId(remedy.node, "node");
    // The schema's own bound, not any safe integer: the store refuses an envelope above it, and
    // a control that accepted the value here only moved that refusal one hop later, where it
    // reads as a failed (or unknown) mutation instead of a bad input (PR #662 review).
    if (
      !Number.isSafeInteger(remedy.seconds) ||
      remedy.seconds < MIN_NODE_TIMEOUT_SECONDS ||
      remedy.seconds > MAX_NODE_TIMEOUT_SECONDS
    ) {
      throw new RuntimeError(
        `seconds must be an integer between ${MIN_NODE_TIMEOUT_SECONDS} and ${MAX_NODE_TIMEOUT_SECONDS} (the envelope schema's bound).`,
        0,
        [],
      );
    }
    if (!Number.isSafeInteger(remedy.computedAtSequence) || remedy.computedAtSequence < 0) {
      throw new RuntimeError("computedAtSequence must be a non-negative integer.", 0, []);
    }
    return this.#verifiedMutation(
      "amendBudget",
      executionId,
      `/v1/executions/${encodeURIComponent(checkedId(executionId, "executionId"))}/amend-budget`,
      { node: nodeId, seconds: remedy.seconds, computedAtSequence: remedy.computedAtSequence },
      nodeId,
      options,
    );
  }

  /**
   * `POST /v1/executions/{id}/claim` — testimony that a parked node's external work is done.
   *
   * THE GRAPH TRAVELS WITH THE REQUEST, exactly as `resume` does and for the same reason: the
   * node's declared `proofKinds` are read from it by the Runtime, never supplied by this caller.
   * `file` is a path on the RUNTIME's filesystem, bounded and relayed verbatim, never read here.
   *
   * `waitSeq` NAMES THE RENDEZVOUS and is not an optimisation. The fold refuses a claim that
   * names a superseded wait (`stale_rendezvous`) instead of silently answering whichever wait is
   * open now; omitting it asks the Runtime to pick, which is precisely the ambiguity the field
   * exists to remove. A caller with a status in hand always has the sequence.
   *
   * A REFUSAL IS A 200. The verb journals `completion_refused` with a registry code, so the
   * mutation succeeds while the claim does not: read the verdict with `claimVerdict`, never from
   * `result`.
   */
  async claimNode(
    executionId: string,
    request: { file: string; node: string; waitSeq?: number; evidence?: ClaimEvidence[] },
    options: MutationOptions = {},
  ): Promise<MutationEvidence> {
    const nodeId = checkedId(request.node, "node");
    const body: Record<string, unknown> = { file: checkedGraphPath(request.file), node: nodeId };
    if (request.waitSeq !== undefined) {
      body.waitSeq = checkedSequence(request.waitSeq, "waitSeq");
    }
    if (request.evidence !== undefined) {
      body.evidence = checkedEvidence(request.evidence);
    }
    return this.#verifiedMutation(
      "claim",
      executionId,
      `/v1/executions/${encodeURIComponent(checkedId(executionId, "executionId"))}/claim`,
      body,
      nodeId,
      options,
    );
  }

  /**
   * `POST /v1/executions/{id}/clear` — countersign an open claim by machine replay.
   *
   * The EVIDENCE is sent, not a hash: the Runtime re-derives the digest and compares it against
   * what the claim journaled, so a bundle that does not match the testimony is refused
   * (`hash_mismatch`) rather than accepted. Sending a hash this caller computed would move that
   * comparison to the side that wants it to pass.
   *
   * `node` is carried for ATTRIBUTION only — the request is keyed by `claimSeq`, which is what
   * identifies the claim being countersigned.
   */
  async clearClaim(
    executionId: string,
    request: { file: string; claimSeq: number; evidence: ClaimEvidence[]; node?: string },
    options: MutationOptions = {},
  ): Promise<MutationEvidence> {
    const body: Record<string, unknown> = {
      file: checkedGraphPath(request.file),
      claimSeq: checkedSequence(request.claimSeq, "claimSeq"),
      evidence: checkedEvidence(request.evidence),
    };
    return this.#verifiedMutation(
      "clear",
      executionId,
      `/v1/executions/${encodeURIComponent(checkedId(executionId, "executionId"))}/clear`,
      body,
      request.node === undefined ? null : checkedId(request.node, "node"),
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
    // #1083 F6: a fixture-only Runtime's answer cannot change while this connection lives (the
    // manifest is a `serve` start flag), so its documented refusal is read ONCE per client. The
    // page asked at connect and again at every new task, and each ask put the same 400 in the
    // browser's network log. A configured Runtime is re-asked: its manifest can be edited.
    if (this.#fixtureOnly) return { configured: false, routes: [] };
    try {
      const reply = await this.#request<{ routes?: ModelRouteSummary[]; configured?: boolean }>({
        method: "GET",
        path: "/v1/gateway/routes",
      });
      // #1083 F6: a current Runtime ANSWERS a fixture-only listing with 200 `configured: false`
      // (no 400 in the browser's console at all). A reply without the field is a configured
      // manifest's listing, exactly as before.
      const configured = reply.configured !== false;
      if (!configured) this.#fixtureOnly = true;
      return { configured, routes: reply.routes ?? [] };
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
        this.#fixtureOnly = true;
        return { configured: false, routes: [] };
      }
      throw error;
    }
  }


  /**
   * Writes one `direct_api` route into the Runtime's own manifest (`PUT /v1/gateway/routes`).
   *
   * THE MANIFEST IS NOT A PARAMETER, and that is the Runtime's rule rather than this client's
   * restraint: a mutation cannot name the file it writes, because a path in a request would let
   * any authenticated caller create a manifest anywhere the server process can write. The server
   * writes the one `serve --manifest` named at startup, or refuses.
   *
   * Fields the caller did not set are not sent. The Runtime owns every default - `enabled` when
   * absent, the `secret_<id>` credential reference, the profile a route advertises - and a client
   * that filled them in would be a second place they are decided.
   */
  async setRoute(draft: {
    id: string;
    provider: string;
    baseUrl: string;
    model: string;
    enabled?: boolean;
    replace?: boolean;
    credentialRef?: string;
    profiles?: string[];
  }): Promise<{ routes: ModelRouteSummary[] }> {
    const body: Record<string, unknown> = {
      id: draft.id,
      provider: draft.provider,
      baseUrl: draft.baseUrl,
      model: draft.model,
    };
    if (typeof draft.enabled === "boolean") body.enabled = draft.enabled;
    if (typeof draft.replace === "boolean") body.replace = draft.replace;
    if (typeof draft.credentialRef === "string" && draft.credentialRef !== "") {
      body.credentialRef = draft.credentialRef;
    }
    if (Array.isArray(draft.profiles)) body.profiles = draft.profiles;
    const reply = await this.#request<{ routes?: ModelRouteSummary[] }>({
      method: "PUT",
      path: "/v1/gateway/routes",
      body,
    });
    // A write invalidates the fixture-only shortcut: this Runtime demonstrably has a manifest.
    this.#fixtureOnly = false;
    return { routes: reply.routes ?? [] };
  }

  /**
   * Stores one route's API key in the Runtime's broker
   * (`PUT /v1/gateway/credentials/{reference}`).
   *
   * THE VALUE GOES IN THE BODY AND NOWHERE ELSE. The Runtime's read audit records request paths
   * and response bodies and never a request body, so a key in the path or the query would land in
   * a plaintext file on the operator's machine. Nothing reads a stored value back - there is no
   * such request - so this method returns only what the Runtime says about the reference.
   */
  async setCredential(draft: {
    reference: string;
    provider: string;
    usableBy: string[];
    value: string;
  }): Promise<{ id: string; routes: string[] }> {
    const reference = checkedId(draft.reference, "reference");
    const reply = await this.#request<{ id?: string; routes?: string[] }>({
      method: "PUT",
      path: `/v1/gateway/credentials/${encodeURIComponent(reference)}`,
      body: {
        value: draft.value,
        provider: draft.provider,
        usableBy: draft.usableBy,
      },
    });
    return { id: reply.id ?? reference, routes: reply.routes ?? [] };
  }

  /**
   * Asks the Runtime whether one route's credential leases (`GET /v1/gateway/probe`).
   *
   * It places NO model call, so an `available` here says the key works and says nothing about the
   * provider. Every caller that renders this has to carry that distinction with it.
   */
  async probeRoute(routeId: string): Promise<{ health: string }> {
    const id = checkedId(routeId, "route");
    const reply = await this.#request<{ health?: string }>({
      method: "GET",
      path: `/v1/gateway/probe?route=${encodeURIComponent(id)}`,
    });
    return { health: reply.health ?? "unknown" };
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

  async readDocument(executionId: string, document: { evidenceId: string; index: number }): Promise<{content: string; contentSha256: string}> {
    const id = checkedId(executionId, "executionId");
    return this.#request({method: "POST", path: `/v1/executions/${encodeURIComponent(id)}/documents/read`, body: {evidenceId: document.evidenceId, index: document.index}});
  }

  async saveDocument(executionId: string, document: { evidenceId: string; index: number }, edit: {content: string; expectedSha256: string; reason: string; idempotencyKey: string}): Promise<{contentSha256: string; notification: {status: "recorded" | "pending"; notifiedRuns: string[]; pendingRuns: string[]}}> {
    const id = checkedId(executionId, "executionId");
    return this.#request({method: "POST", path: `/v1/executions/${encodeURIComponent(id)}/documents/save`,
      body: {document: {evidenceId: document.evidenceId, index: document.index}, ...edit},
      headers: {"Idempotency-Key": edit.idempotencyKey, "X-GraphHelm-Actor": OPERATOR_ACTOR.id, "X-GraphHelm-Actor-Type": "owner"}});
  }
}
