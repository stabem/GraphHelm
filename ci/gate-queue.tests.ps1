# #902: isolated tests for ci/gate-queue.ps1 -- the lane-facing half of the gate runner.
#
# Dot-sources nothing and starts no gate. Every case runs the script as a CHILD PROCESS against a
# throwaway queue directory this file creates and removes itself, because the thing under test is
# the EXIT CODE and the bytes on disk, and both of those are properties of an invocation rather than
# of a function. A dot-sourced `exit` would end this harness instead of being observed.
#
# Homegrown PASS/FAIL harness, matching the sibling suites: this repository carries no Pester
# dependency and one issue's worth of tests is not the place to add one.
#
# DECLARED ASSERTION COUNT, per the discipline the sibling suites already carry: PASS/FAIL alone
# cannot tell a complete run from a partially-vanished one -- a block dropped by a bad merge leaves
# every remaining assertion green, and "21/21 passed" reads exactly as healthy as "26/26 passed" to
# anyone who does not independently know the real number. A mismatch is a THIRD outcome, distinct
# from a failure, because "this run did not measure everything" and "this code is wrong" are
# different sentences.
# 33, DERIVED BY COUNTING THE CALLS rather than copied from a passing run -- a number taken from the
# output is satisfied by whatever the file happens to contain, which is the one thing this constant
# exists to refuse. Two of the blocks are loops, and that is where a naive count goes wrong:
#
#   4  bad-head loop (not hex, 39 chars, 41 chars, uppercase)      5  bad-pr loop
#   1  40-hex inside a longer string    1  no strays left behind   1  no lane   1  absent commit
#   1  valid exits 0                    1  entry at derived path   4  pr/head/lane/timestamp
#   1  no branch name in the entry      1  second enqueue exits 1  1  first entry unchanged
#   3  the race (one zero, one one, one file)
#   1  status exits 0                   1  reads queued            1  reports the runner's text
#   1  never-queued exits 0             1  says "not queued"       1  list exits 0   1  list count
#   1  a missing directory refuses
$ExpectedAssertionCount = 33

$ErrorActionPreference = 'Stop'
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

function Assert-Equal {
    param($Expected, $Actual, [Parameter(Mandatory)] [string] $Message)
    Assert-True -Condition ($Expected -eq $Actual) -Message "$Message (expected '$Expected', got '$Actual')"
}

$scriptUnderTest = Join-Path $PSScriptRoot 'gate-queue.ps1'
$queue = Join-Path ([System.IO.Path]::GetTempPath()) ("gq-" + [guid]::NewGuid().ToString('N').Substring(0, 8))

# A REAL commit from this repository, resolved once. The validator's fourth exit code is about an
# object that does not exist, so the positive case needs one that does -- and inventing forty hex
# characters would test the wrong thing: it would always be absent, and the case would pass for the
# reason the NEGATIVE case is supposed to prove.
$realHead = (& git rev-parse HEAD).Trim()

# EVERY CASE HERE IS A REFUSAL, so the child writes to stderr on purpose -- and under Windows
# PowerShell 5.1 a native command's stderr comes back wrapped in a NativeCommandError. With the
# file's `$ErrorActionPreference = 'Stop'` that record is TERMINATING: the suite died on its first
# refusal, which is every case it was written to measure. Measured here, and the same note is on the
# Bash-vs-PowerShell rules the repository already carries.
#
# So the preference is lowered for the invocation only, and stderr goes to a FILE rather than to
# `$null` -- a discarded stream still passes through the wrapper on its way to being discarded.
function Invoke-Queue {
    param([string[]] $Arguments)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $errFile = [System.IO.Path]::GetTempFileName()
    try {
        $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptUnderTest @Arguments 2> $errFile
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
        Remove-Item -LiteralPath $errFile -Force -ErrorAction SilentlyContinue
    }
    return [pscustomobject]@{ Code = $code; Output = @($out) }
}

