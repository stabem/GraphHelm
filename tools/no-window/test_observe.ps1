param([switch]$RedParent)
$ErrorActionPreference = 'Stop'
if ($env:GRAPHHELM_ALLOW_VISIBLE_TABS -ne '1') {
  Write-Output 'SKIPPED (opt-in)'
  exit 0
}
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

# The real GraphHelm launcher path (#1290): `journey preview --force` on a disposable project whose
# declared launcher is tools/no-window/fixture. A quiet run here proves only this synthetic fixture,
# never a real project; launch failure or missing tools are UNOBSERVED, not a product verdict.
$cli = $env:GRAPHHELM_CLI
if (-not $cli) { $cli = (Get-Command graphhelm.exe -ErrorAction SilentlyContinue).Source }
$sh = (Get-Command sh.exe -ErrorAction SilentlyContinue).Source
$node = (Get-Command node.exe -ErrorAction SilentlyContinue).Source
if (-not $cli -or -not (Test-Path -LiteralPath $cli)) { throw 'UNOBSERVED: launcher case: no GraphHelm CLI in GRAPHHELM_CLI or PATH' }
if (-not $sh -or -not $node) { throw 'UNOBSERVED: launcher case: sh.exe or node.exe is unavailable' }
$cliVersion = (& $cli --version 2>&1 | Out-String).Trim()
Write-Output "launcher case: CLI $cliVersion ($(if ($env:GRAPHHELM_CLI) { 'GRAPHHELM_CLI' } else { 'PATH' })); shell $sh"
$project = Join-Path ([IO.Path]::GetTempPath()) ('gh-nowindow-launch-' + [guid]::NewGuid().ToString('N'))
$marks = Join-Path $project 'marks.txt'
New-Item -ItemType Directory -Path (Join-Path $project '.graphhelm/journeys') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $repo 'tools/no-window/fixture/fixture.sh'), (Join-Path $repo 'tools/no-window/fixture/server.mjs') -Destination $project
$utf8 = New-Object System.Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $project '.graphhelm/journey-fixture.json'), '{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":true}', $utf8)
[IO.File]::WriteAllText((Join-Path $project '.graphhelm/journeys/nowindow.journey.yaml'), @'
schema: graphhelm.journey-flow/1
id: nowindow
title: Fixture service answers its health check
purpose: A client sees the fixture service report that it is healthy.
status: draft
approved: null
base: http://127.0.0.1:8765
actors: [client]
secrets: []
risks: []
screens:
  - id: start
    title: Before any request
    url: /
    state: stable
    expect: [{role: status, name: "404"}]
    scope: unknown
  - id: healthy
    title: The health answer
    url: /health.json
    state: success
    expect: [{role: status, name: "200"}, {role: json, name: status=ok}]
    scope: unknown
edges:
  - id: check
    from: start
    to: healthy
    acts:
      - {kind: http, role: GET, name: /health.json}
paths:
  main: [check]
drift: []
'@.Replace("`r`n", "`n"), $utf8)
$driver = Join-Path $project 'drive.sh'
[IO.File]::WriteAllText($driver, @'
#!/bin/sh
cd "$(dirname "$0")" || exit 3
"$GH_CLI" --json journey preview nowindow --force --project . > start.json 2>&1 || { cat start.json; exit 4; }
i=0
while [ $i -lt 120 ]; do
  "$GH_CLI" --json journey preview nowindow --read --project . > read.json 2>&1
  grep -q '"state":"running"' read.json || break
  i=$((i + 1))
  sleep 1
done
cat read.json
grep -q '"state":"ready"' read.json
'@.Replace("`r`n", "`n"), $utf8)
$launchOutput = ''
$launchExit = $null
try {
  Push-Location $project
  try {
    git init --quiet 2>&1 | Out-Null; git config user.email nowindow@example.invalid; git config user.name nowindow
    & $cli --json init --harness claude-code --project . 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "UNOBSERVED: launcher case: graphhelm init exited $LASTEXITCODE" }
    & $cli --json journey compile nowindow --fmt --include-draft --project . 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "UNOBSERVED: launcher case: journey compile exited $LASTEXITCODE" }
    git add -A 2>&1 | Out-Null; git commit --quiet -m fixture 2>&1 | Out-Null
  } finally { Pop-Location }
  $env:GH_CLI = $cli
  $env:NOWINDOW_MARKS = $marks
  $tabFlag = if ($env:GRAPHHELM_NOWINDOW_TABS -eq '1') { '-ObserveTerminalTabs' } else { '' }
  $script = "& '$observer' $tabFlag -Exe '$sh' -Arguments @('$driver')"
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
  $start = New-Object System.Diagnostics.ProcessStartInfo
  $start.FileName = $shell
  $start.Arguments = "-NoProfile -ExecutionPolicy Bypass -EncodedCommand $encoded"
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $launch = [Diagnostics.Process]::Start($start)
  $launchOutput = $launch.StandardOutput.ReadToEnd() + $launch.StandardError.ReadToEnd()
  $launch.WaitForExit()
  $launchExit = $launch.ExitCode
  $launch.Dispose()
} finally {
  Remove-Item Env:GH_CLI, Env:NOWINDOW_MARKS -ErrorAction SilentlyContinue
}
Write-Output "CASE launcher path: observer exit $launchExit"
Write-Output $launchOutput
$reached = if (Test-Path -LiteralPath $marks) { @(Get-Content -LiteralPath $marks) } else { @() }
$upLine = @($reached | Where-Object { $_ -like 'up *' })
Write-Output "launcher steps reached: $(if ($reached.Count) { $reached -join ', ' } else { 'none' })"
if ($upLine.Count -ne 1 -or -not ($reached -contains 'down')) { throw "UNOBSERVED: launcher case did not reach both up and down (reached: $($reached -join ', '))" }
$childPid = $upLine[0].Substring(3)
& $sh -c "kill -0 $childPid 2>/dev/null"
if ($LASTEXITCODE -eq 0) { & $sh -c "kill $childPid"; throw "launcher case: fixture child $childPid outlived down" }
if (-not $launchOutput.Contains('command exit: 0')) { throw 'UNOBSERVED: launcher case: preview did not end ready, so no product verdict' }
$shownMatch = [regex]::Match($launchOutput, 'console windows shown: (\d+)')
if (-not $shownMatch.Success) { throw 'UNOBSERVED: launcher case: observer printed no window count' }
$tabMatch = [regex]::Match($launchOutput, 'terminal tabs added: (\d+)')
$tabCount = if ($tabMatch.Success) { $tabMatch.Groups[1].Value } else { 'not observed' }
$launcherVerdict = if ($launchExit -eq 0) { 'PASS (synthetic fixture only)' } else { 'FAIL' }
Write-Output "LAUNCHER OBSERVATION: $launcherVerdict; windows $($shownMatch.Groups[1].Value); tabs $tabCount; up and down reached; child stopped"
Remove-Item -LiteralPath $project -Recurse -Force -ErrorAction SilentlyContinue

