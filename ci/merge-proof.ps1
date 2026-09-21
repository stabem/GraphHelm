<#
.SYNOPSIS
    Does a recorded gate run vouch for the head this pull request would merge? (#674(b))

.DESCRIPTION
    #674 measured that no commit on main can be traced back to the run that gated it: the ledger is
    dense and honest about runs, and a squash merge throws away the commit each run names. (a) --
    #696 -- makes the gate record `headSha`, the pull request number and `pushed`, and commit that
    record. This is the half that READS it, and it exists in the repository rather than in one
    person's script for a reason:

    A RULE THAT LIVES IN SOMEBODY'S PRIVATE SCANNER IS NOT A RULE THE REPOSITORY HAS. It cannot be
    run by whoever is holding the button, it cannot be sabotaged, and it rots without anyone
    learning that it did.

    THE PREDICATE, whole:

        SATISFIED  a manifest for this pull request exists such that
                     status is GREEN
                     AND pushed == true
                     AND pullRequest == N
                     AND (   headSha == head
                          OR (    headSha == parent(head)
                              AND every path in the tip commit is under .factory/gate-runs/ ) )
        NOT        a manifest was found and one of those fails -- and the output names WHICH
        ABSENT     no manifest for this pull request exists at all

    EQUALITY THROUGHOUT, NEVER `is-ancestor`. An ancestor test says the remote holds some commit
    this one descends from, which is true of every unpushed commit on a tracked branch -- exactly
    the state (a)'s `pushed` field exists to detect.

    THE EXCEPTION TO `headSha == head` IS EARNED BY THE SECOND CONDITION AND BY NOTHING ELSE. The
    commit that records a run becomes the head, so a manifest can only ever name the parent; that
    is legitimate precisely when the tip added nothing but the record, which is why the manifest
    commit in (a) is pure. A tip that carries anything else is not a head this rule may vouch for.

    NOT AND ABSENT ARE DIFFERENT ANSWERS AND KEEP DIFFERENT EXIT CODES. "A run was recorded and it
    does not vouch for this head" and "nothing recorded anything" call for opposite actions --
    investigate a failure, or run the gate -- and collapsing them is the ambiguity this whole
    ticket exists to remove.

    ADVISORY UNTIL (a) IS ON MAIN, and the mode is READ, never set. Before (a) lands no branch can
    produce the artefact, so a blocking check would refuse every merge and be switched off -- which
    is #658's thesis and the hazard the owner's ordering was chosen against. The mode is derived
    from the repository itself: whether `Get-HeadProvenance` is present in `ci/gate.ps1`. A
    hand-edited flag would be a claim nobody re-derives.

.PARAMETER PullRequest
    The pull request number to judge.

.PARAMETER Head
    The head commit the merge would take, BEFORE the squash. Looked up with `gh` when omitted.

.PARAMETER RepositoryRoot
    Defaults to the repository this script sits in.

.EXAMPLE
    NOT the supported invocation, and it is here to be recognised rather than copied. Run from the
    candidate's own checkout, THE PULL REQUEST SUPPLIES THE PREDICATE THAT JUDGES IT: replacing
    this file with `exit 0` certifies anything, and the self-check below is removed by the same
    edit (#733). Use it only against a checkout you already trust -- your own branch, before you
    ask anyone else to look.

    powershell -NoProfile -ExecutionPolicy Bypass -File ci/merge-proof.ps1 -PullRequest 696

.EXAMPLE
    THE SUPPORTED INVOCATION: the file executed is one the candidate cannot edit. See
    `ci/merge-proof-from-main.ps1`, which does this and additionally VERIFIES that the extracted
    bytes are main's rather than assuming the extraction was faithful.

    git fetch --quiet origin main
    git worktree add --detach --quiet $env:TEMP\mp-main origin/main
    powershell -NoProfile -ExecutionPolicy Bypass -File $env:TEMP\mp-main\ci\merge-proof-from-main.ps1 -PullRequest 696 -RepositoryRoot .
    git worktree remove --force $env:TEMP\mp-main

    NOT `git show ... > file`: on Windows PowerShell 5.1 `>` is `Out-File` and re-encodes, which
    produces a runner that is not main's bytes and still parses (#811 review).
#>
param(
    [Parameter(Mandatory)] [int] $PullRequest,
    [string] $Head,
    [string] $RepositoryRoot,
    # For the caller that pastes this into a merge comment: the same verdict as a machine record,
    # so nothing downstream has to re-parse prose that was written for a person.
    [switch] $Json,
    # Where the slot's ledger lives. Defaulted rather than derived, and reported when it is not
    # there, because the derived form silently pointed at a drive with no such directory.
    [string] $LedgerDirectory,
    # How much of an untrusted tip's file list this is willing to read. A seam, not a knob: the
    # cell that proves overflow fails closed cannot commit five thousand files.
    [int] $MaxTipEntries = 5000,
    # How many records this will read from either store. See the note where the default is applied.
    [int] $MaxManifests,
    # The TOTAL this will read from the store, across every manifest. A seam for the same reason the
    # others are: the cell that proves the aggregate ceiling refuses cannot write two gigabytes.
    [long] $MaxStoreBytes = 64MB
)

$ErrorActionPreference = 'Continue'

# 0 SATISFIED, 2 NOT, 3 ABSENT, 1 the instrument could not answer. Three verdicts and a
# harness-broke, exactly as the suites in this directory report, so a caller can branch on the code
# and never on the prose. This line said 1 NOT and 2 harness-broke while the constants below said
# the opposite, and the paragraph under it has always been right -- a comment is a claim, and this
# one cost two cells that asserted the wrong number against correct behaviour.
# The consumer's scheme, agreed with the desk that will call this: 1 is reserved for THE TOOL
# BROKE, so it can never be confused with a verdict. A caller that treats "non-zero" as "refused"
# would otherwise refuse a merge because this script could not run, which is the failure mode the
# whole ticket is about.
$ExitSatisfied = 0
$ExitHarnessBroke = 1
$ExitNot = 2
$ExitAbsent = 3
$StorePrefix = '.factory/gate-runs/'
# A seam, not a knob: the cell that proves a truncated ledger scan SAYS it was truncated cannot
# write two thousand files, and a limit whose overflow nothing exercises is a limit nobody has seen
# work. Defaulted to the real value everywhere else.
if (-not $PSBoundParameters.ContainsKey('MaxManifests')) { $MaxManifests = 2000 }
$MaxManifestBytes = 1MB

# Set by the ledger scan when a record it READ agrees with the verdict: GREEN and pushed, for this
# head. Separate from the list of what the scan saw, because "we found records" and "a record backs
# this" are different claims and only the second is a witness.
$script:supportingWitness = $false
$script:facts = [ordered]@{
    state = $null; reason = $null; headSha = $null; matched = $null
    tip = @(); otherRecords = @(); scanNotes = @(); self = $null; mode = $null; modeReason = $null; corroboration = @(); disagreement = @(); unreadable = @()
}

function Test-SameText {
    <#
        ORDINAL, because `-ceq` IS NOT. PowerShell's case-sensitive operators are still CULTURE
        aware, and a culture comparison gives some code points no weight at all: measured here,
        'GREEN' plus U+FFFD, U+FE00 or U+00AD all come back -ceq 'GREEN'. Every equality in this
        file decides something -- status is GREEN, headSha is the head, this verifier is the one at
        origin/main -- and each of them was deciding it with a comparer that treats different
        strings as the same string, always in the direction of SATISFIED.

        The category guard in Read-ManifestField cannot cover this: a variation selector is Mn, it
        is legitimate in ordinary text, and refusing it would be a deny-list again. The answer is
        that the COMPARISON stops being approximate, not that the alphabet keeps shrinking.

        Null is not the empty string here: `[string] $null` is '', so a typed parameter would make
        an absent value equal to an empty one. The two nulls are answered before any cast.
    #>
    param(
        [Parameter(Position = 0)] [AllowNull()] [object] $Left,
        [Parameter(Position = 1)] [AllowNull()] [object] $Right
    )
    if ($null -eq $Left -or $null -eq $Right) { return ($null -eq $Left -and $null -eq $Right) }
    return [string]::Equals([string]$Left, [string]$Right, [System.StringComparison]::Ordinal)
}

function Test-NameIsPresent {
    <# Same reason as Test-SameText, for the property-name lookups. #>
    param([Parameter(Mandatory)] [object] $Object, [Parameter(Mandatory)] [string] $Name)
    return @($Object.PSObject.Properties.Name | Where-Object { Test-SameText $_ $Name }).Count -gt 0
}

function Write-Broke {
    <#
        HARNESS-BROKE, in whichever shape the caller asked for. In `-Json` mode a Write-Host line
        before the object makes the output stop being a single JSON document -- a caller piping it
        straight into a parser gets a syntax error instead of a state it could branch on, which is
        the one thing that mode exists to prevent.
    #>
    param([Parameter(Mandatory)] [string] $Reason)
    if ($Json) {
        $script:facts.state = 'HARNESS-BROKE'
        $script:facts.reason = $Reason
        $script:facts | ConvertTo-Json -Depth 6
    } else {
        Write-Host "[merge-proof] HARNESS-BROKE: $Reason" -ForegroundColor Magenta
    }
    exit $ExitHarnessBroke
}

function Write-Verdict {
    param([Parameter(Mandatory)] [string] $Verdict, [Parameter(Mandatory)] [string] $Reason, [Parameter(Mandatory)] [int] $Code)
    $script:facts.state = $Verdict
    $script:facts.reason = $Reason
    if ($Json) {
        # ConvertTo-Json only, on a structure that was filled as the facts were learned. A second
        # place that decides anything would be a second implementation of the verdict.
        $script:facts | ConvertTo-Json -Depth 6
    } else {
        $colour = switch ($Verdict) { 'SATISFIED' { 'Green' } 'ABSENT' { 'Yellow' } default { 'Red' } }
        Write-Host "[merge-proof] $Verdict" -ForegroundColor $colour
        Write-Host "  $Reason"
        foreach ($line in $script:facts.corroboration) { Write-Host "  corroboration only: $line" -ForegroundColor DarkGray }
        # PRINTED, and printed as what it is. A note about where the search looked is not evidence
        # that the search found something, and keeping the two in one list is what let a sentence
        # about the scan stand in for a record.
        foreach ($line in $script:facts.scanNotes) { Write-Host "  scan: $line" -ForegroundColor DarkGray }
        foreach ($line in $script:facts.unreadable) { Write-Host "  FINDING: $line" -ForegroundColor Yellow }
        foreach ($line in $script:facts.disagreement) { Write-Host "  FINDING: $line" -ForegroundColor Yellow }
    }
    exit $Code
}

function Invoke-Git {
    param([Parameter(Mandatory)] [string[]] $GitArgs)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        # stderr away from the parsed stream: git's diagnostics are not data, and under GIT_TRACE
        # they arrive while the exit code is still zero.
        $output = & git @GitArgs 2>$null
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [ordered]@{ exitCode = $code; lines = @($output | ForEach-Object { [string]$_ }) }
}

function Read-ManifestField {
    <#
        ONE READER FOR EVERY FIELD, because guarding field by field loses to fields being added.
        Four rounds of review found the same defect in four fields -- `pullRequest`, `headSha`,
        `status`, `dirtyDiffHash` -- and the fourth arrived in the same commit that fixed the second:
        a hand-written guard protects the fields somebody remembered.

        The defect is always PowerShell's collection semantics. `-cne` and `-eq` over a collection
        are FILTERS, not comparisons: a singleton array filters to empty, empty is false, and the
        branch that would have refused the manifest is skipped. `[string]$v` on `@('GREEN')` returns
        'GREEN', so a cast hides it too.

        Every field is therefore read by runtime TYPE, never cast and never compared before its
        shape is known. `null` is a kind of its own: a collection is not null, and `@()` and
        `@($null)` both have to be refused where the rule is "this field carries no value".
    #>
    param(
        [Parameter(Mandatory)] [object] $Manifest,
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [ValidateSet('string', 'bool', 'int', 'null')] [string] $Kind
    )
    if (-not (Test-NameIsPresent -Object $Manifest -Name $Name)) {
        return [ordered]@{ ok = $false; value = $null; why = "there is no $Name field" }
    }
    $v = $Manifest.$Name
    # A collection is never a scalar, whatever it holds. This is the single check the four separate
    # guards were each missing a different half of.
    if ($v -is [System.Collections.IEnumerable] -and $v -isnot [string]) {
        return [ordered]@{ ok = $false; value = $null
            why = "$Name is a collection, not a single value, and a collection says nothing this rule can read" }
    }
    # BEFORE THE SWITCH, so it covers EVERY kind and not the one I was looking at. The check used
    # to live inside the `string` case, and `dirtyDiffHash` is read as `null` -- so a non-null string
    # carrying a newline or an ESC skipped it entirely and was interpolated raw into the refusal.
    # A rule written inside one branch is a rule about that branch.
    # AND THE POPULATION IS A UNICODE CATEGORY, NOT A RANGE I PICKED BY HAND. The old one was ASCII
    # C0 plus DEL, which is the family I happened to be thinking about. The class is "code points
    # that reorder or hide text once the value is interpolated into something a person reads" -- and
    # the ones that do that best are not in C0 at all: U+202E RIGHT-TO-LEFT OVERRIDE and the isolates
    # U+2066..U+2069 REORDER the characters around them in a terminal or a copied merge comment, so a
    # status can render as a line the manifest does not say. U+200B and its family, and U+FEFF, are
    # invisible: they hide a difference between two values that look identical.
    #
    # Enumerating those by hand is how the previous version got here. .NET already classifies them:
    # Cc (control) covers C0, C1 and DEL; Cf (format) covers every bidi control, the zero-width
    # family, the soft hyphen and the BOM, INCLUDING the ones above the BMP; Zl and Zp are the two
    # separators that break a line without being a control character.
    #
    # Deliberately NOT in the class, because the reason names what it names: private-use and
    # unassigned code points render as visible tofu rather than deceiving anyone, and refusing them
    # would reject a legitimate value over a Unicode version difference between whichever runtimes
    # write and read the manifest. Unpaired surrogates ARE refused: they are not a code point at all,
    # and what they render as is decided by the consumer.
    if ($v -is [string]) {
        $deceptive = @{
            ([System.Globalization.UnicodeCategory]::Control)            = 'a control character'
            ([System.Globalization.UnicodeCategory]::Format)             = 'an invisible formatting character'
            ([System.Globalization.UnicodeCategory]::LineSeparator)      = 'a line-separating character'
            ([System.Globalization.UnicodeCategory]::ParagraphSeparator) = 'a paragraph-separating character'
        }
        for ($i = 0; $i -lt $v.Length; $i++) {
            $ch = $v[$i]
            # A surrogate reached on its own terms is unpaired: the loop steps OVER the tail of a
            # valid pair below, so a low surrogate here had no head, and a high surrogate with no
            # tail falls into the same refusal.
            #
            # MEASURED, so that nobody writes a cell for it and gets a green one: this branch is not
            # reachable through a manifest. ConvertFrom-Json turns the escape a file can carry into
            # U+FFFD before the reader sees it, and a PowerShell string carrying a real lone
            # surrogate does not survive the UTF-8 write either. It stays because the reader is also
            # called on values that did not come from that parser, and because an unpaired surrogate
            # reaching a message is decided by whoever renders it.
            if ([char]::IsLowSurrogate($ch) -or
                ([char]::IsHighSurrogate($ch) -and ($i + 1 -ge $v.Length -or -not [char]::IsLowSurrogate($v[$i + 1])))) {
                return [ordered]@{ ok = $false; value = $null
                    why = ("$Name contains an unpaired surrogate (U+{0:X4} at index $i), which is not a character at all" -f [int]$ch) }
            }
            # GetUnicodeCategory(string, index) resolves a surrogate PAIR at its head, so an astral
            # format character -- U+110BD, U+13430 -- is classified as Cf here instead of slipping
            # through as two surrogates.
            $category = [System.Globalization.CharUnicodeInfo]::GetUnicodeCategory($v, $i)
            if ($deceptive.ContainsKey($category)) {
                $codePoint = if ([char]::IsHighSurrogate($ch)) { [char]::ConvertToUtf32($ch, $v[$i + 1]) } else { [int]$ch }
                return [ordered]@{ ok = $false; value = $null
                    why = ("$Name contains $($deceptive[$category]) (U+{0:X4} at index $i), which no field this tool reads may hold" -f $codePoint) }
            }
            if ([char]::IsHighSurrogate($ch)) { $i++ }
        }
    }

    switch ($Kind) {
        'null' {
            # THE TYPE, NOT THE VALUE. This one printed the raw string, which is how a control
            # character reached the output at all -- the refusal was the vector. `value` is still
            # returned because the caller distinguishes absent from present-and-wrong by it, and it
            # never prints it.
            if ($null -ne $v) {
                return [ordered]@{ ok = $false; value = $v
                    why = "$Name is set (a $($v.GetType().Name)) where this rule requires it to be null" }
            }
        }
        'string' {
            if ($v -isnot [string]) {
                return [ordered]@{ ok = $false; value = $null
                    why = if ($null -eq $v) { "$Name is null" } else { "$Name is not a string (it is $($v.GetType().Name))" } }
            }
        }
        'bool' {
            if ($v -isnot [bool]) {
                return [ordered]@{ ok = $false; value = $null
                    why = if ($null -eq $v) { "$Name is null" } else { "$Name is not a boolean (it is $($v.GetType().Name))" } }
            }
        }
        'int' {
            if (-not ($v -is [int] -or $v -is [long])) {
                return [ordered]@{ ok = $false; value = $null
                    why = if ($null -eq $v) { "$Name is null" } else { "$Name is not a whole number (it is $($v.GetType().Name))" } }
            }
        }
    }
    return [ordered]@{ ok = $true; value = $v; why = $null }
}

function Get-LandingSchemasTree {
    param([string]$Root, [string]$Commit)
    # The whole schemas tree is sufficient only when every catalog entry is confined to it.
    # Unsupported external paths refuse compatibility rather than narrowing the observed closure.
    $size = Invoke-Git -GitArgs @('-C', $Root, 'cat-file', '-s', "${Commit}:schemas/catalog.json")
    $bytes = 0L
    if ($size.exitCode -ne 0 -or -not [long]::TryParse(($size.lines -join '').Trim(), [ref]$bytes) -or $bytes -gt 1MB) { return $null }
    $blob = Invoke-Git -GitArgs @('-C', $Root, 'show', "${Commit}:schemas/catalog.json")
    if ($blob.exitCode -ne 0) { return $null }
    try { $catalog = ConvertFrom-Json -InputObject ($blob.lines -join "`n") -ErrorAction Stop } catch { return $null }
    if ($catalog.schemas -isnot [pscustomobject]) { return $null }
    $entries = @($catalog.schemas.PSObject.Properties)
    if ($entries.Count -gt 256) { return $null }
    foreach ($entry in $entries) {
        $path = $entry.Value.path
        if ($path -isnot [string] -or -not $path.StartsWith('schemas/', [StringComparison]::Ordinal) -or
            $path.Length -gt 1024 -or $path.Contains('\') -or $path.Contains(':') -or
            @($path.Split('/') | Where-Object { $_ -eq '' -or $_ -eq '.' -or $_ -eq '..' }).Count -gt 0) { return $null }
        $exists = Invoke-Git -GitArgs @('-C', $Root, 'cat-file', '-e', "${Commit}:$path")
        if ($exists.exitCode -ne 0) { return $null }
    }
    $tree = Invoke-Git -GitArgs @('-C', $Root, 'rev-parse', '--verify', "${Commit}:schemas")
    if ($tree.exitCode -ne 0) { return $null }
    return ($tree.lines -join '').Trim()
}

function Test-ManifestLandingSnapshot {
    param([object]$Manifest, [object]$ActualPr, [string]$Root, [int]$Number)
    $property = $Manifest.PSObject.Properties['mergeTarget']
    if ($null -eq $property -or $property.Value -isnot [pscustomobject]) { return 'PR-base snapshot is absent or malformed' }
    $snapshot = $property.Value
    $fields = @{}
    foreach ($name in @('mode','head','sha','ref','mergeBase')) {
        $field = Read-ManifestField -Manifest $snapshot -Name $name -Kind string
        if (-not $field.ok) { return "PR-base snapshot $name is absent or malformed" }
        $fields[$name] = $field.value
    }
    if (-not (Test-SameText $fields.mode 'supplied-pr')) { return 'local-only test evidence is not PR-base proof' }
    $pr = Read-ManifestField -Manifest $snapshot -Name pullRequest -Kind int
    $source = Read-ManifestField -Manifest $Manifest -Name headSha -Kind string
    if (-not $pr.ok -or $pr.value -ne $Number -or -not $source.ok -or -not (Test-SameText $source.value $fields.head)) { return 'PR-base snapshot does not identify this manifest source head and PR' }
    if ($fields.head -cnotmatch '^[0-9a-f]{40}$' -or $fields.sha -cnotmatch '^[0-9a-f]{40}$' -or $fields.mergeBase -cnotmatch '^[0-9a-f]{40}$') { return 'PR-base snapshot contains an invalid commit ID' }
    if ($null -eq $ActualPr -or $ActualPr.number -ne $Number -or -not (Test-SameText $fields.ref $ActualPr.baseRefName)) { return 'PR-base snapshot does not match the live PR identity/base name' }
    $current = $ActualPr.baseRefOid
    if ($current -isnot [string] -or $current -cnotmatch '^[0-9a-f]{40}$') { return 'live PR base SHA is unavailable' }
    $base = Invoke-Git -GitArgs @('-C', $Root, 'merge-base', $fields.head, $fields.sha)
    if ($base.exitCode -ne 0 -or -not (Test-SameText (($base.lines -join '').Trim()) $fields.mergeBase)) { return 'PR-base snapshot merge-base does not re-derive' }
    if (-not (Test-SameText $fields.sha $current)) {
        $ancestry = Invoke-Git -GitArgs @('-C', $Root, 'merge-base', '--is-ancestor', $fields.sha, $current)
        if ($ancestry.exitCode -ne 0) { return 'live landing base is not a fast-forward of the captured base' }
        $before = Get-LandingSchemasTree -Root $Root -Commit $fields.sha
        $after = Get-LandingSchemasTree -Root $Root -Commit $current
        $candidateInputs = Get-LandingSchemasTree -Root $Root -Commit $fields.head
        $forkInputs = Get-LandingSchemasTree -Root $Root -Commit $fields.mergeBase
        if (-not $before -or -not $after -or -not $candidateInputs -or -not $forkInputs -or
            -not (Test-SameText $before $after)) { return 'landing schema input closure changed or is unsupported; fresh target evidence is required' }
    }
}

function Test-ManifestVouches {
    <#
        THE PREDICATE, ONCE. A ledger record is a manifest: the gate writes the durable copy and the
        committable copy of one run from the same values. Judging the committed one strictly and the
        durable one by two fields made the ledger the back door to everything the store had closed
        -- a record naming another pull request, or carrying a non-null dirtyDiffHash, still counted
        as the independent witness that suppresses the "committed manifest alone" warning.

        Returns the reasons it does NOT vouch. Empty means it does.
    #>
    param(
        [Parameter(Mandatory)] [object] $Manifest,
        [Parameter(Mandatory)] [string] $Head,
        [string] $Parent,
        [Parameter(Mandatory)] [bool] $TipIsPure,
        [Parameter(Mandatory)] [int] $PullRequest,
        [object[]] $Entries = @(),
        [string] $StorePrefix = ''
    )
    $why = @()

    $prField = Read-ManifestField -Manifest $Manifest -Name 'pullRequest' -Kind 'int'
    if (-not $prField.ok) { $why += ($prField.why + ', so it names no pull request') }
    elseif ([long]$prField.value -ne [long]$PullRequest) { $why += "it names pull request #$($prField.value), not #$PullRequest" }


    # `status` is the field asserting the run was green AT ALL: a wrong value here does not
    # misroute the proof, it manufactures it.
    $statusField = Read-ManifestField -Manifest $Manifest -Name 'status' -Kind 'string'
    if (-not $statusField.ok) { $why += $statusField.why }
    elseif (-not (Test-SameText $statusField.value 'GREEN')) { $why += "status is '$($statusField.value)', not GREEN" }

    # `pushed` is a closed vocabulary: true vouches, false does not, and null means nobody could
    # tell -- which is not proof. Treating unknown as satisfied would reinstate the exact defect
    # (a) added the field to remove.
    $pushedField = Read-ManifestField -Manifest $Manifest -Name 'pushed' -Kind 'bool'
    if (-not $pushedField.ok) {
        # The null case keeps its own sentence. "nobody could tell" is a DIFFERENT fact from "this
        # is the wrong type", and the reader's generic wording lost it -- collapsing five guards
        # into one must not collapse what the five were saying.
        $why += if (Test-SameText $pushedField.why 'pushed is null') { 'pushed is null: nobody could tell whether that head reached the server' }
                else { $pushedField.why + ', so it says nothing this rule can read' }
    }
    elseif (-not $pushedField.value) { $why += 'pushed is false: the head it names never reached the server' }

    # A GREEN written under a dirty tree says the tests passed on something that was never
    # committed, and committing only the manifest afterwards leaves that difference in place. The
    # field must be PRESENT and scalar null: absent means the writer was not the gate, and a
    # collection is not null however empty it looks.
    $dirtyField = Read-ManifestField -Manifest $Manifest -Name 'dirtyDiffHash' -Kind 'null'
    if (-not $dirtyField.ok) {
        $why += if ($null -eq $dirtyField.value) { $dirtyField.why + ', so nothing says the gate worktree was clean' }
                else { $dirtyField.why + ': the gate ran with uncommitted edits, so it measured a tree that no commit holds' }
    }

    # A SCALAR FULL OBJECT ID, checked before it is compared.
    $headField = Read-ManifestField -Manifest $Manifest -Name 'headSha' -Kind 'string'
    if (-not $headField.ok) { $why += ($headField.why + ', so it names no commit') }
    elseif ($headField.value -cnotmatch '^[0-9a-f]{40}$') {
        $why += "headSha '$($headField.value)' is not a full 40-character object id"
    } elseif (-not (Test-SameText $headField.value $Head)) {
        if ($null -eq $Parent) {
            $why += "headSha does not equal the head, and the head's parent could not be read"
        } elseif (-not (Test-SameText $headField.value $Parent)) {
            $why += "headSha is neither the head nor its parent"
        } elseif (-not $TipIsPure) {
            $outside = @($Entries | Where-Object { -not ($_.path.StartsWith($StorePrefix) -and $_.status -cmatch '^[AM]') } |
                    ForEach-Object { "$($_.status) $($_.path)" })
            $why += ("headSha is the head's PARENT, which is only acceptable when the tip adds nothing " +
                "but the record -- and this tip also touches: " + (($outside | Select-Object -First 5) -join ', '))
        }
    }
    # #205: THE SWEEP'S READING IS READ HERE, OR IT IS DECORATION. `ci/gate.ps1` writes
    # `eventContractSweep = { state: measured | notMeasured | absent, reason }` beside `coverage`;
    # a remote that did not answer is `notMeasured` there and never `status: RED`, because that is a
    # fact about the instrument and not about the tree. The verdict is where that fact has to bite:
    # a GREEN whose sweep did not measure is a run that never asked whether the event contract
    # collides with another open branch, and it does not vouch. Missing evidence is refused in
    # BLOCKING mode. Only legacy ADVISORY mode retains compatibility with receipts lacking this field.
    if (Test-NameIsPresent -Object $Manifest -Name 'eventContractSweep') {
        $sweep = $Manifest.eventContractSweep
        $state = $null
        $reason = ''
        $stateShapeIsScalar = $false
        if ($null -ne $sweep -and (Test-NameIsPresent -Object $sweep -Name 'state')) {
            if ($sweep.state -is [string]) { $state = $sweep.state; $stateShapeIsScalar = $true }
        }
        if ($null -ne $sweep -and (Test-NameIsPresent -Object $sweep -Name 'reason')) { $reason = [string] $sweep.reason }
        if (-not $stateShapeIsScalar -or -not (Test-SameText $state 'measured')) {
            $shown = if (-not $stateShapeIsScalar) { 'not a scalar string' } else { $state }
            $detail = if ([string]::IsNullOrWhiteSpace($reason)) { '' } else { " ($reason)" }
            $why += "eventContractSweep is $shown$detail, so the run did not measure whether the event contract collides with another open branch"
        }
    } elseif ($blocking) {
        $why += 'eventContractSweep is absent from a blocking manifest, so this run provides no measured event-contract population'
    }
    if ($script:landingProofRequired) {
        $why += @(Test-ManifestLandingSnapshot -Manifest $Manifest -ActualPr $script:actualLandingPr -Root $RepositoryRoot -Number $PullRequest)
    }
    return $why
}

# THE LEDGER'S LOCATION IS NOT THE VERDICT'S BUSINESS when it sits under somebody's home: the
# house rule is that output exposes no home paths, and a verdict is output like any other. Where it
# is outside home -- the slot's own drive, the usual case -- the path is still named, because
# "absence at a path nobody can see" was the defect that made this scan report zero runs it never
# looked for.
# #903: what a manifest's `scope` object means to a PRESSER. A GREEN over two crates and a GREEN
# over the workspace are the same word, and this verifier is the last thing anyone reads before the
# button. A manifest written before #903 has no `scope` at all -- ABSENT IS NOT "FULL", so it says
# nothing rather than inventing a reassurance.
function Format-ScopeNote {
    param([AllowNull()] [object] $Body)

    if ($null -eq $Body) { return '' }
    # READ BY PROPERTY LOOKUP, NOT BY DOT. `$Body.scope` returns $null with StrictMode off and
    # THROWS under `Set-StrictMode -Version 2.0` -- and EVERY manifest written before #903 lacks
    # the key, so a dot read turns "this record predates the field" into an exception. This file
    # does not set StrictMode today, but it is dot-sourced into harnesses that do, and a library
    # function must not depend on its caller's mode to avoid throwing on ordinary input.
    $scopeProperty = $Body.PSObject.Properties['scope']
    if ($null -eq $scopeProperty -or $null -eq $scopeProperty.Value) { return '' }
    $scope = $scopeProperty.Value
    $read = { param($n) $p = $scope.PSObject.Properties[$n]; if ($p) { $p.Value } else { $null } }
    if (& $read 'full') { return "scope: FULL ($([string](& $read 'reason')))" }
    $crates = @(& $read 'crates')
    $matrix = if (& $read 'matrix') { 'ran' } else { "skipped ($([string](& $read 'matrixReason')))" }
    return ("scope: SCOPED to $($crates.Count) crate(s) [$($crates -join ', ')]; PostgreSQL matrix $matrix -- " +
        'this GREEN covers the selection, not the workspace')
}

function Format-PathForOutput {
    <#
        EVERY path that reaches the output goes through here, not just the ledger's. The first
        version was written for the ledger and the repository root kept its own unredacted
        interpolation one function away -- a rule applied to the subject in front of me instead of
        to its subjects, for the fifth time in this pull request.
    #>
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Path)
    # NOT `$home`: that is an automatic variable and it is READ-ONLY, so assigning to it throws on
    # every call and takes the whole run with it. Measured after the suite died at exit 255 with its
    # output truncated mid-cell -- a crash, not a failed assertion, which is why nothing named it.
    $userHome = [Environment]::GetFolderPath('UserProfile')
    if ($userHome -and $Path -and $Path.StartsWith($userHome, [StringComparison]::OrdinalIgnoreCase)) {
        # GENERIC, because this formatter serves every path now: the ledger, the repository root and the
        # script itself. A placeholder naming one of them read as a lie for the other two.
        return '<a path under the current user profile>'
    }
    return $Path
}

# DEFINED BEFORE ITS FIRST USE, and that is not a style point: PowerShell runs a script top to
# bottom, so the default-root resolution called this while it did not yet exist -- the call failed,
# the message printed the raw path anyway, and the redaction I had just added applied to nothing.
# The cell that caught it is the one that runs a COPY of this script from outside a work tree.

function Get-RemoteSlug {
    <#
        owner/repo for the repository at $RepositoryRoot, or $null when its origin is not a GitHub
        remote. Returning $null rather than a guess is the point: the caller refuses to look
        anything up rather than looking it up somewhere else.
    #>
    param([Parameter(Mandatory)] [string] $RepositoryRoot)
    $probe = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'remote', 'get-url', 'origin')
    if ($probe.exitCode -ne 0 -or -not $probe.lines) { return $null }
    $url = ([string]$probe.lines[0]).Trim()
    # Both spellings GitHub hands out, and nothing else. A local path or a foreign host is not a
    # slug and must not be bent into one.
    if ($url -cmatch '^(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)([^/]+)/(.+?)(?:\.git)?/?$') {
        return "$($Matches[1])/$($Matches[2])"
    }
    return $null
}

if (-not $RepositoryRoot) {
    # THE SCRIPT'S OWN TREE, not the caller's. The parameter documents "the repository this script
    # sits in", and `rev-parse` in the caller's directory answered a different question: run by
    # absolute path from another checkout it judged THAT repository -- silently, with a verdict that
    # looked like a verdict about GraphHelm. A default that answers a different question than the
    # one it is documented to answer is worse than no default.
    $probe = Invoke-Git -GitArgs @('-C', $PSScriptRoot, 'rev-parse', '--show-toplevel')
    if ($probe.exitCode -ne 0 -or -not $probe.lines) {
        # Through the formatter, like every other path -- and this one is the message MOST likely to
        # carry a home path, because the documented recovery for #733 is to extract this script
        # somewhere else and run it from there. The rule was written for the ledger, carried to the
        # repository root last round, and this third site was still printing raw.
        Write-Broke -Reason ("the script at $(Format-PathForOutput -Path $PSScriptRoot) is not inside a git working " +
            'tree, so there is no repository to default to')
    }
    $RepositoryRoot = $probe.lines[0]
}

# THE MODE, DERIVED. `Get-HeadProvenance` is (a)'s function; its presence in the gate on this
# repository is the fact that the artefact this tool demands can actually be produced here.
# THE MODE IS A FACT ABOUT MAIN, NOT ABOUT THIS CHECKOUT. The rule is "(a) has landed", and the
# working tree of whoever runs this says nothing about that -- it says which branch they were on.
# Read from `origin/main` with git, like every other question here.
#
# And a detector that cannot read must not fail PERMISSIVE. `Test-Path` returning false gave
# advisory in silence, so a missing file and a landed (a) were the same answer -- the failure this
# whole tool exists to remove, inside its own switch.
# FETCH FIRST. `origin/main` is a local ref that is only as fresh as the last fetch: a merge holder
# who fetched before (a) landed would read the OLD gate, answer advisory, and every verdict would
# be reported as unenforced on a repository where the rule is live. A ref this tool did not refresh
# is a claim about someone's last fetch, not about main.
$fetch = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'fetch', '--quiet', 'origin', 'main')
if ($fetch.exitCode -ne 0) {
    Write-Broke -Reason ('could not fetch origin/main, so the mode would be decided by whenever this ' +
        'checkout last fetched -- which is a fact about the operator, not about main')
}
$gateAtMain = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'show', 'origin/main:ci/gate.ps1')
if ($gateAtMain.exitCode -ne 0) {
    Write-Broke -Reason ('could not read ci/gate.ps1 at origin/main, so the mode cannot be determined. ' +
        'Fetch first; a mode this tool guessed would be a claim nobody re-derives')
}
# THE DEFINITION, NOT A MENTION OF IT. A regex over the text says yes to a COMMENT, a string, or a
# line in a docstring that happens to name the function -- and this file's own rule is that a
# comment is a claim rather than a fact. The mode decides whether a NOT is enforced, so it is worth
# asking the parser instead of the page.
#
# `ParseInput` returns the AST even when the script has errors, and a `FunctionDefinitionAst` exists
# only where a function is really defined. Failure to parse is not a licence to guess: it falls back
# to the text match and SAYS so, because "main's gate could not be parsed" and "main has no such
# function" are different states and only one of them is about #674(a) landing.
$gateAtMainText = ($gateAtMain.lines -join "`n")
$blocking = $false
$script:landingProofRequired = $false
$modeEvidence = 'the AST of ci/gate.ps1 at origin/main'
# THE PARSER HAS TWO OUTPUT CHANNELS AND I DISCARDED BOTH. `ParseInput` does not THROW on a syntax
# error -- it returns a PARTIAL ast and reports the errors through its second [ref]. So the `catch`
# I wrote for the unparseable case can never fire, and a gate carrying a real definition beside a
# syntax error anywhere else was judged from a tree the parser had already given up on.
#
# A partial parse is not evidence either way: the definition may be missing because it is not there,
# or because the parser stopped before reaching it. That is the difference between "main has not
# landed (a)" and "nobody could tell", and this file refuses to collapse those anywhere else.
$parseErrors = $null
try {
    $gateAst = [System.Management.Automation.Language.Parser]::ParseInput($gateAtMainText, [ref] $null, [ref] $parseErrors)
    if (@($parseErrors).Count -gt 0) {
        $firstError = @($parseErrors)[0]
        Write-Broke -Reason ("ci/gate.ps1 at origin/main does not parse (" + $firstError.Message +
            "), so whether it defines Get-HeadProvenance cannot be decided. A mode read off a partial " +
            'parse would be a guess about whether verdicts here are enforced')
    }
    $blocking = @($gateAst.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    (Test-SameText $node.Name 'Get-HeadProvenance')
            }, $true)).Count -gt 0
    $script:landingProofRequired = @($gateAst.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
            (Test-SameText $node.Name 'Get-LandingSnapshot')
    }, $true)).Count -gt 0
} catch {
    $blocking = ($gateAtMainText -cmatch '(?m)^\s*function\s+Get-HeadProvenance\b')
    $modeEvidence = 'a text match on ci/gate.ps1 at origin/main, which could not be parsed'
    $script:facts.scanNotes += ("ci/gate.ps1 at origin/main could not be parsed, so the mode was decided by matching " +
        'the function definition as text')
}
# The mode AND why it is that mode, so a merge comment quoting this never has to infer it.
$script:facts.mode = if ($blocking) { 'blocking' } else { 'advisory' }
$script:facts.modeReason = if ($blocking) {
    "$modeEvidence defines Get-HeadProvenance, so a gate run records the head it judged"
} else {
    "$modeEvidence does not define Get-HeadProvenance (#674(a) has not landed), so no branch can produce the artefact yet"
}
if (-not $Json) {
    if ($blocking) {
        Write-Host "[merge-proof] mode: BLOCKING -- $($script:facts.modeReason)." -ForegroundColor Cyan
    } else {
        Write-Host "[merge-proof] mode: ADVISORY -- $($script:facts.modeReason). The verdict below is reported and NOT enforced." -ForegroundColor Yellow
    }
}

