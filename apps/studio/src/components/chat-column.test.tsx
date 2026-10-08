import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, useState } from "react";

import { fastUserEvent } from "../test/user-event";
import type { Bot } from "../runtime/team";
import { chatThreads } from "../runtime/threads";
import type { WorkMessage } from "../runtime/work-conversation";
import { ChatColumn, composerMode, type ChatColumnProps } from "./chat-column";

const userEvent = fastUserEvent();
afterEach(cleanup);

const bot = (key: string, name: string, native = false): Bot => ({ key, actorId: native ? null : key, name, hue: 10, role: null, doingNow: "",
  lastRecordAt: null, lastSequence: 0, state: "working", quietMinutes: null, shared: false, native, tasks: [] });
const BOTS = [bot("coordinator", "Coordinator"), bot("kit-1", "loja kit 1"), bot("kit-2", "loja kit 2")];
const msg = (sequence: number, sender: string, to: string | null, text: string): WorkMessage => ({ id: `event-${sequence}`, sequence, sender, to,
  replyTo: null, text, at: null, provenance: "stored", acknowledged: false });
// #393: the agent-to-agent message is tagged with its task, so it lands in that task's thread.
const TASK = { taskId: "issue-384", issue: 384, pr: null, lane: "kit-1", headSha: null, journeys: [], step: "implement" as const,
  blockedBy: null, reviewers: [], mergeSha: null, repoUrl: null, lastSequence: 2 };
const THREADS = chatThreads([msg(1, "coordinator", null, "Plan ready"), { ...msg(2, "kit-1", "kit-2", "Take the cart"), task: "issue-384" }],
  BOTS, "studio-operator", [TASK]);

function props(overrides: Partial<ChatColumnProps> = {}): ChatColumnProps {
  return {
    threads: THREADS, selected: "everyone", onSelect: vi.fn(), unread: { everyone: 0, "task:issue-384": 1 }, bots: BOTS,
    names: { coordinator: "Coordinator", "kit-1": "loja kit 1", "kit-2": "loja kit 2" }, openingCount: 0, cards: null,
    jev: { suggestions: [], loading: false, issue: null }, nativeKeys: new Set(), principal: <aside aria-label="Principal conversation">main</aside>,
    onSend: vi.fn(), sending: false, sendError: "", answering: null, onClearAnswer: vi.fn(), composerFocus: 0, highlight: null,
    onUseSuggestion: vi.fn(), ...overrides,
  };
}

describe("composerMode", () => {
  it("routes each tab to the right composer", () => {
    expect(composerMode(THREADS[0], new Set())).toBe("record+principal");
    expect(composerMode(THREADS[0], new Set(["t-1"]))).toBe("native");
    expect(composerMode(THREADS[1], new Set())).toBe("record");
    expect(composerMode({ key: "direct:t-1", kind: "direct", label: "x", participants: ["t-1"], messages: [] }, new Set(["t-1"]))).toBe("native");
    expect(composerMode({ key: "direct:kit-1", kind: "direct", label: "x", participants: ["kit-1"], messages: [] }, new Set())).toBe("record");
  });
});

