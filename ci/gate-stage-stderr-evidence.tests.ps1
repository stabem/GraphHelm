# #816: a stage whose evidence goes to STDERR records an empty tail, and an empty tail is also
# what a stage that printed NOTHING records.
#
# Invoke-Stage merges the information stream into the pipeline (`& $Body 6>&1`) so Write-Host
# output reaches $capturedLines (#484). Native stderr is in neither of those streams, so a tool
# that diagnoses on stderr -- which is where compilers and test harnesses put diagnostics -- is
# printed to the console by the shell itself and never enters the capture. The manifest then names
# the failing stage and not one word of what it saw, while the line was on screen the whole time.
#
# The CONTROL is the half that shapes the fix. A stage that printed nothing at all also produces an
# empty tail, so `outputTail: []` means EITHER "nothing was printed" OR "everything went to
# stderr": two states with one representation (#751, #755). Capturing stderr is therefore necessary
# and not sufficient -- what remains empty after the capture has to SAY it is empty and why, the
# way #858 writes `head=unknown` instead of leaving a hole.
#
# The gate is far too expensive to run per cell, so this suite runs a VERBATIM SLICE of ci/gate.ps1
# cut out by anchor text and never retyped: the real Protect-GateEvidenceLine and the real
# Invoke-Stage. The bodies are stubs, because the subject is the journey from a child process's
# streams to $record.outputTail, and a stub that writes to one chosen stream is what isolates it.

$ExpectedAssertionCount = 12
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

# ARRANGEMENT FIRST. A file that stopped parsing, or an anchor that stopped matching, yields an
# empty slice -- and every assertion below would then be about a program that was never read, in
# the same green as a passing run. That is the failure this whole suite is about, so it is the one
# failure the suite must not commit itself.
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
# definition; a slice that started any earlier would carry that line into a scriptblock which has
# no file and therefore no $PSScriptRoot. Sourced here directly instead, from the real file.
. (Join-Path $PSScriptRoot 'gate-evidence.ps1')

$sliceProtect = Get-GateSlice -Start 'function Protect-GateEvidenceLine {' -End 'function Invoke-Postgres {'
$sliceStage = Get-GateSlice -Start 'function Invoke-Stage {' -End '# #152: rewrites the canary'
. ([scriptblock]::Create($sliceProtect))
. ([scriptblock]::Create($sliceStage))

$script:failed = @()
$script:stageRecords = New-Object System.Collections.Generic.List[object]

# Each arm is one stage, run through the real function, and read back off the record the gate would
# have written into the manifest.
function Get-StageTail {
    param([Parameter(Mandatory)] [string] $Name, [Parameter(Mandatory)] [scriptblock] $Body)
    $script:stageRecords.Clear()
    $script:failed = @()
    # A stage that THROWS is a distinct outcome from one that fails, and the difference is the
    # whole point of arm D below: without $ErrorActionPreference='Continue' a redirected native
    # stderr line becomes a terminating NativeCommandError, so the stage does not return a code at
    # all. Caught here so that outcome arrives as a named FAIL on the cell that asserts it, instead
    # of killing the suite mid-run -- which is a non-zero exit for a reason no cell states, and
    # therefore the same unreadable red this whole file exists to replace.
    $threw = $false
    $code = $null
    try {
        $code = Invoke-Stage -Name $Name -Body $Body
    } catch {
        $threw = $true
    }
    if ($script:stageRecords.Count -eq 0) {
        return [ordered]@{ exitCode = $code; threw = $threw; hasField = $false; tail = @(); text = ''; failedNames = @($script:failed) }
    }
    $record = $script:stageRecords[0]
    # `$x = if (...) { @() }` yields $NULL, not an empty array: a block whose value is an empty
    # collection emits nothing at all. Written that way, this helper reported a missing field and
    # an empty field identically -- the same two-states-one-representation defect the suite exists
    # to catch, in the instrument that measures it. The presence of the field is therefore carried
    # by its own boolean, never inferred from the emptiness of its value.
    $hasField = $record.Contains('outputTail')
    $tail = @()
    if ($hasField) { $tail = @($record.outputTail) }
    return [ordered]@{ exitCode = $code; threw = $threw; hasField = $hasField; tail = $tail; text = ($tail -join "`n"); failedNames = @($script:failed) }
}

# The marker the fix must write when nothing was captured. Named here, asserted below, so that a
# fix which captures stderr but leaves the silent stage as an empty array still reddens: an empty
# field is the ambiguity this issue exists to remove, not a smaller version of it.
$absentMarker = '<absent:'

