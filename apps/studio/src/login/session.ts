/**
 * Login by ticket. `graphhelm studio open` (or the dev server's printed URL) opens the page with
 * `?session=<ticket or nonce>` and, for a project that joined this Studio's port, `&project=<id>`;
 * the dev server trades it once for that project's Runtime token and a stable key.
 *
 * #1800: one Studio port serves several projects, each with its own token. The page keeps one
 * stable key per project in sessionStorage, so it dies with the tab (the #1264 choice): a stable
 * key buys that project's Runtime token, so it is never kept at rest past the tab. A reload of this
 * tab may switch to any project it opened; a key the dev server no longer accepts is dropped.
 * Tokens are kept only in memory and no credential of one project is ever sent for another.
 */
export interface Session {
  /** The project's id, or `"first"` for the project the dev server was started for without one. */
  key: string;
  token: string;
  project: string | null;
  /** Where this project's Runtime is reached: `""` for `/v1`, `/p/<id>` for a joined project. */
  base: string;
  /** #1833: the project's folder for display (`~` for home), or null when the dev server cannot say. */
  folder: string | null;
  /** This project's stable key, in memory: it proves the owner to `/__studio/add`. */
  sessionKey: string | null;
  /** #1901: the run the project's `.graphhelm/team.json` names, or null when it names none. */
  teamRun: string | null;
}

export const SESSION_KEYS_STORAGE = "graphhelm.studio-next.sessionKeys";

const PROJECT_KEY = /^(?:first|[0-9a-f]{64})$/;
const SESSION_KEY = /^[0-9a-f]{32,128}$/;

type KeyStore = Pick<Storage, "getItem" | "setItem"> | null;

function browserStore(): KeyStore {
  try {
    return globalThis.sessionStorage ?? null;
  } catch {
    return null;
  }
}

function readKeys(store: KeyStore): Record<string, string> {
  try {
    const parsed = JSON.parse(store?.getItem(SESSION_KEYS_STORAGE) ?? "{}") as unknown;
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(([project, key]) => PROJECT_KEY.test(project) && typeof key === "string" && SESSION_KEY.test(key)).slice(0, 32),
    ) as Record<string, string>;
  } catch {
    return {};
  }
}

/** One request per presented secret and project, shared: a one-shot ticket must not be spent twice
 * by a double mount. */
const inFlight = new Map<string, Promise<unknown>>();

function ask(nonce: string, project: string | null): Promise<unknown> {
  const query = `nonce=${encodeURIComponent(nonce)}${project === null || project === "first" ? "" : `&project=${project}`}`;
  const pending = inFlight.get(query);
  if (pending) return pending;
  const request = fetch(`/__studio/session?${query}`, { headers: { Accept: "application/json" } })
    .then((response) => (response.ok ? response.json() : null))
    .catch(() => null);
  inFlight.set(query, request);
  return request;
}

type Reply = { token?: unknown; folder?: unknown; project?: unknown; projectId?: unknown; base?: unknown; sessionKey?: unknown; teamRun?: unknown } | null;

const RUN_ID = /^[A-Za-z0-9._:-]{1,200}$/;

function sessionOf(payload: Reply): { session: Session; sessionKey: string | null } | null {
  if (!payload || typeof payload.token !== "string" || payload.token.trim().length === 0 || payload.token.length > 512) return null;
  const key = typeof payload.projectId === "string" && /^[0-9a-f]{64}$/.test(payload.projectId) ? payload.projectId : "first";
  const base = typeof payload.base === "string" && /^(?:\/p\/[0-9a-f]{64})?$/.test(payload.base) ? payload.base : "";
  const sessionKey = typeof payload.sessionKey === "string" && SESSION_KEY.test(payload.sessionKey) ? payload.sessionKey : null;
  return {
    session: {
      key,
      token: payload.token.trim(),
      project: typeof payload.project === "string" && payload.project.length <= 120 ? payload.project : null,
      base,
      folder: typeof payload.folder === "string" && payload.folder.length <= 4096 ? payload.folder : null,
      sessionKey,
      teamRun: typeof payload.teamRun === "string" && RUN_ID.test(payload.teamRun) ? payload.teamRun : null,
    },
    sessionKey,
  };
}

