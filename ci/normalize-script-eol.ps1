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

    IT VERIFIES ITSELF AFTERWARDS, and that is the last of the five: `git status` was clean before
    the run and is clean after, so it cannot answer whether anything landed. The check is git's own
    `--eol` columns, re-read after the work, over every path this run rewrote AND every path it
    classified as already-LF -- and each of those must be OBSERVED in the re-read, not merely
    un-contradicted by it.

    It does not touch the index, run `checkout`, stage anything, or look at untracked files.

.PARAMETER DryRun
    Reports what would change and changes nothing. Without it the program writes, after every
    refusal above has passed.

    The exit codes: 0 when every covered path got a verdict and, in write mode, the verification
    re-read confirmed it; 1 for a refusal or an unclassified residue; and 2 when the instrument
    itself could not be trusted -- which now includes a verification re-read that did not report a
    path this run rewrote.

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
    # STDERR IS DRAINED CONCURRENTLY, NOT AFTERWARDS, and this line used to sit below the
    # stdout loop. Both streams are redirected, so the child blocks writing once its stderr
    # pipe buffer fills while this side is still blocked reading stdout, and neither ever
    # moves. A hang HAS NO COLOUR: cargo has no per-test timeout and neither does a gate
    # stage, so the run stalls rather than reddening, and a stalled run gets a gate switched
    # off rather than fixed.
    #
    # MEASURED, because the comment that used to be here named the wrong trigger. `GIT_TRACE=1`
    # over this repository is about 200 bytes -- nowhere near the ~4 KB buffer, so the trigger
    # this program warned about could not actually reach it. `GIT_TRACE2_EVENT=1` on the exact
    # command below is 3,585 bytes on this repository, 87% of one buffer, and it is an
    # environment variable a developer exports once and every git child inherits. A slightly
    # larger checkout crosses it.
    #
    # `ReadToEndAsync` started BEFORE the read below is what keeps the child draining while
    # this side works. (Found by the GraphHelm ISSUES 4 lane reviewing #765.)
    $drainStderr = $process.StandardError.ReadToEndAsync()
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
        [void] $drainStderr.Wait()
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

