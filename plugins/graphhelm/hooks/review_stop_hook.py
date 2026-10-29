"""Stop gate (run in-process by `stop_hook.py`): before a Keel-carded change ends, the author reviews its own diff once.

When the project has a Keel card and an uncommitted code diff, and no self-review record written
after that card exists, the first Stop is blocked with the review checklist. The author answers by
writing .graphhelm/self-review.json; the next Stop passes. A record older than the card (left by an
earlier task in a reused checkout) counts as missing. A Stop that is already a hook continuation
always passes, so the hook can never loop.
"""
from pathlib import Path
import subprocess

NO_WINDOW = getattr(subprocess, 'CREATE_NO_WINDOW', 0)
REASON = (
    'Self-review before stopping. Read your own `git diff` as the blind reviewer will, then fix '
    'every major you find and rerun the tests:\n'
    '1. Boundaries: list every value your validation now admits at its edges (empty, maximum length, '
    'path-special values such as dot segments, whitespace, a BOM, a link or directory where a file is '
    'expected). Run each through the real code path and check the result still meets the request.\n'
    '2. Compatibility: run the existing test files of every module you changed; check every existing '
    'caller and default you changed.\n'
    '3. Refusals: a refused input changes nothing and sends nothing; a test proves it.\n'
    '4. Tests: each new test drives the real code path, not a value the test built.\n'
    'Then write .graphhelm/self-review.json with {"checked": [{"input": ..., "result": ..., "fixed": true|false}], '
    '"testsRun": [...]} and stop. A record older than the current Keel card does not count.')


def diff_present(root):
    try:
        out = subprocess.run(['git', '-C', str(root), 'status', '--porcelain', '--', '.', ':!.graphhelm'],
                             capture_output=True, text=True, timeout=20, creationflags=NO_WINDOW).stdout
    except (OSError, subprocess.SubprocessError):
        return False
    return any(line.strip() for line in out.splitlines())


def decide(event, root):
    if event.get('stop_hook_active'):
        return None
    card = root / '.graphhelm/keel-card.json'
    if not card.is_file():
        return None
    record = root / '.graphhelm/self-review.json'
    if record.is_file() and record.stat().st_mtime_ns > card.stat().st_mtime_ns:
        return None
    if not diff_present(root):
        return None
    return {'decision': 'block', 'reason': REASON}


def evaluate(payload):
    """The gate's reason for one Stop payload, or None; a broken check never blocks."""
    if not isinstance(payload, dict) or not payload.get('cwd'):
        return None
    try:
        result = decide(payload, Path(payload['cwd']))
    except Exception:  # noqa: BLE001 - a broken gate never traps the turn
        return None
    return result and result['reason']
