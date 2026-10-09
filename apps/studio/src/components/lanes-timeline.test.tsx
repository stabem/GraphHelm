import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { LanesTimeline } from "./lanes-timeline";

const now = 1_000_000;
const lanes = [
  { lane: "gh-claude-5", silent: true, lastEventAt: 0, bars: [{ kind: "review" as const, label: "#548", start: now - 500, end: now, open: true }] },
  { lane: "gh-claude-6", silent: false, lastEventAt: now, bars: [{ kind: "implement" as const, label: "#559", start: now - 1000, end: now - 750, open: false }] },
];

describe("LanesTimeline", () => {
  it("places bars on the window", () => {
    render(<LanesTimeline lanes={lanes} now={now} windowMs={1000} />);
    const bar = screen.getByText("#559 implement").closest("[data-kind]") as HTMLElement;
    expect(bar.style.left).toBe("0%");
    expect(bar.style.width).toBe("25%");
  });

  it("flags a silent lane in text, not only colour", () => {
    render(<LanesTimeline lanes={lanes} now={now} windowMs={1000} />);
    const row = screen.getByText("gh-claude-5").closest("li")!;
    expect(within(row).getByText("silent")).toBeInTheDocument();
    expect(row.querySelector('[data-silent="true"]')).not.toBeNull();
  });

  it("shows an empty state", () => {
    render(<LanesTimeline lanes={[]} now={now} windowMs={1000} />);
    expect(screen.getByText("No agent has recorded work in this window")).toBeInTheDocument();
  });
});
