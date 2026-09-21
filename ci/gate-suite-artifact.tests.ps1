# #243: the gate's `cli: <suite>` stages ran a DIFFERENT compiled binary than the one
# fingerprinted into staleArtifacts/instrumentSuspect (workspace-wide, --all-features) --
# cargo's feature unification makes `-p graphhelm-cli --test <suite>` and `--workspace
# --all-features` different compiled units for the identical source. A stale per-suite binary
# therefore reached the freshness check's blind spot: present at the stage, absent from the
# population that judges staleness.
#
# The per-suite loop fingerprints its own stage's scope, in the same iteration, before the stage
# runs, into the SAME artefact list the freshness check already reads. This suite runs in the
# background Cargo-free lane, so it checks that wiring with a deterministic collaborator.

$ExpectedAssertionCount = 22
$ErrorActionPreference = 'Stop'
# Two full gates observed the name-only import return while Get-FileHash was still absent in this
# child, even though the same six-way pool was green in isolation. Load the host's own manifest and
# force its script exports into this runspace; the no-auto-load assertion below keeps this
# fail-closed if that explicit import ever stops providing the command.
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1') -Force -ErrorAction Stop
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$hadModuleAutoLoadingPreference = Test-Path variable:PSModuleAutoLoadingPreference
$savedModuleAutoLoading = if ($hadModuleAutoLoadingPreference) { Get-Variable PSModuleAutoLoadingPreference -ValueOnly } else { $null }
try {
    $PSModuleAutoLoadingPreference = 'None'
    $fileHashProviderIsLoaded = $null -ne (Get-Command Get-FileHash -ErrorAction SilentlyContinue)
} finally {
    $PSModuleAutoLoadingPreference = if ($hadModuleAutoLoadingPreference) { $savedModuleAutoLoading } else { 'All' }
}
Assert-True $fileHashProviderIsLoaded `
    'the file-hash provider is already loaded, so a pooled cold start cannot lose it to module auto-loading'

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

try {
    # THE LINK, BY CONTAINMENT. The cli stage must fingerprint ITS OWN scope, BEFORE it runs -- a
    # call anywhere else in the file, or one scoped to the workspace, names a different binary than
    # the one the stage is about to execute.
    #
    # #1053 replaced 58 serial `cargo test --test <suite>` stages with ONE `nextest run
    # -p graphhelm-cli`, so this link is now expressed PER PACKAGE rather than per suite. The
    # property #243 named is unchanged and so are these cells' claims: the fingerprint covers
    # exactly the unit the stage runs, it is taken first, and it lands in the one list the
    # freshness check reads. What changed is that one call now covers all 58 binaries, because one
    # stage now runs all 58. This suite REFUSED outright when the anchors moved -- 0 assertions,
    # HARNESS-BROKE -- rather than passing over an empty slice, which is why the rewrite is
    # deliberate rather than discovered later.
    # ANCHORED ON THE BLOCK'S FIRST STATEMENT, not on the fingerprint call. Starting at the call
    # left `$cliFingerprintStartedUtc` outside the slice, so the failed-fingerprint branch computed
    # `wallTimeSecs` by subtracting a $null and the suite died with op_Subtraction. Measured.
    $cliBlockStart = $gateText.IndexOf('$cliFingerprintStartedUtc = [DateTime]::UtcNow', [System.StringComparison]::Ordinal)
    # THE TERMINATOR IS INCLUDED, and it is not a detail: this slice is handed to
    # `Invoke-Expression` below, so cutting before the block's closing brace yields unbalanced
    # braces and the suite dies on its own harness instead of on its subject. Measured.
    $cliBlockTerminator = '} | Out-Null'
    $cliBlockEnd = if ($cliBlockStart -ge 0) { $gateText.IndexOf($cliBlockTerminator, $cliBlockStart, [System.StringComparison]::Ordinal) } else { -1 }
    if ($cliBlockStart -lt 0 -or $cliBlockEnd -le $cliBlockStart) {
        throw 'HARNESS-BROKE: the cli fingerprint-and-stage block could not be located in gate.ps1'
    }
    $loop = $gateText.Substring($cliBlockStart, ($cliBlockEnd + $cliBlockTerminator.Length) - $cliBlockStart)

    Assert-True ($loop.Contains("Get-TestArtifactManifest -CargoArgs @('-p', 'graphhelm-cli')")) `
        'the cli stage fingerprints ITS OWN scope (-p graphhelm-cli, no --all-features), not the workspace-wide one that feature unification makes a different compiled unit'

    $fingerprintAt = $loop.IndexOf('Get-TestArtifactManifest -CargoArgs', [System.StringComparison]::Ordinal)
    $stageAt = $loop.IndexOf("Invoke-Stage 'cli suites'", [System.StringComparison]::Ordinal)
    Assert-True ($fingerprintAt -ge 0 -and $stageAt -gt $fingerprintAt) `
        'the fingerprint is taken BEFORE the stage runs, so it names the binaries the stage is about to execute rather than ones it already replaced'

    # AND THE STAGE RUNS THE SCOPE THAT WAS FINGERPRINTED. Without this the two could drift apart
    # -- a fingerprint of `-p graphhelm-cli` beside a stage that ran `--workspace` would satisfy
    # every cell above while measuring a different compiled unit, which is #243 restored.
    Assert-True ($loop.Contains('nextest run -p graphhelm-cli --locked')) `
        'and the stage runs exactly that scope, so the fingerprinted unit and the executed unit cannot drift apart'

    Assert-True ($loop.Contains('$artifactManifest.artifacts.Add(')) `
        'the cli artefacts land in the SAME list staleArtifacts/instrumentSuspect already read, not a second population nothing judges'

    # M's named gap (PR #1007): nothing PINS that Write-RunManifest -- which computes
    # staleArtifacts from $ArtifactManifest.artifacts -- runs AFTER the per-suite append. On line
    # numbers alone the derivation sits ABOVE the loop (it lives inside Write-RunManifest, a
    # function defined early and called late); execution order is the reverse of file order, and
    # nothing here held that. Moving the Write-RunManifest CALL above the loop, or computing
    # staleArtifacts eagerly, would leave every cell above green while the per-suite binaries
    # silently left the population before anything judged them.
    $loopEnd = $gateText.IndexOf("Invoke-Stage 'cli suites'", [System.StringComparison]::Ordinal)
    $writeAt = $gateText.IndexOf('Write-RunManifest -Status $status', [System.StringComparison]::Ordinal)
    Assert-True ($loopEnd -ge 0 -and $writeAt -gt $loopEnd) `
        'the manifest (and the staleArtifacts verdict inside it) is written AFTER the cli appends, or the appends reach nothing that judges them'

    # Exercise the real fingerprint reader against controlled Cargo output, without a build.
    . (Join-Path $PSScriptRoot 'gate-evidence.ps1')
    Invoke-Expression (Get-GateSlice -Start 'function Protect-GateEvidenceLine {' -End '# Runs ci/postgres.ps1')
    # Get-TestArtifactManifest calls Read-ArtifactLedger and Get-CrateInputHashes, and neither is in its
# own slice. Without them the reader threw CommandNotFoundException at the first ledger read and the
# suite stopped after 4 of 12 assertions -- reported only as HARNESS-BROKE, because the `finally`
# below runs `exit 2` and kills the process before the exception can surface.
#
# The missing-hash failure is CAUGHT by production and printed as a NOTE, so it looked like the cause
# and is not: Read-ArtifactLedger is the uncaught one. The visible symptom named the wrong function.
#
# Slicing them in rather than stubbing them, so the cells below exercise the real ledger and hash
# readers. ci/gate.ps1:1820..2381 is function definitions only -- no top-level statement runs on
# Invoke-Expression -- and it covers Get-GateToolchainId, Get-CargoDependencyGraph,
# Get-CrateInputHashesFromGraph, Get-CrateInputHashes and Read-ArtifactLedger in one span.
Invoke-Expression (Get-GateSlice -Start 'function Get-GateToolchainId {' -End 'function Get-TestArtifactManifest {')
Invoke-Expression (Get-GateSlice -Start 'function Get-TestArtifactManifest {' -End 'function Get-TargetBuildState {')
    function cargo {
        'error: fingerprint-fixture failed password=synthetic-secret'
        foreach ($number in 1..50) { "diagnostic context $number" }
        $global:LASTEXITCODE = 17
    }
    # Get-TestArtifactManifest reads four variables from its caller's scope: $runStartUtc and $toolchain,
# which this suite already set, plus $repositoryRoot and $actualTargetDir, which it did not. The
# ledger read is not guarded, so an empty $actualTargetDir threw and took the remaining 8 cells with it.
#
# The target dir is a FRESH per-run temp directory, never a shared one: a ledger read against a
# populated target dir would let another run's artefacts decide this cell's verdict.
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$actualTargetDir = Join-Path ([System.IO.Path]::GetTempPath()) ('gate-suite-artifact-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $actualTargetDir -Force | Out-Null
$runStartUtc = [DateTime]::UtcNow
    $toolchain = '+1.97.1'
    $capturedFingerprint = Get-TestArtifactManifest -CargoArgs @('-p', 'graphhelm-cli', '--test', 'fake_suite')
    $script:fingerprintEvidence = @($capturedFingerprint.outputTail)
    $evidenceText = $script:fingerprintEvidence -join "`n"
    Assert-True ($capturedFingerprint.buildExitCode -eq 17 -and $script:fingerprintEvidence.Count -le 40 -and $evidenceText.Contains('fingerprint-fixture') -and -not $evidenceText.Contains('synthetic-secret')) `
        'the real fingerprint reader returns bounded redacted Cargo diagnostics for its failed attempt'
    function cargo {
        '{"reason":"compiler-message","message":{"level":"error","message":"fingerprint-fixture fallback","rendered":"error[E0001]: fingerprint-fixture JSON cause password=json-secret\n  --> fixture.rs:1:1\n"}}'
        foreach ($number in 1..50) { '{"reason":"build-script-executed"}' }
        'ordinary stderr context password=stderr-secret'
        'error: could not compile fixture due to one previous error'
        $global:LASTEXITCODE = 101
    }
    $jsonFingerprint = Get-TestArtifactManifest -CargoArgs @('-p', 'graphhelm-cli', '--test', 'fake_suite')
    $script:fingerprintEvidence = @($jsonFingerprint.outputTail)
    $evidenceText = $script:fingerprintEvidence -join "`n"
    Assert-True ($jsonFingerprint.buildExitCode -eq 101 -and $script:fingerprintEvidence.Count -le 40 -and $evidenceText.Contains('fingerprint-fixture JSON cause') -and $evidenceText.Contains('ordinary stderr context') -and $evidenceText -notmatch 'json-secret|stderr-secret') `
        'Cargo JSON compiler diagnostics survive later messages with ordinary stderr preserved and both secret values redacted'
    function cargo {
        '{"reason":"compiler-message","message":{"level":"error","message":"fallback cause password=fallback-secret","rendered":null}}'
        $global:LASTEXITCODE = 101
    }
    $fallbackFingerprint = Get-TestArtifactManifest -CargoArgs @('-p', 'graphhelm-cli', '--test', 'fake_suite')
    $fallbackText = $fallbackFingerprint.outputTail -join "`n"
    Assert-True ($fallbackText.Contains('error: fallback cause') -and -not $fallbackText.Contains('fallback-secret')) `
        'a compiler message without rendered text retains its level and message with redaction'
    Remove-Item Function:cargo

    # THE RUNTIME ORDER, not text position (H on #1007). The BYTE-OFFSET check two cells up
    # compares positions in the loop's own text -- correct today because the two statements sit
    # straight-line in the loop body, but it would stay green if the fingerprint call were wrapped
    # in `if ($false) { ... }` and never executed: the text stays before, the execution stops
    # happening. This drives the REAL loop text with stubbed collaborators that record their OWN
    # invocation, so a call that never runs leaves no mark rather than a false ordering.
    function Get-TestArtifactManifest {
        param([string[]] $CargoArgs)
        $script:callLog.Add('fingerprint')
        $artifacts = New-Object System.Collections.Generic.List[object]
        if ($script:stubBuildExitCode -eq 0) { $artifacts.Add([ordered]@{ target = $CargoArgs[-1] }) }
        return [ordered]@{ buildExitCode = $script:stubBuildExitCode; artifacts = $artifacts; outputTail = $script:fingerprintEvidence }
    }
    function Invoke-Stage {
        param([string] $Name, [scriptblock] $Body)
        $script:callLog.Add("stage:$Name")
        return 0
    }

    $script:callLog = New-Object System.Collections.Generic.List[string]
    $script:stubBuildExitCode = 0
    $artifactManifest = [ordered]@{ artifacts = New-Object System.Collections.Generic.List[object] }
    $script:stageRecords = New-Object System.Collections.Generic.List[object]
    $script:failed = @()
    Invoke-Expression $loop
    Assert-True (($script:callLog -join ',') -eq 'fingerprint,stage:cli suites') `
        'the fingerprint genuinely EXECUTES before the stage runs, observed through instrumented collaborators rather than assumed from source position -- a fingerprint disabled by dead code would leave no mark here'
    Assert-True ($script:failed.Count -eq 0) `
        'control: a successful fingerprint build does not fail the gate'

    # Codex P1 on this commit: a fingerprint build that FAILS must not be silently discarded -- the
    # stage below runs its OWN cargo invocation regardless, and if THAT happens to succeed the gate
    # would finish GREEN with an incomplete or empty per-suite population, recreating #243's blind
    # spot precisely when fingerprinting failed.
    $script:callLog.Clear()
    $script:stubBuildExitCode = 1
    $script:failed = @()
    Invoke-Expression $loop
    Assert-True (@($script:failed) -contains 'cli suites (fingerprint)') `
        'a cli fingerprint build failure is recorded as a gate failure, not discarded while the stage below is left to succeed or fail on its own'

    Assert-True (@($script:stageRecords | Where-Object { $_.name -eq 'cli suites (fingerprint)' -and $_.passed -eq $false -and $_.exitCode -eq 1 }).Count -eq 1) `
        'a failed fingerprint has its own durable failed-stage record, with the fingerprint exit code'

    $failedRecord = @($script:stageRecords | Where-Object { $_.name -eq 'cli suites (fingerprint)' })[0]
    Assert-True (($failedRecord.outputTail -join "`n") -ceq $evidenceText -and $evidenceText.Contains('fingerprint-fixture')) `
        'the durable failed stage retains the fingerprint diagnostics even when the following suite succeeds'

    # THE HASH CACHE'S KEY, WHICH NOTHING COVERED (#1053, found by both reviewing lanes).
    # Get-CrateInputHashes is memoised across the run because it was recomputed once per suite. The
    # cache is sound only while the workspace source is fixed for the run -- and it is NOT:
    # `tools/ci-canary` is a workspace member (Cargo.toml:29) and `Write-CanaryNonce` rewrites the
    # TRACKED tools/ci-canary/src/nonce.rs every run by design (#152). The nonce's digest is
    # therefore part of the key. Until this cell, deleting `|$nonceStamp` restored a stale hash in a
    # verdict-bearing path -- a false GREEN -- with the whole suite still reporting 12/12.
    #
    # THE ALTERNATING SEQUENCE IS THE POINT. Three behaviours must be told apart and no simple
    # repetition separates them:
    #
    #   nonces A, B, A, C over four calls
    #     a BLIND cache (nonce not in the key)         -> 1   computes once, never again
    #     a LAST-VALUE compare (invalidate on change)  -> 4   A->B, B->A, A->C all read as changes
    #     a CONTENT key (what ships)                   -> 3   the second A HITS the stored entry
    #
    # A last-value compare passes an A,B,C arm and fails here, and A,B,C is what a first draft writes.
    # THE STUB COUNTS AND THEN THROWS, which is not laziness -- it keeps the code path identical to
    # the one every other cell here already drives. Production CATCHES a hash failure and carries on
    # with a NOTE; a stub returning a full hash object instead sends the reader down the
    # `unresolved*` branch, which needs `Sort-Ordinal` -- a function outside the sliced span -- and
    # dies there. Measured: a returning stub throws
    # "The term 'Sort-Ordinal' is not recognized" before the cell can assert anything.
    #
    # The counter is what this cell is about, and it increments before the throw, so the arm measures
    # the cache and nothing else. Changing the subject to make a fixture convenient is how a cell
    # ends up testing its own scaffolding.
    function Get-CrateInputHashes {
        param($WorkspaceRoot, $ToolchainArgument)
        $script:cacheCalls++
        throw 'cache cell: the hash reader is stubbed; production catches this and prints its NOTE'
    }
    # RE-SLICED, because an EARLIER CELL REPLACED IT. `function Get-TestArtifactManifest` at :135 is
    # this suite's own stub, installed to drive the stage-record cells, and it never touches the hash
    # reader. Without this line the arms below called that stub, counted 0, threw nothing, and read
    # as a cache that computes never -- a green-looking zero produced by measuring the wrong subject.
    # File order is not execution order, and a cell that assumes what is bound when it runs is
    # measuring whatever the last cell left behind.
    Invoke-Expression (Get-GateSlice -Start 'function Get-TestArtifactManifest {' -End 'function Get-TargetBuildState {')
    # CARGO-FREE, AND OFF THE TRACKED TREE. The re-slice above restored the REAL producer after
    # `Remove-Item Function:cargo` at the cell before, so at 01a3edec these arms ran NINE real cargo
    # builds from the BACKGROUND suite lane (a shim first on PATH logged every argv) and wrote the
    # tracked `tools/ci-canary/src/nonce.rs` four times while a concurrent gate was hashing it --
    # the regression 06011ca2 introduced against 7d9f497f ("without background Cargo"), found by
    # lane A on #1007. Two repairs: a cargo stub that answers with no artefacts (the cells measure
    # the HASH CACHE, not cargo), counted so the arm proves the stub was what ran; and the nonce the
    # cache keys on lives in a SCRATCH copy of the repository layout for the duration, so the
    # tracked file is never written -- proven by digest afterwards, not restored.
    $script:cargoStubCalls = 0
    function cargo { $script:cargoStubCalls++; $global:LASTEXITCODE = 0 }
    $trackedNoncePath = Join-Path $repositoryRoot 'tools\ci-canary\src\nonce.rs'
    $savedNonce = [System.IO.File]::ReadAllText($trackedNoncePath)
    $savedNonceHash = (Get-FileHash -LiteralPath $trackedNoncePath -Algorithm SHA256).Hash
    $scratchRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("gate-suite-artifact-nonce-" + [guid]::NewGuid().ToString('N'))
    [void][System.IO.Directory]::CreateDirectory((Join-Path $scratchRoot 'tools\ci-canary\src'))
    $noncePathForCell = Join-Path $scratchRoot 'tools\ci-canary\src\nonce.rs'
    [System.IO.File]::WriteAllText($noncePathForCell, $savedNonce)
    $realRepositoryRoot = $repositoryRoot
    $repositoryRoot = $scratchRoot
    try {
        Remove-Variable -Name crateInputHashCache -Scope Script -ErrorAction SilentlyContinue
        $script:cacheCalls = 0
        foreach ($body in @('A', 'B', 'A', 'C')) {
            [System.IO.File]::WriteAllText($noncePathForCell, ($savedNonce + "`n// cell $body"))
            $null = Get-TestArtifactManifest -CargoArgs @('-p', 'graphhelm-cli', '--test', "cell$body")
        }
        Assert-True ($script:cacheCalls -eq 3) `
            "the hash cache is keyed on the nonce's CONTENT, not on a compare with the previous value: A,B,A,C recomputes 3 times, where a blind cache computes 1 and a last-value compare computes 4 (computed $($script:cacheCalls))"

        Remove-Variable -Name crateInputHashCache -Scope Script -ErrorAction SilentlyContinue
        $script:cacheCalls = 0
        [System.IO.File]::WriteAllText($noncePathForCell, $savedNonce)
        foreach ($n in 1..5) {
            $null = Get-TestArtifactManifest -CargoArgs @('-p', 'graphhelm-cli', '--test', "static$n")
        }
        Assert-True ($script:cacheCalls -eq 1) `
            "and it still SAVES when the nonce does not move: five suites, one computation (computed $($script:cacheCalls)) -- without this arm the cell above is satisfied by a cache that never caches at all"
    } finally {
        $repositoryRoot = $realRepositoryRoot
        Remove-Item Function:cargo -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $scratchRoot) { Remove-Item -LiteralPath $scratchRoot -Recurse -Force -ErrorAction SilentlyContinue }
    }
    Assert-True ($script:cargoStubCalls -eq 9) `
        "the nine producer calls above went through the cargo STUB, not a real cargo (stub calls: $($script:cargoStubCalls)); this suite runs in the background lane and must never build"
    Assert-True ((Get-FileHash -LiteralPath $trackedNoncePath -Algorithm SHA256).Hash -eq $savedNonceHash) `
        'the tracked canary nonce was never written by these cells (digest unchanged), so a concurrent gate hashing it saw one value throughout'

    # THE ENROLMENT, DRIVEN. Lane B on #1007 switched the mechanism off (`-gt 0` -> `-gt 999999`)
    # and every suite stayed green: the five text guards assert what the code SAYS, none that it
    # HAPPENS. This cell runs the real producer end to end with a cargo stub that names a real file
    # whose mtime predates the run (so it reads as a reuse), no ledger row (so it is unproven), and
    # watches: the producer must delete the file, call cargo once more, and the retried row must be
    # `rebuilt`. With the mechanism off, enrolled is 0, cargo is called once, and the row stays
    # `unproven-reuse` -- this cell reddens.
    # The row-building path keys the ledger by ConvertTo-ComparablePath, which lives OUTSIDE the
    # span sliced above (:1451, beside the digest helpers); no earlier cell reached it because no
    # earlier stub emitted a compiler-artifact row. Sliced in, not stubbed: it is the real key.
    Invoke-Expression (Get-GateSlice -Start 'function ConvertTo-ComparablePath {' -End 'function Get-CrateDirectoryDigest {')
    $enrolExe = Join-Path $actualTargetDir 'enrol-cell.exe'
    $script:enrolCargoCalls = 0
    $script:enrolSawFileOnFirstCall = $null
    $script:enrolFileGoneOnSecondCall = $null
    function cargo {
        $script:enrolCargoCalls++
        if ($script:enrolCargoCalls -eq 1) { $script:enrolSawFileOnFirstCall = (Test-Path -LiteralPath $enrolExe) }
        if ($script:enrolCargoCalls -eq 2) {
            $script:enrolFileGoneOnSecondCall = -not (Test-Path -LiteralPath $enrolExe)
            [System.IO.File]::WriteAllText($enrolExe, 'rebuilt inside the window')
        }
        $exeJson = ($enrolExe -replace '\\', '\\')
        '{"reason":"compiler-artifact","package_id":"enrol 0.1.0","target":{"name":"enrol"},"profile":{"test":true},"executable":"' + $exeJson + '"}'
        $global:LASTEXITCODE = 0
    }
    [System.IO.File]::WriteAllText($enrolExe, 'stale binary from before this run')
    [System.IO.File]::SetLastWriteTimeUtc($enrolExe, $runStartUtc.AddHours(-1))
    Remove-Variable -Name crateInputHashCache -Scope Script -ErrorAction SilentlyContinue
    $enrolManifest = Get-TestArtifactManifest -CargoArgs @('-p', 'graphhelm-cli', '--test', 'enrol')
    Remove-Item Function:cargo -ErrorAction SilentlyContinue
    # INDEXED, not dotted, and unwrapped: the producer's return travels the pipeline beside any
    # stray output, and an OrderedDictionary read with `.` under StrictMode is the file's own trap.
    $enrolManifest = @($enrolManifest | Where-Object { $_ -is [System.Collections.IDictionary] })[-1]
    # `@($list)[0]` throws "Argument types do not match" on PowerShell 5.1 when the list's only
    # element is an OrderedDictionary; index the List itself, as the producer's own cells do.
    $enrolArtifacts = $enrolManifest['artifacts']
    $enrolRow = if ($null -ne $enrolArtifacts -and $enrolArtifacts.Count -gt 0) { $enrolArtifacts[0] } else { $null }
    Assert-True ($enrolManifest['artifactsEnrolled'] -eq 1 -and $script:enrolCargoCalls -eq 2 -and $script:enrolSawFileOnFirstCall -eq $true -and $script:enrolFileGoneOnSecondCall -eq $true) `
        "a binary with no ledger row that cargo did not rebuild is ENROLLED: the producer deletes it and enumerates once more (enrolled=$($enrolManifest['artifactsEnrolled']), cargo calls=$($script:enrolCargoCalls), file present on call 1=$($script:enrolSawFileOnFirstCall), gone on call 2=$($script:enrolFileGoneOnSecondCall))"
    Assert-True ($null -ne $enrolRow -and [string]$enrolRow['reuseProof'] -eq 'rebuilt') `
        "and the retried row is proven by the mtime rule inside the run's own window: reuseProof=$(if ($null -ne $enrolRow) { $enrolRow['reuseProof'] } else { '<no row>' })"

    # THE COUNTS FOLLOW THE LIST, DRIVEN (lane B on #1007, S7/S8: deleting the whole counts block
    # left every suite green). The append-and-count block is sliced out of the per-suite loop by
    # its own first and last statements and run against two hand-built manifests: a run manifest
    # with zero counts and a cold build, and a suite manifest carrying one unproven row, one
    # enrolment and a warm build. Delete the block and the run manifest's counts stay 0 -- red.
    $countsBlock = Get-GateSlice -Start 'foreach ($cliArtifact in $cliManifest.artifacts) {' -End "Invoke-Stage 'cli suites' {"
    $artifactManifest = [ordered]@{ artifacts = (New-Object System.Collections.Generic.List[object]); artifactsRebuilt = 0; artifactsProvenReuse = 0; artifactsUnprovenReuse = 0; artifactsContaminated = 0; artifactsEnrolled = 0; buildMode = 'cold' }
    $suiteRows = New-Object System.Collections.Generic.List[object]
    $suiteRows.Add([ordered]@{ executable = 'suite.exe'; reuseProof = 'unproven-reuse' })
    $cliManifest = [ordered]@{ artifacts = $suiteRows; artifactsRebuilt = 0; artifactsProvenReuse = 0; artifactsUnprovenReuse = 1; artifactsContaminated = 0; artifactsEnrolled = 1; buildMode = 'warm' }
    Invoke-Expression $countsBlock
    Assert-True ($artifactManifest['artifacts'].Count -eq 1 -and $artifactManifest['artifactsUnprovenReuse'] -eq 1 -and $artifactManifest['artifactsEnrolled'] -eq 1 -and $artifactManifest['buildMode'] -eq 'warm') `
        "the cli append grows the run manifest's COUNTS with its list: unproven $($artifactManifest['artifactsUnprovenReuse']), enrolled $($artifactManifest['artifactsEnrolled']), buildMode $($artifactManifest['buildMode']) after one warm suite with one unproven row"
    Assert-True ($gateText -cmatch ('artifactsEnrolled\s*=\s*Read-ArtifactManifestField -Manifest \$ArtifactManifest -Name ' + "'artifactsEnrolled'")) `
        'and the run manifest PUBLISHES artifactsEnrolled (a count summed but never written is a claim the body cannot make)'

} catch {
    # An aborted suite must say WHY before the finally exits 2: without this, a cell that threw
    # reported only "ran N, expected M" and the cause was invisible (found adding the enrolment cell).
    Write-Host "HARNESS-BROKE: a cell threw: $($_.Exception.GetType().Name): $($_.Exception.Message) (line $($_.InvocationInfo.ScriptLineNumber): $($_.InvocationInfo.Line.Trim()))" -ForegroundColor Magenta
    Write-Host $_.ScriptStackTrace -ForegroundColor Magenta
    throw
} finally {
    if ($actualTargetDir -and (Test-Path -LiteralPath $actualTargetDir)) {
        Remove-Item -LiteralPath $actualTargetDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-suite-artifact: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-suite-artifact: $($script:total)/$($script:total) assertions passed" -ForegroundColor Green
}