function Test-SameText {
    <#
        Two strings equal ORDINALLY, which is not what `-eq` and `-ceq` answer.

        PowerShell's comparison operators go through the CURRENT CULTURE, and a culture comparison
        gives some code points no weight at all. Measured on 5.1.26100.9168:

            ('GREEN' + [char]0xFE00) -eq  'GREEN'   -> True
            ('GREEN' + [char]0xFE00) -ceq 'GREEN'   -> True
            ([char]0xFE00) -eq ''                   -> True   (its Length is 1)
            [string]::Equals('GREEN' + [char]0xFE00, 'GREEN', 'Ordinal')  -> False

        **`-ceq` IS NOT THE STRICT SPELLING.** Case-sensitivity and culture-awareness are
        orthogonal, and this program reached for the `-c` family throughout on the belief that it
        was the careful one. It was not; the belief came from prose (#753, #759) that enumerates
        only the five case-INSENSITIVE operators, and a guard whose blind spot is inherited from
        its own documentation is worse than an unguarded one, because the author believes they are
        covered.

        Ordinal keeps the case-sensitivity every `-ceq` here intended and removes only the culture.

        Found by running #759's `ci/find-culture-comparisons.ps1` -- an AST detector, so it sees
        the operator and not a string that names one -- against this file. It cannot see
        `Sort-Object -Unique -CaseSensitive`, which is asked separately and measured to
        distinguish a zero-weight pair (2, not 1), so that one is left alone.
    #>
    param([Parameter(Mandatory)] [AllowNull()] [AllowEmptyString()] $Left,
          [Parameter(Mandatory)] [AllowNull()] [AllowEmptyString()] $Right)

    if ($null -eq $Left -or $null -eq $Right) { return ($null -eq $Left -and $null -eq $Right) }
    return [string]::Equals([string]$Left, [string]$Right, [System.StringComparison]::Ordinal)
}

function Test-InSet {
    <#
        Membership decided by `Test-SameText`, replacing `-ccontains` / `-cnotcontains`.

        The two sites this matters most for are guards that FAIL OPEN: the UNCLASSIFIED residue
        check, whose whole purpose is that no covered path goes unmentioned, and (in the write
        half) the verifier's vacuity control, which runs after the program has already written.
        A membership test that says "already there" about a path that is not there turns both of
        them into the quiet success they exist to prevent.
    #>
    param([Parameter(Mandatory)] [AllowEmptyCollection()] [AllowNull()] $Set,
          [Parameter(Mandatory)] [AllowNull()] [AllowEmptyString()] $Value)

    foreach ($member in @($Set)) { if (Test-SameText $member $Value) { return $true } }
    return $false
}

function Resolve-ScopePath {
    <#
        The operator's `-Path`, reduced to the repository-relative form every consumer of it needs.

        git normalises a pathspec itself -- `:(literal)./ci` and `:(literal)ci/../ci` both list
        exactly what `:(literal)ci` lists, measured on git 2.47.1.windows.1 -- and
        `Test-ExactDirectory` does not: it matches each segment against the real directory entries,
        so a `.` segment sent it looking for a directory literally named `.`, which exists on no
        filesystem. The two halves of the same run therefore disagreed about what `-Path` meant,
        and the disagreement is not symmetric: the disk half answering "absent" is what drops the
        run into the case-insensitive fallback, widening a scope the operator narrowed correctly.

        A seam that takes a STRING and returns one, for the same reason `Get-DistinctScopeSpellings`
        does. These are verdicts about a path's SHAPE, and a cell that had to build each shape on
        disk could only be run on a machine whose filesystem allowed it.

        `..` pops, and a `..` with nothing left to pop is REFUSED BY NAME. Today such a path reaches
        git, which errors with "is outside repository", and the probe reads that non-zero exit as
        HARNESS-BROKE -- the instrument reporting that it cannot be trusted, when in fact it worked
        and the answer was no. An operator's mistake and a broken tool are different states and the
        exit codes say so: 1 for a refusal, 2 for an instrument that did not answer.

        The separators are `/` and the platform's own, which is what the `-replace` this grew out of
        compared. A backslash is a legal character in a filename on Linux and is not treated as a
        separator there.
    #>
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $RelativePath)

    $separators = '[/' + [regex]::Escape([System.IO.Path]::DirectorySeparatorChar) + ']'
    $segments = New-Object System.Collections.Generic.List[string]
    foreach ($segment in ($RelativePath -split $separators)) {
        if ((Test-SameText $segment '') -or (Test-SameText $segment '.')) { continue }
        if (Test-SameText $segment '..') {
            if ($segments.Count -eq 0) {
                return [ordered]@{ ok = $false; path = ''; reason = 'climbs above the repository root' }
            }
            $segments.RemoveAt($segments.Count - 1)
            continue
        }
        $segments.Add($segment)
    }
    # An EMPTY result is the repository root -- `-Path .`, `-Path a/..`. It is not an error and it is
    # not an empty scope: it is the whole repository, which is what the operator asked for. The
    # caller says so out loud rather than widening in silence.
    return [ordered]@{ ok = $true; path = ($segments -join '/'); reason = '' }
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
    foreach ($segment in @($RelativePath -split '/' | Where-Object { -not (Test-SameText $_ '') })) {
        $match = @(Get-ChildItem -LiteralPath $current -Force -ErrorAction SilentlyContinue |
                Where-Object { Test-SameText $_.Name $segment })
        if ($match.Count -ne 1) { return $false }
        $current = $match[0].FullName
    }
    return $true
}

