/**
 * Getting a session without asking anyone to type a credential.
 *
 * In the ordinary loop the operator runs `graphhelm serve` and `npm run dev` on one machine. The
 * Runtime has already written the bearer token to disk; the dev server reads that same file and
 * hands it to the page (see `devSession` in `vite.config.ts`). So the Studio opens connected and
 * nobody copies a hex string out of a terminal.
 *
 * IT IS A CONVENIENCE, NOT A GUARANTEE, and the difference is the whole design here. A production
 * bundle served from anywhere else has no such endpoint, so this returns `null` and the page asks
 * for the token as before. The absence is expected, not an error, and it is never reported as
 * one — a page that shouted "session unavailable" at every non-dev deployment would be crying
 * about its own correct behaviour.
 *
 * The token still never leaves memory once it arrives. This module hands it to `RuntimeClient`
 * and keeps no copy.
 */

/** Where the dev server offers it. Same-origin by construction: a relative path cannot be pointed
 * at another host by configuration or by a stray environment variable. */
const SESSION_PATH = "/__studio/session";

/** A bound on what will be accepted as a token, so a misconfigured endpoint returning a page of
 * HTML cannot become an `Authorization` header. */
const MAX_TOKEN_LENGTH = 512;

export interface DevSession {
  token: string;
  /** What to call the folder this Runtime serves, when the operator named it
   * (`GRAPHHELM_PROJECT`). `null` leaves the rail saying what it can honestly say. */
  project: string | null;
  /** The absolute source folder, when the local launcher can identify it. */
  projectPath?: string | null;
  onDemandLanes?: string[];
}

/** #1264: where the page keeps the dev server's STABLE session key between loads. It is not the
 * token: it is a key derived from the token file (see `stableNonce` in `vite.config.ts`), useful
 * only to this machine's dev server and only while that token stays the same. It lives in the
 * same per-user browser profile that already keeps it in history via `?session=`. */
export const SESSION_KEY_STORAGE = "graphhelm.studio.sessionKey";

/** The small slice of `Storage` the session key needs; `null` when the browser refuses storage. */
export type KeyStore = Pick<Storage, "getItem" | "setItem" | "removeItem"> | null;

function browserStore(): KeyStore {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

async function ask(fetchImpl: typeof fetch, nonce: string): Promise<Response | null> {
  try {
    const response = await fetchImpl(`${SESSION_PATH}?nonce=${encodeURIComponent(nonce)}`, {
      headers: { Accept: "application/json" },
    });
    return response.ok ? response : null;
  } catch {
    return null;
  }
}

export async function devSession(
  fetchImpl: typeof fetch = globalThis.fetch.bind(globalThis),
  search: string = globalThis.location?.search ?? "",
  store: KeyStore = browserStore(),
): Promise<DevSession | null> {
  // The page presents the nonce it was OPENED with (`?session=<nonce>`, from the URL the dev
  // server printed or the launcher opened). #1264: failing that, it presents the stable key the
  // dev server handed it on an earlier load, so F5, a bookmark, a `?run=` deep link or a Studio
  // restart never sends the owner back to the token gate while the token on disk is unchanged.
  // With neither there is nothing to present, and the connect gate is the answer: the endpoint
  // refuses a bare request precisely so that a request another local user can forge earns nothing.
  const presented = new URLSearchParams(search).get("session");
  let remembered: string | null = null;
  try {
    remembered = store?.getItem(SESSION_KEY_STORAGE) ?? null;
  } catch {
    remembered = null;
  }
  const candidates = [presented, remembered].filter(
    (nonce, index, all): nonce is string => typeof nonce === "string" && nonce.length > 0 && all.indexOf(nonce) === index,
  );
  let response: Response | null = null;
  for (const nonce of candidates) {
    response = await ask(fetchImpl, nonce);
    if (response !== null) break;
    // A remembered key the server now refuses belongs to an old token; keeping it only adds a
    // doomed request to every load.
    if (nonce === remembered) {
      try { store?.removeItem(SESSION_KEY_STORAGE); } catch { /* storage refused: nothing kept */ }
    }
  }
  if (response === null) return null;

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    return null;
  }
  if (payload === null || typeof payload !== "object") return null;

  const token = (payload as { token?: unknown }).token;
  if (typeof token !== "string") return null;
  const trimmed = token.trim();
  if (trimmed.length === 0 || trimmed.length > MAX_TOKEN_LENGTH) return null;

  const project = (payload as { project?: unknown }).project;
  const named = typeof project === "string" ? project.trim() : "";
  const projectPath = (payload as { projectPath?: unknown }).projectPath;
  const folder = typeof projectPath === "string" ? projectPath.trim() : "";
  const roster = (payload as { onDemandLanes?: unknown }).onDemandLanes;
  const key = (payload as { sessionKey?: unknown }).sessionKey;
  if (typeof key === "string" && /^[0-9a-f]{32,128}$/.test(key)) {
    try { store?.setItem(SESSION_KEY_STORAGE, key); } catch { /* storage refused: F5 asks again */ }
  }
  return {
    token: trimmed,
    // Bounded and rendered as text: it is a label from the operator's own environment, but it
    // reaches the DOM and nothing else validates it.
    project: named.length > 0 && named.length <= 120 ? named : null,
    projectPath: folder.length > 0 && folder.length <= 4096 && !/[\u0000-\u001f\u007f]/.test(folder) ? folder : null,
    ...(Array.isArray(roster) ? { onDemandLanes: roster.filter((name): name is string => typeof name === "string" && name.length > 0 && name.length <= 80).slice(0, 100) } : {}),
  };
}

/**
 * What one session told the room about its own model and effort.
 *
 * `effort` is the closed vocabulary the Runtime enforces (`low | medium | high`); `model` is an
 * opaque string, because the set of models changes faster than this repository ships and an enum
 * here would refuse a real declaration for no reason the operator could see.
 */
