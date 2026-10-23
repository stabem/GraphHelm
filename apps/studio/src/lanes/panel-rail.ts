/** #1934: whether this viewer folded the Lanes agent panel to its 40px rail. Kept per viewer in
 * localStorage; a store that is missing or throws (private window, blocked site data) leaves the
 * panel open and never breaks the view. */

export const PANEL_KEY = "graphhelm.studio.lanes.agentPanel";

type Store = Pick<Storage, "getItem" | "setItem">;

function store(): Store | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

export function readCollapsed(from: Store | null = store()): boolean {
  try {
    return from?.getItem(PANEL_KEY) === "collapsed";
  } catch {
    return false;
  }
}

export function writeCollapsed(collapsed: boolean, to: Store | null = store()): void {
  try {
    to?.setItem(PANEL_KEY, collapsed ? "collapsed" : "open");
  } catch {
    /* the choice lasts this page only */
  }
}
