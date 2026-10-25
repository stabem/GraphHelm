# Repair the existing main-check task's action using the protected primary checkout.
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$Repo = (Get-Location).Path,
    [string]$Pythonw = (Get-Command pythonw.exe -ErrorAction Stop).Source,
    [Parameter(Mandatory)][AllowEmptyCollection()][string[]]$RunnerArguments
)
$ErrorActionPreference = 'Stop'
$taskName = 'GraphHelm main whole run'

# A linked worktree is disposable. Its common Git directory belongs to the primary checkout,
# which workspace removal refuses and sweeps do not select as a linked worktree.
$common = & git -C $Repo rev-parse --path-format=absolute --git-common-dir
if ($LASTEXITCODE -ne 0) { throw "Cannot find the primary checkout for $Repo" }
$common = [IO.Path]::GetFullPath($common.Trim())
if ((Split-Path $common -Leaf) -ne '.git') { throw 'A primary, non-bare checkout is required.' }
$primary = Split-Path $common -Parent
$script = Join-Path $primary 'tools/reached-tests/main_whole_run.py'
if (-not (Test-Path -LiteralPath $script -PathType Leaf)) { throw "Main-check runner is missing: $script" }
$pythonPath = (Resolve-Path -LiteralPath $Pythonw).Path
if ((Split-Path $pythonPath -Leaf) -ne 'pythonw.exe') { throw 'Use pythonw.exe so the scheduled check opens no console.' }
if ($RunnerArguments -match '^--rep') { throw 'The repository is selected from the primary checkout, not runner overrides.' }

# Quote Windows argv, including a path ending in a backslash. No shell evaluates these values.
$arguments = @($script, '--repo', $primary) + $RunnerArguments
$quoted = foreach ($argument in $arguments) {
    '"' + (($argument -replace '(\\*)"', '$1$1\"') -replace '(\\+)$', '$1$1') + '"'
}
$action = New-ScheduledTaskAction -Execute $pythonPath -Argument ($quoted -join ' ') -WorkingDirectory $primary
$task = Get-ScheduledTask -TaskName $taskName -ErrorAction Stop
if ($task.State -eq 'Running') { throw 'The main check is running; wait for it to finish before updating its action.' }
if (@($task.Actions).Count -ne 1) { throw 'Expected one existing task action; inspect it before replacing it.' }
if ($PSCmdlet.ShouldProcess($taskName, "Use $script from $primary")) {
    # Updating only Action preserves the existing triggers, principal and settings.
    Set-ScheduledTask -TaskName $taskName -Action $action | Out-Null
}
