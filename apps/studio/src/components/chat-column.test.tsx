import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
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
const THREADS = chatThreads([msg(1, "coordinator", null, "Plan ready"), msg(2, "kit-1", "kit-2", "Take the cart")], BOTS, "studio-operator");

function props(overrides: Partial<ChatColumnProps> = {}): ChatColumnProps {
  return {
    threads: THREADS, selected: "everyone", onSelect: vi.fn(), unread: { everyone: 0, "pair:kit-1+kit-2": 1 }, bots: BOTS,
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
    expect(composerMode(THREADS[1], new Set())).toBe("none");
    expect(composerMode({ key: "direct:t-1", kind: "direct", label: "x", participants: ["t-1"], messages: [] }, new Set(["t-1"]))).toBe("native");
    expect(composerMode({ key: "direct:kit-1", kind: "direct", label: "x", participants: ["kit-1"], messages: [] }, new Set())).toBe("record");
  });
});

describe("ChatColumn", () => {
  it("shows thread tabs with unread counts and labels bot pairs as recorded messages", async () => {
    const p = props();
    render(<ChatColumn {...p} />);
    expect(screen.getByRole("tab", { name: "Everyone" })).toHaveAttribute("aria-selected", "true");
    await userEvent.click(screen.getByRole("tab", { name: "loja kit 1 ↔ loja kit 2, 1 unread" }));
    expect(p.onSelect).toHaveBeenCalledWith("pair:kit-1+kit-2");
  });

  it("explains that a pair tab shows recorded messages, not native chats", () => {
    render(<ChatColumn {...props({ selected: "pair:kit-1+kit-2" })} />);
    expect(screen.getByText(/recorded messages/i)).toHaveTextContent("not their native chats");
    expect(screen.queryByRole("textbox", { name: "Message" })).toBeNull();
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

  it("keeps the principal conversation mounted, only hidden, when a pair tab is selected and back", () => {
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
    view.rerender(<ChatColumn {...props({ principal: <Principal />, selected: "pair:kit-1+kit-2" })} />);
    expect(aside()).toBe(first);
    expect(first.closest("[hidden]")).not.toBeNull();
    view.rerender(<ChatColumn {...props({ principal: <Principal /> })} />);
    expect(aside()).toBe(first);
    expect(first.closest("[hidden]")).toBeNull();
    expect(mounts).toBe(1);
  });
});
