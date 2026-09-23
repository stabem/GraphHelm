<#
.SYNOPSIS
    Decide whether a change is documentation that no code reads, so the queued runner may skip
    building it (#901; owner order 2026-09-23).

.DESCRIPTION
    `.factory/MERGE-CHECKLIST.md` already lets a presser merge a pull request whose file list is
    entirely Markdown that NO CODE reads without a gate receipt (the docs-only exception, #865 /
    #957), provided the merge comment cites the empty grep and its positive control. Until now that
    decision was made by hand, after the runner had already spent a full gate on the same change:
    measured on #1214, about seventeen minutes of the slot for four Markdown files. This script is
    the instrument that decision names, so the runner can take it before building and the presser
    can cite the same output.

    THE RULE, in order; the first that fails decides:
      1. The changed set, minus `.factory/gate-runs/` receipts, is non-empty.
      2. Every path in it ends in `.md` (case-insensitive) and is not under `extensions/`, whose
         Markdown files are digest-bound package contributions, and has no directory segment
         named `tests`, `test`, `fixtures`, `fixture` or `testdata`: a test can WALK such a tree
         and pin every file in it without naming one (review of #1216 by lane 5bdc38, measured:
         `adapters/tool-host/tests/context_quality.rs` hashes every file under
         `tests/fixtures/context-quality/tree/` against a Markdown manifest, so rule 4 never saw
         the name and an edit there would have been skipped and turned main red).
      3. The instrument works: `git grep` with the same revision and pathspecs finds at least one
         line in the tree. A grep that finds nothing because the revision did not resolve, or the
         pathspecs excluded everything, must not read as "nobody reads it".
      4. For each path, the BASENAME appears in no tracked file at `-Head` that is not Markdown and
         not a gate receipt. A mention counts as a read. That is deliberately wider than "the file is
         opened": `AGENTS.md` and `.factory/*.md` are named by several `ci/*.ps1` files, some only
         in prose, and all of them keep such a change on the full gate. The rule may keep a change
         on the gate that could have skipped it; it can never skip a change that a consumer reads
         by name.

    EXIT CODES ARE THE CONTRACT. 0 docs-only (the runner may skip); 1 not docs-only (build);
    2 the question could not be answered (build). A caller acts only on 0.

.PARAMETER ChangedFiles
    ';'-separated repo-relative paths. Omitted, they come from `git diff --name-only`.
#>
param(
    [string] $ChangedFiles,
    [string] $RepoRoot = '.',
    [string] $MergeBase,
    [string] $Head = 'HEAD'
)

Set-StrictMode -Version Latest
# `Continue`, not `Stop`: under Windows PowerShell 5.1 a native command writing to stderr through
# `2>&1` becomes a terminating error under `Stop`, and an unhandled one exits 1 -- which this
# contract reads as "not docs-only" when the truth is "could not decide". The trap below turns every
# other surprise into exit 2 for the same reason.
$ErrorActionPreference = 'Continue'

$ReceiptPrefix = '.factory/gate-runs/'
$GrepExclusions = @('.', ':(exclude)*.md', ':(exclude)*.MD', ":(exclude)$ReceiptPrefix**")

function Write-Verdict {
    param(
        [Parameter(Mandatory)] [int] $Code,
        [Parameter(Mandatory)] [string] $Reason,
        [string[]] $Files = @(),
        [object[]] $Readers = @()
    )
    $document = [ordered]@{
        docsOnly = ($Code -eq 0)
        decided  = ($Code -ne 2)
        reason   = $Reason
        head     = $Head
        files    = @($Files)
        readers  = @($Readers)
        rule     = 'basename named in no non-Markdown tracked file at head, with a positive control'
    }
    Write-Output ($document | ConvertTo-Json -Depth 6 -Compress)
    exit $Code
}

trap {
    Write-Verdict -Code 2 -Reason "unexpected error: $($_.Exception.Message)"
}

function ConvertTo-RepoPath {
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)
    $normalised = $Path -replace '\\', '/'
    while ($normalised.StartsWith('./')) { $normalised = $normalised.Substring(2) }
    return $normalised
}

# ---- the changed set -----------------------------------------------------------------------
if ($PSBoundParameters.ContainsKey('ChangedFiles') -and -not [string]::IsNullOrWhiteSpace($ChangedFiles)) {
    $changed = @($ChangedFiles -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { ConvertTo-RepoPath -Path $_.Trim() })
} else {
    if ([string]::IsNullOrWhiteSpace($MergeBase)) {
        Write-Verdict -Code 2 -Reason 'no merge base to diff against'
    }
    # --no-renames (review of #1216 by lane b9deb2, measured): without it a rename prints only the
    # NEW path, so `git mv src/foo.rs NOTES.md` read as one Markdown change and skipped the gate on a
    # head whose `mod foo;` no longer compiles. With it, the removed `.rs` path is in the set.
    $diff = @(& git -C $RepoRoot diff --no-renames --name-only $MergeBase $Head 2>&1)
    if ($LASTEXITCODE -ne 0) {
        Write-Verdict -Code 2 -Reason "git diff exited $LASTEXITCODE, so the changed set is unknown"
    }
    $changed = @($diff | ForEach-Object { ConvertTo-RepoPath -Path ([string]$_) } | Where-Object { $_ })
}

$docs = @($changed | Where-Object { -not $_.StartsWith($ReceiptPrefix, [System.StringComparison]::Ordinal) })
if ($docs.Count -eq 0) {
    Write-Verdict -Code 2 -Reason 'the changed set is empty once gate receipts are set aside' -Files $changed
}

# ---- every path is Markdown outside the packages --------------------------------------------
foreach ($path in $docs) {
    if (-not $path.EndsWith('.md', [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-Verdict -Code 1 -Reason "not Markdown: $path" -Files $docs
    }
    if ($path.StartsWith('extensions/', [System.StringComparison]::Ordinal)) {
        Write-Verdict -Code 1 -Reason "a package contribution, digest-bound by its manifest: $path" -Files $docs
    }
    $testSegment = @($path -split '/' | Select-Object -SkipLast 1 | Where-Object { @('tests', 'test', 'fixtures', 'fixture', 'testdata') -contains $_.ToLowerInvariant() })
    if ($testSegment.Count -gt 0) {
        Write-Verdict -Code 1 -Reason "test data, which a test may walk without naming a file: $path" -Files $docs
    }
}

# ---- positive control: the instrument can see the tree --------------------------------------
$control = @(& git -C $RepoRoot grep -l -e '.' $Head -- @GrepExclusions 2>&1)
$controlCode = $LASTEXITCODE
if ($controlCode -ne 0 -or $control.Count -eq 0) {
    Write-Verdict -Code 2 -Reason "positive control failed: git grep over non-Markdown files at $Head exited $controlCode with $($control.Count) line(s)" -Files $docs
}

# ---- no non-Markdown file names any of them -------------------------------------------------
$readers = New-Object System.Collections.Generic.List[object]
foreach ($path in $docs) {
    $basename = ($path -split '/')[-1]
    $hits = @(& git -C $RepoRoot grep -l -F -e $basename $Head -- @GrepExclusions 2>&1)
    $code = $LASTEXITCODE
    if ($code -eq 1) { continue }
    if ($code -ne 0) {
        Write-Verdict -Code 2 -Reason "git grep for '$basename' exited $code" -Files $docs
    }
    # `git grep <rev>` prefixes each hit with `<rev>:`; strip it so the reader is a path.
    $names = @($hits | ForEach-Object { ([string]$_) -replace ('^' + [regex]::Escape($Head) + ':'), '' })
    $readers.Add([ordered]@{ path = $path; namedBy = @($names) })
}

if ($readers.Count -gt 0) {
    $first = $readers[0]
    Write-Verdict -Code 1 -Reason "named by code: $($first.path) in $($first.namedBy[0])" -Files $docs -Readers $readers.ToArray()
}

Write-Verdict -Code 0 -Reason "$($docs.Count) Markdown file(s), none named by any non-Markdown file at $Head" -Files $docs
