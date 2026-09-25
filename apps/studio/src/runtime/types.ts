/**
 * The Public Runtime API's wire shapes, as this client reads them.
 *
 * These are the fields the Studio depends on, not a mirror of everything the Runtime replies
 * with. Every interface therefore carries an index signature: the Runtime is allowed to add
 * fields, and a client that broke on an unknown key would turn forward compatibility into an
 * outage. Nothing here is generated from the Rust types on purpose - Studio may use only public
 * contracts, and a generated binding would be a second, silent coupling to internals.
 */

/** The four-key envelope every Runtime reply carries. */
export interface Envelope<T> {
  ok: boolean;
  command: string;
  data: T | null;
  diagnostics: Diagnostic[];
}

export interface Diagnostic {
  code: string;
  severity: string;
  message: string;
  /** A JSON Pointer into the request, or an equivalent stable path. */
  path: string;
  source: string;
}

/** `needs_you` / `can_sleep` / `unknown`, kept open because the verdict vocabulary is the
 * Runtime's to extend and an unknown tag must render as unknown, never crash the page. */
export type Attention = "needs_you" | "can_sleep" | "unknown" | (string & {});

export interface AttentionReason {
  kind?: string;
  node?: string;
  [key: string]: unknown;
}

/** One row of `GET /v1/executions`. */
export interface ExecutionSummary {
  executionId: string;
  mode: string | null;
  status: string;
  attention: Attention;
  startedAt: string | null;
  lastEventAt: string | null;
  headSequence: number;
  /** The executor declared at start, carried on the row (#1064) so the rail can mark a
   * demonstration without opening the run. Absent on streams recorded before the field. */
  executor?: "fixture" | "gateway" | null;
  /** The objective the run declared at start (#1083 F7), so the rail names any run by it from
   * the index alone. `null` when none was declared; absent from an older Runtime. */
  objective?: string | null;
  [key: string]: unknown;
}

export interface ExecutionPage {
  executions: ExecutionSummary[];
  hasMore: boolean;
  nextCursor: string | null;
}

/** `GET /v1/executions/{id}`.
 *
 * `status` is NULLABLE ON THE WIRE, and this type used to lie about it. The Runtime answers an
 * execution id it has never seen with an EMPTY PROJECTION - HTTP 200, `status: null`, head 0
 * (measured 2026-08-30) - not with a 404. The human interface could never reach that shape (the
 * rail only lists executions that exist), so the lie held until the first caller selected an id
 * before starting it, and then one `readable(null)` took the whole page down. The type telling
 * the truth is what makes the compiler sweep every render site, instead of each one being found
 * by a crash. */
export interface ExecutionStatus {
  executionId: string | null;
  mode: string | null;
  status: string | null;
  attention: Attention;
  attentionReasons: AttentionReason[];
  nodeStateCounts: Record<string, number>;
  untriagedInterruptions: unknown[];
  silenceUnevaluated: unknown[];
  startedAt: string | null;
  lastEventAt: string | null;
  nodeLastEventAt: Record<string, string>;
  headSequence: number;
  /** Who produced this run's outcomes, as declared at start (#1064). `"fixture"` means every
   * outcome came from a fixture file and no model or tool was consulted - a demonstration, and
   * the panel says so in words. `"gateway"` means a real executor was wired. Absent or `null`
   * on a stream recorded before the field existed: the Studio then claims nothing either way. */
  executor?: "fixture" | "gateway" | null;
  [key: string]: unknown;
}

/** Advisory drafts for one exact execution head. Reading them never sends a signal. */
export interface ReplySuggestion {
  to: string | null;
  draft: string;
  reason: string;
  sourceSequences: number[];
}

export interface ReplySuggestions {
  executionId: string;
  headSequence: number;
  state: "ready" | "not_needed" | "unavailable";
  reason?: string;
  suggestions: ReplySuggestion[];
}

