# A stage joined from the background prints its evidence into a variable, and the variable is thrown
# away.
#
# `Complete-BackgroundStage` reads the child's stdout and stderr files and re-emits every line with
# `Write-Output`, so that `Invoke-Stage` can capture, redact and tail them exactly as it would for a
# stage that ran in line. It cannot: the call site assigns the join's result --
# `$joined = Complete-BackgroundStage -Started $script:psSuitesStarted` -- and an assignment consumes
# the WHOLE success stream of the function, the child's lines together with the exit code. The lines
# never reach `$capturedLines`, so `outputTail` records `<absent: ... no output ...>` about a child
# that spoke.
#
# MEASURED, on #1009's own red: `ci/run-ps-suites.ps1` exits 2 only after printing
# `HARNESS-BROKE in: <name> (exit 2)`, and the manifest for that run carries
# `outputTail: ["<absent: exit 2 with no output on stdout, stderr or the information stream>"]`.
# 1971 seconds, zero bytes. This is invisible while the stage is green -- the green control manifests
# discard the same output -- so it costs exactly once, on the day the stage reddens and the one line
# that names the cause is the line that goes missing.
#
# WHAT THE SUBJECT IS. Not "the capture writes something": that is satisfied by an empty string. The
# subject is a SENTENCE -- when the child exits 2 having printed `HARNESS-BROKE in: X`, that text is
# in the record the manifest is built from. And it is asserted on BOTH STREAMS, because a fix that
# only carries the stream the child happened to use proves half of a mechanism, and the unproven half
# is where a diagnostic usually goes.
#
# The gate is far too expensive to run per cell, so this suite runs a VERBATIM SLICE of ci/gate.ps1
# cut out by anchor text and never retyped: the real Protect-GateEvidenceLine, the real Invoke-Stage,
# the real Start-BackgroundStage and the real Complete-BackgroundStage. Only the CHILD is a stub --
# a `cmd` that prints on one chosen stream and exits with a chosen code, which is what isolates the
# journey from a detached process's streams to $record.outputTail.

$ExpectedAssertionCount = 10
$ErrorActionPreference = 'Stop'
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) {
        Write-Host "  PASS: $Message" -ForegroundColor Green
    } else {
        $script:failures++
        Write-Host "  FAIL: $Message" -ForegroundColor Red
    }
}

$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

# ARRANGEMENT FIRST, for this suite's own sake: a file that stopped parsing, or an anchor that
# stopped matching, yields an empty slice, and every assertion below would then be about a program
# that was never read -- in the same green as a passing run.
$parseErrors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref] $null, [ref] $parseErrors)
if ($parseErrors.Count -gt 0) {
    Write-Host "HARNESS-BROKE: gate.ps1 does not parse ($($parseErrors.Count) error(s))" -ForegroundColor Magenta
    exit 2
}

function Get-GateSlice {
    param([Parameter(Mandatory)] [string] $Start, [Parameter(Mandatory)] [string] $End)
    $i = $gateText.IndexOf($Start, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -le $i) {
        Write-Host "HARNESS-BROKE: gate.ps1 slice anchors did not match for [$Start]" -ForegroundColor Magenta
        exit 2
    }
    return $gateText.Substring($i, $j - $i)
}

# Invoke-Stage dot-sources gate-evidence.ps1 through $PSScriptRoot on the line ABOVE its own
# definition; a slice that started any earlier would carry that line into a scriptblock which has no
# file and therefore no $PSScriptRoot. Sourced here directly instead, from the real file.
. (Join-Path $PSScriptRoot 'gate-evidence.ps1')

$sliceProtect = Get-GateSlice -Start 'function Protect-GateEvidenceLine {' -End 'function Invoke-Postgres {'
# ONE slice, from Invoke-Stage to the canary comment, because that span is where all three subjects
# live and cutting them apart would let one of them drift out of the suite without a word.
$sliceStage = Get-GateSlice -Start 'function Invoke-Stage {' -End '# #152: rewrites the canary'
. ([scriptblock]::Create($sliceProtect))
. ([scriptblock]::Create($sliceStage))

# Invoke-Stage's own sentinel, defined at gate.ps1:285 and therefore outside every slice above. A
# body that never set an exit code must redden rather than inherit a neighbour's (#896); left
# undefined here, that assignment writes $null and the arms below would read a code the real gate
# never produces.
$MuteStageExitCode = 99

$script:failed = @()
$script:stageRecords = New-Object System.Collections.Generic.List[object]