try {
    Write-Host '=== gate-queue: input the boundary must refuse' -ForegroundColor Cyan

    # THE SHA IS THE FIELD MOST WORTH SMUGGLING A SEPARATOR THROUGH, so the anchors are tested from
    # both ends, not just by one bad value.
    foreach ($bad in @('nothex', ($realHead.Substring(0, 39)), ($realHead + 'a'), $realHead.ToUpperInvariant())) {
        $r = Invoke-Queue @('-Command', 'enqueue', '-PullRequest', '902', '-Head', $bad, '-Lane', 'H', '-QueueDirectory', $queue)
        Assert-Equal 3 $r.Code "a head that is not 40 lowercase hex is refused: '$bad'"
    }

    # A FORTY-HEX RUN INSIDE A LONGER STRING is the case an unanchored pattern accepts, and it is the
    # one that would let a path separator ride along. Distinct from the four above.
    $r = Invoke-Queue @('-Command', 'enqueue', '-PullRequest', '902', '-Head', ("x" + $realHead + "/y"), '-Lane', 'H', '-QueueDirectory', $queue)
    Assert-Equal 3 $r.Code 'a 40-hex run inside a longer string is refused, not matched'

    foreach ($bad in @('0', '-1', 'abc', '../../evil', '9 902')) {
        $r = Invoke-Queue @('-Command', 'enqueue', '-PullRequest', $bad, '-Head', $realHead, '-Lane', 'H', '-QueueDirectory', $queue)
        Assert-Equal 3 $r.Code "a pull request number that is not a positive integer is refused: '$bad'"
    }

    # AND THE REFUSAL LEFT NOTHING BEHIND. A validator that refuses after creating the path has not
    # refused; this is the assertion that separates the two.
    $strays = @(Get-ChildItem -LiteralPath $queue -Recurse -File -ErrorAction SilentlyContinue)
    Assert-Equal 0 $strays.Count 'a refused enqueue creates no file anywhere under the queue directory'

    $r = Invoke-Queue @('-Command', 'enqueue', '-PullRequest', '902', '-Head', $realHead, '-QueueDirectory', $queue)
    Assert-Equal 3 $r.Code 'an entry with no lane is refused: an entry nobody owns cannot be chased'

    # An absent object, spelled correctly. Forty hex that is not a commit here.
    $absent = '0' * 40
    $r = Invoke-Queue @('-Command', 'enqueue', '-PullRequest', '902', '-Head', $absent, '-Lane', 'H', '-QueueDirectory', $queue)
    Assert-Equal 4 $r.Code 'a well-formed sha that is not a commit here is refused with its own code, not with 3'

    Write-Host '=== gate-queue: the ordinary path' -ForegroundColor Cyan

    $r = Invoke-Queue @('-Command', 'enqueue', '-PullRequest', '902', '-Head', $realHead, '-Lane', 'H', '-QueueDirectory', $queue)
    Assert-Equal 0 $r.Code 'a valid enqueue exits 0'
    $entry = Join-Path $queue ("902-" + $realHead.Substring(0, 8) + ".json")
    Assert-True (Test-Path -LiteralPath $entry) 'the entry exists at the derived path'

    $written = (Get-Content -LiteralPath $entry -Raw).Trim()
    $parsed = $written | ConvertFrom-Json
    Assert-Equal 902 $parsed.pr 'the entry records the pull request number'
    Assert-Equal $realHead $parsed.head 'the entry records the head'
    Assert-Equal 'H' $parsed.lane 'the entry records the lane'
    Assert-True ($parsed.enqueued_at -cmatch '^\d{4}-\d\d-\d\dT') 'the entry records an ISO timestamp'

    # THE BRANCH NAME IS NOT IN THE FILE, and this is the assertion that keeps it out. The runner
    # resolves the branch through `gh pr view`; a name here would be a name a stale lane controls.
    Assert-True (-not ($written -match 'branch')) 'the entry carries no branch name for the runner to trust'

    Write-Host '=== gate-queue: create-or-fail, not check-then-write' -ForegroundColor Cyan

    $before = (Get-Content -LiteralPath $entry -Raw)
    $r2 = Invoke-Queue @('-Command', 'enqueue', '-PullRequest', '902', '-Head', $realHead, '-Lane', 'OTHER', '-QueueDirectory', $queue)
    Assert-Equal 1 $r2.Code 'a second enqueue of the same (pr, head) exits 1, not 0 and not 2'
    $after = (Get-Content -LiteralPath $entry -Raw)
    Assert-Equal $before $after 'the refused second enqueue did not overwrite the first entry'

    # THE RACE, and it is the reason this script exists rather than a two-line recipe. Two children
    # started as close together as the harness can manage, both claiming the same name: EXACTLY one
    # may exit 0. A check-then-write passes every case above and fails this one.
    #
    # `Start-Job` DOES NOT INHERIT THE WORKING DIRECTORY -- measured here: a job's `Get-Location`
    # answered `C:\Users\<user>\OneDrive\Documentos` while this shell sat in the repository. The
    # child then ran `git cat-file` outside any repository, was refused, and BOTH claimants failed
    # for a reason that had nothing to do with the race. The repository path is passed in and set
    # explicitly, and this comment is the reason the case can be trusted to be about the race at all.
    $racePr = '9021'
    $repoRoot = (& git rev-parse --show-toplevel).Trim()
    $jobs = @(1, 2) | ForEach-Object {
        Start-Job -ScriptBlock {
            param($s, $pr, $head, $q, $root)
            Set-Location -LiteralPath $root
            $ErrorActionPreference = 'Continue'
            $null = & powershell -NoProfile -ExecutionPolicy Bypass -File $s -Command enqueue -PullRequest $pr -Head $head -Lane RACE -QueueDirectory $q 2>$null
            return $LASTEXITCODE
        } -ArgumentList $scriptUnderTest, $racePr, $realHead, $queue, $repoRoot
    }
    $codes = @($jobs | Wait-Job | Receive-Job)
    $jobs | Remove-Job -Force
    $zeros = @($codes | Where-Object { $_ -eq 0 })
    Assert-Equal 1 $zeros.Count 'two simultaneous claimants of one name: exactly one exits 0'
    $ones = @($codes | Where-Object { $_ -eq 1 })
    Assert-Equal 1 $ones.Count 'and the loser exits 1 (already queued), never 2 (write did not land)'
    $raceFiles = @(Get-ChildItem -LiteralPath $queue -Filter "$racePr-*.json" -File)
    Assert-Equal 1 $raceFiles.Count 'the race produced exactly one entry on disk'

    Write-Host '=== gate-queue: status and list' -ForegroundColor Cyan

    $r = Invoke-Queue @('-Command', 'status', '-PullRequest', '902', '-QueueDirectory', $queue)
    Assert-Equal 0 $r.Code 'status of a queued pull request exits 0'
    Assert-True (@($r.Output) -join ' ' -match 'queued') 'status of an entry with no status file reads queued'

    # A STATUS FILE THE RUNNER WOULD WRITE. The lane reads this; it never polls a process table.
    Set-Content -LiteralPath ([System.IO.Path]::ChangeExtension($entry, '.status')) -Value 'building stage 3' -NoNewline
    $r = Invoke-Queue @('-Command', 'status', '-PullRequest', '902', '-QueueDirectory', $queue)
    Assert-True (@($r.Output) -join ' ' -match 'building stage 3') 'status reports what the runner wrote, not a guess'

    $r = Invoke-Queue @('-Command', 'status', '-PullRequest', '777', '-QueueDirectory', $queue)
    Assert-Equal 0 $r.Code 'status of a pull request that was never queued is not an error'
    Assert-True (@($r.Output) -join ' ' -match 'not queued') 'and it says so in words rather than by silence'

    $r = Invoke-Queue @('-Command', 'list', '-QueueDirectory', $queue)
    Assert-Equal 0 $r.Code 'list exits 0'
    Assert-Equal 2 (@($r.Output).Count) 'list returns both entries and nothing else'

    Write-Host '=== gate-queue: a missing queue directory is not an empty queue' -ForegroundColor Cyan

    $missing = Join-Path ([System.IO.Path]::GetTempPath()) ("gq-absent-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
    $r = Invoke-Queue @('-Command', 'list', '-QueueDirectory', $missing)
    Assert-Equal 5 $r.Code 'listing a directory that does not exist refuses rather than reporting an empty queue'

} finally {
    if (Test-Path -LiteralPath $queue) { Remove-Item -LiteralPath $queue -Recurse -Force -ErrorAction SilentlyContinue }
}

Write-Host ''
if ($script:total -ne $ExpectedAssertionCount) {
    # THE THIRD OUTCOME. Not a failure: a run that did not measure what it was written to measure.
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount" -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $($script:total) of $($script:total)" -ForegroundColor Green
exit 0
