/** #2440: what a step's thumbnail (and the large viewer) shows while its run is active and the
 * step has no frame yet: the spinner, and the word "waiting" for `prefers-reduced-motion`, which
 * swaps the spinner for the static word. The frame replaces it when it arrives. */
export function FrameWaiting() {
  return (
    <span className="frame-waiting" role="status" aria-busy="true" aria-label="waiting for frame">
      <span className="spin" aria-hidden="true" />
      <span className="frame-waiting-text" aria-hidden="true">waiting</span>
    </span>
  );
}
