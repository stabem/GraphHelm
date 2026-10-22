/**
 * Login by ticket. `graphhelm studio open` (or the dev server's printed URL) opens the page with
 * `?session=<ticket or nonce>`; the dev server trades it once for the Runtime token and a stable
 * key. The key is kept in this tab's sessionStorage so a reload stays logged in without the spent
 * ticket. The token itself is kept only in memory.
 */
export interface Session {
  token: string;
  project: string | null;
}

export const SESSION_KEY_STORAGE = "graphhelm.studio-next.sessionKey";

type KeyStore = Pick<Storage, "getItem" | "setItem" | "removeItem"> | null;

function browserStore(): KeyStore {
  try {
    return globalThis.sessionStorage ?? null;
  } catch {
    return null;
  }
}

/** One request per presented secret, shared: a one-shot ticket must not be spent twice by a
 * double mount. */
const inFlight = new Map<string, Promise<unknown>>();

function ask(nonce: string): Promise<unknown> {
  const pending = inFlight.get(nonce);
  if (pending) return pending;
  const request = fetch(`/__studio/session?nonce=${encodeURIComponent(nonce)}`, { headers: { Accept: "application/json" } })
    .then((response) => (response.ok ? response.json() : null))
    .catch(() => null);
  inFlight.set(nonce, request);
  return request;
}

export async function login(search: string, store: KeyStore = browserStore()): Promise<Session | null> {
  const presented = new URLSearchParams(search).get("session");
  let remembered: string | null = null;
  try {
    remembered = store?.getItem(SESSION_KEY_STORAGE) ?? null;
  } catch {
    remembered = null;
  }
  for (const nonce of [presented, remembered]) {
    if (!nonce) continue;
    const payload = (await ask(nonce)) as { token?: unknown; project?: unknown; sessionKey?: unknown } | null;
    if (payload && typeof payload.token === "string" && payload.token.trim().length > 0 && payload.token.length <= 512) {
      if (typeof payload.sessionKey === "string" && /^[0-9a-f]{32,128}$/.test(payload.sessionKey)) {
        try { store?.setItem(SESSION_KEY_STORAGE, payload.sessionKey); } catch { /* reload will ask again */ }
      }
      const project = typeof payload.project === "string" && payload.project.length <= 120 ? payload.project : null;
      return { token: payload.token.trim(), project };
    }
    if (nonce === remembered) {
      try { store?.removeItem(SESSION_KEY_STORAGE); } catch { /* nothing kept */ }
    }
  }
  return null;
}