# THE VERIFIER IS PART OF THE CANDIDATE. A merge holder follows the documented invocation from a
# checkout of the branch under judgement, so the pull request supplies the predicate that judges it:
# replacing this file with `exit 0` certifies anything. Reading `ci/gate.ps1` from main and then
# trusting THIS file was half a check.
#
# The comparison is against main's copy, and it fails CLOSED. Bootstrap is a real state and is named
# rather than silently allowed: until (b) lands there is no copy on main to compare against, and a
# tool that pretended otherwise would be making the claim it exists to refuse.
$selfAtMain = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'show', 'origin/main:ci/merge-proof.ps1')
if ($selfAtMain.exitCode -ne 0) {
    $script:facts.self = 'not on main yet: this verifier cannot be compared against a trusted copy'
    if (-not $Json) {
        Write-Host ('[merge-proof] NOTE: ci/merge-proof.ps1 does not exist at origin/main, so this run ' +
            'cannot check that it is the verifier the repository agreed on.') -ForegroundColor Yellow
    }
} else {
    $here = ([System.IO.File]::ReadAllText($PSCommandPath) -replace "`r`n", "`n").TrimEnd()
    $there = (($selfAtMain.lines -join "`n")).TrimEnd()
    # THE ANTI-TAMPER COMPARISON, and the one where an approximate comparer costs the most: a
    # verifier that differs from main by code points the culture comparer gives no weight to was
    # being called byte-identical to it.
    if (-not (Test-SameText $here $there)) {
        Write-Broke -Reason ('this ci/merge-proof.ps1 is not the one at origin/main, and a predicate supplied ' +
            'by the branch under judgement can certify anything. Run main copy of the verifier against this head')
    }
    $script:facts.self = 'byte-identical to ci/merge-proof.ps1 at origin/main'
    # AND THIS CHECK CANNOT DEFEND ITSELF. A candidate that replaces this file removes the
    # comparison along with everything else, so what it catches is DRIFT -- a stale checkout, an
    # edit somebody forgot -- and not a hostile author. Saying that in the output is the honest
    # form: the control is the OPERATOR running main's copy, and a tool that let this line read as
    # that control would be making exactly the claim it exists to refuse.
    if (-not $Json) {
        Write-Host ('[merge-proof] NOTE: this verifier checked ITSELF against origin/main. That catches ' +
            'drift, not a candidate that removed the check -- for that, run the copy from origin/main:' +
            "`n    git worktree add --detach --quiet `$env:TEMP\mp-main origin/main" +
            "`n    powershell -NoProfile -ExecutionPolicy Bypass -File `$env:TEMP\mp-main\ci\merge-proof-from-main.ps1 -PullRequest <n> -RepositoryRoot ." +
            "`n    git worktree remove --force `$env:TEMP\mp-main" +
            "`n  NOT ``git show ... > file``: on Windows PowerShell 5.1 ``>`` is Out-File and re-encodes, so the runner is not main's bytes and still parses.") -ForegroundColor Yellow
    }
}

