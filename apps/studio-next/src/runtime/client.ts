/** The public Runtime routes the shell reads, through the same-origin proxy: `/v1` for the first
 * project, `/p/<id>/v1` for a project that joined this Studio's port (#1800). */
export interface RunSummary {
  executionId: string;
  status: string;
  objective?: string | null;
  startedAt: string | null;
  lastEventAt: string | null;
}

/** `GET /v1/executions/{id}/tasks` (#1149, #1234): only the Runtime's lane liveness verdicts are read. */
export interface LaneLiveness {
  lane: string;
  free: boolean;
}

export interface JourneyFlow {
  id: string;
  title: string | null;
  name?: string | null;
  status: string;
  findings?: { severity?: string }[];
}

interface Envelope<T> {
  ok: boolean;
  data: T | null;
  diagnostics?: { message?: string }[];
}

export class RuntimeError extends Error {}

const OWNER = { id: "studio-operator", type: "owner" };

export class Runtime {
  readonly #token: string;
  readonly #base: string;

  constructor(token: string, base = "") {
    this.#token = token;
    this.#base = base;
  }

  async #request<T>(method: string, path: string, body?: unknown, headers: Record<string, string> = {}): Promise<T> {
    let response: Response;
    try {
      response = await fetch(`${this.#base}${path}`, {
        method,
        headers: {
          Authorization: `Bearer ${this.#token}`,
          ...(body === undefined ? {} : { "Content-Type": "application/json" }),
          ...headers,
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(30_000),
      });
    } catch {
      throw new RuntimeError("The Runtime is not answering.");
    }
    let envelope: Envelope<T>;
    try {
      envelope = (await response.json()) as Envelope<T>;
    } catch {
      throw new RuntimeError(`The Runtime replied ${response.status} with a body the Studio could not read.`);
    }
    if (!response.ok || envelope.ok !== true || envelope.data === null) {
      throw new RuntimeError(envelope.diagnostics?.[0]?.message ?? `The Runtime refused (${response.status}).`);
    }
    return envelope.data;
  }

  async runs(): Promise<RunSummary[]> {
    const page = await this.#request<{ executions: RunSummary[] }>("GET", "/v1/executions?limit=50");
    return page.executions;
  }

  async journeys(): Promise<JourneyFlow[]> {
    const view = await this.#request<{ flows: JourneyFlow[] }>("GET", "/v1/journey-flows");
    return view.flows;
  }

  async lanes(executionId: string): Promise<LaneLiveness[]> {
    const reply = await this.#request<{ liveness?: { lanes?: LaneLiveness[] } }>("GET", `/v1/executions/${encodeURIComponent(executionId)}/tasks`);
    return Array.isArray(reply.liveness?.lanes) ? reply.liveness.lanes : [];
  }

  /** `POST /v1/executions/{id}/start`. The idempotency key derives from the run id, so a resend of
   * the same start is reconciled by the Runtime instead of starting a second run. */
  async startRun(executionId: string, graph: Record<string, unknown>): Promise<void> {
    await this.#request("POST", `/v1/executions/${encodeURIComponent(executionId)}/start`, { graph, mode: "autopilot" }, {
      "Idempotency-Key": `start-${executionId}`,
      "If-Match": "0",
      "X-GraphHelm-Actor": OWNER.id,
      "X-GraphHelm-Actor-Type": OWNER.type,
    });
  }
}