# One arm: a real detached child, joined through the real functions, in the CALL SHAPE the gate uses.
#
# `$joined = Complete-BackgroundStage ...` is not incidental -- it IS the defect. A helper that
# called the join bare would carry the child's lines to the capture by accident and this suite would
# be green against the broken gate.
function Invoke-BackgroundArm {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $Command
    )
    $script:stageRecords.Clear()
    $script:failed = @()
    $script:lastStageLines = @()
    $started = Start-BackgroundStage -Name $Name -FilePath 'cmd' `
        -ArgumentList @('/c', $Command) -WorkingDirectory $PSScriptRoot
    $threw = $false
    $code = $null
    try {
        $code = Invoke-Stage -Name $Name -Body {
            $joined = Complete-BackgroundStage -Started $started
            if ($null -eq $joined) { throw 'the early start failed; this arm has no subject' }
        }
    } catch {
        $threw = $true
    }
    $hasField = $false
    $tail = @()
    if ($script:stageRecords.Count -gt 0) {
        $record = $script:stageRecords[0]
        # Presence carried by its own boolean, never inferred from emptiness: a missing field and an
        # empty one are the two states this whole family of suites exists to keep apart.
        $hasField = $record.Contains('outputTail')
        if ($hasField) { $tail = @($record.outputTail) }
    }
    return [ordered]@{
        started  = ($null -ne $started)
        exitCode = $code
        threw    = $threw
        hasField = $hasField
        tail     = $tail
        text     = ($tail -join "`n")
        lines    = (@($script:lastStageLines) -join "`n")
    }
}

# The sentence the runner actually prints before it exits 2 (ci/run-ps-suites.ps1:130). Asserted as
# TEXT, so a fix that carries an empty string, or the exit code alone, stays red.
$harnessLine = 'HARNESS-BROKE in: fake-suite'
$absentMarker = '<absent:'

try {
    Assert-True -Condition ($sliceProtect.Length -gt 100 -and
        $sliceStage -match 'function Start-BackgroundStage \{' -and
        $sliceStage -match 'function Complete-BackgroundStage \{') `
        -Message "arrangement: the slices carry Protect ($($sliceProtect.Length) chars), Invoke-Stage, Start-BackgroundStage and Complete-BackgroundStage"

    # THE DEFECT, on the stream the real child used.
    $out = Invoke-BackgroundArm -Name 'background stdout evidence' -Command "echo $harnessLine& exit 2"
    Assert-True -Condition ($out.started -and $out.text -match [regex]::Escape($harnessLine)) `
        -Message "a background child's STDOUT sentence reaches outputTail (tail: '$($out.text)')"
    Assert-True -Condition ($out.exitCode -eq 2) `
        -Message "and the stage still reports the child's own exit code 2 (got $($out.exitCode))"

    # THE OTHER STREAM. `run-ps-suites` prints its refusal on stdout today; nothing makes that
    # permanent, and a diagnostic is exactly the thing that migrates to stderr.
    $err = Invoke-BackgroundArm -Name 'background stderr evidence' -Command "echo $harnessLine 1>&2& exit 2"
    Assert-True -Condition ($err.started -and $err.text -match [regex]::Escape($harnessLine)) `
        -Message "a background child's STDERR sentence reaches outputTail (tail: '$($err.text)')"
    Assert-True -Condition ($err.exitCode -eq 2) `
        -Message "and that stage also reports exit code 2 (got $($err.exitCode))"

    # THE NEGATIVE CONTROL. A capture wired only into the failure path would satisfy every cell
    # above: `outputTail` is written when a stage FAILS. A passing background stage speaks too, and
    # its words are what the run transcript is made of.
    $green = Invoke-BackgroundArm -Name 'background passing stage' -Command 'echo background-child-spoke-and-passed& exit 0'
    Assert-True -Condition ($green.started -and $green.lines -match 'background-child-spoke-and-passed') `
        -Message "a PASSING background child's output still reaches the stage capture (lines: '$($green.lines)')"
    Assert-True -Condition ((-not $green.threw) -and $green.exitCode -eq 0) `
        -Message "and that stage returns 0 without throwing (threw: $($green.threw), code: $($green.exitCode))"

    # #816's marker must survive. A child that genuinely printed nothing has to keep SAYING so,
    # rather than being handed an empty tail that reads identically to evidence that was lost.
    $silent = Invoke-BackgroundArm -Name 'background silent failure' -Command 'exit 2'
    Assert-True -Condition ($silent.started -and $silent.hasField -and $silent.tail.Count -gt 0) `
        -Message "a silent failing background stage records a tail rather than an empty one (field: $($silent.hasField), lines: $($silent.tail.Count))"
    Assert-True -Condition ($silent.text -like "*$absentMarker*") `
        -Message "and that tail names its own absence (tail: '$($silent.text)')"

    # AND THE TWO MUST BE TELLABLE APART, which is the property no cell above proves alone: a fix
    # that wrote the absence marker onto both would pass the marker cell while leaving spoke and
    # silent as indistinguishable as they are today.
    Assert-True -Condition ($out.text -ne $silent.text) `
        -Message 'a background child that spoke and one that did not no longer share one representation'
} finally {
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        # A suite that stops running some of its cells must not report the colour of the cells it did
        # run: the count is the only thing that can tell a green from a green-shaped gap.
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-background-stage-evidence: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-background-stage-evidence: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
