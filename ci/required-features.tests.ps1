# #207: isolated tests for ci/required-features.ps1.
#
# The subject is an ABSENCE. A test target whose features are not enabled is never built, so it
# prints nothing and the run stays green -- "0 tests from a target that never built" and "a target
# with nothing to run" are the same silence. Every cell here therefore exists to make one of those
# two states say something the other does not.
#
# The metadata source is INJECTED, so no case depends on this workspace's manifests except the two
# that deliberately do. A suite whose only input is the real tree can only be observed on the day it
# fires, and this one is meant never to fire.
#
# DECLARED ASSERTION COUNT, derived by counting the calls rather than copied from a run:
#   2  a gated target is returned; an ungated one is not
#   1  the kind and features travel with it
#   1  unreadable metadata is null, not empty
#   1  and Test-... reports that as 'unread', never as 'ran'
#   1  an empty population is 'ran' with Checked 0 -- legitimate, and distinguishable from 'unread'
#   1  a transcript naming the binary satisfies the target
#   1  THE SUBSTRING TRAP: development_concurrency does NOT satisfy concurrency
#   1  and read_concurrency does not either
#   1  a Unix transcript satisfies it too
#   1  a missing target is reported by package/target with its features
#   2  REAL TREE: the two known gated targets are derived from the manifests
#   1  REAL TRANSCRIPT: both are found in a gate log's own shape
$ExpectedAssertionCount = 14

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
    Assert-True -Condition ($Expected -eq $Actual) -Message "$Message (expected '$Expected', got '$Actual')"
}

. (Join-Path $PSScriptRoot 'required-features.ps1')

$fixture = @'
{
  "packages": [
    {
      "name": "gated-crate",
      "targets": [
        { "name": "concurrency", "kind": ["test"], "required-features": ["test-support"] },
        { "name": "plain", "kind": ["test"] }
      ]
    }
  ]
}
'@

Write-Host ''
Write-Host '-- the population comes from the manifests, and only the gated targets are in it --' -ForegroundColor Cyan

$targets = Get-RequiredFeatureTargets -MetadataSource { $fixture }
Assert-Equal 1 @($targets).Count 'exactly the gated target is returned'
Assert-Equal 'concurrency' @($targets)[0].Target 'and it is the one the manifest gates'
Assert-Equal 'test|test-support' "$(@($targets)[0].Kind)|$(@($targets)[0].Features)" `
    'the kind and the features travel with it, so a failure can name what was not enabled'

Write-Host ''
Write-Host '-- UNREADABLE is not EMPTY, and neither of them is RAN --' -ForegroundColor Cyan

$unreadable = Get-RequiredFeatureTargets -MetadataSource { 'this is not json' }
Assert-True -Condition ($null -eq $unreadable) `
    'metadata that cannot be parsed is null, never an empty list that reads as "nothing is gated"'

$verdict = Test-RequiredFeatureTargetsRan -Targets $null -Transcript 'anything'
Assert-Equal 'unread' $verdict.Reason `
    'a population that could not be read is reported as unread, so it can never be reported as ran'

$empty = Test-RequiredFeatureTargetsRan -Targets @() -Transcript 'anything'
Assert-Equal 'ran|0|True' "$($empty.Reason)|$($empty.Checked)|$($empty.Ok)" `
    'a workspace that legitimately gates nothing passes, and says it checked zero'

Write-Host ''
Write-Host '-- the anchor is the BINARY, because the NAME is a substring of two other targets --' -ForegroundColor Cyan

# These three shapes are copied from a real gate transcript (#934's log), not invented: this
# workspace really does carry `concurrency`, `development_concurrency` and `read_concurrency`, and
# only the first is gated behind a feature.
$ran = 'Running tests\concurrency.rs (D:\t\debug\deps\concurrency-c85147712552c1af.exe)'
$development = 'Running tests\development_concurrency.rs (D:\t\debug\deps\development_concurrency-672c75a918c8c917.exe)'
$read = 'Running tests\read_concurrency.rs (D:\t\debug\deps\read_concurrency-9b10a993c1c5b668.exe)'

Assert-Equal 'ran' (Test-RequiredFeatureTargetsRan -Targets $targets -Transcript $ran).Reason `
    'the transcript that names the gated binary satisfies it'

Assert-Equal 'missing' (Test-RequiredFeatureTargetsRan -Targets $targets -Transcript $development).Reason `
    'THE TRAP: a transcript naming only development_concurrency does NOT satisfy concurrency'

Assert-Equal 'missing' (Test-RequiredFeatureTargetsRan -Targets $targets -Transcript $read).Reason `
    'and read_concurrency does not satisfy it either'

Assert-Equal 'ran' (Test-RequiredFeatureTargetsRan -Targets $targets -Transcript 'Running tests/concurrency.rs (/t/debug/deps/concurrency-abc123.exe)').Reason `
    'a Linux transcript satisfies it too, so this cannot pass vacuously on the other platform'

$missing = Test-RequiredFeatureTargetsRan -Targets $targets -Transcript 'nothing of interest here'
Assert-True -Condition ($missing.Missing -join ' ').Contains('gated-crate/concurrency (required-features: test-support)') `
    'a missing target is named with its package and the features that were not enabled'

Write-Host ''
Write-Host '-- and the real tree, so the derivation is not measured only against a fixture --' -ForegroundColor Cyan

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$real = Get-RequiredFeatureTargets -WorkspaceRoot $root
Assert-True -Condition ($null -ne $real) `
    'cargo metadata is readable from this checkout, or every assertion below is about a failed read'
$names = @(@($real) | ForEach-Object { $_.Target } | Sort-Object)
Assert-Equal 'concurrency,repository_conformance' ($names -join ',') `
    'the two targets this workspace gates are DERIVED from its manifests, not listed here'

# A real gate prints the binary path on the same line as the target. This is the shape, taken from
# #934's transcript, so the anchor is measured against what a gate actually emits rather than
# against what this file assumes it emits.
$realTranscript = @'
     Running tests\concurrency.rs (D:\runner-targets\hdd\pr934\debug\deps\concurrency-c85147712552c1af.exe)
     Running tests\repository_conformance.rs (D:\runner-targets\hdd\pr934\debug\deps\repository_conformance-0c3020abfa5d2a90.exe)
'@
Assert-Equal 'ran' (Test-RequiredFeatureTargetsRan -Targets $real -Transcript $realTranscript).Reason `
    'and a real gate transcript satisfies both of them'

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "INCOMPLETE: ran $script:total assertions, expected $ExpectedAssertionCount" -ForegroundColor Yellow
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $script:failures of $script:total" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $script:total of $ExpectedAssertionCount" -ForegroundColor Green
exit 0
