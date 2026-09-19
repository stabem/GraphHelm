# #638: read gate-run manifests and ACCUSE overlapping runs.
#
# Two gate runs whose windows intersect ran at the same time on the same machine. That matters
# because a shared CARGO_TARGET_DIR makes a concurrent run's green untrustworthy by construction
# (#200, and the target-dir contamination rule): the artefacts one run measures may have been
# written by the other. The manifests can already prove concurrency -- each carries its own
# runStartUtc/runEndUtc, so no pairing heuristic and no name normalisation is needed -- and until
# now nothing read them for it.
#
# What this CANNOT answer, stated because the artefact is structurally blind to it: WHO ran. The
# manifest records no agent identity (SLOT.log's agent field is the literal string `gate` in every
# entry), so an overlap here is "one machine ran two gates at once", never "two owners". Deciding
# ownership needs a field nobody writes yet -- see #638.

Set-StrictMode -Version Latest

# A gate manifest is a few KB. Three orders of magnitude of headroom, so a genuine manifest never
# trips this and a pathological file never gets read.
$script:MaxManifestBytes = 4MB

# The pairwise scan is O(N^2). This store holds 68 after a week of use, so 2000 is generous by more
# than an order of magnitude and still refuses long before the loop becomes the problem.
$script:MaxRunsForPairwise = 2000

function Format-UntrustedValue {
    <#
      A manifest is untrusted input, and `cargoTargetDir` is the RAW environment value the gate
      recorded. ci/gate-target-dir.tests.ps1:124-137 deliberately proves that a target dir carrying
      a CRLF followed by `2026-01-01 | gate | FORGED | payload` PASSES validation and is persisted
      EXACTLY -- the repository already treats that as an attack and base64-encodes it for slot
      events. Printing it raw here would let a manifest forge lines in, or emit terminal control
      sequences into, the very output someone reads to decide whether a run can be trusted.
    #>
    param([AllowNull()][string] $Value)

    if ([string]::IsNullOrEmpty($Value)) { return "(none)" }
    $builder = [System.Text.StringBuilder]::new()
    foreach ($char in $Value.ToCharArray()) {
        if ([char]::IsControl($char)) {
            [void] $builder.AppendFormat("<U+{0:X4}>", [int]$char)
        } else {
            [void] $builder.Append($char)
        }
    }
    return $builder.ToString()
}

