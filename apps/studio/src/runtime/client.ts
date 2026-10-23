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
  /** #1235: open PR slices idle past the stall threshold, with who owes the next step. */
  stalls: { key: string; owner: string | null; pr?: number; reason?: "block_unanswered" | "awaiting_verdict" | "awaiting_merge"; idleSince?: string }[];
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
  /** #502: `since` is when the record that moved the slice into its current step was appended. */
  clock?: { since: string | null };
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

/** #1824: `GET /v1/executions/{id}/collisions`, every path two or more open slices name. */
export interface FileCollisions {
  collisions: { path: string; holders: CollisionHolder[] }[];
  truncated: boolean;
}

export interface CollisionHolder {
  key: string;
  taskId: string;
  issue?: number;
  pr?: number;
  lane?: string;
  branch?: string;
  step: string;
}

/** #1864: `GET /v1/capacity` (apps/cli/src/commands/capacity.rs), owner only. */
export interface Capacity {
  providers: { configured: boolean; reason?: string; list: { provider: string; routes: number; enabledRoutes: number; quotaLeft: number | null; quotaLeftReason?: string }[] };
  buildSlots: { configured: number; unreadable: number; busy: number; queued: number };
  memory: { availableBytes: number | null; totalBytes: number | null; reason: string | null };
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
  /** #1953: the run in flight, `null` when none. Absent from older Runtimes: nothing is seeded. */
  running?: { startedAt: string | null; current: string | null; kind: "replay" | "preview" | null } | null;
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
  before: { run: string; current: boolean; commit: string | null; ranAt: string | null; result: string | null; screens: { screen: string; frameUrl: string; viewports?: Partial<Record<"web" | "mobile", string>> }[] } | null;
  /** #1839/#1867: the PR head's newest captured run of this journey, or `null` (absent before #1839). */
  now?: { pr: number; headSha: string; result: string; ranAt: string | null; recordedAt: string | null; screens: { screen: string; frameUrl: string }[] } | null;
}

export interface JourneyScreen { id: string; url: string; title?: string }
export interface JourneyEdge { id: string; from: string; to: string; acts?: { kind: string; role?: string; name?: string; text?: string }[] }

export interface JourneyLastRun {
  status: "passed" | "failed" | "not_run";
  ranAt: string | null;
  kind: "replay" | "preview" | null;
  reason: string | null;
  failingStep: { kind: string; id: string; result: string; reason: string | null; viewport?: "web" | "mobile" | null } | null;
  flowChanged: boolean | null;
  /** #1777: each viewport's status; a run recorded before viewports is web only, mobile `not_run`. */
  viewports?: { web: "passed" | "failed" | "not_run"; mobile: "passed" | "failed" | "not_run" };
  /** #1777: the one viewport that failed while the other passed, else `null`. */
  failsOnlyOn?: "web" | "mobile" | null;
}

/** #1777: what one viewport of a run recorded. */
export interface JourneyViewportRun {
  result?: StepResult;
  screens?: Record<string, { frame: boolean; width?: number; height?: number; result?: StepResult; reason?: string }>;
  edges?: Record<string, { result?: StepResult | "skipped" | null; reason?: string }>;
}

export type StepResult = "pass" | "fail" | "drift";

export interface CliConfirm { argv: string[]; confirmed: boolean; digest: string }

/** #1662/#1912: what one `cli` act printed, as the run keeps it under `outputs.<edge>`: the command,
 * its exit code, and its stdout/stderr masked and capped by the Runtime. */
export interface CliOutput { argv: string; exit: number | null; stdout: string; stderr: string; stdoutSha256?: string; stderrSha256?: string }

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
  /** #1777: the viewports this run plays, in order, and what each recorded. The top-level
   * `screens`/`edges` are the first one's frames with the worst result at any viewport. */
  viewportsPlanned?: ("web" | "mobile")[];
  /** #1777: the viewport being played while `running`. */
  viewport?: "web" | "mobile";
  viewports?: Partial<Record<"web" | "mobile", JourneyViewportRun>>;
  observerSetup?: { state: "installing" | "ready" | "failed"; message: string; error?: string };
  /** #1662/#1909: a draft's `cli` commands and the digest the owner confirms them by. */
  cliConfirm?: CliConfirm;
  launchLog?: { tail?: string };
  driverLog?: { tail?: string };
  /** #1662/#1912: each `cli` edge's command outputs, in the order they ran (once per path that walked it). */
  outputs?: Record<string, CliOutput[]>;
}

/** `GET /v1/journey-flows/{id}/history`: earlier finished runs, newest first, each with the screens
 * it kept a frame for. */
export interface JourneyPastRun {
  run: string;
  state?: "ready" | "failed";
  ranAt?: string;
  result?: StepResult;
  reason?: string;
  kind?: "replay" | "preview";
  digest?: string;
  screens?: Record<string, { frame?: boolean; width?: number; height?: number; result?: StepResult; reason?: string }>;
  edges?: Record<string, { result?: StepResult | "skipped" | null; reason?: string }>;
}

