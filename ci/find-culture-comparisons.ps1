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
        # The short head read BEFORE the file list was taken. Every other field here is measured
        # AFTER the scan, so all of them describe the tree at the end of a window whose contents
        # were read across it: the list comes from the index at T0, the bytes are read over
        # [T0,T1], and the head, distance and dirty count are asked at T1. A checkout landing
        # inside that window makes every one of them name a tree that did not produce these
        # findings -- a provenance line that is confidently wrong, which is the exact defect this
        # line was added to end (Codex P2 on #1006). Empty means it could not be read, which is
        # NOT the same as unchanged and is not allowed to read as it.
        [string] $HeadBefore,
        # The head read AFTER every provenance query, not only after the scan. `$Head` is taken
        # before `rev-list` so the distance can be pinned to it, which leaves the working-tree
        # counts still to come: a checkout between them makes the line report a stable tree while
        # its dirty count describes a different one. The window closes only when the last
        # boundary is after the last question (Codex P2 on #1024, the half the first fix missed).
        [string] $HeadAfterAll,
        # HEAD reflog entry counts, before the listing and after every provenance query.
        # $null when the reflog could not be used. THREE SAMPLES DO NOT PROVE CONTINUITY: a
        # tree that moves and moves back between two readings shows equal heads at both, and
        # the findings still combine two revisions (Codex P2 on #1024). The reflog is the one
        # cheap thing here that DETECTS rather than samples -- git appends an entry for every
        # HEAD update, commit and checkout alike, so a grown count proves movement that equal
        # shas would hide. A count of 0 means the reflog is unusable, not that nothing moved:
        # a live checkout with commits always has at least one entry.
        $ReflogBefore,
        $ReflogAfter,
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

    # ABBREVIATE FOR DISPLAY, COMPARE IN FULL. `rev-parse --short` lengthens an abbreviation as
    # needed to stay unique, so a concurrent fetch or object write changes the TEXT without
    # changing HEAD -- reproduced by a reviewer at `core.abbrev=4`, where the same commit read
    # `23b4` and then `23b41`, and every comparison below would have called that a move (Codex
    # P2 on #1024). The three reads now ask for the full object id; this is the only place a
    # short form is produced, and nothing compares it.
    function Format-Sha {
        param([string] $Value)
        if ([string]::IsNullOrWhiteSpace($Value)) { return '' }
        return $Value.Substring(0, [Math]::Min(8, $Value.Length))
    }
    $headText = if ([string]::IsNullOrWhiteSpace($Head)) { 'an UNKNOWN head' } else { Format-Sha $Head }

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

    # The parser reads WORKING-COPY bytes, while the status count is a trailing observation. A
    # process can edit a scanned file and restore it before status runs, leaving the heads,
    # reflog, and dirty count unchanged even though the findings came from transient bytes. No
    # lock or content snapshot exists here to prove atomic working-copy continuity, so say that
    # limit instead of turning equal trailing observations into a guarantee.
    $workingCopyText = ', working-copy continuity: UNKNOWN (files were read from disk; trailing status does not prove atomic contents)'

    # SAID IN EVERY STATE, including the good one. If movement were reported only when it
    # happened, a line with no such clause would be ambiguous between "did not move" and "was
    # never asked" -- and the quiet reading is the reassuring one. Stating the negative also
    # makes the wiring testable end to end: only a run that really took a head before the scan
    # can print the unchanged sentence.
    #
    # BOTH HEADS, not just the one this parameter was added for. `$headText` is `$Head` OR the
    # literal 'an UNKNOWN head' when the post-scan read failed -- so comparing a real sha to it
    # is never equal, and the line asserted `MOVED during the scan (<sha> -> an UNKNOWN head)`:
    # a movement nobody observed, from a failed read. I applied the empty-is-not-equal
    # discipline to the before-head and not to its twin, which is the shape of a one-sided
    # sweep (J's BLOCK on #1024). Either head missing means the question was not answered.
    # THREE BOUNDARIES, and unchanged is said only when all three agree: before the file list,
    # before the distance is counted, and after every working-tree question. Two would let a
    # tree move and move back between them and still read as stable.
    $unread = @(@($HeadBefore, $Head, $HeadAfterAll) | Where-Object { [string]::IsNullOrWhiteSpace($_) }).Count
    $headsAgree = ($unread -eq 0) -and
        [string]::Equals($HeadBefore, $Head, [System.StringComparison]::Ordinal) -and
        [string]::Equals($Head, $HeadAfterAll, [System.StringComparison]::Ordinal)
    $reflogUsable = ($null -ne $ReflogBefore) -and ($null -ne $ReflogAfter)
    # EITHER SIGNAL IS MOVEMENT, because each one alone has a blind spot the other covers. The
    # TOP LINE catches a move whose entries were pruned away, since a prune cannot remove the
    # newest entry. THE COUNT catches a REPEATED move: the same B->A checkout twice writes
    # byte-identical top text, so the marker is not a unique entry identity -- reproduced at
    # counts 5 to 7 with the top unchanged (Codex P2 on #1024). Requiring both would be an AND
    # of two partial detectors, which is a detector for neither.
    # AN ENTRY PROVES AN OPERATION, NOT A MOVE. `git reset --hard HEAD` appends an entry whose
    # oid is the one HEAD already had: nothing moved, and a count that only grows would report
    # MOVED AND RETURNED (Codex P2 on #1024). What the two ends can establish is that an
    # operation was RECORDED -- and that is enough for the reader, because `reset --hard` also
    # rewrites the working tree the scan was reading. The line says what was observed and names
    # the two readings it cannot tell apart.
    # TWO SIGNALS, TWO DIFFERENT FACTS, and collapsing them is what made the line overstate. The
    # newest entry's OID changing PROVES HEAD stood on another commit: nothing else writes a
    # different oid there. The count growing proves only that an OPERATION was recorded, which a
    # hard reset onto the same commit does without moving anything. Each also covers the other's
    # blind spot: the oid survives pruning, the count survives a repeated move whose newest entry
    # names the same commit as before it.
    # A TABLE, not another branch. Two independent observations -- did the newest entry's OID
    # change, and which way did the count move -- make six states, and this predicate has now
    # been wrong in three of them, once per review round. Enumerating them is what stops the
    # next hole being the next round's finding:
    #
    #   oid changed + count grew    an entry was APPENDED naming another commit: HEAD moved
    #   oid changed + count same    nothing was appended, yet the newest entry changed: the log
    #   oid changed + count fell    was REWRITTEN (`reflog delete`, `expire`), and a rewrite is
    #                               indistinguishable from a move -- report UNKNOWN, not a move
    #   oid same    + count grew    an operation was recorded that left HEAD's id alone
    #   oid same    + count fell    entries were PRUNED: the snapshots are not equal, and saying
    #                               'same count' would contradict the measurement
    #   oid same    + count same    equal snapshots
    #
    # The two rewrite rows are Codex P2s on #1024: `git reflog delete HEAD@{0}` removes the
    # newest entry and EXPOSES an older one with a different oid while HEAD never moves, and a
    # prune that leaves the newest entry alone lowers the count under a sentence claiming the
    # snapshots are equal.
    $reflogOidChanged = $reflogUsable -and
        (-not [string]::Equals($ReflogBefore.Top, $ReflogAfter.Top, [System.StringComparison]::Ordinal))
    $reflogCountGrew = $reflogUsable -and ($ReflogAfter.Count -gt $ReflogBefore.Count)
    $reflogCountFell = $reflogUsable -and ($ReflogAfter.Count -lt $ReflogBefore.Count)
    # THE INITIAL MARKER IS A READING TOO, and every row of the table above silently treated it
    # as an axiom. The whole oid-changed inference is "the newest entry named X before and Y
    # after, so HEAD stood somewhere else in between" -- which only follows if X was where HEAD
    # WAS when the snapshot was taken. `git reflog delete HEAD@{0}` (without `--updateref`, which
    # is the option that would move the ref) removes the newest entry and EXPOSES an older one
    # while HEAD stays put, so the marker starts out naming a commit HEAD is not on. A later
    # in-place `git reset --hard HEAD` then appends an entry naming HEAD: the oid "changes", the
    # count grows, all three head readings are identical -- byte for byte the input shape of
    # MOVED AND RETURNED, from a tree that never left (Codex P2 on #1024). The rewrite happens
    # BEFORE the first snapshot, so the delta table sees ordinary growth and cannot catch it.
    # The check is cheap and local: `$HeadBefore` and `$ReflogBefore` are taken by the caller at
    # the same instant, so they are comparable, and if they disagree the marker is not a
    # position of HEAD and cannot carry a conclusion about HEAD.
    $reflogMarkerDesynced = $reflogUsable -and
        (-not [string]::IsNullOrWhiteSpace($HeadBefore)) -and
        (-not [string]::IsNullOrWhiteSpace($ReflogBefore.Top)) -and
        (-not [string]::Equals($ReflogBefore.Top, $HeadBefore, [System.StringComparison]::Ordinal))
    # A move APPENDS. An oid that changed without an append is the log being rewritten under us.
    # And neither conclusion is available at all once the marker is known to be out of step.
    $reflogMarkerUnusable = $reflogOidChanged -and $reflogMarkerDesynced
    $reflogMoved = $reflogOidChanged -and $reflogCountGrew -and (-not $reflogMarkerDesynced)
    $reflogRewritten = $reflogOidChanged -and (-not $reflogCountGrew) -and (-not $reflogMarkerDesynced)
    $reflogGrew = $reflogMoved -or $reflogCountGrew
    $reflogDelta = if ($reflogUsable -and ($ReflogAfter.Count -gt $ReflogBefore.Count)) {
        "gained $($ReflogAfter.Count - $ReflogBefore.Count) entr(ies)"
    } else {
        'recorded a new entry, and was pruned during the run so the gain cannot be counted'
    }

    # WHAT THIS CANNOT SEE, named because the file states pruning, repetition and
    # three-samples-are-not-continuity separately and their COMPOSITION is a fourth thing. One
    # event blinds both signals at once: a repeated B->A checkout writes byte-identical top
    # text, and a prune landing in the same run can remove exactly as many entries as those
    # moves added -- top equal, count equal, heads equal, and the line says no HEAD update was
    # recorded. It needs a concurrent expirer, which is not hypothetical here: the pruning
    # clause above exists because another lane can expire mid-run. A third signal would not
    # close it; an entry IDENTITY that survives both would be a different mechanism, and this
    # file does not have one. Requiring both signals would be an AND of two partial detectors,
    # which is a detector for neither -- so the OR stands and the gap is declared instead.
    # The reflog also sees only HEAD updates: a working-tree edit with no ref change is
    # invisible to it, which is what the dirty and index-flag counts beside it are for.
    # PROVEN MOVEMENT OUTRANKS AN UNREAD BOUNDARY. `$unread -gt 0` used to be the FIRST branch,
    # so a run whose two successful head readings DISAGREED -- movement already proven -- was
    # reported as UNKNOWN, and a changed reflog oid was suppressed with it. Unknown is the answer
    # when nothing was established, not whenever something was missed: a failed reading removes
    # evidence, it does not remove the evidence that survived (Codex P2 on #1024). The failed
    # boundary is still named, in the branch that reports the movement.
    $readHeads = @(@($HeadBefore, $Head, $HeadAfterAll) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $readHeadsDisagree = (@($readHeads | Select-Object -Unique).Count -gt 1)
    $unreadNote = if ($unread -gt 0) { " ($unread of the 3 head readings failed)" } else { '' }

    # FULL IDS WHEREVER TWO ARE PRINTED TO BE COMPARED. Eight characters NAMES one commit and
    # cannot TELL TWO APART, so any branch showing a pair shows them whole. The first version of
    # this rule was applied to the disagreement branch alone and left the reflog-movement branch
    # abbreviating two distinct oids -- the same one-sided sweep, one commit after writing that a
    # sweep is only as wide as the property named (Codex P2 on #1024).
    $movedText = if ($readHeadsDisagree) {
        ", and the tree MOVED: the head readings DISAGREE ($HeadBefore -> $Head -> " +
        "$HeadAfterAll), so this line cannot say which of them the findings came from$unreadNote"
    } elseif ($reflogMarkerUnusable) {
        ", and whether HEAD MOVED is UNKNOWN: the reflog's newest entry named " +
        "$($ReflogBefore.Top) while HEAD read $HeadBefore at the same instant, so the log was " +
        'ALREADY out of step with HEAD before the scan -- a reflog delete without --updateref ' +
        "does exactly that. Its newest entry then changed to $($ReflogAfter.Top), which an " +
        'in-place operation can produce without HEAD ever leaving the commit above, so the ' +
        'change is not evidence of a move'
    } elseif ($reflogMoved -and ($unread -eq 0)) {
        ", and HEAD MOVED AND RETURNED during the run: all 3 readings say " +
        "$(Format-Sha $Head), and the reflog's newest entry names $($ReflogAfter.Top) " +
        "where it named $($ReflogBefore.Top) before -- HEAD stood on another commit " +
        'in between, so these findings can combine revisions'
    } elseif ($reflogMoved) {
        ", and HEAD MOVED during the run: the reflog's newest entry names $($ReflogAfter.Top) " +
        "where it named $($ReflogBefore.Top) before, which is proof independent of the head " +
        "readings$unreadNote"
    } elseif ($reflogRewritten) {
        ", and the reflog was REWRITTEN during the run: its newest entry changed from " +
        "$($ReflogBefore.Top) to $($ReflogAfter.Top) with no net count growth, which can be " +
        'a reflog delete or an expire; whether an entry was appended is UNKNOWN, so whether HEAD ' +
        'moved is UNKNOWN because a rewrite and a move look the same from here'
    } elseif ($reflogGrew -and $headsAgree) {
        ", and a HEAD OPERATION was RECORDED during the run: all 3 readings say " +
        "$(Format-Sha $Head), the reflog $reflogDelta between them, and its newest entry still " +
        "names $(Format-Sha $ReflogAfter.Top) -- a move and a return, or an in-place operation " +
        'such as a hard reset onto the same commit, are indistinguishable from the ends, so ' +
        'these findings can combine revisions'
    } elseif ($reflogGrew) {
        ", and a HEAD OPERATION was RECORDED during the run: the reflog $reflogDelta between " +
        "the snapshots and its newest entry still names $(Format-Sha $ReflogAfter.Top); whether " +
        "the tree MOVED is UNKNOWN$unreadNote because a head boundary was unread"
    } elseif ($unread -gt 0) {
        ", and whether the tree MOVED is UNKNOWN ($unread of the 3 head readings failed)"
    } elseif ($reflogCountFell) {
        ", and the reflog was PRUNED during the run: its newest entry is unchanged at " +
        "$($ReflogAfter.Top) and its count fell from $($ReflogBefore.Count) to " +
        "$($ReflogAfter.Count), so the snapshots are NOT equal. What is observed is the " +
        "matching newest entry and the smaller count; whether anything was APPENDED in " +
        'between is UNKNOWN, because an in-place operation appends an entry with the SAME ' +
        'oid and a prune of older ones hides it in the net'
    } elseif ($reflogUsable) {
        ', head unchanged at all 3 readings, and the reflog SNAPSHOTS are equal (same newest ' +
        'entry, same count) between the first reading and the last snapshot -- equal snapshots, ' +
        'not proof that no update happened: a move and a return whose entries a concurrent ' +
        'expire removed leaves both readings identical. Nothing here observes what happens ' +
        'after the last snapshot'
    } else {
        ', head equal at all 3 readings, which are SAMPLES: the reflog could not be used, so a move and a move back would not have been seen'
    }

    return "Tree: $headText in $where, $behindText$dirtyText$hiddenText$workingCopyText$movedText"
}

