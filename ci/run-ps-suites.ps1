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
    'event-contract-sweep.tests.ps1',
    'exit-code-shape.tests.ps1',
    'gate-incremental.tests.ps1',
    'gate-native-stderr.tests.ps1',
    'gate-abort-rules.tests.ps1',
    'gate-artifact-reuse.tests.ps1',
    'gate-background-stage-evidence.tests.ps1',
    'gate-canary-outcome.tests.ps1',
    'gate-detached-head.tests.ps1',
    'gate-evidence.tests.ps1',
    'gate-fail-fast.tests.ps1',
    'gate-manifest-provenance.tests.ps1',
    'gate-nextest.tests.ps1',
    'gate-postgres-count.tests.ps1',
    'gate-postgres-early.tests.ps1',
    'gate-postgres-evidence.tests.ps1',
    'gate-run-abort.tests.ps1',
    'gate-rustfmt-path-length.tests.ps1',
    'gate-slot-claim.tests.ps1',
    'gate-slot-wait.tests.ps1',
    'gate-suite-artifact.tests.ps1',
    'gate-script-paths.tests.ps1',
    'gate-stage-overlap.tests.ps1',
    'gate-stage-reddens.tests.ps1',
    'gate-stage-stderr-evidence.tests.ps1',
    'gate-stage-verdict-source.tests.ps1',
    'gate-target-build-state.tests.ps1',
    'gate-target-dir.tests.ps1',
    'required-features.tests.ps1',
    'schema-stage-profile.tests.ps1',
    'gate-verdict.tests.ps1',
    'manifest-name.tests.ps1',
    'run-ps-suites-pool.tests.ps1',
    'normalize-script-eol.tests.ps1',
    'gate-scope-selection.tests.ps1',
    'select-scope.tests.ps1',
    'slot-lock.tests.ps1',
    'studio-stage.tests.ps1',
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

# #1053 item 8: THE SUITES RUN IN A POOL, and the whole cost of this change is the contract below.
#
# They ran one at a time for a median of 457 s, and on the gate that first measured the rest of
# #1053 they were the LARGEST stage at 658.2 s -- larger than `workspace tests`. Nothing made them
# serial but this loop: each suite is already its own process, roots its scratch in
# `GetTempPath()` under a fresh GUID (52 `NewGuid` sites across the set, zero fixed temp paths),
# and no suite writes to a real shared path. The five files that NAME one -- `D:/graphhelm-slot`,
# a runner target root -- name it in prose, as a string handed to a pure predicate, as fixture
# text, or in an assertion that the real path is NOT used. Environment variables and the current
# directory are per-process, so they cannot leak between suites at all.
#
# WHAT A CARELESS POOL WOULD COST, and each of these has a cell in ci/run-ps-suites-pool.tests.ps1:
#
#   1. A LOST EXIT CODE READS AS PASS. This is not hypothetical -- it is the defect the first
#      prototype of this change actually had: all 46 suites came back with an empty `ExitCode` and
#      the runner reported green. `$null` is not 0. Every unreadable code becomes 2 (harness broke),
#      which is the answer that refuses rather than the one that congratulates.
#   2. A SUITE NEVER SCHEDULED LOOKS EXACTLY LIKE A SUITE THAT PASSED. The ledger is set-compared
#      against `$discovered` in BOTH directions after the pool drains; any gap is exit 2, named.
#   3. A HUNG SUITE HAS NO COLOUR. `$SuiteTimeoutSeconds` bounds each child; a kill is 2, never 1
#      and never 0, and it says which suite and how long it was given.
#   4. INTERLEAVED OUTPUT CANNOT BE ATTRIBUTED. Each child writes to its own pair of files and the
#      transcript is replayed whole, per suite, in DISCOVERY order after the last one joins -- so a
#      reader sees the same `[suite] <name>` sections in the same order as before this change.
#
# TWO ORDERS, DELIBERATELY. Dispatch is longest-first (by file size, DERIVED from the tree rather
# than a hand-kept list that would rot); replay is alphabetical. Longest-first is worth more than
# any throttle value here: when this was written, `merge-proof.tests.ps1` (retired 2026-09-24) and
# `gate-manifest-provenance.tests.ps1` were together over 300 KB of the set and dominated the wall
# clock, so starting them last would have made the pool no faster than the loop it replaced.
$throttle = 6
if (-not [string]::IsNullOrWhiteSpace($env:GRAPHHELM_PS_SUITES_THROTTLE)) {
    $parsedThrottle = 0
    # AN UNREADABLE THROTTLE WIDENS TO SERIAL, NEVER TO A BIGGER POOL. Every way of being unsure
    # about this number resolves to the behaviour this file had before the pool existed.
    if ([int]::TryParse($env:GRAPHHELM_PS_SUITES_THROTTLE, [ref] $parsedThrottle) -and $parsedThrottle -ge 1) {
        $throttle = $parsedThrottle
    } else {
        $throttle = 1
        Write-Host "[suites] GRAPHHELM_PS_SUITES_THROTTLE='$($env:GRAPHHELM_PS_SUITES_THROTTLE)' is not a positive integer; running SERIALLY." -ForegroundColor Yellow
    }
}
# A CEILING, NOT A SCHEDULE. The slowest suite measured here was `merge-proof.tests.ps1` (retired
# 2026-09-24) at 487.7 s, so 1800 was roughly 3.7x the real floor then -- far enough that a healthy
# suite never trips it and near enough that a wedged one does not hold the gate for an hour with no
# colour. Overridable only so `ci/run-ps-suites-pool.tests.ps1` can exercise the kill path in under
# a second; an unreadable value keeps the shipped ceiling rather than inventing one.
$SuiteTimeoutSeconds = 1800
if (-not [string]::IsNullOrWhiteSpace($env:GRAPHHELM_PS_SUITE_TIMEOUT_SECONDS)) {
    $parsedTimeout = 0
    if ([int]::TryParse($env:GRAPHHELM_PS_SUITE_TIMEOUT_SECONDS, [ref] $parsedTimeout) -and $parsedTimeout -ge 1) {
        $SuiteTimeoutSeconds = $parsedTimeout
    }
}