if (-not $Head) {
    # -R, SCOPED TO THE REPOSITORY BEING JUDGED. `gh` resolves a pull request number against the
    # CURRENT DIRECTORY's repository, and this tool is run by a merge holder who may be standing
    # anywhere: without the slug, #701 in some other checkout answers with that project's head and
    # every verdict below would be about a commit from another repository entirely.
    $slug = Get-RemoteSlug -RepositoryRoot $RepositoryRoot
    if (-not $slug) {
        Write-Broke -Reason ("could not derive owner/repo from the origin of $(Format-PathForOutput -Path $RepositoryRoot), so a pull request " +
            "lookup would resolve against whatever repository the caller is standing in. Pass -Head")
    }
    $lookup = & gh pr view $PullRequest --repo $slug --json headRefOid --jq '.headRefOid' 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $lookup) {
        Write-Broke -Reason "could not read the head of pull request #${PullRequest} in ${slug}; gh did not answer"
    }
    $Head = ([string]$lookup).Trim()
}
$script:facts.headSha = $Head

# Bootstrap is controlled by trusted main, not a candidate flag. Before its producer lands,
# legacy receipts remain gate evidence without PR-base proof. After activation, local/missing
# snapshots cannot authorize that proof. This adds a target check without changing the canonical
# manifest head/parent/pure-tip predicate above (.factory/MERGE-CHECKLIST.md).
$script:actualLandingPr = $null
if ($script:landingProofRequired) {
    $slug = Get-RemoteSlug -RepositoryRoot $RepositoryRoot
    if (-not $slug) { Write-Broke -Reason 'cannot derive repository identity for live PR-base verification' }
    $metadata = @(& gh pr view $PullRequest --repo $slug --json number,headRefOid,baseRefName,baseRefOid 2>$null)
    $metadataExit = $LASTEXITCODE
    if ($metadataExit -ne 0) { Write-Broke -Reason 'live PR-base metadata could not be read' }
    try { $script:actualLandingPr = ConvertFrom-Json -InputObject ($metadata -join "`n") -ErrorAction Stop }
    catch { Write-Broke -Reason 'live PR-base metadata is malformed' }
    $live = $script:actualLandingPr
    if ($live -isnot [pscustomobject] -or ($live.number -isnot [int] -and $live.number -isnot [long]) -or $live.number -ne $PullRequest -or
        $live.headRefOid -isnot [string] -or -not (Test-SameText $live.headRefOid $Head) -or
        $live.baseRefName -isnot [string] -or $live.baseRefOid -isnot [string] -or
        $live.baseRefOid -cnotmatch '^[0-9a-f]{40}$') {
        Write-Broke -Reason 'live PR metadata does not identify the requested head and base'
    }
    $present = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'cat-file', '-e', "$($live.baseRefOid)^{commit}")
    if ($present.exitCode -ne 0) {
        $fetchBase = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'fetch', '--quiet', '--no-tags', 'origin', $live.baseRefOid)
        if ($fetchBase.exitCode -ne 0) { Write-Broke -Reason 'live PR base object is unavailable' }
    }
    $script:facts.scanNotes += 'PR-base proof is active; target advancement is allowed only for an ancestor with identical confined schema inputs. This is not atomic merge authorization.'
} else {
    $script:facts.scanNotes += 'PR-base proof has not activated on trusted main; legacy receipts establish gate evidence only. Target-aware evidence is required after rollout.'
}

