/* #502 (owner, on the Team tab): "falta uma barra de progresso no node pra saber um timer pra saber
 * a quanto tempo ta la". Each task slice remembers when it entered its current step and how long it
 * spent in the steps it left, from the Runtime's own append time of each record (`occurredAt`, which
 * a lane cannot write), never from the lane's `at`. Pure functions: the fold calls `clockStep` after
 * each record; the view compares the time in step with the run's typical time for that step. */

export type TimedStep = "implement" | "review" | "merge";

export interface StepClock {
  /** When the slice entered its current step (ISO), or null when no record carried a time. */
  since: string | null;
  /** Milliseconds spent in each step the slice has left, summed over every visit. */
  spent: Partial<Record<TimedStep, number>>;
}

export function emptyClock(): StepClock {
  return { since: null, spent: {} };
}

function millis(at: string | null | undefined): number | null {
  if (typeof at !== "string") return null;
  const value = Date.parse(at);
  return Number.isFinite(value) ? value : null;
}

/** Moves the clock when a record changes the slice's step. A record that leaves the step as it was
 * (an assignment, a stray verdict) only starts the clock when it had no start yet. */
export function clockStep(clock: StepClock, before: string, after: string, at: string | null | undefined): void {
  const now = millis(at);
  if (before === after) {
    if (clock.since === null && now !== null) clock.since = at!;
    return;
  }
  const since = millis(clock.since);
  if (since !== null && now !== null && now >= since && (before === "implement" || before === "review" || before === "merge")) {
    clock.spent[before] = (clock.spent[before] ?? 0) + (now - since);
  }
  clock.since = now === null ? null : at!;
}

/** The typical time of a step: the median over the slices that already left it in this run (the
 * merged ones), with how many samples back it. Fewer than `minimum` samples is no typical time:
 * the view then shows the timer alone, rather than a target it invented. */
export function typicalStep(clocks: StepClock[], step: TimedStep, minimum = 3): { ms: number; samples: number } | null {
  const samples = clocks.map((clock) => clock.spent[step]).filter((ms): ms is number => typeof ms === "number").sort((a, b) => a - b);
  if (samples.length < minimum) return null;
  const middle = Math.floor(samples.length / 2);
  const ms = samples.length % 2 === 1 ? samples[middle] : (samples[middle - 1] + samples[middle]) / 2;
  return { ms, samples: samples.length };
}

/** Green under the typical time, amber past it, red past twice it (a "maybe stuck" cue). */
export function pace(elapsedMs: number, typicalMs: number): "under" | "over" | "stuck" {
  return elapsedMs <= typicalMs ? "under" : elapsedMs <= 2 * typicalMs ? "over" : "stuck";
}

/** "45 s", "12 min", "3 h 05", "2 d 4 h": short enough for a node. */
export function duration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h} h ${String(m % 60).padStart(2, "0")}`;
  return `${Math.floor(h / 24)} d ${h % 24} h`;
}
