import { describe, expect, it } from "vitest";

import type { OwnerRequest } from "../graph/requests";
import type { BoardTask } from "../runtime/client";
import { askedCards, askedWhen, requestTitles } from "./asked";
import type { Agent } from "./board";

const request = (requestId: string, issues: number[], extra: Partial<OwnerRequest> = {}): OwnerRequest => ({
  requestId, ask: `ask ${requestId}`, title: null, recordedBy: "owner", sequence: 1, status: "in_progress",
  issues: issues.map((issue) => ({ issue, slices: [] })), ...extra,
});
const agent = (lane: string, issue: number | null, extra: Partial<Agent> = {}): Agent => ({
  lane, column: "implement", idleMinutes: 2, lastRecordAt: null, kind: null, model: null, stepMinutes: null, questions: [], moreQuestions: 0,
  task: issue === null ? null : ({ key: lane, issue, step: "implement" } as BoardTask), ...extra,
});

const slice = (lane: string, issue: number, extra: Partial<BoardTask> = {}) => ({ key: `${lane}-${issue}`, lane, issue, step: "implement", ...extra }) as BoardTask;

describe("what I asked", () => {
  it("lists the newest request first with its title, date and the agents on its issues", () => {
    const asking = { lane: "b", question: "?", waitsOn: "studio-operator", since: "2026-10-22T10:00:00Z", signalId: "s", sequence: 1, laneTotal: 1 };
    // "a" shows another slice on its card but still holds one on #7; "f" released its #9 slice.
    const tasks = [slice("a", 7), slice("a", 99), slice("b", 8), slice("c", 9), slice("d", 9), slice("f", 9, { releasedBy: "graphhelm" })];
    const cards = askedCards(
      [request("r1", [7], { title: "Old ask" }), request("r2", [8, 9], { recordedAt: "2026-10-21T18:30:00" })],
      [agent("a", 99), agent("b", 8, { questions: [asking] }), agent("c", 9, { column: "merge" }), agent("d", 9, { idleMinutes: 45 }), agent("e", null), agent("f", null)],
      tasks,
    );
    expect(cards.map((card) => card.requestId)).toEqual(["r2", "r1"]);
    expect(cards[0]).toMatchObject({ title: "ask r2", when: "21 Oct 18:30", summary: "3 agents · needs a reply" });
    expect(cards[0]!.agents).toEqual([{ lane: "b", tone: "asks" }, { lane: "c", tone: "merge" }, { lane: "d", tone: "quiet" }]);
    expect(cards[1]).toMatchObject({ title: "Old ask", when: null, summary: "1 agent" });
  });

  it("names on each card the newest request its lane serves (#1904)", () => {
    const tasks = [slice("a", 7), slice("b", 8), slice("b", 7)];
    const titles = requestTitles([request("r1", [7], { title: "Old ask" }), request("r2", [8])], [agent("a", 7), agent("b", 8), agent("c", null)], tasks);
    expect([...titles]).toEqual([["b", "ask r2"], ["a", "Old ask"]]);
  });

  it("leaves an absent or unreadable time undated", () => {
    expect(askedWhen(undefined)).toBeNull();
    expect(askedWhen("not a time")).toBeNull();
  });
});
