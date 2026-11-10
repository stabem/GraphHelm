/** What the poll needs of `document`, so a test can stand in for it. */
interface PageVisibility {
  readonly hidden: boolean;
  addEventListener(type: "visibilitychange", listener: () => void): void;
  removeEventListener(type: "visibilitychange", listener: () => void): void;
}

/**
 * Calls `read` at once, then every `ms` while the tab is visible. A hidden tab reads nothing, so a
 * Studio left open in the background does not keep asking the Runtime; one read runs as soon as
 * the tab is shown again. Returns the cleanup for an effect.
 */
export function pollWhileVisible(read: () => void, ms: number, doc: PageVisibility = document): () => void {
  const readIfShown = () => { if (!doc.hidden) read(); };
  read();
  const timer = setInterval(readIfShown, ms);
  doc.addEventListener("visibilitychange", readIfShown);
  return () => {
    clearInterval(timer);
    doc.removeEventListener("visibilitychange", readIfShown);
  };
}
