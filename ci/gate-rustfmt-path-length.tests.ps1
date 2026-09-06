# #895: `cargo fmt --all` hands rustfmt every target of the workspace on ONE command line. On a
# long bench path that line crosses the Windows CreateProcess limit (32767 characters) and cargo-fmt
# dies with `os error 206` before rustfmt reads a byte -- exit 1, and until #882 an EMPTY tail. A
# red with no formatting cause. Measured on J's #826 gate (manifest 12add04e): the same head, the
# same toolchain, rc=1 from a 140-character scratchpad prefix and rc=0 from a 39-character one.
#
# Model, counted on 193674dc: 231 entry files (src/lib.rs, src/main.rs, src/bin/*, tests/*,
# examples/*, benches/*), 9405 characters of relative path. At prefix 39 the `--all` line is about
# 18.7k; at prefix 140 about 42k. The largest package (apps/cli, 45 entry files) stays under 9k at
# prefix 140. So the plan is: `--all` when it fits, one invocation per package when it does not,
# and chunks of a package when even that does not fit -- decided from lengths, never from a guess.
#
# The suite runs VERBATIM SLICES of ci/gate.ps1 cut by anchor text (the planner, the harness
# note, and the stage body), never retyped. Bodies are not executed against cargo: the subject is
# the plan and the classification, and both are pure.

$ExpectedAssertionCount = 16
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

# ARRANGEMENT FIRST: a gate that stopped parsing, or an anchor that stopped matching, yields an
# empty slice and every assertion below would be about a program that was never read.
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

$slicePlan = Get-GateSlice -Start '# BEGIN Get-RustfmtPlan' -End '# END Get-RustfmtPlan'
$sliceNote = Get-GateSlice -Start '# BEGIN Get-RustfmtHarnessNote' -End '# END Get-RustfmtHarnessNote'
$sliceExit = Get-GateSlice -Start '# BEGIN Get-RustfmtStageExit' -End '# END Get-RustfmtStageExit'
$sliceStage = Get-GateSlice -Start "Invoke-Stage 'rustfmt'" -End "Invoke-Stage 'clippy"
. ([scriptblock]::Create($slicePlan))
. ([scriptblock]::Create($sliceNote))
. ([scriptblock]::Create($sliceExit))

# A synthetic workspace shaped like the real one: the counts and lengths above, spread over the
# real package names, under a bench prefix the caller chooses. Relative lengths average 40.
function New-SyntheticWorkspace {
    param([Parameter(Mandatory)] [int] $PrefixLength)
    $prefix = ('P' * $PrefixLength)
    $shape = [ordered]@{
        'graphhelm-cli' = 45; 'graphhelm-tools' = 38; 'graphhelm-events' = 22; 'graphhelm-runtime' = 13
        'graphhelm-tool-host' = 12; 'graphhelm-postgres' = 12; 'graphhelm-graph' = 10; 'graphhelm-policy' = 10
        'graphhelm-schema' = 10; 'graphhelm-governor' = 10; 'graphhelm-simulation' = 9; 'graphhelm-protocols' = 8
        'graphhelm-execution' = 8; 'graphhelm-gateway' = 8; 'graphhelm-extension-host' = 6; 'graphhelm-tool-broker' = 6
        'graphhelm-process-tree' = 4
    }
    $packages = [ordered]@{}
    foreach ($name in $shape.Keys) {
        $files = New-Object System.Collections.Generic.List[string]
        for ($k = 0; $k -lt $shape[$name]; $k++) {
            # 40 characters of relative path each, on average, like the counted tree.
            $files.Add("$prefix/" + $name.Substring(10) + '/tests/' + ('t' * 26) + "$k.rs")
        }
        $packages[$name] = $files.ToArray()
    }
    return $packages
}

function Get-InvocationLength {
    param([Parameter(Mandatory)] [object] $Invocation)
    # The model the planner uses: the joined absolute paths plus the fixed part of the line.
    return ($Invocation.files | ForEach-Object { $_.Length + 1 } | Measure-Object -Sum).Sum + 80
}

Write-Host '[cell] a short bench prefix keeps the single --all invocation (the cheap path survives)'
$short = @(Get-RustfmtPlan -Packages (New-SyntheticWorkspace -PrefixLength 39) -Limit 32767)
Assert-True ($short.Count -eq 1 -and $short[0].mode -eq 'all') "prefix 39: one invocation, mode 'all' (got $($short.Count) x $($short[0].mode))"

