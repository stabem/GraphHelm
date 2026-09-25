<#
.SYNOPSIS
    Derive the gate's scope from what the change can REACH (#903, epic #901, deliverable 2).

.DESCRIPTION
    Maps every changed path to its crate, expands to all transitive dependents over BOTH
    `dependencies` and `dev-dependencies`, and prints the selection as JSON for the gate to act on
    and record. It runs nothing and decides nothing about colour: it answers "which
    crates can this change reach", and the caller runs them.

    IT FAILS CLOSED, AND THAT IS THE WHOLE DESIGN. The escalation list below is a deny-list over a
    CLASS -- "a path whose change can invalidate the dependency graph this selection is derived
    FROM" -- and a list of names cannot see the next member of its class. So every state this
    script cannot map widens the run instead of narrowing it: a path that matches no crate, a
    metadata document that does not parse, a manifest it cannot read. A selector that guesses
    narrow is a selector that silently stops running the stage that would have gone red, and the
    failure is invisible because the remaining stages pass.

    The compiled graph sees COMPILE-TIME coupling only. Semantic drift between `core/protocols` and
    `core/schema` does not appear in it at all -- the board already carries that lesson as "compile
    radius is not semantic radius" -- so those paths are escalation rules rather than graph nodes.

.PARAMETER ChangedFiles
    ';'-separated repo-relative paths. Omitted, they come from `git diff --name-only`.

.PARAMETER MetadataPath
    A `cargo metadata --format-version 1` document. Omitted, cargo is asked directly. Present, no
    cargo runs -- which is what lets the cells build a graph containing the dev-dependency edge and
    the unmapped path that this workspace does not happen to have today.
#>
param(
    [string] $ChangedFiles,
    [string] $MetadataPath,
    [string] $RepoRoot = '.',
    [string] $MergeBase,
    [string] $Head = 'HEAD'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Each rule is a NAME and a predicate, so the selection can report WHICH one fired. A boolean would
# be enough to widen the run and useless to a reader: a selector that escalates for the wrong reason
# keeps escalating, keeps passing, and stops the day that reason moves -- with nothing to notice it.
$EscalationRules = @(
    @{ Name = 'ci/'; Test = { param($p) $p -like 'ci/*' } }
    @{ Name = 'schemas/'; Test = { param($p) $p -like 'schemas/*' } }
    @{ Name = 'core/protocols/'; Test = { param($p) $p -like 'core/protocols/*' } }
    @{ Name = 'Cargo.toml'; Test = { param($p) $p -eq 'Cargo.toml' -or $p -like '*/Cargo.toml' } }
    @{ Name = 'Cargo.lock'; Test = { param($p) $p -eq 'Cargo.lock' -or $p -like '*/Cargo.lock' } }
    @{ Name = 'build.rs'; Test = { param($p) $p -eq 'build.rs' -or $p -like '*/build.rs' } }
    @{ Name = 'rust-toolchain'; Test = { param($p) $p -like 'rust-toolchain*' -or $p -like '*/rust-toolchain*' } }
)

function ConvertTo-RepoPath {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)
    # `TrimStart('./')` takes a CHAR ARRAY, not a prefix: it strips every leading '.' AND '/',
    # so `.factory/gate-runs/x.json` came back as `factory/...` -- a path matching no crate and
    # no escalation rule, which then escalated every run as 'unmapped'. Measured on the real
    # repository; no fixture had a dotfile, so no cell could have caught it.
    $normalised = $Path -replace '\\', '/'
    while ($normalised.StartsWith('./')) { $normalised = $normalised.Substring(2) }
    return $normalised
}

