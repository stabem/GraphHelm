# #667: a collision-proof manifest name, and a write that refuses to overwrite one.
#
# The old name was the head's first 12 characters and a WHOLE-SECOND UTC stamp, written with
# `WriteAllText`, which overwrites. Two runs sharing a head and a second collided in both stores and
# the second write silently replaced the first.
#
# That is worse than an ordinary overwrite. These manifests are the only artefact that can prove two
# gate runs overlapped in time (#638), and the pairs most likely to collide are the MOST concurrent
# ones in the store -- so the collision destroys exactly the evidence it is itself evidence of, and
# leaves no trace of having done so. Same-head runs are routine here (retries against one head);
# only the same-second half was unlikely, and "unlikely" is not a property a record-keeping artefact
# should rest on.
#
# Sub-second precision alone would not close it: two runs can finish in the same millisecond under
# real parallelism. The suffix is what makes the name collision-proof; the timestamp stays because
# a sortable, human-readable name is worth keeping.

Set-StrictMode -Version Latest

# The marker a caller matches to tell 'nothing survived, retry is safe' from 'a finalised manifest
# is still on disk'. ONE producer: the throw below interpolates it and gate.ps1 matches it, so the
# two cannot drift apart -- the same defect I flagged on #679, where a precondition compared a
# message against a private copy of itself.
$script:FinalisedManifestSurvivedKey = 'GraphHelm.FinalisedManifestSurvived'

# #742: 'take another name', told apart from 'this write failed'. The creation path's outer search
# reads this and retries under a fresh name; a correction has no other name to take and reports it.
# A flag, for the same reason as the marker above: a phrase in a message can be spelled by accident.
$script:ManifestStagingNameTakenKey = 'GraphHelm.ManifestStagingNameTaken'

function Test-IOExceptionIsNameTaken {
    <#
      Was this IOException 'the name is already taken', or something else?

      The runtime answers directly: inside `catch [System.IO.IOException]`, $_.Exception is the
      IOException itself and its HResult is the Win32 code. Measured on this machine --
      CreateNew over an existing file gives 0x80070050 (ERROR_FILE_EXISTS); a missing directory
      gives DirectoryNotFoundException with 0x80070003, which is an IOException too and must NOT
      be read as a collision.

      NOT the message. `"The file '...' already exists."` is a .NET resource string selected by
      CurrentUICulture and rewritable in a servicing update, so matching it is an inference about
      what the runtime MEANT. The HResult is what the runtime SAID.

      NOT Test-Path either, which is what this replaces. Re-observing the filesystem after the
      failure asks a different question at a later instant: between the failed open and the check,
      another run can remove the file and turn a real collision into a rethrown fault, or create
      one and turn a fault into a collision. The exception describes THIS failure; the directory
      describes the world now.

      WHAT IT DOES WITH EVERY OTHER CODE, said because the rest of this comment only says what the
      predicate is NOT. Anything it does not name is FALSE, and the callers rethrow: the run fails
      loudly with the operating system's own error rather than quietly trying another name.

      That is a WHITELIST on purpose, and it is where the boundary lives. A blacklist -- "anything
      that is not ERROR_PATH_NOT_FOUND is a collision" -- agrees with this list on every code the
      cells below happened to name, and differs on exactly one: ERROR_SHARING_VIOLATION
      (0x80070020), an existing-but-unopenable file. There is a cell for it.

      The consequence, declared rather than left as a side effect: a file that exists but cannot be
      opened NO LONGER causes a new name to be taken; it fails the run. The window is narrow --
      CreateNew over an existing file reports ERROR_FILE_EXISTS even when the file is locked -- and
      the argument for it is NOT the one from #669. There, a write that failed after CreateNew had
      to rethrow because retrying under a new name would leave a truncated file behind; a FAILED
      CreateNew leaves nothing at all, so that reasoning does not carry here.

      The argument that does: a non-exists failure is not evidence about the NAME. It is almost
      always about the directory -- permissions, a lock, a vanished path -- and retrying under a
      fresh name would fail the same way three times and then report "the name source is not
      producing distinct names", which points the reader at the wrong subsystem. A narrow list
      makes an environment fault surface as itself.
    #>
    param([AllowNull()] $Exception)

    if ($null -eq $Exception) { return $false }
    # ERROR_FILE_EXISTS, and ERROR_ALREADY_EXISTS which some paths report instead.
    return ($Exception.HResult -eq -2147024816) -or ($Exception.HResult -eq -2147024713)
}