# The parent is read BEFORE the store is searched, because the near-miss report below needs it.
$parentEarlyProbe = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'rev-parse', "${Head}^")
$parentEarly = if ($parentEarlyProbe.exitCode -eq 0 -and $parentEarlyProbe.lines) { $parentEarlyProbe.lines[0].Trim() } else { $null }

# THE STORE IS READ AT THE HEAD, NEVER FROM A WORKING TREE. Whoever holds the button is on some
# other branch: reading the disk would miss a perfectly committed manifest (ABSENT for a head that
# IS certified) and would accept a stale file the operator happens to be holding (SATISFIED for a
# head nothing ever gated). Both directions are wrong and the second is the dangerous one.
# AND THE LISTING IS STREAMED. `Invoke-Git` captures everything the command prints before any
# ceiling below can look at it, so a head carrying a million paths under the store was already in
# memory by the time the count refused it: the limit read as a bound and was only a bound on what
# survived. `Select-Object -First` stops `ls-tree` itself, one entry past the ceiling so overflow is
# still detectable.
$previousEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    # 762-DELIBERATE-STOP: $rawPaths -- the bound on what is READ is worth more than the exit code,
    # so the stop stays and the code is read only when we did not cause it. See below the finally.
    $rawPaths = @(& git -C $RepositoryRoot ls-tree -r --name-only --full-tree $Head -- $StorePrefix.TrimEnd('/') 2>$null |
            Select-Object -First ($MaxManifests + 1))
    $listingExit = $LASTEXITCODE
} finally { $ErrorActionPreference = $previousEap }
# AND THE EXIT CODE IS ONLY READ WHEN WE DID NOT STOP THE COMMAND OURSELVES (#762). The streaming
# above is deliberate and stays: `Select-Object -First` bounds what is READ rather than what
# survives, which is the whole reason an untrusted store cannot choose how much this machine spends.
# But stopping a pipeline TERMINATES the native command feeding it, and on PowerShell 7 that leaves
# `$LASTEXITCODE` at -1 with the listing correct -- so reading it unconditionally reported "could
# not list, fetch the head first" for a store whose real defect was being too large. Two refusals
# that both fail closed, one of them naming the wrong cause and sending the operator to fetch a head
# they already have.
#
# THE GENERAL RECIPE FOR #762 -- capture, read the code, then reduce -- IS WRONG HERE, because
# capturing first is exactly the unbounded read the comment above exists to prevent. The two
# requirements genuinely conflict, and the shape that satisfies both is the one already used for the
# tip listing below: a stop we performed on purpose is not a failure to diagnose. `$tipIsPure`
# conjoins `-not $tipOverflow` with its exit code and was never reachable by this defect; this is
# the same structure, made explicit.
$listingStoppedByUs = ($rawPaths.Count -gt $MaxManifests)
if (-not $listingStoppedByUs -and $listingExit -ne 0) {
    Write-Broke -Reason "could not list $StorePrefix at ${Head}. Fetch the head first"
}
$manifests = @()
$unreadable = @()
$storeBytes = 0L
$paths = @($rawPaths | ForEach-Object { [string]$_ } | Where-Object { $_ -ne '' })
# A store is untrusted input like any other tree: a pull request can carry arbitrarily many
# manifests, or one arbitrarily large. Both ceilings are checked BEFORE anything is materialised,
# because a limit applied after the loop is a limit on what is already in memory.
if ($paths.Count -gt $MaxManifests) {
    Write-Broke -Reason ("$($paths.Count) manifests at this head, beyond the $MaxManifests this tool will " +
        "read. That is a store to look at, not a verdict to trust")
}
foreach ($path in $paths) {
    $size = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'cat-file', '-s', "${Head}:$path")
    $bytes = 0L
    if ($size.exitCode -ne 0 -or -not [long]::TryParse((($size.lines -join '')).Trim(), [ref] $bytes)) {
        $unreadable += "$path (its size could not be read at the head)"
        continue
    }
    # AND THE AGGREGATE, not only each file. Two thousand manifests of just under a megabyte each
    # pass the per-file ceiling one at a time and add up to two gigabytes -- the per-file limit reads
    # as a bound and bounds only the WORST SINGLE ENTRY. Same shape as `$MaxManifests` bounding the
    # count while each file stayed unbounded: two ceilings that are individually true and jointly
    # useless.
    #
    # Fails CLOSED, and as HARNESS-BROKE rather than NOT: a store too large to read is a store this
    # tool cannot judge, which is a different answer from "the records here do not vouch".
    $storeBytes += $bytes
    if ($storeBytes -gt $MaxStoreBytes) {
        Write-Broke -Reason ("the manifests at this head total more than $MaxStoreBytes bytes, which is more than this " +
            'tool will read. That is a store to look at, not a verdict to trust')
    }
    if ($bytes -gt $MaxManifestBytes) {
        $unreadable += "$path ($bytes bytes, beyond the $MaxManifestBytes this tool will read)"
        continue
    }
    $blob = Invoke-Git -GitArgs @('-C', $RepositoryRoot, 'show', "${Head}:$path")
    if ($blob.exitCode -ne 0) { $unreadable += "$path (could not be read at the head)"; continue }
    try {
        $parsedBody = (($blob.lines -join "`n") | ConvertFrom-Json)
        # A ROOT THAT IS NOT AN OBJECT PARSES PERFECTLY AND IS NOT A RECORD. `"garbage"` and
        # `[1,2]` are valid JSON, so the parse succeeded, the value went into the store, and it then
        # had no readable `pullRequest` -- which made the answer ABSENT ("nothing recorded a run,
        # run the gate") for a store that holds a structurally corrupt file. Absent and corrupt are
        # different states and only one of them is fixed by re-running.
        if ($parsedBody -isnot [System.Management.Automation.PSCustomObject]) {
            $kind = if ($null -eq $parsedBody) { 'null' } else { $parsedBody.GetType().Name }
            $unreadable += "$path (its JSON root is $kind, not an object, so it holds no fields)"
            continue
        }
        $manifests += ,([ordered]@{ name = $path; body = $parsedBody })
    } catch {
        # An unreadable manifest is not an absent one, and it is REMEMBERED rather than skipped:
        # when it turns out to be the only record for this pull request, "run the gate" is the wrong
        # instruction and the answer has to say corruption instead.
        $unreadable += "$path (does not parse as JSON)"
    }
}

