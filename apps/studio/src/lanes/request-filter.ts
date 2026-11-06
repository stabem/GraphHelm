import type { Agent } from "./board";

/** #2530: the request the Lanes board is narrowed to, kept in the URL as `?ask=<key>` so a link or
 * reload lands on the same filter. Other parameters (view, journey, run) are left as they are. */
const ASK_KEY = /^[A-Za-z0-9_.:/-]{1,200}$/;

/** The `ask` parameter of a query string, or `null` when absent or not a request key. */
export function readAskParam(search: string): string | null {
  const ask = new URLSearchParams(search).get("ask");
  return ask !== null && ASK_KEY.test(ask) && !ask.includes("..") ? ask : null;
}

/** The query string with `ask` set to the key, or removed when the key is `null`. */
export function withAskParam(search: string, ask: string | null): string {
  const query = new URLSearchParams(search);
  if (ask === null) query.delete("ask");
  else query.set("ask", ask);
  const text = query.toString();
  return text === "" ? "" : `?${text}`;
}

/** The agents a board shows: all of them with no request selected, otherwise only the lanes that
 * serve the selected request. Columns count what is left, so a column can be empty. */
export function applyAskFilter<T extends Agent>(agents: readonly T[], lanes: ReadonlySet<string> | null): T[] {
  return lanes === null ? [...agents] : agents.filter((agent) => lanes.has(agent.lane));
}
