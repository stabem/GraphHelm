<#
    #753: list the PowerShell comparisons that decide something with a CULTURE-AWARE comparer.

    `-eq`, `-ne`, `-in`, `-notin` and `-contains` are case-insensitive AND culture aware, and a
    culture comparison gives some code points no weight at all. Measured on this machine:

        ('GREEN' + [char]0xFE00) -eq 'GREEN'                     -> True
        ('green' + [char]0xFE00) -in @('green', 'UNCLASSIFIED')  -> True
        [string]::Equals('GREEN' + [char]0xFE00, 'GREEN', 'Ordinal')  -> False

    U+FE00 is a variation selector: category Mn, ordinary text. So the remedy is never to refuse the
    character -- that is a deny-list growing by one code point per review -- but to stop comparing
    approximately, with `[string]::Equals(..., Ordinal)` where case matters and
    `[string]::Equals(..., OrdinalIgnoreCase)` where it deliberately does not. THAT CHOICE IS PER
    SITE: paths on Windows are case-insensitive, and a closed vocabulary written in one case by one
    producer is not.

    WHAT THIS IS AND IS NOT.

    It is a REPORTER: it lists sites and always exits 0. It is not a gate, and it is deliberately not
    one -- deciding which of these sites is a defect needs the reader to know where the value came
    from, and a gate that answers that question by pattern would be wrong in both directions.

    It is also not a proof of ABSENCE. An empty list means this pattern found nothing in the files it
    was given: multi-line comparisons are found (the AST does not care about lines), but `switch`
    statements, `-match`, `-like` and comparisons hidden behind a variable holding an operator name
    are not looked for at all.

    ASKED OF THE AST, NOT OF THE SOURCE TEXT. A regex over source flags the operator named inside a
    throw MESSAGE -- ci/classify-run.ps1 has one -- and a sweep with a false positive is a sweep
    somebody deletes three months later.

    Comparisons against $null, $true and $false are exempt: those are identity and boolean tests
    rather than text, and listing them would bury the ones that matter.

    .EXAMPLE
        powershell -NoProfile -ExecutionPolicy Bypass -File ci/find-culture-comparisons.ps1
        powershell -NoProfile -ExecutionPolicy Bypass -File ci/find-culture-comparisons.ps1 -Path ci/gate.ps1
#>
[CmdletBinding()]
param(
    # The files to read. Default: every versioned .ps1 in the repository, listed with `git -C <root>`
    # rather than a bare `git ls-files`, because inside a subdirectory that answers about the CWD
    # PREFIX and can come back EMPTY with exit code 0 -- which reads as "no such files" and is "you
    # are somewhere else". The root is asked for first, and the listing is made from there.
    [string[]] $Path,
    [switch] $AsJson
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Files the parser could not read. Carried into the summary so an empty list is never bare.
$script:unreadable = @()

# Every binary operator whose string comparison goes through the current culture.
$CultureOperators = @(
    'Ieq', 'Ine', 'Ceq', 'Cne',
    'Icontains', 'Inotcontains', 'Ccontains', 'Cnotcontains',
    'Iin', 'Inotin', 'Cin', 'Cnotin'
)

function Test-ExemptOperand {
    param([Parameter(Mandatory)] $Node)
    if ($Node -is [System.Management.Automation.Language.VariableExpressionAst]) {
        return ([System.Array]::IndexOf(@('null', 'true', 'false'), $Node.VariablePath.UserPath.ToLowerInvariant()) -ge 0)
    }
    # A number or a bool literal. A STRING constant is not exempt: it is the whole point.
    return ($Node -is [System.Management.Automation.Language.ConstantExpressionAst] -and
        -not ($Node -is [System.Management.Automation.Language.StringConstantExpressionAst]))
}

function Find-CultureComparison {
    <# The one producer of the answer, so the suite and an operator read the same list. #>
    param([Parameter(Mandatory)] [string] $File)

    # A PATH THAT IS NOT THERE IS AN ERROR, NOT A ZERO. Passing two files as `-Path a,b` through
    # `powershell -File` hands this one argument named `a,b`: the parser refused it, the warning
    # scrolled past, and the summary said "0 culture-aware comparisons" -- a clean-looking answer
    # about a file that does not exist. The tool must not have the failure shape it exists to find.
    if (-not (Test-Path -LiteralPath $File -PathType Leaf)) {
        throw "no file at $File. With 'powershell -File', pass one -Path per invocation: a shell that hands over 'a,b' as one argument names a file that does not exist."
    }
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path -LiteralPath $File).Path,
        [ref] $null, [ref] $parseErrors)
    if (@($parseErrors).Count -gt 0) {
        # A partial parse is evidence for nothing: a site may be missing because it is not there, or
        # because the parser stopped before reaching it. Counted as UNREAD rather than as zero, and
        # the summary carries that count so an empty list is never bare.
        Write-Warning "$File does not parse ($(@($parseErrors)[0].Message)); its sites cannot be listed."
        $script:unreadable += $File
        return @()
    }
    return @($ast.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.BinaryExpressionAst] -and
                ([System.Array]::IndexOf($CultureOperators, $node.Operator.ToString()) -ge 0) -and
                -not (Test-ExemptOperand -Node $node.Left) -and
                -not (Test-ExemptOperand -Node $node.Right)
            }, $true) | ForEach-Object {
            [pscustomobject]@{
                File     = $File
                Line     = $_.Extent.StartLineNumber
                Operator = $_.Operator.ToString()
                Text     = ($_.Extent.Text -replace '\s+', ' ')
            }
        })
}

