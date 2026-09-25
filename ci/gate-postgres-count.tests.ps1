# #909 WIRING: the parser is not the fix. `Invoke-PostgresStage` reddening on `none`, and
# `Write-RunManifest` carrying the count, are the two arms that make the count matter.
#
# ISSUES 2 found this on f99b269c and blocked, correctly: with only test-count.tests.ps1 in place,
# deleting the line that adds a `none` stage to $failed (S3), or deleting the manifest field (S4),
# left EVERY suite in the repository green -- including the one this PR adds. A guarded predicate
# with unguarded arms is the defect moved, not removed; the same root C raised on #856 and K's AST
# cells on #833.
#
# `Invoke-PostgresStage` is cut out of ci/gate.ps1 by anchor and RUN here against stubs, rather than
# matched as text: a text cell proves the line is present, and running it proves the line decides.
# The stubs are only the two things that would start a real cluster.
# 23, and I did not get there by counting the file. I wrote 12, the harness answered HARNESS-BROKE
# with 15; after (e) and (f) landed I wrote 20 and it answered 23. Both misses are the same shape: (a)
# loops over three cases and (e) over two variants, each contributing two assertions, and a loop reads
# as one block to the eye. Twice in one file is the argument for declaring the total rather than
# trusting the count -- a suite that silently lost a block would otherwise still report "N/N passed".
$ExpectedAssertionCount = 23

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

Write-Host "`n=== ARRANGEMENT: the function is extractable, or these cells measure nothing ==="
# Non-greedy to the first line that closes at column 0: gate.ps1's functions are top-level.
$fnMatch = [regex]::Match($gateText, '(?ms)^function Invoke-PostgresStage \{.*?^\}')
Assert-True ($fnMatch.Success) 'ARRANGEMENT: Invoke-PostgresStage is cut out of gate.ps1 by anchor'
$fnText = $fnMatch.Value

