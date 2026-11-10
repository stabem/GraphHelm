import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { BoardTask } from "../runtime/client";
import { StepDots } from "./StepDots";
import { dotText, graphDots, graphStage, lanesDots, prLink } from "./step-dot";

// #2532: each dot's line comes from the records the board holds; a dot with no record says
// "not started" and carries no invented text.
const now = new Date("2026-11-05T12:00:00Z");
const task = (extra: Partial<BoardTask> = {}): BoardTask => ({
  key: "alpha-7", issue: 7, pr: null, lane: "alpha", currentReviewer: null, step: "implement", blockedBy: null,
  title: "task 7", prTitle: null, journeys: [], lastSequence: 12, clock: { since: "2026-11-05T11:50:00Z" }, ...extra,
} as BoardTask);

describe("the step dots of a Lanes card", () => {
  it("a done step names its lane; the current step names its lane, time and last record", () => {
    const dots = lanesDots(task(), "implement", false, null, now);
    expect(dots.map((dot) => dot.state)).toEqual(["done", "done", "in progress", "not started", "not started"]);
    expect(dots[0]!.text).toBe("plan · done · alpha · no record in view");
    expect(dots[2]!.text).toMatch(/^implement · in progress · alpha · since .+ · record #12$/);
  });

  it("a stuck current step says stalled, no reason given, and when it went silent", () => {
    const dots = lanesDots(task(), "implement", true, "2026-11-05T11:40:00Z", now);
    expect(dots[2]!.state).toBe("stalled");
    expect(dots[2]!.text).toMatch(/^implement · stalled · no reason given · silent since .+ · alpha · record #12$/);
  });

  it("a stalled step with no silent time says the time is unknown, not a guess", () => {
    expect(dotText("implement", "stalled", { cause: null, who: "alpha", when: null, record: null, silentSince: null, href: null }))
      .toBe("implement · stalled · no reason given · silent time unknown · alpha · no record in view");
  });

  it("a dot with no record says not started and nothing else", () => {
    const dots = lanesDots(task(), "implement", false, null, now);
    expect(dots[4]!.text).toBe("merge · not started");
    expect(dotText("review", "not started", { cause: null, who: "bravo", when: "now", record: "record #3", silentSince: null, href: null })).toBe("review · not started");
  });
});

describe("the step dots of a Graph issue card", () => {
  it("uses the card's stage: done before it, in progress at it, not started after", () => {
    const dots = graphDots(task(), 2, false, null, now);
    expect(dots.map((dot) => dot.state)).toEqual(["done", "done", "in progress", "not started", "not started"]);
  });

  it("a card in the Ready column shows its stage as stalled with the silent time", () => {
    const dots = graphDots(task(), 2, true, "2026-11-05T11:40:00Z", now);
    expect(dots[2]!.text).toContain("stalled · no reason given · silent since");
  });
});

describe("the step a Kanban card's dots stand on", () => {
  const states = (t: BoardTask, proven = false) => graphDots(t, graphStage(t, proven), false, null, now).map((dot) => dot.state);

  it("a card in review has three steps done, review in progress, merge not started", () => {
    expect(states(task({ step: "review" }))).toEqual(["done", "done", "done", "in progress", "not started"]);
  });

  it("a card implementing with a PR open stands on implement", () => {
    expect(states(task({ step: "implement", pr: 12 }))).toEqual(["done", "done", "in progress", "not started", "not started"]);
  });

  it("a card in plan stands on plan", () => {
    expect(states(task({ step: "plan" }))).toEqual(["in progress", "not started", "not started", "not started", "not started"]);
  });

  it("a merged card has every step done", () => {
    expect(states(task({ step: "merged" }))).toEqual(["done", "done", "done", "done", "done"]);
  });

  it("a proven card has every step done whatever its step", () => {
    expect(states(task({ step: "plan" }), true)).toEqual(["done", "done", "done", "done", "done"]);
  });
});

describe("the dot row", () => {
  it("each dot has an aria-label with the same text as its tooltip, and an empty dot is 'not started'", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={lanesDots(task(), "implement", false, null, now)} />);
    expect(html).toContain('aria-label="merge · not started"');
    expect(html).toContain('class="step-dot not-started"');
    expect(html).toContain('class="step-dot in-progress curstep"');
    expect(html).toMatch(/aria-label="implement · in progress · alpha · since [^"]+ · record #12"/);
    expect(html).not.toContain("<a ");
  });

  it("a dot with a link opens it", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={[{ step: "review", state: "done", text: "review · done · bravo", record: null, href: "https://example.test/pr/1" }]} />);
    expect(html).toMatch(/<a [^>]*href="https:\/\/example\.test\/pr\/1"[^>]*aria-label="review · done · bravo"/);
  });
});

