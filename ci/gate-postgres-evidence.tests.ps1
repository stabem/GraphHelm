# #484: focused proof that output emitted by a detached PostgreSQL child survives the boundary
# between Invoke-Postgres and Invoke-Stage. This parses the real Invoke-Stage function from
# gate.ps1 without executing the full gate. The fixture uses a child PowerShell process because
# PowerShell 5.1 stream behavior at that process boundary is the bug's observer.

$ExpectedAssertionCount = 22
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

function Assert-Equal {
    param($Expected, $Actual, [Parameter(Mandatory)] [string] $Message)
    Assert-True -Condition ($Expected -eq $Actual) -Message "$Message (expected [$Expected], got [$Actual])"
}

# Import only the functions under test. Dot-sourcing gate.ps1 would run the full gate.
#
# gate-evidence.ps1 IS dot-sourced, and safely: it defines Select-GateEvidenceLines and does
# nothing else -- no stage runs, no cargo, no slot. Invoke-Stage calls that function to build
# outputTail (#810), so importing Invoke-Stage's text alone leaves it undefined and every
# assertion in this file dies on a missing command rather than on what it is testing.
. (Join-Path $PSScriptRoot 'gate-evidence.ps1')
$gatePath = Join-Path $PSScriptRoot 'gate.ps1'
$tokens = $null
$parseErrors = $null
$gateAst = [System.Management.Automation.Language.Parser]::ParseFile(
    $gatePath,
    [ref]$tokens,
    [ref]$parseErrors
)
if ($parseErrors.Count -gt 0) {
    throw "gate.ps1 parse failed: $($parseErrors[0].Message)"
}

foreach ($functionName in @('Protect-GateEvidenceLine', 'Invoke-Postgres', 'Invoke-Stage')) {
    $definition = $gateAst.Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq $functionName
        }, $true) | Select-Object -First 1
    if ($definition) {
        Invoke-Expression $definition.Extent.Text
    }
}
if (-not (Get-Command Invoke-Stage -CommandType Function -ErrorAction SilentlyContinue)) {
    throw 'HARNESS-BROKE: Invoke-Stage was not found in ci/gate.ps1'
}

$script:failed = @()
$script:stageRecords = New-Object System.Collections.Generic.List[object]
$fixturePath = Join-Path $PSScriptRoot 'fixtures\postgres-evidence-failure.ps1'
$successFixturePath = Join-Path $PSScriptRoot 'fixtures\postgres-evidence-success.ps1'
$credential = "credential-$([guid]::NewGuid().ToString('N'))"
$previousCredential = $env:GRAPHHELM_EVIDENCE_TEST_CREDENTIAL
$secretFile = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-$([guid]::NewGuid().ToString('N'))\initdb.pwfile"
$previousSecretFile = $env:GRAPHHELM_EVIDENCE_TEST_SECRET_FILE
$env:GRAPHHELM_EVIDENCE_TEST_CREDENTIAL = $credential
$env:GRAPHHELM_EVIDENCE_TEST_SECRET_FILE = $secretFile

try {
    Write-Host "`n=== Detached PostgreSQL failure evidence ==="
    $humanLog = @(& {
            $script:stageResult = Invoke-Stage 'PostgreSQL evidence fixture' {
                Invoke-Postgres -ScriptPath $fixturePath
            }
        } 6>&1 | ForEach-Object { [string]$_ })

    $record = $script:stageRecords[0]
    $tail = @($record.outputTail)
    $tailText = $tail -join "`n"
    $logText = $humanLog -join "`n"
    $manifestJson = [ordered]@{
        status = 'RED'
        stages = [object[]]$script:stageRecords
    } | ConvertTo-Json -Depth 6
    $manifest = $manifestJson | ConvertFrom-Json

    Assert-Equal -Expected 101 -Actual $script:stageResult -Message 'native child exit code remains 101'
    Assert-True -Condition ($script:failed -contains 'PostgreSQL evidence fixture') -Message 'stage remains failed'
    Assert-True -Condition (-not $record.passed) -Message 'stage record remains red'
    Assert-Equal -Expected 101 -Actual $record.exitCode -Message 'stage record preserves exit 101'
    Assert-True -Condition ($tail.Count -gt 0) -Message 'failed stage outputTail is non-empty'
    Assert-True -Condition ($tailText -like '*deterministic_postgres_failure*') -Message 'outputTail carries test name'
    Assert-True -Condition ($tailText -like '*assertion failed*') -Message 'outputTail carries failure class'
    Assert-True -Condition ($logText -like '*deterministic_postgres_failure*') -Message 'human log carries test name'
    Assert-True -Condition ($logText -like '*assertion failed*') -Message 'human log carries failure class'
    Assert-True -Condition (@($humanLog | Where-Object { $_ -like 'noise-*' }).Count -le 80) -Message 'human log bounds each detached child stream'
    Assert-True -Condition ($tailText -notlike "*$credential*") -Message 'outputTail redacts credential'
    Assert-True -Condition ($logText -notlike "*$credential*") -Message 'human log redacts credential'
    Assert-True -Condition ($tailText -notlike "*$secretFile*") -Message 'outputTail redacts temporary secret-file path'
    Assert-True -Condition ($logText -notlike "*$secretFile*") -Message 'human log redacts temporary secret-file path'
    Assert-True -Condition ($tailText.Contains('[REDACTED_SECRET_FILE]')) -Message 'outputTail names secret-file redaction'
    Assert-True -Condition ($tailText.Contains('***')) -Message 'outputTail shows a redaction marker'
    Assert-True -Condition (@($manifest.stages[0].outputTail).Count -gt 0) -Message 'manifest JSON preserves outputTail'

    Write-Host "`n=== Detached PostgreSQL green path ==="
    $script:failed = @()
    $script:stageRecords = New-Object System.Collections.Generic.List[object]
    $greenLog = @(& {
            $script:greenResult = Invoke-Stage 'PostgreSQL green fixture' {
                Invoke-Postgres -ScriptPath $successFixturePath
            }
        } 6>&1 | ForEach-Object { [string]$_ })
    $greenRecord = $script:stageRecords[0]
    Assert-Equal -Expected 0 -Actual $script:greenResult -Message 'green child exit code remains zero'
    Assert-True -Condition $greenRecord.passed -Message 'green PostgreSQL stage remains green'
    Assert-Equal -Expected 0 -Actual $script:failed.Count -Message 'green stage is not added to failed list'
    Assert-True -Condition (($greenLog -join "`n") -like '*deterministic_postgres_success*') -Message 'green child output remains in human log'
    Assert-True -Condition (-not ($greenRecord.Contains('outputTail'))) -Message 'green stage does not add failure outputTail'
} finally {
    if ($null -eq $previousCredential) {
        Remove-Item Env:\GRAPHHELM_EVIDENCE_TEST_CREDENTIAL -ErrorAction SilentlyContinue
    } else {
        $env:GRAPHHELM_EVIDENCE_TEST_CREDENTIAL = $previousCredential
    }
    if ($null -eq $previousSecretFile) {
        Remove-Item Env:\GRAPHHELM_EVIDENCE_TEST_SECRET_FILE -ErrorAction SilentlyContinue
    } else {
        $env:GRAPHHELM_EVIDENCE_TEST_SECRET_FILE = $previousSecretFile
    }
}

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
