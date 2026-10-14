# Windows `graphhelm update` build measurements (October 2026)

Summary: Windows build timings separate a cold build from cache reuse and machine speed.

## Decision rule (set before measurement)

If the warm build is much faster than the cold build and the second update is close to
cold-build time, the update is not reusing its cache. Fix cache reuse. If the cold build
itself is much slower than the Linux baseline while the warm build is small, the machine
is slow for this build; prefer a prebuilt Windows binary without GitHub Actions.

Do not infer a result from missing measurements. Mark those cells `OBSERVER_MISSING`.

## Results

| Measurement | Result | Command or source |
|---|---|---|
| Linux cold baseline | 2m04s | Existing issue #1186 comment: `cargo build --locked --profile update -p graphhelm-cli`; fresh target directory, source head `aafbefdd`. |
| Linux warm baseline | 4.6s | Existing issue #1186 comment: same command after touching `apps/cli/src/main.rs`. |
| Windows cold wall time | `OBSERVER_MISSING` | Owner-PC command below. This lane did not run a cold build. |
| Windows cold CPU time | `OBSERVER_MISSING` | The prescribed `Measure-Command` observer records wall time only; no CPU-time observer was specified. |
| Windows warm wall time | `OBSERVER_MISSING` | Owner-PC command below. This lane did not run a warm build. |
| Second `graphhelm update` wall time | `OBSERVER_MISSING` | Owner-PC update observer below. This lane did not run `graphhelm update`. |
| Update cache path | `C:\Users\gabri\.graphhelm\target` | Source inspection: `apps/cli/src/commands/studio.rs`, `managed_target(user_home())`; falls back to `CARGO_TARGET_DIR` for unmanaged sources. The path did not exist in this session when checked. |
| Cache reused by two real updates | `OBSERVER_MISSING` | The cache path is selected in source, but no two consecutive update runs were observed. |
| Defender excludes the target | `OBSERVER_MISSING` | `Get-MpPreference | Select-Object -ExpandProperty ExclusionPath` returned `N/A: Must be an administrator to view exclusions`. No setting was changed. |
| Top 10 Windows build units | `OBSERVER_MISSING` | No Windows timing file was produced in this lane. |

The available evidence does not distinguish a slow Windows machine from a cache
reuse problem. The source now selects a persistent target for a managed clone, but
that implementation detail does not prove a real update reuses it.

## Owner-PC observer commands

Run these in one hidden PowerShell terminal, with no other heavy job running. Submit
both Cargo builds through the shared build slot shown below. Use a fresh target
directory for the cold build. The touching step intentionally invalidates the CLI
crate for the warm build.

```powershell
$target = 'D:/ghc/perf-1186-cold'
$cold = Measure-Command {
    graphhelm --json workspace slot --root D:/gh --lane <lane> --jobs 4 --label windows-cold-build -- cargo +1.97.1 build --locked --profile update -p graphhelm-cli --timings --target-dir $target
}
$cold | Select-Object TotalSeconds
(Get-Item apps/cli/src/main.rs).LastWriteTime = Get-Date
$warm = Measure-Command {
    graphhelm --json workspace slot --root D:/gh --lane <lane> --jobs 4 --label windows-warm-build -- cargo +1.97.1 build --locked --profile update -p graphhelm-cli --timings --target-dir $target
}
$warm | Select-Object TotalSeconds
```

Keep `cargo-timings/cargo-timing.html` and record its ten slowest units. Then inspect
the update target with:

```powershell
$userProfile = [Environment]::GetFolderPath('UserProfile')
$updateTarget = Join-Path $userProfile '.graphhelm/target'
Test-Path -LiteralPath $updateTarget
```

Measure two consecutive real updates with `Measure-Command { graphhelm update }` and
record both durations and whether the second run used the same target. Read Defender
exclusions without changing them:

```powershell
Get-MpPreference | Select-Object -ExpandProperty ExclusionPath
```

The original card's observer covers wall time, not CPU time; add a named CPU-time
observer before filling that cell. If Defender denies access, record that result and
leave the exclusion unknown.
