#!/usr/bin/env python3
"""Replay task 1279's agent-authored tests against the frozen defect and patches.

This checks test sensitivity, not the effect of a test-writing methodology.
It never starts an agent or modifies the repository checkout.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path


REPO = Path(__file__).resolve().parents[2]
PARENT = "e85e41a8eb0b8af6ef6eac95bf39a42cc9277f8c"
PATCH_DIR = "docs/keel/benchmark-evidence/task-1279-v11"
FILES = ("run.py", "test_run.py", "tasks.json")
CASES = {
    "a": (
        "4ce61ca8a045acb05baa5406ba3914d90bc21b02bb1a40f696cfb919863ca712",
        "test_run_agent_sends_unicode_prompt_on_legacy_windows_code_page",
        "UnicodeEncodeError",
        "real_child_on_windows",
    ),
    "b": (
        "8b0ddf1f24ccba3b96496e94406a364d6ed639deeed1c32cde48b34bd2cb286c",
        "test_run_agent_sends_prompt_as_utf8_regardless_of_host_locale",
        "KeyError",
        "mocked_subprocess_arguments",
    ),
    "c": (
        "09824f6d0a38f101df89a5f3fb210abd430c7f0cb3a71e5a3212eb9d3be6dd31",
        "test_run_agent_encodes_prompt_as_utf8_not_locale_default",
        "AssertionError",
        "mocked_subprocess_arguments",
    ),
}


def checked_output(command: list[str], *, cwd: Path) -> bytes:
    return subprocess.run(
        command, cwd=cwd, check=True, capture_output=True, timeout=60
    ).stdout


def run_test(directory: Path, test: str, utf8_mode: str) -> dict[str, object]:
    environment = os.environ.copy()
    environment["PYTHONUTF8"] = utf8_mode
    started = time.perf_counter()
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", f"test_run.py::{test}"],
        cwd=directory,
        env=environment,
        capture_output=True,
        text=True,
        errors="replace",
        timeout=60,
    )
    output = result.stdout + result.stderr
    if "No module named pytest" in output or "no tests ran" in output:
        raise RuntimeError(f"pytest could not run {test}: {output[-500:]}")
    return {
        "exit_code": result.returncode,
        "elapsed_seconds": round(time.perf_counter() - started, 3),
        "output": output,
    }


def proof_issues(rows: list[dict[str, object]], *, windows: bool) -> list[str]:
    """Refuse an apparently green replay that never observed the intended defect."""
    issues = []
    for row in rows:
        arm = row["arm"]
        if row["parent_exit"] == 0:
            issues.append(f"{arm}: parent test did not fail")
        if not row["parent_failed_for_expected_reason"]:
            issues.append(f"{arm}: parent failed for the wrong reason")
        if row["patched_exit"] != 0:
            issues.append(f"{arm}: patched test did not pass")
        if arm == "a" and windows and row.get("parent_exit_with_python_utf8_mode") != 0:
            issues.append("a: UTF-8-mode false green was not reproduced")
    return issues


def replay(scratch_root: Path) -> dict[str, object]:
    scratch_root = scratch_root.resolve()
    if scratch_root == REPO or REPO in scratch_root.parents:
        raise ValueError("scratch root must be outside the repository")
    scratch_root.mkdir(parents=True, exist_ok=True)

    parent_files = {
        name: checked_output(
            ["git", "show", f"{PARENT}:tools/token-bench/{name}"], cwd=REPO
        )
        for name in FILES
    }
    rows: list[dict[str, object]] = []
    with tempfile.TemporaryDirectory(prefix="proof-sensitivity-", dir=scratch_root) as temp:
        temporary_root = Path(temp)
        for arm, (digest, test, red_marker, boundary) in CASES.items():
            patch_bytes = checked_output(
                ["git", "show", f"HEAD:{PATCH_DIR}/{arm}.patch"], cwd=REPO
            )
            if hashlib.sha256(patch_bytes).hexdigest() != digest:
                raise ValueError(f"archived {arm} patch hash changed")
            patch = temporary_root / f"{arm}.patch"
            patch.write_bytes(patch_bytes)

            green_root = temporary_root / arm / "green"
            red_root = temporary_root / arm / "red"
            for root in (green_root, red_root):
                source_dir = root / "tools/token-bench"
                source_dir.mkdir(parents=True)
                for name, content in parent_files.items():
                    (source_dir / name).write_bytes(content)

            subprocess.run(
                ["git", "apply", "--check", str(patch)],
                cwd=green_root,
                check=True,
                capture_output=True,
                timeout=60,
            )
            subprocess.run(
                ["git", "apply", str(patch)],
                cwd=green_root,
                check=True,
                capture_output=True,
                timeout=60,
            )
            red_dir = red_root / "tools/token-bench"
            green_dir = green_root / "tools/token-bench"
            (red_dir / "test_run.py").write_bytes((green_dir / "test_run.py").read_bytes())

            red = run_test(red_dir, test, "0")
            green = run_test(green_dir, test, "0")
            row = {
                "arm": arm,
                "patch_sha256": digest,
                "test": test,
                "observer_boundary": boundary,
                "parent_exit": red["exit_code"],
                "parent_seconds": red["elapsed_seconds"],
                "parent_failed_for_expected_reason": re.search(
                    rf"(?m)^E\s+{re.escape(red_marker)}:", str(red["output"])
                )
                is not None,
                "patched_exit": green["exit_code"],
                "patched_seconds": green["elapsed_seconds"],
            }
            if arm == "a" and sys.platform == "win32":
                utf8_parent = run_test(red_dir, test, "1")
                row["parent_exit_with_python_utf8_mode"] = utf8_parent["exit_code"]
                row["parent_utf8_seconds"] = utf8_parent["elapsed_seconds"]
            rows.append(row)

    issues = proof_issues(rows, windows=sys.platform == "win32")
    return {
        "historical_parent": PARENT,
        "host": platform.platform(),
        "python": platform.python_version(),
        "pytest_invocations": 7 if sys.platform == "win32" else 6,
        "total_pytest_seconds": round(
            sum(
                float(row["parent_seconds"])
                + float(row["patched_seconds"])
                + float(row.get("parent_utf8_seconds", 0))
                for row in rows
            ),
            3,
        ),
        "rows": rows,
        "status": "VALID" if not issues else "INVALID",
        "validation_issues": issues,
        "interpretation": "RED/GREEN alone does not establish boundary or platform coverage",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--scratch-root",
        type=Path,
        default=Path(tempfile.gettempdir()),
        help="outside-repository directory for disposable exact-parent copies",
    )
    args = parser.parse_args()
    result = replay(args.scratch_root)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["status"] == "VALID" else 2


if __name__ == "__main__":
    raise SystemExit(main())
