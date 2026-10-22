/** The public Runtime routes the shell reads, through the same-origin proxy: `/v1` for the first
 * project, `/p/<id>/v1` for a project that joined this Studio's port (#1800). */
import type { OwnerRequest } from "../graph/requests";
import { isFrameUrl } from "../graph/review";
import type { Draft, LaneControl } from "../lanes/drafts";

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

/** #1803: the open board of `GET /v1/executions/{id}/tasks` (apps/cli/src/commands/execution/tasks.rs
 * `OpenTasksView`), only the fields the Lanes board reads. */
export interface TasksBoard {
  now: string;
  tasks: BoardTask[];
  liveness: { lanes: BoardLaneLiveness[] };
  stalls: { key: string; owner: string | null }[];
  waiting: LaneWaiting[];
}

export type TaskStepName = "plan" | "critic" | "implement" | "review" | "merge" | "merged";

export interface BoardTask {
  key: string;
  issue: number | null;
  pr: number | null;
  lane: string | null;
  currentReviewer: string | null;
  step: TaskStepName;
  blockedBy: { reviewer: string } | null;
  title: string | null;
  prTitle: string | null;
  journeys: string[];
  lastSequence: number;
  releasedBy?: string;
}

export interface BoardLaneLiveness {
  lane: string;
  lastRecordAt?: string;
  /** #1851: what the lane reported on its claim (owner view only); absent when it never said. */
  agentKind?: string;
  agentModel?: string;
  owes: boolean;
  silent: boolean;
  free: boolean;
}

/** #1794/#1827: one open question of a lane; only its newest five are rows, `laneTotal` counts all. */
export interface LaneWaiting {
  lane: string;
  question: string;
  waitsOn: string;
  since: string;
  signalId: string;
  sequence: number;
  laneTotal: number;
}

/** #1793: one verified `task.*` record a lane recorded, from its activity log. */
export interface LaneAction {
  kind: string;
  sequence: number;
  issue?: number;
  pr?: number;
  reviewer?: string;
  verdict?: string;
  activity?: string;
  reason?: string;
  title?: string;
  occurredAt?: string | null;
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
  /** The owner's approval block, `null` while none is recorded. */
  approved?: unknown;
  /** Whether the Runtime would accept Approve now (#353): no blocking findings, a passing preview. */
  approvable?: boolean;
  /** #1588: the owner's latest rejection of this exact content, with its reason. */
  rejected?: { note?: string } | null;
}

/** #1802: one journey an issue names, with its newest finished run on main (`null`: never run). */
export interface IssueJourney {
  id: string;
  title: string | null;
  before: { run: string; current: boolean; commit: string | null; ranAt: string | null; result: string | null; screens: { screen: string; frameUrl: string }[] } | null;
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

/** `GET /v1/journey-flows/{id}/history`: earlier finished runs, newest first, each with the screens
 * it kept a frame for. */
export interface JourneyPastRun {
  run: string;
  ranAt?: string;
  result?: StepResult;
  kind?: "replay" | "preview";
  screens?: Record<string, { frame?: boolean; width?: number; height?: number; result?: StepResult }>;
}

interface Envelope<T> {
  ok: boolean;
  data: T | null;
  diagnostics?: { message?: string }[];
}

export class RuntimeError extends Error {}

export const OWNER = { id: "studio-operator", type: "owner" };

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

  /** One screen's captured frame in the current run. `null` when the Runtime has none yet. */
  async journeyFrame(flowId: string, screenId: string): Promise<Blob | null> {
    return this.#image(`/v1/journey-flows/${encodeURIComponent(flowId)}/screens/${encodeURIComponent(screenId)}/frame`);
  }

  /** One screen's frame as an earlier run kept it:
   * `GET /v1/journey-flows/{id}/history/{run}/screens/{screen}/frame`. `null` when that run kept none. */
  async journeyPastFrame(flowId: string, run: string, screenId: string): Promise<Blob | null> {
    return this.#image(`/v1/journey-flows/${encodeURIComponent(flowId)}/history/${encodeURIComponent(run)}/screens/${encodeURIComponent(screenId)}/frame`);
  }

  /** #1802: the journeys an issue's slices name, each with its "Before = main" run and frame URLs
   * (`GET /v1/executions/{id}/issues/{issue}/journeys`, #1795 R4a). */
  async issueJourneys(executionId: string, issue: number): Promise<IssueJourney[]> {
    const view = await this.#request<{ journeys?: IssueJourney[] }>("GET", `/v1/executions/${encodeURIComponent(executionId)}/issues/${issue}/journeys`);
    return Array.isArray(view.journeys) ? view.journeys : [];
  }

