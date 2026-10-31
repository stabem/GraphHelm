# #2325 + #2324: journeys to fix, and a run that waits for you

Approved by the owner with no changes: [#2325](https://github.com/stabem/GraphHelm/issues/2325#issuecomment-6546340907) and [#2324](https://github.com/stabem/GraphHelm/issues/2324#issuecomment-6546341241).

Static mocks built on the current screens. `shots.html#<screen>-<before|after>` renders each PNG.

| screen | change |
|---|---|
| sidebar | `N journeys · M failing` → `· M to fix` (journeys the Runtime cannot read or reports an error finding on) |
| proof | journey card: amber `N to fix` chip that opens the findings (code, pointer, message); the run chip is unchanged and never says "failed" |
| graph | Status `running` → `waiting for you` when the run needs you, plus a `Waits on` row naming the blocked node and the CLI line that approves it |
| lanes | Needs action: `Nothing waits on you.` → `The run waits for you: …`; the count includes it |
