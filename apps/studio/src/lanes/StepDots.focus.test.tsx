/** @vitest-environment jsdom */
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { StepDots } from "./StepDots";
import type { StepDot } from "./step-dot";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const dots: StepDot[] = ["plan","critic","implement","review"].map((step, i) => ({ step, state: i < 2 ? "done" : i === 2 ? "in progress" : "not started", text: step, record: null, href: null }) as StepDot);
it("the tab stop follows the focused dot after an arrow move (#2567)", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(createElement(StepDots, { label: "P", dots })));
  const marks = [...host.querySelectorAll<HTMLElement>("[data-dot]")];
  marks[2]!.focus();
  await act(async () => { marks[2]!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true })); });
  expect(document.activeElement).toBe(marks[3]);
  const stops = [...host.querySelectorAll<HTMLElement>("[data-dot]")].filter((m) => m.tabIndex === 0);
  expect(stops).toHaveLength(1);
  expect(stops[0]).toBe(marks[3]);
  await act(async () => root.unmount()); host.remove();
});
