# #667: isolated tests for ci/manifest-name.ps1.
#
# Dot-sources ONLY manifest-name.ps1, never gate.ps1. Every case writes under a throwaway temp
# directory this script creates and removes itself, so running it needs no slot, no cargo, and no
# coordination with any other lane.
#
# Every Get-ChildItem count below is wrapped in @(). PowerShell collapses an EMPTY result to
# $null, so `.Count` throws under StrictMode and a genuine zero cannot be asserted -- the exact
# defect I fixed in ci/gate-run-overlap.ps1 for #653, met again in my own test file. A zero has
# to be a zero on both sides of the instrument.
#
# Homegrown PASS/FAIL/HARNESS-BROKE harness with a declared expected count, matching
# ci/slot-lock.tests.ps1. The declared total is the point: on the sibling suite for #638 I declared
# 25 and 23 ran, and the harness refused rather than reporting 23 green ones quietly.
#
# THE NUMBER IS NOT REPEATED IN THIS COMMENT, deliberately. It used to be: the prose named a count
# beside the constant that named the same count, and the two drifted apart every time a cell was
# added or removed -- the constant moved with the code and the sentence did not, so the sentence
# taught a number nobody was using. (#686's sibling suite has the same shape and was still arguing
# for 24 while its constant went 35, 36, 33.) One place holds the value; this comment holds only
# what the value MEANS.
#
# What it means: the count is Assert-* CALLS reached at runtime. The two function DEFINITIONS are
# not calls, and Assert-Equal's delegation to Assert-True fires once per call rather than as an
# assertion of its own. A miscount here has twice caught a cell that stopped running while its
# neighbours stayed green -- which is the mechanism working, not a nuisance.
$ExpectedAssertionCount = 50

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$script:total = 0
$script:failed = 0

function Assert-True {
    param([Parameter(Mandatory)][bool] $Condition, [Parameter(Mandatory)][string] $Label)
    $script:total++
    if ($Condition) { Write-Host "PASS  $Label" } else { $script:failed++; Write-Host "FAIL  $Label" }
}

function Assert-Equal {
    param([Parameter(Mandatory)][AllowNull()] $Expected, [Parameter(Mandatory)][AllowNull()] $Actual, [Parameter(Mandatory)][string] $Label)
    Assert-True -Condition ("$Expected" -eq "$Actual") -Label "$Label (expected '$Expected', got '$Actual')"
}

function New-IOExceptionWithCode {
    # HResult has no public setter in .NET Framework, so the field is set by reflection. That is
    # the only way to arrange the case that DISCRIMINATES -- the exact English phrase carrying a
    # DIFFERENT code -- and arranging it is the whole point: a cell that cannot separate the two
    # candidates proves nothing about which one the predicate reads.
    param([string] $Message, [int] $Code)
    $exception = New-Object System.IO.IOException($Message)
    $field = [System.Exception].GetField('_HResult', [System.Reflection.BindingFlags]::NonPublic -bor [System.Reflection.BindingFlags]::Instance)
    if ($null -eq $field) { throw 'HARNESS-BROKE: no _HResult field to set; this fixture cannot arrange its case' }
    $field.SetValue($exception, $Code)
    return $exception
}

. "$PSScriptRoot/manifest-name.ps1"

