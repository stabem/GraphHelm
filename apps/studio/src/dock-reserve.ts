/**
 * How much of the scene's bottom edge the action docks cover (#1083 F9).
 *
 * The overview's scroll container used to reserve a FIXED 70px (112px under 800px) for the dock,
 * while the dock itself wraps to as many rows as the scene's width forces. At 1280x720 the card
 * grid sat behind the dock and could not be scrolled out from under it. The reserve is now
 * measured: a dock is absolutely positioned against the scene's bottom, so it covers its own
 * height plus its `bottom` offset, and the scroll container pads its content by the largest such
 * cover, so the last card can always scroll above the dock.
 *
 * A dock that is not laid out (height 0: hidden, or not rendered yet) reserves nothing.
 */
export function dockReserve(docks: ReadonlyArray<{ height: number; bottom: number }>): number {
  let reserve = 0;
  for (const dock of docks) {
    if (dock.height <= 0) continue;
    reserve = Math.max(reserve, Math.ceil(dock.height + dock.bottom));
  }
  return reserve;
}
