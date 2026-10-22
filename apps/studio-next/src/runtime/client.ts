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
  purpose?: string | null;
  feature?: string | null;
  screens?: JourneyScreen[];
  edges?: JourneyEdge[];
  paths?: Record<string, string[]>;
  /** #1835: the last finished run as its runner recorded it. Absent from older Runtimes: unknown,
   * never read as `not_run`. */
  lastRun?: JourneyLastRun;
}

export interface JourneyScreen { id: string; url: string; title?: string }
export interface JourneyEdge { id: string; from: string; to: string; acts?: { kind: string; role?: string; name?: string; text?: string }[] }

export interface JourneyLastRun {
  status: "passed" | "failed" | "not_run";
  ranAt: string | null;
  kind: "replay" | "preview" | null;
  reason: string | null;
  failingStep: { kind: string; id: string; result: string; reason: string | null } | null;
  flowChanged: boolean | null;
}

export type StepResult = "pass" | "fail" | "drift";

/** `GET|POST /v1/journey-flows/{id}/preview` (#519): the journey's run. While `running`, `screens`
 * holds only the screens reached so far; `observerSetup` reports the browser being prepared (#1763). */
export interface JourneyRun {
  state: "none" | "running" | "ready" | "failed";
  kind?: "replay" | "preview";
  startedAt?: string;
  ranAt?: string;
  current?: string;
  result?: StepResult;
  reason?: string;
  screens?: Record<string, { frame: boolean; width?: number; height?: number; result?: StepResult; reason?: string }>;
  edges?: Record<string, { result?: StepResult | "skipped" | null; reason?: string }>;
  observerSetup?: { state: "installing" | "ready" | "failed"; message: string; error?: string };
  launchLog?: { tail?: string };
  driverLog?: { tail?: string };
}

/** `GET /v1/journey-flows/{id}/history`: earlier finished runs, newest first. */
export interface JourneyPastRun { run: string; ranAt?: string; result?: StepResult; kind?: "replay" | "preview" }

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

  async journeyRun(flowId: string): Promise<JourneyRun> {
    return this.#request<JourneyRun>("GET", `/v1/journey-flows/${encodeURIComponent(flowId)}/preview`);
  }

  /** Run: `POST /v1/journey-flows/{id}/preview` with `force`, so a cached result is not served. An
   * approved flow's replay records its proof into `executionId` when one is open. */
  async startJourneyRun(flowId: string, executionId: string | null): Promise<JourneyRun> {
    return this.#request<JourneyRun>("POST", `/v1/journey-flows/${encodeURIComponent(flowId)}/preview`, {
      force: true, ...(executionId === null ? {} : { executionId }),
    });
  }

  async journeyHistory(flowId: string): Promise<JourneyPastRun[]> {
    const view = await this.#request<{ runs?: JourneyPastRun[] }>("GET", `/v1/journey-flows/${encodeURIComponent(flowId)}/history`);
    return Array.isArray(view.runs) ? view.runs : [];
  }

  /** One screen's captured frame, fetched with the bearer header (never a token in a URL). `null`
   * when the Runtime has none yet (204, 304, 404). */
  async journeyFrame(flowId: string, screenId: string): Promise<Blob | null> {
    let response: Response;
    try {
      response = await fetch(`${this.#base}/v1/journey-flows/${encodeURIComponent(flowId)}/screens/${encodeURIComponent(screenId)}/frame`, {
        headers: { Authorization: `Bearer ${this.#token}` },
        signal: AbortSignal.timeout(30_000),
      });
    } catch {
      throw new RuntimeError("The Runtime is not answering.");
    }
    if (response.status === 204 || response.status === 304 || response.status === 404) return null;
    if (!response.ok) throw new RuntimeError(`The Runtime refused the frame (${response.status}).`);
    if (!(response.headers.get("content-type") ?? "").toLowerCase().startsWith("image/")) throw new RuntimeError("The Runtime did not return an image for this step.");
    return response.blob();
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
