/**
 * #1767: the other projects one Studio serves, for the rail's project switcher.
 *
 * The dev server lists them at `/__studio/identity` by id only (a SHA-256 of the folder), so the
 * route carries no path and no secret. A project's name is what its own session told this tab
 * when it opened; a project this tab has not opened yet is shown by a short id.
 *
 * Switching is a navigation, not a second client inside this page: the next page asks for that
 * project's session with that project's own remembered key, so one project's token never serves
 * another's calls.
 */

import { PROJECT_ID, type KeyStore } from "./session";

const IDENTITY_PATH = "/__studio/identity";
const NAME_STORAGE = "graphhelm.studio.projectName";
const MAX_PROJECTS = 50;

export interface StudioProjectEntry {
  id: string;
  name: string;
  /** Where this project opens: the Studio's own project at the root, any other by `?project=`. */
  href: string;
}

function browserStore(): KeyStore {
  try {
    return globalThis.sessionStorage ?? null;
  } catch {
    return null;
  }
}

/** Remembers what to call a project in this tab, once its session named it. */
export function rememberProjectName(id: string, name: string | null, store: KeyStore = browserStore()): void {
  if (!PROJECT_ID.test(id) || name === null || name.length === 0 || name.length > 120) return;
  try { store?.setItem(`${NAME_STORAGE}.${id}`, name); } catch { /* storage refused: the short id is shown */ }
}

function nameOf(id: string, store: KeyStore): string {
  let saved: string | null = null;
  try { saved = store?.getItem(`${NAME_STORAGE}.${id}`) ?? null; } catch { saved = null; }
  return saved !== null && saved.length > 0 && saved.length <= 120 ? saved : `project ${id.slice(0, 8)}`;
}

/**
 * Every project the Studio serves except `current` (`null`: the Studio's own project), which is
 * remembered as `currentName`. `null` when the dev server has no identity
 * route (a built bundle) or answers something else: then there is nothing to switch to.
 */
export async function otherStudioProjects(
  current: string | null,
  currentName: string | null = null,
  fetchImpl?: typeof fetch,
  store: KeyStore = browserStore(),
): Promise<StudioProjectEntry[] | null> {
  let payload: unknown;
  try {
    const response = await (fetchImpl ?? globalThis.fetch)(IDENTITY_PATH, { headers: { Accept: "application/json" } });
    if (!response.ok) return null;
    payload = await response.json();
  } catch {
    return null;
  }
  if (payload === null || typeof payload !== "object") return null;
  const first = (payload as { projectId?: unknown }).projectId;
  const ids = (payload as { projectIds?: unknown }).projectIds;
  if (!Array.isArray(ids)) return null;
  const own = typeof first === "string" && PROJECT_ID.test(first) ? first : null;
  // The Studio's own project learns its id only here; name it so a joined tab can show it.
  rememberProjectName(current ?? own ?? "", currentName, store);
  return ids
    .filter((id): id is string => typeof id === "string" && PROJECT_ID.test(id))
    .filter((id, index, all) => all.indexOf(id) === index)
    .slice(0, MAX_PROJECTS)
    .filter((id) => id !== (current ?? own))
    .map((id) => ({ id, name: nameOf(id, store), href: id === own ? "/" : `/?project=${id}` }));
}
