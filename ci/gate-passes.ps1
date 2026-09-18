<#
.SYNOPSIS
    Counts the LIVE review passes on a pull request, for the queue ordering in `gate-runner.ps1` (#1133).

.DESCRIPTION
    WHY THIS IS NOT `pulls/N/reviews --jq length`. That was the previous scorer, and it counted
    review OBJECTS. The variable was named `Passes` and the ordering comment said "a pull request
    with two passes is the one whose gate result someone is waiting on", but the number answered a
    different question, in four separate ways:

      * VERDICT. An empty body, a question, or a `CHANGES_REQUESTED` scored the same as a pass.
      * HEAD. A review against a head rebased away a week ago scored forever. The count only rose;
        it never decayed when a rebase invalidated every pass on the branch.
      * AUTHOR. Every session in this factory writes as `stabem`, so the API cannot say who wrote
        what, and a pull request could raise its own priority with no reviewer involved.
      * SURFACE. This repository's protocol puts a pass in an issue COMMENT -- GitHub state is
        always COMMENTED and the verdict is the comment text (`AGENTS.md`). `pulls/N/reviews` is a
        surface our passes largely do not write to; `.factory/tools/census.sh` calls it "the surface
        never read".

    Measured on the live queue 2026-09-17: #1125, with two passes at its current head from two
    lanes and next in line to merge, sat third behind two pull requests scoring 26 and 20 -- review
    objects accumulated across many heads over their lifetimes, not live passes.

    WHAT A PASS IS HERE. The same predicate the merge checklist uses: a verdict word, plus a sha
    that reaches the head being gated, from a lane that is not the author. So this counts DISTINCT
    reviewer sessions whose body carries both a verdict word and the entry's head, excluding the
    entry's own `lane`. It decays by construction: move the head and yesterday's passes stop
    counting, with no bookkeeping anywhere.

    WHAT IT DELIBERATELY DOES NOT DO. It does not decide whether a gate may run, and it must not
    grow into that. It orders a queue. Every entry still runs, age still breaks ties so nothing
    starves, and a pull request scoring zero is delayed, never refused. Ordering is the only
    consequence, which is why a residual over-count (below) costs a queue position and not a merge.
#>

Set-StrictMode -Version Latest

# The verdict vocabulary is CLOSED and ordered longest-first, so `APPROVE-WITH-RISK` is never
# reported as the `APPROVE` inside it. Adding a word here is a protocol change, not a tidy-up.
$script:VerdictWords = @('APPROVE-WITH-RISK', 'BLOCK', 'APPROVE')

# A verdict must appear in the OPENING of the body, where the protocol puts it (identity line
# first, then the verdict word). A body that merely discusses verdicts further down -- a reviewer
# writing "my APPROVE no longer reaches the head", which is a real message that was sent on #1125 --
# must not score. This is the residual: a body whose first lines QUOTE a verdict still counts. It
# costs a queue position, and the alternative (parsing prose for intent) is a worse instrument.
$script:VerdictWindowLines = 6

function Get-SessionIdentity {
    <#
        Reads the session name out of an identity line. Both shapes in use:
            Session: <name> [ref] | Head: <sha8>
            Lane: T · Session: <name> [ref] · Head: <sha8>
        Returns '' when the body carries no identity line at all, which is itself disqualifying:
        an anonymous body cannot be attributed to a lane, so it cannot be one of the two passes.
    #>
    param([string] $Body)
    if ([string]::IsNullOrWhiteSpace($Body)) { return '' }
    # ANCHORED TO A LINE, AND BOUNDED BY SHAPE. Unanchored, this read any `Session:` anywhere in a
    # body -- including PROSE ABOUT identity. Measured on #1141: lane T's pass opens `Lane: T ...`
    # with no identity line and later writes "the `Session:` identity line", so the capture was a
    # BACKTICK. That body grouped under Base='`' while a sibling body of the same lane grouped under
    # its label: one reviewer, two groups, a count of 2 where one lane had passed. It is the
    # quoted-verdict defect one function away -- a body DISCUSSING an identity read as declaring
    # one -- and it inflates, which is the direction that matters.
    #
    # The name's SHAPE ends the capture rather than a separator character. The identity line uses
    # U+00B7 between fields, and a non-ASCII character inside a regex class in this file has already
    # failed silently once against the reading codepage. A name is letters, digits, dot, underscore
    # and hyphen; anything else ends it, whatever punctuation follows.
    # THE `Lane: X <sep> Session: Y` ONE-LINE FORM IS IDENTITY TOO, and anchoring to the line start
    # alone rejected it -- the shape AGENTS.md blesses and the one lane T writes. Losing the base
    # there re-opened the very inflation this anchor closes, through my own door: the same lane
    # would group under its label on one body and its base on another. A BOUNDED `Lane:` prefix is
    # allowed before `Session:`; forty characters is room for a label and a separator and not room
    # for a sentence.
    # A BLOCKQUOTED IDENTITY IS SOMEBODY ELSE'S, and `>` was in this prefix class while the verdict
    # class had already dropped it for the identical reason -- my own inconsistency, found by the
    # author lane. `Match` takes the FIRST hit, so a reviewer who quotes the author's identity line
    # above their own had their pass attributed to the author and dropped. Deflation rather than
    # inflation, which is why it was a risk and not a block, and one character to close.
    $match = [regex]::Match($Body, '(?m)^[\s*_#-]*(?:Lane:.{0,40}?)?Session:[\s]*([A-Za-z0-9][A-Za-z0-9._-]*)')
    if (-not $match.Success) { return '' }
    return $match.Groups[1].Value.Trim()
}

function Test-CarriesVerdict {
    <#
        A VERDICT IS A LINE, NOT A WORD SOMEWHERE IN A PARAGRAPH. The first version of this only
        required the word inside the opening lines, and pinned a cell for a quoted verdict at line
        SEVEN -- a shape that does not occur. The shape that DOES occur, named in review by the
        presser lane: a dead-citation notice whose second line reads "your APPROVE no longer reaches
        this head". Under a window-only rule that inflated exactly the pull requests a presser is
        triaging, using the presser's own traffic to do it.

        So the word must OPEN a line, after nothing but whitespace and markdown emphasis, which is
        where every pass in this repository puts it (`**APPROVE** - ...`, `APPROVE-WITH-RISK - ...`).
        A verdict inside a sentence is prose about a verdict, wherever it sits.
    #>
    param([string] $Body)
    if ([string]::IsNullOrWhiteSpace($Body)) { return $false }
    $lines = @($Body -split "`r?`n" | Select-Object -First $script:VerdictWindowLines)
    # A FENCED BLOCK IS QUOTED TEXT, even when the verdict is alone on its own line inside it.
    # Lane T's residual: the line-anchor rule closed `> APPROVE` and `` `APPROVE` `` but not a
    # bare APPROVE between ``` fences. Same defect, third door.
    $fenced = $false
    foreach ($line in $lines) {
        if ($line -match '^\s*(```|~~~)') { $fenced = -not $fenced; continue }
        if ($fenced) { continue }
        foreach ($word in $script:VerdictWords) {
            # Case-SENSITIVE: the protocol writes the verdict in capitals, and a case-insensitive
            # match would score the word "approve" in an English sentence. The only thing allowed
            # before it is emphasis punctuation; the only thing allowed after is a non-letter.
            # THE PREFIX CLASS IS NARROW ON PURPOSE. `>` and a backtick were in it and lane T
            # refuted them: a BLOCKQUOTED verdict is somebody quoting another lane's pass, and a
            # code-fenced one is a verdict being discussed as text. Both are the quoted-verdict
            # defect returning through a different door -- the trap you named coming back in
            # another state. Only emphasis and list punctuation remain.
            if ($line -cmatch ('^[\s*_#-]*' + [regex]::Escape($word) + '(?![A-Za-z-])')) { return $true }
        }
    }
    return $false
}

function Test-NamesHead {
    <#
        A pass names the sha it measured. Comments cite the short form, the entry carries the full
        40 characters, so the comparison is on the first 8 -- the same width every pass in this
        repository prints. Shorter than 8 is not accepted: a 4-character prefix collides.
    #>
    param([string] $Body, [string] $Head)
    if ([string]::IsNullOrWhiteSpace($Body)) { return $false }
    if ([string]::IsNullOrWhiteSpace($Head) -or $Head.Length -lt 8) { return $false }
    $short = $Head.Substring(0, 8)
    return ($Body -match ('(?<![0-9a-fA-F])' + [regex]::Escape($short)))
}

function Get-LaneParts {
    <#
        A lane has TWO coordinates in this factory and neither alone identifies it:

          Base   the session name with a recycle suffix dropped -- stable across recycles, and
                 SHARED by every subagent of one session.
          Label  a declared `Lane:` letter -- what tells subagents of one session apart, absent on
                 most bodies.

        `AGENTS.md:403` blesses the shape that forced this: two lanes, one session name, told apart
        only by the label. Lane T measured the cost of collapsing it -- #1026 merged on two passes
        (`Lane: R` and `Lane: S`, one session name) and my base-only key scored it 1.

        So the pair is returned and the CALLER picks the coordinate that answers its question:
        grouping uses the base, telling subagents apart uses the label, and excluding the author
        uses the label when both sides declare one and the base otherwise.
    #>
    param([string] $Body, [string] $Name = '')
    if ($Body -and -not $Name) { $Name = Get-SessionIdentity -Body $Body }
    $label = ''
    if ($Body) {
        # THE LABEL'S SHAPE IS BOUNDED; THE SEPARATOR IS NOT TRUSTED. The first version ended the
        # capture at `|` or the U+00B7 the identity line uses, and the non-ASCII alternative never
        # matched -- the script's bytes and the reading codepage disagree, silently, the way
        # encoding damage always does. The label then swallowed the rest of the line INCLUDING the
        # head sha, so one lane commenting at two heads produced two labels and counted twice: a
        # false quorum out of a character nobody can see. A letter run with an optional number
        # covers every observed label (T, R, S, `ISSUES 2`) and cannot run past it.
        $match = [regex]::Match($Body, '(?m)^[\s*_#-]*Lane:[\s]*([A-Za-z]+(?:[ ][0-9]+)?|[0-9]+)(?![A-Za-z0-9])')
        if ($match.Success) { $label = ($match.Groups[1].Value.Trim() -replace '\s+', ' ').ToUpperInvariant() }
    }
    $base = ''
    if (-not [string]::IsNullOrWhiteSpace($Name)) {
        $base = $Name
        $cut = $Name.LastIndexOf('-')
        if ($cut -gt 0) {
            $stem = $Name.Substring(0, $cut)
            $suffix = $Name.Substring($cut + 1)
            if ($stem -cmatch '-[0-9a-f]{6}$' -and $suffix -cmatch '^[0-9a-f]{2}$') { $base = $stem }
        }
    }
    return [pscustomobject]@{ Base = $base; Label = $label }
}

function Get-AuthorLaneParts {
    <#
        THE AUTHOR IS NOT THE ENQUEUER, and this file excluded the wrong one. A queue entry's
        `lane` field is whoever ran `ci/gate-queue.ps1`; the checklist predicate excludes the lane
        whose identity line opens the PULL REQUEST BODY. On a takeover -- one lane enqueueing
        another lane's branch -- or when a presser enqueues, those differ, and the two errors then
        run in OPPOSITE directions: a legitimate reviewer is silently dropped, AND the real author
        can pass their own pull request. The second is a false quorum, which is the harm this file
        exists to remove.

        Found by lane T while assessing #1023, which had measured on #1022 that the two errors
        cancelled by accident. The body is read here; `$Fallback` -- the entry's lane -- is used
        only when the body carries no identity line at all.
    #>
    param(
        [Parameter(Mandatory)] [string] $PullRequest,
        [string] $Fallback = '',
        [string] $Repository = 'stabem/GraphHelm',
        [scriptblock] $Invoker
    )
    if (-not $Invoker) { $Invoker = { param($file, $arguments) Invoke-External $file $arguments } }
    $probe = & $Invoker 'gh' @('api', "repos/$Repository/pulls/$PullRequest", '--jq', '.body')
    if ($probe -and $probe.Code -eq 0 -and $probe.Output.Count -gt 0) {
        $body = ($probe.Output -join "`n")
        $parts = Get-LaneParts -Body $body
        if ($parts.Base -or $parts.Label) {
            return [pscustomobject]@{ Base = $parts.Base; Label = $parts.Label; Known = $true }
        }
    }
    # NOT KNOWING WHO THE AUTHOR IS FAILS TOWARD SAYING SO, never toward trusting the count. If the
    # body could not be read and the caller supplied no fallback, NOBODY is excluded -- and the
    # author's own verdict then scores, which is the false-quorum direction. Lane T found that it
    # was silent as well: `Measured` stayed true, so a caller could not tell an unexcluded author
    # from a genuinely unreviewed pull request. Unreachable through `gate-runner.ps1` today, because
    # it always supplies `$entry.lane` -- and unreachable-today is a property of the arrangement,
    # not of this function.
    $fromFallback = Get-LaneParts -Name $Fallback
    return [pscustomobject]@{
        Base  = $fromFallback.Base
        Label = $fromFallback.Label
        Known = [bool]($fromFallback.Base -or $fromFallback.Label)
    }
}

function Get-LaneKey {
    <#
        One string for the pair, for callers that only need identity equality: the base when there
        is one, else the declared label. `Get-LaneParts` is what the counting uses, because
        grouping and label-splitting need the two coordinates separately.
    #>
    param([string] $Body, [string] $Name = '')
    $parts = Get-LaneParts -Body $Body -Name $Name
    if ($parts.Base) { return $parts.Base }
    if ($parts.Label) { return 'lane:' + $parts.Label }
    return ''
}

function Get-ManifestOnlyParent {
    <#
    .SYNOPSIS
        The parent of $Sha when $Sha changes NOTHING but gate manifests; '' otherwise.

    .DESCRIPTION
        A gate run commits its receipt to `.factory/gate-runs/` and moves the head. Every pass on
        that branch names the head the reviewers read, which is now the PARENT -- so a pull request
        that has just been gated scores zero passes and loses the queue position it earned, which is
        the exact regression this file exists to remove. Reachable in practice through a re-gate
        after a flake.

        `.factory/MERGE-CHECKLIST.md` already answers this for the merge decision: a pass reaches
        the head when `headSha == head` OR `headSha == parent(head)` with a manifest-only tip. This
        is that rule, in the one place the queue asks the same question.

        MEASURED 2026-09-17 on #1125: at `279cffd0` (tip `.factory/gate-runs/9a6b44a27a18-...json`,
        parent `9a6b44a2`) the count was 0; at `9a6b44a2` it was 2. Found in review by the epic's
        author lane, not by me -- I had applied the parent rule to my own pass on that very pull
        request and not to this code.

        AN EMPTY FILE LIST IS NOT MANIFEST-ONLY. `all()` over nothing is true, and answering "yes,
        manifest-only" for a commit whose files could not be read would extend trust to the parent
        on no evidence. It fails toward the narrow answer: the head itself.
    #>
    param(
        [Parameter(Mandatory)] [string] $Sha,
        [string] $Repository = 'stabem/GraphHelm',
        [scriptblock] $Invoker
    )
    if (-not $Invoker) { $Invoker = { param($file, $arguments) Invoke-External $file $arguments } }
    if ([string]::IsNullOrWhiteSpace($Sha)) { return '' }
    $probe = & $Invoker 'gh' @('api', "repos/$Repository/commits/$Sha")
    if (-not $probe -or $probe.Code -ne 0 -or $probe.Output.Count -eq 0) { return '' }
    $commit = $null
    try { $commit = (($probe.Output -join "`n")) | ConvertFrom-Json } catch { return '' }
    # `$null -eq` IS NOT ENOUGH. `'[]' | ConvertFrom-Json` yields an EMPTY COLLECTION, not $null, in
    # Windows PowerShell 5.1, and piping that into `Get-Member` throws "You must specify an object".
    # A surface answering with something that is not a commit object must read as "no parent", not
    # as a crash -- this is a scorer, and a throw here would take the whole queue ordering with it.
    if ($null -eq $commit -or @($commit).Count -eq 0) { return '' }
    $commit = @($commit)[0]
    if ($null -eq $commit -or -not ($commit -is [psobject])) { return '' }
    $files = @()
    if ($commit | Get-Member -Name 'files' -MemberType NoteProperty) {
        $files = @($commit.files | ForEach-Object { [string] $_.filename })
    }
    if ($files.Count -eq 0) { return '' }
    foreach ($file in $files) {
        if (-not $file.StartsWith('.factory/gate-runs/', [System.StringComparison]::Ordinal)) { return '' }
    }
    # EXACTLY ONE PARENT. A merge commit is not a manifest commit, and accepting a pass from one of
    # two parents would be a guess about which side the reviewers read.
    $parents = @()
    if ($commit | Get-Member -Name 'parents' -MemberType NoteProperty) { $parents = @($commit.parents) }
    if ($parents.Count -ne 1) { return '' }
    return [string] $parents[0].sha
}