# A SCALAR INTEGER, by runtime type. `"pullRequest": "42"` compares equal under PowerShell's
# coercion, and `[42, 99]` makes `-eq` a FILTER that returns a non-empty collection -- so an array
# naming several pull requests would satisfy any of them. Same closed-vocabulary rule as `pushed`:
# the field either is a number or it says nothing this tool can read.
$mine = @($manifests | Where-Object {
        $field = Read-ManifestField -Manifest $_.body -Name 'pullRequest' -Kind 'int'
        $field.ok -and ([long]$field.value -eq [long]$PullRequest)
    })
if ($mine.Count -eq 0) {
    # ABSENT, and the reason names the NEAR MISS when there is one. A manifest that names this head
    # under a different pull request number, or under none at all (the pre-#674(a) shape), is not a
    # run recorded for THIS pull request -- so the action is the same as having nothing, which is to
    # run the gate. Reporting it as NOT would send a reader to investigate a failure that did not
    # happen. But saying only "nothing" would hide that a run exists and is nearly the right one,
    # so the near miss is named.
    # THROUGH THE READER, like every other field read. `-cin` with a collection on the left asks
# whether the collection OBJECT is in the list, so a manifest whose headSha is `["<the head>"]`
# was silently left out of the near-miss report -- the one place whose whole job is to say "there
# is something here that nearly matched". A near miss that cannot be reported is the same silence
# the report exists to break.
$nearMisses = @($manifests | Where-Object {
            $h = Read-ManifestField -Manifest $_.body -Name 'headSha' -Kind 'string'
            $h.ok -and ($h.value -cin @($Head, $parentEarly))
        } | ForEach-Object {
            $pr = Read-ManifestField -Manifest $_.body -Name 'pullRequest' -Kind 'int'
            if ($pr.ok) { "$($_.name) names this head under pull request #$($pr.value)" }
            else { "$($_.name) names this head, and $($pr.why)" }
        })
    if ($unreadable.Count -gt 0) {
        # NOT "run the gate". A record exists and cannot be read, so the instruction is to LOOK at
        # it -- re-running would write a second file beside a broken one and hide the corruption.
        $script:facts.unreadable = $unreadable
        Write-Verdict -Verdict 'NOT' -Code $ExitNot -Reason ("a manifest exists at this head and could not be read, so nothing " +
            "can be concluded about the run it recorded -- corruption to look at, not a gate to re-run:`n    " +
            ($unreadable -join "`n    "))
    }
    $reason = "no manifest in $StorePrefix names pull request #$PullRequest. Nothing recorded a run for it, " +
        "which is a different answer from a run that does not vouch for this head -- run the gate."
    if ($nearMisses.Count -gt 0) { $reason += "`n    near miss: " + ($nearMisses -join "`n    near miss: ") }
    # THE EXIT CODE IS THE VERDICT, ALWAYS. Advisory used to force 0, which made the code lie about
    # what was found and contradicted this file's own header. The mode is a SEPARATE fact, in the
    # output and in the JSON; whether to enforce a verdict is the caller's decision, and it belongs
    # in the merge comment rather than hidden inside an exit code.
    Write-Verdict -Verdict 'ABSENT' -Reason $reason -Code $ExitAbsent
}

