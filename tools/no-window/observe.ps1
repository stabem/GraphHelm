<#
.SYNOPSIS
  Observer for the no-console-window rule: runs a command (by default a reached-test plan) and
  counts console windows that become visible while it runs. Prints PASS (zero) or FAIL (any).

.DESCRIPTION
  A WinEvent hook (EVENT_OBJECT_SHOW, out of context) records every top-level window of a console
  class that is shown during the run: classic conhost (ConsoleWindowClass) and Windows Terminal
  (CASCADIA_HOSTING_WINDOW_CLASS), including windows that flash for a few milliseconds. A child
  started with CREATE_NO_WINDOW has a hidden console that is never shown, so it is not counted.
  Windows that were already visible when the observer started are ignored. The command runs as a
  DETACHED_PROCESS (no console), the way an agent harness starts tools, so a child that skips
  CREATE_NO_WINDOW is forced to open a window of its own. Output goes to a log; its tail is printed.
  A bare program name must resolve as a file (use python.exe, cargo.exe or a full path).

.EXAMPLE
  pwsh tools/no-window/observe.ps1
  pwsh tools/no-window/observe.ps1 -- cargo +1.97.1 test --locked -p graphhelm-process -- --test-threads=2
#>
param(
  [string]$Root = (Join-Path $env:TEMP 'gh-nowindow-root'),
  [string]$Lane = 'nowindow',
  [string]$Base = 'origin/main',
  [string]$Exe,
  [string[]]$Arguments,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$Command
)
$ErrorActionPreference = 'Stop'
if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) { Write-Output 'SKIP: the observer needs Windows'; exit 2 }

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
public static class ConsoleWindowWatch {
  delegate void WinEventProc(IntPtr hook, uint evt, IntPtr hwnd, int idObject, int idChild, uint thread, uint time);
  [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint min, uint max, IntPtr mod, WinEventProc proc, uint pid, uint tid, uint flags);
  [DllImport("user32.dll")] static extern bool UnhookWinEvent(IntPtr hook);
  [DllImport("user32.dll")] static extern int GetMessage(out MSG msg, IntPtr hwnd, uint min, uint max);
  [DllImport("user32.dll")] static extern bool PostThreadMessage(uint tid, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder name, int max);
  [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr h; public uint m; public IntPtr w; public IntPtr l; public uint t; public int x; public int y; }
  static readonly object Gate = new object();
  static readonly List<string> Seen = new List<string>();
  static WinEventProc Proc;
  static uint LoopThread;
  static Thread Worker;
  static readonly ManualResetEvent Ready = new ManualResetEvent(false);
  public static void Start() {
    Worker = new Thread(() => {
      LoopThread = GetCurrentThreadId();
      Proc = OnEvent;
      IntPtr hook = SetWinEventHook(0x8002, 0x8002, IntPtr.Zero, Proc, 0, 0, 0x0002);
      Ready.Set();
      MSG msg;
      while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) { }
      UnhookWinEvent(hook);
    });
    Worker.IsBackground = true;
    Worker.Start();
    Ready.WaitOne();
  }
  static void OnEvent(IntPtr hook, uint evt, IntPtr hwnd, int idObject, int idChild, uint thread, uint time) {
    if (idObject != 0 || idChild != 0 || hwnd == IntPtr.Zero) return;
    if (GetAncestor(hwnd, 2) != hwnd) return;
    var name = new StringBuilder(256);
    GetClassName(hwnd, name, name.Capacity);
    var cls = name.ToString();
    if (cls == "ConsoleWindowClass" || cls == "CASCADIA_HOSTING_WINDOW_CLASS") {
      lock (Gate) { Seen.Add(DateTime.Now.ToString("HH:mm:ss.fff") + " " + cls + " 0x" + hwnd.ToString("x")); }
    }
  }
  // Agent harnesses start tools with no console at all. Reproduce that: DETACHED_PROCESS, stdout and
  // stderr to a log file. A console child spawned without CREATE_NO_WINDOW then gets a new window.
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct STARTUPINFO {
    public int cb; public string r; public string d; public string t; public int x, y, w, h, cx, cy, fill, flags;
    public short show, r2; public IntPtr r3, stdin, stdout, stderr; }
  [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr process, thread; public int pid, tid; }
  [StructLayout(LayoutKind.Sequential)] struct SECURITY_ATTRIBUTES { public int length; public IntPtr descriptor; public bool inherit; }
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool CreateProcess(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string cwd, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateFile(string name, uint access, uint share, ref SECURITY_ATTRIBUTES sa, uint disposition, uint attrs, IntPtr template);
  [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
  [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr h, out uint code);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  public static int RunDetached(string commandLine, string cwd, string log) {
    var sa = new SECURITY_ATTRIBUTES { length = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES)), inherit = true };
    IntPtr file = CreateFile(log, 0x40000000, 3, ref sa, 2, 0x80, IntPtr.Zero);
    if (file == new IntPtr(-1)) throw new System.ComponentModel.Win32Exception();
    var si = new STARTUPINFO { cb = Marshal.SizeOf(typeof(STARTUPINFO)), flags = 0x100, stdout = file, stderr = file };
    PROCESS_INFORMATION pi;
    if (!CreateProcess(null, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, true, 0x00000008, IntPtr.Zero, cwd, ref si, out pi))
      throw new System.ComponentModel.Win32Exception();
    WaitForSingleObject(pi.process, 0xFFFFFFFF);
    uint code; GetExitCodeProcess(pi.process, out code);
    CloseHandle(pi.process); CloseHandle(pi.thread); CloseHandle(file);
    return (int)code;
  }
  public static string QuoteArgument(string value) {
    if (value.Length > 0 && value.IndexOfAny(new char[] { ' ', '\t', '\n', '\v', '"' }) < 0) return value;
    var result = new StringBuilder();
    result.Append('"');
    int slashes = 0;
    foreach (char ch in value) {
      if (ch == '\\') { slashes++; continue; }
      if (ch == '"') {
        result.Append('\\', slashes * 2 + 1);
        result.Append('"');
        slashes = 0;
        continue;
      }
      result.Append('\\', slashes);
      slashes = 0;
      result.Append(ch);
    }
    result.Append('\\', slashes * 2);
    result.Append('"');
    return result.ToString();
  }
  public static string[] Stop() {
    PostThreadMessage(LoopThread, 0x0012, IntPtr.Zero, IntPtr.Zero);
    Worker.Join(2000);
    lock (Gate) { return Seen.ToArray(); }
  }
}
'@