function Write-Selection {
    param(
        [Parameter(Mandatory)] [bool] $Escalated,
        [AllowNull()] [string] $Rule,
        [string[]] $Crates = @(),
        [string[]] $Changed = @(),
        [string[]] $Unmapped = @(),
        [bool] $Matrix = $true,
        [string] $MatrixReason = '',
        # TRUE unless a caller says otherwise: an omission must read as "Rust input may have
        # changed", which is the answer that widens.
        [bool] $RustInputsChanged = $true,
        # #901 slice 2: TRUE unless a caller says otherwise, for the same reason as above -- an
        # omission must read as "the suites may be reached", which is the answer that runs them.
        [bool] $PsSuites = $true,
        [string] $PsSuitesReason = 'ran: this is a FULL run'
    )
    # `crates` is EMPTY on an escalation on purpose: a FULL run has no selection, and printing the
    # partial one the script had computed invites a caller to use it.
    $document = [ordered]@{
        escalated      = $Escalated
        escalationRule = $Rule
        crates         = @($Crates | Sort-Object -Unique)
        changedFiles   = @($Changed)
        unmapped       = @($Unmapped)
        matrix         = $Matrix
        matrixReason   = $MatrixReason
        rustInputsChanged = $RustInputsChanged
        psSuites       = $PsSuites
        psSuitesReason = $PsSuitesReason
    }
    Write-Output ($document | ConvertTo-Json -Depth 6 -Compress)
}

# ---- the changed set -----------------------------------------------------------------------
if ($PSBoundParameters.ContainsKey('ChangedFiles') -and -not [string]::IsNullOrWhiteSpace($ChangedFiles)) {
    $changed = @($ChangedFiles -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { ConvertTo-RepoPath -Path $_.Trim() })
} else {
    if ([string]::IsNullOrWhiteSpace($MergeBase)) {
        Write-Selection -Escalated $true -Rule 'no-merge-base' -MatrixReason 'FULL run: no merge base to diff against'
        exit 0
    }
    # --no-renames: a rename prints BOTH sides, so `git mv src/x.rs NOTES.md` still shows the Rust
    # path it removed (review of #1216 by lane b9deb2, measured on the same diff call there).
    $diff = @(& git -C $RepoRoot diff --no-renames --name-only $MergeBase $Head 2>&1)
    if ($LASTEXITCODE -ne 0) {
        # The diff failing is not "nothing changed". An empty changed set would select nothing and
        # run nothing, which is the widest possible failure wearing the narrowest possible output.
        Write-Selection -Escalated $true -Rule 'diff-failed' -MatrixReason 'FULL run: git diff failed, so the changed set is unknown'
        exit 0
    }
    $changed = @($diff | ForEach-Object { ConvertTo-RepoPath -Path ([string]$_) } | Where-Object { $_ })
}

