# A native command that WROTE TO STDERR did not fail, and this gate read it as one that never ran.
#
# MEASURED, in a real gate on 2026-09-20. `ci/gate-artifact-reuse.tests.ps1` died with
#   HARNESS-BROKE: cannot read the dependency graph: 'cargo +1.97.1 metadata --format-version 1
#   --locked --all-features --manifest-path ...\Cargo.toml' did not succeed (ran=False, exit=0)
# while the cargo it ran had SUCCEEDED. Reproduced by hand, holding ~/.cargo/.package-cache with a
# second cargo and probing with the gate's own arguments:
#   probe: ran=False exit=0
#   [NativeCommandError] Blocking waiting for file lock on package cache
#
# THE MECHANISM IS POWERSHELL'S, NOT CARGO'S. Under `2>&1` in PowerShell 5.1 every stderr line a
# native process writes becomes a NativeCommandError record in the capture and clears `$?` -- even
# when the process exited 0. `$?` was being read as "did this command launch at all", and on its
# own it does not answer that question. Measured, in a fresh process:
#   & cmd /c "echo x 1>&2 & exit 0" 2>&1   ->  ran=False  exit=0
#   & cmd /c "echo x & exit 0"      2>&1   ->  ran=True   exit=0
#
# WHY THIS OUTRANKS ONE RED SUITE. `ci/gate.ps1` calls Get-CargoDependencyGraph for its OWN crate
# input hashes, inside a catch that degrades to
#   [gate] NOTE: the crate input hashes could not be computed, so no artefact can be PROVEN reused
# -- in yellow, with the run still green. So a cargo that merely QUEUED on a lock costs the run
# every reused artefact. On this workspace that is a 12 s build pass against a 566 s one, and the
# only thing that would say so is a NOTE nobody reads. The suite going red was the loud half of a
# defect whose expensive half is silent.
#
# THE PROPERTY UNDER TEST IS DISCRIMINATION, not tolerance. Tolerating stderr is easy to get wrong
# in the permissive direction: deleting the `$?` check outright would pass every cell in section B
# and leave the gate unable to tell a cargo that never launched from one that returned 0. So every
# cell here that ACCEPTS has a control beside it that must still REFUSE. The two causes are told
# apart by what the capture holds -- measured on PowerShell 5.1:
#   stderr + exit 0      -> $? False, capture HOLDS NativeCommandError records, $LASTEXITCODE 0
#   not on PATH          -> $? False, capture holds NO such record (the CommandNotFoundException
#                           goes to the console, not into the `2>&1` capture), and $LASTEXITCODE
#                           keeps whatever the PREVIOUS command left -- a stale zero reads as success
#
# THE SHIMS ARE REAL PROCESSES: .cmd files on a PATH this suite controls. A PowerShell function
# named `cargo` would be found by `& cargo` and would be far easier to write, and it would prove
# nothing -- a function cannot produce a NativeCommandError record and cannot set $LASTEXITCODE,
# which are the two things this suite is about. Cell A2 asserts the shim really does produce the
# record, so a shim that quietly stopped being native cannot leave section B green.

$ExpectedAssertionCount = 29
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
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

# ARRANGEMENT FIRST. A file that stopped parsing, or a function that was renamed, yields no subject
# at all -- and every assertion below would then be about a program that was never read.
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($gatePath, [ref] $null, [ref] $parseErrors)
if ($parseErrors.Count -gt 0) {
    Write-Host "HARNESS-BROKE: gate.ps1 does not parse ($($parseErrors.Count) error(s))" -ForegroundColor Magenta
    exit 2
}

function Get-GateFunctionAst {
    param([Parameter(Mandatory)] [string] $Name)
    $wanted = $Name
    $fn = $ast.Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $wanted
        }.GetNewClosure(), $true)
    if ($null -eq $fn) {
        Write-Host "HARNESS-BROKE: $Name was not found in ci/gate.ps1" -ForegroundColor Magenta
        exit 2
    }
    return $fn
}