Write-Host '[cell] a long bench prefix must not produce a line over the limit -- THE #895 CELL'
$long = @(Get-RustfmtPlan -Packages (New-SyntheticWorkspace -PrefixLength 140) -Limit 32767)
$over = @($long | Where-Object { (Get-InvocationLength -Invocation $_) -gt 32767 })
Assert-True ($over.Count -eq 0) "prefix 140: every invocation fits the limit (over: $($over.Count) of $($long.Count))"
Assert-True ($long.Count -gt 1) "prefix 140: the plan splits (got $($long.Count) invocation(s))"
Assert-True (@($long | Where-Object { $_.mode -eq 'all' }).Count -eq 0) 'prefix 140: no invocation is --all'
$covered = @($long | ForEach-Object { $_.files } | Sort-Object -Unique).Count
$expected = @((New-SyntheticWorkspace -PrefixLength 140).Values | ForEach-Object { $_ }).Count
Assert-True ($covered -eq $expected) "prefix 140: every file is still formatted exactly once ($covered of $expected)"

Write-Host '[cell] a package that does not fit by itself is chunked, and every chunk fits'
$huge = [ordered]@{ 'graphhelm-cli' = @(1..900 | ForEach-Object { ('P' * 140) + "/cli/tests/" + ('t' * 26) + "$_.rs" }) }
$chunked = @(Get-RustfmtPlan -Packages $huge -Limit 32767)
$overChunk = @($chunked | Where-Object { (Get-InvocationLength -Invocation $_) -gt 32767 })
Assert-True ($chunked.Count -gt 1 -and $overChunk.Count -eq 0) "one 900-file package: $($chunked.Count) chunks, $($overChunk.Count) over the limit"
Assert-True (@($chunked | Where-Object { $_.mode -ne 'files' }).Count -eq 0) 'chunks run rustfmt on explicit files (mode files)'

Write-Host '[cell] the harness note names os error 206 and is silent otherwise'
$note206 = Get-RustfmtHarnessNote -Tail @('error: The filename or extension is too long. (os error 206)', 'Usage: cargo fmt ...') -BenchPathLength 140
Assert-True ($null -ne $note206 -and $note206 -match '206' -and $note206 -match '140') "a 206 tail yields a note naming the error and the bench path length (got: $note206)"
$noteRed = Get-RustfmtHarnessNote -Tail @('Diff in D:/x-895/core/events/src/lib.rs at line 12:', '-    foo();') -BenchPathLength 40
Assert-True ($null -eq $noteRed) 'a real formatting diff yields no harness note (that red is a verdict)'

Write-Host '[cell] the stage verdict: any invocation that did not succeed reddens -- a crash is not a maximum (M, #913)'
Assert-True ((Get-RustfmtStageExit -Codes @(0, 0, 0)) -eq 0) 'all invocations succeeded -> 0'
Assert-True ((Get-RustfmtStageExit -Codes @(0, 3, 0)) -ne 0) 'one formatting red (3) -> non-zero'
Assert-True ((Get-RustfmtStageExit -Codes @(0, -1073741819, 0)) -ne 0) 'a crashed invocation (0xC0000005 reads as -1073741819) -> non-zero, where a maximum read 0'
Assert-True ((Get-RustfmtStageExit -Codes @(99)) -ne 0) 'the mute sentinel (99) -> non-zero'
Write-Host '[cell] the stage is armed with the planner, the note and the verdict (the guard sits at the arming site)'
Assert-True ($sliceStage -match 'Get-RustfmtPlan') 'the rustfmt stage body calls Get-RustfmtPlan'
Assert-True ($sliceStage -match 'Get-RustfmtHarnessNote') 'the rustfmt stage body calls Get-RustfmtHarnessNote'
Assert-True ($sliceStage -match 'Get-RustfmtStageExit') 'the rustfmt stage body takes its verdict from Get-RustfmtStageExit (never a -gt maximum)'

if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "gate-rustfmt-path-length: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "gate-rustfmt-path-length: $($script:total) assertions passed" -ForegroundColor Green
exit 0
