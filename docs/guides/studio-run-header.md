# Studio run header: project HEAD and the behind-origin warning

When you open a run in the Studio's **Proof** view, the run header shows which project commit the run used.

## Where it shows

In the Proof view, under the run's title. The line reads `HEAD <7 characters>`, for example `HEAD cec265f`. The full commit id is in the line's tooltip.

## What the warning means

If the project's local `origin/main` is ahead of the commit the run used, the header adds an orange line:

> 1 commit behind origin/main (as of the last fetch). This run used a checkout behind origin/main (as of the last fetch). Update the project (git pull), then run again.

The number is the count of commits behind `origin/main` at the last fetch. The Runtime counts against the local `origin/main` ref and does not fetch. So a count can be stale.

## What to do

1. Fetch: `git fetch origin` in the project folder, so the count is current.
2. Update: `git pull` on the branch the project uses.
3. Run again, so the run uses the new commit.

No warning line means the run used the same commit as the local `origin/main` at the last fetch.

## Not checked here

- Runs in the run history (`HISTORY`) that are older than this feature show no HEAD line.
- The **Run again** button was not exercised. Runs in the check were started from the CLI.
- Mobile layout was not checked.

Source: `apps/studio/src/proof/ProofView.tsx` (the `run-checkout` and `run-behind` lines). Observed on origin/main `d74e1af1` in a disposable fixture project, captured by Hinata (`Shared/graphhelm-openbot/studio-truth/2705/`).