Write-Host ''
Write-Host "[suites] $($discovered.Count) discovered; running $throttle at a time, longest first." -ForegroundColor Cyan

# The ledger is seeded with every discovered suite and a $null code, so "never scheduled" is a state
# the aggregation can SEE rather than an absence it has to infer.
$ledger = @{}
foreach ($name in $discovered) { $ledger[$name] = $null }

# WALL TIME PER SUITE, printed beside each replayed transcript. A pool is only ever as fast as its
# slowest member, so the stage that replaces a 658 s serial loop must be able to say WHICH suite is
# now the floor -- otherwise the next person to ask has to re-instrument this file to find out.
$durations = @{}

function Get-SuiteProcessTreeSnapshot {
    param([Parameter(Mandatory)] [int] $RootProcessId)

    try {
        $processes = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, CreationDate -ErrorAction Stop)
        $known = @($RootProcessId)
        $descendants = @()
        do {
            $added = $false
            foreach ($candidate in $processes) {
                $pidValue = [int]$candidate.ProcessId
                $parentValue = [int]$candidate.ParentProcessId
                if ($known -contains $parentValue -and $known -notcontains $pidValue) {
                    $known += $pidValue
                    $descendants += $candidate
                    $added = $true
                }
            }
        } while ($added)
        # Resolve and pin each identity while the tree still exists. A numeric PID is not an
        # identity: Windows may reuse it before cleanup reaches a second Get-Process call.
        $observed = @()
        foreach ($descendant in $descendants) {
            $descendantId = [int]$descendant.ProcessId
            $process = Get-Process -Id $descendantId -ErrorAction SilentlyContinue
            if ($null -eq $process) { continue }
            $null = $process.Handle
            $processStartTicks = [long]$process.StartTime.ToUniversalTime().Ticks
            if ($null -eq $descendant.CreationDate) {
                throw "process $descendantId has no CIM creation identity"
            }
            $cimStartTicks = [long]([datetime]$descendant.CreationDate).ToUniversalTime().Ticks
            # Win32_Process CreationDate is exposed at microsecond precision while Process.StartTime
            # retains 100 ns ticks (measured delta: 0-9 ticks for the same process). More than that
            # means the numeric PID changed owners between the CIM walk and handle capture.
            if ([math]::Abs($processStartTicks - $cimStartTicks) -gt 9) {
                throw "process $descendantId changed identity during tree capture"
            }
            $observed += $process
        }
        return [pscustomobject]@{ Succeeded = $true; Descendants = @($observed) }
    } catch {
        return [pscustomobject]@{ Succeeded = $false; Descendants = @() }
    }
}