/**
 * One event, after normalisation.
 *
 * ON THE WIRE the discriminant and its payload are nested together as
 * `kind: { type, data }` - a serde externally-tagged enum. Flattening them into `kind` and
 * `payload` happens once, in the client, so no component has to know that shape. The raw
 * envelope is NOT preserved here: everything the timeline renders is a named field, and keeping
 * a second copy of the payload around would invite a component to reach past the normalisation.
 */
export interface RuntimeEvent {
  sequence: number;
  kind: string;
  payload: unknown;
  occurredAt: string | null;
  actorId: string | null;
  actorType: string | null;
  idempotencyKey: string | null;
  eventId: string | null;
  /** The ids of whatever this event sealed, in the order the Runtime recorded them.
   *
   * This is the whole reason an event can be terse and still complete. D-036 keeps free-form
   * content OUT of event payloads, so a model's reply, a tool's output and the words of a signal
   * are not in `payload` and never will be - the event carries a REFERENCE and the content is
   * sealed. An interface that reads only `payload` can therefore say that something was said and
   * can never say what. These ids are how the words are fetched back (`readEvidence`). */
  evidenceRefs: string[];
}

export interface EventPage {
  events: RuntimeEvent[];
  /** The stream's current last sequence; 0 for an empty stream. */
  head: number;
}

/**
 * `POST /v1/graph/topology`: a graph document's shape.
 *
 * `semanticHash` is the field that makes the rest usable. An execution's log records its graph's
 * HASH and never its topology, so a client that wants to draw a run's graph must compare this
 * hash against the `graphHash` in that run's `execution_started` event before believing the edges
 * belong to it. `graph/topology.ts` does that comparison; nothing else may skip it.
 */
export interface GraphTopology {
  graphId: string;
  graphVersion: number;
  executionId: string;
  semanticHash: string;
  entrypoints: string[];
  /** Endpoint identities only. Labels and state come from the execution event stream. */
  nodes: Array<{ id: string }>;
  edges: Array<{ id: string; from: string; to: string; type: string }>;
  [key: string]: unknown;
}

/** Who a mutation is recorded as. `owner` is the person at the keyboard; `agent` is the WebMCP
 * adapter acting on the agent's behalf. The two are never conflated - a click by the operator
 * that an agent asked for is still the AGENT's action, and the log must say so. */
/** An actor this Studio SENDS: the two identities it can act as. Narrow on purpose - a request
 * never claims to be the runtime or a human it is not. */
export interface Actor {
  id: string;
  type: "owner" | "agent";
}

/** The wire's actor vocabulary - `$defs.actor.properties.type.enum` in
 * `schemas/event-envelope.schema.json`. Restated as a type here because a JSON import yields
 * `string[]`, not literals; `client.ts` pins this union against the schema's enum at runtime,
 * so the two cannot drift silently (PR #662 review, client.ts:684). */
export type WireActorType = "owner" | "human" | "agent" | "system";

/** An actor as the LEDGER recorded it. Evidence reports what the record holds, and the record
 * may name the runtime (an immediate pause is appended by the driver) or a human - so this is
 * the wire's whole vocabulary, never the narrow request type. */
export interface RecordedActor {
  id: string;
  type: WireActorType;
}

/** What a write tool returns: enough to decide whether the journey actually happened, without
 * the caller having to re-read anything. */
export interface MutationEvidence {
  action:
    | "start"
    | "pause"
    | "approve"
    | "resume"
    | "signal"
    | "cancel"
    | "sweep"
    | "amendBudget"
    | "claim"
    | "clear";
  executionId: string;
  node: string | null;
  /** The actor as RECORDED (or, on a refusal, as sent): the wire's whole vocabulary, because the
   * ledger may attribute an outcome to the runtime or a human - never narrowed to the request
   * type by a cast (PR #662 review). */
  actor: RecordedActor;
  /** The idempotency key this logical action used. Safe to publish: it is a caller-minted
   * correlation id, never a credential, and it is what makes a retry auditable. */
  idempotencyKey: string;
  headBefore: number;
  headAfter: number | null;
  /** `succeeded` when the store moved and the re-read agrees; `refused` when the Runtime said
   * no and nothing changed; `unknown` when the mutation was accepted but the verification read
   * failed, so the caller must not treat it as done. */
  result: "succeeded" | "refused" | "unknown";
  statusAfter: ExecutionStatus | null;
  newEvents: RuntimeEvent[];
  diagnostics: Diagnostic[];
}

