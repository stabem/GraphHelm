$ErrorActionPreference = 'Stop'
$repo = (git rev-parse --show-toplevel).Trim()
$observer = Join-Path $repo 'tools/no-window/observe.ps1'
$shell = (Get-Process -Id $PID).Path
$cmd = Join-Path $env:SystemRoot 'System32/cmd.exe'
$cases = @(
  @{ Name = 'failed command'; Script = "Remove-Item Env:OS -ErrorAction SilentlyContinue; & '$observer' -Exe '$cmd' -Arguments @('/c','exit 7')"; Expected = 1; Contains = @('command exit: 7', 'console windows shown: 0'); Excludes = @('PASS:') },
  @{ Name = 'successful quiet command'; Script = "Remove-Item Env:OS -ErrorAction SilentlyContinue; & '$observer' -Exe '$cmd' -Arguments @('/c','exit 0')"; Expected = 0; Contains = @('command exit: 0', 'Coverage: new top-level console windows only', 'PASS: no console window opened'); Excludes = @('ERROR:') },
  @{ Name = 'space-containing argument'; Script = "Remove-Item Env:OS -ErrorAction SilentlyContinue; & '$observer' -Exe '$cmd' -Arguments @('/c','echo','a b')"; Expected = 0; Contains = @('a b', 'PASS: no console window opened'); Excludes = @('ERROR:') }
)
foreach ($case in $cases) {
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($case.Script))
  $start = New-Object System.Diagnostics.ProcessStartInfo
  $start.FileName = $shell
  $start.Arguments = "-NoProfile -ExecutionPolicy Bypass -EncodedCommand $encoded"
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $process = [Diagnostics.Process]::Start($start)
  $stdout = $process.StandardOutput.ReadToEnd()
  $stderr = $process.StandardError.ReadToEnd()
  $process.WaitForExit()
  $output = $stdout + $stderr
  Write-Output "CASE $($case.Name): exit $($process.ExitCode)"
  Write-Output $output
  if ($process.ExitCode -ne $case.Expected) { throw "$($case.Name): expected exit $($case.Expected), got $($process.ExitCode)" }
  foreach ($text in $case.Contains) { if (-not $output.Contains($text)) { throw "$($case.Name): missing '$text'" } }
  foreach ($text in $case.Excludes) { if ($output.Contains($text)) { throw "$($case.Name): unexpectedly contained '$text'" } }
}
