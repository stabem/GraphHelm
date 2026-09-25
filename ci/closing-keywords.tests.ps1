# #757: the squash carries COMMIT text, not the PR body, so a closing keyword in a commit closed an
# issue the pull request said it would not. Twice -- #675 on 2026-09-01, and #746 on 2026-09-03
# where the offending sentence was a NEGATION written specifically to prevent it.
#
# Every cell here is OFFLINE and drives the two decisions directly, with strings. That is not a
# convenience: the interesting cases are SHAPES of text -- a negation, a code span, a keyword in a
# commit body rather than a PR body, a word that merely ends in a keyword -- and a cell that had to
# create a real pull request to exercise one would be a test of GitHub rather than of this program.
#
# The seam is EXTRACTED from the program rather than copied, keyword list included, so a sabotage
# that removes a keyword from the real list reddens here. A cell holding its own copy of the list
# would pass over a program that had lost one, which is the failure this suite would exist to have.
#
# Measured on Windows PowerShell 5.1.26100.9168.

$ExpectedAssertionCount = 71
$ErrorActionPreference = 'Continue'
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) {
        Write-Host "  PASS: $Message" -ForegroundColor Green
    } else {
        $script:failures++
        Write-Host "  FAIL: $Message" -ForegroundColor Red
    }
}

function Assert-Set {
    <#
        Set equality, printed as sets when it fails.

        `-join` before comparing, because PowerShell's `-eq` on two arrays FILTERS the left by the
        right and returns the matches -- a non-empty result is truthy, so `@(1,2) -eq @(1)` is
        `1`, which reads as "equal". An assertion written that way passes whenever the sets
        overlap at all.
    #>
    param(
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Actual,
        [Parameter(Mandatory)] [AllowEmptyCollection()] [string[]] $Expected,
        [Parameter(Mandatory)] [string] $Message
    )
    $left = (@($Actual) | Sort-Object) -join ','
    $right = (@($Expected) | Sort-Object) -join ','
    Assert-True -Condition ($left -ceq $right) -Message "$Message (got [$left], expected [$right])"
}

$programPath = Join-Path $PSScriptRoot 'closing-keywords.ps1'
if (-not (Test-Path -LiteralPath $programPath)) {
    Write-Host "HARNESS-BROKE: the subject is missing at $programPath" -ForegroundColor Magenta
    exit 2
}
$programText = [System.IO.File]::ReadAllText($programPath)

$seamStart = $programText.IndexOf('$ClosingKeywords = @(')
$seamEnd = $programText.IndexOf('function Invoke-Gh {')
if ($seamStart -lt 0 -or $seamEnd -le $seamStart) {
    Write-Host 'HARNESS-BROKE: the decisions were not found between their anchors' -ForegroundColor Magenta
    exit 2
}
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) "graphhelm-closing-$([guid]::NewGuid().ToString('N'))"
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
$seam = Join-Path $fixtureRoot 'seam.ps1'
[System.IO.File]::WriteAllText($seam, $programText.Substring($seamStart, $seamEnd - $seamStart),
    (New-Object System.Text.UTF8Encoding($false)))

