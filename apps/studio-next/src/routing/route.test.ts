import { describe, expect, it } from "vitest";

import { parseRoute, routeSearch } from "./route";

describe("route", () => {
  it("drops the spent login ticket and keeps run and view", () => {
    expect(routeSearch(parseRoute("?session=studio-fixture&run=demo&view=proof"))).toBe("?run=demo&view=proof");
  });

  it("refuses a run id that is not an id and an unknown view", () => {
    expect(parseRoute("?run=../x&view=admin")).toEqual({ run: null, view: "graph" });
  });
});
