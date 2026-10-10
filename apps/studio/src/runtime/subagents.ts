import type { RuntimeEvent } from "./types";

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

export function isSubagentLifecycleSignal(event: RuntimeEvent): boolean {
  if (event.kind !== "signal_recorded") return false;
  const kind = record(event.payload)?.kind;
  return kind === "agent_subagent_started" || kind === "agent_subagent_stopped";
}