function Test-SameLane {
    <#
        Is $Session the lane named by $AuthorLane? A lane's session name gains a per-recycle suffix
        -- `typesafe-early-access-82126a` enqueues and `typesafe-early-access-82126a-4b` comments --
        so a plain equality misses the author and lets them pass their own pull request.

        THE BOUNDARY IS LOAD-BEARING, and the first version of this had no boundary at all. It asked
        `$AuthorLane -like "$Session*"`, which for an empty $Session becomes `-like "*"` and matches
        every author on earth. That was found by a sabotage: removing the identity requirement
        reddened nothing, because this test was silently swallowing every anonymous body. It cost
        two defects at once -- the identity guard looked load-bearing and was not, and a reviewer
        whose name is a bare prefix of the author's (`t` against `typesafe-...`) had their pass
        dropped as if they were the author.

        So: neither side may be empty, and a prefix only counts when it ends on a `-` separator.
    #>
    param([string] $Session, [string] $AuthorLane)
    if ([string]::IsNullOrWhiteSpace($Session) -or [string]::IsNullOrWhiteSpace($AuthorLane)) { return $false }
    if ($Session -eq $AuthorLane) { return $true }
    # Compare LANE KEYS, so a recycle suffix on either side does not hide the author from itself.
    if ((Get-LaneKey -Name $Session) -eq (Get-LaneKey -Name $AuthorLane)) { return $true }
    foreach ($pair in @(@($Session, $AuthorLane), @($AuthorLane, $Session))) {
        $longer = $pair[0]; $shorter = $pair[1]
        if ($longer.Length -gt $shorter.Length -and $longer.StartsWith($shorter, [System.StringComparison]::Ordinal)) {
            if ($longer.Substring($shorter.Length).StartsWith('-')) { return $true }
        }
    }
    return $false
}