if ($changed.Count -eq 0) {
    Write-Selection -Escalated $true -Rule 'empty-diff' -Changed $changed `
        -MatrixReason 'FULL run: the changed set is empty, which is a question about the diff and not an answer about scope'
    exit 0
}

# ---- KNOWN AND NOT BUILD INPUT --------------------------------------------------------------
#
# A THIRD state, and it exists because the first two collapse two different claims into one output.
# `unmapped-path` means "this path is in no class I know" and `crates: []` means "the selection is
# empty"; both widen, correctly, because an unknown must never narrow. But a `ci/*.tests.ps1` file
# is neither unknown nor build input: it is a PowerShell suite FOR the gate, discovered by
# `ci/run-ps-suites.ps1` and run in a stage that is unconditional. It cannot change what a Rust
# stage measures.
#
# MEASURED BEFORE IT WAS DESIGNED (X, 2026-09-06): the selector was run against the real diff of
# every pull request the fleet handled that day. One of eight skipped the PostgreSQL matrices; five
# of the other seven were exactly this class, escalating under the `ci/` deny-list. The naive
# repair -- exempting suites from that rule -- was measured by K and buys nothing: the path then
# reaches the unmapped rule and escalates for a different reason. So the class has to be NAMED, and
# the emptiness it produces has to be distinguishable from the emptiness nobody can explain.
#
# THE MEMBERSHIP IS DELIBERATELY NARROW. `ci/gate.ps1`, `ci/select-scope.ps1` and every file the gate
# runs stay on the escalation list: a change to the gate changes what every other stage measures,
# which is a different claim from "a suite for the gate changed". Adding a member here is loosening
# an escalation, so it is an edit a reviewer sees and a cell has to survive. (`ci/merge-proof.ps1`
# used to be named here too. It was a verifier the PRESSER ran after the gate, not a stage the gate
# runs, so it could not change what a Rust stage measures; it was retired on 2026-09-24.)
#
# #901 slice 2 ADDS TWO MEMBERS, and neither is a hand-written list.
#
#   ci-tool  a top-level `ci/` file the gate never runs. "Runs" is DERIVED from the gate's own code:
#            every `Join-Path $PSScriptRoot|$repositoryRoot '<file>'` on a non-comment line of
#            `ci/gate.ps1`, followed transitively through the files it names (plus this selector,
#            which decides the gate's scope). Measured 2026-09-23: that closure is 15 files, and it
#            leaves out `gate-runner.ps1`, `merge-proof.ps1`, `classify-run.ps1`,
#            `closing-keywords.ps1`, `gate-queue.ps1` -- the verifiers and the runner, which carried
#            most of the fleet's `ci/` churn and cannot change what a Rust stage measures (the
#            runner, the queue and `merge-proof.ps1` were retired on 2026-09-24). Their
#            suites still run: every `ci/` path reaches the suites stage below. A closure that could
#            not be derived claims every `ci/` file, so the failure widens.
#   studio   `apps/studio/*`, a Node project with its own stage (`Test-StudioScopeChanged` in the
#            gate); no crate compiles a byte of it. It escalated as `unmapped-path` and paid the
#            whole Rust gate plus both PostgreSQL matrices for a TypeScript change.
function Get-GatePathFiles {
    param([Parameter(Mandatory)] [string] $Root)
    $closure = New-Object System.Collections.Generic.HashSet[string]([System.StringComparer]::OrdinalIgnoreCase)
    [void]$closure.Add('gate.ps1')
    [void]$closure.Add('select-scope.ps1')
    $pending = New-Object System.Collections.Generic.Queue[string]
    $pending.Enqueue('gate.ps1')
    while ($pending.Count -gt 0) {
        $name = $pending.Dequeue()
        $file = Join-Path (Join-Path $Root 'ci') $name
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
            if ($name -eq 'gate.ps1') { return $null }
            continue
        }
        $text = [System.IO.File]::ReadAllText($file)
        $text = [regex]::Replace($text, '(?s)<#.*?#>', '')
        foreach ($line in ($text -split "`r?`n")) {
            if ($line.TrimStart().StartsWith('#')) { continue }
            # Either quote, and a name that cannot begin with '.', so `'..'` is never captured as a
            # member (review of #1220 by lane 3f90d6).
            foreach ($match in [regex]::Matches($line, "Join-Path\s+\`$(?:PSScriptRoot|repositoryRoot)\s+['`"](?:ci/)?([A-Za-z0-9_-][A-Za-z0-9._-]*)['`"]")) {
                $named = $match.Groups[1].Value
                if ($closure.Add($named)) { $pending.Enqueue($named) }
            }
        }
    }
    return ,$closure
}

$GatePathFiles = $null
try { $GatePathFiles = Get-GatePathFiles -Root $RepoRoot } catch { $GatePathFiles = $null }

$KnownNonBuildInput = @(
    @{ Name = 'ci-suite'; Test = { param($p) $p -like 'ci/*.tests.ps1' } }
    @{ Name = 'ci-tool'; Test = {
            param($p)
            if ($null -eq $GatePathFiles) { return $false }
            if ($p -notlike 'ci/*' -or $p.Substring(3).Contains('/')) { return $false }
            return -not $GatePathFiles.Contains($p.Substring(3))
        }
    }
    @{ Name = 'studio'; Test = { param($p) $p -like 'apps/studio/*' } }
)

function Get-KnownNonBuildClass {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)
    foreach ($class in $KnownNonBuildInput) { if (& $class.Test $Path) { return $class.Name } }
    return $null
}

function Test-KnownNonBuildInput {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)
    return ($null -ne (Get-KnownNonBuildClass -Path $Path))
}

# #901 slice 2: MARKDOWN IS BUILD INPUT ONLY THROUGH ITS READERS. `include_str!("../../../README.md")`
# makes `README.md` an input of the crate holding that line, and of nothing else. So a changed `.md`
# is replaced by the files that NAME its basename (case-insensitive, fixed string, over the tree this
# selector reads -- `ci/` excluded because every `ci/` reader is the suites stage's business, run
# manifests excluded because they quote paths without reading them, other Markdown excluded because
# prose linking prose builds nothing, `.gitattributes` because it names a file's line endings without consuming it). Those readers then meet every rule below exactly as changed
# files would: a reader in a crate selects that crate, a reader under `schemas/` escalates, a reader
# nobody owns escalates as unmapped. No reader at all means no build input: class `markdown`.
#
# The same "a mention counts as a read" rule `ci/docs-only.ps1` (retired 2026-09-24) used (#901 slice 1), and wide for the
# same reason. `extensions/` stays out: its package digests bind the raw bytes of its Markdown.
# FAIL WIDE: git grep answering anything but 0 or 1 leaves the `.md` as build input, where it
# reaches the unmapped rule and escalates.
# #901 slice 2, BLOCK on #1220 (lane 3f90d6, driven): A NON-BUILD CLASS IS NOT "NO RUST READS IT".
# `core/architect/tests/red_banner_producer.rs` walks every file under `ci/` with
# `root.join("ci")` and fails on a new banner producer -- so a `ci/` suite or tool change CAN turn a
# Rust test red, and the class skipped that test. Before this slice the class still compiled and ran
# the whole workspace (no `-p`), so the exposure was new here. So every class path is asked the
# question the Markdown rule asks: which Rust files NAME it? A Rust file names a path when it holds
# the path, its basename, or any ancestor directory written as a string (`"ci"`, `"ci/`,
# `"apps/studio"`), which is how a walker names what it walks. Readers are added to the build
# input and meet every rule below: a reader in a crate selects that crate, a reader nobody owns
# escalates. FAIL WIDE: git grep answering anything but 0 or 1 keeps the path itself as build input.
function Get-RustReaders {
    param([Parameter(Mandatory)] [string] $Path, [Parameter(Mandatory)] [string] $Root)
    $patterns = New-Object System.Collections.Generic.List[string]
    $patterns.Add($Path)
    $segments = @($Path -split '/')
    $patterns.Add($segments[-1])
    # A directory is named EXACTLY (`"ci"`, `"apps/studio"`), and a two-segment-or-deeper one also as
    # a prefix (`"apps/studio/`). A top-level prefix is not used: `"apps/` is the start of every
    # `"apps/cli/..."` literal in the workspace, which names cli files, not Studio ones (measured:
    # it made a core/protocols test that lists cli paths read as a Studio reader).
    for ($depth = 1; $depth -lt $segments.Count; $depth++) {
        $dir = $segments[0..($depth - 1)] -join '/'
        $patterns.Add('"' + $dir + '"')
        if ($depth -ge 2) { $patterns.Add('"' + $dir + '/') }
    }
    # THE PATTERNS GO THROUGH A FILE (`-f`), NEVER THE COMMAND LINE. Windows PowerShell 5.1 does not
    # escape a `"` inside a native argument: `"ci"` reached git as `ci`, and the unbalanced `"ci/`
    # swallowed the `-- *.rs` after it, so git searched EVERY file and a `.ps1` read as a Rust reader.
    # Measured in this suite before the file was used.
    $patternFile = [System.IO.Path]::GetTempFileName()
    [System.IO.File]::WriteAllText($patternFile, (($patterns | Sort-Object -Unique) -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $code = -1
    $found = @()
    try {
        $found = @(& git -C $Root grep -l -F -f $patternFile -- '*.rs' 2>$null)
        $code = $LASTEXITCODE
    } catch {
        return $null
    } finally {
        $ErrorActionPreference = $previous
        Remove-Item -LiteralPath $patternFile -Force -ErrorAction SilentlyContinue
    }
    if ($code -eq 1) { return ,@() }
    if ($code -ne 0) { return $null }
    return ,@($found | ForEach-Object { ConvertTo-RepoPath -Path ([string]$_) } | Where-Object { $_ })
}

function Get-MarkdownReaders {
    param([Parameter(Mandatory)] [string] $Path, [Parameter(Mandatory)] [string] $Root)
    if ($Path -notmatch '(?i)\.md$' -or $Path -like 'extensions/*' -or $Path -like 'ci/*') { return $null }
    # TEST DATA IS WALKED, NOT NAMED (review of #1216 by lane 5bdc38, the same class here):
    # `adapters/tool-host/tests/context_quality.rs` hash-pins every file under
    # `tests/fixtures/context-quality/tree/` without naming one. Such a path stays build input, where
    # it maps to the crate whose directory holds it.
    $segments = @($Path -split '/')
    if (@($segments | Select-Object -SkipLast 1 | Where-Object { @('tests', 'test', 'fixtures', 'fixture', 'testdata') -contains $_.ToLowerInvariant() }).Count -gt 0) { return $null }
    $leaf = $segments[-1]
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $code = -1
    $found = @()
    try {
        $found = @(& git -C $Root grep -l -i -F -e $leaf -- . ':(exclude)*.md' ':(exclude)*.MD' ':(exclude)ci/**' ':(exclude).factory/gate-runs/**' ':(exclude).gitattributes' 2>$null)
        $code = $LASTEXITCODE
    } catch {
        return $null
    } finally {
        $ErrorActionPreference = $previous
    }
    if ($code -eq 1) { return ,@() }
    if ($code -ne 0) { return $null }
    return ,@($found | ForEach-Object { ConvertTo-RepoPath -Path ([string]$_) } | Where-Object { $_ })
}

# The escalation rules run FIRST and this class is subtracted from what they see, so a suite file
# does not trip `ci/`. Everything else in the diff still reaches every rule below unchanged.
# Run manifests are subtracted here TOO, but only so they cannot make a suite-only diff look mixed.
# They keep their own handling further down (`manifest-only` escalates to FULL), and this block
# refuses to fire on them alone: a diff of nothing but receipts is a question about the diff, and
# the answer to that question is already written below.
# ---- the PowerShell suites (#901 slice 2) -----------------------------------------------------
#
# MEASURED BEFORE IT WAS DESIGNED (2026-09-23): the suites stage is 13-14 minutes of every gate, and
# it ran for every pull request whatever the diff. Its population is `ci/*.tests.ps1`, and those
# suites build their own fixtures: of 53 suites, the ones that touch the real tree read `ci/`
# itself, `Cargo.toml` and `schemas/` -- every one of which escalates above, so an escalated run
# never reaches this block and keeps the suites. What is left here is a narrow run over crates.
#
# THE REACH RULE IS TEXTUAL AND WIDE ON PURPOSE -- the rule `ci/docs-only.ps1` (retired 2026-09-24)
# used: a MENTION
# counts as a read. A changed path reaches the suites when the text under `ci/` names the path
# itself, any ancestor directory of it two segments deep or more (`core/events`), or -- for a file
# that is not Rust source -- its basename. A Rust source basename is excluded because `lib.rs` and
# `main.rs` are fixture text in a dozen suites and would reach everything, which is a rule that
# never narrows; a suite that reads a real `.rs` file names its path, and the path candidate
# catches it. A fixture that merely LOOKS like a real path (`core/events/src/local.rs` in a
# panic line) makes the rule run the suites -- the wrong way to be wrong costs minutes, never a hole.
#
# FAIL WIDE: an unreadable or empty `ci/` is not "nothing reads this", it is an instrument that did
# not look.
function Get-PsSuiteReach {
    param(
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Paths,
        [Parameter(Mandatory)] [string] $Root
    )
    $ciDirectory = Join-Path $Root 'ci'
    $corpus = New-Object System.Text.StringBuilder
    try {
        $files = @(Get-ChildItem -LiteralPath $ciDirectory -File -Recurse -ErrorAction Stop)
        foreach ($file in $files) { [void]$corpus.Append([System.IO.File]::ReadAllText($file.FullName)).Append("`n") }
    } catch {
        return @{ run = $true; reason = "ran: ci/ could not be read ($($_.Exception.Message)), so no reach can be ruled out" }
    }
    if ($files.Count -eq 0 -or $corpus.Length -eq 0) {
        return @{ run = $true; reason = 'ran: ci/ holds no text, so no reach can be ruled out' }
    }
    $text = $corpus.ToString().Replace([string][char]92, '/')
    foreach ($path in $Paths) {
        $candidates = New-Object System.Collections.Generic.List[string]
        $candidates.Add($path)
        $segments = @($path -split '/')
        for ($depth = $segments.Count - 1; $depth -ge 2; $depth--) {
            $candidates.Add(($segments[0..($depth - 1)] -join '/'))
        }
        $leaf = $segments[$segments.Count - 1]
        if (-not $leaf.EndsWith('.rs', [System.StringComparison]::OrdinalIgnoreCase)) { $candidates.Add($leaf) }
        foreach ($candidate in $candidates) {
            if ($text.IndexOf($candidate, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
                return @{ run = $true; reason = "ran: ci/ names '$candidate', which $path reaches" }
            }
        }
    }
    return @{ run = $false; reason = "skipped: no text under ci/ names a changed path, an ancestor directory of one, or a non-Rust basename ($($Paths.Count) path(s))" }
}

$RunManifestPrefix = '.factory/gate-runs/'
$nonManifest = @($changed | Where-Object { -not $_.StartsWith($RunManifestPrefix, [System.StringComparison]::Ordinal) })
$suitePaths = New-Object System.Collections.Generic.List[string]
$buildInputList = New-Object System.Collections.Generic.List[string]
# READERS ARE NOT CHANGES. They select their crate; they do not trip an escalation rule written for
# a CHANGE to that path (a test under `core/protocols/` that walks `ci/` is not a protocol change).
$readerPaths = New-Object System.Collections.Generic.HashSet[string]
$markdownReadBy = [ordered]@{}
foreach ($path in $nonManifest) {
    $class = Get-KnownNonBuildClass -Path $path
    if ($null -ne $class) {
        $rustReaders = Get-RustReaders -Path $path -Root $RepoRoot
        if ($null -eq $rustReaders) { $buildInputList.Add($path); continue }
        if (@($rustReaders).Count -gt 0) {
            $markdownReadBy[$path] = @($rustReaders)
            foreach ($reader in @($rustReaders)) { if (-not $buildInputList.Contains($reader)) { $buildInputList.Add($reader); [void]$readerPaths.Add($reader) } }
        }
        $suitePaths.Add("$path ($class)")
        continue
    }
    $readers = Get-MarkdownReaders -Path $path -Root $RepoRoot
    if ($null -eq $readers) { $buildInputList.Add($path); continue }
    if (@($readers).Count -eq 0) { $suitePaths.Add("$path (markdown)"); continue }
    $markdownReadBy[$path] = @($readers)
    foreach ($reader in @($readers)) { if (-not $buildInputList.Contains($reader)) { $buildInputList.Add($reader); [void]$readerPaths.Add($reader) } }
}
# A path the diff CHANGED is never "only a reader", whichever order the loop met it in.
foreach ($changedPath in $nonManifest) { [void]$readerPaths.Remove($changedPath) }
$buildInput = @($buildInputList)

# #901 slice 2: which changed paths the suites stage has to see. Every `ci/` path does -- the suites
# ARE `ci/` -- and every other path is asked of the text under `ci/` (see `Get-PsSuiteReach` above).
function Get-SuiteReachForChange {
    param([AllowEmptyCollection()] [string[]] $Paths)
    $ciPaths = @($Paths | Where-Object { $_ -like 'ci/*' })
    if ($ciPaths.Count -gt 0) { return @{ run = $true; reason = "ran: ci/ changed ($($ciPaths[0]))" } }
    return Get-PsSuiteReach -Paths $Paths -Root $RepoRoot
}

if ($buildInput.Count -eq 0 -and $suitePaths.Count -gt 0) {
    # KNOWN empty, and the reason says which class made it empty. `rustInputsChanged` is the field
    # the gate reads to tell this apart from an empty list it could not explain -- see the guard in
    # `Read-ScopeSelection`, which still answers FULL when the field is absent or true.
    $reach = Get-SuiteReachForChange -Paths $nonManifest
    Write-Selection -Escalated $false -Rule '' -Changed $changed -Crates @() `
        -Matrix $false `
        -MatrixReason "skipped: no Rust build input changed -- every changed path is a known non-build class: $($suitePaths -join ', ')" `
        -RustInputsChanged $false `
        -PsSuites ([bool]$reach.run) -PsSuitesReason ([string]$reach.reason)
    exit 0
}

# ---- escalation, BEFORE any selection ------------------------------------------------------
foreach ($rule in $EscalationRules) {
    foreach ($path in @($buildInput | Where-Object { -not $readerPaths.Contains($_) })) {
        if (& $rule.Test $path) {
            Write-Selection -Escalated $true -Rule $rule.Name -Changed $changed `
                -MatrixReason "FULL run: $($rule.Name) changed ($path)"
            exit 0
        }
    }
}

# ---- the graph -----------------------------------------------------------------------------
try {
    if ($PSBoundParameters.ContainsKey('MetadataPath') -and -not [string]::IsNullOrWhiteSpace($MetadataPath)) {
        $metadata = [System.IO.File]::ReadAllText($MetadataPath) | ConvertFrom-Json
    } else {
        $raw = & cargo metadata --format-version 1 --no-deps --manifest-path (Join-Path $RepoRoot 'Cargo.toml') 2>&1
        if ($LASTEXITCODE -ne 0) { throw "cargo metadata exited $LASTEXITCODE" }
        $metadata = ($raw -join "`n") | ConvertFrom-Json
    }
    if ($null -eq $metadata -or -not $metadata.packages) { throw 'metadata carried no packages' }
} catch {
    # FAIL CLOSED. A graph that could not be read cannot narrow anything, and the reason travels
    # with the escalation so nobody has to guess whether cargo was missing or the JSON was damaged.
    Write-Selection -Escalated $true -Rule 'metadata-unreadable' -Changed $changed `
        -MatrixReason "FULL run: the dependency graph could not be read ($($_.Exception.Message))"
    exit 0
}

$rootPrefix = (ConvertTo-RepoPath -Path ((Resolve-Path -LiteralPath $RepoRoot -ErrorAction SilentlyContinue).Path)) + '/'
$crates = @{}
foreach ($package in $metadata.packages) {
    $manifest = ConvertTo-RepoPath -Path ([string]$package.manifest_path)
    if ($manifest.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        $manifest = $manifest.Substring($rootPrefix.Length)
    }
    $directory = ($manifest -replace '/Cargo\.toml$', '')
    if ($directory -eq $manifest) { $directory = '' }
    $crates[[string]$package.name] = [ordered]@{
        name      = [string]$package.name
        directory = $directory
        # BOTH kinds, and the dev edge is the one that matters: integration tests live in
        # dev-dependencies, so a `dependencies`-only walk drops exactly the crate whose tests
        # exercise the change -- silently, and with every remaining stage green.
        deps      = @(if ($package.dependencies) { $package.dependencies | ForEach-Object { [string]$_.name } } else { @() })
    }
}

# THE GATE'S OWN RECEIPT IS NOT BUILD INPUT -- the same reasoning #899 used for the freeze rule.
# #674(a) made every authoritative run commit its manifest under this prefix onto the branch it
# judged, so from a branch's SECOND run on the store was always in the diff. The store has been
# git-ignored scratch since 2026-09-24, but a branch cut before then still carries receipts. Left to the unmapped
# rule below it maps to no crate and escalates the run to FULL, every time, for ever.
#
# Measured on #919's real range, where the only other changed file was one `apps/cli` test:
#   escalationRule "unmapped-path", unmapped [".factory/gate-runs/a50e7d0a24ee-....json"]
#
# A scope selector that escalates on its own gate's receipt can never narrow anything -- this
# deliverable failing completely, with every existing cell still green, because no fixture
# contained a manifest path. The exemption is this prefix and nothing wider: any other file under
# `.factory/` still reaches the unmapped rule and still escalates.
$RunManifestStore = '.factory/gate-runs/'
$changed = @($changed | Where-Object { -not $_.StartsWith($RunManifestStore, [System.StringComparison]::Ordinal) })
if ($changed.Count -eq 0) {
    Write-Selection -Escalated $true -Rule 'manifest-only' -Changed @() `
        -MatrixReason 'FULL run: the only changes are run manifests, which are not build input, so there is nothing to derive a scope from'
    exit 0
}

# ---- map changed paths to crates, by LONGEST directory prefix -------------------------------
$seeds = New-Object System.Collections.Generic.HashSet[string]
$unmapped = New-Object System.Collections.Generic.List[string]
foreach ($path in $buildInput) {
    $best = $null
    $bestLength = -1
    foreach ($crate in $crates.Values) {
        if ([string]::IsNullOrEmpty($crate.directory)) { continue }
        $prefix = $crate.directory + '/'
        if ($path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) -and $prefix.Length -gt $bestLength) {
            $best = $crate.name
            $bestLength = $prefix.Length
        }
    }
    if ($null -eq $best) { $unmapped.Add($path) } else { [void]$seeds.Add($best) }
}

if ($unmapped.Count -gt 0) {
    # A path nobody owns is not a path that changes nothing. It is the case the deny-list above did
    # not anticipate, which is the one state where narrowing is least defensible.
    Write-Selection -Escalated $true -Rule 'unmapped-path' -Changed $changed -Unmapped $unmapped.ToArray() `
        -MatrixReason ("FULL run: $($unmapped.Count) changed path(s) map to no crate: $($unmapped -join ', ')" +
            $(if ($markdownReadBy.Count -gt 0) { ' (non-build paths read by: ' + (@($markdownReadBy.Keys | ForEach-Object { "$_ <- $($markdownReadBy[$_] -join ' ')" }) -join '; ') + ')' } else { '' }))
    exit 0
}

# ---- expand to transitive DEPENDENTS (reverse edges) ----------------------------------------
$selected = New-Object System.Collections.Generic.HashSet[string]
foreach ($seed in $seeds) { [void]$selected.Add($seed) }
$changedInPass = $true
while ($changedInPass) {
    $changedInPass = $false
    foreach ($crate in $crates.Values) {
        if ($selected.Contains($crate.name)) { continue }
        foreach ($dependency in $crate.deps) {
            if ($selected.Contains($dependency)) {
                [void]$selected.Add($crate.name)
                $changedInPass = $true
                break
            }
        }
    }
}

# ---- the PostgreSQL matrix -------------------------------------------------------------------
$matrix = $false
$matrixReason = 'skipped: nothing in the selection reaches adapters/postgres-event-store/'
foreach ($name in $selected) {
    $directory = $crates[$name].directory
    if ($directory -like 'adapters/postgres-event-store*') {
        $matrix = $true
        $matrixReason = "ran: $name is in the selection"
        break
    }
}

$suiteReach = Get-SuiteReachForChange -Paths $nonManifest

Write-Selection -Escalated $false -Rule $null -Crates @($selected) -Changed $changed `
    -Matrix $matrix -MatrixReason $matrixReason `
    -PsSuites ([bool]$suiteReach.run) -PsSuitesReason ([string]$suiteReach.reason)
exit 0
