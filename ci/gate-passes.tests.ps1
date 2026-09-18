<#
.SYNOPSIS
    Guard cells for the queue's pass predicate (#1133).

.DESCRIPTION
    Each cell here is paired with a SABOTAGE at the bottom of the file: the predicate is edited to
    the wrong-but-legal form the cell exists to forbid, and the cell must go red. A cell nobody has
    seen fail is a cell that might be asserting nothing -- the defect this whole change repairs was
    a number that looked right for two weeks.

    The counter is driven through its `-Invoker` seam, so no cell touches the network: the fake
    answers with the exact `gh api` shapes, and a body that must not score carries the FULL payload
    of one that must, differing only in the property under test.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'gate-passes.ps1')

$script:Failures = 0
function Assert-True {
    param([bool] $Condition, [string] $Message)
    if ($Condition) { Write-Host "  ok   $Message" } else { Write-Host "  FAIL $Message"; $script:Failures++ }
}

$HEAD = '9a6b44a27a1894d3a5c88166941dd0a6eb3dca78'
$OTHER = 'cff22ac9aa1111111111111111111111111111ff'

function New-Comment { param([string] $Body) return [pscustomobject]@{ body = $Body } }

function New-Invoker {
    # $Comments and $Reviews are arrays of objects; $FailSurfaces names paths that answer non-zero,
    # which is how a partial read is exercised without pretending the network is reliable.
    #
    # THE `pulls/N` PATH IS ANSWERED SEPARATELY, and getting that wrong cost me two false reds: the
    # first version returned the comments array for it, so `Get-AuthorLaneParts` read identity lines
    # out of the JSON TEXT of other lanes' comments and named one of them the author. A fake that
    # answers the wrong surface with plausible content does not fail -- it lies, and the lie looks
    # like the code being broken.
    # -FailAuthor IS SEPARATE FROM -FailSurfaces BECAUSE A SUBSTRING CANNOT NAME ONE SURFACE. A
    # pattern of `/pulls/1125` also matches `/pulls/1125/reviews`, so failing "the author read"
    # through it silently failed the reviews read too, and the cell that was supposed to isolate
    # one hole measured two. The exact-path form is what the author surface needs.
    param($Comments = @(), $Reviews = @(), [string[]] $FailSurfaces = @(), [string] $AuthorBody = '', [switch] $FailAuthor)
    return {
        param($file, $arguments)
        $path = [string] $arguments[1]
        if ($FailAuthor -and $path -match '/pulls/\d+$') { return [pscustomobject]@{ Code = 1; Output = @() } }
        foreach ($bad in $FailSurfaces) { if ($path -like "*$bad*") { return [pscustomobject]@{ Code = 1; Output = @() } } }
        if ($path -match '/pulls/\d+$') { return [pscustomobject]@{ Code = 0; Output = @($AuthorBody) } }
        $payload = if ($path -like '*/issues/*/comments') { $Comments } else { $Reviews }
        $json = (ConvertTo-Json @($payload) -Depth 5)
        if (@($payload).Count -eq 0) { $json = '[]' }
        return [pscustomobject]@{ Code = 0; Output = @($json) }
    }.GetNewClosure()
}

$pass_b_nolabel = "Session: graphhelm-pr-1099-agent-4a311e-f0 [b0b1c9] | Head: 9a6b44a2`n`nAPPROVE-WITH-RISK - same lane, no label."
$pass_a = "Session: eloquent-jones-c66bf5-f9 [3d2fc0] | Head: 9a6b44a2`n`n**APPROVE** - checked the crate."
$pass_b = "Lane: T · Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] · Head: 9a6b44a2`n`nAPPROVE-WITH-RISK - one cell."

