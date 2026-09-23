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

$ExpectedAssertionCount = 98
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
# #1085/#1165: the THIRD protocol document. It is read here because the one-rule-one-place cells
# at the foot of this suite are about the same rule appearing in two of these three files, and a
# sweep that cannot open the third cannot see half of its own population.
$laneLoopPath = Join-Path $repositoryRoot '.factory/lane-loop.md'

# This suite's own file is in the C0 sweep's population below, so it is READ like the other two
# rather than assumed.
$guardPath = Join-Path $repositoryRoot 'ci/liveness-rule.tests.ps1'

foreach ($required in @($checklistPath, $agentsPath, $guardPath, $laneLoopPath)) {
    if (-not (Test-Path -LiteralPath $required)) {
        Write-Host "HARNESS-BROKE: $required is not where this suite expects it" -ForegroundColor Magenta
        exit 2
    }
}

$checklist = [System.IO.File]::ReadAllText($checklistPath)
$agents = [System.IO.File]::ReadAllText($agentsPath)
$guard = [System.IO.File]::ReadAllText($guardPath)
$laneLoop = [System.IO.File]::ReadAllText($laneLoopPath)

# Keep the population guard meaningful without requiring a particular document size.
Assert-True ((-not [string]::IsNullOrWhiteSpace($checklist)) -and (-not [string]::IsNullOrWhiteSpace($agents))) `
    'both documents were read and contain non-whitespace content'

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
    Assert-True ($allNewlines -gt 0) `
        "CONTROL: $($eolPair.Name) has line endings to count ($allNewlines)"
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

# ---------------------------------------------------------------------------------------------
# #1165: ONE RULE, ONE PLACE -- the five contradictions Codex measured between these documents.
#
# Every one of them is the same defect shape this suite was built for: a rule stated twice, in two
# files, in two wordings, with no mechanism keeping them in step. The repair is never "say it the
# same way in both"; it is to state it ONCE and make the other file point. So each cell below is a
# PAIR -- the second statement is gone, AND the pointer that replaced it is present -- because an
# absence on its own is satisfied by a file that was renamed, emptied, or read from the wrong root.
#
# The `-not Contains` halves are the OPERATIVE spellings, not incidental ones: each is the literal
# sentence that carried the contradiction, so a repair that merely reworded it stays red.

# PROSE IS WRAPPED, AND A GUARD THAT MATCHES RAW BYTES AGES AGAINST THE WRAP. Every cell in this
# section quotes a SENTENCE, and a sentence in a markdown document carries whatever newline the
# margin put in it; matching the raw text makes a reflow look like a repair (on the absences) and a
# repair look like a reflow (on the presences). Both directions were observed while writing this
# section: two pointers landed across a wrap and read as missing. So the prose cells below read a
# whitespace-NORMALISED copy, and only the literal-template cells at the end read physical lines --
# there the newline IS the defect, which is why that pair is measured the other way on purpose.
$checklistFlat = ($checklist -replace '\s+', ' ')
$agentsFlat = ($agents -replace '\s+', ' ')

Write-Host '#1165: one rule, one place'

# ROOT 1. The canonical closing checker (`ci/closing-keywords.ps1`) takes `-Closes` as a
# `System.String[]` and accepts the word `none`: ZERO and SEVERAL intended issues are both legal,
# and item 5 of the checklist states the rule as union == intent. The squash-body read-back rule
# demanded "exactly one closing keyword", which refuses a correct `Refs #N` partial delivery and a
# correct two-issue close. Arity is not the property; agreement with the stated intent is.
Assert-True (-not $checklistFlat.Contains('carries exactly one closing keyword')) `
    'the squash-body read-back does not demand ONE closing keyword -- `-Closes none` and `-Closes 1 2` are both legal in the parser item 5 runs, so an arity test refuses correct pull requests'

Assert-True ($checklistFlat.Contains('its closing set equals the stated intent')) `
    'the squash-body read-back states the property that IS decisive (the set equals the intent, deferred to item 5) -- the control for the absence above'

# ROOT 2. `AGENTS.md`'s exhaustion clause was narrowed to "a non-author lane THAT MAY READ", with a
# citation rule attached (the census names the confining order, and a confinement nobody can cite
# excludes nobody). Item 8's exception kept the OLD predicate -- "live on the board" -- so the same
# census answered two ways depending on which file the presser had open.
Assert-True (-not $checklistFlat.Contains('shows NO other non-author lane live on the board')) `
    'item 8 does not carry its own liveness predicate -- `AGENTS.md` narrowed it to eligible-to-read, and a second copy is a second thing to correct'

Assert-True ($checklistFlat.Contains('under the predicate and the citation rule `AGENTS.md` states for the exhaustion clause')) `
    'item 8 defers the predicate AND its citation rule to `AGENTS.md` -- the control for the absence above'