# Get-GateToolchainId falls through to Get-ToolchainId when no toolchain is pinned, so the file that
# defines it has to be here for the subject to be loadable at all.
. (Join-Path $PSScriptRoot 'crate-input-hash.ps1')
foreach ($name in @('Test-NativeCallFailed', 'Get-NativeStderrText', 'Get-GateToolchainId', 'Get-CargoDependencyGraph')) {
    . ([scriptblock]::Create(((Get-GateFunctionAst -Name $name).Extent.Text)))
}

$shimRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-native-stderr-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($shimRoot) | Out-Null

# A one-line cargo metadata document, the shape Get-CargoDependencyGraph folds: one workspace member
# with a manifest_path, and a resolve section. ONE LINE, because the function picks the document by
# `StartsWith('{')` exactly as cargo emits it.
$metadataDocument = '{"packages":[{"id":"path+file:///fixture#shim-crate@0.1.0","name":"shim-crate","manifest_path":"C:\\fixture\\shim-crate\\Cargo.toml"}],"workspace_members":["path+file:///fixture#shim-crate@0.1.0"],"resolve":{"nodes":[{"id":"path+file:///fixture#shim-crate@0.1.0","features":[],"deps":[]}]}}'

function New-NativeShim {
    <#
      .SYNOPSIS
        A directory holding one real `<Name>.cmd` that writes what the cell wants and exits how it says.

      .DESCRIPTION
        STDOUT COMES FROM A FILE, via `type`, not from `echo`. The metadata document is a line of
        JSON full of quotes and braces, and `echo` hands all of it to cmd.exe's parser.
    #>
    param(
        [Parameter(Mandatory)] [string] $Name,
        [AllowEmptyString()] [string] $StderrLine = '',
        [AllowEmptyString()] [string] $Stdout = '',
        [int] $ExitCode = 0
    )
    $dir = Join-Path $shimRoot ([guid]::NewGuid().ToString('N'))
    [System.IO.Directory]::CreateDirectory($dir) | Out-Null
    $lines = @('@echo off')
    if (-not [string]::IsNullOrEmpty($StderrLine)) { $lines += "echo $StderrLine 1>&2" }
    if (-not [string]::IsNullOrEmpty($Stdout)) {
        [System.IO.File]::WriteAllText((Join-Path $dir 'stdout.txt'), $Stdout)
        $lines += 'type "%~dp0stdout.txt"'
    }
    $lines += "exit /b $ExitCode"
    [System.IO.File]::WriteAllText((Join-Path $dir "$Name.cmd"), (($lines -join "`r`n") + "`r`n"))
    return $dir
}

function Invoke-WithPath {
    <#
      .SYNOPSIS
        Runs $Body with PATH set to $Directory plus system32, and nothing else.

      .DESCRIPTION
        NOT PREPENDED. A real cargo further down PATH would answer the cells that expect the shim
        and -- worse -- would make the not-on-PATH control silently run the real thing. system32
        stays because cmd.exe is what runs a .cmd.
    #>
    param([Parameter(Mandatory)] [string] $Directory, [Parameter(Mandatory)] [scriptblock] $Body)
    $saved = $env:PATH
    try {
        $env:PATH = "$Directory;$env:SystemRoot\system32"
        & $Body
    } finally {
        $env:PATH = $saved
    }
}

function Get-Failure {
    <#
      .SYNOPSIS
        The message of whatever $Body threw, or $null when it threw nothing.

      .DESCRIPTION
        RETURNS THE MESSAGE rather than a boolean, so a cell can assert WHICH refusal fired. Two
        different throws in one function are one string apart, and a boolean cannot tell them apart.
    #>
    param([Parameter(Mandatory)] [scriptblock] $Body)
    try {
        & $Body | Out-Null
        return $null
    } catch {
        return [string]$_.Exception.Message
    }
}

