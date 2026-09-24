# #762: nothing that can STOP a pipeline may sit between a native command and the read of its exit
# code.
#
# `Select-Object -First N` stops the pipeline as soon as it has its items, and stopping a pipeline
# TERMINATES the native command feeding it. On PowerShell 7 that leaves `$LASTEXITCODE` at -1 while
# the captured VALUE is correct, so this shape reports a failure that did not happen:
#
#     $value = (& git <something> 2>$null | Select-Object -First 1)
#     if ($LASTEXITCODE -ne 0) { ... }        # -1 on PowerShell 7, 0 on Windows PowerShell 5.1
#
# It shipped once: `ci/find-culture-comparisons.ps1` refused with "not inside a git working tree"
# while holding that tree's path, on a reviewer's host running 7.x. Fixed there in `0851ef97`.
#
# WHY THIS SUITE IS STRUCTURAL AND SAYS SO. The gate runs `powershell.exe` -- Windows PowerShell 5.1
# -- where the pipeline does not terminate the command, and this host has no `pwsh` at all. So the
# RUNTIME behaviour cannot be reddened here by anyone, including whoever reviews this. What is
# asserted is the SHAPE, over the text, which reddens on 5.1 exactly as it would on 7. The defect
# class is "invisible from the interpreter you are standing in", and a suite that pretended to
# measure the runtime would be measuring the wrong edition while reporting it as the right one.
#
# THE SWEEP JOINS WRAPPED PIPELINES, AND THAT IS NOT A DETAIL. The report that opened #762 listed
# six sites in ci/gate.ps1 and stated that ci/merge-proof.ps1 (retired 2026-09-24) had none. Its
# instrument was a per-LINE match, and ci/merge-proof.ps1's two instances put the stopper on the
# CONTINUATION line:
#
#     $rawPaths = @(& git ... ls-tree ... 2>$null |
#             Select-Object -First ($MaxManifests + 1))
#     $listingExit = $LASTEXITCODE
#
# A same-line instrument reads that file as clean. Joining continuations is what found them, so the
# join is asserted by its own canary below rather than assumed.
#
# AND A DELIBERATE STOP IS NOT A DEFECT. In ci/merge-proof.ps1 the stop was load-bearing: it bounded
# what was READ from an untrusted store rather than what survived the read, so the general remedy
# for this ticket -- capture, read the code, then reduce -- was the very unbounded read that file
# went to trouble to prevent. Such sites carry a `762-DELIBERATE-STOP` marker and are PINNED BY NAME
# below (none remain since that file was retired), so a waiver cannot be added silently: a new
# marker fails this suite until someone moves the pin and says why.

$ExpectedAssertionCount = 12
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

# PINNED BY NAME, never by line number: line numbers rot on the next edit above them, and a pin that
# rots gets relaxed instead of read. A file+variable pair survives edits moving the file underneath.
$PinnedWaivers = @()

$NativeCall = '&\s*(git|gh|cargo|npm|node|powershell(\.exe)?|pwsh)\b'
$Stopper = 'Select-Object\s+(-\w+\s+)*-(First|Index)\b'

function Find-StopBeforeExitRead {
    <#
        Returns one record per statement where a native command is piped into something that can
        stop the pipeline. `Live` is true when the exit code is then READ before anything else can
        set it; `Waived` is true when the site declares the stop deliberate.

        THE SAME FUNCTION RUNS ON THE CANARIES AND ON THE REAL TREE. A canary that exercised a
        reimplementation of this logic would certify the copy and say nothing about the sweep.
    #>
    param([Parameter(Mandatory)] [string] $Path)

    $lines = @([System.IO.File]::ReadAllText($Path) -split "`r?`n")
    $found = New-Object System.Collections.Generic.List[psobject]
    $i = 0
    while ($i -lt $lines.Count) {
        if ($lines[$i].TrimStart().StartsWith('#')) { $i++; continue }

        # Join a wrapped pipeline: a line ending in `|` continues into the next one. This is the
        # part a per-line instrument cannot see.
        $statement = $lines[$i]
        $j = $i
        while ($statement.TrimEnd().EndsWith('|') -and ($j + 1) -lt $lines.Count) {
            $j++
            $statement += ' ' + $lines[$j].Trim()
        }

        # Quoted spans are TEXT, not code. Two suites in this directory keep a canary line quoting
        # exactly this defect so their own pattern can be shown to recognise it; accusing those is
        # accusing the documentation of the fix.
        $code = [regex]::Replace($statement, "'[^']*'", "''")
        $code = [regex]::Replace($code, '"[^"]*"', '""')

        if (($code -match $NativeCall) -and ($code -match $Stopper)) {
            # Scan forward for whichever comes first: the read, or another native command that would
            # OVERWRITE $LASTEXITCODE before anyone reads it. The second case is not this defect --
            # the stopped command's code never reaches a decision.
            $live = $false
            for ($k = $j + 1; $k -lt [Math]::Min($j + 7, $lines.Count); $k++) {
                $ahead = $lines[$k]
                if ($ahead.TrimStart().StartsWith('#')) { continue }
                $aheadCode = [regex]::Replace($ahead, "'[^']*'", "''")
                if ($aheadCode -match $NativeCall) { break }
                if ($aheadCode -match '\$LASTEXITCODE') { $live = $true; break }
            }
            # The read can also sit inside the joined statement itself.
            if (-not $live -and ($code -match '\$LASTEXITCODE')) { $live = $true }

            $waiver = $null
            for ($k = [Math]::Max(0, $i - 4); $k -lt $i; $k++) {
                if ($lines[$k] -match '762-DELIBERATE-STOP:\s*(\$\w+)') { $waiver = $Matches[1] }
            }

            $found.Add([pscustomobject]@{
                    File   = Split-Path -Leaf $Path
                    Line   = $i + 1
                    Text   = $statement.Trim()
                    Live   = $live
                    Waived = ($null -ne $waiver)
                    Waiver = $waiver
                })
        }
        $i = if ($j -gt $i) { $j + 1 } else { $i + 1 }
    }
    return $found
}

