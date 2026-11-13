# Proof: one Run at a time for a journey, card and panel (#2939)

Approved by Gabriel (owner) on 2026-11-13 in the session chat: told that a run can be started twice
(card and panel) and that the fix is to switch the other button off for that moment, and asked "Fix
the double Run click next?", the answer was "Yes, fix it".

No new look. Both buttons only enter states they already have in `Proof.dc.html`, for a moment
longer than before.

## Where

The Proof view, for one journey, between a click on Run and the Runtime's answer to that start.

## Before

| Click | The other button for the same journey |
|---|---|
| Run on the journey's card | The panel's Run stays enabled and reads "Run" or "Run again" |
| Run in the panel | The card's Run stays enabled |

A second click sent a second start request. The Runtime answered it with the run already in flight.

## After

| Click | The other button for the same journey |
|---|---|
| Run on the journey's card | The panel's Run is disabled and reads "Running…", with the "Starting the run" status, as when the panel itself starts a run |
| Run in the panel | The card's Run is disabled, as when the column is busy |

Cards of other journeys are unchanged: they stay enabled while the panel starts a run.
