# #896: Invoke-Stage judged a stage by the AMBIENT exit code. `$LASTEXITCODE` is written only by a
# native command, so a body that failed on the PowerShell side wrote nothing and the stage inherited
# its neighbour's verdict. The fix poisons the code before the body runs -- a mute body FAILS with
# the sentinel, and the neighbour's value is destroyed so inheritance is impossible.
#
# The real `Invoke-Stage` is cut out of ci/gate.ps1 by anchor text (the helper region, the same cut
# gate-stage-reddens.tests.ps1 makes) and run in a child PowerShell against three bodies. Nothing
# here runs the gate.
$ExpectedAssertionCount = 18

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

$parseErrors = $null
$null = [System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref]$null, [ref]$parseErrors)
Assert-True ($parseErrors.Count -eq 0) 'ARRANGEMENT: gate.ps1 parses, so what follows is measured rather than empty'

function Get-GateSlice {
    param([Parameter(Mandatory)] [string] $Start, [Parameter(Mandatory)] [string] $End)
    $i = $gateText.IndexOf($Start, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -le $i) { throw "HARNESS-BROKE: gate.ps1 slice anchors did not match for [$Start]" }
    return $gateText.Substring($i, $j - $i)
}

# The helper region: constants, Invoke-Stage, and the dot-source of gate-evidence.ps1 (copied beside
# the slice so that line resolves). Same cut as gate-stage-reddens.
$sliceHelpers = Get-GateSlice -Start '$toolchain = ' -End '# #152: rewrites the canary'
Assert-True ($sliceHelpers.IndexOf('function Invoke-Stage {', [System.StringComparison]::Ordinal) -ge 0) 'ARRANGEMENT: the slice carries the real Invoke-Stage, so the subject is production text'

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-stage-verdict-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'gate-evidence.ps1') -Destination (Join-Path $fixtureRoot 'gate-evidence.ps1') -Force

# Runs the sliced Invoke-Stage over `$BodyText` in a child PowerShell, after a native command has
# left `$Prior` in $LASTEXITCODE -- the neighbour whose verdict a mute body used to inherit.
# Prints one line per record: name|passed|exitCode.
function Invoke-StageSlice {
    param([Parameter(Mandatory)] [string] $BodyText, [int] $Prior = 0)
    $slicePath = Join-Path $fixtureRoot "stage-$([guid]::NewGuid().ToString('N')).ps1"
    $script = @(
        "`$ErrorActionPreference = 'Stop'"
        "`$repositoryRoot = '$($fixtureRoot.Replace("'", "''"))'"
        $sliceHelpers
        "& cmd /c exit $Prior"
        "`$null = Invoke-Stage 'probe' { $BodyText }"
        "foreach (`$r in `$script:stageRecords) { Write-Output ('REC|' + `$r.name + '|' + `$r.passed + '|' + `$r.exitCode) }"
        "Write-Output ('FAILED|' + (`$script:failed -join ','))"
        'exit 0'
    ) -join [Environment]::NewLine
    [System.IO.File]::WriteAllText($slicePath, $script, $utf8NoBom)
    $output = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $slicePath 2>&1 | ForEach-Object { [string]$_ })
    $rec = @($output | Where-Object { $_.StartsWith('REC|', [System.StringComparison]::Ordinal) })
    $failed = @($output | Where-Object { $_.StartsWith('FAILED|', [System.StringComparison]::Ordinal) })
    if ($rec.Count -ne 1) { throw "HARNESS-BROKE: expected one stage record, got $($rec.Count): $($output -join ' / ')" }
    $parts = $rec[0].Split('|')
    return [ordered]@{ passed = [bool]::Parse($parts[2]); exitCode = [int]$parts[3]; failed = $failed[0].Substring(7) }
}