Write-Host 'a live pass from each of two lanes counts two'
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'typesafe-early-access-82126a' `
    -Invoker (New-Invoker -Comments @((New-Comment $pass_a), (New-Comment $pass_b)))
Assert-True (($r.Count -eq 2) -and $r.Measured) "two lanes at the head count 2 and the read is complete (got $($r.Count), measured=$($r.Measured))"

Write-Host 'THE DEFECT #1133 CLOSES: review objects without a verdict or a head score nothing'
# Twenty objects, exactly the shape that scored 20 on #1028 under the old counter: real bodies,
# real threads, none of them a pass at this head.
$noise = @(1..20 | ForEach-Object { New-Comment "Session: some-lane-$_ [aaaaaa] | Head: deadbeef`n`nRe-running the gate, will report." })
$r = Get-LivePassCount -PullRequest '1028' -Head $HEAD -AuthorLane 'other-lane' -Invoker (New-Invoker -Comments $noise)
Assert-True ($r.Count -eq 0) "twenty verdict-less objects score 0, not 20 (got $($r.Count))"

Write-Host 'a verdict naming a different sha does not count -- the score DECAYS across a rebase'
$stale = "Session: eloquent-jones-c66bf5-f9 [3d2fc0] | Head: cff22ac9`n`n**APPROVE** - checked the crate."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $stale)))
Assert-True ($r.Count -eq 0) "a pass naming the pre-rebase head scores 0 at the new head (got $($r.Count))"
# CONTROL, and it is the load-bearing half: the SAME body at the head it names must score, or the
# cell above would pass on a predicate that counts nothing at all.
$r = Get-LivePassCount -PullRequest '1125' -Head $OTHER -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $stale)))
Assert-True ($r.Count -eq 1) "CONTROL: that identical body scores 1 against the head it names (got $($r.Count))"

Write-Host 'two comments from one session are one pass'
$repin = "Session: eloquent-jones-c66bf5-f9 [3d2fc0] | Head: 9a6b44a2`n`n**APPROVE** - re-pin of the same read."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $pass_a), (New-Comment $repin)))
Assert-True ($r.Count -eq 1) "one lane posting twice counts once (got $($r.Count))"

Write-Host 'the author lane cannot pass its own pull request'
$selfpass = "Session: typesafe-early-access-82126a-4b | Head: 9a6b44a2`n`n**APPROVE** - mine, and green."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'typesafe-early-access-82126a' `
    -Invoker (New-Invoker -Comments @((New-Comment $selfpass)))
Assert-True ($r.Count -eq 0) "the enqueuing lane's own verdict scores 0, across its recycle suffix (got $($r.Count))"
# CONTROL: the same body, when the author is somebody else, is a perfectly good pass.
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'unrelated-lane' -Invoker (New-Invoker -Comments @((New-Comment $selfpass)))
Assert-True ($r.Count -eq 1) "CONTROL: that identical body scores 1 for a different author lane (got $($r.Count))"

# AND THE AUTHOR UNDER A SIBLING RECYCLE. The cell above catches the author when the enqueue name
# is a strict prefix; two sibling recycles are not prefixes of each other, so only the lane-key
# comparison sees it. Without this cell that comparison could be deleted and nothing would redden
# -- found by sabotage, which is the second time that check has been the one proving a guard hollow.
$authorSibling = "Session: graphhelm-pr-1099-agent-4a311e-f0 [b0b1c9] | Head: 9a6b44a2`n`n**APPROVE** - mine, and green."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'graphhelm-pr-1099-agent-4a311e-71' `
    -Invoker (New-Invoker -Comments @((New-Comment $authorSibling)))
Assert-True ($r.Count -eq 0) "the author under a SIBLING recycle name still scores 0 (got $($r.Count))"

Write-Host 'a pointer with no verdict word scores nothing, and an anonymous verdict scores nothing'
$pointer = "Session: eloquent-jones-c66bf5-f9 [3d2fc0] | Head: 9a6b44a2`n`nPointer, not a pass. My review of 9a6b44a2 is the issue comment."
$anon = "**APPROVE** at 9a6b44a2 - no identity line anywhere in this body."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $pointer), (New-Comment $anon)))
Assert-True ($r.Count -eq 0) "a verdict-free pointer and an unattributable verdict both score 0 (got $($r.Count))"

Write-Host 'a QUOTED verdict in the second line does not score -- the shape that actually occurs'
# NAMED IN REVIEW by the presser lane, whose dead-citation notices carry exactly this sentence in
# their opening. The cell this replaces pinned a quoted verdict at line SEVEN, which is a shape
# nobody writes: it was green against a predicate that scored every one of these.
$quoted_line2 = "Session: unruffled-babbage-df857c-1d [3e1d4c] | Head: 9a6b44a2`nYour APPROVE no longer reaches this head, so a re-pin is owed."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $quoted_line2)))
Assert-True ($r.Count -eq 0) "a verdict quoted mid-sentence on line 2 scores 0 (got $($r.Count))"
$quoted_mid = "Session: some-lane [aaaaaa] | Head: 9a6b44a2`n`nI read this as APPROVE material once the cell lands."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $quoted_mid)))
Assert-True ($r.Count -eq 0) "a verdict inside a sentence anywhere in the opening scores 0 (got $($r.Count))"
# CONTROLS: the real shapes must still count, or the fix above has broken every pass in the repo.
Assert-True ((Test-CarriesVerdict -Body "Session: x | Head: y`n`n**APPROVE** - checked.") -eq $true) 'CONTROL: a bolded verdict opening a line counts'
Assert-True ((Test-CarriesVerdict -Body "Session: x | Head: y`n`nAPPROVE-WITH-RISK - one residual.") -eq $true) 'CONTROL: a bare verdict opening a line counts'
Assert-True ((Test-CarriesVerdict -Body "Session: x | Head: y`n`n- BLOCK - the golden moved.") -eq $true) 'CONTROL: a verdict after a list marker counts'