describe("ChatColumn", () => {
  it("shows thread tabs with unread counts, one per task", async () => {
    const p = props();
    render(<ChatColumn {...p} />);
    expect(screen.getByRole("tab", { name: "Everyone" })).toHaveAttribute("aria-selected", "true");
    await userEvent.click(screen.getByRole("tab", { name: "Issue #384, 1 unread" }));
    expect(p.onSelect).toHaveBeenCalledWith("task:issue-384");
  });

  it("explains that a task tab shows recorded messages, not native chats", () => {
    render(<ChatColumn {...props({ selected: "task:issue-384" })} />);
    expect(screen.getByText(/recorded messages/i)).toHaveTextContent("not their native chats");
  });

  // #396 (spec §8): speaking inside a task thread tags the message with that task.
  it("sends what is written in a task thread tagged with its task", async () => {
    const p = props({ selected: "task:issue-384", onSend: vi.fn(async () => true) });
    render(<ChatColumn {...p} />);
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "@loja kit 2 rebase please");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(p.onSend).toHaveBeenCalledWith("rebase please", "kit-2", null, "issue-384");
  });

  it("folds a merged or quiet task thread under older until asked", async () => {
    const threads = chatThreads([], BOTS, "studio-operator", [{ ...TASK, step: "merged" as const }]);
    render(<ChatColumn {...props({ threads })} />);
    expect(screen.queryByRole("tab", { name: /^Issue #384/ })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "older (1)" }));
    expect(screen.getByRole("tab", { name: /^Issue #384/ })).toBeInTheDocument();
  });

  // #396 (spec §8): an agent filter on any thread shows only one agent's lines.
  it("filters a thread down to one agent's lines", async () => {
    const threads = chatThreads([msg(1, "coordinator", null, "Plan ready"), msg(2, "kit-1", null, "Taking the cart"), msg(3, "coordinator", null, "Next")],
      BOTS, "studio-operator");
    render(<ChatColumn {...props({ threads })} />);
    expect(screen.getByText("Taking the cart")).toBeInTheDocument();
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Lines from" }), "coordinator");
    expect(screen.queryByText("Taking the cart")).toBeNull();
    expect(screen.getByText("Plan ready")).toBeInTheDocument();
    expect(screen.getByText("Next")).toBeInTheDocument();
  });

  // #396 (spec §8): one Needs you thread gathers every open card, with no conversation under it.
  it("offers a Needs you thread that shows the open cards alone", async () => {
    const onSelect = vi.fn();
    const view = render(<ChatColumn {...props({ cards: <p>Which region?</p>, cardCount: 1, onSelect })} />);
    expect(screen.getByText("Plan ready")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Needs you (1)" }));
    expect(onSelect).toHaveBeenCalledWith("needs-you");
    view.rerender(<ChatColumn {...props({ cards: <p>Which region?</p>, cardCount: 1, selected: "needs-you" })} />);
    expect(screen.getByText("Which region?")).toBeInTheDocument();
    expect(screen.queryByText("Plan ready")).toBeNull();
  });

  // #402 (spec §8): the cards live only in Needs you, never above another thread.
  it("keeps the open cards out of every other thread", () => {
    render(<ChatColumn {...props({ cards: <p>Which region?</p>, cardCount: 1, selected: "everyone" })} />);
    expect(screen.queryByText("Which region?")).toBeNull();
    expect(screen.getByText("Plan ready")).toBeInTheDocument();
  });

  it("keeps sealed records counted while they open", () => {
    render(<ChatColumn {...props({ openingCount: 3 })} />);
    expect(screen.getByText("Opening 3 sealed records…")).toBeInTheDocument();
  });

  it("sends from Everyone to one bot with @name and keeps the principal conversation mounted", async () => {
    const p = props();
    render(<ChatColumn {...p} />);
    expect(screen.getByRole("complementary", { name: "Principal conversation" })).toBeInTheDocument();
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "@loja kit 2 rebase please");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(p.onSend).toHaveBeenCalledWith("rebase please", "kit-2", null);
  });

  it("answers a question with its replyTo and the asker as recipient", async () => {
    const p = props({ selected: "direct:kit-1", threads: [...THREADS, { key: "direct:kit-1", kind: "direct", label: "loja kit 1", participants: ["kit-1"], messages: [] }],
      answering: { asker: "kit-1", signalId: "sig-q" } });
    render(<ChatColumn {...p} />);
    expect(screen.getByText("Answering loja kit 1")).toBeInTheDocument();
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "Wait for review");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(p.onSend).toHaveBeenCalledWith("Wait for review", "kit-1", "sig-q");
  });

  // #402 (spec §8): an answer keeps replyTo and adds the question's task when it had one.
  it("answers a question that named a task with that task", async () => {
    const p = props({ selected: "direct:kit-1", threads: [...THREADS, { key: "direct:kit-1", kind: "direct", label: "loja kit 1", participants: ["kit-1"], messages: [] }],
      answering: { asker: "kit-1", signalId: "sig-q", task: "issue-384" } });
    render(<ChatColumn {...p} />);
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "Ship it");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(p.onSend).toHaveBeenCalledWith("Ship it", "kit-1", "sig-q", "issue-384");
  });

  it("keeps one draft per thread so text typed for one bot never follows to another", async () => {
    const threads = [...THREADS, { key: "direct:kit-1", kind: "direct" as const, label: "loja kit 1", participants: ["kit-1"], messages: [] },
      { key: "direct:kit-2", kind: "direct" as const, label: "loja kit 2", participants: ["kit-2"], messages: [] }];
    const view = render(<ChatColumn {...props({ threads, selected: "direct:kit-1" })} />);
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "only for kit 1");
    view.rerender(<ChatColumn {...props({ threads, selected: "direct:kit-2" })} />);
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("");
    view.rerender(<ChatColumn {...props({ threads, selected: "direct:kit-1" })} />);
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("only for kit 1");
  });

  it("keeps the draft when the send is refused and clears it once delivered", async () => {
    const onSend = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    render(<ChatColumn {...props({ onSend })} />);
    const box = screen.getByRole("textbox", { name: "Message" });
    await userEvent.type(box, "hello all");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(box).toHaveValue("hello all");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(box).toHaveValue("");
  });

  it("puts a Jev suggestion in one dashed card whose Use fills the composer and never sends", async () => {
    const p = props({ jev: { suggestions: [{ to: null, draft: "Ask kit 2 for the cart diff", reason: "kit 2 went quiet", sourceSequences: [1] }], loading: false, issue: null } });
    render(<ChatColumn {...p} />);
    await userEvent.click(screen.getByRole("button", { name: "Use" }));
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Ask kit 2 for the cart diff");
    expect(p.onUseSuggestion).toHaveBeenCalledWith("Ask kit 2 for the cart diff");
    expect(p.onSend).not.toHaveBeenCalled();
  });

  it("keeps the principal conversation mounted, only hidden, when a task tab is selected and back", () => {
    let mounts = 0;
    function Principal() {
      const [state] = useState(() => ({ born: ++mounts }));
      useEffect(() => undefined, []);
      return <aside aria-label="Principal conversation" data-born={state.born}>main</aside>;
    }
    const view = render(<ChatColumn {...props({ principal: <Principal /> })} />);
    const aside = () => document.querySelector('[aria-label="Principal conversation"]') as HTMLElement;
    const first = aside();
    expect(first.closest("[hidden]")).toBeNull();
    view.rerender(<ChatColumn {...props({ principal: <Principal />, selected: "task:issue-384" })} />);
    expect(aside()).toBe(first);
    expect(first.closest("[hidden]")).not.toBeNull();
    view.rerender(<ChatColumn {...props({ principal: <Principal /> })} />);
    expect(aside()).toBe(first);
    expect(first.closest("[hidden]")).toBeNull();
    expect(mounts).toBe(1);
  });
});

/** jsdom lays nothing out: give every element a scroll geometry the tests control. */
function fakeScrollGeometry() {
  let height = 1000;
  const tops = new WeakMap<Element, number>();
  const restore = [
    ["scrollHeight", { configurable: true, get: () => height }],
    ["clientHeight", { configurable: true, get: () => 200 }],
    ["scrollTop", { configurable: true, get(this: Element) { return tops.get(this) ?? 0; }, set(this: Element, value: number) { tops.set(this, value); } }],
  ].map(([name, descriptor]) => {
    const previous = Object.getOwnPropertyDescriptor(HTMLElement.prototype, name as string) ?? Object.getOwnPropertyDescriptor(Element.prototype, name as string);
    Object.defineProperty(HTMLElement.prototype, name as string, descriptor as PropertyDescriptor);
    return () => { if (previous) Object.defineProperty(HTMLElement.prototype, name as string, previous); else delete (HTMLElement.prototype as unknown as Record<string, unknown>)[name as string]; };
  });
  return { grow: (by: number) => { height += by; }, restore: () => restore.forEach((undo) => undo()) };
}

describe("ChatColumn reading (#327)", () => {
  afterEach(() => { try { window.localStorage.clear(); } catch { /* storage refused */ } });

  it("resizes from its edge with the keyboard and the pointer, clamped and remembered", async () => {
    const view = render(<ChatColumn {...props()} />);
    const handle = screen.getByRole("separator", { name: "Resize the chat column" });
    expect(handle).toHaveAttribute("aria-valuenow", "400");
    handle.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(handle).toHaveAttribute("aria-valuenow", "416");
    expect(window.localStorage.getItem("graphhelm.studio.chat-width")).toBe("416");
    fireEvent.pointerDown(handle);
    window.dispatchEvent(new MouseEvent("pointermove", { clientX: 5000 }));
    window.dispatchEvent(new MouseEvent("pointerup"));
    await waitFor(() => expect(window.localStorage.getItem("graphhelm.studio.chat-width")).toBe("760"));
    view.unmount();
    render(<ChatColumn {...props()} />);
    expect(screen.getByRole("separator", { name: "Resize the chat column" })).toHaveAttribute("aria-valuenow", "760");
    expect(screen.getByRole("complementary", { name: "Chat" }).style.getPropertyValue("--chat-w")).toBe("760px");
  });

  it("opens at the newest message and follows new ones while the viewer is at the bottom", () => {
    const geometry = fakeScrollGeometry();
    try {
      const view = render(<ChatColumn {...props()} />);
      const scroller = document.querySelector(".chat-scroll") as HTMLElement;
      expect(scroller.scrollTop).toBe(1000);
      geometry.grow(300);
      const more = chatThreads([msg(1, "coordinator", null, "Plan ready"), msg(2, "kit-1", "kit-2", "Take the cart"), msg(3, "coordinator", null, "Next")], BOTS, "studio-operator");
      view.rerender(<ChatColumn {...props({ threads: more })} />);
      expect(scroller.scrollTop).toBe(1300);
      expect(screen.queryByRole("button", { name: /New messages/ })).toBeNull();
    } finally { geometry.restore(); }
  });

  it("does not move a viewer who scrolled up; offers a New messages pill instead", async () => {
    const geometry = fakeScrollGeometry();
    try {
      const view = render(<ChatColumn {...props()} />);
      const scroller = document.querySelector(".chat-scroll") as HTMLElement;
      scroller.scrollTop = 100;
      fireEvent.scroll(scroller);
      geometry.grow(300);
      const more = chatThreads([msg(1, "coordinator", null, "Plan ready"), msg(2, "kit-1", "kit-2", "Take the cart"), msg(3, "coordinator", null, "Next")], BOTS, "studio-operator");
      view.rerender(<ChatColumn {...props({ threads: more })} />);
      expect(scroller.scrollTop).toBe(100);
      await userEvent.click(screen.getByRole("button", { name: "New messages ↓" }));
      expect(scroller.scrollTop).toBe(1300);
      expect(screen.queryByRole("button", { name: /New messages/ })).toBeNull();
    } finally { geometry.restore(); }
  });

  it("labels advice from an older head and shows a failure with Retry instead of preparing forever", async () => {
    const onRetry = vi.fn();
    const suggestion = { to: null, draft: "Ask kit 2 for the diff", reason: "quiet", sourceSequences: [1] };
    const view = render(<ChatColumn {...props({ jev: { suggestions: [suggestion], loading: false, issue: null, older: true, onRetry } })} />);
    expect(screen.getByText("Based on the run as of a moment ago")).toBeInTheDocument();
    view.rerender(<ChatColumn {...props({ jev: { suggestions: [], loading: false, issue: "Jev could not prepare a suggestion.", older: false, onRetry } })} />);
    expect(screen.queryByText(/preparing a suggestion/)).toBeNull();
    expect(screen.getByText("Jev could not prepare a suggestion.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
