"""Observe scheduler action repair from a disposable worktree without changing a real task."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
SCRIPT = Path(__file__).with_name('register_main_whole_run.ps1')


@unittest.skipUnless(os.name == 'nt', 'Windows Task Scheduler contract')
class ScheduledMain(unittest.TestCase):
    def test_action_survives_removal_of_the_calling_worktree(self):
        with tempfile.TemporaryDirectory(prefix='main check ') as temp:
            root = Path(temp)
            primary, lane = root / 'primary', root / 'disposable'
            primary.mkdir()
            def git(*args):
                return subprocess.run(['git', '-C', str(primary), *args], check=True,
                                      capture_output=True, text=True, timeout=30, creationflags=NO_WINDOW)
            git('init')
            runner = primary / 'tools/reached-tests/main_whole_run.py'
            runner.parent.mkdir(parents=True)
            runner.write_text('import json, sys\nprint(json.dumps(sys.argv[1:]))\n', encoding='utf-8')
            git('add', '.')
            git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                '-c', 'core.hooksPath=NUL', 'commit', '-m', 'fixture')
            git('worktree', 'add', '--detach', str(lane))
            recorder = root / 'observe.ps1'
            # Substitute only the OS scheduler boundary. Git and path resolution are real.
            recorder.write_text(r'''param($Script, $Repo, $Pythonw)
Import-Module ScheduledTasks
function Get-ScheduledTask { [pscustomobject]@{State='Ready'; Actions=@('old')} }
function Set-ScheduledTask { param($TaskName, $Action)
    $Action | Select-Object Execute, Arguments, WorkingDirectory | ConvertTo-Json -Compress | Write-Host }
& $Script -Repo $Repo -Pythonw $Pythonw -RunnerArguments @('--root', 'D:/slot root', '--timeout', '42', '--state', 'D:\a b\', '--github', 'o/"r')
''', encoding='utf-8')
            done = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-File', str(recorder),
                                   str(SCRIPT), str(lane), shutil.which('pythonw.exe')],
                                  capture_output=True, text=True, timeout=60, creationflags=NO_WINDOW)
            self.assertEqual(done.returncode, 0, done.stderr)
            action = json.loads(done.stdout)
            git('worktree', 'remove', str(lane))
            self.assertEqual(Path(action['WorkingDirectory']), primary)
            self.assertNotIn(str(lane), action['Arguments'])
            # Windows command-line parsing and the selected script are exercised by a real child.
            result = subprocess.run('"' + action['Execute'].replace('pythonw.exe', 'python.exe') + '" ' + action['Arguments'],
                                    cwd=action['WorkingDirectory'], capture_output=True, text=True,
                                    timeout=30, creationflags=NO_WINDOW)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout),
                             ['--repo', str(primary), '--root', 'D:/slot root', '--timeout', '42',
                              '--state', 'D:\\a b\\', '--github', 'o/"r'])

    def test_refusals_and_whatif_do_not_write_a_task(self):
        with tempfile.TemporaryDirectory(prefix='schedule refusal ') as temp:
            root = Path(temp)
            subprocess.run(['git', 'init', str(root)], check=True, capture_output=True,
                           timeout=30, creationflags=NO_WINDOW)
            runner = root / 'tools/reached-tests/main_whole_run.py'
            runner.parent.mkdir(parents=True)
            runner.write_text('raise SystemExit(99)\n', encoding='utf-8')
            recorder = root / 'observe.ps1'
            recorder.write_text('''param($Script, $Repo, $Pythonw, $Mode)
$ErrorActionPreference = 'Stop'
Import-Module ScheduledTasks
function Get-ScheduledTask {
    if ($Mode -eq 'missing') { throw 'missing task' }
    if ($Mode -eq 'running') { return [pscustomobject]@{State='Running'; Actions=@('old')} }
    if ($Mode -eq 'multiple') { return [pscustomobject]@{State='Ready'; Actions=@('a','b')} }
    [pscustomobject]@{State='Ready'; Actions=@('old')}
}
function Set-ScheduledTask { throw 'UNEXPECTED TASK WRITE' }
$params = @{Repo=$Repo; Pythonw=$Pythonw; RunnerArguments=@('--timeout','42')}
if ($Mode -eq 'override') { $params.RunnerArguments=@('--rep=disposable') }
if ($Mode -eq 'console') { $params.Pythonw=$Pythonw.Replace('pythonw.exe','python.exe') }
if ($Mode -eq 'whatif') { $params.WhatIf=$true }
try { & $Script @params; if ($Mode -ne 'whatif') { throw 'UNEXPECTED SUCCESS' } }
catch {
    if ($Mode -eq 'whatif' -or $_ -match 'UNEXPECTED') { Write-Error $_; exit 1 }
    Write-Host $_.Exception.Message
}
''', encoding='utf-8')
            reasons = {'override': 'repository is selected', 'console': 'Use pythonw.exe',
                       'missing': 'missing task', 'running': 'main check is running',
                       'multiple': 'one existing task action', 'whatif': 'What if:',
                       'runner': 'runner is missing', 'repo': 'Cannot find the primary checkout'}
            for mode, reason in reasons.items():
                with self.subTest(mode=mode):
                    if mode == 'runner':
                        runner.unlink()
                    done = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-File',
                                           str(recorder), str(SCRIPT), str(root if mode != 'repo' else root / 'absent'),
                                           shutil.which('pythonw.exe'), mode], capture_output=True, text=True,
                                          timeout=60, creationflags=NO_WINDOW)
                    self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
                    self.assertIn(reason, done.stdout)


if __name__ == '__main__':
    unittest.main()
