# #867: the liveness rule says ONE thing, and a census reads THREE surfaces.
#
# WHY THIS SUITE EXISTS. `.factory/MERGE-CHECKLIST.md` carried the corrected liveness rule at item 2
# AND, seventy-six lines later, the rule it replaced -- a CPU delta on the compiling descendant read
# thirty seconds apart, with equality taken to mean wedged. `AGENTS.md` carried the same stale copy.
# Both were true of the same document at the same time, and the action a `wedged` verdict invites is
# to KILL the gate: on the main gate that is the run every `merge-proof` waits for. A document that
# answers differently depending on where it is opened is not half-fixed.
#
# EVERY ABSENCE HERE HAS A PRESENCE BESIDE IT, from the same read. `Assert-True (-not $text.Contains(
# 'x'))` is satisfied by a file that is missing, renamed, empty, or read from the wrong root -- the
# reassuring zero this repository keeps re-learning. So each "the dangerous spelling is gone" is
# paired with "the corrected rule is here", and a HARNESS-BROKE exit covers the file being absent at
# all.
#
# THE SPELLINGS ARE THE OPERATIVE ONES, not the incidental ones. `wedged = equal` and
# `progressing = greater` are the two false CLAIMS; a threshold written `30 s apart` is the third.
# Prose that explains the removal deliberately spells them out in words instead, so this guard does
# not have to carry an exemption list for the text that documents it.

$ExpectedAssertionCount = 66
$ErrorActionPreference = 'Stop'
$script:total = 0
$script:failures = 0

function Assert-True {
    param([Parameter(Mandatory)] [bool] $Condition, [Parameter(Mandatory)] [string] $Message)
    $script:total++
    if ($Condition) { Write-Host "  PASS: $Message" -ForegroundColor Green }
    else { $script:failures++; Write-Host "  FAIL: $Message" -ForegroundColor Red }
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$checklistPath = Join-Path $repositoryRoot '.factory/MERGE-CHECKLIST.md'
$agentsPath = Join-Path $repositoryRoot 'AGENTS.md'

# This suite's own file is in the C0 sweep's population below, so it is READ like the other two
# rather than assumed.
$guardPath = Join-Path $repositoryRoot 'ci/liveness-rule.tests.ps1'

foreach ($required in @($checklistPath, $agentsPath, $guardPath)) {
    if (-not (Test-Path -LiteralPath $required)) {
        Write-Host "HARNESS-BROKE: $required is not where this suite expects it" -ForegroundColor Magenta
        exit 2
    }
}

$checklist = [System.IO.File]::ReadAllText($checklistPath)
$agents = [System.IO.File]::ReadAllText($agentsPath)
$guard = [System.IO.File]::ReadAllText($guardPath)

# The floor is the other half of the harness check: a file that exists and reads as a few bytes
# would satisfy every absence below.
Assert-True (($checklist.Length -gt 20000) -and ($agents.Length -gt 5000)) `
    "both documents were read and are of a plausible size (checklist $($checklist.Length), agents $($agents.Length))"

Write-Host '#867: one liveness rule'

foreach ($pair in @(
    @{ Name = 'MERGE-CHECKLIST.md'; Text = $checklist },
    @{ Name = 'AGENTS.md'; Text = $agents })) {

    Assert-True (-not $pair.Text.Contains('wedged = equal')) `
        "$($pair.Name) does not claim CPU equality means wedged -- children die and leave the sum (70.1 s -> 6.1 s in 15 s, measured on a HEALTHY gate)"

    Assert-True (-not $pair.Text.Contains('progressing = greater')) `
        "$($pair.Name) does not claim a greater CPU sum means progress -- the sum DROPS in a healthy gate, so the test is false in both directions"

    # THE CONTROL for the two absences above: if the corrected rule is missing, the file is not the
    # one this suite is about and the zeros mean nothing.
    Assert-True ($pair.Text.Contains('Wedged is never a 30 s reading')) `
        "$($pair.Name) carries the corrected rule, so the absences above are about this document rather than about a file that was never read"
}