Write-Host 'a verdict discussed in prose below the opening does not score'
$prose = "Session: unruffled-babbage-df857c-1d [3e1d4c] | Head: 9a6b44a2`n`nA note on the queue.`n`nline3`nline4`nline5`nline6`nline7`nYour APPROVE no longer reaches this head, so a re-pin is owed."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $prose)))
Assert-True ($r.Count -eq 0) "a verdict word past the opening window scores 0 (got $($r.Count))"

Write-Host 'APPROVE-WITH-RISK is not reported as the APPROVE inside it'
Assert-True ((Test-CarriesVerdict -Body "Session: x | Head: y`n`nAPPROVE-WITH-RISK - one residual.") -eq $true) 'APPROVE-WITH-RISK carries a verdict'
Assert-True ((Test-CarriesVerdict -Body "Session: x`n`nI would not approve this yet") -eq $false) 'lower-case prose "approve" is not a verdict'
Assert-True ((Test-CarriesVerdict -Body "Session: x`n`nDISAPPROVED by the old gate") -eq $false) 'a longer word containing APPROVE is not a verdict'

Write-Host 'a failed surface is reported as PARTIAL rather than as a zero'
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $pass_a)) -FailSurfaces @('/pulls/'))
Assert-True (($r.Count -eq 1) -and (-not $r.Measured)) "the comment surface still counts and Measured is false (got $($r.Count), measured=$($r.Measured))"
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' -Invoker (New-Invoker -FailSurfaces @('/issues/', '/pulls/'))
Assert-True (($r.Count -eq 0) -and (-not $r.Measured)) 'both surfaces failing is a zero that says it is not a measurement'

Write-Host 'a reviewer whose name is a bare prefix of the author lane still counts'
# FOUND BY SABOTAGE. The first author check asked `$AuthorLane -like "$session*"`, so a reviewer
# named `t` was read as the author of `typesafe-...` and dropped -- and, because an empty session
# name turns that pattern into `-like "*"`, it also swallowed every anonymous body and made the
# identity guard above look load-bearing when it was not. Both directions get a cell.
$shortname = "Session: t [aaaaaa] | Head: 9a6b44a2`n`n**APPROVE** - a lane with a very short name."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'typesafe-early-access-82126a' `
    -Invoker (New-Invoker -Comments @((New-Comment $shortname)))
Assert-True ($r.Count -eq 1) "a one-character reviewer name is not the author of a lane it merely prefixes (got $($r.Count))"
Assert-True ((Test-SameLane -Session 'typesafe-early-access-82126a-4b' -AuthorLane 'typesafe-early-access-82126a') -eq $true) `
    'a recycle suffix after a - is the same lane'
Assert-True ((Test-SameLane -Session 't' -AuthorLane 'typesafe-early-access-82126a') -eq $false) `
    'a prefix that does not end on a - is a different lane'
Assert-True ((Test-SameLane -Session '' -AuthorLane 'typesafe-early-access-82126a') -eq $false) `
    'an empty session is not the author lane -- it is nobody, and the identity guard must be what drops it'