# The parent of the head, and what the tip touched. Both are ordinary git questions on purpose:
# whoever holds the button can re-derive this verdict without running this script.
$parent = $parentEarly
# `--name-status`, not `--name-only`. A name-only listing shows a DELETION under the prefix as a
# path under the prefix, so a tip that REMOVES manifests read as "touches only the store" -- the
# commit that erases the evidence certified exactly like the commit that added it. Only additions
# and modifications inside the store make a tip pure.
# AND BOUNDED. A tip that touches an enormous number of paths is exactly the tip this check is
# about, and reading all of it into memory to find that out lets the untrusted commit choose how
# much the merge holder's machine spends. `Select-Object -First` stops the upstream command, so the
# bound is on what is READ, not merely on what is kept -- and the overflow verdict fails CLOSED:
# a tip too large to list is not a tip that touches only the store.
$tipOverflow = $false
$previousEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    # 762-DELIBERATE-STOP: $rawTip -- same trade as the store listing. `$tipIsPure` conjoins
    # `-not $tipOverflow` with this exit code, so a stop we performed cannot read as a git failure.
    $rawTip = @(& git -C $RepositoryRoot diff-tree --no-commit-id --name-status -r $Head 2>$null |
            Select-Object -First ($MaxTipEntries + 1))
    $tipExit = $LASTEXITCODE
} finally { $ErrorActionPreference = $previousEap }
if ($rawTip.Count -gt $MaxTipEntries) {
    $tipOverflow = $true
    $rawTip = @($rawTip | Select-Object -First $MaxTipEntries)
}
$touchedProbe = [ordered]@{ exitCode = $tipExit; lines = @($rawTip | ForEach-Object { [string]$_ }) }
$entries = @($touchedProbe.lines | Where-Object { $_ -ne '' } | ForEach-Object {
        $parts = $_ -split "`t"
        [ordered]@{ status = $parts[0]; path = if ($parts.Count -gt 1) { $parts[-1] } else { '' } }
    })
$touched = @($entries | ForEach-Object { "$($_.status) $($_.path)" })
$tipIsPure = (-not $tipOverflow -and $touchedProbe.exitCode -eq 0 -and $entries.Count -gt 0 -and
    -not @($entries | Where-Object { -not ($_.path.StartsWith($StorePrefix) -and $_.status -cmatch '^[AM]') }))
if ($tipOverflow) { $touched += "(the tip touches more than $MaxTipEntries paths; it was not read past that)" }
$script:facts.tip = $touched

# The ledger's real home, not a path derived from whichever drive the repository sits on -- that
# built a path on the repository's own drive, which has no such directory, and reported "0 runs"
# without ever having looked. Absence at the wrong path reads exactly like absence.

