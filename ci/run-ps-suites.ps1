# #643: runs every versioned PowerShell suite under ci/, discovered FROM THE TREE.
#
# The suites were versioned, they passed, and no runner invoked them -- a suite nobody runs looks
# exactly like coverage. This is the runner, kept as its own script rather than inlined into
# ci/gate.ps1 so that its own failure modes can be exercised without paying for a full gate.
#
# Exit codes are the contract: 0 all passed, 1 a suite failed, 2 the harness could not vouch for
# the run. The third state is NOT collapsed into the second -- see below.
[CmdletBinding()]
param(
    # Defaulted in the BODY, not here: under Windows PowerShell 5.1 `$PSScriptRoot` is still
    # empty while parameter defaults are bound, so computing the path here fails with
    # "Cannot bind argument to parameter 'Path' because it is an empty string". Measured.
    [string] $SuiteDirectory
)

if (-not $SuiteDirectory) { $SuiteDirectory = $PSScriptRoot }

$ErrorActionPreference = 'Continue'

# PINNED INVENTORY OF NAMES, deliberately not a count. A floor ('>= 4') tolerates the removal it
# exists to catch the moment a fifth suite lands, which is #421's defect a generation later. A
# pinned SET fires in BOTH directions and no number has to be kept true: a discovered suite absent
# from the pin says 'add it', a pinned name with no file says 'a suite was removed'.
$PinnedSuites = @(
    'classify-run.tests.ps1',
    'find-culture-comparisons.tests.ps1',
    'frozen-release-guard.tests.ps1',
    'closing-keywords.tests.ps1',
    'crate-input-hash.tests.ps1',
    'exit-code-shape.tests.ps1',
    'gate-abort-rules.tests.ps1',
    'gate-artifact-reuse.tests.ps1',
    'gate-background-stage-evidence.tests.ps1',
    'gate-canary-outcome.tests.ps1',
    'gate-detached-head.tests.ps1',
    'gate-evidence.tests.ps1',
    'gate-manifest-provenance.tests.ps1',
    'gate-postgres-count.tests.ps1',
    'gate-postgres-early.tests.ps1',
    'gate-postgres-evidence.tests.ps1',
    'gate-queue.tests.ps1',
    'gate-runner.tests.ps1',
    'gate-run-abort.tests.ps1',
    'gate-run-overlap.tests.ps1',
    'gate-rustfmt-path-length.tests.ps1',
    'gate-slot-claim.tests.ps1',
    'gate-slot-wait.tests.ps1',
    'gate-script-paths.tests.ps1',
    'gate-stage-overlap.tests.ps1',
    'gate-stage-reddens.tests.ps1',
    'gate-stage-stderr-evidence.tests.ps1',
    'gate-stage-verdict-source.tests.ps1',
    'gate-target-build-state.tests.ps1',
    'gate-target-dir.tests.ps1',
    'required-features.tests.ps1',
    'gate-verdict.tests.ps1',
    'manifest-name.tests.ps1',
    'merge-proof-from-main.tests.ps1',
    'merge-proof.tests.ps1',
    'normalize-script-eol.tests.ps1',
    'gate-scope-selection.tests.ps1',
    'select-scope.tests.ps1',
    'slot-claim.tests.ps1',
    'slot-lock.tests.ps1',
    'studio-stage.tests.ps1',
    'target-inventory.tests.ps1',
    'test-count.tests.ps1'
)

$discovered = @(
    Get-ChildItem -LiteralPath $SuiteDirectory -Filter '*.tests.ps1' -File -ErrorAction SilentlyContinue |
        Sort-Object -Property Name |
        Select-Object -ExpandProperty Name
)

# An empty discovery is NOT a green: it cannot be told apart from 'every suite passed'. This is the
# whole reason the stage exists, so it refuses rather than reporting success over nothing.
if ($discovered.Count -eq 0) {
    Write-Host "HARNESS-BROKE: no *.tests.ps1 discovered under $SuiteDirectory" -ForegroundColor Magenta
    Write-Host '  Zero discovered suites is indistinguishable from zero failures. Refusing.' -ForegroundColor Magenta
    exit 2
}

$unpinned = @($discovered | Where-Object { $PinnedSuites -notcontains $_ })
if ($unpinned.Count -gt 0) {
    Write-Host "FAILED: on disk but not pinned: $($unpinned -join ', ')" -ForegroundColor Red
    Write-Host '  Add the name to $PinnedSuites in this file. A suite nothing pins is a new orphan.' -ForegroundColor Red
    exit 1
}

$vanished = @($PinnedSuites | Where-Object { $discovered -notcontains $_ })
if ($vanished.Count -gt 0) {
    Write-Host "FAILED: pinned but no file: $($vanished -join ', ')" -ForegroundColor Red
    Write-Host '  If the removal was deliberate, drop the name in the SAME commit and say why.' -ForegroundColor Red
    exit 1
}

# Judged by EXIT CODE, never by parsing summaries. These suites already print in two different
# shapes -- 'N/N passed' and 'PASSED: N/N' -- so a parser would need a third the day someone writes
# the next one. The exit code is the contract all of them already share.
$broke = New-Object System.Collections.Generic.List[string]
$failed = New-Object System.Collections.Generic.List[string]
foreach ($name in $discovered) {
    Write-Host ''
    Write-Host "[suite] $name" -ForegroundColor Cyan
    # -NonInteractive, and it is not decoration (#925). A suite invoked without a value for one of
    # its `[Parameter(Mandatory)]` inputs does not FAIL -- it PROMPTS, and a prompt in a child this
    # runner is waiting on has no colour: the stage does not redden, does not go green, it stops.
    # The gate then hangs with no verdict, which is the worst of the three outcomes.
    #
    # The hang is conditional on stdin being open, which is exactly why it hides: redirect stdin and
    # the same script fails cleanly, so a quick check reports no problem. Measured here, one script
    # with one Mandatory parameter, spawned the way this line spawns a suite:
    #
    #   no -NonInteractive     still running after 8000 ms   <- had to be killed
    #   with -NonInteractive   exited after 211 ms, rc=1     names the missing parameter
    #
    # Nothing under ci/ prompts on purpose -- no Read-Host, no PromptForChoice, no -Confirm -- so the
    # flag costs nothing and removes a whole class. `ci/gate-script-paths.tests.ps1` keeps every
    # spawn under ci/ honest about carrying it.
    & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $SuiteDirectory $name)
    $suiteCode = $LASTEXITCODE
    if ($suiteCode -eq 2) { $broke.Add("$name (exit 2)") }
    elseif ($suiteCode -ne 0) { $failed.Add("$name (exit $suiteCode)") }
}

# The third state SURVIVES aggregation. classify-run.tests.ps1 exits 2 when its assertion count
# disagrees with its declared total: 'the harness broke', not 'a test failed'. Collapsing that into
# a generic red would rebuild, at the runner, the very ambiguity that suite went to the trouble of
# separating.
if ($broke.Count -gt 0) {
    Write-Host ''
    Write-Host "HARNESS-BROKE in: $($broke -join '; ')" -ForegroundColor Magenta
    Write-Host '  A suite could not vouch for its own run. This is not a failing test.' -ForegroundColor Magenta
    exit 2
}
if ($failed.Count -gt 0) {
    Write-Host ''
    Write-Host "FAILED in: $($failed -join '; ')" -ForegroundColor Red
    exit 1
}

Write-Host ''
Write-Host "[suites] $($discovered.Count) discovered, $($discovered.Count) passed: $($discovered -join ', ')" -ForegroundColor Green
exit 0
