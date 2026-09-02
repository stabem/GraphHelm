<#
.SYNOPSIS
    Brings an EXISTING checkout's tracked scripts into line with the `text eol=lf` rule (#647).

.DESCRIPTION
    A fresh clone needs nothing. A checkout that predates the rule keeps CRLF in the working tree
    while the index already holds LF, and `git status` stays clean, so nothing tells the operator
    the files are still wrong. This is the migration for that case.

    IT IS A PROGRAM AND NOT A RECIPE, because the recipe has five measured failure modes (#676),
    and the fifth ends with an operator's file deleted and not restored. The asymmetry of damage
    sets the bar: someone who runs nothing keeps CRLF, which is visible and reversible; someone who
    runs the wrong thing LOSES WORK. So this refuses loudly in any state it cannot prove safe, and
    it never leaves a path deleted.

    HOW IT AVOIDS EACH OF THE FIVE:

    1. `--renormalize` + `checkout` does nothing, because checkout skips a stat-clean path. This
       does not ask git to rewrite the file: it rewrites the BYTES itself, so being stat-clean is
       irrelevant.
    2. A pathspec matching nothing makes `git checkout` abort the whole command. `git ls-files` is
       the enumerator here and returns empty with exit 0 for an unmatched pathspec (measured), so a
       repository with no `.ps1` is an ordinary empty case rather than an abort.
    3. The guard is one expression that DECIDES, not a warning printed next to the destructive
       step. Every refusal returns before any file is touched.
    4. `--force` destroys uncommitted edits inside scripts and leaves `git status` clean, so the
       loss is unrecorded. Nothing here is forced: a file with an uncommitted edit is refused BY
       NAME and left exactly as it is.
    5. The delete-then-restore form deletes every script and restores none when one is
       `skip-worktree`. There is no delete step at all. Each file is rewritten through a temporary
       file in the same directory and moved over the original, so a failure mid-run leaves either
       the old file or the new one -- never nothing.

    WHAT IT DOES NOT DO AT ALL, IN THIS FORM: it does not write. Without `-DryRun` it REFUSES,
    before any enumeration or read. The half that writes is tracked in #693, on branch
    `issue-676-write-mode`, and the five answers above describe what it will do when that lands --
    they are the design under review there, not behaviour available here.

    It also does not touch the index, run `checkout`, stage anything, or look at untracked files.

.PARAMETER DryRun
    REQUIRED in this form. Reports what would change and changes nothing; without it the program
    refuses and points at #693. The exit codes are the diagnosis's own: 0 when every covered path
    got a verdict, 1 for a refusal or an unclassified residue, and 2 when the instrument itself
    could not be trusted.

.PARAMETER Path
    Restrict the sweep to paths under this directory (repository-relative). Default: the whole
    repository.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File ci/normalize-script-eol.ps1 -DryRun

.EXAMPLE
    From Git Bash, which is why this is a program rather than a shell one-liner: `xargs` is not a
    Windows command, and `git ls-files -- '*.sh','*.ps1','*.py'` in PowerShell passes ONE
    comma-joined argument that matches nothing and returns empty WITHOUT an error (#676).

    powershell -NoProfile -ExecutionPolicy Bypass -File ci/normalize-script-eol.ps1
#>
param(
    [switch] $DryRun,
    [string] $Path
)

$ErrorActionPreference = 'Stop'

# The three patterns are the rule's own, repo-wide and with no directory prefix
# (.gitattributes: `*.sh text eol=lf`, `*.ps1 text eol=lf`, `*.py text eol=lf`). They are listed
# here rather than parsed out of .gitattributes because a parser would be a second implementation
# of git's attribute matching; instead every file this script touches is CHECKED against git's own
# answer (`attr/` below), so a pattern that drifted from the rule can only ever mean skipping a
# file, never rewriting one the rule does not cover.
# `:(icase)` because Windows checkouts hold `BUILD.PS1`, and git's attribute matching honours
# core.ignoreCase while a bare lowercase PATHSPEC does not -- so the rule covered such a file and
# this sweep did not even list it. A guard that silently omits part of its population is worse than
# no guard: the part it still sees keeps it looking healthy.
$ScriptPathspecs = @(':(icase)*.sh', ':(icase)*.ps1', ':(icase)*.py')

# Declared here, with the pathspecs, because the count is checked at ENUMERATION -- before the
# per-file loop where the byte bounds live. The first version put it beside the byte bounds and it
# was empty at the point of use, so `$records.Count -gt $MaxPaths` compared a number with nothing
# and refused the first repository it saw, with the ceiling missing from its own message. A bound
# that is not in scope where it is tested is not a bound.
$MaxPaths = 5000
# The enumeration is read before any per-path bound can apply, so it carries its own.
$MaxEnumerationChars = 20000000

# ISO-8859-1 by code page, because `[System.Text.Encoding]::Latin1` does not exist on .NET
# Framework -- it arrived in .NET 5, and this runs under Windows PowerShell 5.1. The encoding is a
# byte-preserving one on purpose: every byte maps to exactly one character and back, so a script
# that is not valid UTF-8 still round-trips unchanged. Reading these files as text in any other
# encoding would re-encode them, which is the damage this script exists to undo.
$Latin1 = [System.Text.Encoding]::GetEncoding(28591)

# git writes path bytes as UTF-8. PowerShell decodes a native command's output with the CONSOLE's
# encoding, which on a default Windows install is a legacy code page, so `café.sh` came back as
# `cafÃ©.sh` -- a path that exists nowhere. The program then reported the file as "missing from the
# working tree" and refused a checkout that was perfectly fine: a refusal that is loud, specific
# and about a file that is right there. Found while sabotaging the quoted-pathname fix, which is
# the only reason this was ever reached.
$previousConsoleEncoding = [Console]::OutputEncoding
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false) } catch { }

