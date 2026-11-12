import { describe, expect, it } from "vitest";

import type { JourneyFlow } from "../runtime/client";
import { filterJourneys, groupJourneys } from "./journey-search";

// #2845: the Proof column's search. A journey matches when the trimmed, lower-cased query is a plain
// substring of the name it is shown by, its id, or its group's name.
const flow = (id: string, over: Partial<JourneyFlow> = {}) => ({ id, title: null, name: null, feature: null, ...over }) as JourneyFlow;
const flows = [
  flow("studio2-lanes-board", { name: "See the lanes", title: "See every agent on the Lanes board", feature: "Studio" }),
  flow("studio2-proof-run", { name: "Run a journey", feature: "Studio" }),
  flow("keel-card-accepted", { title: "Accept in-scope work", feature: "Keel" }),
  flow("skill-driven-delivery"),
];
const ids = (list: readonly JourneyFlow[]) => list.map((entry) => entry.id);

describe("filterJourneys", () => {
  it("returns every journey for an empty or whitespace-only query", () => {
    expect(filterJourneys(flows, "")).toBe(flows);
    expect(ids(filterJourneys(flows, "   "))).toEqual(ids(flows));
  });
  it("matches the display name, the id, and the group name", () => {
    expect(ids(filterJourneys(flows, "a journey"))).toEqual(["studio2-proof-run"]);
    expect(ids(filterJourneys(flows, "proof-run"))).toEqual(["studio2-proof-run"]);
    expect(ids(filterJourneys(flows, "keel"))).toEqual(["keel-card-accepted"]);
    expect(ids(filterJourneys(flows, "studio"))).toEqual(["studio2-lanes-board", "studio2-proof-run"]);
  });
  it("shows the title when there is no name, and does not search the title of a named journey", () => {
    expect(ids(filterJourneys(flows, "in-scope"))).toEqual(["keel-card-accepted"]);
    expect(ids(filterJourneys(flows, "every agent"))).toEqual([]);
  });
  it("ignores case and surrounding spaces", () => {
    expect(ids(filterJourneys(flows, "  SEE THE LANES "))).toEqual(["studio2-lanes-board"]);
  });
  it("returns nothing when nothing matches, and reads the query as text, not a pattern", () => {
    expect(filterJourneys(flows, "billing")).toEqual([]);
    expect(filterJourneys(flows, ".*")).toEqual([]);
    expect(filterJourneys(flows, "(")).toEqual([]);
  });
  it("matches a journey with no name, title or group by its id only", () => {
    expect(ids(filterJourneys(flows, "driven"))).toEqual(["skill-driven-delivery"]);
    expect(ids(filterJourneys(flows, "other"))).toEqual(["skill-driven-delivery"]);
    expect(ids(filterJourneys(flows, "billing"))).toEqual([]);
  });
});

describe("groupJourneys", () => {
  it("groups by feature, an unlabelled journey under Other, and drops a group with no shown journey", () => {
    expect(groupJourneys(flows).map(([group, list]) => [group, ids(list)])).toEqual([
      ["Studio", ["studio2-lanes-board", "studio2-proof-run"]],
      ["Keel", ["keel-card-accepted"]],
      ["Other", ["skill-driven-delivery"]],
    ]);
    expect(groupJourneys(filterJourneys(flows, "keel")).map(([group]) => group)).toEqual(["Keel"]);
  });
});