# EVERY SLOT HAS A STORE, and scanning one of them is the same defect one level up (#911).
#
# The fleet runs at most one gate per disk, each behind its own lock: the HDD's at
# `D:/graphhelm-slot/SLOT.lock` and the SSD's at `E:/graphhelm-slot/SLOT.lock`, and a lane picks
# between them with `SLOT_LOCK` and `GRAPHHELM_SLOT_DIR`. Each writes its manifests beside its own
# lock. Scanning only `D:` therefore made EVERY run taken on the SSD slot report
# "no independent record corroborates it" -- measured three times in one night, on #916, #921 and
# #922, each with a positive control on both stores:
#
#     D:\graphhelm-slot\gate-runs   146 files   'e7b9915e3db7' -> 0
#     E:\graphhelm-slot\gate-runs     8 files   'e7b9915e3db7' -> 1
#
# The manifest was in a slot store every time. The scan looked in one of two, and the #709 warning
# it emitted -- a SATISFIED verdict resting on a manifest written by the author of the code it
# vouches for -- is exactly the warning a reader is meant to weigh. Emitting it on runs that DO
# have an independent record teaches the reader to skip it.
#
# DERIVED, NOT LISTED. A hand-written pair of paths is the population the author wrote: a third
# slot added tomorrow reproduces this defect with every cell still green. The stores come from the
# same source the LOCKS come from -- `GRAPHHELM_SLOT_DIR` when a lane exported it, plus the two
# defaults `slot-claim.sh` and this script already know -- so "where a lock lives" and "where its
# manifests are looked for" cannot drift apart. `-LedgerDirectory` still overrides everything,
# because a caller naming a store means that store and no other.
$slotStores = if ($LedgerDirectory) { @($LedgerDirectory) }
    else {
        @(@($env:GRAPHHELM_SLOT_DIR, 'D:\graphhelm-slot', 'E:\graphhelm-slot') |
            Where-Object { $_ } |
            ForEach-Object { Join-Path $_ 'gate-runs' } |
            ForEach-Object { [System.IO.Path]::GetFullPath($_).TrimEnd([System.IO.Path]::DirectorySeparatorChar) } |
            Select-Object -Unique)
    }