try {
    Assert-True -Condition ($sliceProtect.Length -gt 100 -and $sliceStage.Length -gt 100) `
        -Message "arrangement: both gate.ps1 slices were cut (Protect $($sliceProtect.Length) chars, Invoke-Stage $($sliceStage.Length) chars)"

    # CONTROL ON THE GREEN SIDE FIRST. A passing stage records no tail at all, and must keep not
    # recording one -- otherwise the fix could 'pass' every cell below by writing a marker onto
    # every stage in the gate, green ones included.
    $ok = Get-StageTail -Name 'green stage' -Body { cmd /c "echo nothing-to-see & exit 0" }
    Assert-True -Condition ($ok.exitCode -eq 0) `
        -Message "control: a passing stage returns its exit code 0 (got $($ok.exitCode))"
    Assert-True -Condition (-not $ok.hasField) `
        -Message 'control: a passing stage records no outputTail at all'

    # REGRESSION SIDE. Evidence on stdout is captured today and must stay captured.
    $out = Get-StageTail -Name 'stdout evidence' -Body { cmd /c "echo proof-on-stdout & exit 101" }
    Assert-True -Condition ($out.text -match 'proof-on-stdout') `
        -Message "a failing stage's STDOUT evidence is in outputTail (tail: '$($out.text)')"

    # The 6>&1 half of the merge, guarded so it cannot be traded away. The fix adds a stream; it
    # does not swap one for another, and #484 is the record of what the information stream costs
    # when it is missing.
    $info = Get-StageTail -Name 'information stream evidence' -Body { Write-Host 'proof-on-stream-six'; cmd /c "exit 101" }
    Assert-True -Condition ($info.text -match 'proof-on-stream-six') `
        -Message "a failing stage's Write-Host evidence is still in outputTail (#484 stays fixed)"

    # THE DEFECT (#816). Red before the fix: the line is printed to the console by the shell and
    # never reaches $capturedLines.
    $err = Get-StageTail -Name 'stderr evidence' -Body { cmd /c "echo proof-on-stderr 1>&2 & exit 101" }
    Assert-True -Condition ($err.text -match 'proof-on-stderr') `
        -Message "a failing stage's STDERR evidence is in outputTail (tail: '$($err.text)')"

    # THE OTHER HALF. A stage that genuinely printed nothing must say so in words, not with an
    # empty array that reads identically to evidence that was lost.
    $silent = Get-StageTail -Name 'no output at all' -Body { cmd /c "exit 101" }
    Assert-True -Condition ($silent.hasField -and $silent.tail.Count -gt 0) `
        -Message "a silent failing stage records a tail rather than an empty one (field present: $($silent.hasField), lines: $($silent.tail.Count))"
    Assert-True -Condition ($silent.text -like "*$absentMarker*") `
        -Message "a silent failing stage's tail names its own absence (tail: '$($silent.text)')"

    # ARM D: THE GREEN SIDE OF THE SAME MERGE (L's finding on this pull request). Every cell above
    # watches a stage that FAILS, so all of them survive the removal of `$ErrorActionPreference =
    # 'Continue'` a few lines above the try -- and that line is what keeps 5.1's manufactured
    # NativeCommandError non-terminating now that stderr is redirected. Delete it and the stages
    # that break are the NOISY GREEN ones: cargo writes progress to stderr on a perfectly good
    # build, so a passing stage would throw instead of returning 0, and the gate would redden on
    # every green stage that happened to say something. Nothing here observed that, which made the
    # safety argument in the comment a claim with no cell behind it.
    #
    # Measured on this head before this arm existed: deleting that line left the suite at exit 2
    # with 5 of 9 cells run -- caught, but by the assertion COUNT, in a colour that names no
    # property. This arm names it.
    $noisyGreen = Get-StageTail -Name 'noisy but passing stage' -Body { cmd /c "echo noise-on-stderr 1>&2 & exit 0" }
    Assert-True -Condition (-not $noisyGreen.threw) `
        -Message 'a stage that writes to stderr and SUCCEEDS does not throw (the Continue preference is load-bearing now that stderr is redirected)'
    Assert-True -Condition ($noisyGreen.exitCode -eq 0) `
        -Message "and returns its exit code 0 (got $(if ($null -eq $noisyGreen.exitCode) { 'no code: it threw' } else { $noisyGreen.exitCode }))"
    # `failedNames` alone PASSES under the sabotage, measured: a stage that throws never reaches the
    # accounting, so its name is absent for the wrong reason. The cell has to require that the
    # stage RAN and was not counted, or it reads a crash as a clean pass.
    Assert-True -Condition ((-not $noisyGreen.threw) -and $noisyGreen.failedNames.Count -eq 0) `
        -Message "and ran to completion without being counted as a failed stage (threw: $($noisyGreen.threw), failed: $($noisyGreen.failedNames.Count))"

    # AND THEY MUST BE TELLABLE APART, which is the property neither cell above proves on its own:
    # a fix that wrote the absence marker for BOTH would satisfy the marker cell while leaving the
    # two states as indistinguishable as they are today.
    Assert-True -Condition ($err.text -ne $silent.text) `
        -Message 'lost-to-stderr and genuinely-silent no longer share one representation'
} finally {
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        # A suite that stops running some of its cells must not report the colour of the cells it
        # did run: the count is the only thing that can tell a green from a green-shaped gap.
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-stage-stderr-evidence: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-stage-stderr-evidence: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
