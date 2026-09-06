<#
.SYNOPSIS
    Enqueue a pull request for the gate runner, and read back what happened to it (#902).

.DESCRIPTION
    A lane enqueues and returns to its next issue. It does not launch a gate, does not claim a slot,
    and does not poll a process table -- the runner owns the slots and this file is the only thing
    the lane writes.

    THE QUEUE FILE IS UNTRUSTED INPUT and this script is the boundary that says so. Everything it
    writes is validated here; everything the runner later reads it validates AGAIN, because a file on
    disk carries no evidence of which program wrote it. In particular the branch NAME never travels
    through the queue: the runner resolves the pull request's branch through `gh pr view` at drain
    time. A name in a file is a name a stale lane (or an attacker) controls; a pull request number is
    a key the server resolves.

    THE ENTRY IS A CREATE-OR-FAIL AND THEN A READ-BACK, never "if absent, write". Two steps with a
    window between them is the race the primitive exists to remove -- the shape that was measured
    writing itself into `.factory/tools/slot-claim.sh` before #620 removed it there. The read-back is
    not belt-and-braces: `[System.IO.File]::Open(..., CreateNew)` not throwing proves THIS PROCESS
    created the name, and the read-back proves the BYTES on disk are the ones intended. Those are
    different claims, and only the second is the one a reader depends on.

    EXIT CODES ARE THE CONTRACT, and the caller is told to branch on ZERO, never on a list:

        0  the command did what it says
        1  already queued -- an entry for this (pr, head) exists; NOT an error, and NOT a claim
        2  the write did not land: created and read back different, or vanished
        3  the input is not usable (pr not a positive integer, head not 40 lowercase hex, no lane)
        4  the head is not a commit in this repository
        5  the queue directory could not be created or read

    A reader who learns "1 means busy" from a list will read 3 as free. That is not hypothetical:
    #889 measured it on a sibling script whose exit 3 carried the message "Nobody holds the slot" --
    a path failure whose text described the world. So the rule for callers is one sentence, and it
    survives a sixth code being added later: ACT ONLY ON EXIT 0. Every other code means this command
    did not do what you asked, and the reason is on stderr for a human, not for a branch.

.PARAMETER Command
    enqueue | status | list

.PARAMETER QueueDirectory
    Defaulted in the BODY, not in the param block: under Windows PowerShell 5.1 `$PSScriptRoot` is
    still empty while parameter defaults are bound (measured; `ci/run-ps-suites.ps1` carries the same
    note), so a computed default here binds to an empty string and fails at call time.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidateSet('enqueue', 'status', 'list')] [string] $Command,
    [string] $PullRequest,
    [string] $Head,
    [string] $Lane,
    [string] $QueueDirectory
)

$ErrorActionPreference = 'Stop'

if (-not $QueueDirectory) { $QueueDirectory = 'D:\graphhelm-slot\queue' }

function Write-Refusal {
    param([Parameter(Mandatory)] [string] $Message)
    # To the error stream, so a caller capturing stdout for machine reading does not get prose in it.
    [Console]::Error.WriteLine("[gate-queue] $Message")
}

# A pull request number is a KEY, not a string to interpolate. Anything that is not a positive
# decimal integer is refused before it reaches a path or a `gh` invocation.
function Test-PullRequestNumber {
    param([string] $Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return $false }
    if ($Value -notmatch '^[1-9][0-9]{0,8}$') { return $false }
    return $true
}

# LOWERCASE hex, exactly forty, ANCHORED. Not a bare forty-hex pattern, which matches a run inside a
# longer string: the sha is the field most worth smuggling a path separator through, and the anchors
# are the whole defence. `-cmatch` because `-match` is case-insensitive and the store is lowercase.
function Test-HeadSha {
    param([string] $Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return $false }
    return ($Value -cmatch '^[0-9a-f]{40}$')
}

# The entry NAME is derived here from validated fields and never taken from input, so no caller can
# steer the path even if a later change loosens a validator.
function Get-EntryPath {
    param([string] $Directory, [string] $Number, [string] $Sha)
    return (Join-Path $Directory ("{0}-{1}.json" -f $Number, $Sha.Substring(0, 8)))
}

