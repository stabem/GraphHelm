import type { KeyboardEvent } from "react";

/**
 * Whether a keydown in a chat box means "send" (#1083 F5).
 *
 * ONE PREDICATE FOR EVERY CHAT BOX ON THE PAGE. The new-task composer was taught Enter and the
 * run's own message box was not, so the same printed promise held in one box and silently failed
 * in the other (orchestrator verification of PR #1091: typed into "Say something into this run",
 * pressed Enter, nothing left the page). Both boxes read this function, so they cannot drift.
 *
 * Read the key the way browsers actually deliver it: an automation driver's Enter arrives with
 * `keyCode 0` and an empty `code`; a numeric keypad's has `code: "NumpadEnter"`. Shift+Enter breaks
 * the line. An IME's own Enter - the one that COMMITS a composition - is `isComposing`, or on
 * Chromium before `compositionend` `keyCode 229`; it must never send half-composed text.
 */
export function sendsOnEnter(event: KeyboardEvent<HTMLElement>): boolean {
  const native = event.nativeEvent;
  const enter = event.key === "Enter" || native.code === "Enter" || native.code === "NumpadEnter";
  return enter && !event.shiftKey && !native.isComposing && native.keyCode !== 229;
}