function Read-GateRunWindows {
    <#
      Population comes from the tree, by glob -- never from a hand-kept list, because a list is a
      second thing to forget to update and its staleness looks exactly like a clean result.

      A manifest whose window will not parse is NOT skipped. Skipping is what turns a broken
      instrument into a reassuring zero; such a file is returned in `Indeterminate` so the caller
      reports it as a third state rather than counting it as evidence of no overlap.
    #>
    param([Parameter(Mandatory)][string] $Directory)

    if (-not (Test-Path -LiteralPath $Directory)) {
        throw "gate-run manifest directory not found: $Directory"
    }

    $windows = @()
    $indeterminate = @()

    # Bounded on the DISCOVERED count, before the sort and before a single file is opened. The
    # window-count guard further down never fires if every file is malformed -- none of them
    # becomes a window -- so a directory of 100k broken files sailed past the "aggregate bound"
    # while this loop enumerated, sorted and read all of them. An advertised bound that a
    # sufficiently broken population walks around is worse than no bound, because it is quoted.
    # STREAMED, and stopped at the cap. `@(Get-ChildItem ...)` materialises every FileInfo before
    # the count can be checked, so the guard I added last round bounded the parsing and the sort
    # while leaving the enumeration in front of it unbounded -- the cost moved one step earlier
    # rather than going away. EnumerateFiles is lazy, so this stops after the (cap + 1)th name and
    # never builds the rest.
    $discovered = [System.Collections.Generic.List[System.IO.FileInfo]]::new()
    $tooMany = $false
    foreach ($found in [System.IO.Directory]::EnumerateFiles($Directory, '*.json')) {
        if ($discovered.Count -ge $script:MaxRunsForPairwise) { $tooMany = $true; break }
        $discovered.Add([System.IO.FileInfo]::new($found))
    }
    if ($tooMany) {
        throw "directory holds more than $($script:MaxRunsForPairwise) manifests; refusing before reading them, and stopping the scan rather than counting the rest. Narrow -Directory rather than trusting a partial answer"
    }

    foreach ($file in ($discovered | Sort-Object Name)) {
        $start = $null
        $end = $null
        $reason = $null

        # Bounded BEFORE the read, not after. This script reports on a directory it does not own,
        # and an oversized or pathological manifest would exhaust the reader while it was still
        # deciding what to say -- the detector costing more than the thing it detects. Over the
        # bound is a THIRD STATE like every other unreadable file: never a skip, never a crash.
        # The population stays complete; the file is simply not evidence.
        if ($file.Length -gt $script:MaxManifestBytes) {
            $indeterminate += [pscustomobject]@{
                Name   = $file.Name
                Reason = "exceeds $($script:MaxManifestBytes) bytes ($($file.Length))"
            }
            continue
        }

        try {
            $manifest = Get-Content -LiteralPath $file.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
            # `null`, `42`, `"x"` and `[]` are all VALID json and none of them is a manifest.
            # ConvertFrom-Json returns them happily, and the property reads below would throw under
            # StrictMode OUTSIDE this catch -- killing the whole report instead of marking one file.
            # An instrument that dies on one bad input reports nothing about the good ones.
            if ($null -eq $manifest -or $manifest -is [array] -or $manifest -is [string] -or $manifest -is [valuetype]) {
                $indeterminate += [pscustomobject]@{ Name = $file.Name; Reason = 'json is not an object' }
                continue
            }
        } catch {
            $indeterminate += [pscustomobject]@{ Name = $file.Name; Reason = 'unparseable json' }
            continue
        }

        foreach ($field in @('runStartUtc', 'runEndUtc')) {
            if (-not ($manifest.PSObject.Properties.Name -contains $field)) {
                $reason = "missing $field"
            }
        }
        if ($null -eq $reason) {
            # An explicit offset is REQUIRED, not preferred. `[datetimeoffset]::Parse` on a string
            # with no `Z` and no offset silently adopts the READING machine's local zone, so the
            # same manifest yields a different window on two machines and this tool -- whose entire
            # output is the intersection of windows -- would report an overlap in one place and
            # none in another. A field named `...Utc` that does not say so is malformed, and
            # malformed is the third state, never a guess.
            # Parse FIRST, then demand the offset. Checking the offset first stole the diagnosis
            # from a string that is simply not a timestamp -- `not-a-time` has no offset and is
            # also unparseable, and reporting the narrower cause hides the broader one. Caught by
            # the existing `an unparseable timestamp is named` cell when this check was added.
            try {
                $styles = [System.Globalization.DateTimeStyles]::AssumeUniversal -bor [System.Globalization.DateTimeStyles]::AdjustToUniversal
                $start = [datetimeoffset]::Parse($manifest.runStartUtc, [cultureinfo]::InvariantCulture, $styles).ToUniversalTime()
                $end = [datetimeoffset]::Parse($manifest.runEndUtc, [cultureinfo]::InvariantCulture, $styles).ToUniversalTime()
            } catch {
                $reason = 'unparseable timestamp'
            }
        }
        if ($null -eq $reason) {
            foreach ($pair in @(@('runStartUtc', $manifest.runStartUtc), @('runEndUtc', $manifest.runEndUtc))) {
                if ([string]$pair[1] -notmatch '(Z|[+-]\d{2}:?\d{2})$') {
                    $reason = "$($pair[0]) carries no explicit UTC offset"
                    $start = $null
                    $end = $null
                }
            }
        }
        if ($null -eq $reason -and $end -lt $start) {
            $reason = 'end precedes start'
        }

        if ($null -ne $reason) {
            $indeterminate += [pscustomobject]@{ Name = $file.Name; Reason = $reason }
            continue
        }

        $windows += [pscustomobject]@{
            Name      = $file.Name
            HeadSha   = if ($manifest.PSObject.Properties.Name -contains 'headSha') { $manifest.headSha } else { '(none)' }
            Status    = if ($manifest.PSObject.Properties.Name -contains 'status') { $manifest.status } else { '(none)' }
            # Recorded by only some producers, so it is read as evidence when present and as ABSENT
            # when not -- never defaulted to a value that would make two runs look alike.
            TargetDir = if (($manifest.PSObject.Properties.Name -contains 'cargoTargetDir') -and $manifest.cargoTargetDir) { [string]$manifest.cargoTargetDir } else { $null }
            Start     = $start
            End       = $end
        }
    }

    return [pscustomobject]@{ Windows = @($windows); Indeterminate = @($indeterminate) }
}

