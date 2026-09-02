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

    $encoding = New-Object System.Text.UTF8Encoding($false)

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

        $temps = @()
        $nameLost = $false
        try {
            foreach ($directory in $directories) {
                $temp = [System.IO.Path]::Combine($directory, "$name.tmp")

                # An existing .tmp is a CONCURRENT RESERVATION, not a fault. Another run passed the
                # same `.json` precheck and got here first; its file is not finished, so the `.json`
                # test could not see it. Falling into the generic cleanup below would have deleted
                # THAT run's reservation on the way out, and both runs would have lost their
                # manifest -- the mutual-destruction version of the overwrite this helper exists to
                # stop. So: give up this name, keep only what this invocation created, take a fresh
                # name.
                try {
                    $stream = [System.IO.File]::Open($temp, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
                } catch [System.IO.IOException] {
                    if (Test-IOExceptionIsNameTaken -Exception $_.Exception) { $nameLost = $true; break }
                    throw
                }

                # Tracked the moment it EXISTS, not once it is written. Recording it after the write
                # meant a failed write left the file untracked, so the cleanup below deleted nothing
                # and the .tmp survived -- the cleanup owning less than the code had created.
                $temps += $temp

                $disposed = $false
                try {
                    if ($WriteContent) { & $WriteContent $stream } else {
                        $bytes = $encoding.GetBytes($Json)
                        $stream.Write($bytes, 0, $bytes.Length)
                    }
                    # Disposed HERE, inside the try, and its failure is a real failure. A filesystem
                    # can report a delayed write or flush fault only at Dispose -- swallowing it and
                    # moving on would promote possibly truncated bytes to a final `.json`, which is
                    # precisely the invalid manifest staging exists to prevent.
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
            # ONLY what this invocation created. The previous version rescanned every directory,
            # which is how it could delete a concurrent run's reservation.
            $undeleted = @()
            foreach ($temp in $temps) {
                try { [System.IO.File]::Delete($temp) } catch { $undeleted += $temp }
            }
            if ($undeleted.Count -gt 0) {
                throw "$($original.Exception.Message) -- and the partial file(s) could not be removed: $($undeleted -join ', '). They carry a .tmp suffix so no reader will treat them as manifests, but they are litter"
            }
            throw $original
        }

        if ($nameLost) {
            foreach ($temp in $temps) { try { [System.IO.File]::Delete($temp) } catch { } }
            continue
        }

        # BOTH OR NEITHER, and the moves are where that invariant was still a promise. If the
        # primary move lands and the secondary fails -- the destination appearing concurrently, a
        # sharing violation, any IO fault -- the old code threw with the primary `.json` already
        # VISIBLE to every manifest reader, recording a run that never reaches RUN-END. Half a pair
        # is worse than none: it reads as a complete record.
        $moved = @()
        try {
            foreach ($temp in $temps) {
                $final = $temp.Substring(0, $temp.Length - 4)
                [System.IO.File]::Move($temp, $final)
                $moved += $final
            }
        } catch {
            $original = $_
            $stuck = @()
            # Roll the finalised ones back out of sight. They are files this invocation created
            # moments ago, so deleting them destroys nothing anyone else could be holding.
            foreach ($final in $moved) {
                try { [System.IO.File]::Delete($final) } catch { $stuck += $final }
            }
            foreach ($temp in $temps) {
                if (Test-Path -LiteralPath $temp) {
                    try { [System.IO.File]::Delete($temp) } catch { }
                }
            }
            if ($stuck.Count -gt 0) {
                # TYPED, not a phrase in a sentence. The caller has to tell this apart from every
                # ordinary IO failure, and matching text meant any message that merely CONTAINED
                # the marker -- a slot directory named after it, a path echoed back by the OS --
                # armed the refusal and turned an ordinary durable hiccup into a red gate. That is
                # the availability defect returning through the door of the fix for the integrity
                # one. A flag on the exception cannot be spelled by accident.
                $failure = New-Object System.Exception("$($original.Exception.Message) -- a finalised manifest could not be rolled back: $($stuck -join ', '). A reader will treat it as a complete run that never reached RUN-END, and writing another would make two")
                $failure.Data[$script:FinalisedManifestSurvivedKey] = $true
                throw $failure
            }
            throw $original
        }
        return $moved
    }

    throw "could not find a manifest name free in every store after three attempts (head $HeadSha); the name source is not producing distinct names, and a manifest would have been overwritten"
}

function Write-GateManifestCreateNew {
    <#
      Create-only. `WriteAllText` overwrites by contract; `CreateNew` turns a collision into an
      ERROR, and an error here is a FINDING -- it means two runs really did land on one name, which
      is the concurrency the manifests exist to record.

      ONLY THE CREATE IS A COLLISION. An IOException from the write or the dispose -- a full disk,
      a transient filesystem fault -- is not a name clash, and retrying it under a new name would
      leave the first, TRUNCATED file behind while reporting a healthy manifest, or report a false
      "two collisions" where the real cause was the disk. Those delete the partial file and rethrow
      as themselves. The catch also confirms the path exists before calling it a collision, because
      DirectoryNotFoundException is an IOException too.

      NoRetry is for the SECOND store. Both stores must carry the SAME name -- classify-run.ps1
      finds the durable twin by it -- so renaming there would ORPHAN the copy rather than pair it.
      A collision in the second store is reported, never renamed around.
    #>
    param(
        [Parameter(Mandatory)][string] $Directory,
        [Parameter(Mandatory)][string] $FileName,
        [Parameter(Mandatory)][string] $Json,
        [Parameter(Mandatory)][string] $HeadSha,
        [switch] $NoRetry,
        [scriptblock] $SuffixSource
    )

    $encoding = New-Object System.Text.UTF8Encoding($false)
    $attempt = $FileName
    $passes = if ($NoRetry) { @(1) } else { @(1, 2) }
    foreach ($pass in $passes) {
        $path = [System.IO.Path]::Combine($Directory, $attempt)

        $stream = $null
        try {
            $stream = [System.IO.File]::Open($path, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
        } catch [System.IO.IOException] {
            if (-not (Test-IOExceptionIsNameTaken -Exception $_.Exception)) { throw }
            if ($NoRetry) {
                throw "the manifest name $attempt is already taken in $Directory; renaming here would break the pairing both stores depend on"
            }
            if ($pass -eq 1) {
                $attempt = New-GateManifestFileName -HeadSha $HeadSha -SuffixSource $SuffixSource
                continue
            }
            throw "two manifest names collided in a row under $Directory (last: $attempt); the name source is not producing distinct names, and a manifest would have been overwritten"
        }

        try {
            $bytes = $encoding.GetBytes($Json)
            $stream.Write($bytes, 0, $bytes.Length)
            $stream.Dispose()
            $stream = $null
        } catch {
            # The ORIGINAL error is the one worth reporting, and everything below is cleanup that
            # must not be able to replace it.
            $original = $_

            # Dispose is best-effort. It FLUSHES, so on a full disk it throws the same failure a
            # second time -- and an unguarded Dispose here meant the Remove-Item below never ran,
            # leaving the truncated file under a final name. The next run then reads that corpse as
            # an incumbent and calls it a collision: the exact silent-loss shape this whole helper
            # exists to prevent, reintroduced by its own cleanup path.
            if ($stream) { try { $stream.Dispose() } catch { } }

            # A half-written manifest is worse than none: it parses as far as it got and reads as a
            # complete record of a run that did not finish that way.
            Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue

            throw $original
        }
        return $path
    }
}
