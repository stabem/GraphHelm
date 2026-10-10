import type { RuntimeEvent } from "./types";

/**
 * The Keel/JPD record chain for one node (docs/keel/RECORDS.md): journey -> obligation -> card ->
 * proof, each an ordinary `signal_recorded` whose `kind` names the step. Read from the event
 * payload alone (kind, severity, source), so it needs no sealed evidence and shows a skipped step
 * the moment the thread is opened. `replyTo` lives in the sealed envelope and is not checked here:
 * this answers "was each step recorded for this node", not "do the replies link up".
 */
export const KEEL_STEPS = [
  { kind: "jpd.journey", label: "Journey" },
  { kind: "jpd.obligation", label: "Obligation" },
  { kind: "keel.card", label: "Card" },
  { kind: "keel.proof", label: "Proof" },
] as const;

export type KeelStepKind = (typeof KEEL_STEPS)[number]["kind"];
export type KeelStepState = "recorded" | "missing" | "green" | "red";

export interface KeelChain {
  steps: { kind: KeelStepKind; label: string; state: KeelStepState; count: number }[];
  /** A card or proof exists, so this node did Keel work; false means nothing to show. */
  active: boolean;
  /** How many times a Keel lock (Stop or PreToolUse hook) refused this node's agent. */
  blocked: number;
  /** Steps missing before the last recorded one, plus a missing or red proof once a card exists. */
  gaps: string[];
}

function keelKind(event: RuntimeEvent, nodeId: string): { kind: KeelStepKind; severity: string | null } | null {
  if (event.kind !== "signal_recorded") return null;
  const payload = event.payload as Record<string, unknown> | null;
  if (payload === null || typeof payload !== "object") return null;
  if (payload.sourceKind !== "node" || payload.sourceId !== nodeId) return null;
  const kind = KEEL_STEPS.find((step) => step.kind === payload.kind)?.kind;
  if (kind === undefined) return null;
  return { kind, severity: typeof payload.severity === "string" ? payload.severity : null };
}

export function keelChain(events: RuntimeEvent[], nodeId: string): KeelChain {
  const counts = new Map<KeelStepKind, number>();
  let lastProofSeverity: string | null = null;
  let blocked = 0;
  for (const event of events) {
    const payload = event.payload as Record<string, unknown> | null;
    if (event.kind === "signal_recorded" && payload?.kind === "keel.blocked" && payload.sourceKind === "node" && payload.sourceId === nodeId) blocked += 1;
    const found = keelKind(event, nodeId);
    if (found === null) continue;
    counts.set(found.kind, (counts.get(found.kind) ?? 0) + 1);
    // A later green proof answers the same card and never erases the red one (RECORDS.md), so
    // the chain reads the latest proof while the thread keeps both.
    if (found.kind === "keel.proof") lastProofSeverity = found.severity;
  }
  const steps = KEEL_STEPS.map(({ kind, label }) => {
    const count = counts.get(kind) ?? 0;
    let state: KeelStepState = count > 0 ? "recorded" : "missing";
    if (kind === "keel.proof" && count > 0) {
      state = lastProofSeverity === "high" || lastProofSeverity === "critical" ? "red" : "green";
    }
    return { kind, label, state, count };
  });
  const card = counts.has("keel.card");
  const active = card || counts.has("keel.proof") || blocked > 0;
  const gaps: string[] = [];
  if (active) {
    const lastIndex = Math.max(...steps.map((step, index) => (step.count > 0 ? index : -1)));
    for (const [index, step] of steps.entries()) {
      if (step.kind === "keel.proof") continue;
      // Journey and obligation are optional for a change with no journey (RECORDS.md), but a
      // card that answers an obligation needs both; a missing step BEFORE a recorded one is a skip.
      if (step.state === "missing" && index < lastIndex && (step.kind === "keel.card" || counts.has("jpd.journey") || counts.has("jpd.obligation"))) {
        gaps.push(`${step.label} was skipped.`);
      }
    }
    const proof = steps[3];
    if (card && proof.state === "missing") gaps.push("Card has no proof yet.");
    if (proof.state === "red") gaps.push("Latest proof failed.");
    if (blocked > 0) gaps.push(`A Keel lock stopped this agent ${blocked === 1 ? "once" : `${blocked} times`}.`);
  }
  return { steps, active, blocked, gaps };
}
