/** #1934: the one date format of the Studio (docs/design/studio/Tokens.dc.html), in local time.
 * Under an hour ago: "2 min ago"; earlier today: "00:02"; this year: "23 Oct 00:02"; older:
 * "23 Oct 2025". The full stamp goes in a title tooltip. */

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const pad = (value: number) => String(value).padStart(2, "0");

export interface Stamp {
  /** What the screen shows. */
  text: string;
  /** The full local stamp, for the tooltip; empty when the time is unknown. */
  full: string;
}

/** Formats `at` (an ISO stamp, a Date, or epoch milliseconds) against `now`. An absent or unreadable time
 * reads "—", never a raw string. */
export function when(at: string | number | Date | null | undefined, now: Date = new Date()): Stamp {
  if (at === null || at === undefined || at === "") return { text: "—", full: "" };
  const date = at instanceof Date ? at : new Date(at);
  if (Number.isNaN(date.getTime())) return { text: "—", full: "" };
  const hm = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  const day = `${date.getDate()} ${MONTHS[date.getMonth()]}`;
  const full = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${hm}:${pad(date.getSeconds())} local`;
  const age = now.getTime() - date.getTime();
  if (age >= 0 && age < 3_600_000) {
    const minutes = Math.floor(age / 60_000);
    return { text: minutes === 0 ? "just now" : `${minutes} min ago`, full };
  }
  const sameYear = date.getFullYear() === now.getFullYear();
  if (sameYear && date.getMonth() === now.getMonth() && date.getDate() === now.getDate()) return { text: hm, full };
  return { text: sameYear ? `${day} ${hm}` : `${day} ${date.getFullYear()}`, full };
}