describe("links, causes and focus (#2532 review)", () => {
  it("a PR link is built only from a GitHub owner/name and a PR number", () => {
    expect(prLink("https://github.com/stabem/GraphHelm", 2542)).toBe("https://github.com/stabem/GraphHelm/pull/2542");
    expect(prLink(undefined, 2542)).toBeNull();
    expect(prLink(null, 2542)).toBeNull();
    expect(prLink("https://github.com/stabem/GraphHelm", null)).toBeNull();
    expect(prLink("https://example.test/stabem/GraphHelm", 3)).toBeNull();
    expect(prLink("https://github.com/stabem/GraphHelm/extra", 3)).toBeNull();
    expect(prLink("https://github.com/stabem/GraphHelm", 0)).toBeNull();
  });

  it("a done or current dot with a PR and a repo links to the PR; a not-started dot does not", () => {
    const dots = lanesDots(task({ pr: 9, repoUrl: "https://github.com/stabem/GraphHelm" }), "implement", false, null, now);
    expect(dots[1]!.href).toBe("https://github.com/stabem/GraphHelm/pull/9");
    expect(dots[4]!.href).toBeNull();
  });

  it("a declared cause replaces 'no reason given' on a stalled dot", () => {
    expect(dotText("review", "stalled", { cause: "waiting on the reviewer", who: "bravo", when: null, record: null, silentSince: null, href: null }))
      .toContain("review · stalled · waiting on the reviewer · silent time unknown · bravo");
  });

  it("a non-https link is not rendered as a link, and the row has one tab stop", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={[
      { step: "plan", state: "done", text: "plan · done", record: null, href: "javascript:alert(1)" },
      { step: "implement", state: "in progress", text: "implement · in progress", record: null, href: null },
    ]} />);
    expect(html).not.toContain("<a ");
    expect(html.match(/tabindex="0"/g)).toHaveLength(1);
    expect(html).toContain("curstep");
  });
});

describe("the record in the tooltip", () => {
  it("the record is a link to the same target as the dot", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={[
      { step: "review", state: "done", text: "review · done · bravo · record #3", record: "record #3", href: "https://github.com/stabem/GraphHelm/pull/9" },
    ]} />);
    expect(html).toMatch(/<a [^>]*href="https:\/\/github\.com\/stabem\/GraphHelm\/pull\/9"[^>]*aria-label="review · done · bravo · record #3"/);
    expect(html).toContain('<a href="https://github.com/stabem/GraphHelm/pull/9" tabindex="-1">record #3</a>');
  });
});

describe("one focus target per dot (#2532 review 2)", () => {
  it("a linked dot is the link itself, one dot in the row takes focus, and no link sits inside aria-hidden", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={[
      { step: "plan", state: "done", text: "plan · done · alpha · record #4", record: "record #4", href: "https://github.com/stabem/GraphHelm/pull/9" },
      { step: "implement", state: "in progress", text: "implement · in progress · alpha · record #12", record: "record #12", href: "https://github.com/stabem/GraphHelm/pull/9" },
    ]} />);
    expect(html).not.toMatch(/aria-hidden="true"[^>]*>[^]*<a /);
    const focusable = html.match(/<a [^>]*tabindex="0"/g) ?? [];
    expect(focusable).toHaveLength(1);
    expect(html).toMatch(/<a [^>]*aria-label="plan · done · alpha · record #4"[^>]*tabindex="-1"/);
    expect(html).toContain('role="tooltip"');
    expect(html).not.toMatch(/role="tooltip" aria-hidden/);
  });

  it("each dot describes itself by its tooltip", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={[{ step: "merge", state: "not started", text: "merge · not started", record: null, href: null }]} />);
    const described = html.match(/aria-describedby="([^"]+)"/)![1]!;
    expect(html).toContain(`id="${described}"`);
  });
});


// #2548: one tab stop per row (roving tabindex); the arrow keys move focus between the dots.
describe("roving tabindex (#2548)", () => {
  const dots = [
    { step: "plan" as const, state: "done" as const, text: "plan · done", record: null, href: null },
    { step: "critic" as const, state: "done" as const, text: "critic · done", record: null, href: null },
    { step: "implement" as const, state: "in progress" as const, text: "implement · in progress", record: null, href: null },
  ];
  it("exactly one dot in a row is in the tab order: the live one, the rest are -1", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={dots} />);
    expect(html.match(/tabindex="0"/g)).toHaveLength(1);
    expect(html.match(/tabindex="-1"/g)).toHaveLength(2);
    expect(html).toMatch(/tabindex="0"[^>]*aria-label="implement · in progress"|aria-label="implement · in progress"[^>]*tabindex="0"/);
  });
  it("a row with no live dot leaves its first dot in the tab order", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={[
      { step: "plan", state: "done", text: "plan · done", record: null, href: null },
      { step: "critic", state: "done", text: "critic · done", record: null, href: null },
    ]} />);
    expect(html.match(/tabindex="0"/g)).toHaveLength(1);
    expect(html).toMatch(/tabindex="0"[^>]*aria-label="plan · done"|aria-label="plan · done"[^>]*tabindex="0"/);
  });
});