function Find-OverlappingRuns {
    <#
      Overlap is a STRICT intersection. Two runs where one ends exactly as the next begins share an
      instant, not an interval, and nothing can have been measured concurrently in zero time --
      flagging that would be a guard firing on correct behaviour, which costs more than it catches.
    #>
    param([Parameter(Mandatory)][AllowEmptyCollection()][object[]] $Windows)

    # Bounded BEFORE the nested loop, not inside it. N mutually overlapping windows allocate
    # N(N-1)/2 result objects, so the per-file bound does not bound THIS: a directory of small,
    # perfectly valid manifests still exhausts the reader. Same class as the sweep budget in #646 --
    # a per-item limit that leaves the aggregate unbounded. Over the limit the tool refuses out
    # loud rather than half-reporting, because a truncated overlap list is indistinguishable from a
    # short one.
    if ($Windows.Count -gt $script:MaxRunsForPairwise) {
        throw "population of $($Windows.Count) runs exceeds $($script:MaxRunsForPairwise); the pairwise scan is quadratic and would not complete meaningfully. Narrow -Directory rather than trusting a partial answer"
    }

    # A generic list, not `+=`. Each `+=` reallocates and copies the whole array, so collecting R
    # results costs O(R^2) on TOP of the O(N^2) scan -- with the population bound alone, a fully
    # overlapping set still stalls long before the ceiling it is allowed to reach.
    $overlaps = [System.Collections.Generic.List[object]]::new()
    for ($i = 0; $i -lt $Windows.Count; $i++) {
        for ($j = $i + 1; $j -lt $Windows.Count; $j++) {
            $a = $Windows[$i]
            $b = $Windows[$j]
            $latestStart = if ($a.Start -gt $b.Start) { $a.Start } else { $b.Start }
            $earliestEnd = if ($a.End -lt $b.End) { $a.End } else { $b.End }
            if ($earliestEnd -gt $latestStart) {
                # Overlapping in TIME is all this proves. Whether the two runs could contaminate
                # each other depends on the target directory, which `ci/gate.ps1` explicitly allows
                # a run to isolate -- so a temporal overlap between two isolated runs is a fact
                # about scheduling, not about trust. Only 25 of the 68 manifests on this machine
                # record `cargoTargetDir` at all, so the honest partition has THREE buckets and the
                # missing one is not quietly folded into either of the others.
                # The asymmetry here is the whole point, and it runs one way only.
                #
                # DIFFERENT paths prove NO sharing: two different directories cannot be one
                # directory, on one machine or on two. That conclusion is sound without knowing the
                # host.
                #
                # MATCHING paths prove NOTHING. `D:/graphhelm-target` on two machines is two
                # directories with one name, and the manifest records no machine identity at all --
                # measured: zero of the 29 distinct fields across this store name a host. Calling
                # that `shared-target` would be same-name-different-instance, and it would
                # manufacture the exact bucket this report's headline rests on.
                #
                # So a proven-shared verdict is currently UNREACHABLE, and its count is zero
                # because the artefact cannot establish it -- not because concurrency was measured
                # absent. Those two zeros read alike and mean opposite things.
                # NO BUCKET HERE IS A VERDICT ABOUT SHARING, and the previous revision of this
                # file got that half right. It stopped reading equal path TEXT as proof of a shared
                # directory. It went on reading UNEQUAL text as proof of separate ones, which is
                # the same weakness mirrored: `C:/target` and `C:	arget` are one directory spelt
                # two ways, and so are a trailing separator, a `..`, a symlink, or two mounts of
                # one share. This is not hypothetical here -- gate.ps1 records the RAW environment
                # value (`:483`), and ci/gate-target-dir.tests.ps1:108-115 deliberately ACCEPTS
                # `C:/explicit-isolated-target` and `C:\explicit-isolated-target` as valid, so the
                # producer is documented to emit aliases of one path.
                #
                # Canonicalising the strings would not rescue it either: resolving a symlink or a
                # mount needs the filesystem that wrote them, on a host the manifest does not name.
                #
                # So the labels below describe the EVIDENCE, not a conclusion, and every one of
                # them begins with `unknown` on purpose. The contamination question is currently
                # unanswerable from this artefact, and #638's remedy is what would change that.
                $sharing =
                    if ($null -eq $a.TargetDir -or $null -eq $b.TargetDir) { 'unknown-no-target-dir' }
                    elseif ($a.TargetDir -eq $b.TargetDir) { 'unknown-same-path-text' }
                    else { 'unknown-different-path-text' }
                $overlaps.Add([pscustomobject]@{
                    A               = $a.Name
                    B               = $b.Name
                    # [long], not [int]: manifests are untrusted input, and two adversarial or
                    # corrupted stamps spanning centuries overflow a 32-bit cast, which throws and
                    # kills the whole report rather than classifying one pair. An instrument must
                    # not die on the input it exists to judge.
                    OverlapSeconds  = [long]($earliestEnd - $latestStart).TotalSeconds
                    WindowStartUtc  = $latestStart.ToString('o')
                    WindowEndUtc    = $earliestEnd.ToString('o')
                    Sharing         = $sharing
                    TargetDirA      = $a.TargetDir
                    TargetDirB      = $b.TargetDir
                })
            }
        }
    }
    # Comma operator, not `return @(...)`: PowerShell unrolls a single-element pipeline and
    # collapses an EMPTY array to $null on return, so the honest answer "zero overlaps" would
    # come back as something whose .Count throws under StrictMode. A zero has to be a zero.
    return ,@($overlaps.ToArray())
}