Write-Host '#867: a census reads three surfaces'

# NOT a bare `Contains('pulls/N/comments')`. Measured: that string appears THREE times in the
# checklist, one of them inside the prose recounting the #1004 census -- so deleting the surface
# LIST and the command a lane actually runs left this assertion green. My own sabotage caught it.
# Both operative mentions are asserted instead: the surface list and the runnable command.
Assert-True ($checklist.Contains('`repos/O/R/pulls/N/comments` (the inline')) `
    'the checklist LISTS the inline review-thread surface as one of the three a census reads'

Assert-True ($checklist.Contains('repos/stabem/GraphHelm/pulls/N/comments --jq')) `
    'and gives the command that reads it, beside the other two -- where all three findings on #1004 lived while two lanes read the other two surfaces'

# TWO PRESCRIPTIONS, ONE DOCUMENT. The eligibility census -- the one that decides who may press --
# says `Read BOTH surfaces` while ITEM 6 says three. Both are correct for their own question, and
# nothing said so: a reader meeting BOTH in one item and three in the next has to guess which is
# stale, and the flattering guess is the smaller population. Pinned because the sentence is the
# whole fix -- deleting it leaves two live rules that contradict on their face.
Assert-True ($checklist.Contains('Read BOTH surfaces')) `
    'CONTROL: the eligibility census still says BOTH, so the absence below is about a contradiction that exists'

Assert-True ($checklist.Contains('two here, three in ITEM 6, and the difference is the QUESTION')) `
    'and the document reconciles the two counts instead of leaving a reader to pick one'

Assert-True ($checklist.Contains('has not cast one')) `
    'and it says WHERE a pass lives: a verdict stated only inside an inline thread is not a pass, which is what makes two surfaces right there and three right in item 6'

# The rule that the three-surface line alone does NOT carry: reading all three and publishing
# minutes later is a different defect wearing the same shape. Pinned separately because it was
# learned separately -- three minutes after the surfaces rule was written, on the PR that taught it.
# The third thing item 6 needs and did not have: the FILTER checked before the population.
# A single-backslash `\b` inside a jq string is byte 0x08, so `test("Lane: M\b")` reports zero on every thread ever --
# and it was published as the evidence of eligibility on #1002. Pinned as its own assertion because
# it is a different failure from the surfaces rule and from the same-breath rule: those are about
# WHERE and WHEN you look, this is about the instrument answering at all.
Assert-True ($checklist.Contains('puts a BACKSPACE in the')) `
    'the checklist warns that a jq census filter written with a word-boundary escape reports zero on every input'

Assert-True ($checklist.Contains('not evidence until the SAME command has')) `
    'and states the general form: a filter earns belief only after returning non-zero on a known positive'

# The same class WITHOUT a regex: an empty result and a default that answers in its place are
# indistinguishable. Pinned beside the jq line because a reader who fixes their regex and keeps
# `${VAR:-999999}` has fixed nothing.
Assert-True ($checklist.Contains('a default that')) `
    'and names the version with no regex in it: an empty result plus a shell default reads as an answer'


Assert-True ($checklist.Contains('SAME breath as the comment that publishes it')) `
    'the checklist says the census is taken in the same breath as the comment that publishes it -- a census is a measurement and it decays'

Assert-True ($checklist.Contains('gh pr comment "$N" --body-file "$bodyFile"')) `
    'the publication step uses gh pr comment with a concrete body file'
Assert-True ($checklist.ToLowerInvariant().Contains('immediately re-read the head and all three surfaces')) `
    'the publication rule requires an immediate post-comment re-read of the head and all three surfaces'
Assert-True ($checklist.Contains('returned comment ID')) `
    'the post-read rule identifies the one newly published census comment by the returned ID'
