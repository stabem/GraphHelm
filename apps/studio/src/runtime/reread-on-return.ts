/** #1732: a Studio left open reads the project again when the owner comes back to it.
 *
 * Preview results live in the project's store and every Studio reads them from the Runtime
 * (`lastRun` and `approvable` on `/v1/journey-flows`), so a Studio opened after another one
 * recorded a pass already shows it. A Studio that was open before reads its journey list once;
 * without this, a pass recorded elsewhere stayed `not run` until the owner acted. Returning to the
 * tab (window `focus`, or the document turning `visible`) reads the list again. No timer: nothing
 * is read while the owner is away. Answers the cleanup that removes both listeners. */
export function rereadOnReturn(win: EventTarget, doc: EventTarget & { visibilityState: string }, read: () => void): () => void {
  const again = () => { if (doc.visibilityState === "visible") read(); };
  win.addEventListener("focus", again);
  doc.addEventListener("visibilitychange", again);
  return () => {
    win.removeEventListener("focus", again);
    doc.removeEventListener("visibilitychange", again);
  };
}
