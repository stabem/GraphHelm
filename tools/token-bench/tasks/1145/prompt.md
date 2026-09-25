# gate: both PostgreSQL matrices start after a failed artifact build

The gate starts both PostgreSQL matrices after the Rust artifact build has failed. Two clusters are created, initialised and later joined for a run whose binaries do not exist.

Found by a dead-author branch. PR #1074 (`issue-1053-gate-under-five-minutes`, head `8c080df6`) carries a cell main has no equivalent of — `a failed builder creates no PostgreSQL cluster`, at `ci/gate-postgres-early.tests.ps1:199`. It passes there and would go RED against main today. Main has no failure-path cell at this site at all, which is why nothing caught this.

Measured on `origin/main`:

- `ci/gate.ps1:330` — `$script:matrixSkipped = $skipPostgresFlag -or (-not $script:gateScope.matrix)`. Set once, from a flag and the scope. Nothing else ever assigns it.
- `ci/gate.ps1:4692` — `$artifactManifest = Get-TestArtifactManifest`. On a failed build this returns rather than throwing; the failure is recorded as `buildExitCode` (built at `:2554`), so control continues.
- `ci/gate.ps1:4807-4811` — the early start, guarded only by `if (-not $script:matrixSkipped)`. `buildExitCode` is not consulted here. `grep -n buildExitCode ci/gate.ps1` shows it read at `:4035`, `:4188` and `:5098` — the verdict, the manifest and the end-of-run stamp — and at none of the three early-start lines.

So the two `Start-PostgresStageEarly` calls fire after a build that produced no runnable binaries. The run is still correctly RED at the end, so this is waste and noise rather than a false GREEN: two clusters spun up, a `$env:GRAPHHELM_PG_BIN` assignment, and two joins to unwind, on every failed-build run. On a machine that also hosts the queue runner and live benches, that is real contention at the worst moment — the moment a developer is already waiting to see a build error.

How the branch avoids it, for reference rather than as a proposal: there `Start-PostgresStageEarly` is invoked from `New-GateRustBuildInventory`'s `OnWorkspaceReady` callback, which fires only on `exitCode -eq 0` (`ci/gate-test-inventory.ps1:1018`). That is a consequence of its architecture, which is not landing — see the decision on #1074. Main needs only the guard.

Suggested fix, one line plus a cell:

```powershell
if (-not $script:matrixSkipped -and $artifactManifest.buildExitCode -eq 0) {
```

`-eq 0` rather than `-ne $null`, because `$emptyArtifacts` carries `buildExitCode = $null` (`:4596`, `:4632`) for the case where the manifest could not be built at all, and that case must not start clusters either.

Acceptance: a cell at the `ci/gate-postgres-early.tests.ps1` site that drives the slice with a failing builder stub and asserts zero clusters started — the branch's cell at `:199` is the shape, retargeted from its `Invoke-Stage 'Rust artifact builds' {` anchor to main's `$artifactManifest = Get-TestArtifactManifest` at `:4692`. Mutating the guard away must redden it.

Refs #1074. Pre-adoption state of that branch is pinned at `refs/backup/pr1074-preadopt-head` / `-base` on the remote.