Assert-True ($checklist.Contains('withdraw the verdict')) `
    'a changed head or body withdraws the verdict before a new read and repost'
Assert-True ($checklist.Contains('no atomic GitHub transaction')) `
    'the rule states the optimistic publication limitation explicitly'
Assert-True ($checklist.Contains('read failure means NO AUTHORITY')) `
    'a failed post-publication read cannot produce merge authority'
Assert-True ($checklist.Contains('trap withdraw_on_exit EXIT') -and $checklist.Contains('$publicationFailure = $_')) `
    'both examples handle every post-publication failure, including failed reads and decoding'
Assert-True ($checklist.Contains('gh api --method PATCH --input') -and $checklist.Contains('WITHDRAWN: NO AUTHORITY') -and $checklist.Contains('withdrawal-readback.json') -and $checklist.Contains('$readback.body -cne $withdrawalBody')) `
    'withdrawal replaces the posted body and verifies the public replacement in both examples'
Assert-True ($checklist.Contains('MANUAL CLEANUP REQUIRED') -and $checklist.Contains('Network failure and process termination cannot guarantee withdrawal')) `
    'failed cleanup and unknown publication IDs remain explicit failures, never merge authority'
Assert-True ($checklist.Contains('Stop and manually classify that saved BEFORE snapshot')) `
    'the verdict is based on the saved initial snapshot rather than an earlier census'
Assert-True ($checklist.Contains('read -r -p "Type REVIEWED $before_head') -and $checklist.Contains('$confirmation = Read-Host "Type REVIEWED $beforeHead')) `
    'both publication examples stop for explicit operator review before posting'

# THE CONTRADICTION THIS PR ALMOST SHIPPED (Codex P1 on #1010): the paragraph says the bot filter
# belongs to the pass COUNT and never to the READ, and the documented read command carried
# `select(.user.login != "chatgpt-codex-connector[bot]")` five lines below it. A rule contradicted
# by its own code block is the same defect this branch removes from the liveness rule, committed
# by the change that removes it.
#
# ASSERTED ON THE COMMAND LINE, NOT ON THE FILE. The prose deliberately QUOTES the filter to say
# what was removed, so a file-wide absence check would fail on the sentence explaining the fix.
$readLine = @($checklist -split "`r?`n" | Where-Object { $_ -like '*issues/N/comments --jq*' })
Assert-True (@($readLine).Count -ge 1) `
    'the documented census read command is present (the control: an absence below must be about a line that exists)'

Assert-True (@($readLine | Where-Object { $_ -like '*chatgpt-codex-connector*' }).Count -eq 0) `
    'and it does NOT filter the bot out of the READ -- excluding a bot from the pass count is right, excluding it from what you look at is how three P1s went unread'

Assert-True ($checklist.Contains('is NOT an empty')) `
    'and it says a Codex review with an empty body in the reviews box is not an empty review -- the header is there, the findings are in the third surface'

Write-Host "#1010: the document itself, not only what it says"

# THE FOURTH prose-vs-command divergence on this branch, and the one that made me count them: the
# paragraph says "print each body's author ... and decide by reading" while the commands under it
# printed `.body[0:80]` -- a byte slice, which is neither a body nor a decision. THE FIRST REPAIR
# WAS THE SAME DEFECT IN A NEW SPELLING: `split("\n")[0]` prints the first line, and item 8
# documents bodies whose identity line is NOT their first line, so the census could still miss a
# verdict and qualify a reviewing lane to press. The guard here pinned that truncation as success,
# which is how a test certifies the bug it was written to prevent. Codex raised it on two heads and
# was right both times (P1 x2 on #1010).
#
# ASSERTED ON THE COMMAND LINES, NOT THE FILE: the prose quotes the old spellings to explain them,
# so a file-wide absence check would fail on the sentence describing the fix.
$censusLines = @($checklist -split "`r?`n" |
    Where-Object { $_.Contains('gh api') -and $_.Contains('--jq') -and $_.Contains('repos/stabem/GraphHelm/') })

Assert-True (@($censusLines).Count -eq 3) `
    'the census prescribes exactly three commands, one per surface (the control: every absence below is about lines that exist)'