switch ($Command) {

    'enqueue' {
        if (-not (Test-PullRequestNumber $PullRequest)) {
            Write-Refusal "-PullRequest must be a positive integer; got '$PullRequest'"
            exit 3
        }
        if (-not (Test-HeadSha $Head)) {
            Write-Refusal "-Head must be 40 lowercase hex characters; got '$Head'"
            exit 3
        }
        if ([string]::IsNullOrWhiteSpace($Lane)) {
            Write-Refusal '-Lane is required: an entry nobody owns cannot be chased when it stalls'
            exit 3
        }

        # THE SHA MUST BE A COMMIT HERE. This is the cheap half; the runner repeats it against
        # `origin` at drain time, because between enqueue and drain a branch can be force-pushed and
        # the sha this lane meant can stop existing on the server while still existing locally.
        #
        # NOTHING THAT CAN STOP THE PIPELINE MAY SIT BETWEEN A NATIVE COMMAND AND THE READ OF ITS
        # EXIT CODE (#762, the same rule `ci/gate.ps1` carries at its startup snapshot). Under
        # Windows PowerShell 5.1 a native command's stderr comes back wrapped in a NativeCommandError,
        # and with `$ErrorActionPreference = 'Stop'` that record is TERMINATING -- so `git` refusing
        # an absent object killed this script before it could answer 4, and the caller saw the
        # interpreter's own exit 1 instead. Measured by this script's own suite, which is why the
        # suite runs the script as a child process rather than dot-sourcing it: an `exit` and a
        # terminating error are indistinguishable from inside.
        $probeCode = $null
        $previousPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $null = & git cat-file -e "$Head^{commit}" 2>$null
            $probeCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $previousPreference
        }
        if ($probeCode -ne 0) {
            Write-Refusal "the head $Head is not a commit in this repository"
            exit 4
        }

        try {
            if (-not (Test-Path -LiteralPath $QueueDirectory)) {
                $null = New-Item -ItemType Directory -Path $QueueDirectory -Force
            }
        } catch {
            Write-Refusal "the queue directory could not be created: $($_.Exception.Message)"
            exit 5
        }

        $entry = Get-EntryPath -Directory $QueueDirectory -Number $PullRequest -Sha $Head
        $payload = [ordered]@{
            pr          = [int] $PullRequest
            head        = $Head
            lane        = $Lane
            enqueued_at = (Get-Date).ToUniversalTime().ToString('o')
        }
        $json = ($payload | ConvertTo-Json -Compress)

        # CREATE-OR-FAIL. The kernel refuses every claimant but one; there is no instant between a
        # check and a write for a second lane to fit into, because there is no check.
        $stream = $null
        try {
            $stream = [System.IO.File]::Open(
                $entry,
                [System.IO.FileMode]::CreateNew,
                [System.IO.FileAccess]::Write,
                [System.IO.FileShare]::None)
            $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
            $stream.Write($bytes, 0, $bytes.Length)
        } catch [System.IO.IOException] {
            # An existing name is the ordinary case: this (pr, head) is already queued. It is not an
            # error and it is not a claim, and the caller must not read it as either.
            if (Test-Path -LiteralPath $entry) {
                Write-Refusal "already queued: $entry"
                exit 1
            }
            Write-Refusal "the entry could not be written: $($_.Exception.Message)"
            exit 2
        } catch {
            Write-Refusal "the entry could not be written: $($_.Exception.Message)"
            exit 2
        } finally {
            if ($stream) { $stream.Dispose() }
        }

        # READ BACK. Creating the name and the bytes being right are different facts, and only the
        # second is what the runner will act on.
        $onDisk = $null
        try { $onDisk = (Get-Content -LiteralPath $entry -Raw -ErrorAction Stop).Trim() } catch { }
        if ($onDisk -ne $json) {
            Write-Refusal 'the entry was created but reads back different; not claiming it'
            exit 2
        }

        Write-Output $entry
        exit 0
    }

    'status' {
        if (-not (Test-PullRequestNumber $PullRequest)) {
            Write-Refusal "-PullRequest must be a positive integer; got '$PullRequest'"
            exit 3
        }
        if (-not (Test-Path -LiteralPath $QueueDirectory)) {
            Write-Refusal "no queue directory at $QueueDirectory"
            exit 5
        }
        # `@()` around anything that can be a single item: under 5.1 a lone match comes back as a
        # scalar, a scalar has no `.Count`, and `$a + $b` on two scalars is ADDITION rather than
        # concatenation -- measured 2026-09-05, when two process ids summed into a third that named
        # no process and the union of two sets read as one element.
        $found = @(Get-ChildItem -LiteralPath $QueueDirectory -Filter "$PullRequest-*.json" -File -ErrorAction SilentlyContinue)
        if ($found.Count -eq 0) {
            Write-Output 'not queued'
            exit 0
        }
        foreach ($f in $found) {
            $statusFile = [System.IO.Path]::ChangeExtension($f.FullName, '.status')
            $state = if (Test-Path -LiteralPath $statusFile) {
                (Get-Content -LiteralPath $statusFile -Raw).Trim()
            } else {
                'queued'
            }
            Write-Output ('{0} {1}' -f $f.Name, $state)
        }
        exit 0
    }

    'list' {
        if (-not (Test-Path -LiteralPath $QueueDirectory)) {
            Write-Refusal "no queue directory at $QueueDirectory"
            exit 5
        }
        $entries = @(Get-ChildItem -LiteralPath $QueueDirectory -Filter '*.json' -File -ErrorAction SilentlyContinue |
                Sort-Object -Property CreationTimeUtc)
        foreach ($e in $entries) { Write-Output $e.Name }
        exit 0
    }
}