# Dot-sourced by the suite: when this file is loaded rather than run, it defines the functions and
# stops. `$MyInvocation.InvocationName` is '.' exactly then.
# Ordinal, like everything this tool asks of other files. It reported this very line when run
# over itself, which is the right behaviour: an instrument that exempted itself from its own
# rule would be the first place the rule stopped holding.
if ([string]::Equals($MyInvocation.InvocationName, '.', [System.StringComparison]::Ordinal)) { return }

$files = if ($Path) { @($Path) } else {
    # NO PIPELINE BETWEEN THE COMMAND AND ITS EXIT CODE. `Select-Object -First 1` stops the pipeline
    # as soon as it has its one item, which TERMINATES the native command -- and on PowerShell 7 that
    # leaves `$LASTEXITCODE` at -1 while `$root` holds the correct path. The tool then threw "not
    # inside a git working tree" while holding the path of the git working tree it was in: harness
    # broken, wearing a user error, failing toward the wrong colour. That is the class this whole
    # pull request is about, inside the instrument the pull request delivers.
    #
    # NOT REPRODUCED HERE, and said so rather than left implied: this machine runs Windows PowerShell
    # 5.1, where the pipeline does not terminate the command and both spellings exit 0. It was
    # measured on another host by a reviewer who could not run the documented invocation at all. The
    # gate runs `powershell.exe`, so the gate would never have seen it -- which is precisely why it
    # had to be fixed rather than filed: an instrument invisible to the instrument that would catch
    # it is the one that gets believed.
    #
    # The remedy is the shape, not the operator: capture, read the exit code, and only then reduce.
    $rootOutput = @(& git rev-parse --show-toplevel 2>$null)
    $rootExit = $LASTEXITCODE
    $root = @($rootOutput | Where-Object { $_ } | Select-Object -First 1)
    if ($rootExit -ne 0 -or -not $root) { throw 'not inside a git working tree, and no -Path was given' }
    $root = ([string]$root).Trim()
    # AND THE LISTING'S EXIT CODE IS READ TOO, which it was not: a failed `ls-files` produced an
    # empty list, the map over it produced no files, and the summary said "0 culture-aware
    # comparisons over 0 files" -- a clean answer built out of a failure. Found by the cell written
    # for the line above, in the same file, three lines away: the fix for one site swept up its
    # neighbour, which is the whole reason that cell asserts a SHAPE and not a line number.
    $listOutput = @(& git -C $root ls-files '*.ps1')
    if ($LASTEXITCODE -ne 0) { throw "git ls-files failed in $root, so the file list is not a file list" }
    @($listOutput | Where-Object { $_ } | ForEach-Object { Join-Path $root $_ })
}

$found = @(foreach ($file in $files) { Find-CultureComparison -File $file })

if ($AsJson) {
    $found | ConvertTo-Json -Depth 4
    return
}

foreach ($site in $found) {
    Write-Host ("{0}:{1}  {2}  {3}" -f $site.File, $site.Line, $site.Operator, $site.Text)
}
# Counted through @() rather than off the variables. Under StrictMode a scalar has no `.Count`, and
# ONE result is a scalar -- so the summary line threw exactly when the answer was a single site,
# which is the case an operator is most likely to be looking at.
$foundCount = @($found).Count
$fileCount = @($files).Count
Write-Host ""
$unreadCount = @($script:unreadable).Count
Write-Host ("$foundCount culture-aware comparison(s) over $fileCount file(s)" +
    $(if ($unreadCount -gt 0) { ", and $unreadCount file(s) COULD NOT BE READ: " + (@($script:unreadable) -join ', ') } else { '' }) + '. ' +
    'A LIST, not a verdict: which of these is a defect depends on where the value comes from, ' +
    'and an empty list is not a proof of absence (switch, -match and -like are not looked for).')