$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("exit-code-shape-" + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $sandbox -Force | Out-Null

function New-Canary {
    param([Parameter(Mandatory)] [string] $Name, [Parameter(Mandatory)] [string[]] $Lines)
    $path = Join-Path $sandbox "$Name.ps1"
    [System.IO.File]::WriteAllText($path, ($Lines -join "`r`n"))
    return $path
}

try {
    Write-Host "-- the negative controls, FIRST: a sweep that finds nothing looks identical whether the tree is clean or the pattern is broken --"

    # (A) the defect itself, on one line.
    $onOneLine = New-Canary -Name 'one-line' -Lines @(
        '$branch = (& git symbolic-ref --quiet HEAD 2>$null | Select-Object -First 1)',
        'if ($LASTEXITCODE -ne 0) { throw }'
    )
    $hitA = @(Find-StopBeforeExitRead -Path $onOneLine | Where-Object { $_.Live -and -not $_.Waived })
    Assert-True -Condition ($hitA.Count -eq 1) `
        -Message "the sweep finds the defect when one is put in front of it (found $($hitA.Count))"

    # (B) THE WRAPPED SHAPE -- the one the report's per-line instrument could not see, and the one
    # that hid the two real instances in ci/merge-proof.ps1 (since retired).
    $wrapped = New-Canary -Name 'wrapped' -Lines @(
        '$paths = @(& git ls-tree -r --name-only HEAD 2>$null |',
        '        Select-Object -First ($Max + 1))',
        '$listingExit = $LASTEXITCODE'
    )
    $hitB = @(Find-StopBeforeExitRead -Path $wrapped | Where-Object { $_.Live -and -not $_.Waived })
    Assert-True -Condition ($hitB.Count -eq 1) `
        -Message "and finds it when the stopper sits on the CONTINUATION line, which a per-line match cannot (found $($hitB.Count))"

    # (C) the fixed shape: capture, read the code, then reduce.
    $fixed = New-Canary -Name 'fixed' -Lines @(
        '$output = @(& git symbolic-ref --quiet HEAD 2>$null)',
        '$exit = $LASTEXITCODE',
        '$branch = $output | Select-Object -First 1',
        'if ($exit -ne 0) { throw }'
    )
    $hitC = @(Find-StopBeforeExitRead -Path $fixed | Where-Object { $_.Live -and -not $_.Waived })
    Assert-True -Condition ($hitC.Count -eq 0) `
        -Message "and leaves the remedy alone -- capture, read, reduce is not flagged (found $($hitC.Count))"

    # (D) a canary line QUOTING the defect is documentation, not an occurrence of it.
    $quoted = New-Canary -Name 'quoted' -Lines @(
        '$canaryLine = ''    $root = (& git rev-parse --show-toplevel 2>$null | Select-Object -First 1)''',
        'if ($LASTEXITCODE -ne 0) { throw }'
    )
    $hitD = @(Find-StopBeforeExitRead -Path $quoted | Where-Object { $_.Live -and -not $_.Waived })
    Assert-True -Condition ($hitD.Count -eq 0) `
        -Message "and does not accuse a quoted canary, which is what two suites in this directory keep on purpose (found $($hitD.Count))"

    # (E) the value is used and the code is never read at all. A different defect -- a clean answer
    # built out of a failure -- and NOT this one. Flagging it here would price the wrong unit.
    $neverRead = New-Canary -Name 'never-read' -Lines @(
        '$ref = (& git rev-parse refs/remotes/origin/fixture 2>$null | Select-Object -First 1)',
        '$arranged = ([string]$ref -ne '''')'
    )
    $hitE = @(Find-StopBeforeExitRead -Path $neverRead | Where-Object { $_.Live -and -not $_.Waived })
    Assert-True -Condition ($hitE.Count -eq 0) `
        -Message "and is silent where the exit code is never read at all, which is a different defect (found $($hitE.Count))"

    # (F) a SECOND native command reassigns $LASTEXITCODE before anyone reads it, so the stopped
    # command's code never reaches a decision. Measured shape: ci/gate-manifest-provenance.tests.ps1
    # around its upstream arrangement.
    $overwritten = New-Canary -Name 'overwritten' -Lines @(
        '$branch = (& git symbolic-ref --quiet --short HEAD 2>&1 | Select-Object -First 1)',
        '$resolved = @(& git rev-parse --symbolic-full-name "$branch" 2>&1)',
        '$exit = $LASTEXITCODE'
    )
    $hitF = @(Find-StopBeforeExitRead -Path $overwritten | Where-Object { $_.Live -and -not $_.Waived })
    Assert-True -Condition ($hitF.Count -eq 0) `
        -Message "and is silent when a later native call overwrites the code before it is read (found $($hitF.Count))"

    # (G) the waiver is recognised, and it suppresses the SURVIVOR without hiding the SITE.
    $waived = New-Canary -Name 'waived' -Lines @(
        '# 762-DELIBERATE-STOP: $rawPaths -- the bound on what is read is worth more than the code.',
        '$rawPaths = @(& git ls-tree -r --name-only HEAD 2>$null |',
        '        Select-Object -First ($Max + 1))',
        '$listingExit = $LASTEXITCODE'
    )
    $recordsG = @(Find-StopBeforeExitRead -Path $waived)
    Assert-True -Condition (@($recordsG | Where-Object { $_.Live -and -not $_.Waived }).Count -eq 0 -and
        @($recordsG | Where-Object { $_.Waived }).Count -eq 1) `
        -Message 'and a declared deliberate stop is waived as a survivor while still being COUNTED as a site'

    Write-Host "`n-- and now the real tree --"

    $scripts = @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1' -File | Sort-Object -Property Name)
    $records = New-Object System.Collections.Generic.List[psobject]
    foreach ($script in $scripts) {
        foreach ($record in @(Find-StopBeforeExitRead -Path $script.FullName)) { $records.Add($record) }
    }

    # VACUITY CONTROLS. Every assertion below quantifies over a list, and every one of them is
    # trivially true of an empty one.
    Assert-True -Condition ($scripts.Count -gt 0) `
        -Message "the sweep read at least one script under ci/ (read $($scripts.Count))"

    $swept = @($scripts | ForEach-Object { $_.Name })
    Assert-True -Condition ($swept -contains 'gate.ps1') `
        -Message 'and gate.ps1, the file this ticket names that still exists, is IN the population, not merely near it'

    Assert-True -Condition ($records.Count -gt 0) `
        -Message "and the pattern still matches something in this tree, so a clean answer below is about the tree rather than about the pattern (matched $($records.Count) statements)"

    $survivors = @($records | Where-Object { $_.Live -and -not $_.Waived })
    Assert-True -Condition ($survivors.Count -eq 0) `
        -Message ("no native command's exit code is read after something could have stopped its pipeline" +
            $(if ($survivors.Count -gt 0) {
                    ":`n" + (($survivors | ForEach-Object { "        $($_.File):$($_.Line)  $($_.Text)" }) -join "`n") +
                    "`n      Capture into a variable, read `$LASTEXITCODE, and only then reduce. If the stop is" +
                    ' deliberate, mark it `# 762-DELIBERATE-STOP: $var` and pin it in this file.'
                } else { '' }))

    $declared = @($records | Where-Object { $_.Waived } | ForEach-Object { "$($_.File)|$($_.Waiver)" } | Sort-Object -Unique)
    $unpinned = @($declared | Where-Object { $PinnedWaivers -notcontains $_ })
    $vanished = @($PinnedWaivers | Where-Object { $declared -notcontains $_ })
    Assert-True -Condition ($unpinned.Count -eq 0 -and $vanished.Count -eq 0) `
        -Message ('the declared deliberate stops are exactly the pinned ones' +
            $(if ($unpinned.Count -gt 0) { ". WAIVED BUT NOT PINNED: $($unpinned -join ', ')" } else { '' }) +
            $(if ($vanished.Count -gt 0) { ". PINNED BUT NOT PRESENT: $($vanished -join ', ')" } else { '' }))
} finally {
    Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($script:total -ne $ExpectedAssertionCount) {
    Write-Host "HARNESS-BROKE: ran $($script:total) assertions, expected $ExpectedAssertionCount. A case vanished or was added without updating the declared total." -ForegroundColor Magenta
    exit 2
}
if ($script:failures -gt 0) {
    Write-Host "FAILED: $($script:failures) of $($script:total)" -ForegroundColor Red
    exit 1
}
Write-Host "PASSED: $($script:total)/$($script:total)" -ForegroundColor Green
exit 0
