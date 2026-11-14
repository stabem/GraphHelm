# Failed-action alert and review compare text: fixes B and D (#2946, #2947)

Approved by Gabriel (owner) on 2026-11-14 in the session chat: shown the before/after mock at
<https://claude.ai/artifact/2Q86aajfWGRfgda5wQt7Bd> and asked "Aprova os quatro desenhos (A, B, C,
D)?", the answer was "Aprovo os quatro". This record covers fixes B and D of that mock; A and C are
delivered elsewhere.

## Where

- Fix B (#2946): the shared failed-action alert of [#2445](../2445/README.md), in any panel that
  shows it, most visibly in a narrow column.
- Fix D (#2947): the review modal's compare views (Changes and Overlay). The text is not visible on
  screen: a screen reader reads it, and one approved journey checks it.

## Before

| Part | What it did |
|---|---|
| Alert, code line | With no Runtime code the line read "Code not supplied". |
| Alert, Dismiss | In a narrow column the button shrank and its word broke over three lines. |
| Compare, Changes | Hidden text: "Now layer drawn with blend difference, opacity 1" |
| Compare, Overlay at 50% | Hidden text: "Now layer drawn with blend normal, opacity 0.5" |
| Compare, Overlay at 25% | Hidden text: "Now layer drawn with blend normal, opacity 0.25" |

## After

| Part | What it does |
|---|---|
| Alert, code line | With a code the line is exactly as before. With none, no line is drawn. |
| Alert, Dismiss | Never wraps and never shrinks; the title wraps instead. |
| Compare, Changes | "Showing only what changed" |
| Compare, Overlay at 50% | "Now shown at 50% over Before" |
| Compare, Overlay at 25% | "Now shown at 25% over Before" |

The compare text is still read from the style the browser applied to the Now layer, never from the
requested state. Blend `difference` gives the first sentence; blend `normal` at opacity p gives the
second, with p as a whole percent. Any other applied blend keeps a sentence that names it.

## Unchanged

The alert's colours, spacing, title, cause, next step and Retry. The compare's layout, the visible
figure label, the slider and the modes. Only the words and the Dismiss sizing change.