Assert-True (-not $checklist.Contains('.body[0:')) `
    'no census command prints a byte slice of a body'

Assert-True (@($censusLines | Where-Object { $_.Contains('split("\n")[0]') }).Count -eq 0) `
    'and none truncates at the first newline either -- a first-line print hides exactly the verdict a matcher would have hidden, because item 8 documents bodies whose identity line is not the first line'

Assert-True (@($censusLines | Where-Object { $_.Contains('\(.body)') }).Count -eq 3) `
    'all three emit the COMPLETE body, which is what makes "classify by reading" something a reader can actually do'

Assert-True (@($censusLines | Where-Object { $_.Contains('--paginate') }).Count -eq 3) `
    'and all three paginate -- the reviews command did not, so a PR with more reviews than one page could hide an older verdict on the one surface that carries them'

Assert-True (@($censusLines | Where-Object { $_.Contains('.user.login') }).Count -eq 3) `
    'and all three name the author, including the reviews command, which printed only state and body'

# THE SHELL. The three commands above are bash, and a jq expression carrying a quoted `\n` in
# its source does not survive Windows PowerShell 5.1 -- it reaches gh split in three and answers
# `accepts 1 arg(s), received 3`, exit 1. Measured, not inferred. This document already records
# that hazard for the eligibility census and gives both forms there; item 6 prescribed only the
# one that breaks, on the shell the gate itself runs (Codex P1 on #1010). A census nobody on that
# shell can run is a census nobody takes.
Assert-True ($checklist.Contains('accepts 1 arg(s), received 3')) `
    'the census says WHY a second form exists, with the error the first one gives, rather than offering two shapes and no reason'

