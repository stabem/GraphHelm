/** Project metadata bootstrapped through the server's HttpOnly browser session. */
export interface Session {
  key: string;
  project: string | null;
  base: string;
  folder: string | null;
  teamRun: string | null;
}
const PROJECT_KEY = /^[0-9a-f]{64}$/;
const RUN_ID = /^[A-Za-z0-9._:-]{1,200}$/;
const SESSION_TIMEOUT_MS = 30_000;
// Remove credentials left by older Studio versions; never read or reuse them.
export function clearLegacyCredentials(): void {
  try { globalThis.sessionStorage?.removeItem("graphhelm.studio-next.sessionKeys"); } catch { /* storage may be disabled */ }
}
export async function servedProjects(): Promise<{ first: string | null; all: string[] } | null> {
  try {
    const response = await fetch("/__studio/identity", {headers:{Accept:"application/json"},signal:AbortSignal.timeout(SESSION_TIMEOUT_MS)});
    if (!response.ok) return null;
    const body = await response.json() as {projectId?:unknown;projectIds?:unknown};
    return {first:typeof body.projectId === "string" && PROJECT_KEY.test(body.projectId) ? body.projectId : null,
      all:Array.isArray(body.projectIds) ? body.projectIds.filter((id):id is string=>typeof id === "string" && PROJECT_KEY.test(id)).slice(0,32) : []};
  } catch { return null; }
}
/** One bootstrap try. "retry" means nothing answered yet: a 503 (Runtime or project not ready) or no
 * answer at all. Anything else that fails (404 unknown project, bad body) is final: null. */
async function loginOnce(search: string): Promise<Session[] | null | "retry"> {
  clearLegacyCredentials();
  const wanted = new URLSearchParams(search).get("project");
  if (wanted !== null && !PROJECT_KEY.test(wanted)) return null;
  const served = await servedProjects();
  const ids = [...new Set([wanted ?? served?.first ?? "first", ...(served?.all ?? [])])];
  const sessions: Session[] = [];
  let notReady = false;
  for (const id of ids) {
    try {
      const response = await fetch(`/__studio/session${id === "first" ? "" : `?project=${id}`}`, {headers:{Accept:"application/json"},signal:AbortSignal.timeout(SESSION_TIMEOUT_MS)});
      if (!response.ok) { if (response.status === 503) notReady = true; else if (id === wanted) return null; continue; }
      // A body that is not JSON is an answer, not a missing one: final for the wanted project, never retried.
      let body: {ok?:unknown;projectId?:unknown;project?:unknown;base?:unknown;folder?:unknown;teamRun?:unknown};
      try { body = await response.json() as typeof body; } catch { if (id === wanted) return null; continue; }
      if (body.ok !== true) { if(id === wanted) return null; continue; }
      sessions.push({key:typeof body.projectId === "string" && PROJECT_KEY.test(body.projectId) ? body.projectId : "first",
        project:typeof body.project === "string" ? body.project : null,
        base:typeof body.base === "string" && /^(?:\/p\/[0-9a-f]{64})?$/.test(body.base) ? body.base : "",
        folder:typeof body.folder === "string" ? body.folder : null,
        teamRun:typeof body.teamRun === "string" && RUN_ID.test(body.teamRun) ? body.teamRun : null});
    } catch { notReady = true; }
  }
  return sessions.length ? sessions : notReady ? "retry" : null;
}
export async function login(search: string): Promise<Session[] | null> {
  const result = await loginOnce(search);
  return result === "retry" ? null : result;
}
/** #2696: waits for a Runtime that is not up yet. Retries only while nothing answered ("retry"),
 * at most `delays.length` more times; `onWaiting` fires once, on the first wait. */
export const LOGIN_RETRY_MS = [1_000, 2_000, 4_000, 8_000, 8_000];
export async function loginWaiting(search: string, onWaiting: () => void, isLive: () => boolean = () => true, delays: readonly number[] = LOGIN_RETRY_MS): Promise<Session[] | null> {
  let result = await loginOnce(search);
  for (const delay of delays) {
    if (result !== "retry" || !isLive()) break;
    onWaiting();
    await new Promise((resolve) => setTimeout(resolve, delay));
    if (!isLive()) return null;
    result = await loginOnce(search);
  }
  return result === "retry" ? null : result;
}
export type AddRefusal = "not_a_project" | "runtime_not_running" | "refused";
export async function addFolder(path: string): Promise<{projectId:string}|{refused:AddRefusal}> {
  try {
    const response = await fetch("/__studio/add",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({path}),signal:AbortSignal.timeout(SESSION_TIMEOUT_MS)});
    const body = await response.json() as {ok?:unknown;projectId?:unknown;reason?:unknown};
    if(response.ok && body.ok === true && typeof body.projectId === "string" && PROJECT_KEY.test(body.projectId)) return {projectId:body.projectId};
    return {refused:body.reason === "not_a_project" || body.reason === "runtime_not_running" ? body.reason : "refused"};
  } catch { return {refused:"refused"}; }
}
