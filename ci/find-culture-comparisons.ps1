<#
    #753: list the PowerShell comparisons that decide something with a CULTURE-AWARE comparer.

    `-eq`, `-ne`, `-in`, `-notin` and `-contains` are case-insensitive AND culture aware, and a
    culture comparison gives some code points no weight at all. Measured on this machine:

        ('GREEN' + [char]0xFE00) -eq 'GREEN'                     -> True
        ('green' + [char]0xFE00) -in @('green', 'UNCLASSIFIED')  -> True
        [string]::Equals('GREEN' + [char]0xFE00, 'GREEN', 'Ordinal')  -> False

    U+FE00 is a variation selector: category Mn, ordinary text. So the remedy is never to refuse the
    character -- that is a deny-list growing by one code point per review -- but to stop comparing
    approximately, with `[string]::Equals(..., Ordinal)` where case matters and
    `[string]::Equals(..., OrdinalIgnoreCase)` where it deliberately does not. THAT CHOICE IS PER
    SITE: paths on Windows are case-insensitive, and a closed vocabulary written in one case by one
    producer is not.

    WHAT THIS IS AND IS NOT.

    It is a REPORTER: it lists sites and always exits 0. It is not a gate, and it is deliberately not
    one -- deciding which of these sites is a defect needs the reader to know where the value came
    from, and a gate that answers that question by pattern would be wrong in both directions.

    It is also not a proof of ABSENCE. An empty list means this pattern found nothing in the files it
    was given: multi-line comparisons are found (the AST does not care about lines), but `switch`
    statements, `-match`, `-like` and comparisons hidden behind a variable holding an operator name
    are not looked for at all.

    ASKED OF THE AST, NOT OF THE SOURCE TEXT. A regex over source flags the operator named inside a
    throw MESSAGE -- ci/classify-run.ps1 has one -- and a sweep with a false positive is a sweep
    somebody deletes three months later.

    Comparisons against $null, $true and $false are exempt: those are identity and boolean tests
    rather than text, and listing them would bury the ones that matter.

    .EXAMPLE
        powershell -NoProfile -ExecutionPolicy Bypass -File ci/find-culture-comparisons.ps1
        powershell -NoProfile -ExecutionPolicy Bypass -File ci/find-culture-comparisons.ps1 -Path ci/gate.ps1