Write-Host 'a gate receipt moves the head without costing the passes at its parent'
# FOUND IN REVIEW by the author lane of the epic, and live within the hour: #1125's receipt landed
# at 279cffd0 with parent 9a6b44a2, and the count at the tip was 0 while the passes were all at the
# parent. A re-gate after a flake would have cost that pull request the position it earned.
function New-CommitInvoker {
    param([string[]] $Files, [string[]] $ParentShas, $Comments = @(), [string] $AuthorBody = '')
    return {
        param($file, $arguments)
        $path = [string] $arguments[1]
        if ($path -match '/pulls/\d+$') { return [pscustomobject]@{ Code = 0; Output = @($AuthorBody) } }
        if ($path -like '*/commits/*') {
            $payload = @{
                files   = @($Files | ForEach-Object { @{ filename = $_ } })
                parents = @($ParentShas | ForEach-Object { @{ sha = $_ } })
            }
            return [pscustomobject]@{ Code = 0; Output = @((ConvertTo-Json $payload -Depth 5 -Compress)) }
        }
        $json = (ConvertTo-Json @($Comments) -Depth 5)
        if (@($Comments).Count -eq 0) { $json = '[]' }
        return [pscustomobject]@{ Code = 0; Output = @($json) }
    }.GetNewClosure()
}
$manifestTip = '279cffd0aa2222222222222222222222222222ff'
$receiptOnly = @('.factory/gate-runs/9a6b44a27a18-20260917T203142.351Z-4f508013.json')
$inv = New-CommitInvoker -Files $receiptOnly -ParentShas @($HEAD) -Comments @((New-Comment $pass_a), (New-Comment $pass_b))
$parent = Get-ManifestOnlyParent -Sha $manifestTip -Invoker $inv
Assert-True ($parent -eq $HEAD) "a manifest-only tip yields its parent (got '$parent')"
$r = Get-LivePassCount -PullRequest '1125' -Head $manifestTip -AuthorLane 'typesafe-early-access-82126a' `
    -AlsoAcceptHead $parent -Invoker $inv
Assert-True ($r.Count -eq 2) "the two passes at the parent still count at the manifest tip (got $($r.Count))"
# WITHOUT the parent, the same bodies score 0 -- the control proving the cell above is not passing
# for some unrelated reason. "Without the parent" is now spelled with an EXPLICIT empty argument,
# because omitting it resolves the parent; the property is unchanged, only how you ask for it.
$r = Get-LivePassCount -PullRequest '1125' -Head $manifestTip -AuthorLane 'x' -AlsoAcceptHead '' -Invoker $inv
Assert-True ($r.Count -eq 0) "CONTROL: head-only scoring gives 0 for those same bodies at the tip (got $($r.Count))"

Write-Host 'a caller who passes NOTHING still gets the parent rule -- the default is the safe one'
# FOUND by the author lane measuring the module directly: with the parent composed at the call site,
# a caller who skipped the composition read 0 for a just-gated pull request and could not learn the
# argument existed. Safe behaviour must not depend on remembering -- the same defect class as the
# scorer this file replaces.
$r = Get-LivePassCount -PullRequest '1125' -Head $manifestTip -AuthorLane 'x' -Invoker $inv
Assert-True ($r.Count -eq 2) "omitting -AlsoAcceptHead resolves the parent itself and counts 2 (got $($r.Count))"
# AND AN EXPLICIT EMPTY STRING IS STILL OBEYED: a caller asking for the head alone is not
# overridden. This arm distinguishes ABSENT from EMPTY, and without it the cell above would pass on
# a predicate that ignores the argument entirely.
$r = Get-LivePassCount -PullRequest '1125' -Head $manifestTip -AuthorLane 'x' -AlsoAcceptHead '' -Invoker $inv
Assert-True ($r.Count -eq 0) "an explicit empty -AlsoAcceptHead means the head alone (got $($r.Count))"

Write-Host 'a tip that touches anything but a manifest does NOT lend its parent'
$mixed = New-CommitInvoker -Files @('.factory/gate-runs/x.json', 'core/architect/src/lib.rs') -ParentShas @($HEAD)
Assert-True ((Get-ManifestOnlyParent -Sha $manifestTip -Invoker $mixed) -eq '') 'a tip carrying code as well as a manifest yields nothing'
$codeOnly = New-CommitInvoker -Files @('core/architect/src/lib.rs') -ParentShas @($HEAD)
Assert-True ((Get-ManifestOnlyParent -Sha $manifestTip -Invoker $codeOnly) -eq '') 'a code tip yields nothing'
$empty = New-CommitInvoker -Files @() -ParentShas @($HEAD)
Assert-True ((Get-ManifestOnlyParent -Sha $manifestTip -Invoker $empty) -eq '') 'an EMPTY file list is not manifest-only -- it fails toward the head, not toward trust'
$merge = New-CommitInvoker -Files $receiptOnly -ParentShas @($HEAD, $OTHER)
Assert-True ((Get-ManifestOnlyParent -Sha $manifestTip -Invoker $merge) -eq '') 'a two-parent commit yields nothing -- which side the reviewers read is not guessable'

Write-Host 'a blockquoted or code-fenced verdict does not score'
# LANE T's residual: `>` and a backtick were in the prefix class, so a quoted verdict scored
# through a different door than the one the window rule closed -- the trap named, returning in
# another state.
$bq = "Session: some-lane-aaaaaa [111111] | Head: 9a6b44a2`n`n> **APPROVE** - quoting another lane's pass."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $bq)))
Assert-True ($r.Count -eq 0) "a blockquoted verdict scores 0 (got $($r.Count))"
$fenced = "Session: some-lane-aaaaaa [111111] | Head: 9a6b44a2`n`n``APPROVE`` is the word the scorer looks for."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $fenced)))
Assert-True ($r.Count -eq 0) "a code-spanned verdict scores 0 (got $($r.Count))"

Write-Host 'the dedupe unit is the LANE, across recycle names'
# The hole lane T's own message exposed: addressed as ...-4a311e-f0, signed ...-4a311e-71.
$t71 = "Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] | Head: 9a6b44a2`n`n**APPROVE** - read the crate."
$tf0 = "Session: graphhelm-pr-1099-agent-4a311e-f0 [b0b1c9] | Head: 9a6b44a2`n`nAPPROVE-WITH-RISK - re-read after the fix."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $t71), (New-Comment $tf0)))
Assert-True ($r.Count -eq 1) "two recycles of ONE lane count once (got $($r.Count))"
# LANE T's REFUTATION of "strip after the final dash", with the live names it named:
Assert-True ((Get-LaneKey -Name 'review-subagent-R') -ne (Get-LaneKey -Name 'review-subagent-S')) `
    'review-subagent-R and -S stay two lanes -- the naive strip merged them'
