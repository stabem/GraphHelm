# Proof shows what an HTTP journey saw; no verdict before the first read (#2945, #2903)

Approved by Gabriel (owner) on 2026-11-14 in the session chat: shown the before/after mock at
https://claude.ai/artifact/2Q86aajfWGRfgda5wQt7Bd and asked "Aprova os quatro desenhos (A, B, C,
D)?", the answer was "Aprovo os quatro".

The mock is [`four-fixes-approved.html`](four-fixes-approved.html). This record covers its fix A
(#2945) and fix C (the "Not run yet" flash of #2903). Fixes B and D are separate changes. No new
colour, size or spacing: the tile and the panel reuse the terminal look of
[Proof.dc.html](../Proof.dc.html) (dark ground, mono text, the pill a command's exit code uses).

## Where

The Proof view: the journeys column and the journey panel (run bar, step tiles, step panel).

## Before

| Part | What it shows |
|---|---|
| Kind badge of a journey whose steps are only HTTP requests | Browser |
| Step tile of such a step | "no frame" on a pale ground |
| Step panel | Web 1280 / Mobile 390 tabs, "This run kept no mobile frame of this step", "No frame captured for step N in this run." |
| Verdict beside Run, first half second after a journey opens | "Not run yet", then the real result |

## After

| Part | What it shows |
|---|---|
| Kind badge of an HTTP-only journey | HTTP, with an arrows icon |
| Step tile | On the dark terminal ground: the reply's status, large, and under it its content type |
| Step panel | The request line (for example `GET /health`), the status in the existing exit-code pill (green when the step passed), the content type, the reply text in the existing terminal block, and a grey line: "reply text as recorded, secrets masked · sha256 9878f029…", with "; cut to its last 4096 characters" when the reply was cut |
| Web / Mobile tabs and the "kept no mobile frame" line | Not drawn for such a step: it has no screen |
| Verdict beside Run, until the Runtime has answered | Nothing. The row keeps its height. Then "Not run yet" or the real result, as today |

The reply text is drawn as text, never as markup, and at most 4096 characters. A step whose reply
carries no status (an older Runtime) keeps today's tile and panel.

## Unchanged

Browser and command journeys: their badges, tiles, frames, Web / Mobile tabs and terminal panel.
The verdict for a journey with no run ("Not run yet") once the Runtime has said so, and every
other verdict text.