Assert-True ($checklist.Contains('--slurp "repos/stabem/GraphHelm/$($surface.Route)"')) `
    'and carries a PowerShell form that keeps every jq string out of the argument list -- gh returns JSON, the shell does the formatting'

# The two flattens are the part that looks redundant and is not: --slurp returns one array per
# page, so a single ForEach unwraps the pages and leaves arrays, and the count reads 1 on any PR.
#
# ASSERTED BY POSITION, and the first version was not. `Contains('ForEach-Object { $_ } |
# ForEach-Object { $_ }')` matches THREE places in this document -- the eligibility census
# carries the same idiom -- so deleting a flatten from the block below would have left the
# guard green on somebody else's line. My own sabotage found it, by refusing to apply against
# a non-unique anchor. The line AFTER the --slurp call is the one this cell is about.
$censusPsLines = @($checklist -split "`r?`n")
$slurpIndex = @(0..($censusPsLines.Count - 1) | Where-Object {
        $censusPsLines[$_].Contains('--slurp "repos/stabem/GraphHelm/$($surface.Route)"')
    })
Assert-True (@($slurpIndex).Count -eq 1) `
    'CONTROL: the PowerShell census form occurs exactly once, so the line-after check below is about one known place'

# THE EXIT CODE IS READ ON THE NEXT STATEMENT, and nothing may come between. A failed `gh api`
# prints nothing, so ConvertFrom-Json gets no rows, the surface reports 0 bodies, and the next
# iteration's success overwrites $LASTEXITCODE -- one of the three mandatory surfaces skipped
# while the census looks finished. Asserted by POSITION because that is the actual rule: a
# check five lines down is a different check (Codex P1 on #1010).
Assert-True ($censusPsLines[$slurpIndex[0] + 1].Trim().StartsWith('if ($LASTEXITCODE -ne 0)')) `
    'the line immediately after the gh call reads its exit code, with no pipeline and no statement in between'

Assert-True ($censusPsLines[$slurpIndex[0] + 1].Contains('INCOMPLETE')) `
    'and it refuses loudly rather than warning -- a census missing one of three surfaces is not a census with a note'

Assert-True ($censusPsLines[$slurpIndex[0] + 2].Trim().Contains('ForEach-Object { $_ } | ForEach-Object { $_ }')) `
    'and only then does it parse, keeping BOTH flattens -- one unwraps the pages and leaves the arrays, which reads as a count of 1 on any PR'

# THE SED CLAUSE WAS BACKWARDS. The paragraph named jq as the outlier and then listed `sed`
# beside it, which is false: GNU sed reads the escape as a word boundary, like PCRE and
# Python. Measured with the program in a FILE, because writing it on a shell command line is
# how the backslash gets eaten before sed ever sees it -- `s/M\b/HIT/` rewrites `Lane: M x`,
# which a backspace reading cannot do (Codex P2 on #1010).
Assert-True ($checklist.Contains('TWO LAYERS DECIDE THIS')) `
    'the escape paragraph names the LAYER as well as the tool -- it was wrong in both directions before, first calling sed a backspace tool and then calling jq the exception'

Assert-True ($checklist.Contains('in GNU sed,') -and $checklist.Contains('in a program FILE')) `
    'and it says sed reads a word boundary, with the program in a FILE, which is the only way the backslash reaches sed at all'

Assert-True ($checklist.Contains('equally in an ordinary Python string')) `
    'and it says Python behaves like jq at the literal layer, so a reader taking the natural spelling to Python meets the same trap'

# A CROSS-REFERENCE BY NUMBER IS A CLAIM, and this document has twelve numbered items and no
# guard that any of them is where a sentence says it is. The reconciliation above was written
# INSIDE item 12 and pointed at item 12 for the three-surface census, which is item 6 -- a
# self-reference that defeated the disambiguation it was added to make, and a presser following
# it lands on the wrong surface set (Codex P2 on #1010). Renumbering must break the build rather
# than turn every reference into a quiet lie, so the two items the prose names are resolved.
$numbered = @($checklist -split "`r?`n")
$itemSix = @($numbered | Where-Object { $_.StartsWith('6. **') })
$itemTwelve = @($numbered | Where-Object { $_.StartsWith('12. **') })
Assert-True ((@($itemSix).Count -eq 1) -and (@($itemTwelve).Count -eq 1)) `
    "CONTROL: items 6 and 12 each open exactly once (got $(@($itemSix).Count) and $(@($itemTwelve).Count)), so the checks below are about lines that exist"

Assert-True ($itemSix[0].Contains('Read the CARRY BODIES')) `
    "item 6 is the carry-bodies census, which is what the reconciliation sentence sends a reader to for the third surface (got: $($itemSix[0]))"

Assert-True ($itemTwelve[0].Contains('An empty third-lane set')) `
    "item 12 is the eligibility census, the one that reads BOTH surfaces and decides who may press (got: $($itemTwelve[0]))"

# AND ITEM 1 AGREES WITH ITEM 6, which is the assertion this suite most needed and did not have.
# Item 1 is the instruction a presser reads AT the irreversible act -- the most expensive line in
# the file -- and it said `re-read BOTH boxes` while item 6 prescribed three, and named the inline
# thread surface nowhere in its own text. Reconciling items 6 and 12 and leaving item 1 saying two
# fixed the explanation and left the instruction (D's BLOCK on #1010). A reader who follows item 1
# and never reaches item 6 misses the surface all three #1004 findings lived on.
$itemOneIndex = @(0..($numbered.Count - 1) | Where-Object { $numbered[$_].StartsWith('1. **') })
$itemSixIndex = @(0..($numbered.Count - 1) | Where-Object { $numbered[$_].StartsWith('6. **') })
Assert-True -Condition ((@($itemOneIndex).Count -eq 1) -and (@($itemSixIndex).Count -eq 1)) `
    "CONTROL: items 1 and 6 each open exactly once (got $(@($itemOneIndex).Count) and $(@($itemSixIndex).Count))"

$itemOneText = ($numbered[$itemOneIndex[0]..($itemSixIndex[0] - 1)] -join "`n")
# ALL THREE, not the one this branch added. Asserting only `pulls/N/comments` leaves the guard
# green when item 1 loses `pulls/N/reviews` -- the surface set is the property, and pinning the
# newest member of a set is how a guard ends up defending the last edit instead of the rule
# (J on #1010).
foreach ($surface in @('issues/N/comments', 'pulls/N/reviews', 'pulls/N/comments')) {
    Assert-True -Condition ($itemOneText.Contains($surface)) `
        "item 1 names $surface itself, so a presser reading only the instruction at the button reads every surface -- it said BOTH boxes while item 6 said three"
}

Assert-True -Condition ($itemOneText.Contains('pulls/N/comments')) `
    'item 1 names the inline review-thread surface itself, so a presser reading only the instruction at the button reads all three -- it said BOTH boxes while item 6 said three'

Assert-True -Condition (-not $itemOneText.Contains('BOTH boxes')) `
    'and the two-surface phrasing is gone from it, rather than left beside the correction for a reader to choose between'

# MIXED LINE ENDINGS, which nothing in this repository looks at for a `.md`. A patch written
# with a bare newline leaves ONE LF line in a CRLF document, invisible in every render and
# every diff view, and the next tool that splits on one convention silently reads two lines as
# one or one as two. It happened while writing the fix above. Same class as the C0 sweep
# beside it: a byte nobody looks at, in a file every lane reads.
#
# UNIFORMITY, NOT CRLF, and the first version got this wrong in the exact way this branch
# spent the day fixing: `$allNewlines -eq $crlf` asserts that every ending is CRLF, which is a
# colour that depends on WHERE THE BYTES CAME FROM. `.gitattributes` declares seven extensions
# and several paths and covers neither `.factory/*.md` nor `AGENTS.md`, so what lands on disk
# is decided by the cloner's `core.autocrlf` -- true on this machine, absent on a clean Linux
# checkout. D ran the committed bytes (LF) and got `638 newlines, 0 of them CRLF`: the
# authoritative gate RED over two perfectly consistent files, with a message naming the wrong
# cause. Either convention is fine; a document carrying BOTH is not.
#
# Declaring the two paths in `.gitattributes` would also work and is the more explicit option,
# but it re-normalises those files in every lane's next checkout, which is a change to other
# people's benches from inside a documentation PR. Named here rather than taken.
foreach ($eolPair in @(
    @{ Name = 'MERGE-CHECKLIST.md'; Text = $checklist },
    @{ Name = 'AGENTS.md'; Text = $agents })) {
    $allNewlines = @([regex]::Matches($eolPair.Text, "`n")).Count
    $crlf = @([regex]::Matches($eolPair.Text, "`r`n")).Count
    Assert-True ($allNewlines -gt 100) `
        "CONTROL: $($eolPair.Name) has line endings to count at all ($allNewlines)"
    Assert-True (($crlf -eq 0) -or ($crlf -eq $allNewlines)) `
        "$($eolPair.Name) has UNIFORM line endings ($allNewlines newlines, $crlf of them CRLF) -- either convention is fine, a document carrying both is not, and a bare LF in a CRLF document is invisible to every render and every diff"

    # THE BYTE BOTH GUARDS EXCUSED. A lone CR -- one not followed by LF -- passes the cell above,
    # which counts newlines and CRLF pairs and never sees a CR that begins no pair, AND passes the
    # C0 sweep below, which exempts 9, 10 and 13 as line endings. Byte 13 was excused by the
    # control-byte guard for being a line ending and ignored by the line-ending guard for not
    # being one: two guards, each assuming the other covered it. J shipped exactly this byte into
    # a script an hour before finding it here (J on #1010). The clause that closes it is not a
    # third guard but a narrower rule: a CR is legitimate ONLY immediately before an LF.
    $carriageReturns = @([regex]::Matches($eolPair.Text, "`r")).Count
    Assert-True ($carriageReturns -eq $crlf) `
        "$($eolPair.Name) carries no LONE carriage return ($carriageReturns CR, $crlf of them before an LF) -- a CR that begins no pair is invisible to the uniformity check above and exempt from the control-byte sweep below"
}

# THE EXAMPLE MUST SHOW THE TRAP, not the spelling that works. The first version wrote the sentence
# about byte 0x08 being a backspace and then demonstrated `test("Lane: M\\b")` -- the
# DOUBLE-backslash form, which is the one that behaves as a word boundary. A reader copying it gets correct behaviour and never meets the
# trap the paragraph exists to teach (D on #1010, proved by character code).
Assert-True ($checklist.Contains('The trap is the form you would write')) `
    'the jq escape example is framed as the trap rather than as the working spelling, because the broken one is the natural one'


# THE PRESCRIPTION, after a reviewer showed the first one was wrong. Prescribing a matcher replaced
# one broken filter with two fallible ones -- `startswith` misses a body whose identity line is not
# first, `contains` matches a body that QUOTES another lane. Either can report a reviewer as having
# no verdict and let them press against the third-lane rule.
Assert-True ($checklist.Contains('do NOT filter at all: list every body and classify it by reading')) `
    'the checklist tells a reader to enumerate and classify rather than to filter -- every matcher available here is lossy in one direction or the other'

# NO BYTE WITHOUT A GLYPH, in the two documents this branch edits. There is no guard for this on
# `.md` anywhere in the repository: `core/protocols/tests/authored_strings_across_the_workspace.rs`
# walks `["rs"]` and the manifest sweep walks `["toml"]`, so a control character in a document every
# lane READS lands unnoticed. It already had: this checklist carried `Git\mingw64<BS>in\git.exe`
# before this branch touched it -- a path whose `\b` had become a real 0x08 -- and the
# sentence added here explaining that 0x08 is a backspace contained two more of them.
#
# THE SWEEP THEN LEFT ITS OWN FILE OUT, and its own file was the worst of the three: four raw
# 0x08 bytes, one of them inside the comment above, which asserted the INVERSION while claiming
# to pin the correction. Injecting a 0x08 into the checklist reddened this suite; injecting one
# here left it green. An instrument measuring a population it is not in cannot fail on itself
# (D's BLOCK on #1010).
foreach ($pair in @(
    @{ Name = 'MERGE-CHECKLIST.md'; Text = $checklist },
    @{ Name = 'AGENTS.md'; Text = $agents },
    @{ Name = 'ci/liveness-rule.tests.ps1'; Text = $guard })) {
    $control = @($pair.Text.ToCharArray() | Where-Object {
            ([int] $_ -lt 32) -and ([int] $_ -ne 9) -and ([int] $_ -ne 10) -and ([int] $_ -ne 13)
        }).Count
    Assert-True ($control -eq 0) `
        "$($pair.Name) carries no C0 control byte other than tab and newline (found $control) -- nothing in this repository scans .md for them, and this file already held one"
}

# BALANCED FENCES. An odd count makes the numbered items after the break render as code, and this
# branch introduced one: a prose note was added after a command block that had already closed,
# leaving its closing fence to open a new one (Codex P2 on #1010).
$fences = @($checklist -split "`r?`n" | Where-Object { $_.TrimStart().StartsWith('```') }).Count
Assert-True (($fences % 2) -eq 0) `
    "the checklist's code fences are balanced ($fences) -- an odd count silently turns the rest of the document into a code block"

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount. A case vanished or was added without updating the declared total." -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $($script:total)/$($script:total)" -ForegroundColor Green
