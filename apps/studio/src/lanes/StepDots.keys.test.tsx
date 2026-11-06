/** @vitest-environment jsdom */
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it } from "vitest";

import { StepDots } from "./StepDots";
import type { StepDot } from "./step-dot";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

// #2548: ArrowRight/ArrowLeft move focus between the dots of one row; Home and End go to the ends.
const dots: StepDot[] = [
  { step: "plan", state: "done", text: "plan · done", record: null, href: null },
  { step: "critic", state: "done", text: "critic · done", record: null, href: null },
  { step: "implement", state: "in progress", text: "implement · in progress", record: null, href: null },
  { step: "review", state: "not started", text: "review · not started", record: null, href: null },
];
let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;
afterEach(async () => {
  if (root !== null) await act(async () => root!.unmount());
  host?.remove(); root = null; host = null;
});

it("arrow keys move focus between dots; Home and End jump to the ends", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(createElement(StepDots, { label: "Progress", dots })));
  const marks = [...host.querySelectorAll<HTMLElement>("[data-dot]")];
  expect(marks).toHaveLength(4);
  expect(marks[2]!.getAttribute("tabindex")).toBe("0");
  marks[2]!.focus();
  const press = (key: string) => act(async () => {
    const focused = document.activeElement as HTMLElement;
    focused.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  });
  await press("ArrowRight");
  expect(document.activeElement).toBe(marks[3]);
  await press("ArrowLeft");
  await press("ArrowLeft");
  expect(document.activeElement).toBe(marks[1]);
  await press("Home");
  expect(document.activeElement).toBe(marks[0]);
  await press("End");
  expect(document.activeElement).toBe(marks[3]);
  await press("ArrowRight");
  expect(document.activeElement).toBe(marks[3]);
});
