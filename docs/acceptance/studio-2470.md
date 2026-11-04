# Proof counter acceptance (#2470)

On 2026-11-04, the rebased Studio was served from this issue's worktree on `127.0.0.1:62402` against an isolated synthetic project Runtime on `127.0.0.1:8853`. The browser opened `?view=proof&journey=runtime-health` without a ticket under the merged #2454 session contract. It selected the demo project and Proof journey.

The rendered Proof column showed `0 passed · 0 failed · 0 stale · 14 not run` and `1 previews (not proof)`. The selected card showed `failed · preview, not proof`. Its historical replay was labeled separately. The preview was not counted as passed proof. This is observed browser behavior; the earlier screenshot and reached receipts in the PR belong to the old head.

The synthetic demo run had `preview.budget_exceeded`; this observation does not prove execution success, production behavior, or a fresh installed Studio restart. The focused React test covers a stored replay newer than the list and the mixed states; the browser observation covers the current rendered counter and card labels.
