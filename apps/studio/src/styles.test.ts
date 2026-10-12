/**
 * Guards on the stylesheet itself.
 *
 * WHY THESE EXIST. A scripted edit to `styles.css` left 181 opening braces against 182 closing
 * ones. A stray `}` closes a rule early and the parser drops everything after it up to the next
 * selector - silently, with no error anywhere - and in the same edit `.blob` lost its base rule,
 * leaving an element with a colour and no size. Both shipped. All 127 tests stayed green, because
 * not one of them read the stylesheet.
 *
 * None of what follows is a matter of taste. Braces balance or they do not; a `var()` names a
 * token that exists or it does not; a class a component renders has a rule or it does not. These
 * are the mechanical half of "the interface is not broken", and the mechanical half is the half a
 * test can hold.
 *
 * What they deliberately do NOT check is whether anything LOOKS right. jsdom lays nothing out, so
 * a clipped control and a buried block are invisible here exactly as they were before. That gap is
 * real and is not closed by this file.
 */

import { describe, expect, it } from "vitest";

// Components may import their own stylesheet. Discover those beside the global stylesheet using
// Vite's raw imports, just as componentSource discovers components. Keep structural checks per
// file: an extra opening brace in one stylesheet cannot cancel a closing brace in another.
const STYLESHEETS = import.meta.glob<string>("./**/*.css", {
  query: "?raw", import: "default", eager: true,
});
const CSS = Object.values(STYLESHEETS).join("\n");

/** Comments are stripped before any structural count: prose legitimately contains braces, and a
 * sentence about `{` must not read as a rule. */