interface Envelope<T> {
  ok: boolean;
  data: T | null;
  diagnostics?: { message?: string }[];
}

export class RuntimeError extends Error {}

const listOf = <T,>(value: unknown): T[] => (Array.isArray(value) ? (value as T[]) : []);

/** #1899: a tasks reply as the board reads it. A Runtime older than a field (`waiting` came with
 * #1794) leaves it out; a missing list reads as empty, never as a crash of the whole Studio. */
export function boardOf(reply: Partial<TasksBoard>): TasksBoard {
  return {
    ...reply,
    now: typeof reply.now === "string" ? reply.now : new Date().toISOString(),
    tasks: listOf<BoardTask>(reply.tasks),
    liveness: { ...reply.liveness, lanes: listOf<BoardLaneLiveness>(reply.liveness?.lanes) },
    stalls: listOf<TasksBoard["stalls"][number]>(reply.stalls),
    waiting: listOf<LaneWaiting>(reply.waiting),
  };
}

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

  /** #1909: the owner's "Run these commands?" Confirm: the same `POST /v1/journey-flows/{id}/preview`
   * with `{confirm, confirmDigest}` that `graphhelm journey preview --confirm --confirm-digest` sends.
   * Owner credential only; a flow edited since `digest` was shown is refused (`preview.confirm_stale`). */
  async confirmJourneyRun(flowId: string, digest: string, executionId: string | null): Promise<JourneyRun> {
    return this.#request<JourneyRun>("POST", `/v1/journey-flows/${encodeURIComponent(flowId)}/preview`, {
      confirm: true, confirmDigest: digest, ...(executionId === null ? {} : { executionId }),
    });
  }

  async journeyHistory(flowId: string): Promise<JourneyPastRun[]> {
    const view = await this.#request<{ runs?: JourneyPastRun[] }>("GET", `/v1/journey-flows/${encodeURIComponent(flowId)}/history`);
    return Array.isArray(view.runs) ? view.runs : [];
  }

  /** One screen's captured frame in the current run. `null` when the Runtime has none yet. */
  async journeyFrame(flowId: string, screenId: string, viewport?: "web" | "mobile"): Promise<Blob | null> {
    return this.#image(`/v1/journey-flows/${encodeURIComponent(flowId)}/screens/${encodeURIComponent(screenId)}/frame${viewport === undefined ? "" : `?viewport=${viewport}`}`);
  }

  /** One screen's frame as an earlier run kept it:
   * `GET /v1/journey-flows/{id}/history/{run}/screens/{screen}/frame`. `null` when that run kept none. */
  async journeyPastFrame(flowId: string, run: string, screenId: string, viewport?: "web" | "mobile"): Promise<Blob | null> {
    return this.#image(`/v1/journey-flows/${encodeURIComponent(flowId)}/history/${encodeURIComponent(run)}/screens/${encodeURIComponent(screenId)}/frame${viewport === undefined ? "" : `?viewport=${viewport}`}`);
  }

  /** #1802: the journeys an issue's slices name, each with its "Before = main" run and frame URLs
   * (`GET /v1/executions/{id}/issues/{issue}/journeys`, #1795 R4a). */
  async issueJourneys(executionId: string, issue: number): Promise<IssueJourney[]> {
    const view = await this.#request<{ journeys?: IssueJourney[] }>("GET", `/v1/executions/${encodeURIComponent(executionId)}/issues/${issue}/journeys`);
    return Array.isArray(view.journeys) ? view.journeys : [];
  }

  /** #1802: a frame at a route-relative URL the Runtime handed out. Only its own journey-flow frame
   * routes are fetched, so a crafted URL cannot send the bearer anywhere else. */
  async frameAt(url: string, executionId?: string): Promise<Blob | null> {
    if (!isFrameUrl(url, executionId)) {
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
    return boardOf(await this.#request<Partial<TasksBoard>>("GET", `/v1/executions/${encodeURIComponent(executionId)}/tasks`));
  }

  /** #1804: the paths two or more open slices of a run name (#1824). */
  async collisions(executionId: string): Promise<FileCollisions> {
    const view = await this.#request<Partial<FileCollisions>>("GET", `/v1/executions/${encodeURIComponent(executionId)}/collisions`);
    return { collisions: Array.isArray(view.collisions) ? view.collisions : [], truncated: view.truncated === true };
  }

  /** #1804: what the providers, build slots and memory have left (#1864, owner credentials). */
  async capacity(): Promise<Capacity> {
    return this.#request<Capacity>("GET", "/v1/capacity");
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

  /** #1919: the owner's request, `task.request_recorded` on `POST /v1/executions/{id}/signal`
   * (proof/send-fix.ts `fixBody`). The signal id is the idempotency key; owner credential only. */
  async recordRequest(executionId: string, id: string, body: Record<string, unknown>): Promise<void> {
    await this.#request("POST", `/v1/executions/${encodeURIComponent(executionId)}/signal`, body, this.#owner(id));
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
