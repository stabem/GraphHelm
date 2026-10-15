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
  Write-Output "CASE visible console control: observer exit $($observing.ExitCode); HWND $($parts[0]); owner PID $($parts[1])"
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
