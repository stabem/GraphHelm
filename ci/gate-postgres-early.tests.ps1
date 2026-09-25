# #956 step 2b: the two PostgreSQL matrices run BESIDE the cli suites instead of after them. Each
# matrix is postgres.ps1 in its own child process (own port, own data directory), started behind
# the workspace build and joined at its old position under its old name. What can go wrong is not
# only the spawn plumbing: detached PostgreSQL also needs a parent-owned job. The count
# file and the locale a child reads are the gate's environment AT THE INSTANT OF THE SPAWN, and
# both must be restored after it; a start that fails must fall back to the in-line matrix; the
# join must hand the child's count file to the same reader the in-line path uses; and the
# measurement of the overlap must stay a note (#989 is the instance of the alternative).
#
# The functions are cut out of ci/gate.ps1 by anchor and DRIVEN against stubs that record what
# they were handed, rather than read.

param([switch] $ObservePostgresAbort, [string] $PostgresBin = 'C:/Users/example/tools/pgsql/bin')
$ExpectedAssertionCount = 37
if ($ObservePostgresAbort) { $ExpectedAssertionCount += 2 }
$ErrorActionPreference = 'Stop'
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
if (-not (Test-Path -LiteralPath $gatePath)) {
    Write-Host 'HARNESS-BROKE: ci/gate.ps1 is not beside this suite' -ForegroundColor Magenta
    exit 2
}
$gateText = [System.IO.File]::ReadAllText($gatePath)

function Get-GateSlice {
    param([Parameter(Mandatory)] [string] $Start, [Parameter(Mandatory)] [string] $End, [switch] $IncludeEnd)
    $i = $gateText.IndexOf($Start, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -le $i) { throw "HARNESS-BROKE: slice anchors did not match for [$Start]" }
    $end = if ($IncludeEnd) { $j + $End.Length } else { $j }
    return $gateText.Substring($i, $end - $i)
}

