import { describe, expect, it } from "vitest";
import { groupJourneys } from "./mission-view";

type J = { id: string; feature?: string | null; name: string };
const group = (items: J[]) => groupJourneys(items, (j) => j.feature, (j) => j.name).map((g) => [g.feature, g.items.map((j) => j.id)]);

describe("groupJourneys (#1296)", () => {
  it("groups by feature in name order, ungrouped last", () => {
    expect(group([
      { id: "d", name: "Mark delivered", feature: "Entregas" },
      { id: "x", name: "Old one" },
      { id: "c", name: "Create item", feature: "Itens" },
      { id: "a", name: "Copy message", feature: "Entregas" },
    ])).toEqual([["Entregas", ["a", "d"]], ["Itens", ["c"]], [null, ["x"]]]);
  });
  it("keeps one flat list in the old order when no journey has a feature", () => {
    expect(group([{ id: "b", name: "B" }, { id: "a", name: "A", feature: "  " }])).toEqual([[null, ["b", "a"]]]);
  });
});
