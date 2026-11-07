# #2573: Windows test for the Node side. The hidden-helper chain must show zero windows under the
# EnumWindows observer; the detached control must still fail, so the test catches the bug.
param([string]$Node = (Get-Command node.exe -ErrorAction SilentlyContinue).Source)
$ErrorActionPreference = 'Stop'
if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) { Write-Output 'SKIPPED: Windows only'; exit 0 }
if (-not $Node) { Write-Output 'FAIL: node.exe not found'; exit 1 }
$here = Split-Path -Parent (Resolve-Path $MyInvocation.MyCommand.Path)
$observer = (Resolve-Path (Join-Path $here 'observe.ps1')).Path
$chain = (Resolve-Path (Join-Path $here 'node-chain\hidden-chain.mjs')).Path
$control = (Resolve-Path (Join-Path $here 'node-chain\detached-control.mjs')).Path
$hidden = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $observer -Exe $Node -Arguments $chain 2>&1 | Out-String
$detached = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $observer -Exe $Node -Arguments $control 2>&1 | Out-String
$hiddenOk = $hidden -match 'PASS: no console window opened'
$controlCaught = $detached -match 'FAIL: console windows opened'
if ($hiddenOk) { Write-Output 'CASE hidden-helper chain: PASS (0 windows)' } else { Write-Output 'CASE hidden-helper chain: FAIL'; Write-Output $hidden }
if ($controlCaught) { Write-Output 'CASE detached control: observer caught the window (negative control holds)' } else { Write-Output 'CASE detached control: observer did NOT catch the bug'; Write-Output $detached }
if ($hiddenOk -and $controlCaught) { Write-Output 'PASS'; exit 0 } else { Write-Output 'FAIL'; exit 1 }