function Find-ReparsePointSegment {
    <#
        The first segment of this repository-relative path whose entry on disk is a REPARSE POINT
        -- a junction, a directory symlink, a mount point -- or $null when none of them is.

        This program's whole contract is "what does the repository say about these paths", and a
        path whose parent has been replaced is no longer answering for the repository.
        `Test-Path`, `File::Exists` and `File::Open` all TRAVERSE a reparse point without saying
        so, so the program read through it and reported on whatever was on the other side: a file
        outside the checkout named as one of its scripts, or a real script masked by one that is
        not there. That is "nothing to do where something is wrong" arriving by a different route,
        which is the failure this program was cut down to avoid (#698).

        A prefix comparison against the repository root is NOT enough and is not what this does: a
        junction INSIDE the root resolves to a path under the root and passes such a check while
        still pointing somewhere the index never named. Every segment is asked about its own
        attributes instead, from the root down.

        The LAST segment is walked like the others rather than special-cased. A tracked path
        replaced by a link is the same defect one level in, and giving it its own branch would give
        it a branch no cell here reaches: `New-Item -ItemType Junction` builds a directory link
        without administrator rights, a file symlink does not.

        An entry whose attributes cannot be read is NOT reported as a reparse point. Answering here
        would hand the operator the wrong reason: the read below already has named refusals for a
        path that is missing or cannot be opened, and each of them says something true that this
        one would not.

        The cache is keyed by the repository-relative prefix, so the directories shared by a
        thousand scripts are asked about once. It is passed in rather than held here because a
        function that remembers between calls is one whose verdict depends on call order.
    #>
    param(
        [Parameter(Mandatory)] [string] $Root,
        [Parameter(Mandatory)] [string] $RelativePath,
        [hashtable] $Cache
    )

    $walked = ''
    $current = $Root
    # ORDINAL, like every other comparison in this file (#753). A segment that is a lone
    # zero-weight code point compares -eq to the empty string, so the culture form DROPPED it --
    # and a dropped segment is a segment this guard never asks about, which is the reparse-point
    # check failing OPEN on exactly the path shape an attacker would choose.
    foreach ($segment in @($RelativePath -split '/' | Where-Object { -not (Test-SameText $_ '') })) {
        $walked = if (Test-SameText $walked '') { $segment } else { $walked + '/' + $segment }
        $current = Join-Path $current $segment
        if ($null -ne $Cache -and $Cache.ContainsKey($walked)) {
            if ($Cache[$walked]) { return $walked }
            continue
        }
        $isReparsePoint = $false
        try {
            $attributes = [System.IO.File]::GetAttributes($current)
            $isReparsePoint = (($attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)
        } catch {
            $isReparsePoint = $false
        }
        if ($null -ne $Cache) { $Cache[$walked] = $isReparsePoint }
        if ($isReparsePoint) { return $walked }
    }
    return $null
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
        A `-z` git listing, split into NUL-separated RECORDS out of the child's own stdout as the
        characters arrive, filtered by a predicate the caller supplies, and bounded by what this
        program KEEPS.

        Two things were wrong with the `& git` form this replaces, and the first hid the second.

        `git ls-files -z` writes no newlines, and PowerShell splits a native command's output on
        NEWLINES -- so the whole listing arrives as ONE element. Measured in this repository:
        `& git ls-files -z -- 'ci/*'` returns 1 element of 559 characters containing NULs, not 21
        elements. A ceiling checked "as each line arrives" is therefore checked exactly once, after
        the pipeline has already built the entire listing in memory. The bound was real for a
        newline-delimited listing and vacuous for all three callers here, every one of which
        passes -z.

        And it counted what GIT PRINTED. The extension filter ran afterwards, over records the
        caller had already split out, so a scope holding a million short NON-script paths stayed
        under the character ceiling while materialising every one of them; the count ceiling that
        would have caught it is applied to the scripts, downstream of this. A bound on the wrong
        population is a bound on the wrong question (#699).

        So the ceiling counts the characters KEPT, and what git prints is walked in fixed-size
        chunks and dropped. On overflow the rest of the output is still drained -- in the same
        bounded chunks -- rather than abandoned, because a child blocked on a full pipe is a hang,
        not a refusal.

        THE PREDICATE IS THE ONLY REASON A RECORD IS KEPT, so a caller whose refusals depend on
        seeing a MALFORMED record has to keep those. The enumeration below does exactly that: "could
        not parse a `git ls-files --eol` record" is a refusal, and a record dropped quietly here
        would retire it without anything going red.

        `Arguments` and not `ArgumentList`: the latter arrived in .NET 5 and this runs under Windows
        PowerShell 5.1, so the arguments are quoted explicitly, the same way `Get-IndexBlobBytes`
        does and for the same reasons recorded there.
    #>
    param(
        [Parameter(Mandatory)] [string[]] $GitArgs,
        [scriptblock] $KeepRecord
    )

    # No predicate means every record is kept, which is what the two -Path probes want: they decide
    # over the SPELLINGS of directories, and filtering those by script extension would change which
    # scopes count as ambiguous.
    if ($null -eq $KeepRecord) { $KeepRecord = { param($Record) $true } }

    $quoted = @($GitArgs) | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = 'git'
    $info.Arguments = ($quoted -join ' ')
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    # Redirected and drained, NOT merged into the stream this parses: under GIT_TRACE=1 git writes
    # trace records on stderr while still exiting 0, and a developer with tracing on would have
    # trace lines arriving as paths. `Invoke-Git` documents the same trap.
    $info.RedirectStandardError = $true
    $info.CreateNoWindow = $true
    # git writes path bytes as UTF-8. Named on the child rather than left to the console's code
    # page, because this reads the stream directly and never passes through the console encoding
    # the rest of the program sets.
    $info.StandardOutputEncoding = New-Object System.Text.UTF8Encoding($false)

    $process = [System.Diagnostics.Process]::Start($info)
    # STDERR IS DRAINED CONCURRENTLY, NOT AFTERWARDS, and this line used to sit below the
    # stdout loop. Both streams are redirected, so the child blocks writing once its stderr
    # pipe buffer fills while this side is still blocked reading stdout, and neither ever
    # moves. A hang HAS NO COLOUR: cargo has no per-test timeout and neither does a gate
    # stage, so the run stalls rather than reddening, and a stalled run gets a gate switched
    # off rather than fixed.
    #
    # MEASURED, because the comment that used to be here named the wrong trigger. `GIT_TRACE=1`
    # over this repository is about 200 bytes -- nowhere near the ~4 KB buffer, so the trigger
    # this program warned about could not actually reach it. `GIT_TRACE2_EVENT=1` on the exact
    # command below is 3,585 bytes on this repository, 87% of one buffer, and it is an
    # environment variable a developer exports once and every git child inherits. A slightly
    # larger checkout crosses it.
    #
    # `ReadToEndAsync` started BEFORE the read below is what keeps the child draining while
    # this side works. (Found by the GraphHelm ISSUES 4 lane reviewing #765.)
    $drainStderr = $process.StandardError.ReadToEndAsync()
    $kept = New-Object System.Collections.Generic.List[string]
    $pending = New-Object System.Text.StringBuilder
    $chars = 0L
    $overflowed = $false
    try {
        $chunk = New-Object char[] 65536
        while ($true) {
            $read = $process.StandardOutput.Read($chunk, 0, $chunk.Length)
            if ($read -le 0) { break }
            if ($overflowed) { continue }
            $text = New-Object string($chunk, 0, $read)
            $start = 0
            while ($true) {
                $nul = $text.IndexOf([char]0, $start)
                if ($nul -lt 0) {
                    [void] $pending.Append($text.Substring($start))
                    # A record with no terminator in sight is held, so it carries its own bound:
                    # without one, a listing that never emits a NUL is unbounded in exactly the
                    # buffer this function exists to bound.
                    if ($pending.Length -gt $MaxEnumerationChars) {
                        $overflowed = $true
                        [void] $pending.Clear()
                    }
                    break
                }
                [void] $pending.Append($text.Substring($start, $nul - $start))
                $record = $pending.ToString()
                [void] $pending.Clear()
                $start = $nul + 1
                if (Test-SameText $record '') { continue }
                if (-not (& $KeepRecord $record)) { continue }
                $chars += $record.Length
                if ($chars -gt $MaxEnumerationChars) {
                    $overflowed = $true
                    $kept.Clear()
                    break
                }
                $kept.Add($record)
            }
        }
        # Whatever is left with no terminator after it. Every `-z` record ends in NUL, so this is
        # empty for the callers here; it is what keeps the function correct for output that is not
        # NUL-terminated rather than silently losing the last record.
        if (-not $overflowed -and $pending.Length -gt 0) {
            $record = $pending.ToString()
            if ((& $KeepRecord $record) -and (($chars + $record.Length) -le $MaxEnumerationChars)) {
                $kept.Add($record)
            }
        }
        [void] $drainStderr.Wait()
        $process.WaitForExit()
        if ($overflowed) {
            return [ordered]@{ exitCode = 0; records = @(); overflowed = $true }
        }
        return [ordered]@{ exitCode = $process.ExitCode; records = @($kept); overflowed = $false }
    } finally {
        $pending = $null
        $process.Dispose()
    }
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
# Shared by the -Path guard and the per-file guard, so the directories a thousand scripts have
# in common are asked about once. Declared here rather than inside either, because a cache a
# function keeps for itself makes its verdict depend on how many times it has been called.
$reparseCache = @{}

# THE WRITE PATH IS IN THIS PROGRAM NOW (#693), and every refusal above it still runs first.
#
# #677 shipped the diagnosis alone and refused here, so that the danger #676 measures -- all of
# which lives in the WRITING -- would get its own review rather than arriving as the second half of
# a change a reviewer had already read past. This is that review. Nothing in the diagnosis moved:
# the enumeration, the bounds, the four refusals and the reparse-point guard decide exactly as they
# did, and the write happens only after all of them have passed.
#
# `-DryRun` stays the way to ask what WOULD change, and it is still the only mode that touches
# nothing.

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
# The normalised scope, decided ONCE and consumed by all three of the things that used to derive it
# separately: the two probes, the enumeration pathspec, and the index-flag pathspec below. That last
# one re-derived it from the raw `-Path`, which was the same string only for as long as normalising
# did nothing; with `.` and `..` folded here, a second derivation would scan a scope different from
# the one being swept, and the skip-worktree refusal is exactly the guard that must not be looking
# somewhere else.
$normalisedPath = ''
if ($Path) {
    $resolvedScope = Resolve-ScopePath -RelativePath $Path
    if (-not $resolvedScope.ok) {
        Fail "-Path '$Path' $($resolvedScope.reason). Name a directory inside the repository."
    }
    $normalisedPath = $resolvedScope.path
    if (Test-SameText $normalisedPath '') {
        Write-Host "[eol] -Path '$Path' names the repository root; sweeping the whole repository." -ForegroundColor Cyan
    }
}
if ($normalisedPath) {
    # A junction in the SCOPE redirects the whole sweep, so it is refused before the probes rather
    # than once per file: `-Path lib` where `lib` is a link enumerates the index under `lib` and
    # then reads every one of those paths through the link. Refusing here means no probe, no
    # enumeration and no read happens against a scope this program cannot answer for.
    $taintedScope = Find-ReparsePointSegment -Root $repositoryRoot -RelativePath $normalisedPath -Cache $reparseCache
    if ($null -ne $taintedScope) {
        Fail ("-Path '$Path' is reached through '$taintedScope', which is a reparse point (a " +
            "junction, symlink or mount point). What is on the other side of it is not what the " +
            "index names, so this program cannot answer for it. Name a path inside the checkout.")
    }
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
    if ($exact.records.Count -eq 0) {
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
        $depth = @($normalisedPath -split '/' | Where-Object { -not (Test-SameText $_ '') }).Count
        $spellings = Get-DistinctScopeSpellings -Paths @($fallback.records) -Depth $depth
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
$extensions = @('.sh', '.ps1', '.py')
# The extension filter, handed to the reader instead of run over what the reader collected. It
# lives outside the pathspec because `-Path` is literal above, and it now lives outside the parse
# loop because a scope of a million short NON-script paths passed the character ceiling while
# materialising every record, and the count ceiling that would have caught it is applied to the
# scripts, one level further in (#699).
#
# A record with NO TAB is kept, deliberately. `could not parse a git ls-files --eol record` is a
# REFUSAL, and a predicate that dropped the unparsable record would retire that refusal with
# nothing going red -- the program would sweep on, quietly, over a listing it could not read.
#
# Case-INSENSITIVE, and unlike every other comparison in this file: an extension is a filesystem
# fact, not content, and `A.SH` names the same kind of file as `a.sh`.
$KeepScriptRecord = {
    param($Record)
    $tab = $Record.IndexOf("`t")
    if ($tab -lt 0) { return $true }
    $candidate = $Record.Substring($tab + 1).ToLowerInvariant()
    foreach ($extension in $extensions) {
        if ($candidate.EndsWith($extension)) { return $true }
    }
    return $false
}
$enumeration = Read-BoundedGit -GitArgs $eolArgs -KeepRecord $KeepScriptRecord
if ($enumeration.overflowed) {
    Write-Host ("[eol] REFUSED: the enumeration for this scope keeps more than $MaxEnumerationChars " +
        "characters of tracked scripts, which is more than this program will hold before it has " +
        "decided anything. Narrow the run with -Path.") -ForegroundColor Red
    exit 1
}
if ($enumeration.exitCode -ne 0) {
    Write-Host '[eol] HARNESS-BROKE: git ls-files --eol failed.' -ForegroundColor Magenta
    exit 2
}

# A count bound as well as a byte bound: the per-path records, the parsed entries and the flag
# listing are all proportional to how many paths matched, and thousands of EMPTY scripts weigh
# nothing against the retained-byte budget while still multiplying that state. A ceiling that only
# counts bytes is not a ceiling on a sweep.
$records = @($enumeration.records)
if ($records.Count -eq 0) {
    Write-Host '[eol] no tracked .sh/.ps1/.py files in scope; nothing to do.' -ForegroundColor Cyan
    exit 0
}

$files = @()
foreach ($record in $records) {
    $tab = $record.IndexOf("`t")
    if ($tab -lt 0) {
        Fail "could not parse a `git ls-files --eol` record: [$record]. Refusing rather than guessing which part is the path." 2
    }
    $columns = $record.Substring(0, $tab)
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
if ($normalisedPath) {
    $flagPathspecs += $pathSpecPrefix + $normalisedPath
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
$flagOutput = @(($flagRun.lines -join '') -split "`0" | Where-Object { -not (Test-SameText $_ '') })
# The covered set, computed once and used by BOTH loops. A script the attribute does not cover is
# not this program's business anywhere: flagging a `-text` file as skip-worktree refused a run over
# a file that would never have been touched, and the same path missing from the working tree was
# reported as an uncommitted edit. Two symptoms, one ordering defect -- the exemption has to be
# read before any state is judged.
$covered = @($files | Where-Object { $_.attribute -cmatch 'eol=lf' } | ForEach-Object { $_.path })
foreach ($line in @($flagOutput)) {
    if ($line -cmatch '^\S+\s+(.+)$' -and -not (Test-InSet $covered $Matches[1])) { continue }
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
$redirected = @()
$oversize = @()
$notNormalised = @()
$unsupported = @()
$retained = 0L
foreach ($file in $files) {
    # BEFORE anything is judged about it, including whether it exists.
    if (-not (Test-InSet $covered $file.path)) { continue }

    # BEFORE Test-Path, because Test-Path is one of the three calls that traverse a reparse point
    # without saying so. Asked per file and not only for the scope: the enumeration comes from the
    # INDEX, so a path whose parent was replaced after the checkout is listed exactly like any
    # other and there is no earlier moment at which it looks different.
    $tainted = Find-ReparsePointSegment -Root $repositoryRoot -RelativePath $file.path -Cache $reparseCache
    if ($null -ne $tainted) {
        $redirected += "$($file.path) [reached through the reparse point '$tainted']"
        continue
    }
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
    if (Test-SameText $file.indexEol '-text') {
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
    if (-not (Test-SameText $file.indexEol 'lf') -and -not (Test-SameText $file.indexEol 'none')) {
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
    if (-not (Test-SameText $workingLf $indexLf)) {
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
# FIRST of the refusals, because it is the only one that says the program was not looking at the
# repository at all. The others describe a file this run really did read; this one describes bytes
# that came from somewhere the index never named, and reporting any verdict about those -- even a
# refusal in another bucket's words -- would be a claim about the wrong file.
if ($redirected.Count -gt 0) {
    Fail ("these tracked scripts are reached through a reparse point (a junction, symlink or mount " +
        "point), so what is on the other side is not what the index names and this program cannot " +
        "answer for them. Restore the real directory, or narrow the run with -Path:`n  " +
        ($redirected -join "`n  "))
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
    if (Test-SameText $converted $text) { $alreadyLf += $file.path; continue }

    if (-not $DryRun) {
        # Write beside the target, then REPLACE it. Three review findings turned out to be one
        # defect in the first version -- write to a predictable `.eol-migration.tmp`, delete the
        # original, move -- and one primitive answers all three:
        #
        #   a predictable staging name silently TRUNCATES a file that already has it, and that
        #   file is legal in the repository and does not end in .sh, so no check above sees it;
        #   the explicit Delete opens a window where the path does not exist, which is the state
        #   this program promises never to reach -- an ACL, an antivirus or a race between the two
        #   calls leaves exactly the mode-5 outcome by a different route;
        #   and a Move puts the STAGING file's metadata on the path, discarding the original's
        #   ACLs, DOS attributes and alternate data streams. A byte-clean migration that silently
        #   drops an operator's permissions is not clean.
        #
        # `File.Replace` transfers the source's CONTENT onto the destination while the destination
        # keeps its own identity and metadata, and there is no moment with the path absent. The
        # staging file is created with CreateNew, which THROWS rather than truncating if the name
        # is taken, and its name carries a GUID so the collision is not predictable in the first
        # place.
        $staging = "$full.$([guid]::NewGuid().ToString('N')).eol-tmp"
        $bytesOut = $Latin1.GetBytes($converted)
        # The staging file is created and written inside a try whose CLEANUP covers the write, not
        # only the stream. A full volume or a quota makes the write throw after CreateNew has
        # already made the file, and 'Stop' then ends the run -- leaving a partial GUID-named
        # sidecar in the operator's checkout that nothing would ever explain. The flag is cleared
        # only once the replacement has succeeded.
        $stagingReplaced = $false
        try {
            $stream = [System.IO.File]::Open($staging, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
            try { $stream.Write($bytesOut, 0, $bytesOut.Length) } finally { $stream.Dispose() }

            # A BACKUP PATH, not a null one. Between the read that vetted these bytes and this call
            # there is a window in which an editor can save the file, and the dirty check above
            # cannot see an edit that had not happened when it ran. `File.Replace` hands the
            # destination's exact predecessor to the backup path as PART OF THE SAME OPERATION, so
            # the bytes about to be overwritten are captured rather than lost, and can be compared
            # with the bytes that were vetted. Differ, and the operator saved during the run: the
            # backup goes back and the program refuses.
            $backup = "$full.$([guid]::NewGuid().ToString('N')).eol-backup"
            try {
                [System.IO.File]::Replace($staging, $full, $backup)
            } catch {
                # Replace failed with the original intact. Take the staging file away rather than
                # leaving a sidecar the next run would have to reason about, and refuse: a partial
                # migration that keeps going is how a sweep loses track of what it did.
                Remove-Item -LiteralPath $staging -Force -ErrorAction SilentlyContinue
                Fail "could not replace $($file.path): $($_.Exception.Message). The original is untouched."
            }
            # RAW BYTES, because this comparison is about identity rather than line endings.
            # Bounded like every other read. Another process growing the target past the ceiling
            # between the vetted read and the replace would have its oversized content captured
            # into the backup, and reading THAT unbounded defeats the guard from the one direction
            # nothing else covers.
            $backupInfo = New-Object System.IO.FileInfo($backup)
            if ($backupInfo.Length -gt $MaxScriptBytes) {
                Fail ("$($file.path) grew past the $MaxScriptBytes-byte bound while it was being " +
                    "replaced. Its previous content is at:`n  $backup`nNothing further was read or " +
                    "written; move that file back by hand once you know which version you want.")
            }
            $predecessor = [System.IO.File]::ReadAllBytes($backup)
            # INTEGERS, so the culture never enters: `-eq` on two `[int]` is numeric, and
            # #759's detector lists it because it inspects the OPERATOR and not the operand types.
            # Left as it is, and said so, because a non-zero detector count on this file otherwise
            # reads as unfinished work and invites someone to "fix" a comparison that is correct.
            $unchanged = $predecessor.Length -eq $bytes.Length
            if ($unchanged) {
                for ($i = 0; $i -lt $bytes.Length; $i++) {
                    if ($predecessor[$i] -ne $bytes[$i]) { $unchanged = $false; break }
                }
            }
            if (-not $unchanged) {
                # The rollback needs a backup of its own. Whatever is on disk at THIS instant may
                # be a second save that arrived while the first was being detected, and a null
                # backup here would discard it permanently -- the rollback destroying an edit is
                # the same defect as the write destroying one, one step later. So the current
                # bytes are captured to a named file, the predecessor goes back, and the refusal
                # TELLS THE OPERATOR WHERE THEIR VERSION IS. Nothing this program writes is ever
                # the last copy of anything.
                $rescued = "$full.$([guid]::NewGuid().ToString('N')).eol-rescued"
                [System.IO.File]::Replace($backup, $full, $rescued)
                $already = if ($normalized.Count -gt 0) {
                    "`n$($normalized.Count) file(s) were already rewritten before this happened, and " +
                    "they are NOT rolled back:`n  " + ($normalized -join "`n  ")
                } else {
                    ''
                }
                Fail ("$($file.path) changed on disk between the check and the write, so the bytes this " +
                    "run vetted are not the bytes it was about to overwrite. The file has been put back " +
                    "as this run found it, and whatever was on disk at the moment of the rollback was " +
                    "saved to:`n  $rescued`nCompare the two, keep the one you want, delete the other, " +
                    "and re-run." + $already)
            }
            # NOT SilentlyContinue. A predecessor copy left inside the checkout is untracked content
            # this program created and then failed to clean up; exiting 0 with a "verified" summary
            # while it sits there makes the summary false. Antivirus and open handles are exactly
            # the conditions that cause it, so it is reported by name.
            try {
                [System.IO.File]::Delete($backup)
            } catch {
                Fail ("$($file.path) was normalised, but its predecessor copy could not be removed: " +
                    "$($_.Exception.Message)`n  $backup`nDelete it by hand once nothing is holding it. " +
                    "The file itself is correct; the checkout is not clean.")
            }
            $stagingReplaced = $true
        } finally {
            if (-not $stagingReplaced) { Remove-Item -LiteralPath $staging -Force -ErrorAction SilentlyContinue }
        }
    }
    $normalized += $file.path
}

# The tense is the RESULT, not the intent. `would normalise` under -DryRun and `normalised` after a
# write: a run that says the past tense has already done it, and the verifier below is what earns
# the claim.
$verb = if ($DryRun) { 'would normalise' } else { 'normalised' }
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
# $redirected is deliberately absent: a non-empty one has already exited above, so listing it here
# would be a line that can never run. The residue check is about buckets a path can land in and
# still reach this point.
$classified = @($normalized + $alreadyLf + $notCovered +
    ($dirty + $oversize + $notNormalised + $unsupported | ForEach-Object { ($_ -split ' \[')[0] }))
$unclassified = @($files | Where-Object { -not (Test-InSet $classified $_.path) } | ForEach-Object { $_.path })
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

# Files classified as ALREADY LF are verified too, and that is why this runs even when nothing was
# converted. The classification comes from bytes read earlier; a checkout filter or an editor
# writing CRLF after that read would have the file skipped on a stale snapshot, and a run that
# converted nothing else would then skip the verifier entirely and report success twice over.
$mustReadLf = @($normalized + $alreadyLf)
if ($DryRun -or $mustReadLf.Count -eq 0) { exit 0 }

# The instrument, run again AFTER the work. `git status` cannot answer this question -- it was
# clean before and is clean after -- so the check is git's own eol columns, which is also what a
# reviewer should ask for rather than a clean status (#676).
#
# Through `Read-BoundedGit` with the SAME predicate the enumeration used, not through the
# unbounded `Invoke-Git` the first version of this block reached for: a verifier that re-reads the
# whole listing unbounded is the one place a bound would be most embarrassing to be missing, since
# it runs after the program has already written to the operator's checkout (#699).
$after = Read-BoundedGit -GitArgs $eolArgs -KeepRecord $KeepScriptRecord
if ($after.overflowed -or $after.exitCode -ne 0) {
    Write-Host '[eol] HARNESS-BROKE: the verification re-run of git ls-files did not answer, so nothing here observed the result.' -ForegroundColor Magenta
    exit 2
}
$stillCrlf = @()
$seen = @()
foreach ($record in @($after.records)) {
    $tab = $record.IndexOf("`t")
    if ($tab -lt 0) { continue }
    $path = $record.Substring($tab + 1)
    if (Test-InSet $mustReadLf $path) {
        $seen += $path
        # `w/none` as well as `w/lf`, the same reading as the index test above: a file with no
        # line endings has none to be wrong. Requiring `w/lf` made the verifier declare
        # HARNESS-BROKE over an empty script -- the strictest possible complaint about a file
        # that is exactly as it should be.
        if ($record.Substring(0, $tab) -cnotmatch 'w/(lf|none)') { $stillCrlf += $path }
    }
}
# The verifier's own vacuity control. Every check above quantifies over what the re-run RETURNED,
# and all of them are trivially satisfied by an empty answer -- so a verifier that saw nothing
# would print "verified" in the loudest possible way. Each normalised path must be OBSERVED, not
# merely not-contradicted.
$unobserved = @($mustReadLf | Where-Object { -not (Test-InSet $seen $_) })
if ($unobserved.Count -gt 0) {
    Write-Host "[eol] HARNESS-BROKE: these were rewritten and the verification re-run did not report them at all, so nothing confirmed the result:`n  $($unobserved -join "`n  ")" -ForegroundColor Magenta
    exit 2
}
if ($stillCrlf.Count -gt 0) {
    Write-Host "[eol] HARNESS-BROKE: these must read as LF and git says otherwise -- either a write did not land, or a file classified as already-LF changed underneath this run:`n  $($stillCrlf -join "`n  ")" -ForegroundColor Magenta
    exit 2
}

Write-Host '[eol] verified: every rewritten file now reads w/lf.' -ForegroundColor Green
exit 0