function Get-GateSlice {
    param([Parameter(Mandatory)] [string] $Start, [Parameter(Mandatory)] [string] $End)
    $i = $gateText.IndexOf($Start, [System.StringComparison]::Ordinal)
    $j = if ($i -ge 0) { $gateText.IndexOf($End, $i + $Start.Length, [System.StringComparison]::Ordinal) } else { -1 }
    if ($i -lt 0 -or $j -le $i) {
        # An anchor that stopped matching must NOT read as a passing test: the slice would be empty
        # and every assertion below would be about nothing.
        throw "HARNESS-BROKE: gate.ps1 slice anchors did not match for [$Start]"
    }
    return $gateText.Substring($i, $j - $i)
}

function Invoke-ProbeSlice {
    <#
      .SYNOPSIS
        Runs the gate's nextest probe in a CHILD PowerShell, and reports its exit code and output.

      .DESCRIPTION
        A CHILD PROCESS, not Invoke-Expression, for two reasons that are both about honesty. The
        block ends in `exit 1` on every refusal, and `exit` inside Invoke-Expression would take this
        suite down with it — the refusals would be untestable. And the defect under test is a
        TERMINATING error at SCRIPT SCOPE under `$ErrorActionPreference = 'Stop'`; only a real
        script scope reproduces it, which is also why the preamble sets 'Stop' exactly as
        ci/gate.ps1 does before reaching this block.

        The marker after the slice is the instrument: a block that dies mid-way does not print it,
        and that is precisely how this defect presents — no refusal, no message, just a gate that
        stopped.
    #>
    param(
        [Parameter(Mandatory)] [string] $ShimDirectory,
        [Parameter(Mandatory)] [string] $Pin
    )
    $script = Join-Path $shimRoot ("probe-" + [guid]::NewGuid().ToString('N') + '.ps1')
    $body = @(
        "`$ErrorActionPreference = 'Stop'",
        "`$pinnedNextest = '$Pin'",
        $probeSlice,
        "Write-Host 'PROBE-REACHED-END'"
    ) -join "`r`n"
    [System.IO.File]::WriteAllText($script, $body)
    $saved = $env:PATH
    $savedPreference = $ErrorActionPreference
    try {
        # THE DEFECT UNDER TEST, ONE LEVEL UP. The first draft of this helper omitted the 'Continue'
        # window and the suite died with `HARNESS-BROKE: RemoteException` -- because the child's
        # NativeCommandError arrives in THIS capture, and this file also runs under 'Stop'. Every
        # cell below is about a child that is supposed to write to stderr, so reading it without
        # this window means the suite can never observe the case it exists for.
        $ErrorActionPreference = 'Continue'
        $env:PATH = "$ShimDirectory;$env:SystemRoot\system32;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
        $out = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script 2>&1
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $savedPreference
        $env:PATH = $saved
    }
    return [pscustomobject]@{
        ExitCode = $code
        Output   = (@($out | ForEach-Object { [string]$_ }) -join "`n")
    }
}

$lockLine = 'Blocking waiting for file lock on package cache'
$rustcOutput = "rustc 1.97.1 (0000000000 2026-01-01)`r`nbinary: rustc`r`nhost: x86_64-pc-windows-msvc`r`n"

# The gate's startup probe for the pinned test runner, from the file itself. Sliced rather than
# retyped: a copy of this block would keep passing after the original changed.
# THE END ANCHOR STARTS AT THE STATEMENT, not inside its string. The first draft ended at
# `[gate] test runner: cargo-nextest`, which lands INSIDE the literal of the Write-Host that follows
# the block -- the slice ended with a dangling `Write-Host "` and every child died on
# `The string is missing the terminator`. The arrangement cell below now parses the slice, because
# the substring checks it used to make are all satisfied by a slice that does not compile.
$probeSlice = Get-GateSlice -Start 'function Read-NextestVersionProbe {' -End 'Write-Host "[gate] test runner:'
$probePin = '0.9.145'