Assert-True ((Get-LaneKey -Name 'graphhelm-pr-1099-agent-4a311e-71') -eq 'graphhelm-pr-1099-agent-4a311e') `
    'a suffix after a six-hex token is a recycle and is dropped'
Assert-True ((Get-LaneKey -Name 'eloquent-jones-c66bf5-f9') -eq 'eloquent-jones-c66bf5') 'and the same for this lane'
Assert-True ((Get-LaneKey -Name 'plain-name') -eq 'plain-name') 'a name outside the shape keeps its full form'
# THE LABEL NO LONGER OUTRANKS THE SESSION NAME, and this cell pins the reversal rather than
# being deleted for disagreeing with it. It asserted that a shared `Lane:` letter collapses two
# session names; lane T then measured the cost of that ordering -- one lane mixing the two shapes
# scored twice -- so the base now wins wherever a name exists.
#
# THE RESIDUAL, stated rather than hidden: two DIFFERENT bases that both declare `Lane: T` are
# counted as two lanes. If the board really assigned one letter to both, that is an undercount and
# the pull request loses a queue position. That is the safe direction. The alternative merges two
# bases on a label either could write, and a wrong merge manufactures a quorum -- which is the harm
# this whole file exists to remove, so the failure is aimed here deliberately.
$laneA1 = "Lane: T | Session: whatever-one-aaaaaa [1] | Head: 9a6b44a2`n`n**APPROVE** - one."
$laneA2 = "Lane: T | Session: whatever-two-bbbbbb [2] | Head: 9a6b44a2`n`n**BLOCK** - two."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $laneA1), (New-Comment $laneA2)))
Assert-True ($r.Count -eq 2) "two different bases sharing a Lane letter count separately -- undercount, never a false quorum (got $($r.Count))"
# AND THE LABEL IS STILL THE FALLBACK when there is no session name to key on at all.
Assert-True ((Get-LaneKey -Body 'Lane: T') -eq 'lane:T') 'a body with only a Lane label keys on the label'
Assert-True ((Get-LaneKey -Body 'no identity here at all') -eq '') 'a body with neither keys on nothing and is dropped upstream'

Write-Host 'the four residuals lane T measured after the BLOCK'
# 1. ONE LANE MIXING SHAPES. Lane T measured count=2 from its own comments -- some carrying
# `Lane: T`, some not -- because the label outranked the base. Preferring the label re-opened the
# recycle hole the label was added to close, through a new door. The base wins now.
$mixA = "Lane: T | Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] | Head: 9a6b44a2`n`n**APPROVE** - one."
$mixB = "Session: graphhelm-pr-1099-agent-4a311e-f0 [b0b1c9] | Head: 9a6b44a2`n`nAPPROVE-WITH-RISK - two."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $mixA), (New-Comment $mixB)))
Assert-True ($r.Count -eq 1) "one lane mixing the Lane:-label and bare-Session shapes counts once (got $($r.Count))"

# 2. A LABEL IS CAPTURED WHOLE. `([A-Za-z0-9]+)` dropped the number, so two lanes collapsed.
Assert-True ((Get-LaneKey -Body 'Lane: ISSUES 2') -ne (Get-LaneKey -Body 'Lane: ISSUES 3')) `
    'Lane: ISSUES 2 and ISSUES 3 stay two lanes'
Assert-True ((Get-LaneKey -Body 'Lane: ISSUES 2') -eq 'lane:ISSUES 2') 'and the label keeps its number'

# 3. A HEX-SHAPED SLUG MUST NOT MERGE TWO LANES. `decade` is six hex characters, so the old rule
# stripped `-R` and `-S` from `some-lane-decade-*` and merged them. A suffix is now dropped only
# when it is itself two hex characters, which every observed recycle token is.
Assert-True ((Get-LaneKey -Name 'some-lane-decade-R') -ne (Get-LaneKey -Name 'some-lane-decade-S')) `
    'a hex-shaped slug does not merge -R and -S'
Assert-True ((Get-LaneKey -Name 'graphhelm-pr-1099-agent-4a311e-71') -eq (Get-LaneKey -Name 'graphhelm-pr-1099-agent-4a311e-f0')) `
    'CONTROL: two real recycle tokens still merge to one lane'

