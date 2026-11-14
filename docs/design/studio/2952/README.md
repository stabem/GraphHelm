# Review modal: Now is drawn exactly over Before (#2952)

The approved review modal design ([`Graph.dc.html`](../Graph.dc.html)) is unchanged: Overlay lays Now
over Before, and Changes lights only what moved. This brings the implementation back to that design,
so no new approval is quoted. Issue: [#2952](https://github.com/stabem/GraphHelm/issues/2952).

## Where

The Graph view's review modal (an In review or Merged card), in the Overlay and Changes modes.

## Before

Before was an inline image and Now was placed at the bottom of the same box, so the browser left
the inline baseline gap under Before and drew Now lower. Measured in a real browser at 1280 x 800:
Before at y = 200, Now at y = 204, same width and height. Two frames of one screen showed text
doubled and offset by 4 px, and Changes lit the whole screen instead of only what moved.

## After

Before is a block image, so there is no gap and Now covers exactly Before's box. Measured the same
way, in Overlay and in Changes: Before and Now both at x = 80.59, y = 200, 1118.77 x 699.98.
Overlay shows no doubled text; Changes shows black everywhere except the region that differs.

## Unchanged

Frame and Side by side, the mode buttons, the slider, the captions, the colours, the spacing and the
text for screen readers (and how it is read from the browser's drawn style) are untouched.