/** Every project id this Studio port serves (`GET /__studio/identity`, hashes only): which one
 * the dev server was started for, and all joined ones. `null` when the route does not answer. */
export async function servedProjects(): Promise<{ first: string | null; all: string[] } | null> {
  try {
    const response = await fetch("/__studio/identity", { headers: { Accept: "application/json" } });
    if (!response.ok) return null;
    const body = (await response.json()) as { projectId?: unknown; projectIds?: unknown };
    const hash = /^[0-9a-f]{64}$/;
    const all = Array.isArray(body.projectIds) ? body.projectIds.filter((id): id is string => typeof id === "string" && hash.test(id)).slice(0, 32) : [];
    return { first: typeof body.projectId === "string" && hash.test(body.projectId) ? body.projectId : null, all };
  } catch {
    return null;
  }
}

/** Every project this tab may open here, the one the URL presented first; `null` when none. */
export async function login(search: string, store: KeyStore = browserStore()): Promise<Session[] | null> {
  const query = new URLSearchParams(search);
  const presented = query.get("session");
  const wanted = query.get("project");
  const attempts: [string, string | null][] = [];
  if (presented) attempts.push([presented, wanted !== null && PROJECT_KEY.test(wanted) ? wanted : null]);
  for (const [project, key] of Object.entries(readKeys(store))) attempts.push([key, project]);
  const sessions: Session[] = [];
  const kept: Record<string, string> = {};
  for (const [nonce, project] of attempts) {
    const reply = sessionOf((await ask(nonce, project)) as Reply);
    if (reply === null || sessions.some((known) => known.key === reply.session.key)) continue;
    sessions.push(reply.session);
    if (reply.sessionKey !== null) kept[reply.session.key] = reply.sessionKey;
  }
  try { store?.setItem(SESSION_KEYS_STORAGE, JSON.stringify(kept)); } catch { /* a reload will ask again */ }
  return sessions.length === 0 ? null : sessions;
}

/** #1897: the Runtime refused this project's token, so its stored key is dropped: a reload asks for
 * a new ticket instead of reading OFFLINE forever. Other projects' keys stay. */
export function forgetKey(project: string, store: KeyStore = browserStore()): void {
  try {
    const { [project]: _dropped, ...rest } = readKeys(store);
    store?.setItem(SESSION_KEYS_STORAGE, JSON.stringify(rest));
  } catch { /* nothing stored to drop */ }
}

/** Why `/__studio/add` refused a folder, in the dev server's closed list. */
export type AddRefusal = "not_a_project" | "runtime_not_running" | "refused";

/** #1833 "Add project folder": registers a folder on this Studio port, proved by the stable key of a
 * project this tab already holds AND the new project's own one-shot ticket. On success the new project's key joins this tab's keys (session
 * storage, as at login: it dies with the tab) so the next login opens it. */
export async function addFolder(path: string, ownerKey: string, ticket: string, store: KeyStore = browserStore()): Promise<{ projectId: string } | { refused: AddRefusal }> {
  type AddReply = { ok?: unknown; reason?: unknown; projectId?: unknown; sessionKey?: unknown } | null;
  let reply = null as AddReply;
  try {
    const response = await fetch("/__studio/add", {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-GraphHelm-Studio-Key": ownerKey },
      body: JSON.stringify({ path, ticket }),
    });
    reply = (await response.json()) as AddReply;
  } catch { /* answered below as refused */ }
  if (reply?.ok === true && typeof reply.projectId === "string" && /^[0-9a-f]{64}$/.test(reply.projectId)
    && typeof reply.sessionKey === "string" && SESSION_KEY.test(reply.sessionKey)) {
    try { store?.setItem(SESSION_KEYS_STORAGE, JSON.stringify({ ...readKeys(store), [reply.projectId]: reply.sessionKey })); } catch { /* a ticket can still open it */ }
    return { projectId: reply.projectId };
  }
  return { refused: reply?.reason === "not_a_project" || reply?.reason === "runtime_not_running" ? reply.reason : "refused" };
}
