import { describe, expect, it } from "vitest";

import { parseRoute, routeSearch } from "./route";

describe("route", () => {
  it("drops the spent login ticket and keeps project, run and view", () => {
    const id = "a".repeat(64);
    expect(routeSearch(parseRoute(`?session=studio-fixture&project=${id}&run=demo&view=proof`))).toBe(`?project=${id}&run=demo&view=proof`);
  });

  it("refuses a project, a run id that is not an id and an unknown view", () => {
    expect(parseRoute("?project=../x&run=../x&view=admin")).toEqual({ project: null, run: null, view: "graph" });
  });
});
