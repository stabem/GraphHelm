import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { FrameCompareView } from "./frame-compare-view";

const userEvent = fastUserEvent();

function mount(over: Partial<Parameters<typeof FrameCompareView>[0]> = {}) {
  const onBack = vi.fn();
  render(<FrameCompareView beforeUrl="blob:before" nowUrl="blob:now" diffUrl="blob:diff" beforeLabel="Before (Oct 20)" onBack={onBack} {...over} />);
  return { onBack };
}

describe("FrameCompareView (#1776)", () => {
  it("opens in Overlay at 50%: Now drawn over Before at half opacity", () => {
    mount();
    expect(screen.getByRole("button", { name: "Overlay" })).toHaveAttribute("aria-pressed", "true");
    const slider = screen.getByRole("slider", { name: "Now opacity" });
    expect(slider).toHaveValue("50");
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
    expect(screen.getByRole("slider", { name: "Now opacity" })).toHaveValue("50");
  });

  it("opens in the mode it was asked for", () => {
    mount({ initialMode: "changes" });
    expect(screen.getByRole("button", { name: "Changes" })).toHaveAttribute("aria-pressed", "true");
  });

  it("offers no Changes mode when no diff was computed", () => {
    mount({ diffUrl: null });
    expect(screen.queryByRole("button", { name: "Changes" })).toBeNull();
    expect(screen.getByRole("button", { name: "Overlay" })).toBeInTheDocument();
  });

  it("returns to the frame on Back to frame and on Escape", async () => {
    const { onBack } = mount();
    await userEvent.click(screen.getByRole("button", { name: "Back to frame" }));
    expect(onBack).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(screen.getByRole("group", { name: "Compare Before and Now" }), { key: "Escape" });
    expect(onBack).toHaveBeenCalledTimes(2);
  });
});
