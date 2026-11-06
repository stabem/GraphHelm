import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { BoardTask } from "../runtime/client";
import { StepDots } from "./StepDots";
import { dotText, graphDots, lanesDots } from "./step-dot";

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
    expect(dotText("implement", "stalled", { who: "alpha", when: null, record: null, silentSince: null, href: null }))
      .toBe("implement · stalled · no reason given · silent time unknown · alpha · no record in view");
  });

  it("a dot with no record says not started and nothing else", () => {
    const dots = lanesDots(task(), "implement", false, null, now);
    expect(dots[4]!.text).toBe("merge · not started");
    expect(dotText("review", "not started", { who: "bravo", when: "now", record: "record #3", silentSince: null, href: null })).toBe("review · not started");
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

describe("the dot row", () => {
  it("each dot has an aria-label with the same text as its tooltip, and an empty dot is 'not started'", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={lanesDots(task(), "implement", false, null, now)} />);
    expect(html).toContain('aria-label="merge · not started"');
    expect(html).toContain('class="step-dot not-started"');
    expect(html).toContain('class="step-dot in-progress"');
    expect(html).toMatch(/aria-label="implement · in progress · alpha · since [^"]+ · record #12"/);
    expect(html).not.toContain("<a ");
  });

  it("a dot with a link opens it", () => {
    const html = renderToStaticMarkup(<StepDots label="Progress" dots={[{ step: "review", state: "done", text: "review · done · bravo", href: "https://example.test/pr/1" }]} />);
    expect(html).toContain('<a href="https://example.test/pr/1" aria-label="review · done · bravo">');
  });
});
