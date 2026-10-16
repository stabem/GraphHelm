import { act, renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import type { RuntimeClient } from "./client";
import type { QuestionItem } from "./needs-you";
import { useSay } from "./use-say";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const sent = { result: "sent" as const, diagnostics: [] };

function mount(signal: ReturnType<typeof vi.fn>, selected = "run-a") {
  const clientRef = { current: { signal } as unknown as RuntimeClient };
  const selectedRef = { current: selected };
  const loadExecution = vi.fn(async () => undefined);
  const hook = renderHook((props: { selected: string }) => useSay({ clientRef, selected: props.selected, selectedRef, loadExecution }), {
    initialProps: { selected },
  });
  return { hook, selectedRef, loadExecution };
}

// #1413: the message boxes' state moved out of App, and nothing pinned how a send's busy display
// behaves across a run switch, how an unconfirmed send reads, or when a refusal may be sent.
it("drops the old run's busy display on a switch and never clears the new run's from the old send", async () => {
  const first = deferred<typeof sent>();
  const second = deferred<typeof sent>();
  const signal = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const { hook, selectedRef } = mount(signal);

  let a!: Promise<boolean>;
  act(() => { a = hook.result.current.say("hello", null, "run"); });
  expect(hook.result.current.saying).toBe("run");

  act(() => { selectedRef.current = "run-b"; hook.rerender({ selected: "run-b" }); hook.result.current.resetSay(); });
  expect(hook.result.current.saying).toBeNull();

  let b!: Promise<boolean>;
  act(() => { b = hook.result.current.say("next", null, "talk"); });
  expect(hook.result.current.saying).toBe("talk");
  await act(async () => { first.resolve(sent); await a; });
  expect(hook.result.current.saying, "the old send's finally must not clear the new run's box").toBe("talk");
  await act(async () => { second.resolve(sent); await b; });
  expect(hook.result.current.saying).toBeNull();
});

it("never reports an unconfirmed send as sent, and says so in the box that sent it", async () => {
  const signal = vi.fn(async () => ({ result: "unknown" as const, diagnostics: [] }));
  const { hook, loadExecution } = mount(signal);
  let outcome!: boolean;
  await act(async () => { outcome = await hook.result.current.say("hello", null, "agent"); });
  expect(outcome).toBe(false);
  expect(hook.result.current.sayError).toEqual({ via: "agent", text: expect.stringMatching(/could not be confirmed/) });
  expect(loadExecution).toHaveBeenCalledWith("run-a");
});

it("refuses only a question it can retire, and sends the refusal as a reply to it", async () => {
  const signal = vi.fn(async () => sent);
  const { hook } = mount(signal);
  const question = (signalId: string | null) => ({ kind: "question", key: "q", asker: "codex", text: "?", signalId, recommendations: [], at: null, sequence: 1 }) as QuestionItem;

  let outcome!: boolean;
  await act(async () => { outcome = await hook.result.current.refuse(question(null), "no"); });
  expect(outcome).toBe(false);
  expect(signal).not.toHaveBeenCalled();

  await act(async () => { outcome = await hook.result.current.refuse(question("q-1"), " not now "); });
  expect(outcome).toBe(true);
  expect(signal).toHaveBeenCalledWith("run-a", JSON.stringify({ protocol: "graphhelm-owner-refusal-v1", reason: "not now" }),
    { to: "codex", replyTo: "q-1", kind: "owner_refusal" });
});