$previousCount = $env:GRAPHHELM_PG_COUNT_FILE
$previousLocale = $env:GRAPHHELM_PG_LOCALE
try {
    foreach ($fn in @('New-PostgresStageJob', 'Start-PostgresStageEarly', 'Stop-PostgresStageEarly', 'Complete-PostgresStage', 'Get-NonCLocale', 'Invoke-Postgres')) {
        $m = [regex]::Match($gateText, "(?ms)^function $fn \{.*?^\}")
        if (-not $m.Success) { throw "HARNESS-BROKE: $fn is not in gate.ps1 to be cut out" }
        Invoke-Expression $m.Value
    }

    # STUBS that record the environment at the instant of the spawn -- the fact the real child reads.
    $script:spawns = New-Object System.Collections.Generic.List[object]
    $script:failStart = $false
    $repositoryRoot = Join-Path ([System.IO.Path]::GetTempPath()) 'graphhelm-pg-early-suite'
    function Start-BackgroundStage {
        param($Name, $FilePath, $ArgumentList, $WorkingDirectory)
        $script:spawns.Add([pscustomobject]@{
            Name = $Name; File = $FilePath; Args = @($ArgumentList); Cwd = $WorkingDirectory
            CountFileAtSpawn = $env:GRAPHHELM_PG_COUNT_FILE; LocaleAtSpawn = $env:GRAPHHELM_PG_LOCALE
        })
        if ($script:failStart) { return $null }
        return [pscustomobject]@{ Name = $Name; Process = 'stub'; Out = ''; Err = ''; StartedUtc = '' }
    }

    $env:GRAPHHELM_PG_COUNT_FILE = 'C:\not\a\real\count\file'
    $env:GRAPHHELM_PG_LOCALE = 'C'
    $stubScript = 'C:\stub\postgres.ps1'
    $nonWindows = Start-PostgresStageEarly -Name 'non-Windows control' -WindowsHost $false -ScriptPath $stubScript -SupportScriptPath 'does-not-exist'
    Assert-True ($null -eq $nonWindows -and $script:spawns.Count -eq 0) 'non-Windows returns the inline fallback before loading Windows job support'
    $supportFailure = Start-PostgresStageEarly -Name 'support failure control' -WindowsHost $true -ScriptPath $stubScript -SupportScriptPath 'does-not-exist'
    Assert-True ($null -eq $supportFailure -and $script:spawns.Count -eq 0) 'job support initialization failure returns inline fallback without a child'
    $savedJobFactory = (Get-Item Function:New-PostgresStageJob).ScriptBlock
    try {
        function New-PostgresStageJob { param($SupportScriptPath) throw 'controlled CreateJobObject failure' }
        $jobFailure = Start-PostgresStageEarly -Name 'job construction failure' -WindowsHost $true -ScriptPath $stubScript -SupportScriptPath 'unused'
        Assert-True ($null -eq $jobFailure -and $script:spawns.Count -eq 0) 'job construction failure returns inline fallback without a child'
    } finally { Set-Item Function:New-PostgresStageJob $savedJobFactory }
    $early = Start-PostgresStageEarly -Name 'PostgreSQL ignored matrix' -ScriptPath $stubScript -SupportScriptPath (Join-Path $PSScriptRoot 'postgres.ps1')
    $spawn = $script:spawns[0]
    Assert-True ($null -ne $early -and $spawn.CountFileAtSpawn -eq $early.CountFile -and (Test-Path -LiteralPath $early.CountFile)) `
        'the child is spawned with GRAPHHELM_PG_COUNT_FILE pointing at the file the join will read, and that file exists'
    $decoded = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($spawn.Args[-1]))
    Assert-True ($spawn.LocaleAtSpawn -eq 'C' -and $spawn.File -eq 'powershell' -and (@($spawn.Args) -contains '-NonInteractive') -and $decoded.Contains($stubScript)) `
        'a matrix without a locale keeps the ambient one, runs postgres.ps1 under powershell, and carries -NonInteractive (#925)'
    Assert-True ($env:GRAPHHELM_PG_COUNT_FILE -eq 'C:\not\a\real\count\file' -and $env:GRAPHHELM_PG_LOCALE -eq 'C') `
        'and after the spawn both variables are restored, so the next spawn and the in-line fallback see the ambient environment'
    Remove-Item -LiteralPath $early.CountFile -Force -ErrorAction SilentlyContinue
    $early.Job.Dispose()

    $collation = Start-PostgresStageEarly -Name 'PostgreSQL matrix under a non-C collation' -Locale 'English_United States.1252' -ScriptPath $stubScript -SupportScriptPath (Join-Path $PSScriptRoot 'postgres.ps1')
    $spawn2 = $script:spawns[1]
    Assert-True ($spawn2.LocaleAtSpawn -eq 'English_United States.1252' -and $env:GRAPHHELM_PG_LOCALE -eq 'C') `
        'the collation matrix child sees the non-C locale at its spawn, and the gate does not keep it afterwards'
    Remove-Item -LiteralPath $collation.CountFile -Force -ErrorAction SilentlyContinue
    $collation.Job.Dispose()

    $script:failStart = $true
    $failed = Start-PostgresStageEarly -Name 'PostgreSQL ignored matrix' -ScriptPath $stubScript -SupportScriptPath (Join-Path $PSScriptRoot 'postgres.ps1')
    Assert-True ($null -eq $failed -and $env:GRAPHHELM_PG_COUNT_FILE -eq 'C:\not\a\real\count\file') `
        'a start that fails answers $null -- the join then runs the matrix in line -- and leaves the environment as it found it'
    $script:failStart = $false

    # THE JOIN hands the child's count file and a joining body to the SAME reader the in-line path
    # uses; with nothing started early it takes the in-line path unchanged.
    $script:stageCalls = New-Object System.Collections.Generic.List[object]
    $script:joins = New-Object System.Collections.Generic.List[object]
    function Invoke-PostgresStage {
        param([string] $Name, [string] $CountFile = '', [scriptblock] $Body = $null)
        $script:stageCalls.Add([pscustomobject]@{ Name = $Name; CountFile = $CountFile; HasBody = ($null -ne $Body) })
        if ($null -ne $Body) { & $Body }
    }
    function Complete-BackgroundStage { param($Started) $script:joins.Add($Started); return 0 }
    Complete-PostgresStage -Name 'PostgreSQL ignored matrix' -Early $null
    Assert-True ($script:stageCalls.Count -eq 1 -and $script:stageCalls[0].Name -eq 'PostgreSQL ignored matrix' -and $script:stageCalls[0].CountFile -eq '' -and -not $script:stageCalls[0].HasBody) `
        'with no early start the join runs the matrix in line: the same call as before this change, no count file, no body'
    $fakeEarly = [pscustomobject]@{ Name = 'PostgreSQL ignored matrix'; Started = [pscustomobject]@{ Name = 'child'; Process = 'stub' }; CountFile = 'C:\the\childs\count\file' }
    Complete-PostgresStage -Name 'PostgreSQL ignored matrix' -Early $fakeEarly
    Assert-True ($script:stageCalls.Count -eq 2 -and $script:stageCalls[1].CountFile -eq 'C:\the\childs\count\file' -and $script:stageCalls[1].HasBody) `
        'with an early start the join hands the child''s own count file to the reader, under the same stage name'
    Assert-True ($script:joins.Count -eq 1 -and $script:joins[0].Name -eq 'child') `
        'and the body it hands over JOINS the started child rather than spawning a second one'

    # TWO CHILDREN, BECAUSE ONE CANNOT DISCRIMINATE. The cell above drives this function with a
    # SINGLE fake and asserts `joins.Count -eq 1`, which is satisfied by a correct stash and by a
    # broken one alike -- the same shape the house calls a non-unique anchor faking a green. The
    # body reaches its child through `$script:pgStageToJoin`, ONE script-scope slot, and the
    # gate calls this function twice in a row. What makes that safe is not the slot: it is that
    # `Invoke-Stage` runs `& $Body` SYNCHRONOUSLY, so write -> join -> return completes before the
    # next write. That invariant is invisible at the call site and nothing else pins it.
    #
    # So: two distinct children, and the assertion names the strings. Found independently by two
    # reviewers on this PR (X's adversarial pass and lane O's R991), both of which showed that if a
    # body is ever QUEUED rather than run inline, both joins see the SECOND child -- and the cell
    # above stays green through it.
    $script:joins = New-Object System.Collections.Generic.List[object]
    $earlyA = [pscustomobject]@{ Name = 'PostgreSQL ignored matrix'; Started = [pscustomobject]@{ Name = 'childA'; Process = 'stub' }; CountFile = 'C:\count' }
    $earlyB = [pscustomobject]@{ Name = 'PostgreSQL matrix under a non-C collation'; Started = [pscustomobject]@{ Name = 'childB'; Process = 'stub' }; CountFile = 'C:\count' }
    Complete-PostgresStage -Name 'PostgreSQL ignored matrix' -Early $earlyA
    Complete-PostgresStage -Name 'PostgreSQL matrix under a non-C collation' -Early $earlyB
    Assert-True ($script:joins.Count -eq 2) `
        'two early starts produce two joins, not one'
    Assert-True ($script:joins[0].Name -eq 'childA' -and $script:joins[1].Name -eq 'childB') `
        'and each join receives ITS OWN child: a single shared slot read after both writes would hand childB to both'

    # PLACEMENT, BY CONTAINMENT -- AND THE ANCHOR IS THE BUILD, NOT A NEIGHBOUR.
    #
    # This cell used to slice from `Invoke-Stage 'workspace tests' {` while its message said "after
    # the workspace build". Those are different claims and the gate proved it: the build that makes
    # the matrices' binaries is `Get-TestArtifactManifest`, which runs
    # `cargo test --workspace --all-features --locked --no-run` far above, and `workspace tests` only
    # re-proves what that already built. So the old slice pinned a NEIGHBOUR and the message named a
    # PROPERTY, and moving the starts one stage earlier -- strictly still after the build -- reddened
    # a cell whose sentence had just become MORE true.
    #
    # The anchor is now the build itself. The slice runs from `Get-TestArtifactManifest` to the
    # `workspace tests` stage, so the cell fails if the starts move ABOVE the build (their binaries
    # would not exist) and fails if they slide BELOW `workspace tests` (the 409.9s of cover this
    # placement buys would be gone). Being before the cli suites follows from being before
    # `workspace tests`, which precedes them; it is no longer asserted separately because an
    # assertion that restates a consequence adds a second thing to keep true, not a second check.
    $startSlice = Get-GateSlice -Start '$artifactManifest = Get-TestArtifactManifest' -End "Invoke-Stage 'workspace tests' {"
    Assert-True ($startSlice.Contains("Start-PostgresStageEarly -Name 'PostgreSQL ignored matrix'") -and $startSlice.Contains("Start-PostgresStageEarly -Name 'PostgreSQL matrix under a non-C collation' -Locale") -and $startSlice.Contains('if (-not $script:matrixSkipped -and -not $script:postgresMatrixUnavailable)')) `
        'both matrices are started after a successful build that makes their binaries and before the workspace-tests stage they run beside, and not when the scope skipped them or the build failed'

# #1052 SECOND-PASS FINDING: THE BUILD ANCHOR ALONE LEAVES CLIPPY INSIDE THE SLICE.
# The cell above runs from `Get-TestArtifactManifest` to the `workspace tests` stage, and `clippy`
# sits between them -- so hoisting the starts ABOVE clippy, or INTO its block, stays green while
# breaking the one constraint the commit body argues the placement on. `cargo clippy` COMPILES: it
# drives a compiler over the workspace into the same target directory and holds the build lock
# while it does, so two matrix `cargo test` processes beside it reintroduce exactly the build-lock
# contention this placement exists to avoid. The cell above cannot see that, because a start
# hoisted above clippy is still after the build.
#
# This is a SEPARATE check rather than a narrowed anchor: the build-before-start property and the
# clippy-before-start property fail for different reasons and want different messages. Narrowing
# the slice's start to clippy would have silently retired the "their binaries would not exist"
# arm.
#
# Ordering is asserted on OFFSETS, not on containment: a start placed INSIDE clippy's block is
# still textually after `Invoke-Stage 'clippy ...' {`, so containment cannot tell the two apart.
# The close of clippy's own block is the reference point, and the starts must follow it.
$clippyOpen = $gateText.IndexOf("Invoke-Stage 'clippy (deny warnings)' {", [System.StringComparison]::Ordinal)
$clippyClose = if ($clippyOpen -ge 0) { $gateText.IndexOf('} | Out-Null', $clippyOpen, [System.StringComparison]::Ordinal) } else { -1 }
$firstStart = $gateText.IndexOf("Start-PostgresStageEarly -Name 'PostgreSQL ignored matrix'", [System.StringComparison]::Ordinal)
$secondStart = $gateText.IndexOf("Start-PostgresStageEarly -Name 'PostgreSQL matrix under a non-C collation'", [System.StringComparison]::Ordinal)
# A DECOY FOR EACH ANCHOR: a non-unique anchor would fake a green by matching an earlier copy.
Assert-True (([regex]::Matches($gateText, [regex]::Escape("Invoke-Stage 'clippy (deny warnings)' {"))).Count -eq 1) `
'the clippy stage opening is a unique anchor in ci/gate.ps1, so the ordering cell below cannot be satisfied by a second copy'
Assert-True ($clippyOpen -ge 0 -and $clippyClose -gt $clippyOpen -and $firstStart -gt $clippyClose -and $secondStart -gt $clippyClose) `
'both matrices are started AFTER the clippy stage has closed, not above it and not inside it -- clippy compiles into the shared target directory and holds the build lock, so overlapping it with two matrix cargo processes is the contention this placement avoids'

    $joinSlice = Get-GateSlice -Start 'if ($script:matrixSkipped) {' -End '# #207:'
    Assert-True ($joinSlice.Contains("Complete-PostgresStage -Name 'PostgreSQL ignored matrix' -Early") -and $joinSlice.Contains("Complete-PostgresStage -Name 'PostgreSQL matrix under a non-C collation' -Early") -and -not $joinSlice.Contains("Invoke-PostgresStage -Name 'PostgreSQL")) `
        'both matrices are JOINED at their old position under their old names, and the in-line call is gone from there'

    # A NOTE, NEVER A VERDICT (#989): nothing appends to $failed between the note and the status.
    $noteAnchor = 'NOTE: the PostgreSQL matrices started early but overlapped no other stage'
    $noteIndex = $gateText.IndexOf($noteAnchor, [System.StringComparison]::Ordinal)
    $statusIndex = if ($noteIndex -ge 0) { $gateText.IndexOf('$status = Get-GateStatus -FailedStageCount $failed.Count', $noteIndex, [System.StringComparison]::Ordinal) } else { -1 }
    $between = if ($noteIndex -ge 0 -and $statusIndex -gt $noteIndex) { $gateText.Substring($noteIndex, $statusIndex - $noteIndex) } else { '<no slice>' }
    Assert-True ($noteIndex -ge 0 -and $statusIndex -gt $noteIndex -and $between -notmatch '\$failed\s*\+=') `
        'the overlap of the matrices is reported as a NOTE and nothing is appended to $failed between it and the status derivation'
    $writer = Get-GateSlice -Start 'function Write-RunManifest {' -End "`n}" -IncludeEnd
    Assert-True ($writer.Contains('pgMatricesStartedEarly') -and $writer.Contains('pgMatricesOverlapped')) `
        'the manifest carries pgMatricesStartedEarly and pgMatricesOverlapped, so the saving is measurable from the record'

    # THE FIELD IS THE MEASUREMENT, so the assignment is pinned to the predicate on BOTH names (graphhelm-b8
    # on #991): replacing the right-hand side with `$true` left every cell above green, and a field that
    # can be set by hand can claim a saving the run did not make. Same shape as #985's slotWaitSecs cell.
    Assert-True ($gateText -match "(?m)^\`$script:pgMatricesOverlapped\s*=\s*\(Test-StageOverlapped -Records \`$stageRecords -Name 'PostgreSQL ignored matrix'\) -or\s*?
\s*\(Test-StageOverlapped -Records \`$stageRecords -Name 'PostgreSQL matrix under a non-C collation'\)") `
        'pgMatricesOverlapped is derived from Test-StageOverlapped over the gate''s own records for both matrices, on the assignment itself -- not a literal, not a copy of another field'
    # Exercise the native PowerShell 5 array boundary, including spaces and an apostrophe.
    $fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('pg argv ' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
    $fixturePath = Join-Path $fixtureRoot "child's postgres.ps1"
    $fixtureOutput = Join-Path $fixtureRoot 'argv.json'
    $previousOutput = $env:GRAPHHELM_PG_ARGV_FIXTURE
    $env:GRAPHHELM_PG_ARGV_FIXTURE = $fixtureOutput
    try {
        [IO.File]::WriteAllText($fixturePath, 'param([string[]]$TestArgs) [IO.File]::WriteAllText($env:GRAPHHELM_PG_ARGV_FIXTURE, (ConvertTo-Json -InputObject $TestArgs -Compress)); exit 0')
        $expectedArgs = @('+1.97.1','test','-p','graphhelm-postgres-event-store','--all-features','--locked','--','--ignored','--test-threads=1')
        $realEarly = Start-PostgresStageEarly -Name 'argv control' -ScriptPath $fixturePath -SupportScriptPath (Join-Path $PSScriptRoot 'postgres.ps1')
        $realSpawn = $script:spawns[$script:spawns.Count - 1]
        & powershell.exe @($realSpawn.Args)
        $earlyExit = $LASTEXITCODE
        $realEarly.Job.StopAndDrain(10000)
        $realEarly.Job.Dispose()
        $actualArgs = Get-Content -LiteralPath $fixtureOutput -Raw | ConvertFrom-Json
        Assert-True ($earlyExit -eq 0 -and (($actualArgs -join '|') -ceq ($expectedArgs -join '|'))) 'early child receives exactly the PostgreSQL package argument vector through PowerShell 5'
        Remove-Item -LiteralPath $realEarly.CountFile, $fixtureOutput -Force
        # The exact previously valid command cannot reach the fixture after its parent closes the job.
        $savedPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            & powershell.exe @($realSpawn.Args) 2>$null
            $closedJobExit = $LASTEXITCODE
        } finally { $ErrorActionPreference = $savedPreference }
        Assert-True ($closedJobExit -ne 0 -and -not (Test-Path -LiteralPath $fixtureOutput)) 'a closed parent job refuses assignment before executing the child script'
        Invoke-Postgres -ScriptPath $fixturePath
        $fallbackExit = $LASTEXITCODE
        $actualArgs = Get-Content -LiteralPath $fixtureOutput -Raw | ConvertFrom-Json
        Assert-True ($fallbackExit -eq 0 -and (($actualArgs -join '|') -ceq ($expectedArgs -join '|'))) 'fallback child receives the same PostgreSQL-only vector'

        # Drive the actual abort cleanup against nested handles, not wrapper objects.
        $script:studioStarted = $null
        $script:psSuitesStarted = $null
        $script:killed = @()
        function taskkill { $script:killed += [int]$args[-1] }
        $earlyControls = @(foreach ($id in @(711, 712)) {
            $process = [pscustomobject]@{ Id = $id; HasExited = $false }
            $process | Add-Member -MemberType ScriptMethod -Name Kill -Value { $script:killed += $this.Id; $this.HasExited = $true }
            $process | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value { param($milliseconds) return $true }
            $job = [pscustomobject]@{}
            $job | Add-Member -MemberType ScriptMethod -Name StopAndDrain -Value { param($milliseconds) }
            $job | Add-Member -MemberType ScriptMethod -Name Dispose -Value { }
            [pscustomobject]@{ Started = [pscustomobject]@{ Name = 'fixture'; Process = $process }; CountFile = [IO.Path]::GetTempFileName(); Job = $job }
        })
        $script:pgIgnoredEarly = $earlyControls[0]
        $script:pgCollationEarly = $earlyControls[1]
        $cleanup = Get-GateSlice -Start '    foreach ($started in @($script:studioStarted, $script:psSuitesStarted)) {' -End '    # #700:'
        Invoke-Expression $cleanup
        Assert-True (($script:killed -join ',') -eq '711,712') 'abort cleanup reaches both nested PostgreSQL process handles'
        Assert-True (-not (Test-Path -LiteralPath $earlyControls[0].CountFile) -and -not (Test-Path -LiteralPath $earlyControls[1].CountFile)) 'abort cleanup removes both PostgreSQL count files'
        $earlyControls[0].Job | Add-Member -Force -MemberType ScriptMethod -Name StopAndDrain -Value { param($milliseconds) throw 'controlled drain failure' }
        $earlyControls[0].CountFile = [IO.Path]::GetTempFileName()
        $cleanupRefused = $false
        $script:abortReasons = @()
        function Write-RunAbort { param($Reason) $script:abortReasons += $Reason }
        try { Invoke-Expression $cleanup } catch { $cleanupRefused = $true }
        # REPOINTED AT THE OUTCOME, because the previous form pinned a MECHANISM that did not
        # deliver it. It read `($cleanupRefused -and (Test-Path $CountFile))` -- "throws before slot
        # release and preserves evidence" -- and the two halves are independent:
        #
        #   * the count file survives because `Stop-PostgresStageEarly` threw BEFORE its own
        #     removal statement. That is true with or without an outer throw, and the cell below
        #     still proves it.
        #   * the outer `throw` was supposed to buy "slot retained". It does not:
        #     `ci/slot-lock.ps1` reclaims on pure PID liveness, with no clause for a run that asked
        #     for its slot to be held, so the next gate takes it either way. Worse, throwing out of
        #     this `finally` skipped `Remove-SlotClaim` AND the run's own manifest write, which
        #     sits outside the block -- 60 stage records discarded and the original stage failure
        #     replaced by a sentence about PostgreSQL cleanup.
        #
        # So the throw achieved neither of the two things its message claimed, and cost the receipt.
        # The assertion now names what must be TRUE (evidence kept, abort recorded, caller still
        # reaches its manifest) instead of HOW. Found by X's adversarial review of this PR;
        # independently corroborated by lane O's R991.
        #
        # DISCLOSURE, because this is the shape the house warns about: the same lane that removed
        # the throw rewrote the cell that caught it. Read this pair together, not separately.
        Assert-True (-not $cleanupRefused) `
            'a failed drain does NOT throw out of the cleanup: the caller still reaches its slot release and its manifest write'
        Assert-True (Test-Path -LiteralPath $earlyControls[0].CountFile) `
            'and the failing child''s count file survives the failed drain, so its evidence is still on disk'
        Assert-True ($script:abortReasons.Count -eq 1 -and $script:abortReasons[0] -match 'controlled drain failure') 'cleanup failure writes its abort reason before propagating the error'
        Remove-Item -LiteralPath $earlyControls[0].CountFile -Force
        $earlyControls[0].Started.Process | Add-Member -Force -MemberType ScriptMethod -Name WaitForExit -Value { param($milliseconds) return $false }
        $wrapperRefused = $false
        try { Stop-PostgresStageEarly $earlyControls[0] } catch { $wrapperRefused = $_ -match 'wrapper did not exit' }
        Assert-True $wrapperRefused 'an unobserved wrapper exit refuses the empty-job shortcut'

        if ($ObservePostgresAbort) {
            # Optional local observer: real binaries, no Cargo and no shared gate target. The fake
            # test command signals only readiness; it never writes its inherited database secrets.
            $support = Join-Path $PSScriptRoot 'postgres.ps1'
            $marker = Join-Path $fixtureRoot 'ready'
            $sleeper = Join-Path $fixtureRoot 'sleep.ps1'
            [IO.File]::WriteAllText($sleeper, "[IO.File]::WriteAllText('$($marker.Replace("'","''"))','ready'); Start-Sleep -Seconds 300")
            $sleepCommand = "& '$($sleeper.Replace("'","''"))'"
            $sleepEncoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($sleepCommand))
            [IO.File]::WriteAllText($fixturePath, "param([string[]]`$TestArgs) & '$($support.Replace("'","''"))' -PostgresBin '$($PostgresBin.Replace("'","''"))' -TestCommand powershell.exe -TestArgs @('-NoProfile','-EncodedCommand','$sleepEncoded'); exit `$LASTEXITCODE")
            $realStart = [regex]::Match($gateText, '(?ms)^function Start-BackgroundStage \{.*?^\}')
            Invoke-Expression $realStart.Value
            $repositoryRoot = Split-Path -Parent $PSScriptRoot
            $observed = $null
            $server = $null
            $ownedRoot = $null
            try {
                $observed = Start-PostgresStageEarly -Name 'actual PostgreSQL abort observer' -ScriptPath $fixturePath -SupportScriptPath $support
                $deadline = [DateTime]::UtcNow.AddSeconds(60)
                while (-not (Test-Path -LiteralPath $marker) -and -not $observed.Started.Process.HasExited -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
                if (-not (Test-Path -LiteralPath $marker)) { throw "Actual PostgreSQL observer never reached test command; logs: $($observed.Started.Out), $($observed.Started.Err)" }
                $log = Get-Content -LiteralPath $observed.Started.Out -Raw
                $portMatch = [regex]::Match($log, 'Cluster is accepting connections on 127\.0\.0\.1:(\d+)')
                if (-not $portMatch.Success) { throw 'Missing cluster readiness port' }
                $port = [int]$portMatch.Groups[1].Value
                foreach ($dir in Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter 'graphhelm-pg-*') {
                    $pidFile = Join-Path $dir.FullName 'data/postmaster.pid'
                    if (-not (Test-Path -LiteralPath $pidFile)) { continue }
                    $lines = [IO.File]::ReadAllLines($pidFile)
                    if ($lines.Length -gt 3 -and $lines[3] -eq [string]$port) {
                        $server = [Diagnostics.Process]::GetProcessById([int]$lines[0]); $null = $server.Handle
                        $ownedRoot = $dir.FullName
                        break
                    }
                }
                if ($null -eq $server) { throw 'Could not retain ready server identity' }
                Assert-True (-not $server.HasExited -and $observed.Job.ActiveProcesses -gt 1) 'real PostgreSQL is ready and retained inside the parent-owned job'
                Stop-PostgresStageEarly $observed
                Assert-True ($server.WaitForExit(10000) -and $observed.Started.Process.HasExited) 'abort cleanup observes the detached PostgreSQL server and wrapper exit'
                $observed = $null
            } finally {
                if ($null -ne $observed) { Stop-PostgresStageEarly $observed }
                if ($null -ne $server) { $server.Dispose() }
                if ($ownedRoot) {
                    $resolvedOwned = [IO.Path]::GetFullPath($ownedRoot)
                    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
                    if ([IO.Path]::GetDirectoryName($resolvedOwned) -ne $resolvedTemp -or [IO.Path]::GetFileName($resolvedOwned) -notlike 'graphhelm-pg-*') { throw 'Refusing cleanup outside observer temporary root' }
                    Remove-Item -LiteralPath $resolvedOwned -Recurse -Force
                }
                Remove-Item -LiteralPath $marker, $sleeper -Force -ErrorAction SilentlyContinue
            }
        }
    } finally {
        $env:GRAPHHELM_PG_ARGV_FIXTURE = $previousOutput
        Remove-Item -LiteralPath $fixturePath, $fixtureOutput -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $fixtureRoot -Force -ErrorAction SilentlyContinue
    }

    # #1145: a failed artifact build must prevent PostgreSQL clusters from starting. The placement
    # checks above cannot observe whether the calls fire, so drive the main slice with a builder
    # stub and count starts. A successful build is the control; failed and unknown builds fail closed.
    $startedMarker = Join-Path ([IO.Path]::GetTempPath()) ('pg-1145-' + [guid]::NewGuid().ToString('N'))
    $stubbed = @('Start-PostgresStageEarly','Get-TestArtifactManifest','Invoke-Stage','Get-NonCLocale',
                 'Get-WorkspaceFmtTargets','Get-RustfmtPlan','Get-RustfmtStageExit','Get-RustfmtHarnessNote',
                 'Test-StageOverlapped','Invoke-PostgresStage')
    $savedFunctions = @{}
    foreach ($name in $stubbed) { $savedFunctions[$name] = (Get-Item ("Function:" + $name) -ErrorAction SilentlyContinue) }
    try {
        function Start-PostgresStageEarly { param($Name, $Locale, $ScriptPath, $SupportScriptPath) [IO.File]::AppendAllText($startedMarker, "early`n"); return $null }
        function Get-TestArtifactManifest { return @{ buildExitCode = $script:builderExitCode } }
        function Invoke-Stage { param($Name, $Body) }
        function Get-NonCLocale { 'English_United States.1252' }
        function Get-WorkspaceFmtTargets { @() }
        function Get-RustfmtPlan { param($Targets) @() }
        function Get-RustfmtStageExit { param($Codes) 0 }
        function Get-RustfmtHarnessNote { param($Lines) $null }
        function Test-StageOverlapped { param($Records, $Name) $false }
        function Invoke-PostgresStage { param($Name, $CountFile = '', $Body = $null) [IO.File]::AppendAllText($startedMarker, "inline`n") }
        $script:matrixSkipped = $false
        $PostgresBin = $null
        $repositoryRoot = [IO.Path]::GetTempPath()
        $toolchain = '+1.97.1'

        $script:builderExitCode = 0
        Invoke-Expression $startSlice
        Invoke-Expression $joinSlice
        $startedOnSuccess = @(Get-Content -LiteralPath $startedMarker -ErrorAction SilentlyContinue)
        Remove-Item -LiteralPath $startedMarker -Force -ErrorAction SilentlyContinue

        $script:builderExitCode = 2
        Invoke-Expression $startSlice
        Invoke-Expression $joinSlice
        $startedOnFailure = @(Get-Content -LiteralPath $startedMarker -ErrorAction SilentlyContinue)
        Remove-Item -LiteralPath $startedMarker -Force -ErrorAction SilentlyContinue

    $script:builderExitCode = $null
    Invoke-Expression $startSlice
    Invoke-Expression $joinSlice
    $startedOnUnknown = @(Get-Content -LiteralPath $startedMarker -ErrorAction SilentlyContinue)
    Remove-Item -LiteralPath $startedMarker -Force -ErrorAction SilentlyContinue

    # The manifest receives the native `$LASTEXITCODE`, an Int32. Other zero-like values are not a
    # successful artifact result: accepting them would make the start predicate and the strict
    # availability decision disagree. Int64 zero is included deliberately as an unknown shape.
    $zeroLikeCases = @(
        [pscustomobject]@{ Name = 'string zero'; Value = '0' }
        [pscustomobject]@{ Name = 'Boolean false'; Value = [bool]$false }
        [pscustomobject]@{ Name = 'Double zero'; Value = [double]0 }
        [pscustomobject]@{ Name = 'Int64 zero'; Value = [int64]0 }
    )
    $zeroLikeCounts = @{}
    foreach ($case in $zeroLikeCases) {
        $script:builderExitCode = $case.Value
        Invoke-Expression $startSlice
        Invoke-Expression $joinSlice
        $zeroLikeCounts[$case.Name] = @((Get-Content -LiteralPath $startedMarker -ErrorAction SilentlyContinue)).Count
        Remove-Item -LiteralPath $startedMarker -Force -ErrorAction SilentlyContinue
    }
    } finally {
        foreach ($name in $stubbed) {
            $saved = $savedFunctions[$name]
            if ($null -eq $saved) { Remove-Item ("Function:" + $name) -ErrorAction SilentlyContinue }
            else { Set-Item ("Function:" + $name) $saved.ScriptBlock }
        }
        Remove-Item -LiteralPath $startedMarker -Force -ErrorAction SilentlyContinue
    }
    Assert-True (@($startedOnSuccess | Where-Object { $_ -eq 'early' }).Count -eq 2 -and @($startedOnSuccess | Where-Object { $_ -eq 'inline' }).Count -eq 2) `
        'CONTROL: a successful artifact build with early-start failure falls back inline for both matrices'
    Assert-True (@($startedOnFailure).Count -eq 0) 'a failed artifact build starts no PostgreSQL cluster, including at the inline join'
    Assert-True (@($startedOnUnknown).Count -eq 0) 'an artifact manifest with no build result starts no PostgreSQL cluster, including at the inline join'
    Assert-True ($zeroLikeCounts['string zero'] -eq 0) 'a string zero build result starts no PostgreSQL cluster'
    Assert-True ($zeroLikeCounts['Boolean false'] -eq 0) 'a Boolean false build result starts no PostgreSQL cluster'
    Assert-True ($zeroLikeCounts['Double zero'] -eq 0) 'a Double zero build result starts no PostgreSQL cluster'
    Assert-True ($zeroLikeCounts['Int64 zero'] -eq 0) 'an Int64 zero build result is unknown and starts no PostgreSQL cluster'

    # Coverage is part of the same promise: an unavailable matrix must be recorded as skipped, not
    # merely avoided. Execute the real Get-RunCoverage function through the real manifest call
    # expression so removing postgresMatrixUnavailable from that composition turns this cell RED.
    $coverageFunction = [regex]::Match($gateText, '(?ms)^function Get-RunCoverage \{.*?^\}').Value
    Invoke-Expression $coverageFunction
    $coverageCall = [regex]::Match($gateText, '(?m)^\s*\$coverage = Get-RunCoverage .*?$').Value.Trim()
    $script:matrixSkipped = $false
    $script:postgresMatrixUnavailable = $true
    $buildMode = 'unknown'
    $NonPullRequest = $false
    $LandingSnapshotPath = $null
    Invoke-Expression $coverageCall
    Assert-True ($coverage.postgres -eq 'skipped' -and -not $coverage.complete) `
        'an unavailable PostgreSQL matrix is recorded as skipped and makes coverage incomplete'
} catch {
    Write-Host $_
    Write-Host $_.ScriptStackTrace
    throw
} finally {
    $env:GRAPHHELM_PG_COUNT_FILE = $previousCount
    $env:GRAPHHELM_PG_LOCALE = $previousLocale
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-postgres-early: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-postgres-early: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