function Fail {
    param([Parameter(Mandatory)] [string] $Message, [int] $Code = 1)
    Write-Host "[eol] REFUSED: $Message" -ForegroundColor Red
    exit $Code
}

function Get-IndexBlobBytes {
    <#
        The index blob as RAW BYTES, with no shell and no temporary file.

        Three earlier shapes were wrong for three different reasons and are worth naming, because
        each looks fine until the input is unusual:

          a PowerShell variable   captures the blob as a STRING and re-encodes it -- the exact
                                  damage this program exists to undo;
          `cmd /c ... > file`     expands %NAME% inside the command line even when quoted, so a
                                  script named `%PATH%.sh` had its object argument rewritten
                                  before git saw it;
          Start-Process -ArgumentList @(...)
                                  joins the array with spaces and does NOT quote the elements
                                  under Windows PowerShell 5.1, so a checkout path or a script
                                  name containing a space arrives at git as two arguments.

        So the arguments are quoted here, explicitly, and the output is read from the process's
        own byte stream.
    #>
    param(
        [Parameter(Mandatory)] [string] $RepositoryRoot,
        [Parameter(Mandatory)] [string] $Object,
        [Parameter(Mandatory)] [long] $MaxBytes
    )

    $quoted = @('-C', $RepositoryRoot, 'cat-file', 'blob', $Object) |
        ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = 'git'
    $info.Arguments = ($quoted -join ' ')
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.CreateNoWindow = $true

    $process = [System.Diagnostics.Process]::Start($info)
    $buffer = New-Object System.IO.MemoryStream
    try {
        # Copied in bounded chunks rather than with CopyTo, so the ceiling holds even if the
        # object turns out larger than the size that authorised this read.
        $chunk = New-Object byte[] 65536
        while ($true) {
            $read = $process.StandardOutput.BaseStream.Read($chunk, 0, $chunk.Length)
            if ($read -le 0) { break }
            if (($buffer.Length + $read) -gt $MaxBytes) { return $null }
            $buffer.Write($chunk, 0, $read)
        }
        $process.StandardError.ReadToEnd() | Out-Null
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { return $null }
        # `,` before the array. PowerShell UNROLLS a returned collection, and an EMPTY byte[]
        # unrolls to nothing at all -- so an empty tracked script came back as $null and was
        # refused as "its index blob could not be read", which is the failure signal this
        # function uses for a real failure. The comma wraps it so a zero-length result stays a
        # zero-length array and keeps its meaning: read successfully, nothing in it.
        return ,$buffer.ToArray()
    } finally {
        $buffer.Dispose()
        $process.Dispose()
    }
}

function Test-ExactDirectory {
    <#
        Does this relative path exist on disk with EXACTLY this spelling?

        `Test-Path` cannot answer on Windows: the filesystem is case-insensitive, so it says yes
        for `Ci` when only `ci` exists. Each segment is therefore matched against the real
        directory entries, case-sensitively, walking down from the repository root.
    #>
    param([Parameter(Mandatory)] [string] $Root, [Parameter(Mandatory)] [string] $RelativePath)

    $current = $Root
    foreach ($segment in @($RelativePath -split '/' | Where-Object { $_ -ne '' })) {
        $match = @(Get-ChildItem -LiteralPath $current -Force -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -ceq $segment })
        if ($match.Count -ne 1) { return $false }
        $current = $match[0].FullName
    }
    return $true
}