function Test-ManifestRollbackLeftFinalised {
    <#
      The DECISION, extracted so it can be tested even though its ARMING cannot be.

      Arming this needs a delete that fails -- the finalised primary held open without
      delete-sharing, a Windows reader or antivirus -- in the window between the move and the
      rollback. Nothing in the suite can reach that window: the WriteContent seam runs BEFORE the
      moves, so no final file exists yet to hold.

      What CAN be tested is what the caller does with the answer, and that is the half that
      corrupts the record when it is wrong: a fallback that proceeds here writes a SECOND
      complete-looking manifest while RUN-END names one. So the predicate lives here, with the
      marker it matches, and gate.ps1 asks it rather than carrying its own copy of the text.
    #>
    param([AllowNull()] $Exception)

    if ($null -eq $Exception) { return $false }
    if ($null -eq $Exception.Data) { return $false }
    return [bool] $Exception.Data[$script:FinalisedManifestSurvivedKey]
}

function Test-ManifestStagingNameTaken {
    <#
      'Take another name', told apart from 'this write failed' (#742).

      A staging file that already exists is a CONCURRENT RESERVATION: another run passed the same
      precheck and got here first. The creation path answers that by taking a fresh name, which is
      not a thing a correction can do -- it has exactly one name, the one already on disk. So the
      two callers need to read the same condition and act differently on it, and the condition is a
      TYPED flag rather than a phrase: matching text means any message that merely CONTAINS the
      words arms the retry, which is how the sibling marker above was bitten.
    #>
    param([AllowNull()] $Exception)

    if ($null -eq $Exception) { return $false }
    if ($null -eq $Exception.Data) { return $false }
    return [bool] $Exception.Data[$script:ManifestStagingNameTakenKey]
}

function New-GateManifestFileName {
    <#
      `head12-yyyyMMddTHHmmss.fffZ-xxxxxxxx.json` -- sortable and readable as before, plus 8 hex
      characters of randomness. Nothing parses this name (checked across ci/: the only construction
      site was gate.ps1, and classify-run.ps1 treats it as an opaque key shared by both stores), so
      the suffix costs nothing downstream.

      `SuffixSource` exists so a test can be deterministic. Asserting that two calls to a RANDOM
      generator differ proves luck, not formatting -- a legitimate collision (1 in 2^32 per pair)
      would fail the suite for no defect at all. Production keeps the random default; a caller that
      needs to know what it will get says so.
    #>
    param(
        [Parameter(Mandatory)][string] $HeadSha,
        [datetime] $Now = [DateTime]::UtcNow,
        [string] $Suffix,
        [scriptblock] $SuffixSource
    )

    if ([string]::IsNullOrWhiteSpace($Suffix)) {
        $Suffix = if ($SuffixSource) { [string](& $SuffixSource) } else { [guid]::NewGuid().ToString('n').Substring(0, 8) }
    }
    $head = if ($HeadSha.Length -ge 12) { $HeadSha.Substring(0, 12) } else { $HeadSha }
    # InvariantCulture, not the ambient one. `ToString` formats the YEAR with the current culture's
    # calendar, so under th-TH a 2026 run is stamped 2569 -- the filename stops being the documented
    # Gregorian UTC stamp, sorting breaks against every earlier manifest, and the fixed-year cells
    # fail for a reason that has nothing to do with this code. A name is an identifier; it must not
    # depend on where the machine thinks it is.
    return "$head-$($Now.ToString('yyyyMMddTHHmmss.fffZ', [cultureinfo]::InvariantCulture))-$Suffix.json"
}

function Invoke-TransientFileRetry {
    <#
      One file operation, retried while the failure is one waiting can fix (#976).

      WHY THIS EXISTS AND WHY IT IS HERE RATHER THAN AROUND THE CALLER. `File.Replace` fails on
      Windows whenever anything holds the destination for an instant -- a concurrent gate, the
      indexer, a scanner. This file already records that as measured: "under five concurrent gates,
      File.Replace -> Unable to remove the file to be replaced". One attempt turned that instant
      into a RED run, and the reproduction always failed because reproducing it means running ONE
      gate on an idle machine, which is the one condition under which it cannot happen (seven
      isolated attempts across two lanes, #947).

      THE LEVEL IS THE POINT. A retry around the WHOLE pair commit was tried first and is wrong:
      that operation is several files as one unit, its guarantee is both-or-neither, and a second
      attempt runs against a world the first already touched -- measured, it left one copy rewritten
      and the other not, which is the half-pair this file exists to prevent (2 of 60 cells red).
      A SINGLE `Replace` or `Move` is atomic: it happened or it did not, so from the loop's point of
      view a retried attempt merely took longer, and the caller's rollback is untouched.

      AND ONLY THE TRANSIENT CLASS. `FileNotFoundException` and `DirectoryNotFoundException` derive
      from `IOException`, and waiting does not make a missing file appear -- retrying those would
      delay a correct failure by the whole budget and say nothing new. Sharing violations and access
      denials are the ones a lock clears.
    #>
    param(
        [Parameter(Mandatory)][scriptblock] $Operation,
        [int] $Attempts = 3,
        [int] $RetryDelayMilliseconds = 150
    )

    $total = [Math]::Max(1, $Attempts)
    for ($attempt = 1; $attempt -le $total; $attempt++) {
        try {
            & $Operation
            return
        } catch {
            # UNWRAPPED FIRST, AND THIS IS THE WHOLE THING (#984's review). A static .NET call that
            # throws inside PowerShell does not surface its own exception: the engine wraps it in a
            # `MethodInvocationException` and puts the real one in `InnerException`. Measured on a
            # real failing `File.Move` (destination exists):
            #
            #   caught type           System.Management.Automation.MethodInvocationException
            #   is IOException?       False                   <- the classification the cells relied on
            #   inner type            System.IO.IOException
            #
            # So testing `$_.Exception` alone made `$transient` false for EVERY real file operation,
            # and the retry rethrew immediately on exactly the momentary lock it exists to absorb.
            # The first cells missed it because they `throw` raw exception instances from a
            # scriptblock, which the engine does not wrap -- a fixture that never crossed the
            # boundary it was written to cover.
            $transient = $false
            $exception = $_.Exception
            while ($exception) {
                if ((($exception -is [System.IO.IOException]) -and
                     -not ($exception -is [System.IO.FileNotFoundException]) -and
                     -not ($exception -is [System.IO.DirectoryNotFoundException])) -or
                    ($exception -is [System.UnauthorizedAccessException])) {
                    $transient = $true
                    break
                }
                $exception = $exception.InnerException
            }
            if (-not $transient -or $attempt -eq $total) { throw }
            if ($RetryDelayMilliseconds -gt 0) { Start-Sleep -Milliseconds $RetryDelayMilliseconds }
        }
    }
}

function Write-ManifestPairContent {
    <#
      ONE run's content reaches BOTH stores, or neither. The half-pair invariant lives HERE and
      nowhere else (#742).

      It used to live in two places: this file's `Write-GateManifestPair` and, hand-copied, the
      manifest CORRECTION path in `gate.ps1`. The correction cannot call `Write-GateManifestPair`
      -- that reserves a NEW name on every call, so routing a correction through it would write a
      SECOND pair and leave the stale one exactly where readers look. So the correction borrowed the
      discipline instead, and two implementations of one invariant drifted, silently, as the issue
      predicted they would.

      THE SPLIT THAT FIXES IT is between the NAME and the WRITE. Reserving a free name is the
      creation path's own concern; writing given content to given paths atomically is shared. So
      this function takes the final PATHS as an input and knows nothing about how they were chosen:

        new run    -> Write-GateManifestPair reserves a fresh name, then calls this with -Commit Create
        correction -> gate.ps1 passes the names it already holds, then calls this with -Commit Replace

      WHY THE TWO COMMIT PRIMITIVES ARE NOT MERGED. They are not two spellings of one operation:
      creation requires the destination to be ABSENT (`File.Move` refuses to overwrite), correction
      requires it to be PRESENT (`File.Replace` is a rename over an existing file). Merging them
      would mean one of the two callers losing a precondition it depends on. What is shared is
      everything AROUND the primitive -- staging, the flush fault, both-or-neither, the rollback --
      and that is what lives here.
    #>
    param(
        [Parameter(Mandatory)][string[]] $FinalPaths,
        [Parameter(Mandatory)][AllowEmptyString()][string] $Json,
        [ValidateSet('Create', 'Replace')][string] $Commit = 'Create',
        [scriptblock] $WriteContent,
        # The seam a cell needs to arm a PARTIAL commit. It runs after every staging file exists and
        # before the first commit -- the only window in which the first commit can succeed and the
        # second fail, which is the state the rollback exists for. Production never passes it.
        [scriptblock] $BeforeCommit,
        # #976: how many times a SINGLE file operation may be attempted, and the pause between.
        # Cells set the delay to 0 so a suite does not sleep; production keeps a pause long enough
        # for a scanner or an indexer to let go of the destination.
        [int] $Attempts = 3,
        [int] $RetryDelayMilliseconds = 150
    )

    $encoding = New-Object System.Text.UTF8Encoding($false)
    # `.tmp` for a creation, `.correcting` for a correction. Both are names no manifest reader
    # treats as a run, which is the property that matters; they are kept distinct so a leftover
    # says which path stranded it.
    $stagingSuffix = if ($Commit -eq 'Create') { '.tmp' } else { '.correcting' }

    $staged = @()
    $nameLost = $false
    try {
        foreach ($final in $FinalPaths) {
            $temp = "$final$stagingSuffix"

            # An existing staging file is a CONCURRENT RESERVATION, not a fault. Another run passed
            # the same precheck and got here first; its file is not finished, so the `.json` test
            # could not see it. Falling into the generic cleanup below would delete THAT run's
            # reservation on the way out and both runs would lose their manifest -- the
            # mutual-destruction version of the overwrite this helper exists to stop.
            try {
                $stream = [System.IO.File]::Open($temp, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
            } catch [System.IO.IOException] {
                if (Test-IOExceptionIsNameTaken -Exception $_.Exception) { $nameLost = $true; break }
                throw
            }

            # Tracked the moment it EXISTS, not once it is written. Recording it after the write
            # meant a failed write left the file untracked, so the cleanup below deleted nothing.
            $staged += $temp

            $disposed = $false
            try {
                if ($WriteContent) { & $WriteContent $stream } else {
                    $bytes = $encoding.GetBytes($Json)
                    $stream.Write($bytes, 0, $bytes.Length)
                }
                # Disposed HERE, inside the try, and its failure is a real failure. A filesystem can
                # report a delayed write or flush fault only at Dispose -- swallowing it would
                # promote possibly truncated bytes to a final record. `File.WriteAllText`, which the
                # correction path used before this extraction, cannot make that distinction at all.
                $stream.Dispose()
                $disposed = $true
            } finally {
                # Only for the unwinding path: the original error has already won, and a second
                # complaint from Dispose must not replace it.
                if (-not $disposed) { try { $stream.Dispose() } catch { } }
            }
        }
    } catch {
        $original = $_
        # ONLY what this invocation created.
        $undeleted = @()
        foreach ($temp in $staged) {
            try { [System.IO.File]::Delete($temp) } catch { $undeleted += $temp }
        }
        if ($undeleted.Count -gt 0) {
            throw "$($original.Exception.Message) -- and the partial file(s) could not be removed: $($undeleted -join ', '). They carry a staging suffix so no reader will treat them as manifests, but they are litter"
        }
        throw $original
    }

    if ($nameLost) {
        foreach ($temp in $staged) { try { [System.IO.File]::Delete($temp) } catch { } }
        # TYPED, so the caller can tell "take another name" from "this write failed". The creation
        # path's outer search reads this flag and tries a fresh name; a correction has no other name
        # to take and reports it.
        $lost = New-Object System.Exception('a concurrent writer holds the staging name for this manifest')
        $lost.Data[$script:ManifestStagingNameTakenKey] = $true
        throw $lost
    }

    if ($BeforeCommit) { & $BeforeCommit $staged }

    # BOTH OR NEITHER. If the first commit lands and the second fails -- the destination appearing
    # concurrently, a sharing violation, any IO fault -- stopping here would leave HALF A PAIR, and
    # half a pair is worse than none: it reads as a complete record.
    $committed = @()
    $backups = @()
    try {
        for ($i = 0; $i -lt $staged.Count; $i++) {
            if ($Commit -eq 'Create') {
                $source = $staged[$i]; $destination = $FinalPaths[$i]
                Invoke-TransientFileRetry -Attempts $Attempts -RetryDelayMilliseconds $RetryDelayMilliseconds `
                    -Operation { [System.IO.File]::Move($source, $destination) }
            } else {
                # REPLACE, NOT COPY. `File.Copy(src, dst, overwrite)` writes THROUGH the destination:
                # a kill halfway leaves a truncated file where a VALID record was, which is the one
                # direction worse than not correcting at all. `File.Replace` is a rename over an
                # existing file on the same volume -- both conditions hold here.
                #
                # Measured on this runtime, because the obvious alternative does not exist here:
                #   File.Replace(3-arg)  : True
                #   File.Move(overwrite) : False   (.NET Core 3.0+ only)
                #
                # THE BACKUP ARGUMENT IS THE ROLLBACK (#742). This call site used to pass a real
                # null here, and that one argument is why a correction could not be undone: with no
                # backup the destination's original bytes are GONE the instant the replace lands, so
                # a later failure in the pair had nothing to restore from and the two stores were
                # left disagreeing. Measured on this runtime:
                #
                #   Replace(src, dest, null)   -> dest='CORRECTED', the directory holds ONLY dest
                #   Replace(src, dest, backup) -> dest='CORRECTED', backup='ORIGINAL'
                #
                # Full paths for the same reason as everywhere else in this file.
                $backup = "$($FinalPaths[$i]).backup"
                $source = [System.IO.Path]::GetFullPath($staged[$i])
                $destination = [System.IO.Path]::GetFullPath($FinalPaths[$i])
                $backupFull = [System.IO.Path]::GetFullPath($backup)
                Invoke-TransientFileRetry -Attempts $Attempts -RetryDelayMilliseconds $RetryDelayMilliseconds `
                    -Operation { [System.IO.File]::Replace($source, $destination, $backupFull) }
                $backups += $backup
            }
            $committed += $FinalPaths[$i]
        }
    } catch {
        $original = $_
        $stuck = @()
        if ($Commit -eq 'Create') {
            # Roll the finalised ones back out of sight. They are files this invocation created
            # moments ago, so deleting them destroys nothing anyone else could be holding.
            foreach ($final in $committed) {
                try { [System.IO.File]::Delete($final) } catch { $stuck += $final }
            }
        } else {
            # RESTORE FROM THE BACKUP, newest first. A replace commit is undone by replacing the
            # destination BACK with the bytes the backup holds -- `File.Replace(backup, final, null)`
            # is a rename over the same volume, so each restore is atomic and consumes its backup.
            #
            # A backup that is MISSING is counted as stuck rather than ignored. It means the commit
            # landed and its backup did not survive, which leaves a corrected copy this function
            # cannot undo -- the exact state the caller must be told about, and silence here would
            # report a clean rollback over a half-corrected pair.
            for ($k = $committed.Count - 1; $k -ge 0; $k--) {
                $backup = "$($committed[$k]).backup"
                if (-not (Test-Path -LiteralPath $backup)) { $stuck += $committed[$k]; continue }
                try {
                    # RETRIED FOR THE SAME REASON AND MORE URGENTLY (#976): a rollback defeated by a
                    # momentary lock leaves exactly the half-corrected pair this function exists to
                    # prevent, and unlike the commit it has nothing left to fall back to.
                    $backupFull = [System.IO.Path]::GetFullPath($backup)
                    $finalFull = [System.IO.Path]::GetFullPath($committed[$k])
                    Invoke-TransientFileRetry -Attempts $Attempts -RetryDelayMilliseconds $RetryDelayMilliseconds `
                        -Operation { [System.IO.File]::Replace($backupFull, $finalFull, [NullString]::Value) }
                } catch { $stuck += $committed[$k] }
            }
        }
        # Whatever the outcome, no backup is left as litter: a `.backup` beside a manifest is not a
        # manifest, but it is a file a later reader has to explain.
        foreach ($backup in $backups) {
            if (Test-Path -LiteralPath $backup) { try { [System.IO.File]::Delete($backup) } catch { } }
        }
        foreach ($temp in $staged) {
            if (Test-Path -LiteralPath $temp) {
                try { [System.IO.File]::Delete($temp) } catch { }
            }
        }
        if ($stuck.Count -gt 0) {
            # TYPED, not a phrase in a sentence. The caller has to tell this apart from every
            # ordinary IO failure, and matching text meant any message that merely CONTAINED the
            # marker -- a slot directory named after it, a path echoed back by the OS -- armed the
            # refusal and turned an ordinary durable hiccup into a red gate. A flag on the exception
            # cannot be spelled by accident.
            $failure = New-Object System.Exception("$($original.Exception.Message) -- a finalised manifest could not be rolled back: $($stuck -join ', '). A reader will treat it as a complete run that never reached RUN-END, and writing another would make two")
            $failure.Data[$script:FinalisedManifestSurvivedKey] = $true
            throw $failure
        }
        throw $original
    }

    # The pair is committed, so the backups have served their purpose. Removed here rather than
    # left for the caller: they exist only for the window this function owns.
    foreach ($backup in $backups) {
        if (Test-Path -LiteralPath $backup) { try { [System.IO.File]::Delete($backup) } catch { } }
    }

    return $committed
}

function Sync-ManifestCopies {
    <#
      Make every mutable copy of a manifest agree with the bytes that were PUBLISHED, or leave
      them all as they were (#938).

      This is the THIRD writer of the half-pair discipline. #742 named two -- `Write-GateManifestPair`
      and the correction path -- and moved both onto `Write-ManifestPairContent`. Reconciliation was
      the one nobody had counted, and it carried the same hand-copied shape with the same three
      weaknesses plus one of its own:

        `WriteAllText`          an existing `.reconciling` is clobbered rather than recognised as a
                                concurrent reservation, and a delayed flush fault reported only at
                                Dispose is invisible.
        null backup             `File.Replace` was passed a real null, so a partial rewrite had
                                nothing to restore from.
        per-path loop           it rewrote each copy as it went, so a failure on the second left the
                                first rewritten -- half a pair, the exact state the pair writer
                                exists to refuse, in the code whose own comment says so.
        the temp LEAKED         and this one was unique to here: the `catch` set a message and
                                returned, and nothing deleted the staging file. Every transient
                                failure left a `<manifest>.json.reconciling` beside the manifest,
                                permanently. Measured on this machine 2026-09-06, under five
                                concurrent gates: `File.Replace` -> "Unable to remove the file to be
                                replaced".

      DECIDE FIRST, WRITE ONCE. The subset that disagrees is computed before anything is written,
      then handed to `Write-ManifestPairContent` as ONE unit, which is what makes both-or-neither
      reachable at all: a loop that writes as it walks cannot roll back what it already committed.

      AND ABSENT IS NOT THE SAME AS DIFFERENT. `ReadAllText` failing used to land a MISSING copy in
      the same bucket as one whose bytes differ, and `File.Replace` cannot create a destination --
      so a vanished twin reported `could not be rewritten: FileNotFound`, a message about the wrong
      thing. Missing copies are partitioned out and reported in their own words.
    #>
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][string[]] $Paths,
        [Parameter(Mandatory)][AllowEmptyString()][string] $Content,
        # Passed through to the writer so a cell can arm a partial commit. Production never sets it.
        [scriptblock] $BeforeCommit
    )

    # Deduplicated by RESOLVED path, for the reason `Write-GateManifestPair` records: two spellings
    # can be one file, and reconciling it twice would read the second pass's own staging file.
    $unique = @()
    foreach ($candidate in $Paths) {
        if ([string]::IsNullOrWhiteSpace($candidate)) { continue }
        $full = [System.IO.Path]::GetFullPath($candidate)
        if (-not ($unique | Where-Object { $_ -eq $full })) { $unique += $full }
    }

    $missing = @()
    $disagreeing = @()
    foreach ($path in $unique) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { $missing += $path; continue }
        $onDisk = $null
        try { $onDisk = [System.IO.File]::ReadAllText($path) } catch { $onDisk = $null }
        # Ordinal, like every other equality in this file: a manifest is bytes, and a
        # culture-sensitive comparison can call two different records equal (#753).
        if (-not [string]::Equals([string]$onDisk, [string]$Content, [System.StringComparison]::Ordinal)) {
            $disagreeing += $path
        }
    }

    $problems = @()
    $reconciled = @()
    if ($disagreeing.Count -gt 0) {
        try {
            Write-ManifestPairContent -FinalPaths $disagreeing -Json $Content -Commit 'Replace' `
                -BeforeCommit $BeforeCommit | Out-Null
            $reconciled = $disagreeing
        } catch {
            $problems += "a manifest copy does not match what was published and could not be rewritten: $($_.Exception.Message)"
        }
    }
    # REPORTED EVEN WHEN THE REWRITE SUCCEEDED. A run whose twin has vanished has half a record on
    # disk however well the other half was repaired, and a caller that only heard about the rewrite
    # would call that success.
    if ($missing.Count -gt 0) {
        $problems += "a manifest copy that should exist is not on disk, so it cannot be reconciled: $($missing -join ', ')"
    }

    return [pscustomobject]@{
        Reconciled = @($reconciled)
        Missing    = @($missing)
        Failure    = $(if ($problems.Count -gt 0) { $problems -join ' -- and ' } else { $null })
    }
}

function Write-GateManifestPair {
    <#
      Writes ONE run to BOTH stores under ONE name, or writes neither.

      Three defects drove this shape, and each one alone would have kept the old design:

      1. A partial file must never carry a manifest name. Content goes to a `.tmp` sibling first
         and is MOVED into place only once it is complete, so a failed write cannot leave something
         that later readers -- the #638 glob, classify-run.ps1 -- will treat as a run. Deletion of a
         leftover `.tmp` is genuinely best-effort, because a `.tmp` is not a manifest; deletion of a
         half-written `.json` never was.

      2. The name must be free in BOTH stores before EITHER is finalised. Renaming only in the
         second store orphaned the copy; warning and carrying on left the FIRST run's manifest
         sitting exactly where the second run's twin was expected, and classify-run.ps1:267-273
         identifies a twin by basename alone -- so the second run's classification later overwrote
         the first run's record. Reserving the name in both places first is what makes "same name
         in both stores" an invariant rather than a hope.

      3. Failure to clean up must be reported, not swallowed. A `.tmp` that cannot be removed is
         noise; but if a MOVE half-succeeds the caller has to hear about it.
    #>
    param(
        [Parameter(Mandatory)][string] $PrimaryDirectory,
        [Parameter(Mandatory)][AllowEmptyString()][string] $Json,
        [Parameter(Mandatory)][string] $HeadSha,
        [string] $SecondaryDirectory,
        [scriptblock] $SuffixSource,
        [scriptblock] $WriteContent,
        # Pinned only by tests, and it has to exist: with a live clock the timestamp differs on
        # every attempt, so two calls never produce the same name and a collision cannot be staged.
        # A cell that cannot arrange the condition it names is a cell that proves nothing.
        [datetime] $Now
    )

    # DEDUPLICATED, and this one is self-inflicted: if both stores resolve to one directory --
    # GRAPHHELM_SLOT_DIR pointing at the repository, a trailing separator, a `.` segment -- the
    # first pass creates `<name>.json.tmp` and the second pass reads THAT SAME FILE as a concurrent
    # reservation. The run then abandons its own name, three times, and every otherwise-valid gate
    # ends with no manifest. My own concurrency guard, biting the run that armed it.
    #
    # Text alone could not settle this in #638 -- two spellings can be one directory and the
    # manifest names no host. Here the filesystem IS present, so GetFullPath resolves the aliases
    # that matter (separator, `.`, `..`, case on Windows) instead of guessing from the string.
    $directories = @()
    foreach ($candidate in @($PrimaryDirectory, $SecondaryDirectory)) {
        if ([string]::IsNullOrWhiteSpace($candidate)) { continue }
        $full = [System.IO.Path]::GetFullPath($candidate).TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
        if (-not ($directories | Where-Object { $_ -eq $full })) { $directories += $full }
    }

    foreach ($pass in 1, 2, 3) {
        $name = if ($PSBoundParameters.ContainsKey('Now')) {
            New-GateManifestFileName -HeadSha $HeadSha -SuffixSource $SuffixSource -Now $Now
        } else {
            New-GateManifestFileName -HeadSha $HeadSha -SuffixSource $SuffixSource
        }
        $taken = $false
        foreach ($directory in $directories) {
            if (Test-Path -LiteralPath ([System.IO.Path]::Combine($directory, $name))) { $taken = $true; break }
        }
        if ($taken) { continue }

        $finalPaths = @()
        foreach ($directory in $directories) { $finalPaths += [System.IO.Path]::Combine($directory, $name) }

        # THE NAME IS THIS FUNCTION'S CONCERN; THE WRITE IS NOT (#742). Everything below the name --
        # staging, the flush fault, both-or-neither, the rollback -- is Write-ManifestPairContent's,
        # and the manifest CORRECTION path in gate.ps1 calls the same function with -Commit Replace.
        # That is the whole point of the split: the half-pair invariant has one implementation, so
        # the two callers cannot drift apart the way they had.
        try {
            return Write-ManifestPairContent -FinalPaths $finalPaths -Json $Json -Commit 'Create' -WriteContent $WriteContent
        } catch {
            # A staging name lost to a concurrent writer is not a failure of this run: it means take
            # ANOTHER name, which is a thing only this function can do. Read as a typed flag rather
            # than matched as text, for the reason the marker above records.
            if (Test-ManifestStagingNameTaken -Exception $_.Exception) { continue }
            throw
        }
    }

    throw "could not find a manifest name free in every store after three attempts (head $HeadSha); the name source is not producing distinct names, and a manifest would have been overwritten"
}