  /** #1802: a frame at a route-relative URL the Runtime handed out. Only its own journey-flow frame
   * routes are fetched, so a crafted URL cannot send the bearer anywhere else. */
  async frameAt(url: string): Promise<Blob | null> {
    if (!isFrameUrl(url)) {
      throw new RuntimeError("The Runtime named a frame outside its journey frames.");
    }
    return this.#image(url);
  }

  /** A frame fetched with the bearer header (never a token in a URL). `null` for 204, 304, 404. */
  async #image(path: string): Promise<Blob | null> {
    let response: Response;
    try {
      response = await fetch(`${this.#base}${path}`, {
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

  /** Approve: `POST /v1/journey-flows/{id}/approve` (#353), exactly `graphhelm journey approve`.
   * Owner credential only; a flow with findings or no passing preview is refused. */
  async approveJourney(flowId: string): Promise<void> {
    await this.#request("POST", `/v1/journey-flows/${encodeURIComponent(flowId)}/approve`);
  }

  /** Reject: `POST /v1/journey-flows/{id}/reject` with `{"note"}` (#1588). The Runtime refuses a
   * blank note; owner credential only. */
  async rejectJourney(flowId: string, note: string): Promise<void> {
    await this.#request("POST", `/v1/journey-flows/${encodeURIComponent(flowId)}/reject`, { note });
  }

  async lanes(executionId: string): Promise<LaneLiveness[]> {
    const reply = await this.#request<{ liveness?: { lanes?: LaneLiveness[] } }>("GET", `/v1/executions/${encodeURIComponent(executionId)}/tasks`);
    return Array.isArray(reply.liveness?.lanes) ? reply.liveness.lanes : [];
  }

  /** #1803: the open board of a run (tasks, liveness, stalls and open questions). */
  async board(executionId: string): Promise<TasksBoard> {
    return this.#request<TasksBoard>("GET", `/v1/executions/${encodeURIComponent(executionId)}/tasks`);
  }

  /** #1801: the owner's requests with their issues, PR slices and status (#1792 R1,
   * `GET /v1/executions/{id}/requests`). */
  async requests(executionId: string): Promise<OwnerRequest[]> {
    const view = await this.#request<{ requests?: OwnerRequest[] }>("GET", `/v1/executions/${encodeURIComponent(executionId)}/requests`);
    return Array.isArray(view.requests) ? view.requests : [];
  }

  /** #1793: a lane's last `limit` actions, newest first (owner credentials). */
  async laneActivity(executionId: string, lane: string, limit: number): Promise<LaneAction[]> {
    const log = await this.#request<{ actions?: LaneAction[] }>("GET", `/v1/executions/${encodeURIComponent(executionId)}/lanes/${encodeURIComponent(lane)}/activity?limit=${limit}`);
    return Array.isArray(log.actions) ? log.actions : [];
  }

  /** #1803: the owner's message to a lane, `POST /v1/executions/{id}/signal`, sent as the draft
   * holds it (lanes/drafts.ts `messageBody`). The draft id is both the signal id and the
   * idempotency key, and a resend carries the same body byte for byte, so the Runtime reconciles
   * it instead of refusing it (#1837). */
  async messageLane(executionId: string, draft: Draft): Promise<void> {
    await this.#request("POST", `/v1/executions/${encodeURIComponent(executionId)}/signal`, draft.body, this.#owner(draft.id));
  }

  /** #1847: the owner's Wake, Reassign or Stop of one lane,
   * `POST /v1/executions/{id}/lanes/{lane}/{wake|reassign|stop}`, owner credential only. The
   * draft's id is the `Idempotency-Key` and its body carries `at`, so a resend is a replay. */
  async controlLane(executionId: string, lane: string, control: LaneControl, draft: Draft): Promise<void> {
    await this.#request("POST", `/v1/executions/${encodeURIComponent(executionId)}/lanes/${encodeURIComponent(lane)}/${control}`, draft.body, this.#owner(draft.id));
  }

  #owner(key: string): Record<string, string> {
    return { "Idempotency-Key": key, "X-GraphHelm-Actor": OWNER.id, "X-GraphHelm-Actor-Type": OWNER.type };
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
