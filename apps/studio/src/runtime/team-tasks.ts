import { digestOf } from "./customs";
import type { EvidenceContent, RuntimeEvent } from "./types";
import { clockStep, emptyClock, type StepClock } from "./step-timing";

const PROTOCOL = "graphhelm-native-task-v1";
const MAX_ENVELOPE_BYTES = 1024 * 1024;

export function isClaudeTaskSignal(event: RuntimeEvent): boolean {
  if (event.kind !== "signal_recorded") return false;
  const kind = record(event.payload)?.kind;
  return kind === "agent_task_created" || kind === "agent_task_completed";
}

export interface ClaudeTaskObservation {
  executionId: string;
  nativeTaskId: string;
  taskSubject: string;
  createdByTeammateName: string | null;
  completedByTeammateName: string | null;
  sourceId: string;
  parentSessionId: string;
  createdSequence: number | null;
  createdAt: string | null;
  createdEvidenceId: string | null;
  completedSequence: number | null;
  completedAt: string | null;
  completedEvidenceId: string | null;
}

export interface ClaudeTaskReadModel {
  executionId: string;
  tasks: ClaudeTaskObservation[];
  rejected: number;
}

export interface ReadClaudeTasksOptions {
  executionId: string;
  events: RuntimeEvent[];
  readEvidence: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function text(value: unknown, maximum: number): string | null {
  return typeof value === "string" && value.length > 0 && value.length <= maximum && !/[\u0000-\u001f\u007f]/.test(value)
    ? value
    : null;
}

function sequence(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function taskIdentity(value: unknown): string | null {
  return typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(value) ? value : null;
}

function journeyIds(value: unknown): string[] | null {
  return Array.isArray(value) && value.every((id) => taskIdentity(id) !== null) ? value as string[] : null;
}

function json(content: EvidenceContent): Record<string, unknown> | null {
  if (content.mediaType !== "application/json" || typeof content.content !== "string") return null;
  if (new TextEncoder().encode(content.content).byteLength > MAX_ENVELOPE_BYTES) return null;
  try {
    return record(JSON.parse(content.content));
  } catch {
    return null;
  }
}

function rawHash(value: string): string {
  return value.startsWith("sha256:") ? value.slice("sha256:".length) : value;
}

/** Reads only sealed Claude task lifecycle envelopes. Completion means Claude marked a task
 * complete; it does not mean its output was reviewed or accepted. */
export async function readClaudeTasks({ executionId, events, readEvidence }: ReadClaudeTasksOptions): Promise<ClaudeTaskReadModel> {
  const ordered = [...events].sort((a, b) => a.sequence - b.sequence);
  const tasks = new Map<string, ClaudeTaskObservation>();
  let rejected = 0;
  for (const event of ordered) {
    if (!isClaudeTaskSignal(event)) continue;
    const payload = record(event.payload);
    const kind = payload?.kind;
    const phase = kind === "agent_task_created" ? "created" : "completed";
    const seq = sequence(event.sequence);
    const evidenceId = event.evidenceRefs.length === 1 ? event.evidenceRefs[0] : null;
    const sourceId = text(payload?.sourceId, 128);
    const signalId = text(payload?.signalId, 256);
    const envelopeHash = text(payload?.envelopeSha256, 128);
    if (seq === null || evidenceId === null || sourceId === null || signalId === null || envelopeHash === null) {
      rejected++;
      continue;
    }
    let evidence: EvidenceContent;
    try { evidence = await readEvidence(executionId, evidenceId); } catch { rejected++; continue; }
    if (evidence.evidenceId !== evidenceId || evidence.content.length > MAX_ENVELOPE_BYTES) { rejected++; continue; }
    const envelope = json(evidence);
    const detailText = typeof envelope?.description === "string" && envelope.description.length <= MAX_ENVELOPE_BYTES
      ? envelope.description : null;
    const detail = detailText === null ? null : (() => {
      try { return record(JSON.parse(detailText)); } catch { return null; }
    })();
    const source = record(envelope?.source);
    const host = detail?.host;
    const parent = taskIdentity(detail?.parentSessionId);
    const taskId = taskIdentity(detail?.nativeTaskId);
    const subject = text(detail?.taskSubject, 256);
    const teammate = detail?.teammateName === null ? null : text(detail?.teammateName, 128);
    let actor = typeof host === "string" && parent !== null ? `${host}-session-${parent}` : null;
    if (actor !== null && actor.length > 128) {
      const identityHash = rawHash(await digestOf(new TextEncoder().encode(`${host}\u0000${parent}`).buffer, globalThis.crypto.subtle));
      actor = `agent-session-${identityHash.slice(0, 48)}`;
    }
    const actualHash = rawHash(evidence.contentSha256);
    const computedHash = rawHash(await digestOf(new TextEncoder().encode(evidence.content).buffer, globalThis.crypto.subtle));
    if (
      detail?.protocol !== PROTOCOL || detail?.executionId !== executionId || detail?.phase !== phase || host !== "claude" ||
      parent === null || taskId === null || subject === null || (detail?.teammateName !== null && teammate === null) ||
      actor === null || actor.length > 128 || source?.type !== "tool" || source?.id !== actor ||
      sourceId !== actor || payload?.sourceKind !== "tool" || event.actorId !== actor || event.actorType !== "agent" ||
      envelope?.id !== signalId || envelope?.type !== kind || envelope?.evidence === undefined ||
      !Array.isArray(envelope.evidence) || envelope.evidence.length !== 1 || envelope.evidence[0] !== executionId ||
      rawHash(envelopeHash) !== actualHash || actualHash !== computedHash
    ) { rejected++; continue; }
    const key = `${actor}\u0000${taskId}`;
    const current = tasks.get(key);
    if (phase === "created") {
      if (current?.createdSequence !== null && current !== undefined) { rejected++; continue; }
      tasks.set(key, { executionId, nativeTaskId: taskId, taskSubject: subject,
        createdByTeammateName: teammate, completedByTeammateName: current?.completedByTeammateName ?? null,
        sourceId: actor, parentSessionId: parent, createdSequence: seq, createdAt: event.occurredAt, createdEvidenceId: evidenceId,
        completedSequence: current?.completedSequence ?? null, completedAt: current?.completedAt ?? null,
        completedEvidenceId: current?.completedEvidenceId ?? null });
    } else {
      if (current?.completedSequence !== null && current !== undefined) { rejected++; continue; }
      tasks.set(key, current ? { ...current, completedByTeammateName: teammate, completedSequence: seq, completedAt: event.occurredAt, completedEvidenceId: evidenceId }
        : { executionId, nativeTaskId: taskId, taskSubject: subject, createdByTeammateName: null,
          completedByTeammateName: teammate,
          sourceId: actor, parentSessionId: parent, createdSequence: null, createdAt: null, createdEvidenceId: null,
          completedSequence: seq, completedAt: event.occurredAt, completedEvidenceId: evidenceId });
    }
  }
  return { executionId, tasks: [...tasks.values()].sort((a, b) => (a.createdSequence ?? a.completedSequence ?? 0) - (b.createdSequence ?? b.completedSequence ?? 0)), rejected };
}

/* #386 (journey-first spec §7): the `task.*` records a lane writes at each delivery step, one
 * `graphhelm-task-event-v1` document in the signal's description. The per-task graph is folded
 * from these alone; GitHub is linked, never polled. */
const TASK_EVENT_SCHEMA = "graphhelm-task-event-v1";
const VERDICTS = ["APPROVE", "APPROVE-WITH-RISK", "BLOCK"] as const;
type Verdict = typeof VERDICTS[number];

export type TaskEventKind = "task.claimed" | "task.planned" | "task.pr_opened" | "task.review_assigned" | "task.review_verdict" | "task.merged" | "task.critic_verdict";

/** #467: one round of the blind design critic, as its `task.critic_verdict` record states it. */
export interface CriticRound { round: number; score: number; passScore: number; maxRounds: number; verdict: "pass" | "revise" | "exhausted" }

export interface TaskEventRecord {
  kind: TaskEventKind;
  actorId: string;
  sequence: number;
  taskId: string;
  issue?: number;
  pr?: number;
  lane?: string;
  /** The claimant's report of who assigned the work, never independently verified. */
  assignedBy?: string;
  branch?: string;
  headSha?: string;
  journeys?: string[];
  reviewer?: string;
  verdict?: Verdict;
  commentUrl?: string;
  /** `owner/name` of the GitHub repository, from the record that opens the task (#420). */
  repo?: string;
  mergeSha?: string;
  closes?: number[];
  /** #477: the issue's (claimed) or the PR's (pr_opened) title and one-line summary. */
  title?: string;
  summary?: string;
  /** #514: the issue whose work turned this task up (on `task.claimed`). */
  parent?: number;
  /** #502: when the Runtime appended the record (its own clock, not the lane's `at`). */
  occurredAt?: string | null;
  /** #467: the round a `task.critic_verdict` states. */
  critic?: CriticRound;
  /** #480: the lane's keel plan, from `task.planned`. */
  plan?: TaskPlan;
}

/** #477: optional words for the Team tab. Absent is `{}`; present but malformed is `false` (the
 * record is refused, like the Runtime's admission). */
function words(document: Record<string, unknown>): { title?: string; summary?: string } | false {
  const out: { title?: string; summary?: string } = {};
  // Characters (code points), as the schema, the Runtime and the recipe count them: `.length`
  // counts UTF-16 units, so an emoji would count twice (#486 review).
  const characters = (value: unknown, maximum: number) =>
    typeof value === "string" && Array.from(value).length <= maximum ? text(value, value.length) : null;
  if (document.title !== undefined) {
    const title = characters(document.title, 200);
    if (title === null) return false;
    out.title = title;
  }
  if (document.summary !== undefined) {
    const summary = characters(document.summary, 300);
    if (summary === null) return false;
    out.summary = summary;
  }
  return out;
}

const TASK_CLASSES = ["docs", "code", "user_visible", "invariant"] as const;
const PROOFS = ["none", "tests", "journey", "both"] as const;

/** #480: what `task.planned` carries: the keel plan's decided classes, review count, proof, the
 * critic its class gets (#467), and the plan in one line. */
export interface TaskPlan {
  classes: typeof TASK_CLASSES[number][];
  reviews: number;
  proof: typeof PROOFS[number];
  critic: { mode: "none" | "design"; passScore: number; maxRounds: number };
  summary: string;
}

function bounded(value: unknown, min: number, max: number): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= min && value <= max ? value : null;
}

/** The same bounds as `task-event.schema.json`'s `task.planned` and the Runtime's admission. */
function taskPlan(document: Record<string, unknown>): TaskPlan | null {
  const classes = Array.isArray(document.classes) && document.classes.length >= 1 && document.classes.length <= 4
    && document.classes.every((value, at, all) => TASK_CLASSES.some((known) => known === value) && all.indexOf(value) === at)
    ? document.classes as TaskPlan["classes"] : null;
  const reviews = bounded(document.reviews, 1, 5);
  const proof = PROOFS.find((value) => value === document.proof);
  const critic = record(document.critic);
  const mode = critic?.mode === "none" || critic?.mode === "design" ? critic.mode : null;
  const passScore = bounded(critic?.passScore, 1, 10);
  const maxRounds = bounded(critic?.maxRounds, 1, 5);
  const criticKeys = critic !== null && Object.keys(critic).every((key) => key === "mode" || key === "passScore" || key === "maxRounds");
  const summary = typeof document.summary === "string" && Array.from(document.summary).length <= 300 ? text(document.summary, document.summary.length) : null;
  return classes !== null && reviews !== null && proof !== undefined && mode !== null && passScore !== null
    && maxRounds !== null && criticKeys && summary !== null
    ? { classes, reviews, proof, critic: { mode, passScore, maxRounds }, summary } : null;
}

function count(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0 ? value : null;
}

/** `owner/name` (#420): absent is `null`, malformed is `false` (the record is refused). */
function repository(value: unknown): string | null | false {
  if (value === undefined) return null;
  // The same rule as `task-event.schema.json` `$defs.repo` and the Runtime's admission.
  return typeof value === "string" && value.length <= 140
    && /^[A-Za-z0-9][A-Za-z0-9-]{0,38}\/(\.\.?[A-Za-z0-9_-][A-Za-z0-9_.-]{0,97}|[A-Za-z0-9_-][A-Za-z0-9_.-]{0,99})$/.test(value)
    ? value : false;
}

function sha(value: unknown): string | null {
  return typeof value === "string" && /^[0-9a-f]{7,64}$/.test(value) ? value : null;
}

/** One record, or `null` when it is not a well-formed `task.*` document or names another actor
 * than the one that recorded it (the Runtime refuses those too; this keeps an old log honest). */
export function parseTaskEvent(kind: string, actorId: string, description: string): Omit<TaskEventRecord, "sequence"> | null {
  if (description.length > MAX_ENVELOPE_BYTES) return null;
  let document: Record<string, unknown> | null;
  try { document = record(JSON.parse(description)); } catch { return null; }
  const taskId = taskIdentity(document?.taskId);
  if (document === null || document.schema !== TASK_EVENT_SCHEMA || taskId === null || count(document.revision) === null) return null;
  const base = { actorId, taskId };
  switch (kind) {
    case "task.claimed": {
      const repo = repository(document.repo);
      if (repo === false) return null;
      const issue = count(document.issue);
      const lane = text(document.lane, 128);
      const assignedBy = document.assignedBy === undefined ? undefined
        : typeof document.assignedBy === "string" && Array.from(document.assignedBy).length <= 128 && !/\p{Cc}/u.test(document.assignedBy)
          ? text(document.assignedBy, document.assignedBy.length) : null;
      if (assignedBy === null || (assignedBy !== undefined && assignedBy === lane)) return null;
      const branch = text(document.branch, 256);
      const said = words(document);
      if (said === false) return null;
      const parent = document.parent === undefined ? undefined : count(document.parent);
      if (parent === null) return null;
      // #577: a claim may name its journeys; a malformed list drops the field, never the claim.
      const journeys = journeyIds(document.journeys);
      return issue !== null && lane === actorId && branch !== null
        ? { ...base, kind, issue, lane, branch, ...said, ...(assignedBy === undefined ? {} : { assignedBy }), ...(repo === null ? {} : { repo }), ...(parent === undefined ? {} : { parent }), ...(journeys === null ? {} : { journeys }) } : null;
    }
    case "task.planned": {
      const plan = taskPlan(document);
      return plan !== null && document.lane === actorId ? { ...base, kind, lane: actorId, plan } : null;
    }
    case "task.pr_opened": {
      const repo = repository(document.repo);
      if (repo === false) return null;
      const pr = count(document.pr);
      const headSha = sha(document.headSha);
      const lane = text(document.lane, 128);
      const journeys = journeyIds(document.journeys);
      const said = words(document);
      if (said === false) return null;
      return pr !== null && headSha !== null && lane === actorId && journeys !== null ? { ...base, kind, pr, headSha, lane, journeys, ...said, ...(repo === null ? {} : { repo }) } : null;
    }
    case "task.review_assigned": {
      const pr = count(document.pr);
      const headSha = sha(document.headSha);
      const reviewer = text(document.reviewer, 128);
      return pr !== null && headSha !== null && reviewer !== null ? { ...base, kind, pr, headSha, reviewer } : null;
    }
    case "task.review_verdict": {
      const pr = count(document.pr);
      const headSha = sha(document.headSha);
      const verdict = VERDICTS.find((value) => value === document?.verdict);
      const commentUrl = text(document.commentUrl, 512);
      return pr !== null && headSha !== null && document.reviewer === actorId && verdict !== undefined && commentUrl !== null
        ? { ...base, kind, pr, headSha, reviewer: actorId, verdict, commentUrl } : null;
    }
    case "task.merged": {
      const pr = count(document.pr);
      const mergeSha = sha(document.mergeSha);
      const closes = Array.isArray(document.closes) && document.closes.every((n) => count(n) !== null) ? document.closes as number[] : null;
      return pr !== null && mergeSha !== null && closes !== null && document.merger === actorId ? { ...base, kind, pr, mergeSha, closes } : null;
    }
    case "task.critic_verdict": {
      // The same rule as the Runtime's admission: the verdict agrees with the score and the round,
      // so an old or foreign log cannot draw an under-threshold pass.
      const within = (value: unknown, low: number, high: number) =>
        typeof value === "number" && Number.isSafeInteger(value) && value >= low && value <= high ? value : null;
      const round = within(document.round, 1, 5);
      const score = within(document.score, 0, 10);
      const passScore = within(document.passScore, 1, 10);
      const maxRounds = within(document.maxRounds, 1, 5);
      if (round === null || score === null || passScore === null || maxRounds === null || round > maxRounds || document.lane !== actorId) return null;
      const verdict = document.verdict === "pass" && score >= passScore ? "pass"
        : document.verdict === "revise" && score < passScore && round < maxRounds ? "revise"
        : document.verdict === "exhausted" && score < passScore && round === maxRounds ? "exhausted" : null;
      return verdict === null ? null : { ...base, kind, lane: actorId, critic: { round, score, passScore, maxRounds, verdict } };
    }
    default:
      return null;
  }
}

/** #480: a claimed task is planning until its `task.planned`; a `design` plan then waits on its
 * critic (#467) before implementation. */
export type TaskStep = "plan" | "critic" | "implement" | "review" | "merge" | "merged";

/** #514 (owner: a node for the fix and the re-review): one round per BLOCK. The fix is done when a
 * `pr_opened` names a newer head; the re-review is that head's review. Derived from records only. */
export interface ReviewRound {
  reviewer: string;
  /** The head the BLOCK was on, and the comment that gave the reason. */
  headSha: string;
  commentUrl: string;
  /** The head the author pushed in answer, once a `pr_opened` named it. */
  fixHead: string | null;
  /** #502: when the BLOCK and the fix were recorded (the Runtime's clock), for the round's timers. */
  blockedAt: string | null;
  fixedAt: string | null;
}

export type StrayVerdict =
  | { reason: "unrecorded"; reviewer: string; verdict: string; headSha: string; record: TaskEventRecord }
  | { reason: "superseded"; reviewer: string; verdict: string; headSha: string; supersededBy: string };

export interface TaskState {
  /** #460: one slice of a task, unique in the fold: its PR once one is recorded, else its claim. */
  key: string;
  taskId: string;
  /** The branch the slice's `task.claimed` named, if any. */
  branch: string | null;
  issue: number | null;
  pr: number | null;
  lane: string | null;
  assignedBy?: string | null;
  /** Only explicit review assignments; `reviewers` also includes verdict authors. */
  assignedReviewers?: string[];
  headSha: string | null;
  journeys: string[];
  /** #480: the lane's recorded keel plan; `null` (or absent, before #480) when none was recorded. */
  plan?: TaskPlan | null;
  /** The step that is lit. */
  step: TaskStep;
  /** A BLOCK on the current head that no newer head (#608) or verdict has answered: the red edge
   * into the next step. */
  blockedBy: { reviewer: string; headSha: string; commentUrl: string } | null;
  reviewers: string[];
  mergeSha: string | null;
  /** `https://github.com/<owner>/<repo>`, read off a verdict's comment URL; links need it. */
  repoUrl: string | null;
  /** #457: verdicts that do not speak for the current head, newest last, each with its reason: a head
   * no `pr_opened` has named yet (applied if that `pr_opened` arrives later, e.g. a back-fill), or an
   * older recorded head that a newer push superseded. Dropping them drew "no review" where one exists. */
  strayVerdicts: StrayVerdict[];
  /** #477: the issue's title and summary (from `task.claimed`) and the PR's (from `task.pr_opened`). */
  title: string | null;
  summary: string | null;
  prTitle: string | null;
  prSummary: string | null;
  /** #467: the design critic's newest recorded round, or `null` when the task has none. */
  critic: CriticRound | null;
  /** Every head a `pr_opened` named, in order. */
  recordedHeads: string[];
  /** #514: the issue whose work turned this task up; the Studio draws it inside that issue's card. */
  parent: number | null;
  /** #514: the BLOCK rounds of the review loop, oldest first. */
  rounds: ReviewRound[];
  /** #502: when the slice entered its step and what it spent in the steps it left. */
  clock: StepClock;
  lastSequence: number;
}

function applyVerdict(state: TaskState, event: TaskEventRecord): void {
  if (event.reviewer && !state.reviewers.includes(event.reviewer)) state.reviewers.push(event.reviewer);
  // Old logs carry no `repo`: a verdict's comment URL still names the repository.
  state.repoUrl ??= /^(https:\/\/github\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+)\//.exec(event.commentUrl ?? "")?.[1] ?? state.repoUrl;
  if (event.verdict === "BLOCK") {
    state.blockedBy = { reviewer: event.reviewer ?? "", headSha: event.headSha ?? "", commentUrl: event.commentUrl ?? "" };
    // A second BLOCK on the same unanswered head (another reviewer) is the same round.
    const last = state.rounds.at(-1);
    if (last === undefined || last.fixHead !== null || last.headSha !== event.headSha) {
      state.rounds.push({ reviewer: event.reviewer ?? "", headSha: event.headSha ?? "", commentUrl: event.commentUrl ?? "", fixHead: null,
        blockedAt: event.occurredAt ?? null, fixedAt: null });
    }
    state.step = "review";
  } else {
    state.blockedBy = null;
    state.step = "merge";
  }
}

/** #460: the slice of its task a record belongs to. An issue can be worked in several PRs (#356: one
 * PR merged while the next slice was claimed), and one state per issue let the first merge hide the
 * rest. A claim on a branch no slice holds opens a slice, which that lane's next `pr_opened` joins;
 * every later record names its PR (admission requires `pr`) and lands in that PR's slice, so a
 * merge ends only its own. */
/** #562: a design critic round whose task nobody has claimed. It holds no issue, PR or branch, so
 * it is not a task card: the Team tab lists it in a "No task" row until a claim adopts it. */
export function isUnclaimedCritic(task: TaskState): boolean {
  return task.critic !== null && task.issue === null && task.pr === null && task.branch === null;
}

function sliceFor(slices: TaskState[], event: TaskEventRecord): TaskState {
  // Merge records may use `pr-N` as their task id even when the author claimed `issue-N`.
  // The PR identifies the existing slice across those task ids.
  const byPr = event.pr === undefined ? undefined : slices.find((slice) => slice.pr === event.pr);
  if (byPr !== undefined) return byPr;
  const group = slices.filter((slice) => slice.taskId === event.taskId);
  const open = (slice: TaskState) => slice.pr === null && slice.step !== "merged"
    && (slice.lane === null || event.lane === undefined || slice.lane === event.lane);
  const add = () => {
    const slice: TaskState = {
      key: "", taskId: event.taskId, branch: null, issue: null, pr: event.kind === "task.claimed" ? null : event.pr ?? null,
      lane: null, assignedBy: null, assignedReviewers: [], headSha: null, journeys: [], plan: null,
      step: event.kind === "task.claimed" || event.kind === "task.planned" ? "plan" : "implement", blockedBy: null, reviewers: [], mergeSha: null,
      repoUrl: null, title: null, summary: null, prTitle: null, prSummary: null, strayVerdicts: [], critic: null, recordedHeads: [], parent: null, rounds: [], clock: emptyClock(), lastSequence: 0,
    };
    slices.push(slice);
    return slice;
  };
  // A claim names its branch: the same branch is the same slice (a re-claim), any other opens one.
  if (event.kind === "task.claimed") {
    // A critic round recorded before the claim belongs to this task: the claim adopts it (#562).
    return group.find((slice) => slice.branch !== null && slice.branch === event.branch)
      ?? group.find(isUnclaimedCritic) ?? add();
  }
  // A PR no slice holds yet joins the oldest open claim of its task (for pr_opened, the same
  // lane's): PRs open in the order their slices were claimed. The claim's slice becomes that PR's
  // slice. A merge recorded with no pr_opened (#449's own log) still lands on the issue's claim
  // instead of opening a second graph.
  // #480 (gh-claude-10's BLOCK on 2bba9de4): a plan or a critic round is the recording lane's own
  // record and names no PR, so it belongs to that lane's newest open claim, never to another
  // lane's claim of the same issue (a handover leaves the first lane's claim open).
  if ((event.kind === "task.planned" || event.kind === "task.critic_verdict") && event.lane !== undefined) {
    const own = group.filter((slice) => slice.lane === event.lane);
    const mine = own.filter((slice) => slice.pr === null && slice.step !== "merged").at(-1) ?? own.at(-1);
    if (mine !== undefined) return mine;
    // A planned with no claim of this lane opens its own slice; a critic round keeps #562's route.
    if (event.kind === "task.planned") return add();
  }
  const claimed = group.filter((slice) => event.kind === "task.pr_opened" ? open(slice)
    : slice.pr === null && slice.step !== "merged").at(0);
  if (claimed !== undefined) {
    if (event.pr !== undefined) claimed.pr = event.pr;
    return claimed;
  }
  // A record without a PR (none is admitted today) stays with the newest slice of its task.
  if (event.pr === undefined && group.length > 0) return group[group.length - 1];
  return add();
}

/** Folds records in sequence order into one state per task slice (spec §7): a new head re-arms review
 * but keeps the red edge until a verdict lands on a newer head than the BLOCK's. */
export function foldTaskEvents(records: TaskEventRecord[]): TaskState[] {
  const slices: TaskState[] = [];
  for (const event of [...records].sort((a, b) => a.sequence - b.sequence)) {
    const state = sliceFor(slices, event);
    if (state.step === "merged") continue;
    state.lastSequence = event.sequence;
    // #480 (gh-claude-2): Plan and Critic hold time only once a plan is recorded. Before that, the
    // claim's time is Implement's, as it was before #480, so old tasks' medians stay comparable.
    // Judged after the record is applied, so the `task.planned` that ends Plan books the claim's
    // time as Plan's.
    const timed = (step: TaskStep) => (step === "plan" || step === "critic") && (state.plan ?? null) === null ? "implement" : step;
    const before = state.step;
    switch (event.kind) {
      case "task.claimed":
        state.assignedBy = event.assignedBy ?? null;
        state.branch = event.branch ?? state.branch;
        state.parent = event.parent ?? state.parent;
        state.title = event.title ?? state.title;
        state.summary = event.summary ?? state.summary;
        state.repoUrl = event.repo !== undefined ? `https://github.com/${event.repo}` : state.repoUrl;
        state.issue = event.issue ?? state.issue;
        state.lane = event.lane ?? state.lane;
        state.journeys = event.journeys?.length ? event.journeys : state.journeys;
        break;
      case "task.planned":
        state.lane = event.lane ?? state.lane;
        state.plan = event.plan ?? state.plan ?? null;
        // A plan recorded late (after the PR) informs the graph without moving it back.
        if (state.step === "plan" || state.step === "critic") state.step = state.plan?.critic.mode === "design" ? "critic" : "implement";
        break;
      case "task.pr_opened":
        state.repoUrl = event.repo !== undefined ? `https://github.com/${event.repo}` : state.repoUrl;
        state.pr = event.pr ?? state.pr;
        state.lane = event.lane ?? state.lane;
        state.prTitle = event.title ?? state.prTitle;
        state.prSummary = event.summary ?? state.prSummary;
        state.headSha = event.headSha ?? state.headSha;
        // #577: an empty list on pr_opened does not erase the journeys named at claim.
        state.journeys = event.journeys?.length ? event.journeys : state.journeys;
        state.step = "review";
        // #514: a newer head after a BLOCK is the author's fix; its review is the re-review.
        {
          const round = state.rounds.at(-1);
          if (round !== undefined && round.fixHead === null && event.headSha !== undefined && event.headSha !== round.headSha) {
            round.fixHead = event.headSha;
            round.fixedAt = event.occurredAt ?? null;
          }
        }
        // #608: the author answered the BLOCK with a newer head, so nothing is blocked now: the
        // task waits on the re-review. The BLOCK itself stays in `rounds`, with its fixHead.
        if (state.blockedBy !== null && event.headSha !== undefined && event.headSha !== state.blockedBy.headSha) state.blockedBy = null;
        if (event.headSha !== undefined && !state.recordedHeads.includes(event.headSha)) state.recordedHeads.push(event.headSha);
        // A verdict that arrived before this head's pr_opened (a back-fill) now speaks for it.
        for (const stray of state.strayVerdicts.filter((entry) => entry.reason === "unrecorded" && entry.headSha === state.headSha)) {
          state.strayVerdicts.splice(state.strayVerdicts.indexOf(stray), 1);
          if (stray.reason === "unrecorded") applyVerdict(state, stray.record);
        }
        break;
      case "task.review_assigned":
        if (event.reviewer && !state.assignedReviewers?.includes(event.reviewer)) (state.assignedReviewers ??= []).push(event.reviewer);
        if (event.reviewer && !state.reviewers.includes(event.reviewer)) state.reviewers.push(event.reviewer);
        break;
      case "task.review_verdict":
        // A verdict on a head other than the current one says nothing about the current one.
        if (event.headSha !== state.headSha) {
          const base = { reviewer: event.reviewer ?? "", verdict: event.verdict ?? "", headSha: event.headSha ?? "" };
          state.strayVerdicts.push(event.headSha !== undefined && state.recordedHeads.includes(event.headSha)
            ? { ...base, reason: "superseded", supersededBy: state.headSha ?? "" }
            : { ...base, reason: "unrecorded", record: event });
          break;
        }
        applyVerdict(state, event);
        break;
      case "task.critic_verdict":
        state.critic = event.critic ?? state.critic;
        // The round names who recorded it; an unclaimed task has no other source for it (#562).
        state.lane ??= event.lane ?? null;
        // #480: a passing round ends the Critic step; a revise or an exhausted round keeps it lit.
        if (state.step === "critic" && state.critic?.verdict === "pass") state.step = "implement";
        break;
      case "task.merged":
        state.pr = event.pr ?? state.pr;
        state.mergeSha = event.mergeSha ?? null;
        state.blockedBy = null;
        state.step = "merged";
        break;
    }
    clockStep(state.clock, timed(before), timed(state.step), event.occurredAt);
    if (event.kind === "task.merged" && event.closes !== undefined) {
      for (const claim of slices) {
        if (claim !== state && claim.pr === null && claim.step !== "merged" && claim.issue !== null && event.closes.includes(claim.issue)) {
          const claimBefore = claim.step;
          claim.step = "merged";
          claim.mergeSha = event.mergeSha ?? null;
          claim.blockedBy = null;
          claim.lastSequence = event.sequence;
          clockStep(claim.clock, timed(claimBefore), "merged", event.occurredAt);
        }
      }
    }
  }
  slices.forEach((slice, index) => {
    // A slice opened by a PR record (no claim seen) still belongs to its issue.
    slice.issue ??= slices.find((other) => other.taskId === slice.taskId && other.issue !== null)?.issue ?? null;
    slice.key = slice.pr !== null ? `${slice.taskId}#pr-${slice.pr}` : `${slice.taskId}#claim-${index}`;
  });
  return slices.sort((a, b) => a.lastSequence - b.lastSequence);
}

export function isTaskEventSignal(event: RuntimeEvent): boolean {
  if (event.kind !== "signal_recorded") return false;
  const kind = record(event.payload)?.kind;
  return typeof kind === "string" && kind.startsWith("task.");
}

/** How many envelope reads one fold keeps in flight (#185). A browser opens about six connections
 * per host anyway; more than that only queues in the browser. */
const EVIDENCE_READS_IN_FLIGHT = 8;

/** #185: the same envelope object comes back from the caller's evidence cache on every tick; its
 * digest is computed once, not on every re-fold. */
const digests = new WeakMap<EvidenceContent, Promise<string>>();
function contentDigest(evidence: EvidenceContent): Promise<string> {
  let pending = digests.get(evidence);
  if (pending === undefined) {
    pending = digestOf(new TextEncoder().encode(evidence.content).buffer, globalThis.crypto.subtle).then(rawHash);
    digests.set(evidence, pending);
  }
  return pending;
}

/** Reads the sealed `task.*` envelopes of a run and folds them (#391). An envelope whose hash does
 * not match its record, whose type is not the recorded kind, or whose signer is not the actor
 * that recorded it is skipped: the Runtime refuses those, and an old log must not draw them. */
/** The verified `task.*` records themselves, in sequence order, each with its `occurredAt`: the
 * Graph tab's lanes timeline needs when each one happened, which the fold does not keep. */
export async function readTaskEventRecords({ executionId, events, readEvidence }: ReadClaudeTasksOptions): Promise<TaskEventRecord[]> {
  const ordered = [...events].sort((a, b) => a.sequence - b.sequence).filter(isTaskEventSignal);
  const read = async (event: RuntimeEvent): Promise<TaskEventRecord | null> => {
    const payload = record(event.payload);
    const kind = payload?.kind as string;
    const seq = sequence(event.sequence);
    const evidenceId = event.evidenceRefs.length === 1 ? event.evidenceRefs[0] : null;
    const envelopeHash = text(payload?.envelopeSha256, 128);
    if (seq === null || evidenceId === null || envelopeHash === null || typeof event.actorId !== "string") return null;
    let evidence: EvidenceContent;
    try { evidence = await readEvidence(executionId, evidenceId); } catch { return null; }
    if (evidence.evidenceId !== evidenceId) return null;
    const computed = await contentDigest(evidence);
    if (rawHash(envelopeHash) !== computed || rawHash(evidence.contentSha256) !== computed) return null;
    const envelope = json(evidence);
    if (envelope?.type !== kind || record(envelope?.source)?.id !== event.actorId || typeof envelope?.description !== "string") return null;
    const parsed = parseTaskEvent(kind, event.actorId, envelope.description);
    return parsed === null ? null : { ...parsed, sequence: seq, occurredAt: event.occurredAt };
  };
  // #185: a gh-team-sized run has ~1,250 envelopes, and reading them one round trip at a time
  // kept the Team tab empty for minutes. A few reads stay in flight at once; each result keeps
  // its position, so the fold still sees the records in sequence order.
  const results: (TaskEventRecord | null)[] = new Array(ordered.length).fill(null);
  let next = 0;
  const worker = async () => {
    while (next < ordered.length) {
      const index = next++;
      results[index] = await read(ordered[index]);
    }
  };
  await Promise.all(Array.from({ length: Math.min(EVIDENCE_READS_IN_FLIGHT, ordered.length) }, worker));
  return results.filter((entry): entry is TaskEventRecord => entry !== null);
}
