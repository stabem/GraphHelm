// #479: the owner read the Studio chat as code: identity lines, raw #numbers, shas and paths in
// one unbroken run. Regressions caught: the identity line shown as body text, no headline, a long
// body never folding, issue/PR numbers and journey ids that are not links, ids not set apart, and
// any path by which a message could inject markup or a non-http link. Cost: jsdom, no network.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";

import { fastUserEvent } from "../test/user-event";
import { SaidLinks, SaidText, splitIdentity } from "./said";

const userEvent = fastUserEvent();
afterEach(cleanup);

const links = { repoUrl: "https://github.com/stabem/GraphHelm", journeyIds: new Set(["studio-run-actions"]) };

function said(text: string, value: Parameters<typeof SaidLinks.Provider>[0]["value"] = links) {
  return render(<SaidLinks.Provider value={value}><div data-testid="said"><SaidText text={text} /></div></SaidLinks.Provider>);
}

describe("SaidText", () => {
  it("turns the identity line into a chip and the first line into a bold headline", () => {
    said("Session: gh-claude-4 · Head: 3da974f6c0ffee00 — #466 head moved to 44824644\nThe reviewer asked for two paths.");
    const chip = screen.getByTitle("Session gh-claude-4 · head 3da974f6c0ffee00");
    expect(chip).toHaveTextContent("gh-claude-4 · 3da974f6");
    expect(screen.getByTestId("said").textContent).not.toContain("Session:");
    const headline = screen.getByText((_, element) => element?.tagName === "STRONG" && element.textContent === "#466 head moved to 44824644");
    expect(headline.closest("p")).toHaveClass("said-headline");
    expect(screen.getByText("The reviewer asked for two paths.")).toBeInTheDocument();
  });

  it("reads an identity signed on the last line, without a head", () => {
    said("**Every lane: record your current task now.**\n- If you work on an issue, record claimed.\n\nSession: claude-coordinator");
    expect(screen.getByTitle("Session claude-coordinator")).toHaveTextContent(/^claude-coordinator$/);
    expect(screen.getByTestId("said").textContent).not.toContain("Session:");
    expect(screen.getByText("Every lane: record your current task now.").tagName).toBe("STRONG");
    expect(screen.getByTestId("said").textContent).not.toContain("**");
  });

  it("drops a signature that only repeats the message's own sender", () => {
    render(<SaidText text={"Done\nSession: claude-coordinator"} author="claude-coordinator" />);
    expect(screen.queryByTitle("Session claude-coordinator")).toBeNull();
    cleanup();
    render(<SaidText text={"Done\nSession: gh-claude-4 · Head: 3da974f6"} author="gh-claude-4" />);
    expect(screen.getByTitle("Session gh-claude-4 · head 3da974f6")).toBeInTheDocument();
  });

  it("links issues and PRs to the run's repository and opens a known journey", async () => {
    const onOpenJourney = vi.fn();
    said("Done\nCloses #465 and PR #466; watch studio-run-actions, not studio-unknown.", { ...links, onOpenJourney });
    expect(screen.getByRole("link", { name: "#465" })).toHaveAttribute("href", "https://github.com/stabem/GraphHelm/issues/465");
    expect(screen.getByRole("link", { name: "PR #466" })).toHaveAttribute("href", "https://github.com/stabem/GraphHelm/pull/466");
    await userEvent.click(screen.getByRole("button", { name: "studio-run-actions" }));
    expect(onOpenJourney).toHaveBeenCalledWith("studio-run-actions");
    expect(screen.queryByRole("button", { name: "studio-unknown" })).toBeNull();
  });

  it("makes no GitHub link without a repository", () => {
    said("Done\nCloses #465.", { repoUrl: null, journeyIds: new Set() });
    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.getByTestId("said")).toHaveTextContent("Closes #465.");
  });

  it("renders bullets, bold, inline code and links, and dims shas, paths and codes", () => {
    said("Result\n- **green**: `vitest` 858\n- see https://example.com/x and [the PR](https://github.com/stabem/GraphHelm/pull/1)\n- head 3da974f6 in apps/studio/src/App.tsx said drift.locator_missing");
    const items = within(screen.getByRole("list")).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(within(items[0]!).getByText("green").tagName).toBe("STRONG");
    expect(within(items[0]!).getByText("vitest").tagName).toBe("CODE");
    expect(within(items[1]!).getByRole("link", { name: "https://example.com/x" })).toHaveAttribute("href", "https://example.com/x");
    expect(within(items[1]!).getByRole("link", { name: "the PR" })).toHaveAttribute("rel", "noreferrer noopener");
    for (const dim of ["3da974f6", "apps/studio/src/App.tsx", "drift.locator_missing"]) {
      expect(within(items[2]!).getByText(dim)).toHaveClass("said-dim");
    }
  });

  it("folds a long body after six lines and unfolds it on request", async () => {
    const body = Array.from({ length: 10 }, (_, i) => `line ${i + 1}`).join("\n");
    said(`Headline\n${body}`);
    expect(screen.getByTestId("said")).toHaveTextContent("line 6");
    expect(screen.getByTestId("said")).not.toHaveTextContent("line 7");
    await userEvent.click(screen.getByRole("button", { name: "Show more" }));
    expect(screen.getByTestId("said")).toHaveTextContent("line 10");
    expect(screen.getByRole("button", { name: "Show less" })).toHaveAttribute("aria-expanded", "true");
  });

  it("never renders markup or a non-http link from a message", () => {
    said("Hi\n<img src=x onerror=alert(1)> <b>bold</b> [x](javascript:alert(1)) javascript:alert(2)");
    const root = screen.getByTestId("said");
    expect(root.querySelector("img, b, script")).toBeNull();
    expect(root.textContent).toContain("<img src=x onerror=alert(1)>");
    expect(screen.queryByRole("link")).toBeNull();
  });

  it("keeps a short one-line message whole and a long one as plain paragraphs", () => {
    said("Browser check incomplete: no screenshots supplied.");
    expect(screen.getByTestId("said")).toHaveTextContent("Browser check incomplete: no screenshots supplied.");
    expect(splitIdentity("no identity here")).toBeNull();
  });
});