#>
[CmdletBinding()]
param(
    # The files to read. Default: every versioned .ps1 in the repository, listed with `git -C <root>`
    # rather than a bare `git ls-files`, because inside a subdirectory that answers about the CWD
    # PREFIX and can come back EMPTY with exit code 0 -- which reads as "no such files" and is "you
    # are somewhere else". The root is asked for first, and the listing is made from there.
    [string[]] $Path,
    # WHICH FAMILIES TO LOOK FOR. `binary` is the default and keeps this tool's headline number
    # stable: the operators. `all` adds the three families measured below, each row carrying the
    # family it came from, because they have different sizes and very different signal-to-noise --
    # a hashtable lookup by key is usually somebody's own dictionary, and drowning the operators in
    # those would be how this tool stops being read.
    [ValidateSet('binary', 'all')] [string] $Include = 'binary',
    [switch] $AsJson
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Files the parser could not read. Carried into the summary so an empty list is never bare.
$script:unreadable = @()

# Every binary operator whose string comparison goes through the current culture.
$CultureOperators = @(
    'Ieq', 'Ine', 'Ceq', 'Cne',
    'Icontains', 'Inotcontains', 'Ccontains', 'Cnotcontains',
    'Iin', 'Inotin', 'Cin', 'Cnotin'
)

# MEASURED ON THIS MACHINE, with a discriminating probe and a control, because the obvious list is
# wrong in both directions. Payload: 'GREEN' + U+FE00, a code point the culture comparer folds.
#
#   FOLDS (culture-sensitive -- these are the families this tool looks for):
#     switch ($v) { 'GREEN' {...} }        MATCHED   -- and `switch -CaseSensitive` matches too
#     $hashtable[$v] / [ordered]@{}[$v]    HIT
#     'GREEN'.StartsWith($v)               True
#     'GREEN'.EndsWith($v)                 True
#     'GREEN'.IndexOf($v)                  0         -- LastIndexOf too
#     'GREEN'.CompareTo($v)                0         -- [string]::Compare the same
#
#   DOES NOT FOLD (ordinal already -- NOT flagged, and listing them would bury the ones that matter):
#     'GREEN'.Contains($v)                 False
#     'GREEN'.Equals($v)                   False
#     'GREEN'.Replace($v, 'X')             GREEN     -- unchanged
#     $v -like 'GREEN' / $v -match '^GREEN$'         False
#
#   CONTROL, a visible difference, negative everywhere it should be:
#     'GREEN'.Contains('GREENX')  False    'GREEN'.StartsWith('GREENX')  False
#     'GREEN'.IndexOf('GREENX')   -1
#
# `.Equals` and `.Contains` being ordinal is the correction that matters: a review list I was handed
# included them, and flagging a call that is already ordinal is a false positive -- which is how a
# sweep gets deleted three months later.
$CultureMembers = @('StartsWith', 'EndsWith', 'IndexOf', 'LastIndexOf', 'CompareTo', 'Compare')

function Test-HasStringComparison {
    <# An explicit StringComparison argument settles it, whatever the method name is. #>
    param([Parameter(Mandatory)] $Node)
    foreach ($argument in @($Node.Arguments)) {
        if ($argument.Extent.Text -match 'StringComparison') { return $true }
    }
    return $false
}

function Test-ExemptOperand {
    param([Parameter(Mandatory)] $Node)
    if ($Node -is [System.Management.Automation.Language.VariableExpressionAst]) {
        return ([System.Array]::IndexOf(@('null', 'true', 'false'), $Node.VariablePath.UserPath.ToLowerInvariant()) -ge 0)
    }
    # A number or a bool literal. A STRING constant is not exempt: it is the whole point.
    return ($Node -is [System.Management.Automation.Language.ConstantExpressionAst] -and
        -not ($Node -is [System.Management.Automation.Language.StringConstantExpressionAst]))
}

function Find-CultureComparison {
    <# The one producer of the answer, so the suite and an operator read the same list. #>
    param([Parameter(Mandatory)] [string] $File, [ValidateSet('binary', 'all')] [string] $Include = 'binary')

    # A PATH THAT IS NOT THERE IS AN ERROR, NOT A ZERO. Passing two files as `-Path a,b` through
    # `powershell -File` hands this one argument named `a,b`: the parser refused it, the warning
    # scrolled past, and the summary said "0 culture-aware comparisons" -- a clean-looking answer
    # about a file that does not exist. The tool must not have the failure shape it exists to find.
    if (-not (Test-Path -LiteralPath $File -PathType Leaf)) {
        throw "no file at $File. With 'powershell -File', pass one -Path per invocation: a shell that hands over 'a,b' as one argument names a file that does not exist."
    }
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path -LiteralPath $File).Path,
        [ref] $null, [ref] $parseErrors)
    if (@($parseErrors).Count -gt 0) {
        # A partial parse is evidence for nothing: a site may be missing because it is not there, or
        # because the parser stopped before reaching it. Counted as UNREAD rather than as zero, and
        # the summary carries that count so an empty list is never bare.
        Write-Warning "$File does not parse ($(@($parseErrors)[0].Message)); its sites cannot be listed."
        $script:unreadable += $File
        return @()
    }
    $found = @($ast.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.BinaryExpressionAst] -and
                ([System.Array]::IndexOf($CultureOperators, $node.Operator.ToString()) -ge 0) -and
                -not (Test-ExemptOperand -Node $node.Left) -and
                -not (Test-ExemptOperand -Node $node.Right)
            }, $true) | ForEach-Object {
            [pscustomobject]@{
                File     = $File
                Kind     = 'binary'
                Line     = $_.Extent.StartLineNumber
                Operator = $_.Operator.ToString()
                Text     = ($_.Extent.Text -replace '\s+', ' ')
            }
        })

    if (-not [string]::Equals($Include, 'all', [System.StringComparison]::Ordinal)) { return $found }

    # A SWITCH CLAUSE IS A COMPARISON WITH NO OPERATOR IN SIGHT, and it is the idiomatic way to
    # dispatch on a closed vocabulary in PowerShell -- which is exactly what a vocabulary guard is.
    # `-CaseSensitive` does not help: measured, it matches the folded value too.
    foreach ($switchStatement in @($ast.FindAll({
                    param($node) $node -is [System.Management.Automation.Language.SwitchStatementAst]
                }, $true))) {
        foreach ($clause in $switchStatement.Clauses) {
            if ($clause.Item1 -is [System.Management.Automation.Language.StringConstantExpressionAst]) {
                $found += [pscustomobject]@{
                    File     = $File
                    Kind     = 'switch'
                    Line     = $clause.Item1.Extent.StartLineNumber
                    Operator = 'switch-clause'
                    Text     = ($clause.Item1.Extent.Text -replace '\s+', ' ')
                }
            }
        }
    }

    # A LOOKUP BY KEY IS A COMPARISON THE DICTIONARY PERFORMS. Only VARIABLE keys are listed: a
    # literal key is written by the author and cannot carry a smuggled code point, so listing those
    # would add noise with no reading behind it. A numeric index is an array subscript, not a
    # comparison at all.
    foreach ($index in @($ast.FindAll({
                    param($node) $node -is [System.Management.Automation.Language.IndexExpressionAst]
                }, $true))) {
        if ($index.Index -is [System.Management.Automation.Language.ConstantExpressionAst]) { continue }
        $found += [pscustomobject]@{
            File     = $File
            Kind     = 'index'
            Line     = $index.Extent.StartLineNumber
            Operator = 'index-by-key'
            Text     = ($index.Extent.Text -replace '\s+', ' ')
        }
    }

    foreach ($call in @($ast.FindAll({
                    param($node)
                    $node -is [System.Management.Automation.Language.InvokeMemberExpressionAst] -and
                    $node.Member -is [System.Management.Automation.Language.StringConstantExpressionAst]
                }, $true))) {
        if ([System.Array]::IndexOf($CultureMembers, $call.Member.Value) -lt 0) { continue }
        if (Test-HasStringComparison -Node $call) { continue }
        $found += [pscustomobject]@{
            File     = $File
            Kind     = 'member'
            Line     = $call.Extent.StartLineNumber
            Operator = $call.Member.Value
            Text     = ($call.Extent.Text -replace '\s+', ' ')
        }
    }

    return $found
}