$root = Join-Path ([System.IO.Path]::GetTempPath()) ("manifest-name-tests-" + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $root | Out-Null

try {
    $head = 'a' * 40
    # ParseExact with InvariantCulture, and this is the finding turned on its own test: the cell
    # below proves the FILENAME does not follow the machine's calendar, and the fixture that feeds
    # it was parsed with the AMBIENT one. Under th-TH the one-argument Parse reads these digits
    # through the Buddhist calendar before the cell ever swaps cultures, so $instant would not be
    # Gregorian 2026 and the assertion could fail while production was correct. A cell that proves
    # locale-independence must not be locale-dependent to set itself up.
    $instant = [datetime]::ParseExact(
        '2026-08-28T07:01:49.123',
        'yyyy-MM-ddTHH:mm:ss.fff',
        [cultureinfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal
    )

    # ---- THE DEFECT: same head, same second, and now distinct names -------------------------
    # This is the exact pair the old scheme collapsed into one file. Both names are generated at
    # the SAME instant, so only the suffix can separate them -- sub-second precision alone would
    # still collide here, which is why the suffix exists.
    # Deterministic suffixes, injected. Calling the RANDOM generator twice and asserting the
    # results differ proves luck, not formatting: a legitimate 1-in-2^32 collision would fail this
    # suite for no defect. This is the same discipline as the fixed instant one line down -- if the
    # instant is pinned so it cannot be what separates the names, the suffix must be pinned too, or
    # the cell quietly depends on the thing it is supposed to be testing around.
    $first = New-GateManifestFileName -HeadSha $head -Now $instant -Suffix 'aaaaaaaa'
    $second = New-GateManifestFileName -HeadSha $head -Now $instant -Suffix 'bbbbbbbb'
    Assert-True -Condition ($first -ne $second) -Label 'two names from the same head and instant differ by suffix alone'

    # And the injection point itself is exercised, or the parameter could rot unused.
    $sequence = [System.Collections.Queue]::new(@('s1', 's2'))
    $fromSource = New-GateManifestFileName -HeadSha $head -Now $instant -SuffixSource { $sequence.Dequeue() }
    Assert-True -Condition ($fromSource.EndsWith('-s1.json')) -Label 'SuffixSource supplies the suffix when no literal is given'
    Assert-True -Condition ($first.StartsWith('aaaaaaaaaaaa-')) -Label 'the name still leads with head12, so it stays sortable by run'
    Assert-True -Condition ($first -like '*20260828T070149.123Z*') -Label 'the stamp keeps sub-second precision and is human-readable'
    Assert-True -Condition ($first.EndsWith('.json')) -Label 'the name is still a .json file the glob will find'

    # ---- 'name taken' is decided by the runtime's code, not by its prose --------------------
    # The message `"The file '...' already exists."` is a .NET resource string chosen by
    # CurrentUICulture and rewritable in a servicing update; matching it infers what the runtime
    # MEANT. The HResult is what it SAID. No cell here asserts a failure under another culture:
    # this machine has no localised satellites, so that failure is not demonstrable and a cell
    # claiming it would assert what cannot be shown.
    $taken = New-IOExceptionWithCode -Message '' -Code -2147024816   # 0x80070050 ERROR_FILE_EXISTS
    Assert-True -Condition (Test-IOExceptionIsNameTaken -Exception $taken) -Label 'an EMPTY message with the exists HResult still decides name-taken'

    $german = New-IOExceptionWithCode -Message 'Die Datei existiert bereits.' -Code -2147024713   # 0x800700B7 ERROR_ALREADY_EXISTS
    Assert-True -Condition (Test-IOExceptionIsNameTaken -Exception $german) -Label 'a non-English message with an exists HResult decides name-taken'

    $englishButNotExists = New-IOExceptionWithCode -Message 'The file already exists.' -Code -2147024893   # 0x80070003 ERROR_PATH_NOT_FOUND
    Assert-True -Condition (-not (Test-IOExceptionIsNameTaken -Exception $englishButNotExists)) -Label 'the exact English phrase with a different HResult is NOT name-taken'

    Assert-True -Condition (-not (Test-IOExceptionIsNameTaken -Exception $null)) -Label 'a null exception is not name-taken'

    # THE POPULATION CELL, and it is the one that pins the boundary rather than the form. The three
    # cells above kill a decider that ignores the HResult, one that reads the message, and one that
    # accepts any 0x8007 code -- but a BLACKLIST ("anything that is not ERROR_PATH_NOT_FOUND is a
    # collision") agrees with the real predicate on every code they name. Measured: whitelist and
    # blacklist differ on exactly ONE input, ERROR_SHARING_VIOLATION -- an existing file that cannot
    # be opened. Without this cell the suite fixes the shape of the answer and leaves the edge of it
    # unspecified.
    $sharing = New-IOExceptionWithCode -Message 'The process cannot access the file because it is being used by another process.' -Code -2147024864   # 0x80070020 ERROR_SHARING_VIOLATION
    Assert-True -Condition (-not (Test-IOExceptionIsNameTaken -Exception $sharing)) -Label 'an existing-but-unopenable file is NOT name-taken, so a blacklist decider dies here'

    # ---- the stamp does not move with the machine's calendar --------------------------------
    # Under a non-Gregorian calendar (th-TH is Buddhist) `ToString` formats the YEAR in that
    # calendar, so a 2026 run would be stamped 2569: the filename stops being the documented
    # Gregorian UTC stamp and no longer sorts against any manifest written elsewhere.
    $previousCulture = [System.Threading.Thread]::CurrentThread.CurrentCulture
    try {
        [System.Threading.Thread]::CurrentThread.CurrentCulture = [cultureinfo]::GetCultureInfo('th-TH')
        $underThai = New-GateManifestFileName -HeadSha $head -Now $instant -Suffix 'culture'
    } finally {
        [System.Threading.Thread]::CurrentThread.CurrentCulture = $previousCulture
    }
    $underInvariant = New-GateManifestFileName -HeadSha $head -Now $instant -Suffix 'culture'
    Assert-Equal $underInvariant $underThai 'the name is identical under a Buddhist-calendar culture'
    Assert-True -Condition ($underThai -like '*20260828T*') -Label 'the year stays Gregorian rather than following the machine'

    # ---- a short head is not a crash --------------------------------------------------------
    # `Substring(0, 12)` on a shorter string throws; the old line would have died on a test double.
    Assert-True -Condition ((New-GateManifestFileName -HeadSha 'abc' -Now $instant).StartsWith('abc-')) -Label 'a head shorter than 12 characters is used whole rather than throwing'

    # ---- no BOM: the manifest is machine-read by a strict parser ----------------------------
    # This used to read a file left behind by a `Write-GateManifestCreateNew` cell, so the property
    # was asserted about a writer the gate never runs (#692). It writes through the production path
    # now: a BOM here is only a defect if the bytes the gate actually produces carry one.
    $bomCase = Join-Path $root 'bom'; New-Item -ItemType Directory -Path $bomCase | Out-Null
    # @() because PowerShell UNROLLS a single-element array -- the same trap the concurrent-run cell
    # below documents, and indexing a bare string would compare its first character.
    $bomPath = @(Write-GateManifestPair -PrimaryDirectory $bomCase -Json '{"run":"bom"}' -HeadSha $head -Now $instant -SuffixSource { 'bom' })[0]
    $bytes = [System.IO.File]::ReadAllBytes($bomPath)
    Assert-True -Condition (-not ($bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)) -Label 'the file the gate writes carries no UTF-8 BOM'

    # ---- THE POST-CREATE FAILURE PATH, now actually reachable -------------------------------
    # The previous cell here was VACUOUS and the review caught it: it passed $null for a mandatory
    # [string], so PowerShell rejected the argument during parameter binding and the function body
    # never ran at all. Both assertions stayed green with the entire cleanup block deleted. I had
    # even declared a partial seal on this path -- the seal was itself understated, because I had
    # not verified the cell reached the function.
    #
    # The helper now takes a WriteContent seam for exactly this: a failure AFTER CreateNew cannot
    # be provoked from outside otherwise.
    $case = Join-Path $root 'postcreate'; New-Item -ItemType Directory -Path $case | Out-Null
    $threw = $false
    try {
        Write-GateManifestPair -PrimaryDirectory $case -Json '{"run":"doomed"}' -HeadSha $head -WriteContent { throw 'simulated disk-full during write' } | Out-Null
    } catch { $threw = $true }
    Assert-True -Condition $threw -Label 'a failure after CreateNew fails the call'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $case -Filter '*.json').Count 'no manifest-named file survives a failed write'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $case -Filter '*.tmp').Count 'the staged temp file is cleaned up too'
    # WHAT HAS NO RED HERE: the branch that REPORTS an undeletable temp. Disabling it leaves this
    # suite green, because provoking it needs a delete that fails -- a lock or a persistent fault
    # the test cannot arrange. Verified by running that sabotage, not assumed. It ships on
    # mechanism, and the exposure is small by construction: what it fails to remove is a `.tmp`,
    # which no reader treats as a manifest. That is the whole reason staging came first.

    # ---- a partial file never carries a manifest name --------------------------------------
    # Content is staged in a .tmp and MOVED into place only when complete, so the window in which a
    # manifest-named file is incomplete does not exist. This is what makes the cleanup best-effort
    # honestly: a leftover .tmp is litter, where a half-written .json was a false record.
    $case = Join-Path $root 'staging'; New-Item -ItemType Directory -Path $case | Out-Null
    $seen = [System.Collections.Generic.List[string]]::new()
    Write-GateManifestPair -PrimaryDirectory $case -Json '{"run":"ok"}' -HeadSha $head -WriteContent {
        param($stream)
        $seen.Add((@(Get-ChildItem -LiteralPath $case -Filter '*.json').Count).ToString())
        $bytes = [System.Text.Encoding]::UTF8.GetBytes('{"run":"ok"}')
        $stream.Write($bytes, 0, $bytes.Length)
    } | Out-Null
    Assert-Equal '0' $seen[0] 'while the content is being written, no .json exists yet'
    Assert-Equal 1 @(Get-ChildItem -LiteralPath $case -Filter '*.json').Count 'the manifest appears only once it is complete'

    # ---- a concurrent run's reservation is respected, not deleted --------------------------
    # The `.json` precheck cannot see a run that owns only the `.tmp` yet. Before this, the second
    # run fell into the generic cleanup and deleted the FIRST run's reservation on its way out --
    # both runs losing their manifest, the mutual-destruction version of the overwrite this helper
    # exists to stop.
    $case = Join-Path $root 'concurrent'; New-Item -ItemType Directory -Path $case | Out-Null
    $held = New-GateManifestFileName -HeadSha $head -Now $instant -Suffix 'held'
    $heldTmp = Join-Path $case "$held.tmp"
    [System.IO.File]::WriteAllText($heldTmp, 'first run, still writing', (New-Object System.Text.UTF8Encoding($false)))
    $suffixes = [System.Collections.Queue]::new(@('held', 'free'))
    $written = Write-GateManifestPair -PrimaryDirectory $case -Json '{"run":"second"}' -HeadSha $head -Now $instant -SuffixSource { $suffixes.Dequeue() }
    # @() around the result: PowerShell UNROLLS a single-element array, so `$written[0]` indexed
    # into a string and compared its first character. Third time this unrolling has bitten in this
    # work -- the other two were empty results collapsing to $null.
    Assert-True -Condition (@($written)[0] -like '*-free.json') -Label 'a name whose .tmp is held by another run is abandoned for a fresh one'
    Assert-True -Condition (Test-Path -LiteralPath $heldTmp) -Label "the other run's reservation is left alone"
    Assert-Equal 'first run, still writing' ([System.IO.File]::ReadAllText($heldTmp)) "the other run's bytes are untouched"

    # ---- a fault raised from INSIDE the write callback does not promote the file ------------
    # NAMED FOR WHAT IT PROVES, after sabotage showed it does not prove what I first called it.
    #
    # I labelled this "a Dispose that fails must not promote the file". It does not test that:
    # disposing twice is idempotent, so the throw here comes from the Write on a closed stream --
    # the write path again, not the dispose path. Wrapping the helper's own `$stream.Dispose()`
    # back in `try { } catch { }` leaves this suite GREEN, which is how I know.
    #
    # SO THE DISPOSE-TIME FAULT HAS NO RED. The fix is still right by mechanism -- a filesystem can
    # report a delayed flush fault only at Dispose, and swallowing it would promote possibly
    # truncated bytes to a final .json, the exact invalid manifest staging exists to prevent. But
    # provoking it needs a filesystem that fails on flush, which this suite cannot arrange, and I
    # am not going to let a cell whose name implies otherwise stand in for that. Second time on
    # this same branch that I nearly claimed coverage I do not have.
    $case = Join-Path $root 'disposefails'; New-Item -ItemType Directory -Path $case | Out-Null
    $threw = $false
    try {
        Write-GateManifestPair -PrimaryDirectory $case -Json '{"run":"x"}' -HeadSha $head -WriteContent {
            param($stream)
            $stream.Dispose()
            # Disposed twice: the second raises, standing in for a delayed flush fault.
            $stream.Write([byte[]]@(1), 0, 1)
        } | Out-Null
    } catch { $threw = $true }
    Assert-True -Condition $threw -Label 'a fault raised inside the write callback fails the call'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $case -Filter '*.json').Count 'nothing is promoted to a final name after that fault'

    # ---- one name across BOTH stores, chosen before either is finalised --------------------
    # Writing the stores independently let the second rename itself (orphaning the twin) or collide
    # and warn -- leaving the PREVIOUS run's manifest exactly where classify-run.ps1:267-273 looks
    # for this run's, so the later classification overwrote the earlier record.
    $primary = Join-Path $root 'pair-a'; New-Item -ItemType Directory -Path $primary | Out-Null
    $secondary = Join-Path $root 'pair-b'; New-Item -ItemType Directory -Path $secondary | Out-Null
    $written = Write-GateManifestPair -PrimaryDirectory $primary -SecondaryDirectory $secondary -Json '{"run":"paired"}' -HeadSha $head
    Assert-Equal 2 @($written).Count 'both stores receive the run'
    Assert-Equal ([System.IO.Path]::GetFileName(@($written)[0])) ([System.IO.Path]::GetFileName(@($written)[1])) 'both stores carry the SAME name'

    # ---- a failed SECOND move rolls the first one back -------------------------------------
    # Arranged, not hoped for: the WriteContent callback runs AFTER the name precheck and BEFORE
    # the moves, so an obstruction created from inside it reaches exactly the window where the
    # first move can succeed and the second cannot. Half a pair is worse than none -- the primary
    # .json is visible to every manifest reader and records a run that never reaches RUN-END.
    $primary = Join-Path $root 'roll-a'; New-Item -ItemType Directory -Path $primary | Out-Null
    $secondary = Join-Path $root 'roll-b'; New-Item -ItemType Directory -Path $secondary | Out-Null
    $blockName = New-GateManifestFileName -HeadSha $head -Now $instant -Suffix 'block'
    $threw = $false
    try {
        Write-GateManifestPair -PrimaryDirectory $primary -SecondaryDirectory $secondary -Json '{"run":"half"}' -HeadSha $head -Now $instant -SuffixSource { 'block' } -WriteContent {
            param($stream)
            # A DIRECTORY at the secondary's final path: File.Move cannot overwrite it.
            $blocker = Join-Path $secondary $blockName
            if (-not (Test-Path -LiteralPath $blocker)) { New-Item -ItemType Directory -Path $blocker | Out-Null }
            $bytes = [System.Text.Encoding]::UTF8.GetBytes('{"run":"half"}')
            $stream.Write($bytes, 0, $bytes.Length)
        } | Out-Null
    } catch { $threw = $true }
    Assert-True -Condition $threw -Label 'a failed second move fails the call'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $primary -Filter '*.json' -File).Count 'the FIRST store is rolled back rather than left holding half a pair'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $primary -Filter '*.tmp' -File).Count 'no staging file is left behind either'

    # ---- both stores resolving to ONE directory is not a collision with itself -------------
    # GRAPHHELM_SLOT_DIR can point at the repository, and a path can arrive with a trailing
    # separator or a `.` segment. Undeduplicated, the first pass creates the .tmp and the second
    # reads its OWN file as a concurrent reservation -- abandoning the name three times and ending
    # every valid gate run with no manifest. The concurrency guard biting the run that armed it.
    $case = Join-Path $root 'samedir'; New-Item -ItemType Directory -Path $case | Out-Null
    $written = Write-GateManifestPair -PrimaryDirectory $case -SecondaryDirectory $case -Json '{"run":"one"}' -HeadSha $head
    Assert-Equal 1 @($written).Count 'one directory named twice produces ONE manifest, not a stalemate'
    Assert-Equal 1 @(Get-ChildItem -LiteralPath $case -Filter '*.json').Count 'and one file on disk'

    $case = Join-Path $root 'aliasdir'; New-Item -ItemType Directory -Path $case | Out-Null
    $alias = (Join-Path $case '.') + [System.IO.Path]::DirectorySeparatorChar
    $written = Write-GateManifestPair -PrimaryDirectory $case -SecondaryDirectory $alias -Json '{"run":"one"}' -HeadSha $head
    Assert-Equal 1 @($written).Count 'a trailing separator and a . segment are the same directory'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $case -Filter '*.tmp').Count 'no staging file is stranded by the dedup'

    # ---- a survived finalised manifest is announced with the shared marker ------------------
    # gate.ps1 matches this marker to decide whether a primary-only retry is SAFE. Without it the
    # retry writes a second complete-looking manifest while RUN-END names one -- two records of one
    # run. The marker has a single producer so the two sides cannot drift; a private copy of a
    # message on the matching side is the defect this repository has already been bitten by.
    Assert-True -Condition (-not [string]::IsNullOrWhiteSpace($script:FinalisedManifestSurvivedKey)) -Label 'the survived-manifest key is defined by the library, not by its caller'

    # TYPED state, not a phrase in a sentence. The predicate's ARMING still has no red -- provoking
    # it needs a delete that fails between the move and the rollback, and the WriteContent seam runs
    # before the moves -- but the DECISION does, and it is the half that corrupts the record when
    # wrong. The third cell is the one that matters: an ordinary IO failure whose message HAPPENS to
    # contain the marker text must NOT arm the refusal, or a slot directory named after it turns
    # every durable hiccup into a red gate. That is exactly what matching text allowed.
    $typed = New-Object System.Exception('a finalised manifest could not be rolled back')
    $typed.Data[$script:FinalisedManifestSurvivedKey] = $true
    Assert-True -Condition (Test-ManifestRollbackLeftFinalised -Exception $typed) -Label 'a rollback that left a finalised manifest is recognised by its typed state'

    $ordinary = New-Object System.Exception('access denied writing D:\slot\gate-runs')
    Assert-True -Condition (-not (Test-ManifestRollbackLeftFinalised -Exception $ordinary)) -Label 'an ordinary durable failure is NOT mistaken for it'

    $lookalike = New-Object System.Exception('access denied writing D:\FINALISED-MANIFEST-SURVIVED\gate-runs')
    Assert-True -Condition (-not (Test-ManifestRollbackLeftFinalised -Exception $lookalike)) -Label 'a PATH that merely spells the old marker does not arm the refusal'

    Assert-True -Condition (-not (Test-ManifestRollbackLeftFinalised -Exception $null)) -Label 'a null exception does not arm the refusal'

    $case = Join-Path $root 'stuck'; New-Item -ItemType Directory -Path $case | Out-Null
    $secondary = Join-Path $root 'stuck-b'; New-Item -ItemType Directory -Path $secondary | Out-Null
    $stuckName = New-GateManifestFileName -HeadSha $head -Now $instant -Suffix 'stuck'
    $message = ''
    $held = $null
    try {
        Write-GateManifestPair -PrimaryDirectory $case -SecondaryDirectory $secondary -Json '{"run":"x"}' -HeadSha $head -Now $instant -SuffixSource { 'stuck' } -WriteContent {
            param($stream)
            # Blocks the secondary move. It does NOT hold the primary open -- see the note below.
            $blocker = Join-Path $secondary $stuckName
            if (-not (Test-Path -LiteralPath $blocker)) { New-Item -ItemType Directory -Path $blocker | Out-Null }
            $bytes = [System.Text.Encoding]::UTF8.GetBytes('{"run":"x"}')
            $stream.Write($bytes, 0, $bytes.Length)
        } | Out-Null
    } catch { $message = $_.Exception.Message }
    if ($held) { $held.Dispose() }
    Assert-True -Condition ($message -ne '') -Label 'a blocked secondary move still fails the call'
    # WHAT THIS DOES NOT PROVE, named rather than implied: the STUCK rollback. Making the delete
    # fail needs the finalised primary held open without delete sharing -- a reader or antivirus in
    # production, which this suite cannot arrange from inside the callback that runs before the
    # move exists. So the marker's DEFINITION is asserted above and gate.ps1's refusal to fall back
    # rests on mechanism. Said plainly because a cell named for the stuck path, passing on the
    # ordinary path, would read as coverage of the case that actually corrupts the record.

    # ---- a name already taken in the SECOND store is skipped for BOTH ----------------------
    # The invariant is the pair, so an incumbent anywhere disqualifies the name everywhere -- the
    # first store must not be finalised under a name the second cannot accept.
    $primary = Join-Path $root 'pair-c'; New-Item -ItemType Directory -Path $primary | Out-Null
    $secondary = Join-Path $root 'pair-d'; New-Item -ItemType Directory -Path $secondary | Out-Null
    $incumbentName = New-GateManifestFileName -HeadSha $head -Now $instant -Suffix 'taken'
    [System.IO.File]::WriteAllText((Join-Path $secondary $incumbentName), '{"run":"incumbent"}', (New-Object System.Text.UTF8Encoding($false)))
    $threw = $false
    try {
        Write-GateManifestPair -PrimaryDirectory $primary -SecondaryDirectory $secondary -Json '{"run":"newcomer"}' -HeadSha $head -SuffixSource { 'taken' } -Now $instant | Out-Null
    } catch { $threw = $true }
    Assert-True -Condition $threw -Label 'a name source that cannot avoid an incumbent throws rather than overwriting'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $primary -Filter '*.json').Count 'the FIRST store is never finalised under a name the second cannot accept'
    Assert-Equal '{"run":"incumbent"}' ([System.IO.File]::ReadAllText((Join-Path $secondary $incumbentName))) 'the incumbent in the second store is untouched'

    # ---- #742: a CORRECTION is both stores or neither, exactly as a creation is --------------
    # The correction path used to carry its own copy of this discipline in gate.ps1. It could not
    # roll back, and the reason was one argument: `File.Replace` was passed a real null for
    # destinationBackupFileName, so the destination's original bytes were gone the instant the first
    # replace landed. Measured on this runtime: with a null backup the directory afterwards holds
    # only the replaced file. Its own message admitted the outcome -- "an earlier copy WAS corrected,
    # so the two stores now disagree" -- which is half a pair, the state the creation path has
    # refused to leave since #674.
    $ca = Join-Path $root 'corr-a'; New-Item -ItemType Directory -Path $ca | Out-Null
    $cb = Join-Path $root 'corr-b'; New-Item -ItemType Directory -Path $cb | Out-Null
    $corrName = 'aaaaaaaaaaaa-20260906T000000.000Z-corr.json'
    $pathA = Join-Path $ca $corrName
    $pathB = Join-Path $cb $corrName
    [System.IO.File]::WriteAllText($pathA, '{"status":"GREEN"}')
    [System.IO.File]::WriteAllText($pathB, '{"status":"GREEN"}')

    # ARRANGEMENT, asserted rather than assumed: the accessor below reads what this cell wrote. A
    # rollback assertion against a path nothing ever wrote passes for free (#920, my own).
    Assert-Equal '{"status":"GREEN"}' ([System.IO.File]::ReadAllText($pathA)) 'ARRANGEMENT: the first store holds the record the correction rewrites'

    $threw = $false
    try {
        Write-ManifestPairContent -FinalPaths @($pathA, $pathB) -Json '{"status":"RED"}' -Commit 'Replace' -BeforeCommit {
            param($staged)
            # The SECOND commit's source, removed in the only window where the first can land and
            # the second cannot: after every staging file exists, before the first replace.
            # `File.Replace` fails when its source is gone.
            [System.IO.File]::Delete($staged[1])
        } | Out-Null
    } catch { $threw = $true }

    Assert-True -Condition $threw -Label 'a failed second replace fails the call'
    Assert-Equal '{"status":"GREEN"}' ([System.IO.File]::ReadAllText($pathA)) 'the FIRST store is rolled back, not left corrected while the second still says otherwise'
    Assert-Equal '{"status":"GREEN"}' ([System.IO.File]::ReadAllText($pathB)) 'and the second store is untouched'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $ca -Filter '*.correcting' -File).Count 'no staging file survives the rollback'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $ca -Filter '*.backup' -File).Count 'and the backup the rollback needs is not left behind as litter'

    # CONTROL, and it is the half that makes the four assertions above mean something: the same call
    # WITHOUT the obstruction corrects both stores. A cell that never reached the commit at all
    # would satisfy every rollback assertion by having corrected nothing, and would keep satisfying
    # them after the rollback was deleted.
    $corrected = Write-ManifestPairContent -FinalPaths @($pathA, $pathB) -Json '{"status":"RED"}' -Commit 'Replace'
    Assert-Equal 2 @($corrected).Count 'CONTROL: an unobstructed correction commits BOTH stores'
    Assert-Equal '{"status":"RED"}' ([System.IO.File]::ReadAllText($pathA)) 'CONTROL: and the first store really is corrected, so the commit path was reached'

} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $script:total assertions, expected $ExpectedAssertionCount"
    exit 2
}
if ($script:failed -gt 0) { Write-Host "FAILED: $script:failed of $script:total"; exit 1 }
Write-Host "PASSED: $script:total of $ExpectedAssertionCount"