function Invoke-SuiteTaskkill {
    param([Parameter(Mandatory)] [int] $ProcessId)

    try {
        $killer = Start-Process -FilePath 'taskkill.exe' -PassThru -WindowStyle Hidden `
            -ArgumentList @('/PID', [string]$ProcessId, '/T', '/F')
        $null = $killer.Handle
        if (-not $killer.WaitForExit(10000)) {
            try { $killer.Kill() } catch { }
            return $false
        }
        return $killer.ExitCode -eq 0
    } catch {
        return $false
    }
}

function Stop-RememberedSuiteDescendants {
    param([Parameter(Mandatory)] [AllowEmptyCollection()] [System.Diagnostics.Process[]] $Processes)

    # These are the handle-backed Process objects captured with the tree, not a second lookup by
    # PID. Kill every observed descendant through that retained handle. The snapshot already
    # includes every generation in the tree, so taskkill's PID-based /T walk is unnecessary here
    # and could target an unrelated process after reuse.
    foreach ($process in $Processes) {
        try {
            if (-not $process.HasExited) { $process.Kill() }
        } catch {
            return $false
        }
    }
    $deadline = (Get-Date).AddSeconds(5)
    do {
        $remaining = @()
        foreach ($process in $Processes) {
            try {
                if (-not $process.HasExited) { $remaining += $process }
            } catch {
                return $false
            }
        }
        if ($remaining.Count -eq 0) { return $true }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $deadline)
    return $false
}

function Stop-SuiteProcessTree {
    param([Parameter(Mandatory)] [System.Diagnostics.Process] $Process)

    # Remember the descendants while the wrapper still names them. If the wrapper exits before
    # taskkill attaches, Windows can no longer use the root PID to find that tree; the remembered
    # child process identities are the only bounded way to finish the timeout without leaking work
    # or acting on a numeric PID that Windows has already reused.
    $tree = Get-SuiteProcessTreeSnapshot -RootProcessId $Process.Id
    if (-not $tree.Succeeded) {
        # Best-effort stop the wrapper, but never call that sufficient: without the snapshot there
        # is no evidence that descendants were found or stopped.
        try {
            if (-not $Process.HasExited) { $null = Invoke-SuiteTaskkill -ProcessId $Process.Id }
            $null = $Process.HasExited -or $Process.WaitForExit(1000)
        } catch { }
        return $false
    }
    if ($Process.HasExited) {
        return Stop-RememberedSuiteDescendants -Processes @($tree.Descendants)
    }

    $rootKilled = Invoke-SuiteTaskkill -ProcessId $Process.Id
    try { $wrapperExited = $Process.HasExited -or $Process.WaitForExit(1000) } catch { $wrapperExited = $false }
    if (-not $wrapperExited) { return $false }
    if (-not (Stop-RememberedSuiteDescendants -Processes @($tree.Descendants))) {
        return $false
    }
    return $true
}

function Read-SuiteStream {
    param(
        [Parameter(Mandatory)] [string] $Path,
        [int] $TimeoutMilliseconds = 5000
    )

    # `Process.HasExited` and release of Start-Process's redirected file handles are not one
    # observable instant on Windows. The full gate measured the gap as a sharing violation while a
    # focused run missed it. Retry only that IO race and refuse after a bounded admission window;
    # this deadline does not claim to bound one File.ReadAllText call already in progress.
    $deadline = [DateTime]::UtcNow.AddMilliseconds([math]::Max(0, $TimeoutMilliseconds))
    while ($true) {
        try {
            return [pscustomobject]@{
                Succeeded = $true
                Text = [System.IO.File]::ReadAllText($Path)
                Reason = $null
            }
        } catch [System.IO.IOException] {
            if ([DateTime]::UtcNow -ge $deadline) {
                return [pscustomobject]@{
                    Succeeded = $false
                    Text = $null
                    Reason = 'redirected stream remained locked after its writer exited'
                }
            }
            Start-Sleep -Milliseconds 25
        } catch {
            return [pscustomobject]@{
                Succeeded = $false
                Text = $null
                Reason = 'redirected stream could not be read'
            }
        }
    }
}

$dispatchOrder = @(
    Get-ChildItem -LiteralPath $SuiteDirectory -Filter '*.tests.ps1' -File -ErrorAction SilentlyContinue |
        Sort-Object -Property Length -Descending |
        Select-Object -ExpandProperty Name |
        Where-Object { $discovered -contains $_ }
)

$scratch = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-ps-suites-$([guid]::NewGuid().ToString('N'))"
$null = New-Item -ItemType Directory -Path $scratch -Force
$nativeWindowsModules = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\Modules'

try {
    $live = New-Object System.Collections.ArrayList
    $queue = New-Object System.Collections.Queue
    foreach ($name in $dispatchOrder) { [void]$queue.Enqueue($name) }

    while ($queue.Count -gt 0 -or $live.Count -gt 0) {
        while ($queue.Count -gt 0 -and $live.Count -lt $throttle) {
            $name = [string]$queue.Dequeue()
            $outPath = Join-Path $scratch "$name.out"
            $errPath = Join-Path $scratch "$name.err"
            # -NonInteractive, and it is not decoration (#925). A suite invoked without a value for
            # one of its `[Parameter(Mandatory)]` inputs does not FAIL -- it PROMPTS, and a prompt
            # in a child this runner waits on has no colour: the stage does not redden, does not go
            # green, it stops. The gate then hangs with no verdict, the worst of the three outcomes.
            # Measured, one script with one Mandatory parameter: without the flag it was still
            # running after 8000 ms and had to be killed; with it, exit 211 ms, rc=1, naming the
            # missing parameter. `ci/gate-script-paths.tests.ps1` keeps every spawn under ci/ honest
            # about carrying it -- and in a pool it matters more, not less, because a prompting
            # child holds a slot forever.
            # Start-Process inherits PSModulePath verbatim. A pwsh host may put its Core Utility
            # module ahead of the Windows module, hiding Get-FileHash in this 5.1 child. Put the
            # native module root first for the launch, then restore the runner's environment.
            $originalModulePath = [Environment]::GetEnvironmentVariable('PSModulePath', 'Process')
            try {
                [Environment]::SetEnvironmentVariable('PSModulePath', "$nativeWindowsModules;$originalModulePath", 'Process')
                $process = Start-Process -FilePath 'powershell' -PassThru -NoNewWindow `
                    -ArgumentList @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $SuiteDirectory $name)) `
                    -RedirectStandardOutput $outPath -RedirectStandardError $errPath
                # Cache the handle immediately, before a very short-lived child can lose its
                # readable ExitCode. Restoring the environment can happen after this boundary.
                $null = $process.Handle
            } finally {
                [Environment]::SetEnvironmentVariable('PSModulePath', $originalModulePath, 'Process')
            }
            [void]$live.Add([pscustomobject]@{ Name = $name; Process = $process; Started = [DateTime]::UtcNow })
        }

        Start-Sleep -Milliseconds 200

        foreach ($entry in @($live)) {
            $done = $false
            if ($entry.Process.HasExited) {
                $done = $true
                $code = $entry.Process.ExitCode
                # $null IS NOT 0. See defect (1).
                if ($null -eq $code) { $ledger[$entry.Name] = 'unreadable' }
                else { $ledger[$entry.Name] = [int]$code }
            } elseif (([DateTime]::UtcNow - $entry.Started).TotalSeconds -gt $SuiteTimeoutSeconds) {
                $done = $true
                if (-not (Stop-SuiteProcessTree -Process $entry.Process)) {
                    Write-Host "HARNESS-BROKE: could not confirm termination of the process tree for $($entry.Name)." -ForegroundColor Magenta
                    exit 2
                }
                $ledger[$entry.Name] = 'timeout'
            }
            if ($done) {
                $durations[$entry.Name] = [math]::Round(([DateTime]::UtcNow - $entry.Started).TotalSeconds, 1)
                $live.Remove($entry)
            }
        }
    }

    # REPLAYED WHOLE, PER SUITE, IN DISCOVERY ORDER -- so this transcript reads exactly like the
    # serial one it replaces, whatever order the pool actually ran them in.
    foreach ($name in $discovered) {
        Write-Host ''
        $took = ''
        if ($durations.ContainsKey($name)) { $took = " ($($durations[$name])s)" }
        Write-Host "[suite] $name$took" -ForegroundColor Cyan
        foreach ($streamPath in @((Join-Path $scratch "$name.out"), (Join-Path $scratch "$name.err"))) {
            if (Test-Path -LiteralPath $streamPath) {
                $stream = Read-SuiteStream -Path $streamPath
                if (-not $stream.Succeeded) {
                    Write-Host "HARNESS-BROKE: transcript for $name $($stream.Reason)." -ForegroundColor Magenta
                    exit 2
                }
                $text = $stream.Text
                if (-not [string]::IsNullOrEmpty($text)) { Write-Host $text.TrimEnd() }
            }
        }
    }
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}

# Judged by EXIT CODE, never by parsing summaries. These suites already print in two different
# shapes -- 'N/N passed' and 'PASSED: N/N' -- so a parser would need a third the day someone writes
# the next one. The exit code is the contract all of them already share.
$broke = New-Object System.Collections.Generic.List[string]
$failed = New-Object System.Collections.Generic.List[string]

# SET-COMPARED BOTH WAYS BEFORE ANY VERDICT. See defect (2): a suite the pool never scheduled, or
# one the ledger grew that nobody discovered, must be a refusal and not a silent difference.
$unrun = @($discovered | Where-Object { $null -eq $ledger[$_] })
$strayKeys = @($ledger.Keys | Where-Object { $discovered -notcontains $_ })
if ($unrun.Count -gt 0 -or $strayKeys.Count -gt 0) {
    Write-Host ''
    if ($unrun.Count -gt 0) { Write-Host "HARNESS-BROKE: discovered but never ran: $($unrun -join ', ')" -ForegroundColor Magenta }
    if ($strayKeys.Count -gt 0) { Write-Host "HARNESS-BROKE: a result exists for a suite nobody discovered: $($strayKeys -join ', ')" -ForegroundColor Magenta }
    Write-Host '  A suite that was never scheduled is indistinguishable from one that passed. Refusing.' -ForegroundColor Magenta
    exit 2
}

foreach ($name in $discovered) {
    $code = $ledger[$name]
    if ($code -is [string]) {
        # Both non-numeric outcomes are HARNESS-BROKE, and each says which one it was.
        if ($code -eq 'timeout') { $broke.Add("$name (exceeded $SuiteTimeoutSeconds s and was killed)") }
        else { $broke.Add("$name (exit code unreadable)") }
    } elseif ($code -eq 2) { $broke.Add("$name (exit 2)") }
    elseif ($code -ne 0) { $failed.Add("$name (exit $code)") }
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
$slowest = @($durations.GetEnumerator() | Sort-Object -Property Value -Descending | Select-Object -First 3 |
    ForEach-Object { "$($_.Key) $($_.Value)s" })
if ($slowest.Count -gt 0) {
    Write-Host "[suites] slowest: $($slowest -join ' | ') -- the pool cannot finish before the first of these" -ForegroundColor Cyan
}
Write-Host "[suites] $($discovered.Count) discovered, $($discovered.Count) passed: $($discovered -join ', ')" -ForegroundColor Green
exit 0
