# Journey run panel: a failed read is not a failed start (#2935)

Approved by Gabriel (owner) on 2026-11-13 in the session chat: told that "Run could not start" is
shown when only a read failed and that the fix needs new text, and asked "Fix the wrong 'Run could
not start' title next?", the answer was "Yes, fix it".

Only the words inside the shared failed-action alert of [#2445](../2445/README.md) change. Its
layout, buttons, colours and spacing are untouched.

## Where

The Proof view's journey panel, when a journey is opened and the first read of its run fails. No
Run or Confirm was pressed.

## Before

| Part | Text |
|---|---|
| Title | Run could not start |
| Cause, when the Runtime gives none | The Runtime refused to start the journey run. |
| Next step | Fix the cause and try Run again. |

## After

| Part | Text |
|---|---|
| Title | Run could not be read |
| Cause, when the Runtime gives none | The Runtime could not return this journey's run. |
| Next step | Try opening this journey again. |

The words follow the neighbouring "Run history could not be read" alert. No command is suggested,
since no flow was sent to be started.

## Unchanged

A refused Run or Confirm keeps "Run could not start" with its cause, next step and, for a flow
failure, the `graphhelm journey validate --all` command.
