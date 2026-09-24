<#
.SYNOPSIS
    What a squash of this pull request would CLOSE, read from both texts that decide it (#757).

.DESCRIPTION
    GitHub links a closing keyword from two different places and they are not the same text:

      the PR BODY        is what `closingIssuesReferences` reports, and what the PR page shows;
      the COMMIT MESSAGES are what a SQUASH carries into the commit that lands on the default
                         branch, and therefore what actually fires on merge.

    Twice in this repository an issue was closed that the pull request said it would not (#675 on
    2026-09-01, #746 on 2026-09-03), and the second time the sentence responsible was a NEGATION
    written to prevent exactly that: `This does NOT close #717.` still contains `close #717`, and a
    parser looking for a keyword next to a number does not read English. The issue closed two
    seconds after the merge.

    A QUALIFIER is not a guard either, and it is the harder one to see because it is what a
    CAREFUL author writes. Caught by hand on #781 while this program was being written:
    `SCOPE, STATED: this closes #710's second finding only.` -- narrowing to a human, a close
    to the parser, in a commit whose pull request body correctly said `Refs #710`.

    THE INSTRUMENT LESSON, which is why this program exists rather than a paragraph:
    `closingIssuesReferences` is precise, authoritative-sounding, and scoped to neither text this
    program reads -- see the CALIBRATION for a case where it reports a closure NO text names. Trusting
    it retired the commit-message check that was the one actually gating. A stronger instrument that
    answers a NARROWER question is worse than a weak one that answers yours, because it reforms the
    doubt that would have made you look.

    So the gate is the UNION of both texts, compared against the closures the operator says they
    intend. Not the body. Not the commits. Both, together, equal to a stated intent.

    THIS PROGRAM NEVER CONSULTS `closingIssuesReferences`, and there is a second, independent
    reason beyond the one above. It is not merely a narrower reading of the same texts -- it is a
    THIRD text with a scope condition of its own: the BASE BRANCH. Measured on 2026-09-04 by the
    GraphHelm ISSUES 2 lane, on three pull requests whose body and commits each carried exactly one
    closing keyword:

      PR 767, base main                             linked: 546
      PR 770, base main                             linked: 708
      PR 779, base issue-708-reader-silence-grace   linked: (empty)

    GitHub auto-links closing keywords only on a pull request targeting the DEFAULT branch, so the
    linked set is empty for every stacked pull request -- while the squash still carries `Closes
    #617` into whatever it lands on. A gate reading that field would call a stacked pull request
    harmless while two of its texts declare a closure: the same defect as the one this program was
    built for, with the sign flipped -- there the field reported the intended closure and missed the
    accidental one, here it reports none at all.

    Both directions say the same thing. That field is not one of the halves and must never be
    allowed to answer alone.

    EVERY TEXT THAT DECIDES A CLOSURE HAS ITS OWN SCOPE CONDITION, AND NO TWO ARE THE SAME. This
    table is the point of the program, and it is worth more than any single rule derived from it,
    because the rules keep turning out to be about one instance:

      TEXT                        WHO READS IT           SCOPED TO            HOW IT GOES BLIND
      ------------------------    -------------------    -----------------    -----------------------
      closingIssuesReferences     the PR page, bots      the DEFAULT branch   silent on any stacked PR
      the PR BODY                 this program           nothing              --
      gh --json commits           this program           the BASE BRANCH      blind below the base
      the PR TITLE                this program           nothing              --

    The middle column is the one that bites, and it bites in a specific way: the answer changes
    under a git operation that touches NEITHER THE CODE NOR THE INTENT. Retarget a stacked pull
    request and the commit list grows; this program's verdict can go from green to red with no
    edit to anything a human wrote.

    So the program REFUSES a pull request whose base is not the merge target rather than reporting
    a green it cannot back -- see `Test-MergeTargetBase`, and #779 for the measurement. The rule
    that follows, for anyone running this by hand: RUN IT AFTER THE RETARGET, NEVER BEFORE.

    THE TITLE WAS THE THIRD BLIND SPOT AND IT WAS THIS PROGRAM'S OWN. A squash's default SUBJECT
    is the pull request title plus " (#N)" -- measured on this repository, where #767, #770 and
    #613 landed with a subject exactly equal to their title. A closing keyword in a title fires,
    and until #792 this program read only the body and the commits. Nothing in 200 pull requests
    here has ever carried one, so it was a gap and not a wound; it is closed because "no instance
    yet" is not a property of the instrument.

    Which is the whole lesson, stated once: THREE AUTHORITATIVE-SOUNDING SOURCES, THREE DIFFERENT
    BLIND SPOTS, AND THE FIELD NAMED AFTER THE JOB IS THE LEAST TRUSTWORTHY OF THEM.

.PARAMETER Number
    The pull request to read.

.PARAMETER Closes
    The issue numbers this pull request is INTENDED to close, with or without a leading `#`.

    For a pull request that closes NOTHING -- a partial delivery that says `Refs #N` on purpose --
    pass **`-Closes none`**.

    NOT `-Closes @()`: `-File` passes arguments as TEXT, so `@()` arrives as three literal
    characters. That is not hypothetical -- it is how this parameter was first used against four
    real pull requests, each reporting a missing closure for an issue named `@()`.

    And NOT `-Closes ''` either, which this documentation prescribed until someone ran it from a
    PowerShell prompt: there the empty LITERAL is dropped while the native command line is built,
    so it never reaches the binder and the run dies with "Missing an argument for parameter
    'Closes'" -- an error about the wrong thing, from a layer this program cannot explain from. It
    survives from bash and from cmd -- and NOT when splatted, which the table below records as a
    correction: splatting the argument array was believed to work and does not. It survives from two
    launchers out of four, which is exactly why it went unnoticed.

    Anything that is not digits after its `#`, and is not the word `none`, is refused by name.

    Required, and deliberately not defaulted: a check that derives the intent from the same text it
    is checking cannot disagree with it, and a check that cannot disagree is decoration. The
    operator states the intent, and this program says whether the texts agree.

.PARAMETER Repository
    `owner/name`. Defaults to whatever `gh` resolves for the current directory.

.INPUTS
    None. The texts are fetched with `gh`.

.OUTPUTS
    Exit 0 when the union equals the intent, 1 when it does not, 2 when the texts could not be read
    and therefore nothing was measured.

    The report also prints a `linked` line: GitHub's own `closingIssuesReferences`, shown as a
    LABELLED SECOND READING THAT DECIDES NOTHING. It is not consulted, for the reasons in the
    DESCRIPTION and in the CALIBRATION below -- it answers a different question from the one this
    program asks, and it does not read the same texts -- and adding it to the verdict would
    reintroduce exactly the instrument that retired the commit-message check.

    Displaying it is a different act from consulting it. When the three texts agree and this field
    is empty, the operator is looking at a keyword GitHub will not link (#873) and the squash
    message is what will do the closing. Without the line that disagreement is silent, which is the
    one state where a tighter-on-purpose instrument looks identical to a broken one.

    Three states, kept apart: a number, `(none)` when GitHub linked nothing, and `(unread)` when the
    field never arrived. `(unread)` is NOT `(none)` -- an absence that arrived as a failure must
    never be read as a finding.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File ci/closing-keywords.ps1 -Number 746 -Closes 735

.NOTES
    CALIBRATION, recorded here rather than in a comment somewhere else, because a check whose
    relationship to the real parser is unwritten gets "fixed" until it agrees:

    this program's match is TIGHTER than GitHub's, and the cause is a CODE SPAN. Measured on four
    pull requests on 2026-09-05 (#873). The two on the right are the positive control -- without
    them an empty answer from the API is indistinguishable from a broken query -- and the trailing
    period appears in one of each pair, so it is not the period:

      #865  body `` `Closes #796`. ``   linked: (none)      #856  body `Closes #822`   linked: 822
      #862  body `` `Closes #815` ``    linked: (none)      #833  body `Closes #751.`  linked: 751

    The difference is the backticks. This program flags all four. GitHub links the right-hand two.

    (Those four bodies have since been un-backticked, so the table is the RECORD of a measurement
    and not something a reader can re-run against them today. `the_backtick_direction` in the test
    suite is the part that still executes.)

    AND IT IS STILL HAPPENING, WHICH IS THE POINT OF WRITING IT DOWN. A fifth instance, 2026-09-07,
    a different author again, caught on a pull request that was otherwise ready to press -- two
    passes, a GREEN gate, `merge-proof` (retired 2026-09-24) SATISFIED:

      pull request 966
        body     : #841, #925        <- #925 cited inside a code span, in "Notes for reviewers"
        commits  : #841
        intended : #841
        REFUSED: merging this would close #925, which is not in the stated intent.
        closingIssuesReferences  [841]      <- GitHub linked NOTHING for 925

    The citation was explaining a stacking relationship, which is the ordinary way it happens: an
    author reaches for backticks around a coordinate and the parser reads a closure. **So a presser
    meeting this refusal on a ready pull request is meeting the DESIGNED false positive**, not a
    defect -- the union really does declare a closure the page shows none of, and the tool is right
    to make somebody look. What it costs is one reading of one sentence, which is the trade this
    direction was chosen for.

    Five instances across five authors now, and none of them noticed while writing.

    That direction is the safe one and is chosen on purpose. A false positive costs a human one
    reading of a sentence. A false negative is what closed #717. **Do not tune this to agree with
    GitHub.** It is the cheap alarm that decides when to look, never a model of the parser, and the
    cell named `the_calibration` exists to make anyone who tries watch a test go red.

    A FOURTH CAUSE, AND IT IS NOT TEXT AT ALL (#941, measured 2026-09-07). The three causes above
    are all "the texts say a closure and the field is empty". This one runs the other way -- the
    field names a closure that NO text anywhere names:

      pull request 941
        closingIssuesReferences   [902]                 <- #902 is an ISSUE, open
        title keyword             none
        body / commit keywords    `fixes #911` only     <- #911 is a PULL REQUEST, merged
        branch                    issue-902-queue-delivers

    GitHub does not treat a pull request as a closing reference, and the field agrees: it says 902
    and not 902, 911. The source is the BRANCH LINK a branch created from an issue's Development
    panel carries. It is not text, so no edit to a body or a commit message removes it, and this
    program -- which reads text -- cannot explain it or see it coming.

    So a reader can legitimately meet `body: (none) · commits: (none) · linked: 902` and conclude
    one of the two must be broken. Neither is. That row means the merge will close 902 through a
    link nobody wrote in words, and the only place to change it is the issue's Development panel.

    Which is why the `linked` column is printed rather than consulted, and why "body-scoped" was
    the wrong shorthand: the field answers a different question from a different set of inputs, and
    the four causes met on one board in one night are the argument for showing both answers rather
    than picking one. Measured by the GraphHelm ISSUES 3 lane while reviewing this program, and
    re-measured here before it was written down.

    IT CAN ANSWER WITH THE COMMITS YOU JUST REPLACED. Measured on this program's own pull request:
    a force-push amended the offending commit, the check ran seconds later, and it still reported
    the OLD commit's closure. Re-run moments afterwards, with nothing else changed, it reported the
    new one. The push succeeded and the read was stale -- GitHub had not yet reflected it -- and a
    stale read here fails toward the DANGEROUS colour in one direction and the annoying one in the
    other: it can show a closure you have already removed (annoying), and it can equally show a
    clean list for a force-push whose new commit reintroduced one (dangerous).

    So: after amending or force-pushing, confirm the commit list this program printed matches
    `git log`, or run it twice. The exit code is a verdict about what GitHub reported, and what
    GitHub reports is not always what you pushed a moment ago.

    STACKED PULL REQUESTS ARE MEASURED AGAINST THEIR OWN BASE. `gh pr view --json commits` lists the
    commits this pull request adds to ITS base, so a branch stacked on another branch does not carry
    the parent's closing keywords here -- measured on #768, stacked on #765: its commit list shows
    `#698` alone and not #765's `#699`. That is the right reading, because a squash of #768 into its
    base carries exactly those commits. But it also means RETARGETING a stacked branch to `main`
    changes what it would carry, so the check has to be re-run after the retarget rather than before
    it.

    DECLARED LIMIT: only bare `#123` is recognised. `owner/repo#123` and full issue URLs are
    keyword-linkable on GitHub and are not matched here, so a cross-repository closure is invisible
    to this program. Nothing in this repository has used that form; if one does, this is where it
    is missing rather than a thing anyone discovers.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [int] $Number,
    # BOTH attributes, and neither is what makes "closes nothing" work. AllowEmptyString lets an
    # empty string reach the body once it ARRIVES; whether it arrives depends on the launcher.
    #
    # Reported by the GraphHelm ISSUES lane as "-File drops the empty argument" after it failed on
    # their merge checklist. Measured across four launchers before acting on it:
    #
    #     bash        -> powershell -File ... -Closes ''       WORKS
    #     cmd.exe     -> powershell -File ... -Closes ""       WORKS
    #     PowerShell  -> & powershell -File ... -Closes ''     FAILS  "Missing an argument"
    #     PowerShell  -> the same call, SPLATTED array         FAILS  identically  <- corrected
    #
    # THE FOURTH ROW WAS WRONG, and it was the row a reader would have reached for. Reported by L
    # after it bit them on the #1006 census; measured again on 2026-09-08 against THIS script from
    # Windows PowerShell 5.1, both forms, and both answer with the same words:
    #
    #     closing-keywords.ps1 : Missing an argument for parameter 'Closes'.
    #     Specify a parameter of type 'System.String[]' and try again.
    #
    # Splatting the ARGUMENT ARRAY does not help, because the empty literal is dropped while
    # PowerShell builds the native command line -- before the array shape matters. The parameter is
    # `System.String[]`, which is why an argument that arrives as nothing binds as absent rather
    # than as empty. From PowerShell, launch through bash or cmd; there is no in-shell form.
    #
    # So the trigger is not `-File`; it is PowerShell's own construction of a native command line
    # dropping an empty LITERAL, in some contexts and not others. Their failure was real and their
    # mechanism was right; the scope was wider than the behaviour.
    #
    # This program's documentation used to prescribe `-Closes ''` for "closes nothing" -- the MOST
    # COMMON intent on this board, every partial delivery and every `Refs #N`. A remedy that works
    # on two launchers out of four is worse than none, because the operator who hits either of the
    # other two reads the binder's complaint as a fact about their pull request. `-Closes none`
    # survives all
    # four; see `ConvertTo-IntendedClosures`.
    [Parameter(Mandatory)] [AllowEmptyCollection()] [AllowEmptyString()] [string[]] $Closes,
    [string] $Repository,
    # The branch a squash of this pull request would LAND ON. A parameter rather than the constant
    # 'main' because it is one side of a comparison this program now REFUSES on, and a comparison
    # whose other side is hard-coded cannot be reached by a cell (#792).
    [string] $MergeTarget = 'main'
)

$ErrorActionPreference = 'Stop'

# Every keyword GitHub fires on, in every inflection it accepts. Written out rather than built from
# a stem: `close|closes|closed` is three words and `clos(e|es|ed)` is a pattern whose next reader
# has to re-derive what it covers, and this list is the whole claim the program makes.
$ClosingKeywords = @(
    'close', 'closes', 'closed',
    'fix', 'fixes', 'fixed',
    'resolve', 'resolves', 'resolved'
)

function ConvertTo-IntendedClosures {
    <#
        The operator's `-Closes` argument, turned into issue numbers or REFUSED by name.

        An unparseable token used to survive `TrimStart('#')` and be compared as though it were an
        issue, so the run reported a MISSING closure that was really the operator's own argument --
        a refusal about nothing, wearing the clothes of a finding.

        That is not a hypothetical and it is not abuse. `-File` passes every argument as TEXT, so
        `-Closes @()` -- the obvious spelling for "closes nothing", and the one the first version of
        this program's own documentation gave -- arrives as the three literal characters `@()`. It
        was used that way against four real pull requests before anyone noticed, and each one
        reported a missing closure for an issue named `@()`.

        The spelling that works from every launcher measured is the word `none`. An empty string
        works from bash and from cmd, and is dropped when PowerShell builds a native command line
        with an empty LITERAL -- so it worked for whoever wrote the documentation and failed for
        the first person to run it from a PowerShell prompt. A remedy whose success depends on the
        shell the operator happens to be in is not a remedy.

        `none` is matched OrdinalIgnoreCase, deliberately, and it is the one comparison here that
        is: an operator typing `None` or `NONE` means the same thing, and unlike a manifest field
        this token comes from a human at a prompt rather than from a machine-written file.

        In the seam rather than in the script body for the same reason the union is: a decision the
        body holds is a decision no cell can reach.
    #>
    param([Parameter(Mandatory)] [AllowEmptyCollection()] [AllowEmptyString()] [string[]] $Tokens)

    $numbers = New-Object System.Collections.Generic.List[string]
    foreach ($token in @($Tokens)) {
        $trimmed = ([string]$token).Trim().TrimStart('#')
        if ($trimmed -eq '') { continue }
        if ([string]::Equals($trimmed, 'none', [System.StringComparison]::OrdinalIgnoreCase)) { continue }
        if ($trimmed -notmatch '^\d+$') {
            return [ordered]@{ ok = $false; numbers = @(); offending = ([string]$token) }
        }
        if (-not $numbers.Contains($trimmed)) { $numbers.Add($trimmed) }
    }
    return [ordered]@{ ok = $true; numbers = @($numbers); offending = '' }
}

function Get-ClosingReferences {
    <#
        The issue numbers a closing keyword names in this text, as strings, deduplicated.

        A seam that takes a STRING and returns a set, for the same reason the other decisions in
        this directory do: every interesting case here is a SHAPE of text -- a negation, a code
        span, a keyword in a commit body rather than a PR body -- and a cell that had to create a
        real pull request to exercise one would be a test of GitHub.

        The match is case-insensitive because GitHub's is: `CLOSES #1` closes issue 1.

        There is no attempt to understand the sentence. That is the entire point. `This does NOT
        close #717.` returns 717, because GitHub returns 717, and a program that were clever enough
        to see the negation would disagree with the thing that actually presses the button.
    #>
    param([Parameter(Mandatory)] [AllowEmptyString()] [string] $Text)

    $pattern = '(?i)\b(' + ($ClosingKeywords -join '|') + ')\b\s*#(\d+)'
    $found = New-Object System.Collections.Generic.List[string]
    foreach ($match in [regex]::Matches($Text, $pattern)) {
        $number = $match.Groups[2].Value
        if (-not $found.Contains($number)) { $found.Add($number) }
    }
    # `,` before the array, the same guard `Get-IndexBlobBytes` in this directory carries and for
    # the same reason: PowerShell UNROLLS a returned collection, and an EMPTY one unrolls to
    # NOTHING -- the caller receives $null rather than a set with no members. Here that is worse
    # than a crash, because $null.Count is 0 and every "this text closes nothing" comparison would
    # keep agreeing while the function had stopped returning a set at all.
    return ,@($found)
}

function Compare-ClosureIntent {
    <#
        The two ways the texts can disagree with the operator, kept apart because they mean
        opposite things and want opposite fixes.

        UNEXPECTED: the texts close something the operator did not name. This is #675 and #746 --
        an issue closed by a sentence nobody meant as an instruction. The fix is to the text.

        MISSING: the operator names a closure the texts do not carry. That is not harmless. It
        means the intended closure depends on whichever half of the text the squash happens to
        carry, and the operator finds out afterwards. The fix is also to the text, in the other
        direction.

        Sorted numerically rather than lexically, so `#9` does not sort after `#10` in a message a
        human is about to compare by eye.
    #>
    param(
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Union,
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Intended
    )

    $normalise = { param($values) @(@($values) | ForEach-Object { ([string]$_).TrimStart('#').Trim() } |
            Where-Object { $_ -ne '' }) }
    $left = & $normalise $Union
    $right = & $normalise $Intended
    $bySize = { param($values) @($values | Sort-Object { [int]$_ } -Unique) }
    # `@( ... )` around each, for the unrolling reason recorded on `Get-ClosingReferences`: an
    # empty `Sort-Object` result is $null, and a property that is $null where a caller expects a
    # set is the same defect one level in.
    return [ordered]@{
        unexpected = @(& $bySize @($left | Where-Object { $right -notcontains $_ }))
        missing    = @(& $bySize @($right | Where-Object { $left -notcontains $_ }))
    }
}

function Test-PullRequestPayload {
    <#
        Did the answer actually CARRY the two texts, or is it merely not an error?

        Contributed by the GraphHelm ISSUES 2 lane, who hit this one layer up. Their audit ran the
        same subject twice with nothing changed and got `Closes #708` once and an empty body once;
        the body was 8908 characters both times, so the empty answer was the INSTRUMENT. Their
        one-liner discarded gh's stderr and piped into `grep`, and grep over empty stdin exits 1
        with no output -- which prints exactly what a successful fetch that found no closing
        keyword prints. **A failed fetch and a clean pull request were the same observation**, and
        the failure read as CLEAN, so the merge would have proceeded.

        This program already refuses a non-zero exit and an unparseable answer, so it does not
        inherit that. It DID inherit the layer below, and this function is why it no longer does.
        Measured before writing it: a payload of `{}` -- gh exits 0, the JSON parses, and neither
        field is there -- produced empty texts, an empty intent, and the program printed
        "the union of both texts equals the stated intent". A green over nothing at all.

        THE INVARIANT THAT MAKES IT CHECKABLE: a pull request always has at least one commit. Zero
        is not a pull request with nothing in it; it is an answer that did not carry what was asked
        for. So zero commits is HARNESS-BROKE and never a finding.

        AN EMPTY BODY IS LEGAL AND IS NOT REFUSED. That distinction is the whole care in this
        function: `body` ABSENT means the answer is the wrong shape, `body` present and empty means
        somebody wrote a pull request without a description, which is allowed and closes nothing.
        Collapsing the two would trade a false green for a false refusal, and a check that refuses
        legitimate work gets switched off.
    #>
    param([Parameter(Mandatory)] [AllowNull()] $Payload)

    if ($null -eq $Payload) {
        return [ordered]@{ ok = $false; reason = 'the answer was empty' }
    }
    $names = @($Payload.PSObject.Properties.Name)
    if ($names -notcontains 'body') {
        return [ordered]@{ ok = $false; reason = 'the answer carries no `body` field' }
    }
    if ($names -notcontains 'commits') {
        return [ordered]@{ ok = $false; reason = 'the answer carries no `commits` field' }
    }
    if (@($Payload.commits).Count -eq 0) {
        return [ordered]@{
            ok     = $false
            reason = 'the answer carries ZERO commits, and every pull request has at least one'
        }
    }
    if ($names -notcontains 'title') {
        return [ordered]@{ ok = $false; reason = 'the answer carries no `title` field' }
    }
    if ($names -notcontains 'baseRefName') {
        return [ordered]@{ ok = $false; reason = 'the answer carries no `baseRefName` field' }
    }
    # An empty base is the wrong SHAPE and never a pull request with no base. Unlike `body`, which
    # is legitimately empty when nobody wrote a description, every pull request targets a branch.
    if ([string]::IsNullOrWhiteSpace([string]$Payload.baseRefName)) {
        return [ordered]@{
            ok     = $false
            reason = 'the answer carries an EMPTY `baseRefName`, and every pull request targets a branch'
        }
    }
    return [ordered]@{ ok = $true; reason = '' }
}

function Test-MergeTargetBase {
    <#
        Is the pull request's base the branch a squash would LAND ON -- and therefore, is the
        commit list this program reads the one that would actually be carried?

        `gh pr view --json commits` is computed by GitHub AGAINST THE BASE BRANCH. On a stacked
        pull request whose base is not the merge target, the list STOPS at that base and this
        program is blind to every commit below it. Measured on #779 (2026-09-04), whose base
        branch `issue-708-reader-silence-grace` still existed after its own pull request had
        squash-merged into main:

            as it stood                    5 commits returned   fired {617}        GREEN
            base retargeted, NOT rebased   8 commits returned   fired {617, 708}   red
            rebased onto main, pushed      5 commits returned   fired {617}        green, correct

        `Closes #708` sat in the body of a commit the endpoint could not see, and #708 was
        already CLOSED. So the gate answered GREEN in a state where merging was wrong, and the
        operator's NEXT REQUIRED ACTION -- retargeting, without which nothing reaches main -- is
        what changed the answer.

        A green that expires on the next git operation is not a weaker green. It is an answer to a
        question nobody asked, and reporting it is the exact failure this program exists to
        prevent: an instrument silent about its own scope reforms the doubt that would have made
        someone look. So this REFUSES rather than warns.

        ORDINAL, because git branch names are case-sensitive and PowerShell's `-eq` is not. Nor is
        `-ceq`: case-sensitivity and culture-awareness are ORTHOGONAL, and only the second one is
        what makes a zero-weight character compare equal to nothing at all (#753).
    #>
    param(
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $Base,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $MergeTarget
    )

    if ([string]::Equals($Base, $MergeTarget, [System.StringComparison]::Ordinal)) {
        return [ordered]@{ ok = $true; reason = '' }
    }
    return [ordered]@{
        ok     = $false
        reason = ("its base is '$Base' and not '$MergeTarget', so the commit list gh returns stops " +
            "at that base and this program cannot see what lies below it")
    }
}

function Read-PullRequestTexts {
    <#
        The WHOLE path from what `gh` returned to the two texts, or a refusal naming which step
        failed.

        It lives here rather than in the script body for the third time in this program's short
        life, and the reason has not changed: a decision the body holds is a decision no cell can
        reach. `Test-PullRequestPayload` could be perfect and never called, and every cell for it
        would still pass -- which is precisely the defect this program was written about, one level
        further out.

        Three ways to fail, kept apart because they mean different things to whoever reads the
        refusal: the instrument did not run, the instrument answered something that is not JSON,
        and the instrument answered JSON that does not carry what was asked for. None of the three
        may fall through to a verdict.
    #>
    param([Parameter(Mandatory)] [AllowNull()] $Answer)

    if ($null -eq $Answer -or $Answer.exitCode -ne 0) {
        return [ordered]@{ ok = $false; reason = 'gh could not read the pull request'; body = ''; commits = ''; title = ''; base = ''; linked = (Get-LinkedReading -Payload $null) }
    }
    $payload = $null
    try {
        $payload = $Answer.text | ConvertFrom-Json
    } catch {
        return [ordered]@{ ok = $false; reason = "gh's answer did not parse as JSON"; body = ''; commits = ''; title = ''; base = ''; linked = (Get-LinkedReading -Payload $null) }
    }
    $shape = Test-PullRequestPayload -Payload $payload
    if (-not $shape.ok) {
        return [ordered]@{ ok = $false; reason = $shape.reason; body = ''; commits = ''; title = ''; base = ''; linked = (Get-LinkedReading -Payload $null) }
    }
    # Both halves of a commit message. GitHub links from the subject as well as the body, and
    # #746's offending keyword was in a body while its intended one was in a subject -- reading
    # only one half would have found exactly one of the two.
    $commitText = (@($payload.commits | ForEach-Object { @($_.messageHeadline, $_.messageBody) }) -join "`n")
    return [ordered]@{
        ok      = $true
        reason  = ''
        body    = [string]$payload.body
        commits = $commitText
        title   = [string]$payload.title
        base    = [string]$payload.baseRefName
        linked  = Get-LinkedReading -Payload $payload
    }
}

function Get-ClosureVerdict {
    <#
        The whole decision, over the two texts and the stated intent.

        It lives HERE rather than in the script body because the body cannot be tested without a
        pull request, and the claim this program makes is precisely the one the body used to hold:
        that the gate is the UNION of both texts. A version that read only the body would have
        passed every cell in this suite while being the exact defect #757 is about, because both
        halves it exercises -- the pattern and the comparison -- were individually correct in #746
        too. Counting conjuncts is not testing them.

        So the body below fetches and prints. Everything that DECIDES is in this function.
    #>
    param(
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $BodyText,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $CommitText,
        [Parameter(Mandatory)] [AllowEmptyString()] [string] $TitleText,
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Intended
    )

    $fromBody = Get-ClosingReferences -Text $BodyText
    $fromCommits = Get-ClosingReferences -Text $CommitText
    # THE TITLE IS A DECIDING TEXT AND THIS PROGRAM USED TO BE BLIND TO IT. A squash's default
    # SUBJECT is the pull request title plus " (#N)" -- measured on this repository, where #767,
    # #770 and #613 all landed with subject exactly equal to their title (#774 differs only
    # because its merger hand-wrote one). A closing keyword in a title therefore fires, and
    # neither of the two texts this program read would have carried it.
    #
    # Nothing in the 200 pull requests on this repository has ever had one, so this closes a gap
    # rather than a wound, and that is stated so the next reader does not go looking for the
    # incident.
    $fromTitle = Get-ClosingReferences -Text $TitleText
    $union = @(@($fromBody) + @($fromCommits) + @($fromTitle))
    $comparison = Compare-ClosureIntent -Union $union -Intended $Intended
    return [ordered]@{
        body       = @($fromBody)
        commits    = @($fromCommits)
        title      = @($fromTitle)
        unexpected = @($comparison.unexpected)
        missing    = @($comparison.missing)
    }
}

<#
.SYNOPSIS
    The refusal that names issues a merge would close and the operator did not intend.

.DESCRIPTION
    A FUNCTION, and deliberately inside the seam this directory's suite slices, so the cell that
    feeds this sentence back through `Get-ClosingReferences` reads the SHIPPED text rather than a
    copy of it. A copy would agree until the day somebody edits the message, which is the one day
    the cell is for.

    THE NOUN IS NOT DECORATION (#965). `Get-ClosingReferences` keys on `\b(keyword)\b\s*#(\d+)`:
    only whitespace may sit between the word and the hash. So `close #815` is a closure instruction
    and `close issue #815` is inert, while reading identically to a person.

    Without it this message could not be pasted into the two places it is most useful -- a pull
    request body, a review comment -- because quoting it re-triggers the tool on the very numbers
    being quoted. Measured on the pull request for #873: a second run refused on a number that by
    then existed only inside the quoted refusal from the first. The workaround was to elide the
    numbers, which removes the part a reader needs.

    The numbers themselves are NOT softened. Nothing here may make the message inert by saying less
    than it did; the suite asserts both halves.
#>
function Get-UnexpectedClosureRefusal {
    param([Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Numbers)
    $list = @($Numbers)
    $noun = if ($list.Count -eq 1) { 'issue' } else { 'issues' }
    $rendered = if ($list.Count -eq 0) { '(none)' } else { (@($list) | ForEach-Object { "#$_" }) -join ', ' }
    return ("[closing] REFUSED: merging this would close $noun $rendered, which is not in the " +
        "stated intent. Remember the keyword fires inside a NEGATION too: write ``Refs #N`` or " +
        "``Scope: #N stays open``, never a closing word next to a number you mean to keep open.")
}

function Get-LinkedReading {
    <#
        GitHub's own `closingIssuesReferences`, as a THREE-STATE reading.

        The third state is the whole point. `read = $false` means the field never arrived -- an old
        `gh` that does not know the name, or a fallback fetch that deliberately dropped it -- and
        rendering that as "GitHub linked nothing" would manufacture the exact disagreement this
        line exists to surface. An absence that arrived as a failure must never be read as a
        finding, and `(none)` is a finding.

        Read from the PAYLOAD rather than re-queried, so it is the same snapshot as the texts. A
        second `gh` call would answer about a different instant, and this field's whole job here is
        to be compared against those texts.
    #>
    param([Parameter(Mandatory)] [AllowNull()] $Payload)

    if ($null -eq $Payload) { return [ordered]@{ read = $false; numbers = @() } }
    $names = @($Payload.PSObject.Properties | ForEach-Object { $_.Name })
    if ($names -notcontains 'closingIssuesReferences') {
        return [ordered]@{ read = $false; numbers = @() }
    }
    # Present but null is still READ: gh returns null for a pull request GitHub linked nothing for,
    # which is the #873 state itself and must not be confused with the field being absent.
    $entries = @($Payload.closingIssuesReferences)
    $numbers = @($entries | Where-Object { $null -ne $_ } | ForEach-Object { [string]$_.number } |
        Where-Object { $_ -match '^\d+$' })
    return [ordered]@{ read = $true; numbers = @($numbers) }
}

function Format-LinkedReading {
    <#
        The three states as the operator sees them. A function rather than an inline `Write-Host`
        for the reason this file has already learned twice: a decision the script body holds is a
        decision no cell can reach.
    #>
    param([Parameter(Mandatory)] [AllowNull()] $Reading)

    if ($null -eq $Reading -or -not $Reading.read) { return '(unread)' }
    if (@($Reading.numbers).Count -eq 0) { return '(none)' }
    return (@($Reading.numbers) | ForEach-Object { "#$_" }) -join ', '
}

function Invoke-Gh {
    <#
        `gh` read through its own exit code, with stderr kept OUT of the stream this parses.

        Same trap `ci/normalize-script-eol.ps1` documents for git: under Windows PowerShell 5.1 a
        redirected native stderr line becomes a NativeCommandError and `$ErrorActionPreference =
        'Stop'` promotes it to a terminating error, so an authentication failure would kill this
        function before its own exit-code check could report HARNESS-BROKE -- the diagnostic
        unreachable in exactly the case it exists for.
    #>
    param([Parameter(Mandatory)] [string[]] $Arguments)

    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & gh @Arguments 2>$null
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [ordered]@{ exitCode = $code; text = (@($output) -join "`n") }
}

$stated = ConvertTo-IntendedClosures -Tokens $Closes
if (-not $stated.ok) {
    Write-Host ("[closing] REFUSED: -Closes was given [$($stated.offending)], which is not an issue " +
        "number. Write the digits, with or without a leading hash. For a pull request that closes " +
        "nothing, pass the word none: -Closes none") -ForegroundColor Red
    exit 1
}
$intended = @($stated.numbers)

# `closingIssuesReferences` is asked for in the SAME call as the texts, so the second reading is a
# view of one snapshot rather than of a later instant.
#
# AND IT FALLS BACK, because an unknown --json field is fatal to the WHOLE call -- measured:
# `gh pr view N --json body,notARealField` exits non-zero with "Unknown JSON field" and returns no
# body at all. Without this retry, a `gh` that did not know the name would turn a working gate into
# exit 2 for the sake of a line that decides nothing. A decorative reading must never be able to
# take the load-bearing one down with it.
$requiredFields = 'body,commits,title,baseRefName'
function Get-GhArguments {
    param([Parameter(Mandatory)] [string] $Fields)
    $built = @('pr', 'view', "$Number", '--json', $Fields)
    if ($Repository) { $built += @('--repo', $Repository) }
    return $built
}
$answer = Invoke-Gh -Arguments (Get-GhArguments -Fields "$requiredFields,closingIssuesReferences")
if ($null -eq $answer -or $answer.exitCode -ne 0) {
    $answer = Invoke-Gh -Arguments (Get-GhArguments -Fields $requiredFields)
}
$read = Read-PullRequestTexts -Answer $answer
if (-not $read.ok) {
    Write-Host ("[closing] HARNESS-BROKE: for pull request $Number, $($read.reason), so neither " +
        "text was measured. An absence that arrived as a failure must never be read as a finding.") -ForegroundColor Magenta
    exit 2
}
# BEFORE any measurement, because the texts below are only worth reading if they are the texts a
# squash would carry. See Test-MergeTargetBase: gh computes the commit list against the BASE, so a
# pull request stacked on something other than the merge target hides every commit below it.
#
# This exits 1 rather than 2. It is a finding ABOUT THE PULL REQUEST -- it is not in a state that
# can be merged correctly -- and not an instrument that failed to run. A 2 invites a caller to
# treat it as "skipped".
$target = Test-MergeTargetBase -Base $read.base -MergeTarget $MergeTarget
if (-not $target.ok) {
    Write-Host ""
    Write-Host ("[closing] REFUSED: pull request $Number cannot be gated as it stands, because " +
        "$($target.reason).") -ForegroundColor Red
    Write-Host ("           Retargeting alone does NOT fix this: it changes what GitHub COMPARES " +
        "against, not what the branch CARRIES. Rebase onto the merge target first --") -ForegroundColor Red
    Write-Host ("             git rebase --onto origin/$MergeTarget <the old base's head> <this branch>") -ForegroundColor Red
    Write-Host ("           then push, retarget, and run this again. RUN IT AFTER, NEVER BEFORE: " +
        "the answer changes with the base.") -ForegroundColor Red
    exit 1
}

$bodyText = $read.body
$commitText = $read.commits

$verdict = Get-ClosureVerdict -BodyText $bodyText -CommitText $commitText -TitleText $read.title -Intended $intended

$show = { param($values) if (@($values).Count -eq 0) { '(none)' } else { (@($values) | ForEach-Object { "#$_" }) -join ', ' } }
Write-Host ""
Write-Host "[closing] pull request $Number" -ForegroundColor Cyan
Write-Host "  title    : $(& $show $verdict.title)   <- the squash's SUBJECT line"
Write-Host "  body     : $(& $show $verdict.body)"
Write-Host "  commits  : $(& $show $verdict.commits)   <- the text a SQUASH carries"
Write-Host "  intended : $(& $show $intended)"
# A SECOND READING, PRINTED AND NOT CONSULTED. Below the four lines above and visually apart from
# them, because it is not a fifth text of equal standing -- it is what GitHub's own parser made of
# ONE of them. Nothing below reads it.
Write-Host "  linked   : $(Format-LinkedReading -Reading $read.linked)   <- GitHub's own answer, from texts and links this program cannot see. Decides nothing here."

if ($verdict.unexpected.Count -eq 0 -and $verdict.missing.Count -eq 0) {
    Write-Host "[closing] the union of all three texts equals the stated intent." -ForegroundColor Green
    exit 0
}

if ($verdict.unexpected.Count -gt 0) {
    Write-Host (Get-UnexpectedClosureRefusal -Numbers $verdict.unexpected) -ForegroundColor Red
}
if ($verdict.missing.Count -gt 0) {
    Write-Host ("[closing] REFUSED: $(& $show $verdict.missing) is intended to close and appears in neither text, " +
        "so nothing would close it. An intended closure that depends on what the squash happens " +
        "to carry is one the operator finds out about afterwards.") -ForegroundColor Red
}
exit 1
