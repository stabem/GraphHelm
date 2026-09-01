# #643: the gate hands script paths to `powershell -File`. A path that does not resolve makes the
# stage die at exit 127 -- loud, but only on a FULL gate run, which is the one thing nobody does
# while iterating. This suite reads those paths straight out of ci/gate.ps1 and resolves them, so a
# typo in the wire is caught by a suite that costs seconds instead of by a gate that costs an hour.
#
# It exists because a real one shipped: `'ci\run-ps-suites.ps1'` was written through a layer that
# interprets escapes, `\r` became a literal CARRIAGE RETURN, and the stage pointed at
# `ci<CR>un-ps-suites.ps1`. Five cells had verified the runner and not one had verified the WIRE.
#
# The population is DERIVED from the file, never a hand list: a hand list is born correct and rots
# on the next site somebody adds, which is the same defect one generation later.

$ExpectedAssertionCount = 4
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

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$gateText = [System.IO.File]::ReadAllText($gatePath)

# Two shapes, because the gate uses both and they never share a token:
#   -File (Join-Path $repositoryRoot 'ci/run-ps-suites.ps1')   <- the stage invocation
#   -File ci/gate.ps1                                          <- bare, in the module documentation
# The two shapes are counted SEPARATELY, and that is the whole point of the split. Pooling them
# lets the documentation matches -- which are prose and can never break the gate -- keep the total
# non-zero while the EXECUTABLE match silently stops being observed. A count that pools a load-
# bearing population with a decorative one answers a question nobody asked. (Found by Codex on
# review of the first version, which pooled them.)
$executableSites = New-Object System.Collections.Generic.List[string]
foreach ($match in [regex]::Matches($gateText, "-File\s+\(Join-Path\s+\`$repositoryRoot\s+'([^']+)'\)")) {
    $executableSites.Add($match.Groups[1].Value)
}
$documentationSites = New-Object System.Collections.Generic.List[string]
foreach ($match in [regex]::Matches($gateText, "-File\s+([A-Za-z0-9_./\-]+\.ps1)")) {
    $documentationSites.Add($match.Groups[1].Value)
}
$sites = New-Object System.Collections.Generic.List[string]
$sites.AddRange($executableSites)
$sites.AddRange($documentationSites)

# The vacuity controls, first: every assertion below quantifies over a list, and every one of them
# is trivially true of an empty list. A regex that stops matching would otherwise report the gate
# as clean -- the loudest possible green for the emptiest possible measurement.
Assert-True -Condition ($executableSites.Count -gt 0) `
    -Message "the extraction found at least one EXECUTABLE -File site in ci/gate.ps1 (found $($executableSites.Count))"

Assert-True -Condition ($documentationSites.Count -gt 0) `
    -Message "the extraction found at least one documented -File path in ci/gate.ps1 (found $($documentationSites.Count))"

$controlChars = @($sites | Where-Object { $_ -match '[\x00-\x1f]' })
Assert-True -Condition ($controlChars.Count -eq 0) `
    -Message "no -File path carries a control character (offending: $($controlChars.Count))"

# Test-Path THROWS on a path containing a control character rather than returning $false, and a
# throw here kills the script mid-run: it exits 1 without ever reaching the assertion-count guard,
# so "the harness broke" arrives dressed as "an assertion failed". Measured on the real defect
# bytes. A path the filesystem refuses to even evaluate is a path that does not resolve.
$missing = @($sites | Where-Object {
        try { -not (Test-Path -LiteralPath (Join-Path $repositoryRoot $_)) } catch { $true }
    })
Assert-True -Condition ($missing.Count -eq 0) `
    -Message "every -File path in ci/gate.ps1 resolves to a file (missing: $($missing -join ', '))"

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}

$passed = $script:total - $script:failures
$color = if ($script:failures -eq 0) { 'Green' } else { 'Red' }
Write-Host "$passed/$script:total passed" -ForegroundColor $color
if ($script:failures -gt 0) { exit 1 }
exit 0