# Dot-sourced by the suite: when this file is loaded rather than run, it defines the functions and
# stops. `$MyInvocation.InvocationName` is '.' exactly then.
# Ordinal, like everything this tool asks of other files. It reported this very line when run
# over itself, which is the right behaviour: an instrument that exempted itself from its own
# rule would be the first place the rule stopped holding.
# ------------------------------------------------------------------------------------------
# #835: WHICH TREE THIS NUMBER CAME FROM.
#
# A lane ran this sweep from its own session worktree and read `3 culture-aware comparison(s) over
# 3 file(s)`. From a fresh worktree of the same head: `354 over 36`. Same tool, same command, exit 0
# both times, no error either way -- the twelve-fold under-report reads exactly like a clean
# repository, and it was caught only because the total disagreed with another pass's.
#
# So the headline number now arrives with the tree it was taken from. This is NOT a refusal: the
# tool is a reporter, and pointing it at an old tree on purpose is a legitimate thing to do. What it
# must not do is leave the reader to assume otherwise, because every command succeeded.
#
# THE MAIN-CHECKOUT TEST IS `--git-dir` VS `--git-common-dir`, NEVER A FILE COUNT. In a directory
# that resolves to the main checkout, `git ls-files` is scoped to the CWD PREFIX -- it answers about
# that path, not about the repository -- so a file count there reads 0, and that zero is the very
# hazard this line exists to expose. An instrument for measuring a hazard must not be subject to it.
function Format-TreeProvenance {
    param(
        [string] $Head,
        [string] $GitDir,
        [string] $GitCommonDir,
        # $null when it could not be measured. NOT 0: "no commits behind" and "I could not ask" are
        # different facts, and collapsing them onto 0 is how the reassuring zero gets back in.
        $Behind,
        # #1006 review: `rev-list --count HEAD..origin/main` counts what origin/main has and HEAD
        # does not. A branch AHEAD of main and missing nothing answers 0 -- and the first version of
        # this function printed "level with origin/main" for it, which is FALSE and was quoted as
        # evidence in this PR's own body. "Level" is now said only when BOTH sides are zero.
        $Ahead,
        # The scan reads WORKING-COPY bytes (`git ls-files` selects tracked paths; the parser reads
        # the file on disk), so a dirty tree is not the commit named here. Two different inputs
        # under one label is the confusion this whole line exists to end.
        $Dirty,
        # Scanned files marked assume-unchanged or skip-worktree: `git status` cannot see them.
        $Hidden,
        [string] $BehindReason
    )

    $where = if ([string]::IsNullOrWhiteSpace($GitDir) -or [string]::IsNullOrWhiteSpace($GitCommonDir)) {
        'a checkout whose kind could NOT be determined'
    } elseif ([string]::Equals($GitDir.TrimEnd('/', '\'), $GitCommonDir.TrimEnd('/', '\'), [System.StringComparison]::OrdinalIgnoreCase)) {
        # Ordinal-ignore-case rather than `-eq`: paths on Windows are case-insensitive and that is a
        # per-site decision this file argues for elsewhere, so it is spelled rather than inherited.
        'the MAIN checkout'
    } else {
        'a linked worktree'
    }

    $headText = if ([string]::IsNullOrWhiteSpace($Head)) { 'an UNKNOWN head' } else { $Head }

    $behindText = if ($null -eq $Behind) {
        "distance from origin/main: UNKNOWN ($BehindReason)"
    } elseif (([int] $Behind -eq 0) -and ($null -ne $Ahead) -and ([int] $Ahead -eq 0)) {
        'level with origin/main'
    } elseif ([int] $Behind -eq 0) {
        $aheadPart = if ($null -eq $Ahead) { 'ahead: UNKNOWN' } else { "$Ahead ahead" }
        "0 commits behind origin/main, $aheadPart"
    } else {
        $aheadPart = if ($null -eq $Ahead) { '' } else { ", $Ahead ahead" }
        "$Behind commit(s) BEHIND origin/main (so anything newer is NOT in this count)$aheadPart"
    }

    $dirtyText = if ($null -eq $Dirty) {
        ', working tree: UNKNOWN'
    } elseif ([int] $Dirty -gt 0) {
        ", and $Dirty uncommitted change(s) were SCANNED rather than the commit above"
    } else {
        ''
    }

    # An index flag makes the count above unreliable rather than wrong, and saying which is the
    # point: a reader who sees "0 uncommitted" on a tree with a hidden edit is being misled by
    # silence, which is the failure mode this line exists to end.
    $hiddenText = if (($null -ne $Hidden) -and ([int] $Hidden -gt 0)) {
        ", and $Hidden scanned file(s) are hidden from `git status` by an index flag, so the count above is NOT reliable"
    } else {
        ''
    }

    return "Tree: $headText in $where, $behindText$dirtyText$hiddenText"
}

# The three git questions behind the line above, each with its own exit code read on the NEXT
# statement. `$LASTEXITCODE` is lost across a pipeline, which this file has already been bitten by
# once (see the `--show-toplevel` note below).
function Get-TreeProvenance {
    param(
        [Parameter(Mandatory)] [string] $Root,
        # The files this run actually read. `git status` covers the whole repository, so counting it
        # unscoped made a modified README read as "SCANNED" -- a false sentence in the line that
        # exists to stop false sentences (review of #1006). Empty means "ask about nothing".
        [string[]] $ScannedPaths = @()
    )

    $headOutput = @(& git -C $Root rev-parse --short HEAD 2>$null)
    $headExit = $LASTEXITCODE
    $head = ''
    if ($headExit -eq 0) {
        $first = @($headOutput | Where-Object { $_ } | Select-Object -First 1)
        if ($first) { $head = ([string]$first).Trim() }
    }

    # ONE call for both, so the two paths can never come from different moments.
    $dirsOutput = @(& git -C $Root rev-parse --path-format=absolute --git-dir --git-common-dir 2>$null)
    $dirsExit = $LASTEXITCODE
    $gitDir = ''
    $commonDir = ''
    if ($dirsExit -eq 0) {
        $lines = @($dirsOutput | Where-Object { $_ })
        if ($lines.Count -ge 2) {
            $gitDir = ([string]$lines[0]).Trim()
            $commonDir = ([string]$lines[1]).Trim()
        }
    }

    # BOTH SIDES, from one command. `--left-right --count` prints "<behind>	<ahead>" for
    # `origin/main...HEAD`: left is what origin/main has and HEAD does not, right is the reverse.
    # The two-dot form used here first answered only the left side, so a branch AHEAD of main read
    # as `0` and was printed as "level" -- false, and quoted as evidence in this PR's own body.
    $behind = $null
    $ahead = $null
    $reason = ''
    $countOutput = @(& git -C $Root rev-list --left-right --count origin/main...HEAD 2>$null)
    $countExit = $LASTEXITCODE
    if ($countExit -ne 0) {
        $reason = 'origin/main is not present in this checkout'
    } else {
        $first = @($countOutput | Where-Object { $_ } | Select-Object -First 1)
        $text = if ($first) { ([string]$first).Trim() } else { '' }
        $parts = @($text -split '\s+' | Where-Object { $_ })
        $leftParsed = 0
        $rightParsed = 0
        if ($parts.Count -ge 2 -and [int]::TryParse($parts[0], [ref] $leftParsed) -and [int]::TryParse($parts[1], [ref] $rightParsed)) {
            $behind = $leftParsed
            $ahead = $rightParsed
        } else {
            $reason = "rev-list --left-right --count printed '$text'"
        }
    }

    # The working tree, because the scan reads it rather than the commit. Counted, not described:
    # a number a reader can compare beats an adjective.
    # Scoped to the scanned paths, because the claim is about what was READ. A repository-wide
    # count would be true about the repository and false about this sweep.
    $dirty = $null
    if (@($ScannedPaths).Count -gt 0) {
        $statusOutput = @(& git -C $Root status --porcelain --untracked-files=no -- @ScannedPaths 2>$null)
        $statusExit = $LASTEXITCODE
        if ($statusExit -eq 0) {
            $dirty = @($statusOutput | Where-Object { $_ }).Count
        }
    } else {
        $dirty = 0
    }

    # #1006 review, reproduced by the reviewer: `git status` OMITS a file marked
    # `--assume-unchanged` or `--skip-worktree`, while the parser still reads its edited
    # working-copy bytes. So the dirty count above can read 0 on a tree whose scanned input has
    # changed -- the false-clean this whole line exists to prevent, hiding behind an index flag.
    # `git ls-files -v` is where the flags are visible: a LOWERCASE status letter means
    # assume-unchanged, `S` means skip-worktree. Counted, not resolved: the honest answer is that
    # the count cannot be trusted, not a different number.
    $hidden = 0
    if (@($ScannedPaths).Count -gt 0) {
        $flagOutput = @(& git -C $Root ls-files -v -- @ScannedPaths 2>$null)
        if ($LASTEXITCODE -eq 0) {
            # ORDINAL, and the reason is this file: my first spelling used `-ceq`, which is
            # case-sensitive and still CULTURE-AWARE -- and this sweep flagged it in its own
            # source on the first run. The tool caught its author, which is the strongest thing
            # it can do and the reason the check exists at all.
            $hidden = @($flagOutput | Where-Object {
                    $_ -and (
                        [char]::IsLower($_[0]) -or
                        [string]::Equals([string]$_[0], 'S', [System.StringComparison]::Ordinal)
                    )
                }).Count
        }
    }

    return Format-TreeProvenance -Head $head -GitDir $gitDir -GitCommonDir $commonDir -Behind $behind -Ahead $ahead -Dirty $dirty -Hidden $hidden -BehindReason $reason
}

if ([string]::Equals($MyInvocation.InvocationName, '.', [System.StringComparison]::Ordinal)) { return }

$files = if ($Path) { @($Path) } else {
    # NO PIPELINE BETWEEN THE COMMAND AND ITS EXIT CODE. `Select-Object -First 1` stops the pipeline
    # as soon as it has its one item, which TERMINATES the native command -- and on PowerShell 7 that
    # leaves `$LASTEXITCODE` at -1 while `$root` holds the correct path. The tool then threw "not
    # inside a git working tree" while holding the path of the git working tree it was in: harness
    # broken, wearing a user error, failing toward the wrong colour. That is the class this whole
    # pull request is about, inside the instrument the pull request delivers.
    #
    # NOT REPRODUCED HERE, and said so rather than left implied: this machine runs Windows PowerShell
    # 5.1, where the pipeline does not terminate the command and both spellings exit 0. It was
    # measured on another host by a reviewer who could not run the documented invocation at all. The
    # gate runs `powershell.exe`, so the gate would never have seen it -- which is precisely why it
    # had to be fixed rather than filed: an instrument invisible to the instrument that would catch
    # it is the one that gets believed.
    #
    # The remedy is the shape, not the operator: capture, read the exit code, and only then reduce.
    $rootOutput = @(& git rev-parse --show-toplevel 2>$null)
    $rootExit = $LASTEXITCODE
    $root = @($rootOutput | Where-Object { $_ } | Select-Object -First 1)
    if ($rootExit -ne 0 -or -not $root) { throw 'not inside a git working tree, and no -Path was given' }
    $root = ([string]$root).Trim()
    # #835: kept for the provenance line at the end. The root is the tree this sweep READ, and the
    # summary has to be able to name it after this expression has gone out of scope.
    $script:sweepRoot = $root
    # AND THE LISTING'S EXIT CODE IS READ TOO, which it was not: a failed `ls-files` produced an
    # empty list, the map over it produced no files, and the summary said "0 culture-aware
    # comparisons over 0 files" -- a clean answer built out of a failure. Found by the cell written
    # for the line above, in the same file, three lines away: the fix for one site swept up its
    # neighbour, which is the whole reason that cell asserts a SHAPE and not a line number.
    $listOutput = @(& git -C $root ls-files '*.ps1')
    if ($LASTEXITCODE -ne 0) { throw "git ls-files failed in $root, so the file list is not a file list" }
    @($listOutput | Where-Object { $_ } | ForEach-Object { Join-Path $root $_ })
}

$found = @(foreach ($file in $files) { Find-CultureComparison -File $file -Include $Include })

if ($AsJson) {
    # #1006 review: this path returned before the provenance line existed, so a machine caller got
    # findings from a stale checkout with no head and no distance -- the very false reading the
    # line was added to end, surviving in the one mode nobody looks at.
    #
    # The shape changes from a bare array to `{ tree, sites }`. MEASURED before changing it: no
    # caller in this repository passes `-AsJson` (`git grep AsJson` over ci/, .factory/, apps/,
    # tools/, core/ finds only this script), so there is no consumer to break -- and provenance
    # BESIDE the JSON on another stream would be the same absence one pipe along.
    [ordered]@{
        tree  = if ($Path) { 'not asked -- an explicit -Path was given' } else { Get-TreeProvenance -Root $script:sweepRoot -ScannedPaths $files }
        sites = @($found)
    } | ConvertTo-Json -Depth 5
    return
}

foreach ($site in $found) {
    Write-Host ("{0}:{1}  [{2}] {3}  {4}" -f $site.File, $site.Line, $site.Kind, $site.Operator, $site.Text)
}
# Counted through @() rather than off the variables. Under StrictMode a scalar has no `.Count`, and
# ONE result is a scalar -- so the summary line threw exactly when the answer was a single site,
# which is the case an operator is most likely to be looking at.
$foundCount = @($found).Count
$fileCount = @($files).Count
Write-Host ""
# THE CAVEAT IS DERIVED FROM THE MODE THAT RAN, not written once. Both modes used to end with the
# same sentence -- "switch, -match and -like are not looked for" -- and in `all` that is FALSE in the
# very output that lists switch findings. An instrument whose product is a claim about a corpus was
# closing with a wrong claim about its own scope, and a reader who trusted it in `all` mode would
# conclude the sweep is blind to exactly the family it had just reported. (Found by D in review.)
$blindTo = if ([string]::Equals($Include, 'all', [System.StringComparison]::Ordinal)) {
    '-match, -like, and any comparison reached through a variable holding an operator name'
} else {
    'switch clauses, lookups by key and the culture-sensitive string members (run with -Include all ' +
    'for those), plus -match, -like and any comparison reached through a variable holding an operator name'
}
$unreadCount = @($script:unreadable).Count
if ([string]::Equals($Include, 'all', [System.StringComparison]::Ordinal)) {
    foreach ($kind in @('binary', 'switch', 'index', 'member')) {
        $ofKind = @($found | Where-Object { [string]::Equals($_.Kind, $kind, [System.StringComparison]::Ordinal) })
        Write-Host ("  $kind : $(@($ofKind).Count)")
    }
    Write-Host ('  (not looked for, because they are ORDINAL and measured so: .Equals, .Contains, ' +
        '.Replace, -like, -match. A false positive is how a sweep gets deleted.)')
}
# #835: the tree, printed with the number rather than left for the reader to assume. Only asked in
# repository-wide mode -- with an explicit -Path the files are whatever was named on the command
# line, and a line about the current directory's checkout would be describing something else.
$provenance = if ($Path) {
    'Tree: not asked -- an explicit -Path was given, so this counts the files named on the command line and nothing else'
} else {
    Get-TreeProvenance -Root $script:sweepRoot -ScannedPaths $files
}
Write-Host $provenance
Write-Host ("$foundCount culture-aware comparison(s) over $fileCount file(s)" +
    $(if ($unreadCount -gt 0) { ", and $unreadCount file(s) COULD NOT BE READ: " + (@($script:unreadable) -join ', ') } else { '' }) + '. ' +
    'A LIST, not a verdict: which of these is a defect depends on where the value comes from, ' +
    "and an empty list is not a proof of absence -- not looked for in this mode: $blindTo.")
