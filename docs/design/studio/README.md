# Studio design

These files are the owner-approved design of the Studio, the screens that the Studio v2 rebuild (#1790)
implements. The live, editable copy is the design artifact:
https://claude.ai/artifact/6zmUpDHeLfSo75ZRmVvSm5

| File | Screen |
|---|---|
| `Sidebar.dc.html` | Shared left menu: projects, their runs and journeys, runtime notices |
| `Topbar.dc.html` | Shared top bar: project, run, Graph / Lanes / Proof, Start a team run |
| `Graph.dc.html` | Work by request: owner requests, the issues and PRs opened for them, review modal with Before / Now |
| `Lanes.dc.html` | Agent control: agents by state, idle time, questions waiting for a reply, needs-action queue, capacity, live feed |
| `Proof.dc.html` | Journeys column, run bar with setup log, step frames, Before / Now compare, web and mobile |
| `canvas.json` | Layout of the canvas the files come from |
| `frame-sample.webp` | Sample captured frame used by the mock-ups |

The `.dc.html` files use the artifact's Design Component format, and their images point at the
artifact's asset store (`/_blob/...`). They open in the artifact, not as plain pages. The data on
them is illustrative.

## How a visual change is made

1. Change the design artifact first, and keep it built on the current screen rather than redrawn
   from nothing.
2. The owner approves it there.
3. Copy the approved files into this folder in the same pull request that implements the change,
   and link the artifact in the issue.

A visual Studio change that has no approved design here is not merged.
