import { describe, expect, it } from "vitest";

import stylesheet from "../styles.css?raw";

/** The declarations every rule gives a selector (last one wins), read from the shipped stylesheet. */
function declared(selector: string): Record<string, string> {
  const css = stylesheet.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: Record<string, string> = {};
  for (const rule of css.matchAll(/([^{}@]+)\{([^{}]*)\}/g)) {
    if (!rule[1]!.split(",").some((each: string) => each.trim() === selector)) continue;
    for (const part of rule[2]!.split(";")) {
      const at = part.indexOf(":");
      if (at > 0) out[part.slice(0, at).trim()] = part.slice(at + 1).trim();
    }
  }
  return out;
}

describe("a step card's label wraps instead of cutting (#2028)", () => {
  it("wraps to at most 3 lines at line-height 1.35, breaking long words, with no ellipsis", () => {
    const label = declared(".step-label");
    expect(label).toMatchObject({ "-webkit-line-clamp": "3", "line-clamp": "3", "line-height": "1.35", "overflow-wrap": "anywhere", overflow: "hidden" });
    expect(label["white-space"]).toBeUndefined();
    expect(label["text-overflow"]).toBeUndefined();
  });

  it("boxes what the step checked: 2px in the result colour, radius 4, dark halo, over the frame", () => {
    expect(declared(".check-mark")).toMatchObject({ position: "absolute", border: "2px solid var(--faint)", "border-radius": "4px", "box-shadow": "0 0 0 2px #0a0a0bcc" });
    expect(declared(".check-marks.passed .check-mark")["border-color"]).toBe("#3ddc97");
    expect(declared(".check-marks.failed .check-mark")["border-color"]).toBe("#ff6b6b");
    expect(declared(".step-thumb")["position"]).toBe("relative");
    expect(declared(".check-marks")).toMatchObject({ position: "absolute", inset: "0" });
  });
});

describe("the step frame shows the whole screenshot (#1966)", () => {
  it.each([".compare-frame img", ".phone img"])("%s scales to its box width, keeps its aspect ratio and is never cropped", (selector) => {
    const image = declared(selector);
    expect(image["width"]).toBe("100%");
    expect(image["height"]).toBe("auto");
    expect(image["max-width"]).toBe("100%");
    expect(image["object-fit"]).toBe("contain");
  });

  it("on desktop the frame is bounded by the box it lives in, down from one viewport-tall shell (#1971)", () => {
    const shell = ".shell:has(.compare-frame:not(.terminal))";
    expect(declared(shell)["height"]).toMatch(/100d?vh/);
    expect(declared(`${shell} .proof-main`)["overflow-y"]).toBe("auto");
    for (const box of [`${shell} .main`, `${shell} .proof`, `${shell} .proof-main`]) expect(declared(box)["min-height"]).toBe("0");
    expect(declared(`${shell} .compare`)).toMatchObject({ flex: "1 1 0", "min-height": "320px" });
    expect(declared(".compare-frame:not(.terminal) > img")).toMatchObject({ flex: "1 1 0", "min-height": "0", height: "100%" });
    expect(declared(".compare-frame:not(.terminal) .compare-side img")).toMatchObject({ flex: "1 1 0", "min-height": "0" });
    expect(declared(".compare-frame:not(.terminal) .compare-overlay img")).toMatchObject({ position: "absolute", inset: "0" });
    expect(stylesheet).not.toMatch(/calc\(100d?vh\s*-\s*\d+px\)/);
  });

  it("the side panel scrolls inside its own column so no row is clipped (#1978)", () => {
    expect(declared(".shell:has(.compare-frame:not(.terminal)) .compare-side-panel")).toMatchObject({ "min-height": "0", "overflow-y": "auto" });
  });

  it("the phone box grows with the frame and gives it no scroll of its own", () => {
    const phone = declared(".phone");
    expect(phone["height"]).toBe("auto");
    expect(phone["overflow"]).toBe("hidden");
  });

  it("an overlay lays Now exactly over Before at the same size", () => {
    expect(declared(".compare-overlay img + img")).toMatchObject({ position: "absolute", left: "0", top: "0", height: "100%" });
  });
});