# ONE FORM FOR BOTH READINGS. The before and after counts are compared, so they must be taken
# the same way; two hand-written call sites is how a comparison starts measuring two things.
# $null on any doubt: a non-zero exit, or a count of 0, which a live checkout with commits
# never has -- so "the reflog is off" cannot read as "nothing moved".
function Get-HeadReflogMark {
    param([Parameter(Mandatory)] [string] $Root)
    # `--no-abbrev`, and the reason is a PROPERTY rather than this call: `git reflog show`
    # abbreviates the object id in its first column by default (measured: 8 characters against
    # 40 with the flag), and abbreviations LENGTHEN under a concurrent object write. This top
    # line is compared with `String::Equals`, so the same entry would have read as a new one
    # and the sweep would have reported MOVED AND RETURNED. Same defect as the `rev-parse
    # --short` one two commits ago -- and finding it there and not here is the third one-sided
    # sweep on this branch (J on #1024). The rule, applied to every git call in this file
    # rather than to the one under discussion: ANY git output this tool COMPARES AS TEXT must
    # be the full object id. The reflog MESSAGE may still contain a short sha, and that is
    # fine: it is stored text, not a value git recomputes, so it cannot lengthen underneath us.
    # `-c color.ui=false` on EVERY git call in this file, not on this one. A developer with
    # `color.ui=always` gets ANSI sequences wrapped around the first column, so the oid parsed
    # out of it is a decorated string and the shape assertion fails -- the authoritative gate red
    # for a configuration that has nothing to do with what is measured (Codex P2 on #1024). That
    # is the THIRD time this file inherited a developer's git config, after core.logAllRefUpdates
    # and GIT_DEFAULT_HASH, so it is swept as a property rather than patched at the named site:
    # a command whose OUTPUT IS PARSED must not be configurable by the person running it.
    # AND `--no-color` ON THE COMMAND ITSELF, because the config pin is not enough. `color.diff`
    # is more specific than `color.ui` and wins for the log family, which `reflog show` belongs
    # to: measured here, `-c color.diff=always -c color.ui=false` still returns the oid wrapped in
    # ESC[33m, and adding `--no-color` returns it clean (Codex P2 on #1024). So the rule is
    # sharper than the one written above it: a blanket config pin covers the general case, and
    # where the command HAS its own option, the option is the authority -- a subordinate setting
    # can always override the general one.
    $entries = @(& git -c color.ui=false -C $Root reflog show HEAD --no-abbrev --no-color 2>$null)
    if ($LASTEXITCODE -ne 0) { return $null }
    $lines = @($entries | Where-Object { $_ })
    if ($lines.Count -eq 0) { return $null }
    # THE NEWEST ENTRY, not the count. `git reflog expire` prunes OLD entries, so a count can
    # fall while HEAD moved -- another lane expiring mid-run offsets the entries an A->B->A
    # checkout appended, and a detector built on counts reports no movement (Codex P2 on
    # #1024). Pruning never removes the most recent entry, and every HEAD update appends one,
    # so the top line changes if and only if HEAD was updated. The count is kept only to say
    # HOW MANY updates, and to notice a prune.
    # THE OID, not the whole display line. `--no-abbrev` makes the first token the full object
    # id of what HEAD pointed at after that entry, and that is the thing that says WHERE HEAD is.
    # The rest of the line is a message, and two operations leaving HEAD on the same commit write
    # different messages -- comparing the line would call that a move.
    $newest = ([string]$lines[0]).Trim()
    $oid = @($newest -split '\s+' | Where-Object { $_ } | Select-Object -First 1)
    if (-not $oid) { return $null }
    return [pscustomobject]@{ Top = [string]$oid; Count = $lines.Count }
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
        [string[]] $ScannedPaths = @(),
        # Read by the CALLER before the file list was taken. This function cannot take it
        # itself: by the time it runs, the scan is over.
        [string] $HeadBefore = '',
        # Taken by the CALLER at the same instant as $HeadBefore, for the same reason.
        $ReflogBefore = $null
    )

    $headOutput = @(& git -c color.ui=false -C $Root rev-parse HEAD 2>$null)
    $headExit = $LASTEXITCODE
    $head = ''
    if ($headExit -eq 0) {
        $first = @($headOutput | Where-Object { $_ } | Select-Object -First 1)
        if ($first) { $head = ([string]$first).Trim() }
    }

    # ONE call for both, so the two paths can never come from different moments.
    $dirsOutput = @(& git -c color.ui=false -C $Root rev-parse --path-format=absolute --git-dir --git-common-dir 2>$null)
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
    # AGAINST THE SHA ALREADY READ, not against `HEAD` again. Asking git for `HEAD` a second
    # time reopens the same window one level down: a checkout between the `rev-parse` above and
    # this call would leave the head and the distance describing different commits, inside the
    # one line whose job is to say which commit these findings came from (J's BLOCK on #1024).
    # The dirty and hidden counts below CANNOT be pinned this way -- they are working-tree
    # facts with no commit to name -- which is why the line reports them as read rather than
    # as properties of the sha.
    if ([string]::IsNullOrWhiteSpace($head)) {
        $reason = 'the head could not be read, so no distance can be pinned to it'
    } else {
        $countOutput = @(& git -c color.ui=false -C $Root rev-list --left-right --count "origin/main...$head" 2>$null)
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
    }

    # The working tree, because the scan reads it rather than the commit. Counted, not described:
    # a number a reader can compare beats an adjective.
    # Scoped to the scanned paths, because the claim is about what was READ. A repository-wide
    # count would be true about the repository and false about this sweep.
    $dirty = $null
    if (@($ScannedPaths).Count -gt 0) {
        $statusOutput = @(& git -c color.ui=false -C $Root status --porcelain --untracked-files=no -- @ScannedPaths 2>$null)
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
        $flagOutput = @(& git -c color.ui=false -C $Root ls-files -v -- @ScannedPaths 2>$null)
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

    # ORDER MATTERS HERE, and the previous version had it backwards. The reflog read used to come
    # AFTER the final head read, which reopened the window it exists to close: a checkout landing
    # between the two left all three head samples equal and the reflog top changed, and the line
    # said MOVED AND RETURNED when HEAD had moved and NOT returned (Codex P2 on #1024). With the
    # reflog first, a move in that gap shows up as the head DISAGREEING, which is the true
    # statement. Something has to be last; the head is the reading whose disagreement is not a
    # wrong conclusion.
    # SOMETHING IS READ LAST, and whatever it is has an unobserved tail. Putting the reflog last
    # made a move that did NOT return read as one that did; putting the head last leaves the
    # interval between the reflog snapshot and the final head read covered by no detector, so an
    # A->B->A completed inside it is invisible to every reading (Codex P2 on #1024). A further
    # snapshot only moves the tail; it does not remove it. So the ORDER is chosen for which
    # failure is a true sentence -- a move in the tail makes the heads disagree, which is true --
    # and the line NAMES the interval the detector covers instead of claiming the whole run.
    $reflogAfter = Get-HeadReflogMark -Root $Root

    # THE LAST BOUNDARY, after every question above. Its own statement, its own exit code, and
    # empty on failure so the line says UNKNOWN rather than assuming stability.
    $endOutput = @(& git -c color.ui=false -C $Root rev-parse HEAD 2>$null)
    $headAfterAll = ''
    if ($LASTEXITCODE -eq 0) {
        $endFirst = @($endOutput | Where-Object { $_ } | Select-Object -First 1)
        if ($endFirst) { $headAfterAll = ([string]$endFirst).Trim() }
    }

    return Format-TreeProvenance -Head $head -GitDir $gitDir -GitCommonDir $commonDir -Behind $behind -Ahead $ahead -Dirty $dirty -Hidden $hidden -HeadBefore $HeadBefore -HeadAfterAll $headAfterAll -ReflogBefore $ReflogBefore -ReflogAfter $reflogAfter -BehindReason $reason
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
    # BEFORE the listing, which is the first thing that reads repository state. Read on its own
    # statement with its own exit code, for the reason the whole file argues: a failed read must
    # leave this empty so the summary says UNKNOWN, never quietly equal to the head measured at
    # the end (Codex P2 on #1006).
    $headBeforeOutput = @(& git -c color.ui=false -C $root rev-parse HEAD 2>$null)
    $script:sweepHeadBefore = ''
    if ($LASTEXITCODE -eq 0) {
        $firstHead = @($headBeforeOutput | Where-Object { $_ } | Select-Object -First 1)
        if ($firstHead) { $script:sweepHeadBefore = ([string]$firstHead).Trim() }
    }
    $script:sweepReflogBefore = Get-HeadReflogMark -Root $root
    # AND THE LISTING'S EXIT CODE IS READ TOO, which it was not: a failed `ls-files` produced an
    # empty list, the map over it produced no files, and the summary said "0 culture-aware
    # comparisons over 0 files" -- a clean answer built out of a failure. Found by the cell written
    # for the line above, in the same file, three lines away: the fix for one site swept up its
    # neighbour, which is the whole reason that cell asserts a SHAPE and not a line number.
    $listOutput = @(& git -c color.ui=false -C $root ls-files '*.ps1')
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
    # caller in this repository passed `-AsJson` (`git grep AsJson` over ci/, the since-retired
    # process directory, apps/, tools/, core/ found only this script), so there is no consumer to
    # break -- and provenance BESIDE the JSON on another stream would be the same absence one pipe
    # along.
    [ordered]@{
        tree  = if ($Path) { 'not asked -- an explicit -Path was given' } else { Get-TreeProvenance -Root $script:sweepRoot -ScannedPaths $files -HeadBefore $script:sweepHeadBefore -ReflogBefore $script:sweepReflogBefore }
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
    Get-TreeProvenance -Root $script:sweepRoot -ScannedPaths $files -HeadBefore $script:sweepHeadBefore -ReflogBefore $script:sweepReflogBefore
}
Write-Host $provenance
Write-Host ("$foundCount culture-aware comparison(s) over $fileCount file(s)" +
    $(if ($unreadCount -gt 0) { ", and $unreadCount file(s) COULD NOT BE READ: " + (@($script:unreadable) -join ', ') } else { '' }) + '. ' +
    'A LIST, not a verdict: which of these is a defect depends on where the value comes from, ' +
    "and an empty list is not a proof of absence -- not looked for in this mode: $blindTo.")
