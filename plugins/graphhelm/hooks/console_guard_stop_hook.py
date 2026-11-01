"""Stop gate (run in-process by `stop_hook.py`): a session that changed a .py runs the console-window guard once.

When the working tree has a changed or new `.py` and the repository ships the guard
`tools/reached-tests/test_no_console_window.py`, the gate runs it with a deadline. A red guard blocks
the Stop once with the guard's own failure lines. A Stop that is already a hook continuation always
passes, so the gate can never loop; a guard that cannot run (missing interpreter, deadline) never
blocks, because a broken check must not trap the turn.
"""
from pathlib import Path
import subprocess
import sys

NO_WINDOW = getattr(subprocess, 'CREATE_NO_WINDOW', 0)
GUARD = 'tools/reached-tests/test_no_console_window.py'
DEADLINE_SECONDS = 60
MAX_REASON_LINES = 30


def changed_python(root):
    try:
        out = subprocess.run(['git', '-C', str(root), 'status', '--porcelain', '--untracked-files=all', '--', '*.py'],
                             capture_output=True, text=True, timeout=20, creationflags=NO_WINDOW).stdout
    except (OSError, subprocess.SubprocessError):
        return False
    return any(line[3:].strip().endswith('.py') and not line.startswith(' D') for line in out.splitlines())


def run_guard(root):
    """(passed, output); passed is None when the guard could not be observed."""
    try:
        done = subprocess.run([sys.executable, '-m', 'unittest', GUARD], cwd=str(root), capture_output=True,
                              text=True, encoding='utf-8', errors='replace', timeout=DEADLINE_SECONDS,
                              creationflags=NO_WINDOW)
    except (OSError, subprocess.SubprocessError):
        return None, ''
    return done.returncode == 0, (done.stdout or '') + (done.stderr or '')


def toplevel(cwd):
    """The repository root above `cwd`, so a session started in a subdirectory is still checked."""
    try:
        out = subprocess.run(['git', '-C', str(cwd), 'rev-parse', '--show-toplevel'], capture_output=True,
                             text=True, timeout=20, creationflags=NO_WINDOW).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return None
    return Path(out) if out else None


def note(text):
    """One content-free line on stderr: the host shows it to the operator, never to the model."""
    print(f'console-guard: {text}', file=sys.stderr)


def decide(event, cwd):
    root = toplevel(cwd)
    if root is None or not (root / GUARD).is_file() or not changed_python(root):
        return None
    if event.get('stop_hook_active'):
        note('continuation stop passes without rerunning the guard')
        return None
    passed, output = run_guard(root)
    if passed is None:
        note(f'guard unobserved (did not finish within {DEADLINE_SECONDS} s); the stop passes')
        return None
    if passed:
        return None
    lines = [line for line in output.splitlines() if line.strip()][-MAX_REASON_LINES:]
    return ('The repository console-window guard fails on your change '
            f'(`python -m unittest {GUARD}`). Every child process passes '
            '`creationflags=NO_WINDOW` (AGENTS.md "No console windows"). Fix it, rerun the guard, then stop.\n'
            + '\n'.join(lines))


def evaluate(payload):
    """The gate's reason for one Stop payload, or None; a broken check never blocks."""
    if not isinstance(payload, dict) or not payload.get('cwd'):
        return None
    try:
        return decide(payload, Path(payload['cwd']))
    except Exception:  # noqa: BLE001 - a broken gate never traps the turn
        return None
