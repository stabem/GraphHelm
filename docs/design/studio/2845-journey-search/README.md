# Search the journeys in Proof (#2845)

Static before/after mocks of the Proof view's journeys column (`aside.proof-journeys`), drawn from the real Studio on the journey fixture (27 journeys).

- `before.html` / `before.png`: today. Header, `Approve N` / `Run all`, the two count lines, then the grouped list. No way to find one journey except scrolling.
- `after.html` / `after-empty.png`: proposed. A `Search journeys` field directly above the list and below the count lines. Empty: the list is exactly as today; no status line, no clear button.
- `after-typed.png`: the query `review` typed. The list shows only the matching journeys, the groups with none disappear, a clear button `×` sits inside the field, and a status line `3 of 27 journeys` sits under it.
- `after-none.png`: the query `billing`. The status line reads `0 of 27 journeys` and the line `No journey matches “billing”.` replaces the list.
- `real-empty.png`, `real-typed.png`, `real-none.png`: the built Studio in the same three states (query `review`, then `billing`), captured headless on the journey fixture. Evidence of the implementation, not part of the approved design; the fixture has 29 journeys and different card states than the mock's, and the focused field shows the browser's default focus ring.

What the mock fixes: the filter matches the trimmed, case-insensitive query as a substring of a journey's display name, its id or its group's name. `Run all` and `Approve N` are not filtered (the mock's labels and counts do not change). The selected journey stays selected when its card is filtered out. The header keeps the total. The field takes the project-folder input's look (card background, `--edge-strong` border, 6px radius, `--fs-meta`); no new colour, font or token.

Known mock defect (told to the owner): in `after-none.png` the card of an invalid journey (`skill-driven-delivery`) may still be drawn, because the mock script could not read its name. The implementation hides it like any other journey that does not match.

Decided where the mock shows nothing: Escape in the field clears it and keeps focus there; the clear button returns focus to the field; the query is not stored (not in the URL or the browser), so a reload or another project starts empty; the page tools still find every journey, filtered or not; the field is a real `type=search` input limited to 80 characters and the query is shown as text only.

Approved by Gabriel (owner), 2026-11-11, on `before.png`, `after-empty.png`, `after-typed.png` and `after-none.png`.
Owner approval on GitHub: https://github.com/stabem/GraphHelm/issues/2845#issuecomment-6810835503.