# A visible, test-owned console is the positive control. Compare the event with
# an independent HWND/PID lookup while that window is still alive.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class TestConsoleWindows {
  delegate bool EnumProc(IntPtr hwnd, IntPtr data);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr data);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder name, int count);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetProcessTimes(IntPtr process, out long creation, out long exit, out long kernel, out long user);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder name, ref int size);
  [StructLayout(LayoutKind.Sequential)] struct PROCESS_BASIC_INFORMATION { public IntPtr reserved1, pebBaseAddress, reserved2a, reserved2b, uniquePid, inheritedPid; }
  [DllImport("ntdll.dll")] static extern int NtQueryInformationProcess(IntPtr process, int infoClass, out PROCESS_BASIC_INFORMATION info, int length, out int returned);
  const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
  public static string[] Identity(uint pid) {
    IntPtr process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
    if (process == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
    try {
      long creation, exit, kernel, user;
      if (!GetProcessTimes(process, out creation, out exit, out kernel, out user)) throw new System.ComponentModel.Win32Exception();
      var path = new StringBuilder(1024);
      int size = path.Capacity;
      if (!QueryFullProcessImageName(process, 0, path, ref size)) throw new System.ComponentModel.Win32Exception();
      PROCESS_BASIC_INFORMATION info;
      int returned;
      if (NtQueryInformationProcess(process, 0, out info, Marshal.SizeOf(typeof(PROCESS_BASIC_INFORMATION)), out returned) < 0) throw new System.ComponentModel.Win32Exception();
      return new string[] { creation.ToString(), System.IO.Path.GetFileName(path.ToString()), info.inheritedPid.ToInt64().ToString() };
    } finally { CloseHandle(process); }
  }
  public static string[] Snapshot() {
    var found = new List<string>();
    EnumWindows((hwnd, data) => {
      var name = new StringBuilder(256);
      GetClassName(hwnd, name, name.Capacity);
      if (IsWindowVisible(hwnd) && (name.ToString() == "ConsoleWindowClass" || name.ToString() == "CASCADIA_HOSTING_WINDOW_CLASS")) {
        uint pid;
        uint thread = GetWindowThreadProcessId(hwnd, out pid);
        if (thread != 0 && pid != 0) {
          string[] identity = Identity(pid);
          found.Add("0x" + hwnd.ToString("x") + "|" + pid + "|" + name + "|" + identity[0] + "|" + identity[1] + "|" + identity[2]);
        }
      }
      return true;
    }, IntPtr.Zero);
    return found.ToArray();
  }
}
'@
$before = @([TestConsoleWindows]::Snapshot())
$observerStart = New-Object System.Diagnostics.ProcessStartInfo
$observerStart.FileName = $shell
$ping = Join-Path $env:SystemRoot 'System32/PING.EXE'
$observerCommand = "& '$observer' -Exe '$cmd' -Arguments @('/c','$ping -n 6 127.0.0.1 >nul')"
$observerEncoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($observerCommand))
$observerStart.Arguments = "-NoProfile -ExecutionPolicy Bypass -EncodedCommand $observerEncoded"
$observerStart.UseShellExecute = $false
$observerStart.CreateNoWindow = $true
$observerStart.RedirectStandardOutput = $true
$observerStart.RedirectStandardError = $true
$observing = [Diagnostics.Process]::Start($observerStart)
$control = $null
try {
  Start-Sleep -Milliseconds 900
  $controlStart = New-Object System.Diagnostics.ProcessStartInfo
  $controlStart.FileName = $cmd
  $controlStart.Arguments = "/c $ping -n 4 127.0.0.1 >nul"
  $controlStart.UseShellExecute = $false
  $controlStart.CreateNoWindow = $false
  $control = [Diagnostics.Process]::Start($controlStart)
  $newWindows = @()
  for ($attempt = 0; $attempt -lt 30 -and $newWindows.Count -eq 0; $attempt++) {
    Start-Sleep -Milliseconds 100
    $newWindows = @([TestConsoleWindows]::Snapshot() | Where-Object { $before -notcontains $_ })
  }
  if ($newWindows.Count -ne 1) { throw "UNOBSERVED: expected one new visible console window, found $($newWindows.Count)" }
  $parts = $newWindows[0].Split('|')
  $independentIdentity = [TestConsoleWindows]::Identity([uint32]$parts[1])
  $control.WaitForExit()
  $stdout = $observing.StandardOutput.ReadToEnd()
  $stderr = $observing.StandardError.ReadToEnd()
  $observing.WaitForExit()
  $output = $stdout + $stderr
  Write-Output "CASE visible console control: observer exit $($observing.ExitCode) (expected 1); HWND $($parts[0]); owner PID $($parts[1])"
  Write-Output $output
  if (-not $output.Contains("$($parts[2]) $($parts[0])")) { throw 'UNOBSERVED: positive control window was not captured by observer' }
  if (-not $output.Contains("$($parts[2]) $($parts[0]) pid=$($parts[1])")) { throw 'Captured console window lacks its independently measured owner PID' }
  if (-not $output.Contains("pid=$($parts[1]) created=$($independentIdentity[0]) name=$($independentIdentity[1]) parent=$($independentIdentity[2])")) { throw 'Captured console window lacks independently measured process identity and parent edge' }
  if ($observing.ExitCode -ne 1) { throw "visible control: expected observer exit 1, got $($observing.ExitCode)" }
} finally {
  if ($control -and -not $control.HasExited) { $control.Kill(); $control.WaitForExit() }
  if (-not $observing.HasExited) { $observing.Kill(); $observing.WaitForExit() }
  if ($control) { $control.Dispose() }
  $observing.Dispose()
}