# #841: TWO stages in one child, the second of which THROWS -- which the single-stage helper above
# cannot express, because it requires exactly one record and a throwing stage produces none.
#
# Returns what the two script-scoped captures hold AFTER the throw, joined with `~` so one output
# line carries a whole transcript. The markers are printed by `cmd`, not by PowerShell, because the
# capture path under test is the native-command one: `& $Body 2>&1 6>&1 | ForEach-Object {...}`.
function Invoke-ThrowingSecondStage {
    param([Parameter(Mandatory)] [string] $SecondBodyText)
    $slicePath = Join-Path $fixtureRoot "throwing-$([guid]::NewGuid().ToString('N')).ps1"
    $script = @(
        "`$ErrorActionPreference = 'Stop'"
        "`$repositoryRoot = '$($fixtureRoot.Replace("'", "''"))'"
        $sliceHelpers
        "`$null = Invoke-Stage 'first' { & cmd /c `"echo STAGE-A-MARKER & exit 0`" }"
        "`$threw = `$false"
        "try { `$null = Invoke-Stage 'second' { $SecondBodyText } } catch { `$threw = `$true }"
        "Write-Output ('THREW|' + `$threw)"
        "Write-Output ('LAST|' + ((`$script:lastStageLines) -join '~'))"
        "Write-Output ('ALL|' + ((`$script:allStageLines) -join '~'))"
        'exit 0'
    ) -join [Environment]::NewLine
    [System.IO.File]::WriteAllText($slicePath, $script, $utf8NoBom)
    $output = @(& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $slicePath 2>&1 |
            ForEach-Object { [string]$_ })
    $get = {
        param($prefix)
        $hit = @($output | Where-Object { $_.StartsWith($prefix, [System.StringComparison]::Ordinal) })
        if ($hit.Count -ne 1) { throw "HARNESS-BROKE: expected one $prefix line, got $($hit.Count): $($output -join ' / ')" }
        return $hit[0].Substring($prefix.Length)
    }
    return [ordered]@{
        threw = [bool]::Parse((& $get 'THREW|'))
        last  = (& $get 'LAST|')
        all   = (& $get 'ALL|')
    }
}