# 4. A FENCED BLOCK IS QUOTED TEXT even when the verdict stands alone inside it -- the third door
# into the same defect, after the window rule and the prefix class.
$nl = "`n"
$fencedAlone = "Session: some-lane-aaaaaa [1] | Head: 9a6b44a2" + $nl + $nl + '```' + $nl + "APPROVE" + $nl + '```'
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $fencedAlone)))
Assert-True ($r.Count -eq 0) "a bare verdict inside a fenced block scores 0 (got $($r.Count))"
Assert-True ((Test-CarriesVerdict -Body ("Session: x | Head: y" + $nl + $nl + "**APPROVE** - checked.")) -eq $true) `
    'CONTROL: a real verdict outside any fence still counts'

Write-Host 'two subagent lanes under ONE session name count twice (the #1026 shape)'
# LANE T CONTESTED MY REVERSAL WITH THIS, measured live: #1026 merged on two passes reading
# `Lane: R` and `Lane: S` over one session name, and my base-only key scored it 1. AGENTS.md:403
# blesses that shape. The rule is lane T's: group by base, and split by label only when EVERY body
# in the group declares one.
$subR = "Lane: R | Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] | Head: 9a6b44a2`n`n**APPROVE** - R read the crate."
$subS = "Lane: S | Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] | Head: 9a6b44a2`n`nAPPROVE-WITH-RISK - S read the docs."
$authorT = "Lane: T | Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] | Head: 9a6b44a2"
$r = Get-LivePassCount -PullRequest '1026' -Head $HEAD -AuthorLane 'ignored-when-the-body-speaks' `
    -Invoker (New-Invoker -Comments @((New-Comment $subR), (New-Comment $subS)) -AuthorBody $authorT)
Assert-True ($r.Count -eq 2) "Lane R and Lane S under one session name count 2 (got $($r.Count))"
# AND THE AUTHOR IS EXCLUDED BY LABEL HERE, not by base: all three share the session name, so a
# base-keyed exclusion would drop both real reviewers and score 0.
$selfT = "Lane: T | Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] | Head: 9a6b44a2`n`n**APPROVE** - mine."
$r = Get-LivePassCount -PullRequest '1026' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $subR), (New-Comment $subS), (New-Comment $selfT)) -AuthorBody $authorT)
Assert-True ($r.Count -eq 2) "the author's own Lane T verdict is dropped and R/S survive (got $($r.Count))"
# MIXED SHAPES INSIDE ONE BASE still collapse, which is the case that forced the base to win.
$r = Get-LivePassCount -PullRequest '1026' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $subR), (New-Comment $pass_b_nolabel)) -AuthorBody 'Session: someone-else-aaaaaa')
Assert-True ($r.Count -eq 1) "one labelled and one unlabelled body under one base count once (got $($r.Count))"
# THE DISCRIMINATING ARM. With only ONE labelled member the distinct-label count is also 1, so the
# cell above passes even on `$labelled.Count -gt 0` -- found by that sabotage staying green. TWO
# labelled members plus one unlabelled is the mix that separates "every body declares one" from
# "at least one does": lane T's rule collapses it to a single lane, the loose rule splits it.
$r = Get-LivePassCount -PullRequest '1026' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $subR), (New-Comment $subS), (New-Comment $pass_b_nolabel)) `
                          -AuthorBody 'Session: someone-else-aaaaaa')
Assert-True ($r.Count -eq 1) "R + S + one unlabelled body of the same base collapse to 1, not 2 (got $($r.Count))"

Write-Host 'the AUTHOR lane comes from the PR body, not from the queue entry (enqueuer)'
# FOUND BY LANE T assessing #1023, which measured on #1022 that the two errors cancelled by
# accident. On a takeover the enqueuer is one lane and the author another, and the old code then
# dropped a real reviewer AND let the real author pass their own pull request.
$realAuthor = "Session: typesafe-early-access-82126a | Head: 9a6b44a2"
$authorsOwn = "Session: typesafe-early-access-82126a-4b [490d65] | Head: 9a6b44a2`n`n**APPROVE** - mine, and green."
$enqueuersPass = "Session: unruffled-babbage-df857c-1d [3e1d4c] | Head: 9a6b44a2`n`n**APPROVE** - I enqueued this for them and also read it."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'unruffled-babbage-df857c' `
    -Invoker (New-Invoker -Comments @((New-Comment $authorsOwn), (New-Comment $enqueuersPass)) -AuthorBody $realAuthor)
