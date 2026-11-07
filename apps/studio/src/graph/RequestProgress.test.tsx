import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { RequestProgress } from "./RequestProgress";
import type { RequestProgress as Progress } from "./requests";

// #2529: a partly filled segment must not fade into the track the empty segments use. Each
// segment is its own slot with a track behind a fill, so a 0-of-N bar reads as N slots.
const progress: Progress = {
  chip: { key: "in_progress", label: "In progress", color: "#f5a524" },
  pct: "0%",
  done: 0,
  total: 3,
  segs: [
    { issue: 101, state: "implementing", color: "#f5a524", fill: 0.3 },
    { issue: 102, state: "stalled", color: "#e5484d", fill: 0 },
    { issue: 103, state: "not started", color: "#5b5b63", fill: 0 },
  ],
  legend: [],
};

describe("the request progress bar", () => {
  it("draws one slot per issue, each with its own track", () => {
    const html = renderToStaticMarkup(<RequestProgress progress={progress} />);
    expect(html.match(/title="#\d+ /g)).toHaveLength(3);
    expect(html).not.toContain("transparent");
  });

  it("fills a partial segment from its own track, not from a transparent gradient", () => {
    const html = renderToStaticMarkup(<RequestProgress progress={progress} />);
    expect(html).toContain('class="req-fill"');
    expect(html).toMatch(/class="req-fill"[^>]*width:30%/);
  });

  // #2534: a stalled segment and a not-started segment, both empty, differ on the bar itself.
  it("marks a stalled slot apart from a not-started one, on the bar and not only in the legend", () => {
    const html = renderToStaticMarkup(<RequestProgress progress={{
      ...progress,
      segs: [
        { issue: 201, state: "stalled", color: "#ff6b6b", fill: 0 },
        { issue: 202, state: "not started", color: "#5b5b63", fill: 0 },
      ],
    }} />);
    expect(html).toContain('class="req-slot stalled" title="#201 stalled"');
    expect(html).toContain('class="req-slot" title="#202 not started"');
  });
});