# Exercise an existing, test-owned Terminal HWND. A title change or selecting a tab
# cannot stand in for a newly created TabItem with a distinct UIA runtime ID.
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$terminalName = 'gh-tab-test-' + [guid]::NewGuid().ToString('N')
$firstTitle = 'gh-baseline-' + [guid]::NewGuid().ToString('N')
$secondTitle = 'gh-added-' + [guid]::NewGuid().ToString('N')
$wt = (Get-Command wt.exe -ErrorAction SilentlyContinue).Source
if (-not $wt) { throw 'UNOBSERVED: Windows Terminal is unavailable' }
$pythonw = Join-Path (Split-Path (Get-Command python.exe).Source) 'pythonw.exe'
if (-not (Test-Path -LiteralPath $pythonw)) { throw 'UNOBSERVED: windowless Python command is unavailable' }
$tabCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem)
$classCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, 'CASCADIA_HOSTING_WINDOW_CLASS')
function Find-OwnedTerminal {
  $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $classCondition)
  foreach ($window in $windows) {
    $tabs = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabCondition)
    foreach ($tab in $tabs) { if ($tab.Current.Name.StartsWith($firstTitle)) { return $window } }
  }
  return $null
}
function Get-TabIds($window) {
  $tabs = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabCondition)
  return @($tabs | ForEach-Object { $_.GetRuntimeId() -join '.' })
}
function Start-ObservedCommand([string]$extra, [int]$seconds = 7) {
  $script = "& '$observer' $extra -Exe '$pythonw' -Arguments @('-c','import time; time.sleep($seconds)')"
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
  $start = New-Object System.Diagnostics.ProcessStartInfo
  $start.FileName = $shell
  $start.Arguments = "-NoProfile -ExecutionPolicy Bypass -EncodedCommand $encoded"
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  return [Diagnostics.Process]::Start($start)
}
$owned = $null
$tabObserver = $null
try {
  Start-Process -FilePath $wt -ArgumentList @('-w', $terminalName, 'new-tab', '--title', $firstTitle, $cmd, '/c', 'ping -n 30 127.0.0.1 >nul') | Out-Null
  for ($attempt = 0; $attempt -lt 40 -and -not $owned; $attempt++) {
    Start-Sleep -Milliseconds 100
    $owned = Find-OwnedTerminal
  }
  if (-not $owned) { throw 'UNOBSERVED: test-owned Terminal baseline tab was not visible through accessibility' }
  $hwnd = $owned.Current.NativeWindowHandle
  $baseline = @(Get-TabIds $owned)
  if ($baseline.Count -ne 1) { throw "UNOBSERVED: owned Terminal baseline has $($baseline.Count) tabs, expected one" }
  if (-not $RedParent) {
    $quiet = Start-ObservedCommand '-ObserveTerminalTabs' 2
    try {
      $quietOutput = $quiet.StandardOutput.ReadToEnd() + $quiet.StandardError.ReadToEnd()
      $quiet.WaitForExit()
      Write-Output "CASE existing Terminal quiet: observer exit $($quiet.ExitCode)"
      if ($quiet.ExitCode -ne 0 -or -not $quietOutput.Contains('terminal tabs added: 0') -or -not $quietOutput.Contains('Coverage: top-level console windows and existing Windows Terminal tabs.')) { throw "Quiet Terminal control failed: $quietOutput" }
    } finally { if (-not $quiet.HasExited) { $quiet.Kill(); $quiet.WaitForExit() }; $quiet.Dispose() }
  }
  $mode = if ($RedParent) { '' } else { '-ObserveTerminalTabs' }
  $tabObserver = Start-ObservedCommand $mode
  $preamble = ''
  if (-not $RedParent) {
    for ($lineCount = 0; $lineCount -lt 2; $lineCount++) { $preamble += $tabObserver.StandardOutput.ReadLine() + [Environment]::NewLine }
    if (-not $preamble.Contains('Terminal tab baseline ready.')) { throw "UNOBSERVED: observer baseline was not ready: $preamble" }
  } else { Start-Sleep -Milliseconds 1100 }
  Start-Process -FilePath $wt -ArgumentList @('-w', $terminalName, 'new-tab', '--title', $secondTitle, $cmd, '/c', 'ping -n 25 127.0.0.1 >nul') | Out-Null
  $added = $null
  for ($attempt = 0; $attempt -lt 40 -and -not $added; $attempt++) {
    Start-Sleep -Milliseconds 100
    $current = Find-OwnedTerminal
    if ($current -and $current.Current.NativeWindowHandle -eq $hwnd) {
      $ids = @(Get-TabIds $current)
      if ($ids.Count -eq 2) { $added = @($ids | Where-Object { $baseline -notcontains $_ }) }
    }
  }
  if (-not $added -or $added.Count -ne 1) { throw 'UNOBSERVED: no distinct second tab appeared in the unchanged owned HWND' }
  $output = $preamble + $tabObserver.StandardOutput.ReadToEnd() + $tabObserver.StandardError.ReadToEnd()
  $tabObserver.WaitForExit()
  $expectedTabExit = if ($RedParent) { 0 } else { 1 }
  Write-Output "CASE existing Terminal tab: HWND $hwnd; baseline $($baseline[0]); added $($added[0]); observer exit $($tabObserver.ExitCode) (expected $expectedTabExit)"
  Write-Output $output
  if ($RedParent) {
    if ($tabObserver.ExitCode -ne 0 -or -not $output.Contains('new tabs in an existing Windows Terminal were not observed')) { throw 'UNOBSERVED: parent did not return the expected top-level-only quiet result' }
    throw 'RED: existing Terminal tab was independently observed but the parent observer reported PASS'
  }
  if ($tabObserver.ExitCode -ne 1 -or -not $output.Contains("tab-added: hwnd=0x$($hwnd.ToString('x')) id=$($added[0])")) { throw 'New tab in unchanged Terminal HWND was not reported' }
  $selectionObserver = Start-ObservedCommand '-ObserveTerminalTabs' 3
  try {
    Start-Sleep -Milliseconds 1000
    $tabs = $owned.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabCondition)
    $baselineTab = @($tabs | Where-Object { ($_.GetRuntimeId() -join '.') -eq $baseline[0] })[0]
    if (-not $baselineTab) { throw 'UNOBSERVED: baseline tab disappeared before selection control' }
    $select = $baselineTab.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
    $select.Select()
    $selectionOutput = $selectionObserver.StandardOutput.ReadToEnd() + $selectionObserver.StandardError.ReadToEnd()
    $selectionObserver.WaitForExit()
    Write-Output "CASE existing Terminal selection only: observer exit $($selectionObserver.ExitCode)"
    if ($selectionObserver.ExitCode -ne 0 -or -not $selectionOutput.Contains('terminal tabs added: 0')) { throw "Selection-only Terminal control failed: $selectionOutput" }
  } finally { if (-not $selectionObserver.HasExited) { $selectionObserver.Kill(); $selectionObserver.WaitForExit() }; $selectionObserver.Dispose() }
} finally {
  if ($tabObserver) {
    if (-not $tabObserver.HasExited) { $tabObserver.Kill(); $tabObserver.WaitForExit() }
    $tabObserver.Dispose()
  }
  if ($owned) {
    try { $pattern = $owned.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern); $pattern.Close() } catch { Write-Output "UNOBSERVED: could not close test-owned Terminal HWND $hwnd : $_" }
  }
}
Write-Output "PASS: all observer controls passed; launcher product observation: $launcherVerdict"
