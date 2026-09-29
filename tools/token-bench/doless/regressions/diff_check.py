"""Regression observer for tasks whose reached code has no test suite (docs, formatting, strings):
`git diff --check` over everything changed since the checkout's baseline commit (committed or
not), plus `rustfmt --check` for each Rust file named on the command line. Both are pre-existing
tools; this file only aims them at the session's diff. Run from the checkout."""
import subprocess
import sys


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(list(args), capture_output=True, text=True, encoding="utf-8", errors="replace")


base = run("git", "rev-list", "--max-parents=0", "HEAD").stdout.strip()
if not base:
    print("FAIL: no baseline commit")
    raise SystemExit(1)
check = run("git", "diff", "--check", base)
if check.returncode != 0:
    print("FAIL: git diff --check\n" + check.stdout[-800:])
    raise SystemExit(1)
for path in sys.argv[1:]:
    fmt = run("rustfmt", "+1.97.1", "--edition", "2024", "--check", path)
    if fmt.returncode != 0:
        print(f"FAIL: rustfmt --check {path}\n" + (fmt.stdout + fmt.stderr)[-800:])
        raise SystemExit(1)
print("PASS")
