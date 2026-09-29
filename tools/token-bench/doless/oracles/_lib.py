"""Shared helpers for the do-less oracles. An oracle is run as `python <oracle> <checkout>` from
outside the checkout; `DOLESS_ANSWER` names a file holding the agent's final answer. Exit 0 means
the task's promise holds; anything else means it does not, with the reason on stdout."""
from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
from pathlib import Path

WT = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path.cwd()


def fail(message: str) -> None:
    print(f"FAIL: {message}")
    raise SystemExit(1)


def ok(message: str = "promise holds") -> None:
    print(f"PASS: {message}")
    raise SystemExit(0)


def read(path: str) -> str:
    target = WT / path
    if not target.is_file():
        fail(f"{path} is missing")
    return target.read_text(encoding="utf-8")


def answer() -> str:
    raw = os.environ.get("DOLESS_ANSWER")
    return Path(raw).read_text(encoding="utf-8") if raw and Path(raw).is_file() else ""


def load_runner():
    """The checkout's own tools/token-bench/run.py, imported fresh."""
    path = WT / "tools" / "token-bench" / "run.py"
    spec = importlib.util.spec_from_file_location(f"runner_under_test_{os.getpid()}", path)
    module = importlib.util.module_from_spec(spec)
    sys.path.insert(0, str(path.parent))
    spec.loader.exec_module(module)
    return module


def section(text: str, heading: str) -> str:
    """The body under a Markdown heading, up to the next heading of the same or a higher level."""
    start = text.find(heading)
    if start < 0:
        fail(f"heading {heading!r} not found")
    level = len(heading) - len(heading.lstrip("#"))
    body = text[start + len(heading):]
    for marker in ("\n" + "#" * n + " " for n in range(1, level + 1)):
        cut = body.find(marker)
        if cut >= 0:
            body = body[:cut]
    return body


def rustfmt_check(path: str) -> None:
    proc = subprocess.run(["rustfmt", "+1.97.1", "--edition", "2024", "--check", path], cwd=WT,
                          capture_output=True, text=True, encoding="utf-8", errors="replace")
    if proc.returncode != 0:
        fail(f"rustfmt --check {path} exit {proc.returncode}: {(proc.stdout + proc.stderr)[-400:]}")


def git_changed_paths() -> list[str]:
    """Paths changed relative to the checkout's baseline commit (the task's parent)."""
    def run(*args: str) -> str:
        return subprocess.run(["git", *args], cwd=WT, capture_output=True, text=True, check=True).stdout
    base = run("rev-list", "--max-parents=0", "HEAD").strip()
    changed = run("diff", "--name-only", base) + run("ls-files", "--others", "--exclude-standard")
    return sorted({line for line in changed.splitlines() if line.strip()})


REPO = Path(__file__).resolve().parents[4]


def repo_show(sha: str, path: str) -> bytes:
    """A file as it is at `sha` in the benchmark's own repository (never the agent's checkout)."""
    return subprocess.run(["git", "show", f"{sha}:{path}"], cwd=REPO, capture_output=True, check=True).stdout


def added_rust_tests() -> int:
    """`#[test]` attributes the session added to Rust files: baseline commit to the scored snapshot."""
    def run(*args: str) -> str:
        return subprocess.run(["git", *args], cwd=WT, capture_output=True, text=True, check=True,
                              encoding="utf-8", errors="replace").stdout
    base = run("rev-list", "--max-parents=0", "HEAD").strip()
    diff = run("diff", base, "HEAD", "--", "*.rs")  # the scored snapshot; regression restores files
    return sum(1 for line in diff.splitlines() if line.startswith("+") and line[1:].strip() == "#[test]")


def cargo_test(package: str, test_target: str, name: str, timeout: int = 1500) -> None:
    """Run one named integration test in the checkout; fail unless it ran and passed."""
    try:
        proc = subprocess.run(["cargo", "test", "-q", "-p", package, "--test", test_target, name, "--",
                               "--exact"], cwd=WT, capture_output=True, text=True, encoding="utf-8",
                              errors="replace", timeout=timeout)
    except subprocess.TimeoutExpired:
        fail(f"cargo test {name} timed out after {timeout}s")
    out = proc.stdout + proc.stderr
    if proc.returncode != 0 or "1 passed" not in out:
        fail(f"hidden test {name} did not pass (exit {proc.returncode}): {out[-600:]}")


def hidden_unittest(sha: str, path: str, extra: str = "", timeout: int = 600) -> None:
    """Run the unittest file `path` as it is at `sha` (normally the task's known fix) against the
    checkout's code: the file is written beside the original under a hidden name, so it imports the
    checkout's modules, and removed afterwards. `extra` is test code inserted before the file's
    `if __name__ == "__main__":` guard. Fail unless every test ran and passed."""
    text = repo_show(sha, path).decode("utf-8")
    guard = 'if __name__ == "__main__":'
    if extra:
        if guard not in text:
            fail(f"{path} at {sha[:8]} has no main guard to extend")
        text = text.replace(guard, extra.rstrip() + "\n\n\n" + guard, 1)
    target = WT / Path(path).with_name("_doless_hidden_" + Path(path).name)
    target.write_text(text, encoding="utf-8")
    try:
        proc = subprocess.run([sys.executable, str(target)], cwd=WT, capture_output=True, text=True,
                              encoding="utf-8", errors="replace", timeout=timeout)
    except subprocess.TimeoutExpired:
        fail(f"hidden {path} timed out after {timeout}s")
    finally:
        target.unlink(missing_ok=True)
    out = proc.stdout + proc.stderr
    if proc.returncode != 0 or "\nOK" not in out:
        fail(f"hidden {path} from {sha[:8]} did not pass (exit {proc.returncode}): {out[-700:]}")


def diff_since_base(*paths: str) -> str:
    """The unified diff from the checkout's baseline commit to the scored snapshot (HEAD)."""
    def run(*args: str) -> str:
        return subprocess.run(["git", *args], cwd=WT, capture_output=True, text=True, check=True,
                              encoding="utf-8", errors="replace").stdout
    base = run("rev-list", "--max-parents=0", "HEAD").strip()
    return run("diff", base, "HEAD", "--", *paths)
