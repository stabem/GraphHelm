import { when } from "./when";

/** A time as the Studio shows it: the short form, with the full local stamp in the tooltip (#1934). */
export function When({ at, now, className }: { at: string | number | Date | null | undefined; now?: Date; className?: string }) {
  const stamp = when(at, now);
  const iso = at === null || at === undefined || stamp.full === "" ? undefined : new Date(at).toISOString();
  return <time className={className} dateTime={iso} title={stamp.full || undefined}>{stamp.text}</time>;
}
