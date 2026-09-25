/** The three shared readings. Kept apart from any component so the rail, the board and the panel
 * cannot drift into spelling a state or an instant three different ways. */

/** Instants render short: this is a console, and a full date on every line is noise. The full
 * value stays in the element's `dateTime`, so a reader who needs it can still get it.
 *
 * TODAY renders as time alone; any other day carries its day — "needs you · 06:16 PM" read the
 * same whether the run moved nine hours ago or thirty-three, and triage across a day boundary
 * was guesswork (round-4, 3am on-call). */
export function clock(value?: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return value;
  const time = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(date);
  if (date.toDateString() === new Date().toDateString()) return time;
  const day = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(date);
  return `${day} · ${time}`;
}

export function fullInstant(value?: string | null): string {
  if (!value) return "Not recorded";
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return value;
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "medium" }).format(date);
}

/** Wire vocabulary is snake_case; a person reads words. The value itself is never translated -
 * `waiting_input` becomes "waiting input", never "waiting for input", because the reader has to
 * be able to match what they see against the API. */
export function readable(value: string): string {
  return value.replaceAll("_", " ");
}

/**
 * A stable hue for an actor id, so every persona and agent wears one colour everywhere - the
 * roster chip, the thread avatar, the rail. Derived, never assigned: nobody maintains a colour
 * table, two views can never disagree about who is teal, and a persona born a minute ago has a
 * colour before any human has seen it. FNV-1a because the input is short and adversarially
 * boring; this is identity, not cryptography.
 */
export function hueOf(id: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < id.length; i += 1) {
    hash ^= id.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0) % 360;
}

/** The letter an avatar wears. The first alphanumeric, so "guarda-do-registo" is G, not "-". */
export function initialOf(id: string): string {
  const match = id.match(/[a-z0-9]/i);
  return (match?.[0] ?? "?").toUpperCase();
}

/** A relative age for the canvas, where "when" matters less than "how long ago". The full
 * instant stays wherever a `dateTime` or `title` carries it. */
export function ago(value?: string | null): string {
  if (!value) return "\u2014";
  const then = new Date(value).valueOf();
  if (Number.isNaN(then)) return "\u2014";
  const minutes = Math.max(0, Math.round((Date.now() - then) / 60000));
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.round(hours / 24)}d`;
}

export type Verdict = "needs" | "calm" | "unknown";

export function verdictOf(attention: string): { key: Verdict; label: string } {
  if (attention === "needs_you") return { key: "needs", label: "Needs you" };
  if (attention === "can_sleep") return { key: "calm", label: "Can sleep" };
  return { key: "unknown", label: "Unknown" };
}

/** The sixteen lifecycle states in the Runtime's own declaration order. Rendered in full, zeroes
 * included: an omitted bucket reads as "no such problem", which is how a red run looks calm. */
export const LIFECYCLE_STATES = [
  "draft",
  "ghost",
  "linting",
  "ready",
  "queued",
  "running",
  "waiting_input",
  "waiting_capacity",
  "paused",
  "blocked",
  "succeeded",
  "failed",
  "waived",
  "skipped",
  "cancelled",
  "invalidated",
] as const;

/** The states that mean a person is needed. They take the signal colour, and nothing else does. */
export function isAlarming(state: string): boolean {
  return state === "blocked" || state === "failed" || state === "waiting_input" || state === "waiting_capacity";
}

/** The id shape `graph/draft.ts` mints for a task the Studio starts: `run-` and a UUID tail.
 * A hand-named run (`demo-deploy`) is named by its id already; only a generated one needs a
 * name from somewhere else. */
export function isGeneratedRunId(id: string): boolean {
  return /^run-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id);
}

/** How much of an objective a rail row or a header carries before it ellipsises. */
export const RUN_LABEL_LENGTH = 72;

/**
 * What to call a run (#1077, #1083 F7). ANY run whose briefing carries an objective is named by
 * it - the sentence it was started for, read back from the store - truncated, with the id kept
 * as the address wherever the label stands. A run started from the CLI or over HTTP holds an
 * objective exactly like a composer-started `run-<uuid>`, and naming it by its id alone hid that
 * sentence until the run was opened (#1083 F7). A run whose briefing carries no objective has
 * nothing truer than its id, and is named by it.
 *
 * The graph document's `name` is deliberately NOT a fallback: the draft graph's name is the
 * placeholder "New task", and two runs called "New task" are the defect this exists to close.
 */
export function runLabel(
  id: string,
  briefing: { objective: string | null; name?: string | null } | null | undefined,
): string {
  const objective = briefing?.objective?.trim() ?? "";
  if (objective.length === 0) return id;
  const oneLine = objective.replace(/\s+/g, " ");
  return oneLine.length <= RUN_LABEL_LENGTH ? oneLine : `${oneLine.slice(0, RUN_LABEL_LENGTH - 1).trimEnd()}\u2026`;
}