$repo = (git rev-parse --show-toplevel).Trim()
if (-not $Command -or $Command.Count -eq 0) {
  $out = Join-Path $env:TEMP 'gh-nowindow-feedback.json'
  $python = (Get-Command python.exe, py.exe -ErrorAction SilentlyContinue | Select-Object -First 1).Source
  $Command = @($python, (Join-Path $repo 'tools/reached-tests/run_reached.py'), '--repo', $repo,
    '--base', $Base, '--head', 'HEAD', '--root', $Root, '--lane', $Lane, '--output', $out)
}
if ($Exe) {
  $Command = @($Exe) + @($Arguments)
}
if (-not $Command -or $Command.Count -eq 0) { throw 'Provide -Exe and -Arguments, or a command after the observer options.' }
Write-Output ("observing: " + ($Command -join ' '))
[ConsoleWindowWatch]::Start()
$started = Get-Date
$log = Join-Path $env:TEMP 'gh-nowindow-observe.log'
$line = ($Command | ForEach-Object { [ConsoleWindowWatch]::QuoteArgument($_) }) -join ' '
$exit = [ConsoleWindowWatch]::RunDetached($line, $repo, $log)
Get-Content $log -Tail 20
Start-Sleep -Milliseconds 500
$windows = [ConsoleWindowWatch]::Stop()
$seconds = [math]::Round(((Get-Date) - $started).TotalSeconds, 1)
$windows | ForEach-Object { Write-Output "  shown: $_" }
Write-Output "command exit: $exit; seconds: $seconds; console windows shown: $($windows.Count)"
if ($windows.Count -gt 0) { Write-Output 'FAIL: console windows opened during the run'; exit 1 }
if ($exit -ne 0) { Write-Output 'ERROR: observed command exited non-zero'; exit 1 }
Write-Output 'Coverage: new top-level console windows only; new tabs in an existing Windows Terminal were not observed.'
Write-Output 'PASS: no console window opened'
exit 0
