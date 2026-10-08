import { digestOf } from "./customs";
import type { EvidenceContent, RuntimeEvent } from "./types";

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

export type TaskEventKind = "task.claimed" | "task.pr_opened" | "task.review_assigned" | "task.review_verdict" | "task.merged";

export interface TaskEventRecord {
  kind: TaskEventKind;
  actorId: string;
  sequence: number;
  taskId: string;
  issue?: number;
  pr?: number;
  lane?: string;
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
      const branch = text(document.branch, 256);
      return issue !== null && lane === actorId && branch !== null ? { ...base, kind, issue, lane, branch, ...(repo === null ? {} : { repo }) } : null;
    }
    case "task.pr_opened": {
      const repo = repository(document.repo);
      if (repo === false) return null;
      const pr = count(document.pr);
      const headSha = sha(document.headSha);
      const lane = text(document.lane, 128);
      const journeys = Array.isArray(document.journeys) && document.journeys.every((id) => taskIdentity(id) !== null) ? document.journeys as string[] : null;
      return pr !== null && headSha !== null && lane === actorId && journeys !== null ? { ...base, kind, pr, headSha, lane, journeys, ...(repo === null ? {} : { repo }) } : null;
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
    default:
      return null;
  }
}

export type TaskStep = "implement" | "review" | "merge" | "merged";

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
  headSha: string | null;
  journeys: string[];
  /** The step that is lit. */
  step: TaskStep;
  /** A BLOCK that no verdict on a newer head has answered: the red edge into the next step. */
  blockedBy: { reviewer: string; headSha: string; commentUrl: string } | null;
  reviewers: string[];
  mergeSha: string | null;
  /** `https://github.com/<owner>/<repo>`, read off a verdict's comment URL; links need it. */
  repoUrl: string | null;
  /** #457: verdicts that do not speak for the current head, newest last, each with its reason: a head
   * no `pr_opened` has named yet (applied if that `pr_opened` arrives later, e.g. a back-fill), or an
   * older recorded head that a newer push superseded. Dropping them drew "no review" where one exists. */
  strayVerdicts: StrayVerdict[];
  /** Every head a `pr_opened` named, in order. */
  recordedHeads: string[];
  lastSequence: number;
}

function applyVerdict(state: TaskState, event: TaskEventRecord): void {
  if (event.reviewer && !state.reviewers.includes(event.reviewer)) state.reviewers.push(event.reviewer);
  // Old logs carry no `repo`: a verdict's comment URL still names the repository.
  state.repoUrl ??= /^(https:\/\/github\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+)\//.exec(event.commentUrl ?? "")?.[1] ?? state.repoUrl;
  if (event.verdict === "BLOCK") {
    state.blockedBy = { reviewer: event.reviewer ?? "", headSha: event.headSha ?? "", commentUrl: event.commentUrl ?? "" };
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
function sliceFor(slices: TaskState[], event: TaskEventRecord): TaskState {
  const group = slices.filter((slice) => slice.taskId === event.taskId);
  const open = (slice: TaskState) => slice.pr === null && slice.step !== "merged"
    && (slice.lane === null || event.lane === undefined || slice.lane === event.lane);
  const add = () => {
    const slice: TaskState = {
      key: "", taskId: event.taskId, branch: null, issue: null, pr: event.kind === "task.claimed" ? null : event.pr ?? null,
      lane: null, headSha: null, journeys: [], step: "implement", blockedBy: null, reviewers: [], mergeSha: null,
      repoUrl: null, strayVerdicts: [], recordedHeads: [], lastSequence: 0,
    };
    slices.push(slice);
    return slice;
  };
  // A claim names its branch: the same branch is the same slice (a re-claim), any other opens one.
  if (event.kind === "task.claimed") {
    return group.find((slice) => slice.branch !== null && slice.branch === event.branch) ?? add();
  }
  const own = event.pr === undefined ? undefined : group.find((slice) => slice.pr === event.pr);
  if (own !== undefined) return own;
  // A PR no slice holds yet joins the oldest open claim of its task (for pr_opened, the same
  // lane's): PRs open in the order their slices were claimed. The claim's slice becomes that PR's
  // slice. A merge recorded with no pr_opened (#449's own log) still lands on the issue's claim
  // instead of opening a second graph.
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
    switch (event.kind) {
      case "task.claimed":
        state.branch = event.branch ?? state.branch;
        state.repoUrl = event.repo !== undefined ? `https://github.com/${event.repo}` : state.repoUrl;
        state.issue = event.issue ?? state.issue;
        state.lane = event.lane ?? state.lane;
        break;
      case "task.pr_opened":
        state.repoUrl = event.repo !== undefined ? `https://github.com/${event.repo}` : state.repoUrl;
        state.pr = event.pr ?? state.pr;
        state.lane = event.lane ?? state.lane;
        state.headSha = event.headSha ?? state.headSha;
        state.journeys = event.journeys ?? state.journeys;
        state.step = "review";
        if (event.headSha !== undefined && !state.recordedHeads.includes(event.headSha)) state.recordedHeads.push(event.headSha);
        // A verdict that arrived before this head's pr_opened (a back-fill) now speaks for it.
        for (const stray of state.strayVerdicts.filter((entry) => entry.reason === "unrecorded" && entry.headSha === state.headSha)) {
          state.strayVerdicts.splice(state.strayVerdicts.indexOf(stray), 1);
          if (stray.reason === "unrecorded") applyVerdict(state, stray.record);
        }
        break;
      case "task.review_assigned":
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
      case "task.merged":
        state.pr = event.pr ?? state.pr;
        state.mergeSha = event.mergeSha ?? null;
        state.blockedBy = null;
        state.step = "merged";
        break;
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

/** Reads the sealed `task.*` envelopes of a run and folds them (#391). An envelope whose hash does
 * not match its record, whose type is not the recorded kind, or whose signer is not the actor
 * that recorded it is skipped: the Runtime refuses those, and an old log must not draw them. */
export async function readTaskEvents({ executionId, events, readEvidence }: ReadClaudeTasksOptions): Promise<TaskState[]> {
  const records: TaskEventRecord[] = [];
  for (const event of [...events].sort((a, b) => a.sequence - b.sequence)) {
    if (!isTaskEventSignal(event)) continue;
    const payload = record(event.payload);
    const kind = payload?.kind as string;
    const seq = sequence(event.sequence);
    const evidenceId = event.evidenceRefs.length === 1 ? event.evidenceRefs[0] : null;
    const envelopeHash = text(payload?.envelopeSha256, 128);
    if (seq === null || evidenceId === null || envelopeHash === null || typeof event.actorId !== "string") continue;
    let evidence: EvidenceContent;
    try { evidence = await readEvidence(executionId, evidenceId); } catch { continue; }
    if (evidence.evidenceId !== evidenceId) continue;
    const computed = rawHash(await digestOf(new TextEncoder().encode(evidence.content).buffer, globalThis.crypto.subtle));
    if (rawHash(envelopeHash) !== computed || rawHash(evidence.contentSha256) !== computed) continue;
    const envelope = json(evidence);
    if (envelope?.type !== kind || record(envelope?.source)?.id !== event.actorId || typeof envelope?.description !== "string") continue;
    const parsed = parseTaskEvent(kind, event.actorId, envelope.description);
    if (parsed !== null) records.push({ ...parsed, sequence: seq });
  }
  return foldTaskEvents(records);
}