const CODE = CSS.replace(/\/\*[\s\S]*?\*\//g, "");

/**
 * Custom properties this stylesheet READS but deliberately does not DEFINE, each with the reason.
 * A bare allowlist would let a typo in a token name pass as intentional.
 */
const SET_ELSEWHERE: Record<string, string> = {
  "--rail": "App.tsx writes it inline from the operator's dragged width; the CSS reads it with a fallback",
  "--dock-reserve": "App.tsx writes it inline on .scene from the docks' measured height (#1083 F9); the CSS reads it with a fallback",
  "--bot-hue": "team-canvas.tsx writes it inline on each .team-bot from the bot persona hue; the CSS reads it for the avatar, outline and pulse",
};

/** Classes the components render that intentionally have no rule of their own. Each entry names
 * why the wrapper remains readable through its styled descendants or an ancestor rule. */
const NO_RULE_NEEDED: Record<string, string> = {};

// #86: these handoff-tree classes are included by the component/CSS globs below:
// delegation-tree, delegation-note, delegation-roots, delegation-label, delegation-source.

/**
 * Every component's source, as text.
 *
 * `import.meta.glob` is resolved by Vite at build time, so the set is fixed when this test is
 * compiled - a component added later is picked up on the next run without anyone listing it here,
 * and a component deleted cannot leave a stale path behind.
 */
function componentSource(): string {
  const modules = import.meta.glob("./**/*.tsx", { query: "?raw", import: "default", eager: true });
  return Object.values(modules).join("\n");
}

describe.each(Object.entries(STYLESHEETS))("stylesheet %s parses at all", (_path, source) => {
  const code = source.replace(/\/\*[\s\S]*?\*\//g, "");
  /** The one that shipped. An unbalanced brace is not a style opinion - it is a file the browser
   * reads differently from how it was written. */
  it("balances its braces", () => {
    const open = (code.match(/\{/g) ?? []).length;
    const close = (code.match(/\}/g) ?? []).length;
    expect(
      { open, close },
      "a stray brace closes a rule early and the parser silently drops everything after it",
    ).toEqual({ open: close, close });
  });

  /** Where the imbalance is, so the failure above is actionable rather than a number. */
  it("never closes a rule that was not open", () => {
    let depth = 0;
    const offenders: string[] = [];
    code.split("\n").forEach((line, index) => {
      for (const character of line) {
        if (character === "{") depth += 1;
        else if (character === "}") {
          depth -= 1;
          if (depth < 0) {
            offenders.push(`line ${index + 1}: ${line.trim()}`);
            depth = 0;
          }
        }
      }
    });
    expect(offenders, "these braces close a rule that was never opened").toEqual([]);
  });
});

describe("every token is real", () => {
  const defined = new Set(
    [...CSS.matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map((match) => match[1] as string),
  );
  const used = new Set([...CSS.matchAll(/var\((--[a-z0-9-]+)/g)].map((match) => match[1] as string));

  /** A `var()` naming nothing renders as nothing, which on a background or a colour is an
   * invisible element rather than a visible error. */
  it("defines every custom property it reads", () => {
    const missing = [...used].filter((token) => !defined.has(token) && !(token in SET_ELSEWHERE));
    expect(missing, "these are read by a rule and defined by nobody").toEqual([]);
  });

  /** A token defined and never read is a claim the file cannot keep - twice already, the header
   * described a scale step or a meaning colour that nothing used. */
  it("reads every custom property it defines", () => {
    const unused = [...defined].filter((token) => !used.has(token));
    expect(unused, "these are defined and never read; use them or drop them with their claim").toEqual([]);
  });
});

describe("issue graph cell labels stay whole", () => {
  it("keeps the stage label full width so the mark gives up space first", async () => {
    const css = (await import("./components/mission-graph.css?raw")).default as string;
    const label = css.split("\n").find((line) => line.startsWith(".mg-cell-label {")) ?? "";
    expect(label).toMatch(/flex: none/);
    expect(label).not.toMatch(/text-overflow: ellipsis|overflow: hidden/);
  });
});

describe("the Team tab is a column that reserves the dock (#481)", () => {
  /** The owner could not scroll the Team tab past ~11 task rows: the list was clipped under Run
   * actions. Text-level only, like #435's cells; the 1440x900 viewport run is the PR's observer. */
  const rule = (selector: string) => {
    const at = CODE.indexOf(`${selector} {`);
    return at === -1 ? "" : CODE.slice(at + selector.length + 2, CODE.indexOf("}", at));
  };
  it("lays the tab panel out as a flex column with the dock's cover reserved below it", () => {
    const panel = rule("#studio-panel-team");
    expect(panel).toMatch(/display:\s*flex/);
    expect(panel).toMatch(/flex-direction:\s*column/);
    expect(panel).toMatch(/padding-bottom:\s*var\(--dock-reserve/);
    expect(rule("#studio-panel-team[hidden]")).toMatch(/display:\s*none/);
  });
  it("scrolls the task list in its own box and gives the team canvas the rest", () => {
    const list = rule("#studio-panel-team > .task-graphs");
    expect(list).toMatch(/overflow-y:\s*auto/);
    expect(list).toMatch(/max-height:/);
    expect(rule("#studio-panel-team > .team-canvas")).toMatch(/flex:\s*1/);
  });
});

describe("the Lanes kanban fits six columns without a minimum-width overflow (#759)", () => {
  it("lets the six columns shrink to the available board width", () => {
    expect(STYLESHEETS["./components/work-kanban.css"])
      .toMatch(/\.wk\s*\{[^}]*grid-template-columns:\s*repeat\(6,\s*minmax\(0,\s*1fr\)\)[^}]*min-width:\s*0/s);
  });
  it("keeps card titles within two lines before truncating", () => {
    const kanban = STYLESHEETS["./components/work-kanban.css"];
    expect(kanban).toMatch(/\.wk \.mg-node-title\s*\{[^}]*-webkit-line-clamp:\s*2[^}]*overflow:\s*hidden/s);
  });
});

describe("the issue graph row title stays inside its five columns (#796)", () => {
  it("bounds the title to the grid while keeping the full task title available", () => {
    const graph = STYLESHEETS["./components/mission-graph.css"];
    expect(graph).toMatch(/\.mg-rowtitle\s*\{[^}]*width:\s*calc\(\(100%\s*-\s*48px\)\s*\/\s*5\)[^}]*white-space:\s*nowrap[^}]*overflow:\s*hidden[^}]*text-overflow:\s*ellipsis/s);
  });
});