Assert-True ($r.Count -eq 1) "the enqueuer's pass counts and the real author's does not (got $($r.Count))"
Assert-True ($r.Sessions -join ',' -like '*unruffled-babbage*') "and the surviving pass is the enqueuer's (got '$($r.Sessions -join ',')')"
# THE ENTRY'S LANE IS THE FALLBACK ONLY, for a body with no identity line at all.
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'typesafe-early-access-82126a' `
    -Invoker (New-Invoker -Comments @((New-Comment $authorsOwn)) -AuthorBody 'a body with no identity line')
Assert-True ($r.Count -eq 0) "with no identity in the body the entry's lane still excludes the author (got $($r.Count))"

Write-Host 'the label stops at its own token, whatever separator the identity line uses'
# FOUND BY READING THE SESSIONS LIST, not the count: on live data the label came back as
# "T · SESSION: ... · HEAD: 2FC8B3E8" because the U+00B7 alternative in the old regex never matched
# the bytes in this file. The count was right by luck; the KEY carried the head sha, so one lane
# commenting at two heads would have produced two labels and counted twice.
$dot = [char]0x00b7
$pipeForm = "Lane: T | Session: some-lane-aaaaaa-71 [x] | Head: 9a6b44a2"
$dotForm  = "Lane: T $dot Session: some-lane-aaaaaa-71 [x] $dot Head: 9a6b44a2"
Assert-True ((Get-LaneParts -Body $pipeForm).Label -eq 'T') "the pipe form yields exactly T (got '$((Get-LaneParts -Body $pipeForm).Label)')"
Assert-True ((Get-LaneParts -Body $dotForm).Label -eq 'T') "the U+00B7 form yields exactly T (got '$((Get-LaneParts -Body $dotForm).Label)')"
Assert-True ((Get-LaneParts -Body 'Lane: ISSUES 2 | Session: a-aaaaaa-71').Label -eq 'ISSUES 2') 'a two-word label keeps both words and nothing after'
# THE FALSE QUORUM IT CREATED: one lane, two heads, both reaching the head under the parent rule.
$atHead   = "Lane: T $dot Session: some-lane-aaaaaa-71 [x] $dot Head: 9a6b44a2`n`n**APPROVE** - read at the head."
$atParent = "Lane: T $dot Session: some-lane-aaaaaa-71 [x] $dot Head: cff22ac9`n`nAPPROVE-WITH-RISK - read at the parent."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AlsoAcceptHead $OTHER `
    -Invoker (New-Invoker -Comments @((New-Comment $atHead), (New-Comment $atParent)) -AuthorBody 'Session: someone-else-aaaaaa')
Assert-True ($r.Count -eq 1) "one lane citing two accepted heads counts once, not twice (got $($r.Count))"

Write-Host 'an author that could not be established is reported, not silently trusted'
# LANE T's follow-up, folded in because its direction is false quorum and not undercount: with the
# body unreadable and no fallback, nobody is excluded and the AUTHOR'S OWN verdict scores. It was
# silent too -- Measured stayed true. Unreachable through gate-runner.ps1 today only because that
# caller always supplies $entry.lane, which is a property of the arrangement and not of this code.
$authorsOwnPass = "Session: typesafe-early-access-82126a-4b [490d65] | Head: 9a6b44a2`n`n**APPROVE** - mine."
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD `
    -Invoker (New-Invoker -Comments @((New-Comment $authorsOwnPass)) -FailAuthor)
Assert-True ($r.Measured -eq $false) "an unreadable author body makes Measured false (got $($r.Measured))"
# AND THE FALLBACK STILL ESTABLISHES AN AUTHOR, so a runner supplying the entry's lane is measured.
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD -AuthorLane 'typesafe-early-access-82126a' `
    -Invoker (New-Invoker -Comments @((New-Comment $authorsOwnPass)) -FailAuthor)
Assert-True (($r.Count -eq 0) -and ($r.Measured -eq $true)) `
    "with the entry's lane supplied the author is still excluded and the read is measured (count=$($r.Count), measured=$($r.Measured))"

Write-Host 'a body DISCUSSING an identity line does not declare one'
# FOUND BY THE AUTHOR LANE, on a body that inflated THEIR OWN pull request -- #1141 read 2 with one
# reviewer. `Get-SessionIdentity` was unanchored, so prose mentioning `Session:` produced a capture
# (a backtick, from "the ``Session:`` identity line"), and that body grouped separately from a
# sibling body of the same lane. The quoted-verdict defect, one function away.
$dot = [char]0x00b7
$prose = "Lane: T (subagent) $dot Head: 9a6b44a2`n`n**APPROVE** - read it.`n`nThe body drops the ``Session:`` identity line that AGENTS.md requires."
Assert-True ((Get-SessionIdentity -Body $prose) -eq '') "prose about ``Session:`` yields no identity (got '$(Get-SessionIdentity -Body $prose)')"
# THE ONE-LINE FORM IS STILL IDENTITY. Anchoring to the line start alone rejected it, which lost the
# base for the shape AGENTS.md blesses and re-opened the same inflation through the other door.
$oneLine = "Lane: T $dot Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] $dot Head: 9a6b44a2"
Assert-True ((Get-SessionIdentity -Body $oneLine) -eq 'graphhelm-pr-1099-agent-4a311e-71') `
    "the Lane-prefixed one-line form still declares an identity (got '$(Get-SessionIdentity -Body $oneLine)')"
