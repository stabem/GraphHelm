import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { useFrameCompare, type CompareSubject } from "./frame-compare-view";

const userEvent = fastUserEvent();

// Stands in for the frame: the tools go in a bar, the view in the viewport, as TestCanvas does.
function Frame({ subject, onFrame }: { subject: CompareSubject; onFrame: () => void }) {
  const { tools, view } = useFrameCompare(subject, onFrame);
  return (<><div role="toolbar" aria-label="Frame bar">{tools}</div><div>{view}</div></>);
}

function mount(over: Partial<CompareSubject> = {}) {
  const onFrame = vi.fn();
  const subject: CompareSubject = { beforeUrl: "blob:before", nowUrl: "blob:now", diffUrl: "blob:diff", beforeLabel: "Before (Oct 20)", mode: "overlay", ...over };
  render(<Frame subject={subject} onFrame={onFrame} />);
  return { onFrame };
}

describe("useFrameCompare (#1776)", () => {
  it("puts Frame, Side by side, Overlay and Changes in the frame bar, and opens in Overlay at 50%", () => {
    mount();
    const bar = screen.getByRole("toolbar", { name: "Frame bar" });
    const modes = screen.getByRole("group", { name: "Compare mode" });
    expect(bar).toContainElement(modes);
    expect([...modes.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Frame", "Side by side", "Overlay", "Changes"]);
    expect(screen.getByRole("button", { name: "Overlay" })).toHaveAttribute("aria-pressed", "true");
    const slider = screen.getByRole("slider", { name: "Now opacity" });
    expect(bar).toContainElement(slider);
    expect(slider).toHaveValue("50");
    expect(screen.queryByRole("button", { name: "Back to frame" })).toBeNull();
    expect(screen.getByRole("img", { name: "Before (Oct 20)" })).toHaveAttribute("src", "blob:before");
    expect(screen.getByRole("img", { name: "Now" })).toHaveStyle({ opacity: "0.5" });
  });

  it("applies the slider value as the Now frame's opacity", () => {
    mount();
    fireEvent.change(screen.getByRole("slider", { name: "Now opacity" }), { target: { value: "20" } });
    expect(screen.getByRole("img", { name: "Now" })).toHaveStyle({ opacity: "0.2" });
    fireEvent.change(screen.getByRole("slider", { name: "Now opacity" }), { target: { value: "100" } });
    expect(screen.getByRole("img", { name: "Now" })).toHaveStyle({ opacity: "1" });
  });

  it("switches between side by side, overlay and changes", async () => {
    mount();
    await userEvent.click(screen.getByRole("button", { name: "Side by side" }));
    expect(screen.getByRole("button", { name: "Side by side" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("slider")).toBeNull();
    const pair = screen.getByRole("group", { name: "Before and Now side by side" });
    expect(pair.querySelectorAll("img")).toHaveLength(2);
    await userEvent.click(screen.getByRole("button", { name: "Changes" }));
    expect(screen.getByRole("img", { name: "Now, changed regions highlighted" })).toHaveAttribute("src", "blob:diff");
    expect(screen.queryByRole("img", { name: "Before (Oct 20)" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Overlay" }));
    expect(screen.getByRole("slider", { name: "Now opacity" })).toBeInTheDocument();
  });

  it("takes focus when it opens, so Escape works from the keyboard", async () => {
    const { onFrame } = mount();
    expect(screen.getByRole("group", { name: "Compare Before and Now" })).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(onFrame).toHaveBeenCalledTimes(1);
  });

  it("opens in the mode it was asked for", () => {
    mount({ mode: "changes" });
    expect(screen.getByRole("button", { name: "Changes" })).toHaveAttribute("aria-pressed", "true");
  });

  it("offers no Changes mode when no diff was computed", () => {
    mount({ diffUrl: null, mode: "changes" });
    expect(screen.queryByRole("button", { name: "Changes" })).toBeNull();
    expect(screen.getByRole("button", { name: "Overlay" })).toHaveAttribute("aria-pressed", "true");
  });

  it("returns to the frame on Frame and on Escape", async () => {
    const { onFrame } = mount();
    await userEvent.click(screen.getByRole("button", { name: "Frame" }));
    expect(onFrame).toHaveBeenCalledTimes(1);
    await userEvent.keyboard("{Escape}");
    expect(onFrame).toHaveBeenCalledTimes(2);
  });

  it("draws nothing while no compare is open", () => {
    function Closed() { const { tools, view } = useFrameCompare(null, () => {}); return <div data-testid="c">{tools}{view}</div>; }
    render(<Closed />);
    expect(screen.getByTestId("c")).toBeEmptyDOMElement();
  });
});