/** One route from the Runtime's gateway manifest, as `GET /v1/gateway/routes` reports it.
 *
 * `model` is what a person choosing is actually choosing; `id` is the deployer's own label for the
 * wiring behind it. Both are shown, because a picker offering only ids asks someone to choose a
 * model they cannot see, and one offering only models cannot say which of two identical models is
 * billed how. `model` is nullable: the manifest need not declare one, and inventing a default here
 * would answer a question the manifest did not. */
export interface ModelRouteSummary {
  id: string;
  provider: string;
  transport: string;
  billingMode: string;
  model: string | null;
  /** Public manifest fields needed to edit a route without replacing hidden configuration. */
  baseUrl?: string | null;
  credentialRef?: string | null;
  profiles: string[];
  enabled: boolean;
  [key: string]: unknown;
}

/** Sealed evidence, opened. `content` is the plaintext - a model's reply, a tool's output - and
 * `sensitivity` names the class it belongs to so a reader knows what they are holding rather than
 * having to infer it from where the id came from. */
export interface EvidenceContent {
  evidenceId: string;
  mediaType: string;
  sensitivity: string;
  contentSha256: string;
  content: string;
}

/**
 * `GET /v1/executions/{id}/briefing` (#1063): what a harness that was not there needs to pick a
 * run up. The Studio reads it for the two fields nothing else public carries - `name` and
 * `objective` are sealed OUT of the event payloads (D-036), and #1071 records them unsealed in
 * the form declaration and serves them here. Shapes mirror `core/execution/src/briefing.rs`
 * (camelCase on the wire); the fields the Studio does not render are typed loosely on purpose.
 */
export interface Briefing {
  /** The graph document's `metadata.name` at start; `null` for a history declared before it. */
  name: string | null;
  /** The operator's request in their own words - the first entrypoint's objective. */
  objective: string | null;
  /** `fixture` | `gateway`, or `null` when the declaration did not carry it. */
  executor: string | null;
  graphHash?: string | null;
  graphVersion?: number | null;
  decisions?: unknown[];
  workDone?: unknown[];
  pending?: AttentionReason[];
  unevaluated?: unknown[];
  /** `{ kind, ... }` - `resume_held` | `answer` | `diagnose` | `dispatch` | `finished` | `nothing`. */
  nextStep: { kind: string; [key: string]: unknown };
  asOfSequence: number;
  [key: string]: unknown;
}

/**
 * One artefact a completion claim presents, as the Runtime's own parser accepts it.
 *
 * A DIGEST AND A SIZE, never the artefact. "Proof" here means the material that shows the work
 * happened — a test report, a diff, a log — and nothing in this shape is signature-verified. The
 * artefact stays wherever it is; what travels is a fingerprint of it, which is why a browser can
 * produce one without uploading anything.
 */
export interface ClaimEvidence {
  /** The proof KIND this item answers, matched by name against the node's declared `proofKinds`.
   * Fewer kinds than declared is refused (`evidence_budget_unmet`); extra kinds are accepted and
   * recorded as unverified, never counted as stronger proof. */
  kind: string;
  /** `sha256:<64 lowercase hex>`. */
  contentHash: string;
  size: number;
}

/** The wait a parked node is holding open, as the status payload renders it under
 * `customs.nodes.<id>.openWait`. `atSequence` is the ENVELOPE SEQUENCE of the event that parked
 * the node, and it is the wait's identity: a node re-parks, so a name identifies the node but
 * never the wait. */
export interface OpenWait {
  atSequence: number;
  deadline?: string | null;
}
