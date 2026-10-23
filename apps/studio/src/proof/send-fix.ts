import type { JourneyFlow, JourneyRun } from "../runtime/client";
import type { Step } from "./steps";

/** #1919: the Runtime admits an ask of at most 2000 characters (`task.request_recorded`). */
export const ASK_MAX = 2000;
/** The note the owner may add, kept short so the facts always fit. */
export const NOTE_MAX = 500;

/** Whether a finished run offers Send to fix: it failed or drifted. A run still playing, one that
 * could not finish, or a pass offers nothing. */
export function offersFix(run: JourneyRun | null): boolean {
  return run?.state === "ready" && (run.result === "fail" || run.result === "drift");
}

/** The first step that failed, else the first that drifted; `null` when none did. */
export function failingStep(steps: readonly Step[]): Step | null {
  return steps.find((step) => step.status === "failed") ?? steps.find((step) => step.status === "drift") ?? null;
}

/** The request's id, one per journey and run: the same run always names the same request, so
 * pressing again (or after a reload) finds the request already recorded, never a second one. */
export function fixRequestId(flowId: string, run: JourneyRun): string {
  const when = (run.ranAt ?? run.startedAt ?? "unknown").toLowerCase().replace(/[^a-z0-9]+/g, "");
  const flow = flowId.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 64);
  return `fix-${flow}-${when.slice(0, 24)}`;
}

const tail = (text: string, max: number) => (text.length > max ? `…${text.slice(text.length - max)}` : text);

/** The ask an agent starts from: the journey and its flow file, the run, the failing step with its
 * reason, the command's exit and stderr (a command step) or the frame to open (a screen step), and
 * the owner's note. Always at most `ASK_MAX` characters. */
export function fixAsk(flow: JourneyFlow, run: JourneyRun, steps: readonly Step[], note: string): string {
  const step = failingStep(steps);
  const lines = [
    `Fix the journey ${flow.id} (${flow.name || flow.title || flow.id}): its last run ${run.result === "drift" ? "drifted" : "failed"}.`,
    `Flow file: .graphhelm/journeys/${flow.id}.journey.yaml`,
    `Run: ${run.kind ?? "preview"} at ${run.ranAt ?? run.startedAt ?? "an unrecorded time"}`,
  ];
  if (step === null) {
    lines.push(`Failing step: none recorded${run.reason ? `; the run says: ${run.reason}` : ""}`);
  } else {
    const where = (Object.entries(step.viewports) as [string, Step["status"]][])
      .filter(([, status]) => status === "failed" || status === "drift").map(([viewport]) => viewport);
    lines.push(`Failing step: ${step.n} "${step.label}" (screen ${step.screen}${step.edge ? `, edge ${step.edge}` : ""})${where.length > 0 ? ` at ${where.join(" and ")}` : ""}`);
    if (step.reason) lines.push(`Reason: ${tail(step.reason, 300)}`);
    const last = step.outputs[step.outputs.length - 1];
    if (step.cli && last !== undefined) {
      lines.push(`Command: ${last.argv}`, `Exit: ${last.exit ?? "none"}`);
      if (last.stderr.trim()) lines.push(`Stderr (tail): ${tail(last.stderr.trim(), 400)}`);
    } else {
      lines.push(`Frame: GET /v1/journey-flows/${flow.id}/preview, screen ${step.screen}${step.frame ? "" : " (no frame kept)"}`);
    }
  }
  const owner = note.trim().slice(0, NOTE_MAX);
  if (owner) lines.push(`Owner's note: ${owner}`);
  const ask = lines.join("\n");
  return ask.length > ASK_MAX ? `${ask.slice(0, ASK_MAX - 1)}…` : ask;
}

/** The request's short title: "Fix <journey name>". */
export function fixTitle(flow: JourneyFlow): string {
  return `Fix ${flow.name || flow.title || flow.id}`.slice(0, 120);
}

/** The `POST /v1/executions/{id}/signal` body of the owner's `task.request_recorded` (#1792), the
 * shape tools/task-record/task_record.py sends: the owner names itself in `recordedBy`. */
export function fixBody(executionId: string, requestId: string, title: string, ask: string, at: string, owner: string): Record<string, unknown> {
  const document = { schema: "graphhelm-task-event-v1", taskId: `request-${requestId}`, revision: 1, at, requestId, ask, title, recordedBy: owner };
  return {
    signal: {
      id: requestId, type: "task.request_recorded", source: { type: "user", id: owner }, severity: "low",
      emittedAt: at, evidence: [executionId], description: JSON.stringify(document),
    },
  };
}
