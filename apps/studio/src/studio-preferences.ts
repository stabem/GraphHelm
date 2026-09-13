/** Browser-only display preferences. Runtime credentials never enter this module. */

export const MAX_PROJECT_NAME_LENGTH = 120;
export const MAX_REMOVED_RUNS = 2000;
const MAX_ID_LENGTH = 256;
const NAME_PREFIX = "graphhelm.studio.project-name.";
const REMOVED_PREFIX = "graphhelm.studio.removed-runs.";

function key(prefix: string, runtimeKey: string): string {
  return `${prefix}${encodeURIComponent(runtimeKey.slice(0, MAX_ID_LENGTH))}`;
}

function validId(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= MAX_ID_LENGTH && !/[\u0000-\u001f\u007f]/.test(value);
}

export function validProjectName(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const name = value.trim();
  if (name.length === 0 || name.length > MAX_PROJECT_NAME_LENGTH || /[\u0000-\u001f\u007f]/.test(name)) return null;
  return name;
}

export function loadProjectName(runtimeKey: string, fallback: string): string {
  const safeFallback = validProjectName(fallback) ?? "this runtime";
  try {
    const stored = window.localStorage.getItem(key(NAME_PREFIX, runtimeKey));
    return validProjectName(stored) ?? safeFallback;
  } catch {
    return safeFallback;
  }
}

export function saveProjectName(runtimeKey: string, value: string): boolean {
  const name = validProjectName(value);
  if (name === null) return false;
  try {
    window.localStorage.setItem(key(NAME_PREFIX, runtimeKey), name);
    return true;
  } catch {
    return false;
  }
}

export function loadRemovedRuns(runtimeKey: string): string[] {
  try {
    const raw = window.localStorage.getItem(key(REMOVED_PREFIX, runtimeKey));
    if (raw === null || raw.length > MAX_REMOVED_RUNS * (MAX_ID_LENGTH + 4)) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return [...new Set(parsed.filter(validId))].slice(0, MAX_REMOVED_RUNS);
  } catch {
    return [];
  }
}

export function saveRemovedRuns(runtimeKey: string, ids: string[]): boolean {
  const safe = [...new Set(ids.filter(validId))];
  if (safe.length > MAX_REMOVED_RUNS) return false;
  try {
    window.localStorage.setItem(key(REMOVED_PREFIX, runtimeKey), JSON.stringify(safe));
    return true;
  } catch {
    return false;
  }
}
