import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
// Its own instance: see the helper for why this is not a shared const.
const userEvent = fastUserEvent();

import { useState } from "react";

import { Composer } from "./compose";

/**
 * #1098 D1: the objective now lives on the PAGE, so these cells mount the composer inside the
 * smallest possible owner of that state. Driving the real two-way binding is the point - a stub
 * that ignored `onObjectiveChange` would let a composer that never reports its text pass.
 */
function Owned(props: Omit<Parameters<typeof Composer>[0], "objective" | "onObjectiveChange">) {
  const [objective, setObjective] = useState("");
  return <Composer {...props} objective={objective} onObjectiveChange={setObjective} />;
}

/**
 * Every control the composer offers is gated on `busy` - INCLUDING discard.
 *
 * Discard was the one sibling without the gate (PR #467 review): while `startTask` was in
 * flight the operator could throw the draft away, and the pending call then landed state for a
 * task that no longer existed on screen. The N-review lesson attached to this exact finding:
 * checking that the parent passes `busy` down is not checking that a child uses it.
 */
describe("the composer under a pending start", () => {
  const mount = (busy: boolean) =>
    render(
      <Owned choice={null} busy={busy} error="" onSend={vi.fn()} onCancel={vi.fn()} />,
    );

  it("gates discard while the start is in flight", () => {
    mount(true);
    expect(screen.getByRole("button", { name: "Discard this task" })).toBeDisabled();
  });

  it("frees discard the moment nothing is pending", () => {
    mount(false);
    expect(screen.getByRole("button", { name: "Discard this task" })).toBeEnabled();
  });
});

/**
 * #1083 F5: the hint says `Enter sends`, and a real browser's Enter did not send. These cells
 * dispatch the keydown the way a browser does - a native `KeyboardEvent` on the focused textarea,
 * bubbling to React's root listener - rather than React's synthetic helper, and check that the
 * send happened and the newline was suppressed.
 */
describe("Enter in the composer", () => {
  function typed(text: string) {
    const onSend = vi.fn();
    render(<Owned choice={{ configured: false, routes: [] }} busy={false} error="" onSend={onSend} onCancel={vi.fn()} />);
    const box = screen.getByLabelText("What should this task do?") as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: text } });
    box.focus();
    return { box, onSend };
  }
  const press = (target: HTMLElement, init: KeyboardEventInit & { keyCode?: number }) => {
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    if (init.keyCode !== undefined) Object.defineProperty(event, "keyCode", { value: init.keyCode });
    target.dispatchEvent(event);
    return event;
  };

  it("sends on a native Enter keydown and keeps the newline out of the text", () => {
    const { box, onSend } = typed("Summarize the README");
    const event = press(box, { key: "Enter", code: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledWith("Summarize the README", null);
    expect(event.defaultPrevented).toBe(true);
  });

  it("sends on an automation driver's Enter (keyCode 0, empty code) and on the keypad's", () => {
    const { box, onSend } = typed("Draft release notes");
    press(box, { key: "Enter", code: "", keyCode: 0 });
    press(box, { key: "Enter", code: "NumpadEnter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledTimes(2);
  });

  it("sends through the keyboard path a user-event Enter takes", async () => {
    const { onSend } = typed("Map the repository");
    await userEvent.keyboard("{Enter}");
    expect(onSend).toHaveBeenCalledWith("Map the repository", null);
  });

  it("never sends on Shift+Enter or on an IME's committing Enter", () => {
    const { box, onSend } = typed("日本語");
    press(box, { key: "Enter", code: "Enter", shiftKey: true, keyCode: 13 });
    press(box, { key: "Enter", code: "Enter", isComposing: true, keyCode: 13 });
    press(box, { key: "Process", code: "Enter", keyCode: 229 });
    expect(onSend).not.toHaveBeenCalled();
  });
});