$presentStores = @($slotStores | Where-Object { Test-Path -LiteralPath $_ })
if ($presentStores.Count -eq 0) {
    $script:facts.scanNotes += ("no ledger found at any slot store (" +
        (($slotStores | ForEach-Object { Format-PathForOutput -Path $_ }) -join ', ') +
        ") -- which is not the same as a ledger holding no runs")
} else {
    # THE LEDGER IS A STORE LIKE THE OTHER ONE, and it had neither ceiling. It accumulates a file
    # per gate run forever, so "many retries" is its NORMAL state rather than an attack, and one
    # oversized file was read whole before anything asked whether it was even about this head. A
    # corroboration source that can stop the verdict from being returned is worse than none.
    # THE CAP APPLIES TO DISCOVERY, NOT AFTER IT. `Sort-Object` has to hold every FileInfo before
    # `Select-Object` can drop any, so the previous form bounded what was PARSED while discovery and
    # sorting still grew with a ledger that gains a file per gate run forever.
    #
    # The gate names its manifests `<HEAD12>-<timestamp>.json`, so the head this run is about is in
    # the NAME: the scan asks the filesystem for the two prefixes that can possibly corroborate this
    # verdict instead of reading the whole ledger and discarding almost all of it. Bounded per
    # prefix as well, because a name pattern is not a guarantee.
    $ledgerFiles = @()
    foreach ($prefix in @(@($Head, $parent) | Where-Object { $_ -and $_.Length -ge 12 } |
                ForEach-Object { $_.Substring(0, 12) } | Select-Object -Unique)) {
        # ONE PAST THE CEILING, so truncation is DETECTABLE. Taking exactly the ceiling keeps an
        # unspecified provider-ordered subset and says nothing: a corroborating GREEN retry or a
        # disagreeing RED attempt can vanish from the evidence, and the answer changes with
        # filesystem enumeration order while claiming no witness was found.
        # ACROSS EVERY PRESENT STORE, and the ceiling is applied to the COMBINED set rather than
        # per store: a budget is a guard, and looping stores with a per-store cap would have
        # doubled it silently -- the same "less refusal arrives inside a refactor" shape #922 was
        # reviewed for.
        $prefixFiles = @($presentStores |
                ForEach-Object { Get-ChildItem -LiteralPath $_ -Filter "$prefix-*.json" -File -ErrorAction SilentlyContinue } |
                Select-Object -First ($MaxManifests + 1))
        if ($prefixFiles.Count -gt $MaxManifests) {
            $script:facts.scanNotes += ("more than $MaxManifests ledger files start with $prefix, so the scan was " +
                'TRUNCATED: a corroborating or disagreeing record may not have been read')
            $prefixFiles = @($prefixFiles | Select-Object -First $MaxManifests)
        }
        $ledgerFiles += $prefixFiles
    }
    # And the limit is NAMED rather than left to look like an empty ledger: a manifest written under
    # some other naming is not seen by this scan, and corroboration that was never looked for must
    # not read as corroboration that was looked for and missing.
    # A NOTE ABOUT THE SCAN IS NOT A RECORD, and mixing them was a regression I introduced one
    # commit ago: this line is unconditional, so `corroboration` was never empty, and the test for
    # "no independent record corroborates this" stopped firing. The #709 warning -- the one saying a
    # SATISFIED verdict rests on a manifest written by the author of the code it vouches for --
    # disappeared exactly when the ledger directory existed and held nothing matching. Adding a
    # sentence about the search made the search look successful.
    $script:facts.scanNotes += ("the ledger was scanned by run-name prefix (" +
        (($presentStores | ForEach-Object { Format-PathForOutput -Path $_ }) -join ', ') +
        "), so a manifest stored under another naming is not looked for here")
    foreach ($file in $ledgerFiles) {
        # THE SAME BUDGET, NOT A SECOND ONE. `$MaxStoreBytes` bounded the committed store and left
        # the ledger unbounded, which is the per-file-versus-per-run mistake one directory over:
        # two thousand entries just under the per-file ceiling, in each of two prefixes, is four
        # gigabytes read before a verdict. The budget is per RUN of this verifier and both stores
        # spend from it, so "how much will this read" has one answer instead of two that are each
        # individually true.
        $storeBytes += $file.Length
        if ($storeBytes -gt $MaxStoreBytes) {
            Write-Broke -Reason ("the records this verdict would read total more than $MaxStoreBytes bytes across the " +
                'committed store and the ledger, which is more than this tool will read. That is a store to look at, ' +
                'not a verdict to trust')
        }
        if ($file.Length -gt $MaxManifestBytes) {
            $script:facts.scanNotes += "$($file.Name): $($file.Length) bytes, beyond the $MaxManifestBytes this tool will read"
            continue
        }
        $body = $null
        try { $body = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json } catch {
            # NAMED, not skipped. The durable copy is the only independent witness this tool has, and
            # a corrupt one is worth more said than swallowed -- most of all under SATISFIED, where
            # the committed copy is accepted and nobody looks again. Same rule the committed store
            # already follows: absent and corrupt ask for opposite things.
            $script:facts.disagreement += ("the ledger entry $($file.Name) names this head and does not parse as JSON, " +
                'so the independent record of that run is corrupt')
            continue
        }
        if ($body -isnot [System.Management.Automation.PSCustomObject]) {
            $script:facts.disagreement += ("the ledger entry $($file.Name) parses but its JSON root is not an object, " +
                'so it holds no fields to corroborate anything')
            continue
        }
        # THE TWIN COMPARISON RUNS FIRST, and the head filter after it. Twins are matched by NAME --
        # the run identity the gate writes both copies under -- while the head filter answers a
        # DIFFERENT question, which record can corroborate THIS verdict. Running the filter first
        # dropped the most interesting thing this scan can find: a committed manifest naming this
        # head whose same-named ledger twin names a different one, two copies of one run disagreeing
        # about which commit it judged, discarded before the comparison ever ran.
        # A DISAGREEMENT IS A FINDING, NOT A TIEBREAK. If the local ledger and the committed store
        # say different things about the same head, that is worth printing loudly and worth nobody
        # resolving silently -- two records of one run that do not agree is a fact about the
        # recording, and letting either one win would hide it.
        # BY RUN IDENTITY -- the file name the pair was written under -- and not by head. The gate
        # writes the durable copy and the committable copy of ONE run under the same name, and a
        # retry on the same commit legitimately leaves a RED attempt and a later GREEN attempt in
        # the ledger. Joining by head made those two attempts look like two disagreeing copies of a
        # single run, so a correct SATISFIED reported store corruption that was really just a retry.
        foreach ($committed in @($manifests | Where-Object { (Test-SameText ([System.IO.Path]::GetFileName($_.name)) $file.Name) })) {
            # BOTH SIDES BY SHAPE. These two values came from ConvertFrom-Json on different files,
            # so collection semantics applied on both, and casting both sides made `@('a')` and 'a'
            # compare EQUAL -- a weaker check on the same field the candidate loop guards strictly.
            # EVERY FIELD THE PROOF USES, not the two I happened to think of. `status` and `headSha`
            # agreeing while `pullRequest`, `pushed` or `dirtyDiffHash` disagree is the interesting
            # case and the one an author would produce: flipping the committed copy's pull request
            # to the requested number and `pushed` to true reached SATISFIED while the independent
            # record said otherwise, and nothing printed.
            foreach ($fieldName in @('status', 'headSha', 'pullRequest', 'pushed', 'dirtyDiffHash')) {
                $kind = switch ($fieldName) {
                    'pullRequest' { 'int' }
                    'pushed' { 'bool' }
                    'dirtyDiffHash' { 'null' }
                    default { 'string' }
                }
                $left = Read-ManifestField -Manifest $body -Name $fieldName -Kind $kind
                $right = Read-ManifestField -Manifest $committed.body -Name $fieldName -Kind $kind
                # Two records of one run that DISAGREE ABOUT BEING UNREADABLE is still a
                # disagreement: one of them holds a value the other does not.
                # TWO UNREADABLE VALUES ARE NOT AGREEMENT. When both sides fail the reader -- two
                # different non-null dirtyDiffHash strings, say -- `ok` is false on both, the
                # equality branch below never runs, and the pair reads as consistent. That is an
                # instrument failing toward the wrong colour: it cannot READ the values, which is a
                # different fact from the values matching.
                #
                # The raw values are compared in that case, and only in that case: they never reach
                # the output, only the decision of whether to report a disagreement.
                if (-not $left.ok -and -not $right.ok) {
                    $leftRaw = if (Test-NameIsPresent -Object $body -Name $fieldName) { [string]$body.$fieldName } else { '<absent>' }
                    $rightRaw = if (Test-NameIsPresent -Object $committed.body -Name $fieldName) { [string]$committed.body.$fieldName } else { '<absent>' }
                    if (-not (Test-SameText $leftRaw $rightRaw)) {
                        $script:facts.disagreement += ("the slot ledger and the committed $($committed.name) are the two copies of ONE run " +
                            "and neither has a readable ${fieldName}, but the two unreadable values DIFFER: " +
                            "ledger says $($left.why), committed says $($right.why)")
                    }
                } elseif ($left.ok -ne $right.ok) {
                    $script:facts.disagreement += ("the slot ledger and the committed $($committed.name) are the two copies of ONE run " +
                        "and only one of them has a readable ${fieldName}: ledger says $($left.why), committed says $($right.why)")
                } elseif ($left.ok -and -not (Test-SameText $left.value $right.value)) {
                    $script:facts.disagreement += ("the slot ledger and the committed $($committed.name) are the two copies of ONE run " +
                        "and they disagree on ${fieldName}: ledger says $($left.value), the committed copy says $($right.value)")
                }
            }
            # #993 (Codex, "compare landing evidence between manifest twins"): `mergeTarget` became
            # authorisation evidence (Test-ManifestLandingSnapshot reads it) but the loop above did
            # not compare it, so a committed copy saying `supplied-pr` beside a ledger saying
            # `non-pr` read as consistent. Its subfields are compared by the same rule as the scalar
            # fields: a twin absent on one side, or any subfield differing, is a disagreement.
            $ledgerTarget = if (Test-NameIsPresent -Object $body -Name 'mergeTarget') { $body.PSObject.Properties['mergeTarget'].Value } else { $null }
            $committedTarget = if (Test-NameIsPresent -Object $committed.body -Name 'mergeTarget') { $committed.body.PSObject.Properties['mergeTarget'].Value } else { $null }
            if (($null -eq $ledgerTarget) -ne ($null -eq $committedTarget)) {
                $script:facts.disagreement += ("the slot ledger and the committed $($committed.name) are the two copies of ONE run " +
                    "and only one of them carries a mergeTarget snapshot")
            } elseif ($null -ne $ledgerTarget) {
                foreach ($sub in @('mode', 'ref', 'head', 'sha', 'mergeBase', 'pullRequest')) {
                    $l = if (Test-NameIsPresent -Object $ledgerTarget -Name $sub) { [string]$ledgerTarget.$sub } else { '<absent>' }
                    $r = if (Test-NameIsPresent -Object $committedTarget -Name $sub) { [string]$committedTarget.$sub } else { '<absent>' }
                    if (-not (Test-SameText $l $r)) {
                        $script:facts.disagreement += ("the slot ledger and the committed $($committed.name) are the two copies of ONE run " +
                            "and they disagree on mergeTarget.${sub}: ledger says $l, the committed copy says $r")
                    }
                }
            }
        }
        $ledgerHeadField = Read-ManifestField -Manifest $body -Name 'headSha' -Kind 'string'
        if (-not $ledgerHeadField.ok) {
            # Not skipped in silence: a ledger entry nobody can read is a fact about the ledger.
            $script:facts.scanNotes += "$($file.Name): $($ledgerHeadField.why), so it corroborates nothing"
            continue
        }
        # From here on the question is corroboration, so an entry about another head is simply not
        # about this verdict -- and its twin disagreement, if any, has already been reported above.
        if ($ledgerHeadField.value -cnotin @($Head, $parent)) { continue }
        $ledgerStatusField = Read-ManifestField -Manifest $body -Name 'status' -Kind 'string'
        $ledgerPushedField = Read-ManifestField -Manifest $body -Name 'pushed' -Kind 'bool'
        $script:facts.corroboration += ("$($file.Name): status=" +
            $(if ($ledgerStatusField.ok) { $ledgerStatusField.value } else { "<$($ledgerStatusField.why)>" }) +
            ' pushed=' + $(if ($ledgerPushedField.ok) { $ledgerPushedField.value } else { "<$($ledgerPushedField.why)>" }))
        # A SECOND WITNESS HAS TO AGREE WITH WHAT IT IS WITNESSING. Everything the scan touched was
        # landing in one list, and the SATISFIED path suppressed the "committed manifest alone"
        # warning whenever that list was non-empty -- so an oversized file nobody read, an entry
        # with an unreadable head, or a RED record saying the opposite all made an author-written
        # GREEN manifest look independently corroborated. Listing a record is not agreeing with it.
        # A WITNESS IS HELD TO THE PROOF, NOT TO TWO OF ITS FIELDS. GREEN and pushed were enough to
        # suppress the warning, so a ledger record naming another pull request, or carrying a
        # non-null dirtyDiffHash, or naming the parent of an impure tip, counted as independent
        # corroboration for something it does not actually support.
        $ledgerWhy = @(Test-ManifestVouches -Manifest $body -Head $Head -Parent $parent -TipIsPure $tipIsPure `
                -PullRequest $PullRequest -Entries $entries -StorePrefix $StorePrefix)
        if ($ledgerWhy.Count -eq 0) {
            $script:supportingWitness = $true
        } else {
            $script:facts.scanNotes += ("$($file.Name) does not vouch for this head either, so it is not a second " +
                'witness: ' + ($ledgerWhy -join '; '))
        }
    }
}

# Collected BEFORE any verdict is written, because a verdict that exits early would otherwise
# carry no corroboration at all -- and SATISFIED is the outcome where a disagreeing ledger matters
# most: it is the one nobody re-examines.
# EVERY CANDIDATE IS EVALUATED BEFORE ANY VERDICT IS WRITTEN. Returning on the first good manifest
# discarded whatever the loop had already found: an earlier committed RED for the same head vanished
# from a SATISFIED answer, so a retry read as an unqualified first-time pass. The verdict is still
# decided by the good record; the others are reported beside it.
$evaluated = @()
foreach ($candidate in $mine) {
    $m = $candidate.body
    # THE SAME PREDICATE THE LEDGER IS HELD TO, called from one place so the two cannot drift.
    $why = @(Test-ManifestVouches -Manifest $m -Head $Head -Parent $parent -TipIsPure $tipIsPure `
            -PullRequest $PullRequest -Entries $entries -StorePrefix $StorePrefix)
    $headField = Read-ManifestField -Manifest $m -Name 'headSha' -Kind 'string'
    $evaluated += ,([ordered]@{ name = $candidate.name; matched = $headField.value; why = $why })
}

$failures = @($evaluated | Where-Object { $_.why.Count -gt 0 } | ForEach-Object { "$($_.name): " + ($_.why -join '; ') })
$good = @($evaluated | Where-Object { $_.why.Count -eq 0 })
if ($good.Count -gt 0) {
    $winner = $good[0]
    $script:facts.matched = if (Test-SameText $winner.matched $Head) { 'head' } else { 'parent' }
    # A verdict decided by a good manifest does not make a broken neighbour disappear. It is
    # reported with the answer rather than instead of it -- skipping it silently is how a store
    # rots without anyone learning that it did.
    $script:facts.unreadable = $unreadable
    $script:facts.otherRecords = $failures
    # THE COMMITTED STORE IS WRITTEN BY THE AUTHOR, and this tool cannot tell a manifest the
    # gate wrote from one somebody typed. Nothing here closes that -- it needs a signature, and
    # that is #709 -- so a SATISFIED verdict that has no independent corroboration says so
    # rather than reading as proof. The slot ledger is the only second witness available today
    # and it lives on the machine that ran the gate, which is the point.
    # Records only. The `-cnotmatch '^no ledger found'` filter that used to live here was doing
    # this job by pattern-matching one known note out of the list, which held for exactly as long
    # as nobody added a second note -- and then I added one.
    if (-not $script:supportingWitness) {
        $script:facts.disagreement += ('this verdict rests on the committed manifest alone: no independent ' +
            'record corroborates it, and a manifest is written by the same author as the code it vouches for (#709)')
    }
    # #903: name the coverage in the same breath as the verdict. Looked up by name because the
    # evaluation carries the verdict fields, not the document.
    $winnerBody = @($manifests | Where-Object { $_.name -eq $winner.name } | ForEach-Object { $_.body }) |
        Select-Object -First 1
    $scopeNote = Format-ScopeNote -Body $winnerBody
    if (-not [string]::IsNullOrWhiteSpace($scopeNote)) { $script:facts.scanNotes += $scopeNote }
    $reason = "$($winner.name) vouches for $Head" +
        $(if (-not (Test-SameText $winner.matched $Head)) { " (it names the parent $parent, and the tip touches only $StorePrefix)" } else { '' })
    if ($failures.Count -gt 0) {
        $reason += ("`n    and the store holds records for this pull request that do NOT vouch for it:`n    " +
            ($failures -join "`n    "))
    }
    Write-Verdict -Verdict 'SATISFIED' -Reason $reason -Code $ExitSatisfied
}

# The slot's own ledger, CORROBORATION ONLY, printed after the verdict is already decided. A rule
# that quietly consults a store nobody else can read is not a rule the repository has either.
# AND IT CARRIES WHAT IT KNOWS. The ABSENT and SATISFIED paths both report unreadable records and
# this one did not, so a store holding a broken manifest beside a merely-wrong one reported the
# wrong one and hid the broken one entirely -- in the human output and in the JSON.
$script:facts.unreadable = $unreadable
$reason = "a manifest names pull request #$PullRequest and does not vouch for ${Head}:`n    " + ($failures -join "`n    ")
if ($unreadable.Count -gt 0) {
    $reason += ("`n    and the store also holds records that could not be read at all:`n    " + ($unreadable -join "`n    "))
}
Write-Verdict -Verdict 'NOT' -Reason $reason -Code $ExitNot