function Get-LivePassCount {
    <#
    .SYNOPSIS
        Distinct reviewer sessions holding a live pass on $PullRequest at $Head.

    .OUTPUTS
        [pscustomobject] with Count, Sessions, and Measured. MEASURED IS NOT COSMETIC: a zero from
        a `gh` failure and a zero from a pull request nobody has reviewed are different facts, and
        the caller logs them differently. The previous scorer could not tell them apart -- an API
        blip read as "no passes" and silently cost that entry its queue position.
    #>
    param(
        [Parameter(Mandatory)] [string] $PullRequest,
        [Parameter(Mandatory)] [string] $Head,
        [string] $AuthorLane = '',
        # A SECOND HEAD A PASS MAY NAME, for the manifest-only-tip rule. OMIT IT AND THIS RESOLVES
        # IT ITSELF; pass it (even as '') to control it, which is what the cells do.
        #
        # The first version required the CALLER to compose this with `Get-ManifestOnlyParent`, so
        # the rule would be visible at the call site. Wrong trade, and the author lane caught it by
        # measuring the module directly: a caller who skipped the composition read 0 for a
        # just-gated pull request and had no way to learn the argument existed. Visibility for one
        # reader is not worth a default that is wrong for every other caller -- and that is the same
        # defect class as the scorer this file replaces, safe behaviour depending on somebody
        # remembering. The safe thing is now what happens when you ask for nothing.
        [string] $AlsoAcceptHead = '',
        [string] $Repository = 'stabem/GraphHelm',
        [scriptblock] $Invoker
    )
    if (-not $Invoker) { $Invoker = { param($file, $arguments) Invoke-External $file $arguments } }
    # `ContainsKey` rather than `-not $AlsoAcceptHead`: a caller who deliberately passes '' is
    # asking for the head alone and must not be silently overridden. Only ABSENCE looks it up.
    if (-not $PSBoundParameters.ContainsKey('AlsoAcceptHead')) {
        $AlsoAcceptHead = Get-ManifestOnlyParent -Sha $Head -Repository $Repository -Invoker $Invoker
    }

    $bodies = New-Object System.Collections.Generic.List[string]
    $measured = $true
    # BOTH SURFACES. Comments are where the protocol puts a pass; reviews are where some lanes also
    # post one, and a pointer with no verdict word scores nothing on either, by design.
    foreach ($path in @("repos/$Repository/issues/$PullRequest/comments", "repos/$Repository/pulls/$PullRequest/reviews")) {
        $probe = & $Invoker 'gh' @('api', $path, '--paginate')
        if (-not $probe -or $probe.Code -ne 0 -or $probe.Output.Count -eq 0) { $measured = $false; continue }
        $parsed = $null
        try { $parsed = (($probe.Output -join "`n")) | ConvertFrom-Json } catch { $measured = $false; continue }
        if ($null -eq $parsed) { $measured = $false; continue }
        foreach ($item in @($parsed)) {
            if ($item -and ($item | Get-Member -Name 'body' -MemberType NoteProperty)) {
                $bodies.Add([string] $item.body)
            }
        }
    }

    # WHO THE AUTHOR IS, read from the pull request body rather than taken from the entry's
    # `lane` (the enqueuer). See Get-AuthorLaneParts for why the two differ and why the difference
    # points at a false quorum.
    $author = Get-AuthorLaneParts -PullRequest $PullRequest -Fallback $AuthorLane -Repository $Repository -Invoker $Invoker
    # An unknown author is a hole in the measurement, not a licence to count everything.
    if (-not $author.Known) { $measured = $false }

    # GROUP BY BASE, SPLIT BY LABEL ONLY WHEN THE WHOLE GROUP DECLARES ONE (lane T's rule, exact on
    # all three live cases it was derived from). Two subagent lanes of one session declare `Lane: R`
    # and `Lane: S` and must count twice; one lane that writes a label on some comments and not on
    # others must count once. "Every body in the group has a label" is what separates those, and it
    # is why the decision cannot be made one body at a time.
    $groups = @{}
    $qualifying = New-Object System.Collections.Generic.List[psobject]
    foreach ($body in $bodies) {
        if (-not (Test-CarriesVerdict -Body $body)) { continue }
        $namesHead = (Test-NamesHead -Body $body -Head $Head)
        if (-not $namesHead -and $AlsoAcceptHead) {
            $namesHead = (Test-NamesHead -Body $body -Head $AlsoAcceptHead)
        }
        if (-not $namesHead) { continue }
        $parts = Get-LaneParts -Body $body
        if (-not $parts.Base -and -not $parts.Label) { continue }

        # THE AUTHOR IS EXCLUDED BY LABEL WHEN BOTH SIDES DECLARE ONE, and by base otherwise.
        # On #1026 the author is `Lane: T` and the two passes are `Lane: R`/`Lane: S` under the SAME
        # session name -- excluding by base there would drop both real reviewers.
        if ($parts.Label -and $author.Label) {
            if ($parts.Label -eq $author.Label) { continue }
        } elseif ($parts.Base -and $author.Base -and (Test-SameLane -Session $parts.Base -AuthorLane $author.Base)) {
            continue
        }

        $qualifying.Add($parts)
    }

    # A LABEL-ONLY BODY BELONGS TO THE BASE ITS OWN LANE DECLARES ELSEWHERE. Anchoring the identity
    # stopped prose from inventing a base, and left the other half: a lane that writes
    # `Lane: T <sep> Session: X` on one comment and `Lane: T` alone on another still grouped under
    # two keys -- the base and the label -- and counted twice. One reviewer, two groups, which is
    # the inflation direction. Bodies that carry BOTH coordinates teach the map; label-only bodies
    # are resolved through it, and a label with no base anywhere keeps its own key.
    $labelToBase = @{}
    foreach ($parts in $qualifying) {
        if ($parts.Base -and $parts.Label -and -not $labelToBase.ContainsKey($parts.Label)) {
            $labelToBase[$parts.Label] = $parts.Base
        }
    }
    foreach ($parts in $qualifying) {
        $base = $parts.Base
        if (-not $base -and $parts.Label -and $labelToBase.ContainsKey($parts.Label)) {
            $base = $labelToBase[$parts.Label]
        }
        $groupKey = if ($base) { $base } else { 'lane:' + $parts.Label }
        if (-not $groups.ContainsKey($groupKey)) { $groups[$groupKey] = New-Object System.Collections.Generic.List[psobject] }
        $groups[$groupKey].Add($parts)
    }

    $sessions = New-Object System.Collections.Generic.List[string]
    foreach ($groupKey in $groups.Keys) {
        $members = @($groups[$groupKey])
        $labelled = @($members | Where-Object { $_.Label })
        if ($labelled.Count -eq $members.Count) {
            foreach ($label in @($labelled | ForEach-Object { $_.Label } | Select-Object -Unique)) {
                $sessions.Add($groupKey + '/' + $label)
            }
        } else {
            $sessions.Add($groupKey)
        }
    }

    return [pscustomobject]@{
        Count    = $sessions.Count
        Sessions = @($sessions)
        Measured = $measured
    }
}