Write-Host "`n=== (a) the arm: a 'none' verdict reddens the stage, an 'unknown' one does not ==="
foreach ($case in @(
    @{ verdict = 'none';    groups = 15; expectFailed = $true;
       why = "a stage that reported summaries and executed nothing MUST reach `$failed" },
    @{ verdict = 'unknown'; groups = 0;  expectFailed = $false;
       why = "a stage whose count could not be read must NOT redden -- 'I could not see' is not 'it ran nothing'" },
    @{ verdict = 'measured'; groups = 3; expectFailed = $false;
       why = 'a stage that measured something is left alone' }
)) {
    # Fresh state per case, in THIS script's scope, which is the scope the extracted function writes to.
    $script:failed = @()
    $script:postgresExecution = [ordered]@{}
    $script:caseVerdict = $case.verdict
    $script:caseGroups = $case.groups

    # The two stubs, and only these two: everything else in the function runs for real.
    function Invoke-Stage { param($Name, $Body) & $Body }
    function Invoke-Postgres {
        # What the real postgres.ps1 writes: flat key=value, LF, no BOM.
        $payload = @(
            "verdict=$($script:caseVerdict)",
            "executed=0",
            "groups=$($script:caseGroups)",
            "stdoutLines=1847"
        ) -join "`n"
        [System.IO.File]::WriteAllText($env:GRAPHHELM_PG_COUNT_FILE, $payload + "`n",
            [System.Text.UTF8Encoding]::new($false))
    }

    . ([scriptblock]::Create($fnText))
    Invoke-PostgresStage -Name 'PostgreSQL probe stage' | Out-Null

    $reddened = @($script:failed) -contains 'PostgreSQL probe stage'
    Assert-True ($reddened -eq $case.expectFailed) "($($case.verdict)) $($case.why)"
    Assert-True ($script:postgresExecution.Contains('PostgreSQL probe stage')) `
        "($($case.verdict)) the stage's count is recorded whatever the verdict, so the manifest can carry it"
}

Write-Host "`n=== (b) the count read back is the one the child wrote, not a default ==="
$script:failed = @()
$script:postgresExecution = [ordered]@{}
$script:caseVerdict = 'measured'
$script:caseGroups = 3
. ([scriptblock]::Create($fnText))
Invoke-PostgresStage -Name 'PostgreSQL probe stage' | Out-Null
$record = $script:postgresExecution['PostgreSQL probe stage']
Assert-True ($record.verdict -eq 'measured') '(b1) the verdict crosses the file boundary'
Assert-True ($record.groups -eq 3) '(b2) and so does an integer field'
Assert-True ($record.stdoutLines -eq 1847) '(b3) stdoutLines crosses too, so the truncation stays visible in the record'

Write-Host "`n=== (c) an unreadable count is UNKNOWN, never a zero ==="
# No count file at all: the child died, or never wrote. The record must not invent a measurement.
$script:failed = @()
$script:postgresExecution = [ordered]@{}
function Invoke-Postgres { }   # writes nothing
. ([scriptblock]::Create($fnText))
Invoke-PostgresStage -Name 'PostgreSQL silent stage' | Out-Null
Assert-True ($script:postgresExecution['PostgreSQL silent stage'].verdict -eq 'unknown') `
    '(c1) a stage that published no count reads unknown'
Assert-True ((@($script:failed) -contains 'PostgreSQL silent stage') -eq $false) `
    '(c2) and does not redden: a reader that saw nothing must not condemn the stage'

Write-Host "`n=== (d) the manifest arm: the field is in the object Write-RunManifest builds ==="
# Text, with both controls, because the manifest object is built inside a function that writes files.
# The counter must see the ASSIGNMENT and not a mention, or a comment naming the field would pass it.
function Measure-ManifestKey {
    param([Parameter(Mandatory)] [string] $Text, [Parameter(Mandatory)] [string] $Key)
    @([regex]::Matches($Text, '(?m)^\s*' + [regex]::Escape($Key) + '\s*=\s*\S')).Count
}
Assert-True ((Measure-ManifestKey -Text $gateText -Key 'postgresExecution') -ge 1) `
    '(d1) postgresExecution is assigned in gate.ps1, so the count reaches the manifest'
Assert-True ((Measure-ManifestKey -Text "# postgresExecution is a nice idea" -Key 'postgresExecution') -eq 0) `
    '(d2) CONTROL: a prose mention is not an assignment, so (d1) is about the field and not about the word'
Assert-True ((Measure-ManifestKey -Text "        postgresExecution  = `$script:postgresExecution" -Key 'postgresExecution') -eq 1) `
    '(d3) CONTROL: the counter does see a real assignment line, so (d1) is not vacuous'

Write-Host "`n=== (e) #909 regression: a run with NO stdout must not kill the counter ==="
# Found by the unnumbered ISSUES lane at f99b269c. `Tee-Object -Variable` assigns only when the
# pipeline produces an object; postgres.ps1 runs under `Set-StrictMode -Version 2.0`, where reading an
# unset variable THROWS -- and cargo writes compile diagnostics to stderr, so a build failure is
# exactly the non-zero-exit, empty-stdout case. The throw lands before `exit $exitCode` on the last
# line of the file, so 101 is reported as 1 and no count file is written.
#
# EXECUTED, not asserted about the real file: the hazard lives in a script that starts a PostgreSQL
# cluster before it reaches this code, so the pattern is run in a child PowerShell under the same
# StrictMode, once WITHOUT the fix and once WITH it. That pair is what proves the line is load-bearing
# rather than decorative. The cell below it pins the line's presence in the real subject.
$probeDir = Join-Path ([System.IO.Path]::GetTempPath()) ("pg-count-probe-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force $probeDir | Out-Null
try {
    foreach ($variant in @(
        @{ name = 'WITHOUT the initializer'; init = ''; expectOk = $false },
        @{ name = 'WITH the initializer'; init = '$capturedStdout = @()'; expectOk = $true }
    )) {
        $body = @(
            # Single-quoted throughout. My first version built this array with DOUBLE quotes, so
            # PowerShell expanded $ErrorActionPreference at build time and the probe received
            # `Stop = 'Continue'` -- it died on a term it could not resolve, and the cell reported a
            # failure that was about my quoting rather than about the subject. The probe needs no
            # preference line at all: `cmd /c "exit 7"` writes to neither stream.
            'Set-StrictMode -Version 2.0',
            $variant.init,
            '& cmd /c "exit 7" | Tee-Object -Variable capturedStdout',
            '$code = $LASTEXITCODE',
            'try {',
            '    $lines = @($capturedStdout | ForEach-Object { [string] $_ })',
            '    Write-Host "READ-OK code=$code lines=$($lines.Count)"',
            '} catch {',
            '    Write-Host "READ-THREW: $($_.Exception.Message)"',
            '}'
        ) -join "`n"
        $probePath = Join-Path $probeDir 'probe.ps1'
        [System.IO.File]::WriteAllText($probePath, $body, [System.Text.UTF8Encoding]::new($false))
        $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $probePath 2>&1 | Out-String

        $readOk = $out -match 'READ-OK code=7 lines=0'
        Assert-True ($readOk -eq $variant.expectOk) `
            "(e) $($variant.name): the read $(if ($variant.expectOk) { 'succeeds and keeps exit 7' } else { 'throws' })"
        if (-not $variant.expectOk) {
            Assert-True ($out -match 'READ-THREW') `
                '(e) CONTROL: without the initializer the failure is the unset-variable throw, not something else'
        } else {
            Assert-True ($out -notmatch 'READ-THREW') `
                '(e) with the initializer nothing throws, so a failing run still reaches its exit code'
        }
    }
} finally { Remove-Item -Recurse -Force $probeDir -ErrorAction SilentlyContinue }

Write-Host "`n=== (f) and the line is in the real subject, before the pipe ==="
$pgText = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'postgres.ps1'))
$initIndex = $pgText.IndexOf('$capturedStdout = @()', [System.StringComparison]::Ordinal)
$teeIndex = $pgText.IndexOf('| Tee-Object -Variable capturedStdout', [System.StringComparison]::Ordinal)
Assert-True ($initIndex -ge 0) '(f1) postgres.ps1 declares $capturedStdout'
Assert-True ($teeIndex -ge 0) '(f2) ARRANGEMENT: the Tee line is still the one that assigns it'
Assert-True (($initIndex -ge 0) -and ($teeIndex -ge 0) -and ($initIndex -lt $teeIndex)) `
    '(f3) and the declaration comes BEFORE the pipe, which is the only order that helps'
Assert-True ($pgText.IndexOf('Set-StrictMode -Version 2.0', [System.StringComparison]::Ordinal) -ge 0) `
    '(f4) CONTROL: postgres.ps1 really does run under StrictMode, or (e) is about a hazard it does not have'

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount" -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "$($script:total - $script:failures)/$($script:total) passed, $($script:failures) FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "$($script:total)/$($script:total) passed" -ForegroundColor Green
exit 0