try {
    # ================================================================ A
    Write-Host ''
    Write-Host '=== A. the shim reproduces the measured condition, and can produce a green ===' -ForegroundColor Cyan

    $silentCargo = New-NativeShim -Name 'cargo' -Stdout $metadataDocument -ExitCode 0
    $silentGraph = Invoke-WithPath -Directory $silentCargo -Body {
        Get-CargoDependencyGraph -WorkspaceRoot 'C:\fixture' -ToolchainArgument ''
    }
    Assert-True ($silentGraph.members.Count -eq 1 -and @($silentGraph.packages.Values | ForEach-Object { $_.name }) -contains 'shim-crate') `
        'ARRANGEMENT: a silent shim that exits 0 is read as a graph, so this harness can be green at all'

    # THE SHIM IS NATIVE, asserted rather than assumed. Every cell below is about a record only a
    # real process can produce; a shim that became a function would make section B pass for the
    # wrong reason and take the controls down with it.
    $chattyProbe = New-NativeShim -Name 'probe' -StderrLine $lockLine -Stdout 'ok' -ExitCode 0
    $probeCapture = Invoke-WithPath -Directory $chattyProbe -Body {
        $ErrorActionPreference = 'Continue'
        $out = & probe 2>&1
        [pscustomobject]@{ Ran = $?; Exit = $LASTEXITCODE; Output = $out }
    }
    $nativeRecords = @($probeCapture.Output | Where-Object {
            $_ -is [System.Management.Automation.ErrorRecord] -and $_.FullyQualifiedErrorId -like 'NativeCommand*'
        })
    Assert-True ($nativeRecords.Count -gt 0) `
        'ARRANGEMENT: the shim is a REAL process -- its stderr arrives as NativeCommandError records'
    Assert-True ((@($nativeRecords | ForEach-Object { [string]$_ }) -join ' ').Contains($lockLine)) `
        "ARRANGEMENT: the record carries cargo's own line, '$lockLine'"
    Assert-True ((-not $probeCapture.Ran) -and $probeCapture.Exit -eq 0) `
        'ARRANGEMENT: stderr with exit 0 answers ran=False exit=0 -- the exact pair the gate published'

    # ================================================================ B
    Write-Host ''
    Write-Host '=== B. Get-CargoDependencyGraph survives a cargo that only TALKED ===' -ForegroundColor Cyan

    $chattyCargo = New-NativeShim -Name 'cargo' -StderrLine $lockLine -Stdout $metadataDocument -ExitCode 0
    $chattyFailure = Invoke-WithPath -Directory $chattyCargo -Body {
        Get-Failure { Get-CargoDependencyGraph -WorkspaceRoot 'C:\fixture' -ToolchainArgument '' }
    }
    Assert-True ($null -eq $chattyFailure) `
        "a cargo that printed '$lockLine' and exited 0 is NOT read as a cargo that failed"

    # CAUGHT, not allowed to abort the file. These two cells are RED before the fix, and a cell that
    # reports its failure is worth more than one that takes the other twenty down with it.
    $chattyGraph = Invoke-WithPath -Directory $chattyCargo -Body {
        try { Get-CargoDependencyGraph -WorkspaceRoot 'C:\fixture' -ToolchainArgument '' } catch { $null }
    }
    Assert-True ($null -ne $chattyGraph -and $chattyGraph.members.Count -eq 1) `
        'and the graph it returns is the one cargo printed, not an empty one'
    Assert-True ($null -ne $chattyGraph -and @($chattyGraph.nodes.Keys) -contains 'path+file:///fixture#shim-crate@0.1.0') `
        'the resolve section survives the stderr line that precedes it'

    # ================================================================ C
    Write-Host ''
    Write-Host '=== C. CONTROLS: what must still be refused ===' -ForegroundColor Cyan

    $failingCargo = New-NativeShim -Name 'cargo' -StderrLine 'error: could not resolve' -Stdout $metadataDocument -ExitCode 1
    $failingMessage = Invoke-WithPath -Directory $failingCargo -Body {
        Get-Failure { Get-CargoDependencyGraph -WorkspaceRoot 'C:\fixture' -ToolchainArgument '' }
    }
    Assert-True ($null -ne $failingMessage -and $failingMessage.Contains('did not succeed')) `
        'CONTROL: a cargo that exits 1 is still refused, stderr or no stderr'

    $quietFailingCargo = New-NativeShim -Name 'cargo' -Stdout $metadataDocument -ExitCode 1
    $quietFailingMessage = Invoke-WithPath -Directory $quietFailingCargo -Body {
        Get-Failure { Get-CargoDependencyGraph -WorkspaceRoot 'C:\fixture' -ToolchainArgument '' }
    }
    Assert-True ($null -ne $quietFailingMessage -and $quietFailingMessage.Contains('did not succeed')) `
        'CONTROL: a SILENT cargo that exits 1 is refused -- the exit code is judged on its own'

    # THE CONTROL THAT KILLS THE LAZY FIX. Deleting the `$?` check would pass every cell in B, and
    # this one would then go green on a stale zero left by some earlier command.
    $noCargo = New-NativeShim -Name 'not-cargo' -Stdout 'x' -ExitCode 0
    $absentMessage = Invoke-WithPath -Directory $noCargo -Body {
        # A SUCCESSFUL native call first, so $LASTEXITCODE holds a stale 0 exactly as it would
        # inside a gate that has already run something. Without it this cell could pass on an empty
        # $LASTEXITCODE and say nothing about the case that matters.
        $ErrorActionPreference = 'Continue'
        & not-cargo | Out-Null
        Get-Failure { Get-CargoDependencyGraph -WorkspaceRoot 'C:\fixture' -ToolchainArgument '' }
    }
    Assert-True ($null -ne $absentMessage) `
        'CONTROL: a cargo that is not on PATH is refused even when $LASTEXITCODE holds a stale 0'

    $mumblingCargo = New-NativeShim -Name 'cargo' -StderrLine $lockLine -Stdout 'not json at all' -ExitCode 0
    $mumblingMessage = Invoke-WithPath -Directory $mumblingCargo -Body {
        Get-Failure { Get-CargoDependencyGraph -WorkspaceRoot 'C:\fixture' -ToolchainArgument '' }
    }
    Assert-True ($null -ne $mumblingMessage -and $mumblingMessage.Contains('printed no JSON document')) `
        'CONTROL: exit 0 with no document is refused by the SHAPE check, naming the document'

    # ================================================================ D
    Write-Host ''
    Write-Host '=== D. a refusal must name what it saw ===' -ForegroundColor Cyan
    # The throw that started all this printed `ran=False, exit=0` and dropped the capture, so the
    # one line that explained it -- cargo's own -- reached nobody. An instrument that fails without
    # naming what it saw sends the next reader to the wrong subject.
    Assert-True ($null -ne $failingMessage -and $failingMessage.Contains('could not resolve')) `
        "the refusal quotes the process's stderr, not only its exit code"
    Assert-True ($null -ne $failingMessage -and -not $failingMessage.Contains('workspace_members')) `
        'and it quotes the stderr ALONE -- a metadata document inside a throw is megabytes of noise'

    # ================================================================ E
    Write-Host ''
    Write-Host '=== E. Get-GateToolchainId survives a rustup that only TALKED ===' -ForegroundColor Cyan

    $infoLine = 'info: downloading component'
    $chattyRustup = New-NativeShim -Name 'rustup' -StderrLine $infoLine -Stdout $rustcOutput -ExitCode 0
    $rustupFailure = Invoke-WithPath -Directory $chattyRustup -Body {
        Get-Failure { Get-GateToolchainId -ToolchainArgument '+1.97.1' }
    }
    Assert-True ($null -eq $rustupFailure) `
        'a rustup that wrote an info line and exited 0 is NOT read as a rustup that failed'

    # CAUGHT for the same reason as the cargo graph above: RED before the fix, and reporting.
    $toolchainId = Invoke-WithPath -Directory $chattyRustup -Body {
        try { [string](Get-GateToolchainId -ToolchainArgument '+1.97.1') } catch { '' }
    }
    Assert-True ($toolchainId.StartsWith('rustc 1.97.1', [System.StringComparison]::Ordinal)) `
        'and the id it returns is the version rustc printed'

    # THE REMEDY MUST NOT FAIL INTO THE DEFECT IT REPAIRS. Before the fix a chatty rustup threw, so
    # its stderr could never reach the id. Now that stderr is tolerated, an unfiltered join would
    # fold `info: downloading component` INTO the toolchain id -- and that id keys the artifact
    # ledger, so it would quietly name a compiler generation no other run reproduces.
    # BOTH HALVES IN ONE CELL, because either half alone is satisfied by the empty string: before
    # the fix this function threw and `$toolchainId` was '', which contains no info line and would
    # have reported a green for the absence of an id rather than the absence of contamination.
    Assert-True ($toolchainId.StartsWith('rustc ', [System.StringComparison]::Ordinal) -and -not $toolchainId.Contains($infoLine)) `
        'the stderr line is NOT folded into the id the artifact ledger is keyed by'

    # ================================================================ F
    Write-Host ''
    Write-Host '=== F. CONTROLS for the toolchain id ===' -ForegroundColor Cyan

    $failingRustup = New-NativeShim -Name 'rustup' -StderrLine 'error: toolchain is not installed' -Stdout $rustcOutput -ExitCode 1
    $failingRustupMessage = Invoke-WithPath -Directory $failingRustup -Body {
        Get-Failure { Get-GateToolchainId -ToolchainArgument '+1.97.1' }
    }
    Assert-True ($null -ne $failingRustupMessage -and $failingRustupMessage.Contains('is not installed')) `
        'CONTROL: a rustup that exits 1 is refused, and the refusal quotes its stderr'

    $absentRustupMessage = Invoke-WithPath -Directory $noCargo -Body {
        $ErrorActionPreference = 'Continue'
        & not-cargo | Out-Null
        Get-Failure { Get-GateToolchainId -ToolchainArgument '+1.97.1' }
    }
    Assert-True ($null -ne $absentRustupMessage) `
        'CONTROL: a rustup that is not on PATH is refused even when $LASTEXITCODE holds a stale 0'

    # WITHOUT THIS CELL the stderr filter above could return an EMPTY id and cell E would still be
    # green: an empty string contains no info line either. The shape is the intrinsic property --
    # `rustc -Vv` opens with `rustc `, whatever rustup chose to say around it.
    $mumblingRustup = New-NativeShim -Name 'rustup' -StderrLine $infoLine -Stdout "info: nothing here`r`n" -ExitCode 0
    $mumblingRustupMessage = Invoke-WithPath -Directory $mumblingRustup -Body {
        Get-Failure { Get-GateToolchainId -ToolchainArgument '+1.97.1' }
    }
    # NAMING THE REFUSAL, not merely counting one. This shim also writes stderr, so before the fix
    # it was refused by the `$?` check -- the very thing being removed. Asserting only "something
    # threw" would carry that pass across the fix and say nothing about the shape check replacing it.
    Assert-True ($null -ne $mumblingRustupMessage -and $mumblingRustupMessage.Contains('rustc version')) `
        'CONTROL: exit 0 whose stdout is not a rustc version is refused BY THE SHAPE CHECK, so the id can never be empty'

    $silentRustup = New-NativeShim -Name 'rustup' -Stdout $rustcOutput -ExitCode 0
    $silentId = Invoke-WithPath -Directory $silentRustup -Body {
        try { [string](Get-GateToolchainId -ToolchainArgument '+1.97.1') } catch { '' }
    }
    Assert-True ($silentId.StartsWith('rustc 1.97.1', [System.StringComparison]::Ordinal)) `
        'CONTROL: a silent rustup still answers -- the cell above is a shape check, not a blanket refusal'

    # ================================================================ G
    Write-Host ''
    Write-Host '=== G. every site that reads $? carries the discrimination ===' -ForegroundColor Cyan
    # BY ENUMERATION, not by naming the two functions known today. The defect is a PROPERTY of
    # reading `$?` after a `2>&1` native call, so a third site added next year inherits this cell
    # instead of escaping it.
    #
    # SEARCHED IN THE AST, NOT IN THE TEXT, and the first draft of this cell is why. Matching
    # `NativeCommand` against `.Extent.Text` reported Get-CargoDependencyGraph as already
    # discriminating -- it was not, it merely carried the word in a COMMENT explaining
    # $ErrorActionPreference. Comments are not AST nodes, so prose about a rule cannot stand in for
    # the rule.
    $readers = @($ast.FindAll({
                param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst]
            }, $true) | Where-Object { $_.Extent.Text -match '=\s*\$\?' })
    Assert-True ($readers.Count -ge 2) `
        "ARRANGEMENT: ci/gate.ps1 has at least two functions reading `$? after a native call (found $($readers.Count)), so the cells below have subjects"

    $undelegated = @($readers | Where-Object {
                $calls = @($_.FindAll({
                            param($node) $node -is [System.Management.Automation.Language.CommandAst]
                        }, $true) | Where-Object { [string]$_.GetCommandName() -eq 'Test-NativeCallFailed' })
                $calls.Count -eq 0
            } | ForEach-Object { $_.Name })
    $missingNote = if ($undelegated.Count -gt 0) { " -- missing in: $($undelegated -join ', ')" } else { '' }
    Assert-True ($undelegated.Count -eq 0) `
        "every function reading `$? routes its verdict through Test-NativeCallFailed$missingNote"

    # AND THE THING THEY DELEGATE TO ACTUALLY DISCRIMINATES. Without this cell the one above is
    # satisfied by a helper that returns `-not $Ran` -- both callers would delegate, the structure
    # would look right, and the defect would be one indirection further away than before.
    $helperLiterals = @((Get-GateFunctionAst -Name 'Test-NativeCallFailed').FindAll({
                param($node) $node -is [System.Management.Automation.Language.StringConstantExpressionAst]
            }, $true) | Where-Object { $_.Value -match 'NativeCommand' })
    Assert-True ($helperLiterals.Count -gt 0) `
        'and Test-NativeCallFailed tells them apart by the NativeCommandError record, not by $? alone'
    # ================================================================ H
    Write-Host ''
    Write-Host '=== H. the startup probe must SURVIVE a chatty toolchain ===' -ForegroundColor Cyan
    # THE ONE NATIVE `2>&1` IN THIS FILE THAT RUNS AT SCRIPT SCOPE. Every other one opens an
    # $ErrorActionPreference='Continue' window first; this one inherits the 'Stop' set at the top of
    # ci/gate.ps1. Measured on PS 5.1: under 'Stop', at script scope, a native call whose process
    # writes ANY stderr line raises a TERMINATING NativeCommandError -- the next statement never
    # runs and the process exits 1.
    #
    # WHICH MAKES A SUCCESSFUL PROBE FATAL. `cargo` here is the rustup shim (rust-toolchain.toml
    # pins the channel), and rustup writes `info: syncing channel updates ...` to stderr while the
    # command it proxies still prints the right version and exits 0. The gate then dies before this
    # refusal can print, before the CARGO_TARGET_DIR precondition and before the canary -- with a
    # raw PowerShell error, for a correctly installed and correctly pinned runner.
    #
    # THE TEXT-LEVEL SUITE CANNOT SEE THIS. ci/gate-nextest.tests.ps1 asserts the refusal strings
    # and their order are PRESENT in gate.ps1, and they are present whether or not they are
    # reachable. Reachability needs the block to actually run, which is what these cells do.
    $sliceErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseInput($probeSlice, [ref] $null, [ref] $sliceErrors)
    Assert-True ($sliceErrors.Count -eq 0 -and $probeSlice.Contains('cargo nextest --version') -and
        $probeSlice.Contains('is not installed') -and $probeSlice.Contains('ci/tool-versions.json pins')) `
        "ARRANGEMENT: the slice PARSES ($($sliceErrors.Count) errors) and carries the probe and BOTH refusals ($($probeSlice.Length) chars)"

    $goodVersion = "cargo-nextest-cargo-nextest $probePin`r`n"
    $silentCargo2 = New-NativeShim -Name 'cargo' -Stdout $goodVersion -ExitCode 0
    $silentProbe = Invoke-ProbeSlice -ShimDirectory $silentCargo2 -Pin $probePin
    Assert-True ($silentProbe.ExitCode -eq 0 -and $silentProbe.Output.Contains('PROBE-REACHED-END')) `
        'ARRANGEMENT: a silent, correctly-pinned cargo gets through the probe, so this harness can be green at all'

    # RED BEFORE THE FIX. The runner is installed and correctly pinned; rustup merely spoke.
    $chattyCargo2 = New-NativeShim -Name 'cargo' -StderrLine 'info: syncing channel updates for 1.97.1' -Stdout $goodVersion -ExitCode 0
    $chattyProbeRun = Invoke-ProbeSlice -ShimDirectory $chattyCargo2 -Pin $probePin
    Assert-True ($chattyProbeRun.ExitCode -eq 0 -and $chattyProbeRun.Output.Contains('PROBE-REACHED-END')) `
        'a cargo that wrote to stderr and exited 0 with the PINNED version does not kill the gate at startup'

    $absentProbe = Invoke-ProbeSlice -ShimDirectory $noCargo -Pin $probePin
    Assert-True ($absentProbe.ExitCode -eq 1 -and $absentProbe.Output.Contains('is not installed')) `
        'CONTROL: an ABSENT cargo still reaches the refusal and exits 1, naming the install command'

    $failingCargo2 = New-NativeShim -Name 'cargo' -StderrLine 'error: no such subcommand' -Stdout '' -ExitCode 101
    $failingProbe = Invoke-ProbeSlice -ShimDirectory $failingCargo2 -Pin $probePin
    Assert-True ($failingProbe.ExitCode -eq 1 -and $failingProbe.Output.Contains('is not installed')) `
        'CONTROL: a cargo that exits non-zero is still refused -- tolerating stderr is not tolerating failure'

    # THE CELL THAT KILLS AN OVER-PERMISSIVE FIX. Wrapping the call in a Continue window must not
    # also swallow the PIN comparison: a chatty cargo carrying the WRONG version is still wrong.
    $wrongCargo = New-NativeShim -Name 'cargo' -StderrLine 'info: syncing channel updates for 1.97.1' -Stdout "cargo-nextest-cargo-nextest 0.9.99`r`n" -ExitCode 0
    $wrongProbe = Invoke-ProbeSlice -ShimDirectory $wrongCargo -Pin $probePin
    Assert-True ($wrongProbe.ExitCode -eq 1 -and $wrongProbe.Output.Contains('ci/tool-versions.json pins')) `
        'CONTROL: a CHATTY cargo with the wrong version is still refused by the pin comparison'
} catch {
    Write-Host "HARNESS-BROKE: a cell threw: $($_.Exception.GetType().Name): $($_.Exception.Message) (line $($_.InvocationInfo.ScriptLineNumber): $($_.InvocationInfo.Line.Trim()))" -ForegroundColor Magenta
    Write-Host $_.ScriptStackTrace -ForegroundColor Magenta
    throw
} finally {
    if (Test-Path -LiteralPath $shimRoot) {
        Remove-Item -LiteralPath $shimRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Host ''
    if ($script:total -ne $ExpectedAssertionCount) {
        Write-Host "HARNESS-BROKE: expected $ExpectedAssertionCount assertions, ran $($script:total)" -ForegroundColor Magenta
        exit 2
    }
    if ($script:failures -gt 0) {
        Write-Host "gate-native-stderr: $($script:failures) of $($script:total) assertions FAILED" -ForegroundColor Red
        exit 1
    }
    Write-Host "gate-native-stderr: $($script:total) assertions passed" -ForegroundColor Green
    exit 0
}