function Get-DistinctScopeSpellings {
    <#
        The distinct spellings of the first `$Depth` segments, compared CASE-SENSITIVELY.

        A seam that takes strings on purpose. The list this decides over comes from `git ls-files`,
        never from the disk, so a cell can feed it `@('parent/ci/x.sh','parent/CI/x.sh')` and get a
        verdict on any filesystem. Written inline it could only be exercised on a case-sensitive
        volume, which meant the sabotage that removes `-CaseSensitive` proved nothing on the
        machine the suite runs on -- a guard whose test depends on the developer's disk.

        `Sort-Object -Unique` is case-INSENSITIVE by default, which collapsed `ci` and `CI` into one
        and made the ambiguity refusal that consumes this unable to fire at all. A refusal that
        cannot fire is not a refusal.
    #>
    param([Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Paths, [Parameter(Mandatory)] [int] $Depth)

    return @(@($Paths) | ForEach-Object { (@($_ -split '/')[0..($Depth - 1)]) -join '/' } |
            Sort-Object -Unique -CaseSensitive)
}

function Read-BoundedGit {
    <#
        A git listing read under a character ceiling enforced AS IT ARRIVES.

        Three call sites read listings -- the two `-Path` probes and the main enumeration -- and
        all three run before any per-path bound can apply, because they PRODUCE the list the paths
        are filtered out of. `-Path .` in a large repository materialised every tracked entry
        first. The ceiling has to be checked while the lines are consumed rather than after the
        collection exists, or it is a limit on something already in memory.
    #>
    param([Parameter(Mandatory)] [string[]] $GitArgs)

    $collected = New-Object System.Collections.Generic.List[string]
    $chars = 0L
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $code = 0
    try {
        foreach ($line in (& git @GitArgs 2>$null)) {
            $text = [string]$line
            $chars += $text.Length
            if ($chars -gt $MaxEnumerationChars) {
                return [ordered]@{ exitCode = 0; lines = @(); overflowed = $true }
            }
            $collected.Add($text)
        }
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [ordered]@{ exitCode = $code; lines = @($collected); overflowed = $false }
}

function Invoke-Git {
    param([Parameter(Mandatory)] [string[]] $GitArgs)
    # 'Continue' around the native call, restored after. Under Windows PowerShell 5.1 a redirected
    # native stderr line becomes a NativeCommandError, and $ErrorActionPreference = 'Stop' promotes
    # it to a terminating error -- so `git rev-parse` failing OUTSIDE a working tree would kill this
    # function before its own exit-code check could report HARNESS-BROKE. The diagnostic would be
    # unreachable in exactly the case it exists for. gate.ps1's Invoke-Stage documents the same
    # trap; the tests in this directory hit it too and take the same way out.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        # stderr to $null, NOT merged into the output. `2>&1` puts git's diagnostics into the
        # stream this function's callers PARSE -- and under GIT_TRACE=1 git writes trace records
        # on stderr while still exiting 0, so a developer with tracing on would have trace lines
        # parsed as paths. The exit code is the signal; the diagnostics belong to the console.
        $output = & git @GitArgs 2>$null
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [ordered]@{ exitCode = $code; lines = @($output | ForEach-Object { [string]$_ }) }
}

# HARNESS-BROKE, not a refusal: these say the instrument is not usable, which is a different state
# from "the tree is not safe to touch" and must not be read as one.
$probe = Invoke-Git -GitArgs @('rev-parse', '--show-toplevel')
if ($probe.exitCode -ne 0) {
    Write-Host '[eol] HARNESS-BROKE: not inside a git working tree (git rev-parse failed).' -ForegroundColor Magenta
    exit 2
}
$repositoryRoot = $probe.lines[0]

# THE WRITE PATH IS NOT IN THIS PULL REQUEST, and the refusal is here -- before any enumeration,
# any read, any decision -- so there is no arrangement in which this program writes.
#
# #676 measures the damage the copy-paste recipe does, and all of it lives in the WRITING: a
# deleted file that is never restored, an edit destroyed with `git status` clean afterwards. That
# danger earns its own review with its own surface rather than arriving as the second half of a
# change a reviewer has already read past.
#
# The code that writes exists, is green, and carries 28 resolved review findings: branch
# `issue-676-write-mode`, tracked in #693. Cutting it out reduces DEFECT, not observation --
# everything below still names every case the recipe destroys.
if (-not $DryRun) {
    Fail ("write mode is not in this pull request, which ships the diagnosis only. Re-run with " +
        "-DryRun to see what would change. The write half is tracked in #693, on branch " +
        "issue-676-write-mode.")
}

# `git ls-files --eol -z` emits NUL-separated records; within a record the three attribute columns
# are separated from the path by a TAB. Measured on git 2.47.1 before relying on it: an unmatched
# pathspec yields an empty list and exit 0, unlike `git checkout`, which prints an error naming
# only the missing extension.
# --full-name because `git ls-files` reports paths relative to the CURRENT DIRECTORY, so
# running this from `ci/` with an absolute -File path would yield `classify-run.ps1` and the
# join below would look for it at the repository root and refuse it as missing. The paths this
# program prints, refuses over and joins are all repository-relative, and this is what makes
# that true rather than assumed.
# `-C $repositoryRoot` as well as `--full-name`, because they answer different halves and only
# both together are enough: --full-name fixes the FORM of the paths, and -C fixes their
# SCOPE. Run from a subdirectory without -C, `git ls-files` lists only what is under the
# current directory, so the program reported success having silently swept a fraction of the
# repository -- exit 0 and a file at the root still CRLF. Caught by the cell written for the
# --full-name fix, which is the only reason the scope half was noticed at all.
# `icase` alongside `literal`, for the same reason the extensions carry it: on a case-insensitive
# Windows checkout `-Path CI` names the existing `ci`, and a case-sensitive pathspec would return
# no records at all -- the program then exits 0 having touched nothing, which is the worst way to
# be wrong. It is the same axis as `BUILD.PS1`, on the directory side rather than the file side.
#
# -Path is a DIRECTORY the operator typed, not a pattern. Pasting it in front of `*.sh` makes its
# own characters part of a pathspec, so a real directory named `scope[1]` becomes a wildcard that
# matches `scope1/` and skips the files actually asked for. `:(literal)` turns off pathspec magic
# for that argument, and the extensions are filtered here instead -- which also removes the
# interaction between the two entirely. With no -Path the three globs are used as before: there is
# no operator string in them to be reinterpreted.
$eolArgs = @('-C', $repositoryRoot, 'ls-files', '--eol', '--full-name', '-z', '--')
# Case-SENSITIVE first, case-insensitive only as a fallback. `icase` alone selects both `ci/` and
# `CI/` where the volume distinguishes them -- widening a scope the operator narrowed. Asking for
# the exact spelling first means a checkout that HAS it gets exactly it, and the fallback only
# runs where the exact spelling matched nothing, which is the Windows case this was added for.
$pathSpecPrefix = ':(literal)'
if ($Path) {
    $normalisedPath = $Path -replace [regex]::Escape([System.IO.Path]::DirectorySeparatorChar), '/'
    $exact = Read-BoundedGit -GitArgs @('-C', $repositoryRoot, 'ls-files', '-z', '--', ":(literal)$normalisedPath")
    # A FAILED probe is not an empty one. `$exact.exitCode -eq 0` is required before the emptiness
    # is believed, because a transient failure returns empty output too -- and reading that as "the
    # exact spelling matches nothing" silently widens the scope to the case-insensitive form. The
    # failure is HARNESS-BROKE: the instrument did not answer, which is not the same as answering
    # no.
    if ($exact.overflowed) {
        Write-Host ("[eol] REFUSED: the exact-spelling probe for -Path lists more than " +
            "$MaxEnumerationChars characters. Narrow the run.") -ForegroundColor Red
        exit 1
    }
    if ($exact.exitCode -ne 0) {
        Write-Host '[eol] HARNESS-BROKE: the exact-spelling probe for -Path failed, so nothing here knows whether the directory exists as written.' -ForegroundColor Magenta
        exit 2
    }
    if ((($exact.lines -join '') -replace "`0", '').Trim() -eq '') {
        # AN EMPTY SCOPE IS NOT A MISSING ONE. On a case-sensitive checkout holding an untracked
        # `Ci/` beside a tracked `ci/a.sh`, the exact probe comes back empty -- there are no index
        # entries under `Ci/` -- and falling back to `icase` then reported a file from a directory
        # the operator did not name. The disk is asked whether the spelling they typed exists
        # before the fallback is allowed: if it does, the scope is empty and that is the answer.
        if (Test-ExactDirectory -Root $repositoryRoot -RelativePath $normalisedPath) {
            Write-Host "[eol] -Path '$Path' exists and holds no tracked scripts; nothing to do." -ForegroundColor Cyan
            exit 0
        }
        $pathSpecPrefix = ':(literal,icase)'
        # And if the case-insensitive form then matches MORE THAN ONE spelling, the operator's
        # request is ambiguous and this program does not get to pick. `-Path Ci` where both `ci/`
        # and `CI/` exist would otherwise sweep both -- a scope wider than the one that was asked
        # for, chosen silently.
        $fallback = Read-BoundedGit -GitArgs @('-C', $repositoryRoot, 'ls-files', '-z', '--', ":(literal,icase)$normalisedPath")
        # The segment `-Path` NAMES, not the first segment of the path. `-Path parent/Ci` matching
        # both `parent/ci` and `parent/CI` yields ONE distinct first segment -- `parent` -- so the
        # ambiguity went unseen and both were swept. The depth of the requested path decides which
        # segments to compare.
        $depth = @($normalisedPath -split '/' | Where-Object { $_ -ne '' }).Count
        $spellings = Get-DistinctScopeSpellings -Paths @(($fallback.lines -join '') -split "`0" |
                Where-Object { $_ -ne '' }) -Depth $depth
        if ($fallback.overflowed -or $fallback.exitCode -ne 0) {
            Write-Host '[eol] HARNESS-BROKE: the case-insensitive probe for -Path did not answer, so nothing here knows whether the scope is ambiguous.' -ForegroundColor Magenta
            exit 2
        }
        if ($spellings.Count -gt 1) {
            Fail ("-Path '$Path' matches no directory exactly, and matches more than one when case " +
                "is ignored: " + ($spellings -join ', ') + ". Name the one you mean.")
        }
    }
    $eolArgs += $pathSpecPrefix + $normalisedPath
} else {
    foreach ($spec in $ScriptPathspecs) { $eolArgs += $spec }
}
$enumeration = Read-BoundedGit -GitArgs $eolArgs
if ($enumeration.overflowed) {
    Write-Host ("[eol] REFUSED: the enumeration for this scope passes $MaxEnumerationChars characters " +
        "of git output, which is more than this program will hold before it has decided anything. " +
        "Narrow the run with -Path.") -ForegroundColor Red
    exit 1
}
$eolRaw = $enumeration.lines -join ''
if ($enumeration.exitCode -ne 0) {
    Write-Host '[eol] HARNESS-BROKE: git ls-files --eol failed.' -ForegroundColor Magenta
    exit 2
}

$records = @($eolRaw -split "`0" | Where-Object { $_ -ne '' })
# A count bound as well as a byte bound: the per-path records, the parsed entries and the flag
# listing are all proportional to how many paths matched, and thousands of EMPTY scripts weigh
# nothing against the retained-byte budget while still multiplying that state. A ceiling that only
# counts bytes is not a ceiling on a sweep.
if ($records.Count -eq 0) {
    Write-Host '[eol] no tracked .sh/.ps1/.py files in scope; nothing to do.' -ForegroundColor Cyan
    exit 0
}

$extensions = @('.sh', '.ps1', '.py')
$files = @()
foreach ($record in $records) {
    $tab = $record.IndexOf("`t")
    if ($tab -lt 0) {
        Fail "could not parse a `git ls-files --eol` record: [$record]. Refusing rather than guessing which part is the path." 2
    }
    $columns = $record.Substring(0, $tab)
    # The extension filter lives here rather than in the pathspec, because -Path is literal above.
    # Case-INSENSITIVE on purpose, and unlike every other comparison in this file: an extension is
    # a filesystem fact, not content, and `A.SH` names the same kind of file as `a.sh`.
    if (-not ($extensions | Where-Object { $record.Substring($tab + 1).ToLowerInvariant().EndsWith($_) })) { continue }
    # The ceiling is enforced AS THE LIST GROWS. Checked after the loop it is a bound on something
    # already built, and a scope of very many SHORT paths passes the character bound above while
    # still multiplying this state -- the same defect as bounding bytes and not count, one level
    # in.
    if ($files.Count -ge $MaxPaths) {
        Fail ("this scope matches more than $MaxPaths tracked scripts, which is more than this " +
            "program will hold state for at once. Narrow the run with -Path and repeat.")
    }
    $files += [ordered]@{
        path      = $record.Substring($tab + 1)
        vetted    = $null
        indexEol  = if ($columns -cmatch 'i/(\S+)') { $Matches[1] } else { '?' }
        worktree  = if ($columns -cmatch 'w/(\S+)') { $Matches[1] } else { '?' }
        attribute = if ($columns -cmatch 'attr/(.*?)\s*$') { $Matches[1] } else { '' }
    }
}

# The count ceiling, applied to the SCRIPTS and not to everything the pathspec matched. With
# `-Path` naming a directory the enumeration returns every tracked entry beneath it, so counting
# $records refused `-Path .` in any large repository -- a ceiling on the wrong population, which is
# a ceiling on the wrong question. The extension filter above has already run here.
if ($files.Count -gt $MaxPaths) {
    Fail ("this scope matches $($files.Count) tracked scripts, beyond the $MaxPaths this program " +
        "will hold state for at once. Narrow the run with -Path and repeat; the bound is on the " +
        "COUNT because a thousand empty scripts cost nothing in bytes and everything in bookkeeping.")
}

# REFUSAL 1: index flags. `git ls-files -v` marks skip-worktree with `S` and assume-unchanged with
# a lowercase letter. Failure mode 5 is what happens when these are swept along: the files are
# deleted and the restore refuses the whole batch, so the operator's file is simply gone. They are
# named and the run stops -- excluding them silently would leave a checkout half-migrated with no
# record of which half.
# The SAME pathspecs the sweep uses, including -Path. Scanning the whole repository here would
# abort a `-Path ci` migration over a flagged script somewhere it was never going to touch --
# a refusal that is true about the repository and false about the work being asked for.
$flagPathspecs = @()
if ($Path) {
    $flagPathspecs += $pathSpecPrefix + ($Path -replace [regex]::Escape([System.IO.Path]::DirectorySeparatorChar), '/')
} else {
    foreach ($spec in $ScriptPathspecs) { $flagPathspecs += $spec }
}
$flagged = @()
# -z, like the enumeration above. Without it git applies `core.quotePath` and emits
# `S "cafÃ©.sh"` for a non-ASCII name -- the extension test then sees a trailing quote,
# skips the record, and the skip-worktree refusal never fires for exactly the file that needed it.
# A guard that silently stops seeing some of its population is worse than no guard, because the
# population it still sees keeps it looking healthy.
# The exit code, checked. A transient index read error here returns empty or partial stdout, and an
# empty flag list is INDISTINGUISHABLE from "nothing is flagged" -- the refusal that protects an
# operator's skip-worktree file would simply not happen, quietly. Same vacuity rule as the
# post-write verifier: an instrument that saw nothing must not report an all-clear.
$flagRun = Invoke-Git -GitArgs (@('-C', $repositoryRoot, 'ls-files', '-v', '-z', '--full-name', '--') + $flagPathspecs)
if ($flagRun.exitCode -ne 0) {
    Write-Host '[eol] HARNESS-BROKE: the skip-worktree enumeration failed, so nothing here observed the index flags.' -ForegroundColor Magenta
    exit 2
}
$flagOutput = @(($flagRun.lines -join '') -split "`0" | Where-Object { $_ -ne '' })
# The covered set, computed once and used by BOTH loops. A script the attribute does not cover is
# not this program's business anywhere: flagging a `-text` file as skip-worktree refused a run over
# a file that would never have been touched, and the same path missing from the working tree was
# reported as an uncommitted edit. Two symptoms, one ordering defect -- the exemption has to be
# read before any state is judged.
$covered = @($files | Where-Object { $_.attribute -cmatch 'eol=lf' } | ForEach-Object { $_.path })
foreach ($line in @($flagOutput)) {
    if ($line -cmatch '^\S+\s+(.+)$' -and ($covered -cnotcontains $Matches[1])) { continue }
    # -cmatch, NOT -match. PowerShell's -match is case-INSENSITIVE by default, so `[a-z]` also
    # matches `H`, which is git's letter for an ordinary cached file. The first run of this script
    # refused the entire repository -- 38 normal files reported as skip-worktree -- and the refusal
    # was word-perfect and completely wrong. A guard whose predicate is case-blind reads every file
    # as the state it exists to catch.
    if ($line -cmatch '^([a-z]|S)\s+(.+)$') { $flagged += "$($Matches[2]) [$($Matches[1])]" }
}
if ($flagged.Count -gt 0) {
    Fail ("these tracked scripts carry a skip-worktree or assume-unchanged flag, and a migration " +
        "that sweeps them along deletes them without restoring them (#676 mode 5). Clear the flag " +
        "with ``git update-index --no-skip-worktree <path>`` and re-run, or migrate with a fresh " +
        "clone:`n  " + ($flagged -join "`n  "))
}

# REFUSAL 2: uncommitted edits. `--force` would destroy these and leave `git status` clean
# afterwards, so the loss would go unrecorded (#676 mode 4). A file whose ONLY difference is CRLF
# is not an edit -- that is the very thing being migrated -- so the comparison is made on bytes
# with the line endings taken out, which is the same distinction `git diff -w` draws by hand.
# Repository content is untrusted, and every file here is read WHOLE three times over -- the
# working bytes, the index blob, and the converted copy. A tracked script far larger than any
# script has reason to be would exhaust memory before any refusal could be printed, so the size
# is checked from the directory entry BEFORE the first read. 8 MiB is far above every script in
# this repository (the largest is a few tens of KiB) and far below anything that could hurt.
$MaxScriptBytes = 8MB
# The per-file bound does not bound the SWEEP: every vetted file's bytes are retained until the
# write phase, so the aggregate needs its own ceiling. Same shape as the workspace sweep's byte
# budget in core/protocols, and for the same reason -- a per-item limit says nothing about a total.
$MaxRetainedBytes = 256MB
$dirty = @()
$oversize = @()
$notNormalised = @()
$unsupported = @()
$retained = 0L
foreach ($file in $files) {
    # BEFORE anything is judged about it, including whether it exists.
    if ($covered -cnotcontains $file.path) { continue }

    $full = Join-Path $repositoryRoot $file.path
    if (-not (Test-Path -LiteralPath $full)) {
        $dirty += "$($file.path) [missing from the working tree]"
        continue
    }
    # An index blob that is not already LF means the tree was never renormalised after the
    # attribute landed. Normalising BOTH sides for the comparison then hides a real difference and
    # the working copy reads clean, so this program would rewrite a file whose index still
    # disagrees with it. That is a repository-level state to fix with `git add --renormalize`, not
    # something a working-tree sweep may paper over.
    if ($file.indexEol -ceq '-text') {
        # git itself says this is not text. The commonest cause for a tracked script is UTF-16 --
        # a normal encoding for Windows PowerShell -- whose CRLF is 0D 00 0A 00, with no adjacent
        # CR-LF pair for a byte replacement to find. Such a file would be reported as ALREADY
        # NORMALISED while still carrying every CRLF it started with, and a silent no-op that
        # reads as success is the one outcome this program must never produce. So it is named
        # here rather than skipped, and named with the reason a reader can act on.
        $unsupported += "$($file.path) [git reports it as binary; UTF-16 is the usual cause]"
        continue
    }
    # `none` is not `crlf`. git reports it for a blob with NO line endings at all -- an empty
    # script, or one unterminated line -- and such a file cannot disagree with the rule because
    # there is nothing in it to disagree. Refusing it sent an operator to renormalise a file that
    # has no line endings to normalise. `lf` and `none` are both fine; anything else is the
    # unrenormalised index this refusal is for.
    if ($file.indexEol -cne 'lf' -and $file.indexEol -cne 'none') {
        $notNormalised += "$($file.path) [index is $($file.indexEol)]"
        continue
    }
    # A named refusal rather than an exception: another process removing or replacing the path
    # between the existence check and this read makes Get-Item throw, and a stack trace with a
    # full path in it is not the diagnosis this program promises.
    try {
        $length = (Get-Item -LiteralPath $full -ErrorAction Stop).Length
    } catch {
        $dirty += "$($file.path) [its metadata could not be read: $($_.Exception.GetType().Name)]"
        continue
    }
    if ($length -gt $MaxScriptBytes) {
        $oversize += "$($file.path) [$length bytes]"
        continue
    }
    # The length is re-checked WHILE reading, from the handle that is actually being read: the
    # directory-entry check above is a different instant, and a file grown or replaced in between
    # would be allocated in full before anything noticed. Reading through a bounded stream makes
    # the bound a property of the read rather than of a stat taken earlier.
    # A tracked script replaced in the working tree by a DIRECTORY passes Test-Path and reports a
    # null Length, so it slid past the size check and then threw an uncaught
    # UnauthorizedAccessException out of File.Open -- a stack trace where this program promises a
    # named refusal. `File::Exists` is false for a directory, which is the distinction Test-Path
    # does not draw.
    if (-not [System.IO.File]::Exists($full)) {
        $dirty += "$($file.path) [not a regular file in the working tree]"
        continue
    }
    # A regular file can still refuse to open: an ACL that denies read, or a handle held without
    # read sharing. `File::Exists` cannot see either, so the open is guarded and becomes a named
    # refusal rather than an exception with a full path in it.
    try {
        $handle = [System.IO.File]::Open($full, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
    } catch {
        $dirty += "$($file.path) [could not be opened for reading: $($_.Exception.GetType().Name)]"
        continue
    }
    try {
        # Captured ONCE. `$handle.Length` is read from the file system on every access, so a file
        # grown between the check and the allocation would be checked at one size and allocated at
        # another -- the bound authorising a buffer it never measured.
        $openLength = $handle.Length
        if ($openLength -gt $MaxScriptBytes) {
            $oversize += "$($file.path) [$openLength bytes at open]"
            continue
        }
        $working = New-Object byte[] $openLength
        $read = 0
        while ($read -lt $openLength) {
            $chunk = $handle.Read($working, $read, $openLength - $read)
            if ($chunk -le 0) { break }
            $read += $chunk
        }
    } finally {
        $handle.Dispose()
    }

    # UTF-16 is refused rather than skipped. A UTF-16LE PowerShell script -- a common Windows
    # encoding -- writes CRLF as 0D 00 0A 00, so there is no adjacent CR-LF pair for the byte
    # replacement to find and the file would be reported as ALREADY LF while still carrying every
    # CRLF it started with. A silent no-op that reads as success is the one outcome this program
    # must never produce, so the encoding is detected from the BOM and named.
    if ($working.Length -ge 2 -and (
            ($working[0] -eq 0xFF -and $working[1] -eq 0xFE) -or
            ($working[0] -eq 0xFE -and $working[1] -eq 0xFF))) {
        $unsupported += "$($file.path) [UTF-16 byte-order mark]"
        continue
    }

    # The INDEX side has its own size, and the working-tree entry says nothing about it: a script
    # committed at 40 MiB whose working copy has since been replaced by one short line passes the
    # check above and is then materialised in full. `cat-file -s` asks git for the blob's size
    # without producing a byte of it.
    # The path is resolved to a BLOB OID once, and the size and the read then both name that
    # oid. `:path` is re-resolved on every use, so another git process updating the index between
    # the two calls would let the size of one blob authorise the read of a different one -- the
    # bound defeated without either call being wrong on its own.
    $oidProbe = Invoke-Git -GitArgs @('-C', $repositoryRoot, 'rev-parse', ":$($file.path)")
    if ($oidProbe.exitCode -ne 0) {
        $dirty += "$($file.path) [its index entry could not be resolved]"
        continue
    }
    $blobOid = ($oidProbe.lines -join '').Trim()
    $sizeProbe = Invoke-Git -GitArgs @('-C', $repositoryRoot, 'cat-file', '-s', $blobOid)
    if ($sizeProbe.exitCode -ne 0) {
        $dirty += "$($file.path) [its index blob could not be sized]"
        continue
    }
    $blobSize = 0L
    if (-not [long]::TryParse(($sizeProbe.lines -join '').Trim(), [ref] $blobSize)) {
        $dirty += "$($file.path) [git did not answer with a blob size]"
        continue
    }
    if ($blobSize -gt $MaxScriptBytes) {
        $oversize += "$($file.path) [index blob $blobSize bytes]"
        continue
    }

    $indexBytes = Get-IndexBlobBytes -RepositoryRoot $repositoryRoot -Object $blobOid -MaxBytes $MaxScriptBytes
    if ($null -eq $indexBytes) {
        $dirty += "$($file.path) [its index blob could not be read]"
        continue
    }
    $workingLf = $Latin1.GetString($working).Replace("`r`n", "`n")
    $indexLf = $Latin1.GetString($indexBytes).Replace("`r`n", "`n")
    # -cne, NOT -ne. PowerShell's string comparisons are case-INSENSITIVE by default, so an edit
    # that changes only letter casing compares EQUAL and the file is rewritten instead of refused --
    # the promised refusal silently absent for a real edit. This is the same defect as the
    # skip-worktree predicate's `-match` matching `H`, so every comparison in this file that is
    # about CONTENT or a PATH is now case-sensitive: the class, not the site.
    if ($workingLf -cne $indexLf) {
        $dirty += "$($file.path) [uncommitted edit]"
    } else {
        # The bytes that passed the check are KEPT. Re-reading the file in the write loop below
        # would take a snapshot nothing has vetted -- an editor saving in between would have its
        # new content converted and written, and the backup comparison would then compare the
        # predecessor against those same unvetted bytes and agree with itself. The promised
        # refusal would never fire, and the edit would be rewritten rather than preserved.
        $file.vetted = $working
        $retained += $working.Length
        if ($retained -gt $MaxRetainedBytes) {
            Fail ("this scope holds more script bytes than the ${MaxRetainedBytes}-byte working " +
                "budget: the vetted contents are kept until the write phase, so a few hundred " +
                "near-limit files would exhaust memory before anything could be reported. Narrow " +
                "the run with -Path and repeat.")
        }
    }
}
if ($unsupported.Count -gt 0) {
    Fail ("these tracked scripts are UTF-16, whose CRLF this program cannot see and would silently " +
        "report as already normalised. Convert them to UTF-8 first, or exempt them, but do not " +
        "let a no-op read as a success:`n  " + ($unsupported -join "`n  "))
}
if ($notNormalised.Count -gt 0) {
    Fail ("these tracked scripts have an index blob that is not LF, so the index itself was never " +
        "renormalised after the attribute landed. Fix that first -- `git add --renormalize <path>` " +
        "and commit -- because a working-tree sweep cannot make an index agree with the rule:`n  " +
        ($notNormalised -join "`n  "))
}
if ($oversize.Count -gt 0) {
    Fail ("these tracked scripts are larger than the $MaxScriptBytes-byte bound this program reads " +
        "within. Refusing rather than reading them: a migration that runs out of memory partway " +
        "through is the one failure mode worse than not running at all.`n  " + ($oversize -join "`n  "))
}
if ($dirty.Count -gt 0) {
    Fail ("these tracked scripts differ from the index by more than their line endings. Migrating " +
        "them would destroy the edit and leave ``git status`` clean, so nothing would record the " +
        "loss (#676 mode 4). Commit or stash them first:`n  " + ($dirty -join "`n  "))
}

# The work. Nothing above this line has touched a file.
$normalized = @()
$alreadyLf = @()
$notCovered = @()
foreach ($file in $files) {
    # git's own answer about the attribute, not this script's pattern list. A file the rule does
    # not cover is left alone even if it matched a pattern here.
    if ($file.attribute -cnotmatch 'eol=lf') { $notCovered += $file.path; continue }
    # NOT `$file.worktree -ceq 'lf'`. That column was read during enumeration; by the time this
    # loop runs a checkout filter or an editor may have written CRLF bytes underneath it. The
    # decision is made from the bytes that were actually vetted, a few lines below, where the
    # conversion is a no-op for a file that is already LF.

    # GetFullPath, because `git rev-parse --show-toplevel` answers with forward slashes and
    # Join-Path then produces a mixed-separator path. .NET's Move tolerates that; Replace
    # refuses it outright with "The path is not of a legal form" -- measured, on the first
    # fixture run after this block changed.
    $full = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot $file.path))
    $bytes = $file.vetted
    if ($null -eq $bytes) { continue }
    $text = $Latin1.GetString($bytes)
    $converted = $text.Replace("`r`n", "`n")
    if ($converted -ceq $text) { $alreadyLf += $file.path; continue }

    $normalized += $file.path
}

# Always 'would': this program does not write, and a past tense would say it had.
$verb = 'would normalise'
Write-Host ''
Write-Host "[eol] $verb $($normalized.Count) file(s); $($alreadyLf.Count) already LF; $($notCovered.Count) not covered by the rule." -ForegroundColor Cyan
foreach ($p in $normalized) { Write-Host "  $verb  $p" }
foreach ($p in $notCovered) { Write-Host "  skipped (rule does not cover it)  $p" -ForegroundColor Yellow }

# THE RESIDUE IS AN OUTPUT LINE, NOT SILENCE.
#
# This program's whole product is a classification, so the failure to fear is not a wrong verdict
# -- it is a file that got NO verdict and was therefore never mentioned. "Nothing to do" where the
# recipe would have destroyed something is worse than having no tool at all, because it tells the
# operator everything is fine.
#
# So every enumerated path the rule covers must land in exactly one bucket, and anything left over
# is REPORTED BY NAME and makes the run non-zero. This survives any future narrowing of the
# program: whatever the buckets become, the residue is still the thing nobody looked at.
$classified = @($normalized + $alreadyLf + $notCovered +
    ($dirty + $oversize + $notNormalised + $unsupported | ForEach-Object { ($_ -split ' \[')[0] }))
$unclassified = @($files | Where-Object { $classified -cnotcontains $_.path } | ForEach-Object { $_.path })
if ($unclassified.Count -gt 0) {
    Write-Host ''
    Write-Host ("[eol] UNCLASSIFIED -- these are covered by the rule and this run reached no verdict " +
        "on them, which is the one outcome a diagnosis must never report as quiet:`n  " +
        ($unclassified -join "`n  ")) -ForegroundColor Red
    exit 1
}

# $covered, not $files: an extension-matching file the attribute exempts is not a covered path,
# and counting it here overstated what this run actually judged.
Write-Host "[eol] every covered path has a verdict: $($covered.Count) classified, none left over." -ForegroundColor Green
exit 0