# ROOT 3. `AGENTS.md` exempts the runner's manifest commit and a conflict-free merge OR REBASE of
# `origin/main` from the mixed-authorship sentence. The checklist enumerated the same list and
# dropped the rebase, so a rebased branch was exempt in one file and mixed in the other. The copy
# had ALREADY drifted at the moment it was written, which is the argument against copying it.
Assert-True (-not $checklistFlat.Contains('are the two exemptions')) `
    'the checklist does not enumerate the authorship exemptions -- its enumeration had already dropped `rebase` from the list it was copying'

Assert-True ($checklistFlat.Contains('is stated in `AGENTS.md` and only there')) `
    'the checklist points at the single statement of the exemptions -- the control for the absence above'

# ROOT 4. Item 12 is the census, and item 8 says in as many words that ITEM 12 GOVERNS who presses.
# But item 12's four classes put a lane in `verdict` by the lane-is-the-unit rule, and under the
# subagent exception the two passes are subagents OF THE SPAWNING LANE -- so item 12 disqualified
# the very session item 8's new sentence appointed as presser. The exception has to live in the
# item that decides the question, and item 8 must not answer it a second time.
Assert-True (-not $checklistFlat.Contains('Under THIS exception the spawning session presses')) `
    'item 8 does not decide who presses under the exception -- item 8 itself says item 12 governs the census, so the answer belongs there, once'

$item12Clause = 'classifies the SUBAGENT and not the lane, and does not disqualify the spawning lane'
Assert-True ($checklistFlat.Contains($item12Clause)) `
    'item 12 carries the subagent exception as a reading of its own `verdict` class -- without it the exception appoints a presser its own census disqualifies'

# POSITION, not merely presence: a clause saying "item 12 governs" is worthless if the clause sits
# in item 8. Proven by index against item 12's own heading, which occurs once in this file.
$item12Heading = '12. **An empty third-lane set is a reading, not a wait.**'
Assert-True (($checklistFlat.IndexOf($item12Heading) -ge 0) -and ($checklistFlat.IndexOf($item12Clause) -gt $checklistFlat.IndexOf($item12Heading))) `
    'the subagent-exception clause sits AFTER item 12 begins, i.e. inside the item that governs the census -- presence alone would be satisfied by the same sentence left in item 8'

# ROOT 5. The attestation forbids reading "this pull request's reviews", and the paragraph under it
# said independence is not from the record and that findings are SUPPOSED to reach later readers
# through the pull request. Both are right about different moments; neither said which moment. A
# reader could satisfy either sentence while violating the other, so the boundary is written out.
Assert-True ($agentsFlat.Contains('CLOSED until this reader has posted its own pass at this head')) `
    '`AGENTS.md` names the instant at which the record opens -- without it the attestation and the not-from-the-record sentence are each a licence to break the other'

Assert-True ($agentsFlat.Contains('OPEN at all times')) `
    '`AGENTS.md` enumerates what a pass reader may read throughout -- the other half of the boundary, so the closed list reads as a cut and not as a blanket'

# ONE STATEMENT OF EACH LITERAL FORM. The disclosure form and the attestation line are both grepped
# for, and the checklist carried a retyped copy of one and a paraphrase of the other -- already
# drifting (`Lane: X` against the `Lane: <letter>` the form specifies). A grep-stable literal that
# exists in two files is a literal that will stop being one.
Assert-True (-not $checklistFlat.Contains('Lane: X (subagent')) `
    'the checklist does not retype the disclosure form -- its copy already spelled the lane field differently from the form `AGENTS.md` specifies'

Assert-True (-not $checklistFlat.Contains('did not read this pull request')) `
    'the checklist does not paraphrase the attestation line -- a census greps for the literal, and the literal must have exactly one home'

Assert-True ($checklistFlat.Contains('the literal attestation line `AGENTS.md` specifies, verbatim and unparaphrased')) `
    'the checklist requires the attestation by reference instead of reproducing it -- the control for the two absences above'