describe("the stylesheet and the components agree", () => {
  /** The second half of the same defect: `.blob` survived as modifiers with no base rule, so the
   * element had a colour and no size and would have rendered as nothing. */
  /**
   * ONLY THE STATIC HALF, and that limit is the point. A `className={`node ${mood}`}` carries a
   * real class and a modifier chosen at runtime; reading identifiers out of the `${...}` yields
   * VARIABLE NAMES - `tone`, `tool` - and reports them as missing rules. A guard that cries wolf
   * is a guard somebody switches off, so this reads the literal text and nothing else.
   *
   * What it therefore cannot see: a class that only ever exists as an interpolated value. That is
   * a declared hole, not an oversight - the defect this exists for (`.blob` losing its base rule)
   * was a plain string, and so is every class that names an element rather than a state.
   */
  it("has a rule for every class the components render", () => {
    const source = componentSource();
    const rendered = new Set(
      [...source.matchAll(/className=(?:"([^"]*)"|\{`([^`]*)`\})/g)]
        // Drop the interpolations, keep the literal text around them.
        .flatMap((match) => (match[1] ?? match[2] ?? "").replace(/\$\{[^}]*\}/g, " ").split(/\s+/))
        // A fragment ending in `-` is a PREFIX (`tool-${tool}`), never a whole class name.
        .filter((name) => /^[a-z][a-z0-9-]*$/.test(name) && !name.endsWith("-")),
    );
    const orphans = [...rendered].filter(
      (name) => !(name in NO_RULE_NEEDED) && !new RegExp(`\\.${name}[\\s,{:.]`).test(CSS),
    );
    expect(orphans, "these classes are rendered and styled by nothing").toEqual([]);
  });

  /**
   * EVERY ELEMENT IS STYLED BY SOMETHING THAT ALWAYS APPLIES.
   *
   * The check above only asks whether a class is MENTIONED, and that is not enough. When `.blob`
   * lost its base rule it kept three modifiers - `.node.waiting .blob { background }` and two
   * siblings - so it was still mentioned everywhere while having a colour and no size, which
   * renders as nothing at all. The sabotage walked straight past the guard written for it.
   *
   * The property that separates that from healthy code is CONDITIONALITY, not shape. `.blob`'s
   * every rule sat under a compound carrying a state (`.node.waiting`), so in the ordinary case -
   * a node that is not waiting - nothing styled it. `.topstrip .run-name` and `.composer .send`
   * are descendants of a structural ancestor and always apply; requiring those to own a base rule
   * as well would report four healthy elements and teach the reader to ignore this test.
   *
   * So: a class rendered on its own must have at least one rule that is unconditional - a base
   * rule, or a descendant rule whose ancestors are plain elements rather than states.
   */
  it("styles every solo class with a rule that always applies", () => {
    const source = componentSource();
    const solo = new Set(
      [...source.matchAll(/className="([a-z][a-z0-9-]*)"/g)].map((match) => match[1] as string),
    );

    /** Every selector in the file, one per rule, comments already gone. */
    const selectors = [...CODE.matchAll(/(^|\})\s*([^{}]+?)\s*\{/g)].map((match) =>
      (match[2] as string).replace(/\s+/g, " ").trim(),
    );

    /** `.a.b` - two classes on one element - is a STATE compound: it applies only sometimes. */
    const conditional = (selector: string) => /\.[a-z0-9-]+\.[a-z0-9-]+/.test(selector);

    const unstyled = [...solo].filter((name) => {
      if (name in NO_RULE_NEEDED) return false;
      const mentions = selectors.filter((selector) =>
        new RegExp(`\\.${name}(?![a-z0-9-])`).test(selector),
      );
      return !mentions.some((selector) => !conditional(selector));
    });

    expect(
      unstyled,
      "every rule for these sits under a state, so in the ordinary case nothing styles them",
    ).toEqual([]);
  });

  it("keeps the top-bar title on one line with an ellipsis", () => {
    const rule = (CSS.match(/\.topbar-mission \.run-name\s*\{([^}]*)\}/g) ?? []).join(" ");
    expect(rule).toMatch(/white-space:\s*nowrap/);
    expect(rule).toMatch(/text-overflow:\s*ellipsis/);
    expect(rule).toMatch(/overflow:\s*hidden/);
    expect(CSS.match(/\.topbar-mission\s*\{[^}]*\}/)?.[0]).toMatch(/flex-wrap:\s*nowrap/);
  });
  /** The first rule whose selector is exactly `selector`, comments stripped. */
  const ruleOf = (selector: string) => {
    const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return CODE.match(new RegExp(`(?:^|[}\\s])${escaped}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
  };

  // #325: the page scrolled 1633px past a 1064px window because `.sr-only` labels (position:
  // absolute) in the scrolled run list had no positioned ancestor, so their containing block was
  // the initial one and each label far down the list extended the document.
  it("keeps absolutely positioned labels inside the shell and inside the run list (#325)", () => {
    expect(ruleOf(".sr-only")).toMatch(/position:\s*absolute/);
    expect(ruleOf(".app")).toMatch(/position:\s*relative/);
    expect(ruleOf(".app")).toMatch(/overflow:\s*hidden/);
    expect(ruleOf(".projects")).toMatch(/position:\s*relative/);
  });

  it("keeps the Chat column below the top bar at narrow viewport widths (#797)", () => {
    expect(CODE).toMatch(/\.chat-col\s*\{[^}]*grid-row:\s*2\s*\/\s*3/);
  });

  it("keeps the selected-run identity on one line instead of one glyph per row (#325)", () => {
    const rule = ruleOf(".run-selection-identity");
    expect(rule).not.toMatch(/flex-basis:\s*100%/);
    expect(rule).toMatch(/white-space:\s*nowrap/);
    expect(rule).toMatch(/text-overflow:\s*ellipsis/);
  });

  it("keeps the project backdrop out of desktop layout flow (#891)", () => {
    const stylesheet = STYLESHEETS["./styles.css"]?.replace(/\/\*[\s\S]*?\*\//g, "") ?? "";
    const desktopRules = stylesheet.split("@media (max-width: 900px)")[0] ?? "";
    expect(desktopRules).toMatch(/\.projects-backdrop\s*\{[^}]*display:\s*none/s);
  });
});

describe("Proof progress text stays readable in narrow cards", () => {
  it("allows the line to wrap between count phrases", () => {
    const rule = (CSS.match(/\.mv-progress-text\s*\{([^}]*)\}/g) ?? []).join(" ");
    expect(rule).not.toMatch(/white-space:\s*nowrap/);
  });
});

describe("the Graph WORK rail fits a narrow viewport (#884)", () => {
  it("lets the rail and its group cards shrink to the available width", () => {
    const mission = STYLESHEETS["./components/mission-view.css"];
    expect(mission).toMatch(/\.mv-journeys\s*\{[^}]*max-width:\s*100%/s);
    expect(mission).toMatch(/\.mv-groups\s*\{[^}]*max-width:\s*100%/s);
  });

  it("keeps the mobile graph inside the viewport and scrollable within its box", () => {
    const mission = STYLESHEETS["./components/mission-view.css"];
    const graph = STYLESHEETS["./components/mission-graph.css"];
    expect(mission).toMatch(/\.mv-body\s*\{[^}]*min-width:\s*0/s);
    expect(mission).toMatch(/\.mv\s+\.mg-graph\s*\{[^}]*width:\s*100%[^}]*max-width:\s*100%/s);
    expect(graph).toMatch(/\.mg-scroll\s*\{[^}]*overflow-x:\s*auto/s);
  });

  it("lets the mobile project title yield to the attention pill", () => {
    expect(CSS).toMatch(/\.topbar-mission\s+\.run-project\s*\{[^}]*min-width:\s*0[^}]*overflow:\s*hidden[^}]*text-overflow:\s*ellipsis/s);
  });

  it("gives the mobile run title its own readable row", () => {
    expect(CSS).toMatch(/@media\s*\(max-width:\s*600px\)\s*\{[^}]*\.topbar-mission\s*\{[^}]*flex-basis:\s*100%/s);
    expect(CSS).toMatch(/@media\s*\(max-width:\s*600px\)\s*\{\s*\.topbar-mission\s*\{[^}]*flex-basis:\s*100%[^}]*\}\s*\.topbar-mission\s+\.run-project\s*\{[^}]*flex-shrink:\s*0/s);
    expect(CSS).toMatch(/\.topbar-mission\s+\.run-selection-identity\s*\{[^}]*flex-basis:\s*100%/s);
  });
});