try {
    Write-Host ''
    Write-Host '-- CONTROL: a body that speaks is judged by what it said --' -ForegroundColor Cyan
    $spoke0 = Invoke-StageSlice -BodyText '& cmd /c exit 0' -Prior 7
    Assert-True ($spoke0.passed -and $spoke0.exitCode -eq 0) "a body ending in a native exit 0 passes, whatever the neighbour left (got passed=$($spoke0.passed) code=$($spoke0.exitCode))"
    $spoke3 = Invoke-StageSlice -BodyText '& cmd /c exit 3' -Prior 0
    Assert-True ((-not $spoke3.passed) -and $spoke3.exitCode -eq 3) "a body ending in a native exit 3 fails with 3, its own code (got passed=$($spoke3.passed) code=$($spoke3.exitCode))"
    Assert-True ($spoke3.failed -eq 'probe') 'and the failing stage is named in $failed'

    Write-Host ''
    Write-Host '-- THE DEFECT: a mute body no longer inherits its neighbour --' -ForegroundColor Cyan
    $muteAfterPass = Invoke-StageSlice -BodyText "Write-Host 'pure PowerShell, no native command'" -Prior 0
    Assert-True (-not $muteAfterPass.passed) 'a body that sets no exit code FAILS -- even after a neighbour that passed (the inheritance that used to read as a pass)'
    Assert-True ($muteAfterPass.exitCode -eq 99) "and its recorded code is the sentinel 99, not the neighbour's 0 (got $($muteAfterPass.exitCode))"
    $muteAfterFail = Invoke-StageSlice -BodyText "Write-Host 'still mute'" -Prior 7
    Assert-True ((-not $muteAfterFail.passed) -and $muteAfterFail.exitCode -eq 99) "a mute body after a neighbour that failed with 7 reports 99, not 7: the neighbour's value was destroyed (got $($muteAfterFail.exitCode))"
    Assert-True ($muteAfterPass.failed -eq 'probe' -and $muteAfterFail.failed -eq 'probe') 'and both mute stages land in $failed by name'

    Write-Host ''
    Write-Host '-- a PowerShell-side failure inside the body is a red, not a pass --' -ForegroundColor Cyan
    $lookupFailed = Invoke-StageSlice -BodyText "Get-ChildItem -LiteralPath (Join-Path `$repositoryRoot 'no-such-root') -ErrorAction SilentlyContinue | Out-Null" -Prior 0
    Assert-True ((-not $lookupFailed.passed) -and $lookupFailed.exitCode -eq 99) "a lookup that found nothing and set no code is 99/FAIL, not 'zero findings, therefore pass' (got passed=$($lookupFailed.passed) code=$($lookupFailed.exitCode))"

    Write-Host ''
    Write-Host '-- the wiring, read from the file --' -ForegroundColor Cyan
    # THE PROPERTY IS "NO STATEMENT RUNS BETWEEN THEM", not "the two are within 120 characters".
    # The distance was a proxy for it and the literal `& $Body 6>&1` pinned a redirection list that
    # is not this cell's subject: #816 adds `2>&1` to the same invocation, which moved the text and
    # broke this cell while changing nothing it is about. A comment between the two is harmless -- a
    # comment does not run -- and a native command is the whole danger, so the cell now reads the
    # lines BETWEEN and requires every one of them to be blank or a comment (K, rebasing #882).
    $poisonAt = $gateText.IndexOf('$global:LASTEXITCODE = $MuteStageExitCode', [System.StringComparison]::Ordinal)
    $bodyAt = $gateText.IndexOf('& $Body ', [System.StringComparison]::Ordinal)
    # The anchors' success is its own boolean and is never inferred from the emptiness of the
    # result: `$x = if (...) { @() }` yields $NULL, so a clean between-region and a failed anchor
    # search came back identical -- two states, one representation, in the instrument that measures
    # it. That is the #816 defect, and it caught its own author while writing this line.
    $anchorsFound = ($poisonAt -ge 0 -and $bodyAt -gt $poisonAt)
    $between = @()
    if ($anchorsFound) {
        $between = @($gateText.Substring($poisonAt, $bodyAt - $poisonAt) -split "`n" |
                Select-Object -Skip 1 |
                Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and $_.TrimStart() -notmatch '^#' })
    }
    Assert-True ($anchorsFound -and $between.Count -eq 0) `
        "no statement runs between the poison and the body, so nothing can overwrite it (anchors found: $anchorsFound; offending lines: $($between.Count)$(if ($between.Count) { ' -> ' + ($between -join ' | ') }))"
    Assert-True ($gateText.IndexOf('$MuteStageExitCode = 99', [System.StringComparison]::Ordinal) -ge 0) 'the sentinel is the named 99, not a bare literal'
    Assert-True ($gateText.IndexOf('$global:LASTEXITCODE = 0', [System.StringComparison]::Ordinal) -lt 0) 'and nothing clears the code to 0 before a body, which would turn every mute body into a pass'

    Write-Host ''
    Write-Host '-- #841: a stage that THROWS publishes its own capture, never the neighbour''s --' -ForegroundColor Cyan

    # The body prints through `cmd` and THEN throws, so there is real captured output to publish and
    # a terminating error to skip past. Before the fix the two publications were the last statements
    # of the `try`, so this arrangement skipped both.
    $threwAfterPrinting = Invoke-ThrowingSecondStage -SecondBodyText '& cmd /c "echo STAGE-B-MARKER & exit 0"; throw ''deliberate'''

    # ARRANGEMENT, asserted rather than assumed: if the body stopped throwing, every cell below would
    # pass for the wrong reason -- they would be measuring the ordinary path the other 13 already cover.
    Assert-True $threwAfterPrinting.threw `
        'ARRANGEMENT: the second stage really did throw, so what follows is measured on the unwinding path'

    # THE DEFECT. Not "the value is empty" -- empty would redden and send someone looking. The value
    # was the PREVIOUS STAGE'S transcript, which answers confidently about the wrong subject.
    Assert-True ($threwAfterPrinting.last -notmatch 'STAGE-A-MARKER') `
        "after a stage that threw, `$script:lastStageLines does not still hold the PREVIOUS stage's capture (got [$($threwAfterPrinting.last)])"

    # The other half, and it is what makes the first a fix rather than a clearing. Assigning `$null`
    # on entry would satisfy the cell above and lose the evidence the throwing stage did produce.
    Assert-True ($threwAfterPrinting.last -match 'STAGE-B-MARKER') `
        "and it holds what the throwing stage itself printed before it threw (got [$($threwAfterPrinting.last)])"

    # THE RUNNING CONCATENATION, which `required-features coverage` writes out as the run transcript.
    # A throwing stage used to contribute nothing to it, so whatever it had printed was absent from
    # the record entirely -- the transcript is evidence, and evidence that silently omits a stage is
    # worse than a short one.
    Assert-True ($threwAfterPrinting.all -match 'STAGE-A-MARKER' -and $threwAfterPrinting.all -match 'STAGE-B-MARKER') `
        "and the run transcript keeps BOTH stages, so a stage that threw is not missing from the record (got [$($threwAfterPrinting.all)])"

    # CONTROL: publishing in the `finally` did not swallow the error it now survives. A fix that
    # recorded the evidence and ate the exception would pass all four cells above and turn every
    # throwing stage into a silent pass, which is the #896 defect wearing a different hat.
    $ordinary = Invoke-ThrowingSecondStage -SecondBodyText '& cmd /c "echo STAGE-B-MARKER & exit 0"'
    Assert-True ((-not $ordinary.threw) -and ($ordinary.last -match 'STAGE-B-MARKER') -and ($ordinary.last -notmatch 'STAGE-A-MARKER')) `
        "CONTROL: a second stage that does NOT throw is unchanged -- no throw, its own capture, not the neighbour's (threw=$($ordinary.threw), last=[$($ordinary.last)])"

} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "$($script:total)/$($script:total) passed" -ForegroundColor Green