# LITERAL TEMPLATES RENDER ON ONE PHYSICAL LINE. Both are meant to be COPIED, and both had been
# wrapped by an editor mid-sentence: a reader who copies a wrapped template pastes a newline into
# the line a census greps for, and the grep then answers zero on a compliant pass. `Contains` is
# blind to this once the file is one string, so each is measured against the file's LINES.
$agentsLines = $agents -split "`n"
$attestationLines = @($agentsLines | Where-Object { $_.Contains('`Attestation: spawned new for this') })
Assert-True (($attestationLines.Count -eq 1) -and ($attestationLines[0].Contains('before posting.`'))) `
    "the attestation template opens and closes on ONE physical line ($($attestationLines.Count) opening line(s) found) -- a template wrapped mid-sentence is copied with a newline into the string a census greps for"

$laneLoopLines = $laneLoop -split "`n"
$identityLines = @($laneLoopLines | Where-Object { $_.Contains('`Lane: <letter> (subagent') })
Assert-True (($identityLines.Count -eq 1) -and ($identityLines[0].Contains('Head: <sha8>`'))) `
    "the subagent identity-line template opens and closes on ONE physical line ($($identityLines.Count) opening line(s) found) -- same defect, same measurement, in the file that tells a lane what to publish"

# THE ONE LITERAL THAT LEGITIMATELY LIVES IN TWO FILES, AND THE CELL THAT KEEPS IT ONE LITERAL.
# `AGENTS.md` states the LANE FIELD form; lane-loop.md composes that field into the whole identity
# line a lane publishes, which is the thing lane-loop.md is for. So the bytes appear twice on
# purpose - and until now nothing compared them, which is exactly how the `Lane: X` copy in the
# checklist drifted. This cell holds NO third copy of the form: it lifts it out of `AGENTS.md` at
# run time and asks whether lane-loop.md's template still contains those bytes. If `AGENTS.md`
# restates the form, the extraction finds no single source and the cell says so rather than passing.
$formMatches = @([regex]::Matches($agents, '`(Lane: <letter> \(subagent[^`]*)`'))
Assert-True (($formMatches.Count -eq 1) -and $laneLoop.Contains($formMatches[0].Groups[1].Value)) `
    "lane-loop.md's identity-line template embeds the lane-field form `AGENTS.md` states, byte for byte ($($formMatches.Count) form(s) found in AGENTS.md) -- the form is copied here on purpose and this is the only thing holding the two copies together"

# Keep the third-document population guard meaningful without requiring a particular size.
Assert-True (-not [string]::IsNullOrWhiteSpace($laneLoop)) `
    'lane-loop.md was read and contains non-whitespace content'


Write-Host '#1085: the gate deletes its own target as it runs'

# OWNER ORDER, 2026-09-20: the gates must delete themselves as they run; the order names AGENTS.md
# as where it has to be written. Measured that night: the three managed target roots held ~150
# per-PR targets, almost all of them for MERGED or CLOSED pull requests, and `E:` sat below the
# 30 GB floor this same document sets, so the second gate slot could not be used at all. The
# section said only that the runner removes the PREVIOUS target of the SAME pull request before a
# re-run and that the presser removes it after a merge -- so every gated PR that is never pressed,
# and every closed one, leaves a target behind for good.
Assert-True ($agentsFlat.Contains('removes `<TargetRoot>\pr<N>` as soon as the run ends and its receipt is pushed')) `
    'the Disk hygiene section makes the RUNNER delete its own target at the end of the run -- removing only the same PR''s previous target leaves one behind for every PR that is gated and never re-run'

Assert-True ($agentsFlat.Contains('the build cache is not evidence, the manifest on the server is')) `
    'the section says why the target may go at once -- a reader who believes the target IS the evidence will not delete it, whatever the rule says'

Assert-True ($agentsFlat.Contains('benches under `-BenchRoot` once their pull request is no longer open')) `
    'the runner''s own benches are covered too -- the same measurement found ~90 stale benches under the board''s bench root, which no lane owns and no sentence named'

# The presser line is the BACKSTOP, not a duplicate: a run the runner could not clean (killed,
# crashed, disk full) still has an owner. A repair that deleted it would trade one leak for another.
Assert-True ($agentsFlat.Contains('whoever presses the merge removes `<TargetRoot>\pr<N>` afterwards, the backstop for a run the runner could not clean')) `
    'the presser''s removal survives, relabelled as the backstop -- the control for the three presences above, and the cell that catches a repair which moves the duty instead of adding to it'

# The section dates every incident it carries. This one is the reason the rule changed.
Assert-True ($agentsFlat.Contains('measured again 2026-09-20')) `
    'the 2026-09-20 measurement is named with its date, the way the rest of this section names its incidents -- a rule whose incident is not written down is re-argued at the next gate'



Write-Host '#1165 (Codex BLOCK): the boundary is per-head, and the parser has an executable file path'

# HOLE 1, as measured by Codex on the published head. The boundary block cut the record by CONTENT
# at the head under review, and then undid the cut one paragraph later: "at a LATER head nothing is
# closed at all" is true of findings about EARLIER heads and false of the head the reader is
# actually reading. A reader that re-pins to the newest head is a FIRST reader of THAT head, and
# under the old sentence it could open the other pass on that same head and copy it -- the precise
# thing the attestation exists to forbid. The repair keeps the relay (earlier heads stay open) and
# holds the cut where it belongs (this reader's own head).
Assert-True (-not $agentsFlat.Contains('at a LATER head nothing is closed at all')) `
    'the boundary does not open the whole record at a later head -- a reader at a newer head is a FIRST reader of that head, and that sentence let it consume the other pass on the very head it was about to vouch for'

Assert-True ($agentsFlat.Contains('findings about EARLIER heads are open, and findings and verdicts about the head this reader is itself reading stay CLOSED until it has posted its own pass at that head')) `
    'the boundary states the per-head rule that replaces it -- the control for the absence above, and the half that keeps a relayed finding reaching every later reader'

# HOLE 1b. The CLOSED list cut by content but named only review surfaces, while the OPEN list named
# the pull request body and the issue UNCONDITIONALLY. A finding copied out of a review and into
# the body or the issue therefore changed class by being moved, which is a bypass anyone can take
# by accident. The cut is by what a passage CARRIES, so it has to hold on every surface.
Assert-True ($agentsFlat.Contains('on whichever surface it was posted - a comment, a review, an inline review thread, the pull request body, or an issue')) `
    'the CLOSED list binds every surface a finding can be copied to -- otherwise copying a finding from a review into the body or the issue launders it into the OPEN list'

Assert-True ($agentsFlat.Contains('except any passage of them carrying a verdict word or another reader''s findings on the head under review')) `
    'the OPEN list carries the same content restriction on the pull request body, the title and the issue -- the other half of the bypass, on the side that enumerates what may be read'

# POSITION, not merely presence: the restriction is worthless sitting in the CLOSED paragraph. The
# two list headings each occur once, so the index comparison is exact.
$openHeading = 'OPEN at all times:'
$closedHeading = 'CLOSED until this reader has posted its own pass at this head:'
$openRestriction = 'except any passage of them carrying a verdict word or another reader''s findings on the head under review'
Assert-True (($agentsFlat.IndexOf($openHeading) -ge 0) -and ($agentsFlat.IndexOf($openRestriction) -gt $agentsFlat.IndexOf($openHeading)) -and ($agentsFlat.IndexOf($openRestriction) -lt $agentsFlat.IndexOf($closedHeading))) `
    'the restriction sits INSIDE the OPEN list, between its heading and the CLOSED one -- presence alone is satisfied by the same words left in the closed paragraph, where they restrict nothing'

# HOLE 2. The squash-body read-back told the presser to re-read the composed LOCAL file "with item
# 5's parser". Measured against the program: `ci/closing-keywords.ps1` declares `-Number` as a
# MANDATORY `[int]` and fetches the pull request through `gh` -- it has no file, text or stdin
# input at all, so the instruction named a run nobody can perform, on a checklist whose whole point
# is that a step either executes or is not a step. The repair says that plainly and hands over the
# invocation that DOES work: the AST lift this same file already carries for reading a landed
# squash message, pointed at the composed file instead.
Assert-True (-not $checklistFlat.Contains('with item 5''s parser, against the intent the presser has already stated')) `
    'the squash-body read-back no longer prescribes a run of item 5''s parser over a local file -- `ci/closing-keywords.ps1` takes `-Number` and fetches the pull request, so that step could not be executed as written'

Assert-True ($checklistFlat.Contains('takes no file, text or stdin input -- `-Number` is mandatory and it fetches the pull request through `gh`')) `
    'the checklist states the limit of the program in as many words -- a presser who is told to "use item 5''s parser" and finds no `-File` switch guesses, and a guess is what this file exists to prevent'

Assert-True ($checklistFlat.Contains('Get-ClosingReferences -Text ([System.IO.File]::ReadAllText($bodyPath))')) `
    'the checklist hands over the executable invocation for the LOCAL composed file -- the AST lift it already prescribes for a landed squash message, with the composed body as its text'

Assert-True ($checklistFlat.Contains('`-Number`/`-Closes` run against the pull request stays a SEPARATE step')) `
    'the remote check is kept as its own step -- the two ask different questions (what the composed file carries, versus what the union of the pull request''s texts carries) and collapsing them would retire the one item 5 gates on'
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