Assert-True ((Get-SessionIdentity -Body "Session: eloquent-jones-c66bf5-f9 [3d2fc0] | Head: x") -eq 'eloquent-jones-c66bf5-f9') 'the plain form still works'
# AND THE INFLATION ITSELF: one lane, one labelled body and one prose body, counts once.
$labelled = "Lane: T $dot Session: graphhelm-pr-1099-agent-4a311e-71 [b6e8b5] $dot Head: 9a6b44a2`n`nAPPROVE-WITH-RISK - one."
$prosePass = "Lane: T (subagent) $dot Head: 9a6b44a2`n`n**APPROVE** - two.`n`nI note the ``Session:`` line is absent."
$r = Get-LivePassCount -PullRequest '1141' -Head $HEAD -AuthorLane 'x' `
    -Invoker (New-Invoker -Comments @((New-Comment $labelled), (New-Comment $prosePass)) -AuthorBody 'Session: someone-else-aaaaaa')
Assert-True ($r.Count -eq 1) "one lane writing both shapes counts once, not twice (got $($r.Count))"

Write-Host 'a QUOTED identity line is somebody else's'
# AUTHOR LANE's residual on the anchoring fix, and my own inconsistency: `>` was in the identity
# prefix class while the verdict class had already dropped it for the same reason. `Match` takes
# the FIRST hit, so a reviewer who quotes the author's identity line above their own had their pass
# attributed to the author and DROPPED. Deflation, not inflation -- a lost pass rather than a fake
# quorum -- which is why it was a risk and not a block.
$quoting = "> Session: typesafe-early-access-82126a | Head: 9a6b44a2`n> (quoting the author)`n`nSession: eloquent-jones-c66bf5-f9 [3d2fc0] | Head: 9a6b44a2`n`n**APPROVE** - mine."
Assert-True ((Get-SessionIdentity -Body $quoting) -eq 'eloquent-jones-c66bf5-f9') `
    "a reviewer quoting the author is still themselves (got '$(Get-SessionIdentity -Body $quoting)')"
$dot = [char]0x00b7
Assert-True ((Get-LaneParts -Body ("> Lane: T $dot Session: x-aaaaaa-71")).Label -eq '') 'a quoted Lane label declares nothing'
# AND THE CONSEQUENCE: the quoting reviewer's pass counts, and is not dropped as the author's.
$r = Get-LivePassCount -PullRequest '1125' -Head $HEAD `
    -Invoker (New-Invoker -Comments @((New-Comment $quoting)) -AuthorBody 'Session: typesafe-early-access-82126a')
Assert-True ($r.Count -eq 1) "the quoting reviewer's pass survives the author exclusion (got $($r.Count))"

Write-Host 'the name capture is bounded by shape, and that bound is load-bearing'
# LANE T's finding: the bounded capture carried six lines of rationale and no cell. Restoring the
# old unbounded form while KEEPING the anchor left the whole suite green -- rationale is not a
# guard, which is the argument this entire file makes about other people's code.
#
# What the bound refuses is a live shape: session names like `/root/pr_triage` appear on real
# passes in this repository, and refusing them is what makes such an author UNKNOWN rather than
# silently keyed on a path. The bound is therefore the thing standing between a path-shaped name
# and a lane key, and it needs a cell in both directions.
Assert-True ((Get-SessionIdentity -Body 'Session: /root/pr_triage | Head: x') -eq '') `
    "a path-shaped session name is refused, not captured (got '$(Get-SessionIdentity -Body 'Session: /root/pr_triage | Head: x')')"
Assert-True ((Get-SessionIdentity -Body 'Session: /root | Head: x') -eq '') 'and a bare path too'
# CONTROLS, which matter more than the refusals: a tightening that refused everything would pass
# both lines above.
Assert-True ((Get-SessionIdentity -Body 'Session: eloquent-jones-c66bf5-f9 [3d2fc0] | Head: x') -eq 'eloquent-jones-c66bf5-f9') 'CONTROL: a real name is still captured'
Assert-True ((Get-SessionIdentity -Body 'Session: 12ab34 | Head: x') -eq '12ab34') 'CONTROL: a digit-leading name is still captured'
Assert-True ((Get-SessionIdentity -Body 'Session: a.b_c-d | Head: x') -eq 'a.b_c-d') 'CONTROL: dot, underscore and hyphen are part of a name'

Write-Host 'a head shorter than 8 characters is refused rather than prefix-matched'
$r = Get-LivePassCount -PullRequest '1125' -Head '9a6b' -AuthorLane 'x' -Invoker (New-Invoker -Comments @((New-Comment $pass_a)))
Assert-True ($r.Count -eq 0) "a 4-character head cannot be matched and scores 0 (got $($r.Count))"

Write-Host ''
if ($script:Failures -gt 0) { Write-Host "FAILED: $($script:Failures) assertion(s)"; exit 1 }
Write-Host 'all pass-predicate cells green'
exit 0