try {
    # Dot-sourced rather than run as a child, unlike this directory's other seams. Those decide over
    # the filesystem and need a process of their own; these two are pure functions of a string, so
    # the cells can hold the RETURN VALUES instead of parsing text a child printed -- one less layer
    # between the assertion and the thing it is about.
    . $seam

    Write-Host ''
    Write-Host '-- what a closing keyword names --' -ForegroundColor Cyan

    Assert-Set -Actual (Get-ClosingReferences -Text 'Closes #735') -Expected @('735') `
        -Message 'the ordinary spelling is found'

    # THE CELL THIS PROGRAM EXISTS FOR. #746's pull request carried this sentence, written to
    # PREVENT the closure, and #717 closed two seconds after the merge. A negation is not a guard in
    # a field a machine reads, and a program clever enough to see the negation would DISAGREE with
    # the thing that actually presses the button.
    Assert-Set -Actual (Get-ClosingReferences -Text 'This does NOT close #717.') -Expected @('717') `
        -Message 'a NEGATION still names the issue: the parser does not read English (#746)'

    $everyKeyword = 'close #1 closes #2 closed #3 fix #4 fixes #5 fixed #6 resolve #7 resolves #8 resolved #9'
    Assert-Set -Actual (Get-ClosingReferences -Text $everyKeyword) `
        -Expected @('1', '2', '3', '4', '5', '6', '7', '8', '9') `
        -Message 'all nine inflections fire, so none of them is a safe spelling'

    Assert-Set -Actual (Get-ClosingReferences -Text 'CLOSES #1') -Expected @('1') `
        -Message 'and the match is case-insensitive, because GitHub s is'

    Write-Host ''
    Write-Host '-- what is safe to write, and what only looks like a keyword --' -ForegroundColor Cyan

    Assert-Set -Actual (Get-ClosingReferences -Text 'Refs #752') -Expected @() `
        -Message 'Refs #N names an issue without closing it'

    Assert-Set -Actual (Get-ClosingReferences -Text 'Scope: #710 stays open') -Expected @() `
        -Message 'and so does the sentence the guidance tells people to write instead'

    # A discrimination cell, not a coverage one. Without it the pattern could drop its word
    # boundary -- `close` inside `foreclose` -- and every cell above would still pass, because they
    # all feed text where the keyword really is a word.
    Assert-Set -Actual (Get-ClosingReferences -Text 'foreclose #9 and reclosed #8') -Expected @() `
        -Message 'a word that merely ENDS in a keyword does not fire'

    Assert-Set -Actual (Get-ClosingReferences -Text 'Closes #5 and closes #5 again') -Expected @('5') `
        -Message 'the same issue named twice is one closure, not two'

    # A LIVE CATCH, not a constructed one. Found by the GraphHelm ISSUES lane in their own commit
    # on #781 while this pull request was open, and contributed as a fixture:
    #
    #     SCOPE, STATED: this closes #710's second finding only.
    #
    # Their intent was `Refs #710` and their pull request body said exactly that. The commit
    # message would have closed the issue on squash anyway.
    #
    # It is a better fixture than anything above it, because it is the sentence a CAREFUL author
    # writes. Every other case here is either a bare keyword or an explicit negation; this one is a
    # SCOPE STATEMENT, which is the form someone reaches for precisely when they are trying not to
    # over-close. The qualifier reads to a human as a narrowing and to the parser as a close.
    #
    # It also closes a hole in this suite rather than merely illustrating one. The pattern already
    # matched it, so the program would have refused that merge -- but no cell fed it a qualifier, so
    # a later "improvement" that skipped a keyword followed by a possessive or a narrowing phrase
    # would have left every existing cell green.
    Assert-Set -Actual (Get-ClosingReferences -Text 'SCOPE, STATED: this closes #710''s second finding only.') `
        -Expected @('710') `
        -Message 'a SCOPE STATEMENT still closes: the qualifier narrows for a human and not for the parser'

    # And the spelling that actually narrows, so the guidance has a demonstrated alternative rather
    # than an asserted one.
    Assert-Set -Actual (Get-ClosingReferences -Text 'SCOPE: this addresses the second finding of #710 only. Refs #710') `
        -Expected @() `
        -Message 'while the same scope written without a closing word closes nothing'

    Write-Host ''
    Write-Host '-- the stated intent, which is an argument and can be nonsense --' -ForegroundColor Cyan

    $one = ConvertTo-IntendedClosures -Tokens @('735')
    Assert-True -Condition ($one.ok -and (@($one.numbers) -join ',') -ceq '735') `
        -Message 'a number is a number'
    $hashed = ConvertTo-IntendedClosures -Tokens @('#735')
    Assert-True -Condition ($hashed.ok -and (@($hashed.numbers) -join ',') -ceq '735') `
        -Message 'and so is the same number with its hash'

    # The spelling for a partial delivery that closes nothing on purpose -- `Refs #N` in the body,
    # nothing in the commits. Four open pull requests in this repository are exactly that shape.
    $nothing = ConvertTo-IntendedClosures -Tokens @('')
    Assert-True -Condition ($nothing.ok -and @($nothing.numbers).Count -eq 0) `
        -Message "an empty string is the intent CLOSES NOTHING, not an error"

    # THE ONE THAT ALREADY HAPPENED. `-File` passes arguments as TEXT, so `-Closes @()` -- the
    # obvious spelling, and the one this program's own first documentation gave -- arrives as three
    # literal characters. It was used that way against four real pull requests and each reported a
    # missing closure for an issue named `@()`: a refusal about nothing, wearing the clothes of a
    # finding.
    $literal = ConvertTo-IntendedClosures -Tokens @('@()')
    Assert-True -Condition ((-not $literal.ok) -and $literal.offending -ceq '@()') `
        -Message 'a token that is not digits is REFUSED BY NAME rather than compared as an issue'

    $mixed = ConvertTo-IntendedClosures -Tokens @('735', 'abc')
    Assert-True -Condition ((-not $mixed.ok) -and $mixed.offending -ceq 'abc') `
        -Message 'and one bad token refuses the whole argument, naming the token and not the position'

    Write-Host ''
    Write-Host '-- an answer that is merely not an error is not an answer --' -ForegroundColor Cyan
    #
    # Contributed by the GraphHelm ISSUES 2 lane, who hit this one layer up: their audit ran the
    # same subject twice with nothing changed and got `Closes #708` once and an empty body once.
    # The body was 8908 characters both times, so the empty answer was the INSTRUMENT -- and their
    # command could not tell a failed fetch from a clean pull request, because grep over empty
    # stdin prints exactly what a successful fetch with no keyword prints. The failure read as
    # CLEAN, which is the direction that lets a merge through.
    #
    # This program refuses a non-zero exit and an unparseable answer, so it did not inherit that.
    # It DID inherit the layer below, and these cells are why it no longer does. MEASURED before
    # they were written: a payload of `{}` -- gh exits 0, the JSON parses, neither field is there --
    # produced empty texts, an empty intent, and the verdict "the union of both texts equals the
    # stated intent". A green over nothing at all.

    $good = ('{"body":"Closes #1","commits":[{"messageHeadline":"h","messageBody":"b"}],' + '"title":"fix: an ordinary title","baseRefName":"main"' + '}') | ConvertFrom-Json
    Assert-True -Condition (Test-PullRequestPayload -Payload $good).ok `
        -Message 'CONTROL: a real answer passes, or the refusals below are a function that refuses everything'

    # The exact payload that measured green before this existed.
    $nothing = '{}' | ConvertFrom-Json
    Assert-True -Condition (-not (Test-PullRequestPayload -Payload $nothing).ok) `
        -Message 'an answer carrying NEITHER field is refused, not read as two empty texts'

    $noCommits = '{"body":"Closes #1"}' | ConvertFrom-Json
    Assert-True -Condition (-not (Test-PullRequestPayload -Payload $noCommits).ok) `
        -Message 'and so is one with a body and no commits field at all'

    # THE MIRROR, AND MY OWN SABOTAGE FOUND IT MISSING. Disabling the `body` check left the suite at
    # 36/36: every absent-body cell above ALSO lacks `commits`, so each was refused by the commits
    # arm and none of them ever exercised the body arm. Two conjuncts, one of them untested, and
    # both cells passing -- which is this repository's oldest lesson about counting conjuncts
    # instead of testing them, in the suite written to catch exactly that class.
    $noBody = '{"commits":[{"messageHeadline":"h","messageBody":"b"}]}' | ConvertFrom-Json
    $absent = Test-PullRequestPayload -Payload $noBody
    Assert-True -Condition ((-not $absent.ok) -and $absent.reason -cmatch 'body') `
        -Message 'an answer with COMMITS but no body field is refused, and the refusal names the body'

    # THE INVARIANT THAT MAKES THIS CHECKABLE. Zero commits is not a pull request with nothing in
    # it; there is no such thing. It is an answer that did not carry what was asked for.
    $emptyCommits = '{"body":"Closes #1","commits":[]}' | ConvertFrom-Json
    $zero = Test-PullRequestPayload -Payload $emptyCommits
    Assert-True -Condition ((-not $zero.ok) -and $zero.reason -cmatch 'ZERO commits') `
        -Message 'zero commits is HARNESS-BROKE and says so, because every pull request has at least one'

    # THE TWO FIELDS #792 ADDED, each refused BY NAME. Both are fetched now -- the title because
    # a squash's subject is the title and a keyword there fires, the base because gh computes the
    # commit list against it. An answer missing either is the wrong shape, exactly as for `body`.
    $noTitle = ('{"body":"Closes #1","commits":[{"messageHeadline":"h","messageBody":"b"}],"baseRefName":"main"}') | ConvertFrom-Json
    $titleGone = Test-PullRequestPayload -Payload $noTitle
    Assert-True -Condition ((-not $titleGone.ok) -and $titleGone.reason -cmatch 'title') `
        -Message 'an answer with no title field is refused, and the refusal names the title'

    $noBase = ('{"body":"Closes #1","commits":[{"messageHeadline":"h","messageBody":"b"}],"title":"t"}') | ConvertFrom-Json
    $baseGone = Test-PullRequestPayload -Payload $noBase
    Assert-True -Condition ((-not $baseGone.ok) -and $baseGone.reason -cmatch 'baseRefName') `
        -Message 'an answer with no baseRefName field is refused, and the refusal names it'

    # NOT the same as an empty body, and the asymmetry is the point. A pull request with no
    # DESCRIPTION is ordinary; a pull request with no BASE does not exist, so an empty one is the
    # instrument answering wrong rather than a fact about the pull request.
    $blankBase = ('{"body":"Closes #1","commits":[{"messageHeadline":"h","messageBody":"b"}],"title":"t","baseRefName":""}') | ConvertFrom-Json
    $baseBlank = Test-PullRequestPayload -Payload $blankBase
    Assert-True -Condition ((-not $baseBlank.ok) -and $baseBlank.reason -cmatch 'EMPTY') `
        -Message 'an EMPTY baseRefName is refused too: every pull request targets a branch'

    # THE CONTROL THAT KEEPS THIS FROM BEING OVER-STRICT, and it is the one that matters most.
    # An EMPTY body is legal -- somebody wrote a pull request without a description -- and closes
    # nothing. An ABSENT body means the answer is the wrong shape. Collapsing the two would trade a
    # false green for a false refusal, and a check that refuses legitimate work gets switched off.
    $emptyBody = ('{"body":"","commits":[{"messageHeadline":"h","messageBody":"Closes #2"}],' + '"title":"fix: an ordinary title","baseRefName":"main"' + '}') | ConvertFrom-Json
    Assert-True -Condition (Test-PullRequestPayload -Payload $emptyBody).ok `
        -Message 'an EMPTY body is legal and is NOT refused: absent and empty are different answers'

    Write-Host ''
    Write-Host '-- the whole fetch-to-text path, including the three ways it refuses --' -ForegroundColor Cyan
    #
    # The cells above test `Test-PullRequestPayload`. They would ALL still pass over a program that
    # never called it -- which is this program's own subject one level out, and the exact hole the
    # contributing lane named: a cell for "no closing keyword in the body" passes identically
    # against a program that cannot reach GitHub at all.
    #
    # So the path from what gh returned to the two texts is one function, and these drive it.

    $failed = Read-PullRequestTexts -Answer ([ordered]@{ exitCode = 1; text = '' })
    Assert-True -Condition ((-not $failed.ok) -and $failed.reason -cmatch 'could not read') `
        -Message 'a non-zero exit refuses, and never falls through to two empty texts'

    $garbage = Read-PullRequestTexts -Answer ([ordered]@{ exitCode = 0; text = 'not json at all' })
    Assert-True -Condition ((-not $garbage.ok) -and $garbage.reason -cmatch 'JSON') `
        -Message 'an answer that is not JSON refuses, naming the parse rather than the content'

    # THE ONE THE CONTRIBUTED FINDING IS ABOUT: gh exits 0, the JSON parses, and it carries nothing.
    $hollow = Read-PullRequestTexts -Answer ([ordered]@{ exitCode = 0; text = '{}' })
    Assert-True -Condition (-not $hollow.ok) `
        -Message 'a successful call that carried NEITHER text refuses -- measured green before this existed'

    $answer = '{"body":"Closes #1","commits":[{"messageHeadline":"fix: a","messageBody":"Closes #2"}],' + '"title":"fix: an ordinary title","baseRefName":"main"' + '}'
    $good = Read-PullRequestTexts -Answer ([ordered]@{ exitCode = 0; text = $answer })
    Assert-True -Condition ($good.ok -and $good.body -ceq 'Closes #1') `
        -Message 'CONTROL: a real answer succeeds and carries the body verbatim'
    Assert-True -Condition ($good.commits -cmatch 'fix: a' -and $good.commits -cmatch 'Closes #2') `
        -Message 'and carries BOTH halves of the commit message, subject as well as body'

    Write-Host ''
    Write-Host '-- the intent, passed the way an operator actually passes it: through -File --' -ForegroundColor Cyan
    #
    # EVERY CELL ABOVE RUNS IN THIS PROCESS, AND THAT IS WHY THEY COULD NOT SEE THIS. The defect
    # lives in PowerShell's construction of a CHILD'S COMMAND LINE, not in the parameter
    # declaration: an empty argument is dropped there, before any binder runs, so
    # `-Closes ''` -- the spelling this program's own documentation prescribed for "closes
    # nothing" -- dies with "Missing an argument for parameter 'Closes'". Dot-invoked, the same
    # declaration binds it fine. Found by the GraphHelm ISSUES lane by USING the program on the
    # merge checklist rather than reading it, and their sharpest point was that a same-process cell
    # PASSES today and would stay green through any remedy, including a wrong one.
    #
    # So this seam carries the program's REAL param block and its REAL token reader, and is
    # launched as a child exactly as an operator launches the program.
    $intentSeam = Join-Path $fixtureRoot 'intent-seam.ps1'
    $paramStart = $programText.IndexOf('[CmdletBinding()]')
    # SINGLE quotes. In double quotes PowerShell interpolates `$ErrorActionPreference` and searches
    # for the variable's VALUE followed by " = 'Stop'", which is in no file -- and the seam then
    # reports HARNESS-BROKE about an anchor that is right there.
    $paramEnd = $programText.IndexOf('$ErrorActionPreference = ''Stop''')
    if ($paramStart -lt 0 -or $paramEnd -le $paramStart) {
        Write-Host 'HARNESS-BROKE: the param block was not found between its anchors' -ForegroundColor Magenta
        exit 2
    }
    [System.IO.File]::WriteAllText($intentSeam,
        $programText.Substring($paramStart, $paramEnd - $paramStart) +
        $programText.Substring($seamStart, $seamEnd - $seamStart) + @'

$stated = ConvertTo-IntendedClosures -Tokens $Closes
Write-Output ('ok=' + $stated.ok + ' numbers=[' + (@($stated.numbers) -join ',') + '] offending=[' + $stated.offending + ']')
'@, (New-Object System.Text.UTF8Encoding($false)))

    $viaFile = {
        param($Arguments)
        $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $intentSeam @Arguments 2>&1 |
                ForEach-Object { [string]$_ })
        return ($out -join "`n")
    }

    # CONTROL FIRST: a real number survives the command line, or every refusal below is a script
    # that cannot be invoked at all.
    $numberViaFile = & $viaFile @('-Number', '1', '-Closes', '744')
    Assert-True -Condition ($numberViaFile -cmatch [regex]::Escape('numbers=[744]')) `
        -Message 'CONTROL: an ordinary -Closes survives -File and reaches the reader'

    # NO CELL FOR `-Closes ''`, AND THE REASON IS THE FINDING.
    #
    # It was reported as "-File drops the empty argument" and I set out to pin it. Measured across
    # four launchers instead:
    #
    #   bash        -> powershell -File ... -Closes ''       WORKS   (count=1 first=[])
    #   cmd.exe     -> powershell -File ... -Closes ""       WORKS
    #   PowerShell  -> & powershell -File ... -Closes ''     FAILS   "Missing an argument"
    #   PowerShell  -> the same call with a SPLATTED array   FAILS identically (corrected 2026-09-08:
    #                                                       measured against the real script, both forms
    #                                                       answer "Missing an argument for parameter
    #                                                       'Closes'... type 'System.String[]'")
    #   this suite  -> both shapes                           WORKS
    #
    # So the trigger is not `-File`. It is PowerShell's construction of a native command line
    # dropping an empty LITERAL, in some invocation contexts and not others -- and this suite's own
    # context is one of the ones where it does not happen. A cell asserting the failure would pass
    # or fail depending on how the SUITE was launched, which is a guard tested only on the author's
    # disk. A cell asserting the success would freeze a launcher-specific accident as a contract.
    #
    # Recorded here rather than guarded, with the measurement attached so the next person can
    # re-take it. What made the whole question moot is `-Closes none`, below: it survives every
    # launcher in that table, which is why it is the documented spelling now and why the empty
    # string is documented as NOT one.

    # AND THE SPELLING THAT DOES. This is the assertion that would redden if anyone removed the
    # sentinel and went back to prescribing the empty string.
    $noneViaFile = & $viaFile @('-Number', '1', '-Closes', 'none')
    Assert-True -Condition ($noneViaFile -cmatch [regex]::Escape('ok=True') -and $noneViaFile -cmatch [regex]::Escape('numbers=[]')) `
        -Message '-Closes none survives -File and reads as CLOSES NOTHING'

    $casedViaFile = & $viaFile @('-Number', '1', '-Closes', 'NONE')
    Assert-True -Condition ($casedViaFile -cmatch [regex]::Escape('numbers=[]')) `
        -Message 'and so does NONE: this token comes from a human at a prompt, so it is the one OrdinalIgnoreCase comparison here'

    # The sentinel must not become a hole: junk is still refused by name.
    $junkViaFile = & $viaFile @('-Number', '1', '-Closes', 'nonesuch')
    Assert-True -Condition ($junkViaFile -cmatch [regex]::Escape('ok=False') -and $junkViaFile -cmatch [regex]::Escape('offending=[nonesuch]')) `
        -Message 'and a token that merely STARTS with the sentinel is still refused, naming itself'

    Write-Host ''
    Write-Host '-- the calibration, against two real merged pull requests --' -ForegroundColor Cyan

    # BOTH of these are quoted from the merged artefacts, not invented, because a calibration built
    # from a text someone made up calibrates against their imagination. Measured 2026-09-04 with
    # `gh pr view <n> --json body,commits,closingIssuesReferences`.

    # #746, the incident. The body named ONE closure and GitHub's own parser reported exactly that
    # one; the COMMITS named two, and the squash closed both. #717 closed two seconds after the
    # merge, from a sentence written to prevent it.
    $body746 = 'Closes #735'
    $commits746 = "docs(717): declare that the two platforms are not equally strong`n`nThis does NOT close #717.`n`nCloses #735"
    Assert-Set -Actual (Get-ClosingReferences -Text $body746) -Expected @('735') `
        -Message 'CALIBRATION #746: the BODY names one closure, which is what closingIssuesReferences reported'
    # Driven through the WHOLE decision, not through an intermediate. The union legitimately carries
    # `735` twice, once from each text, and reducing it is part of what is being tested.
    #
    # This is also the cell that makes the suite non-vacuous as a whole. `Get-ClosingReferences` and
    # `Compare-ClosureIntent` were BOTH individually correct on #746 -- the pattern found `close
    # #717` and the comparison would have flagged it -- and the issue still closed, because the
    # program only ever handed one of the two texts to them. Every cell above this one would pass
    # over a version that read the body alone. Counting conjuncts is not testing them.
    $verdict746 = Get-ClosureVerdict -BodyText $body746 -CommitText $commits746 -TitleText '' -Intended @('735')
    Assert-Set -Actual $verdict746.unexpected -Expected @('717') `
        -Message 'and the whole decision over BOTH texts catches #717 -- the closure that actually fired'
    Assert-Set -Actual $verdict746.body -Expected @('735') `
        -Message 'with the two halves still reported apart, so an operator can see WHICH text carried it'
    Assert-Set -Actual $verdict746.commits -Expected @('717', '735') `
        -Message 'and the commit half is the one holding the surprise'

    # #754, and this one is a CORRECTION to the calibration #757 records. That ticket says the grep
    # flags this line and GitHub does not link it, offering it as evidence the check is tighter than
    # the parser. Measured against the real body: this pattern does NOT flag it, and GitHub links
    # nothing either -- they agree, so the line is not evidence about the direction at all.
    #
    # The direction is still chosen deliberately: a false positive costs one reading of a sentence
    # and a false negative is what closed #717. But it is chosen on the argument, not on this
    # example, and the cell says so rather than letting a retold measurement stand.
    $dash = [char]0x2014
    $line754 = '`Refs #752`, not `Closes` ' + $dash + ' #752 also owns the structural question'
    Assert-Set -Actual (Get-ClosingReferences -Text $line754) -Expected @() `
        -Message "CALIBRATION #754: NOT flagged here, and GitHub links nothing either -- #757's example does not reproduce"

    # THE DIRECTION, PINNED (#873). A closing keyword inside a CODE SPAN is flagged here and is not
    # linked by GitHub -- measured on #865 and #862, whose bodies read `` `Closes #796`. `` and
    # `` `Closes #815` `` and whose closingIssuesReferences were both empty, against #856 and #833
    # as the positive control.
    #
    # This cell exists to be IN THE WAY. The obvious repair is to strip code spans before matching,
    # and it is wrong: it would miss `` `Closes #815` `` -- a body that genuinely intends to close,
    # written by an author who reached for backticks out of habit -- trading a false positive that
    # costs one reading of a sentence for the false negative that closed #717.
    Assert-Set -Actual (Get-ClosingReferences -Text '`Closes #796`.') -Expected @('796') `
        -Message 'the_backtick_direction: a keyword in a CODE SPAN is flagged, though GitHub links nothing (#873)'
    # AND THE BOUNDARY, measured rather than assumed -- I asserted the opposite of this first and the
    # cell caught me. The pattern is `\b(keyword)\b\s*#(\d+)`: only WHITESPACE may sit between the
    # keyword and the hash. So a span AROUND the phrase is flagged (above) and a backtick BETWEEN
    # its halves is not.
    #
    # This is a discrimination cell, not a wish. It pins where the tighter-than-GitHub direction
    # actually stops, so the next reader does not infer from the cell above that any backtick
    # anywhere is caught -- and so that widening `\s*` to `[^#]*` reddens something.
    Assert-Set -Actual (Get-ClosingReferences -Text 'Fixed `#4` by hand') -Expected @() `
        -Message 'but a backtick BETWEEN the keyword and the number breaks the match: only whitespace may sit there'

    Write-Host ''
    Write-Host "-- GitHub's own reading, shown and never consulted --" -ForegroundColor Cyan

    # THE THREE STATES, AND THE ONE THAT MATTERS IS THE THIRD. `(unread)` is not `(none)`: rendering
    # a field that never arrived as "GitHub linked nothing" manufactures the exact disagreement the
    # line was added to surface, and it would do so on every machine with an older `gh`.
    $linkedSome = Get-LinkedReading -Payload ([pscustomobject]@{ closingIssuesReferences = @([pscustomobject]@{ number = 796 }) })
    Assert-True -Condition ($linkedSome.read -and (@($linkedSome.numbers) -join ',') -ceq '796') `
        -Message 'a linked issue is read as read, with its number'

    $linkedEmpty = Get-LinkedReading -Payload ([pscustomobject]@{ closingIssuesReferences = @() })
    Assert-True -Condition ($linkedEmpty.read -and @($linkedEmpty.numbers).Count -eq 0) `
        -Message 'an EMPTY list is READ and empty -- GitHub linking nothing is a finding, not an absence'

    # gh returns null, not [], for some pull requests. That is still an answer.
    $linkedNull = Get-LinkedReading -Payload ([pscustomobject]@{ closingIssuesReferences = $null })
    Assert-True -Condition ($linkedNull.read -and @($linkedNull.numbers).Count -eq 0) `
        -Message 'a NULL field is read as read-and-empty, not as unread'

    $linkedAbsent = Get-LinkedReading -Payload ([pscustomobject]@{ body = 'Closes #796' })
    Assert-True -Condition ((-not $linkedAbsent.read) -and @($linkedAbsent.numbers).Count -eq 0) `
        -Message 'a field that never arrived is UNREAD -- the fallback fetch drops it, and an old gh never had it'

    Assert-True -Condition ((-not (Get-LinkedReading -Payload $null).read)) `
        -Message 'and a null payload is unread rather than empty'

    # The renderer, separately, because a correct tri-state printed through two identical strings
    # is a tri-state nobody can see.
    Assert-True -Condition ((Format-LinkedReading -Reading $linkedSome) -ceq '#796') `
        -Message 'the renderer prints the number with its hash'
    Assert-True -Condition ((Format-LinkedReading -Reading $linkedEmpty) -ceq '(none)') `
        -Message 'an empty reading prints (none)'
    Assert-True -Condition ((Format-LinkedReading -Reading $linkedAbsent) -ceq '(unread)') `
        -Message 'an unread reading prints (unread)'
    # THE DISCRIMINATION. Both of the above could return one string and every cell but this passes.
    Assert-True -Condition ((Format-LinkedReading -Reading $linkedEmpty) -cne (Format-LinkedReading -Reading $linkedAbsent)) `
        -Message '(none) and (unread) are DIFFERENT text, so a failed read can never read as a finding'

    Write-Host ''
    Write-Host "-- and GitHub's reading stays OUT of the decision (#873) --" -ForegroundColor Cyan

    # TRUE IS NOT THE SAME AS KEPT TRUE. The program's central claim -- that `linked` is printed and
    # never consulted -- holds structurally today: `Get-ClosureVerdict` has no parameter for it, so
    # the decision function cannot see it. That is stronger than any assertion about behaviour, and
    # it is also one line away from being false, with nothing red. These pin it.
    #
    # The parameter SET, not the absence of a name. A cell asserting `-notcontains 'Linked'` passes
    # over a fifth parameter called `ApiReading`, which is the same defect wearing a hat.
    $verdictParameters = @((Get-Command Get-ClosureVerdict).Parameters.Keys |
        Where-Object { $_ -notin [System.Management.Automation.PSCmdlet]::CommonParameters })
    $expectedParameters = @('BodyText', 'CommitText', 'TitleText', 'Intended')
    $extraParameters = @($verdictParameters | Where-Object { $expectedParameters -notcontains $_ })
    Assert-True -Condition ($extraParameters.Count -eq 0) `
        -Message "the decision function takes the three texts and the intent, and nothing else (extra: $($extraParameters -join ', '))"
    $absentParameters = @($expectedParameters | Where-Object { $verdictParameters -notcontains $_ })
    Assert-True -Condition ($absentParameters.Count -eq 0) `
        -Message "CONTROL: and it really does take all four, so the cell above is not passing over a renamed function (absent: $($absentParameters -join ', '))"

    # THE OTHER HALF. The reader could stay out of the decision function and still reach a decision
    # through `$read.linked` in the script body -- an `if` on it, a value folded into the exit code.
    # Scanned over the program text BELOW the seam, which is the part the cells above cannot reach.
    $bodyStart = $programText.IndexOf('function Invoke-Gh {')
    $scriptBody = $programText.Substring($bodyStart)
    $linkedUses = @($scriptBody -split "`r?`n" | Where-Object {
        $_ -match 'Format-LinkedReading' -or $_ -match '\$read\.linked'
    })
    # Vacuity first: a scan that found nothing would satisfy the rule below by measuring nothing.
    Assert-True -Condition ($linkedUses.Count -ge 1) `
        -Message "the scan found the reading being used at all in the script body (found $($linkedUses.Count))"
    $decidingUses = @($linkedUses | Where-Object { $_.TrimStart() -notmatch '^Write-Host' })
    Assert-True -Condition ($decidingUses.Count -eq 0) `
        -Message "every use of GitHub's reading below the seam is a Write-Host: printed, never consulted"

    Write-Host ''
    Write-Host '-- the union against the stated intent --' -ForegroundColor Cyan

    # #746 exactly: the body named one closure, the commits named two, and the button was pressed on
    # a reading of the body alone.
    $seen = Compare-ClosureIntent -Union @('735', '717', '735') -Intended @('735')
    Assert-Set -Actual $seen.unexpected -Expected @('717') `
        -Message '#746 reproduced: a closure the commits carry and the intent does not is UNEXPECTED'
    Assert-Set -Actual $seen.missing -Expected @() `
        -Message 'and nothing is reported missing, because the intended closure is there'

    $agreed = Compare-ClosureIntent -Union @('735') -Intended @('735')
    Assert-True -Condition ($agreed.unexpected.Count -eq 0 -and $agreed.missing.Count -eq 0) `
        -Message 'agreement is silent in both directions, so the check can actually pass'

    # Not harmless, and this is why it is its own verdict rather than folded into the one above: an
    # intended closure that appears in NEITHER text depends on whatever the squash happens to carry,
    # and the operator finds out afterwards.
    $absent = Compare-ClosureIntent -Union @() -Intended @('735')
    Assert-Set -Actual $absent.missing -Expected @('735') `
        -Message 'an intended closure carried by neither text is MISSING, not merely quiet'

    $written = Compare-ClosureIntent -Union @('735') -Intended @('#735')
    Assert-True -Condition ($written.unexpected.Count -eq 0 -and $written.missing.Count -eq 0) `
        -Message 'the operator may write 735 or #735: the hash is not a second spelling to get wrong'

    # Numeric, not lexical. `#9` sorting after `#10` in a list a human compares by eye is how a
    # correct refusal gets read as a wrong one.
    $ordered = Compare-ClosureIntent -Union @('10', '9', '735') -Intended @()
    Assert-True -Condition ((@($ordered.unexpected) -join ',') -ceq '9,10,735') `
        -Message "the report sorts numerically (got [$(@($ordered.unexpected) -join ',')])"
    Write-Host ''
    Write-Host '-- the BASE BRANCH, which scopes the commit list (#792) --' -ForegroundColor Cyan

    # gh computes `--json commits` against the BASE. A pull request stacked on anything other than
    # the merge target hides every commit below that base, so the program refuses rather than
    # reporting a green it cannot back. Measured on #779: 5 commits from the old base, 8 after the
    # retarget, and `Closes #708` sat in the sixth.
    $onTarget = Test-MergeTargetBase -Base 'main' -MergeTarget 'main'
    Assert-True -Condition $onTarget.ok `
        -Message 'a pull request based on the merge target is gateable'

    $stacked = Test-MergeTargetBase -Base 'issue-708-reader-silence-grace' -MergeTarget 'main'
    Assert-True -Condition (-not $stacked.ok) `
        -Message 'a pull request based on ANOTHER BRANCH is refused, because the commit list stops there'
    # The reason must name BOTH sides. A refusal that says only "wrong base" sends the operator to
    # look up what it should have been, and this program is the thing that knows.
    Assert-True -Condition ($stacked.reason -like "*issue-708-reader-silence-grace*" -and $stacked.reason -like "*main*") `
        -Message 'and the refusal names the base it FOUND and the one it WANTED'

    # ORDINAL, both cells. Git branch names are case-sensitive, so `Main` is not `main` and a
    # comparison that says otherwise would gate a branch that does not exist.
    $cased = Test-MergeTargetBase -Base 'Main' -MergeTarget 'main'
    Assert-True -Condition (-not $cased.ok) `
        -Message 'a branch differing only in CASE is refused: git branch names are case-sensitive'

    # THE CELL THAT REDDENS IF `Ordinal` IS EVER DOWNGRADED TO `-eq`. U+FE00 is a variation
    # selector: it has ZERO WEIGHT under culture-aware comparison, so PowerShell's `-eq` -- and
    # `-ceq` too, since case-sensitivity and culture-awareness are ORTHOGONAL -- reports this name
    # EQUAL to 'main'. The guard would then fail OPEN on a branch that is not main, which is
    # precisely the direction an attacker picks and the defect #753 is about.
    $zeroWeight = Test-MergeTargetBase -Base "main$([char]0xFE00)" -MergeTarget 'main'
    Assert-True -Condition (-not $zeroWeight.ok) `
        -Message 'a ZERO-WEIGHT character does not make a branch name equal to main (ordinal, not -eq)'

    Write-Host ''
    Write-Host "-- the TITLE is the squash's SUBJECT line, and it decides (#792) --" -ForegroundColor Cyan

    # A squash's default subject is the pull request TITLE plus " (#N)" -- measured on this
    # repository, where #767, #770 and #613 all landed with a subject exactly equal to their title.
    # So a closing keyword in a title FIRES, and until #792 this program read neither it nor
    # anything that would have carried it. Nothing in 200 pull requests here has ever had one, so
    # this closes a gap rather than a wound; "no instance yet" is not a property of the instrument.
    $titleOnly = Get-ClosureVerdict -BodyText 'Refs #700' -CommitText 'chore: tidy' -TitleText 'fix: closes #701' -Intended @()
    Assert-Set -Actual $titleOnly.unexpected -Expected @('701') `
        -Message 'a closure carried ONLY by the title is caught: the squash subject is a deciding text'
    Assert-Set -Actual $titleOnly.title -Expected @('701') `
        -Message 'and the title half is reported apart, so the operator can see WHICH text carried it'

    # The common case must not have moved. Every pull request on the board has a title, almost none
    # of them carry a keyword, and a check that flagged those would be switched off.
    $noTitle = Get-ClosureVerdict -BodyText 'Closes #735' -CommitText 'fix: work`n`nCloses #735' -TitleText 'fix(735): an ordinary title with no keyword next to a number' -Intended @('735')
    Assert-True -Condition ($noTitle.unexpected.Count -eq 0 -and $noTitle.missing.Count -eq 0 -and $noTitle.title.Count -eq 0) `
        -Message 'an ordinary title contributes nothing, so the usual pull request still passes'

    Write-Host ''
    Write-Host '-- the refusal can be quoted where it is discussed (#965) --' -ForegroundColor Cyan

    # THE SHIPPED SENTENCE, not a copy: `Get-UnexpectedClosureRefusal` comes from the seam above.
    # AGENTS.md mandates running this tool before a press, so the line that explains a refusal is
    # the thing most likely to be pasted into a pull request body or a review comment -- and until
    # #965 that paste re-triggered the tool on the numbers it was quoting. Measured on #873: run 2
    # refused on a number that by then existed ONLY inside run 1's quoted refusal.
    $refusal = Get-UnexpectedClosureRefusal -Numbers @('815', '925')
    Assert-Set -Actual (Get-ClosingReferences -Text $refusal) -Expected @() `
        -Message 'the tool own refusal, fed back through the parser, names nothing: it can be pasted where it is discussed'

    # AND IT STILL SAYS WHICH. Without this, deleting the numbers would satisfy the cell above --
    # and eliding them is exactly the workaround #965 was filed to remove.
    Assert-True -Condition ($refusal.Contains('#815') -and $refusal.Contains('#925')) `
        -Message 'and it still names every offending number, in a form a human reads without decoding'

    # THE CONTROL. Same sentence, the noun taken out, which is what it said before #965. If this
    # did not name both numbers, the cell above would be measuring a broken parser rather than the
    # adjacency the fix is about.
    $adjacent = $refusal.Replace('close issues #', 'close #')
    Assert-Set -Actual (Get-ClosingReferences -Text $adjacent) -Expected @('815') `
        -Message 'CONTROL: with the noun removed the first number fires again, so the cell above is about the ADJACENCY'

    # One number reads as one. A message that said "issues #815" would be inert too, and would look
    # like a program that cannot count.
    Assert-True -Condition ((Get-UnexpectedClosureRefusal -Numbers @('815')).Contains('close issue #815')) `
        -Message 'a single offender is named in the singular'

} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount." -ForegroundColor Magenta
    exit 2
}

$passed = $script:total - $script:failures
$color = if ($script:failures -eq 0) { 'Green' } else { 'Red' }
Write-Host "$passed/$script:total passed" -ForegroundColor $color
if ($script:failures -gt 0) { exit 1 }
exit 0
