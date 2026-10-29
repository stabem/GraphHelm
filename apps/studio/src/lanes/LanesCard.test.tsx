import { describe, expect, it } from "vitest";
import type { ReactElement } from "react";

import { Card } from "./LanesView";
import type { Agent } from "./board";

describe("agent card selection (#2233)", () => {
  it("a click anywhere on the card picks its lane, not only the name button", () => {
    const agent = { lane: "openbot-ben", column: "implement", task: null, questions: [], moreQuestions: 0, idleMinutes: 3, stepMinutes: null, lastRecordAt: null, kind: null, model: null } as unknown as Agent;
    const picks: string[] = [];
    const card = Card({ agent, origins: ["zuvemo-desenvolvimento-ops"], selected: false, serves: null, stripe: null, unlinked: false, last: undefined, onPick: (lane) => picks.push(lane) }) as ReactElement<{ onClick?: () => void }>;
    expect(card.type).toBe("li");
    card.props.onClick?.();
    expect(picks).toEqual(["openbot-ben"]);
  });
});
