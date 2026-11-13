# Proof journeys read failure: Retry only (#2913)

Approved by Gabriel (owner) on 2026-11-13 in the session chat: asked "Drop the Dismiss button from
the Proof error panel so the spinner bug can be fixed?", the answer was "Yes, drop it".

This narrows the shared failed-action alert of [#2445](../2445/README.md) for one place.

## Where

The Proof view when the journeys read failed and there are no journeys to show. The alert
("Journeys could not be read") is then the whole panel, not a message above content.

## Before

The alert had Dismiss and Retry. Dismiss hid the alert and left "Reading the journeys…" on screen
with no read in flight, until a view change or a tab refocus.

## After

The alert has Retry only. It stays until a read succeeds. Nothing else in the alert changes: title,
code, cause, next step, colours and spacing are those of #2445.

## Why

Dismiss means "I have read this, show me what is behind it". Here nothing is behind it, so the only
honest action is to ask again. Every other alert keeps Dismiss, because there the screen behind it
is still usable.
