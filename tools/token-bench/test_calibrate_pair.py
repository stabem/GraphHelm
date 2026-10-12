"""Offline calibration tests for paired task inference."""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path


HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "calibrate_pair.py"
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def run_calibration(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        capture_output=True,
        text=True,
        creationflags=NO_WINDOW,
        check=False,
    )


def calibration_report() -> dict:
    result = run_calibration("--seed", "183", "--cohorts", "40", "--task-counts", "6")
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


def test_calibration_null_noise_stays_below_preregistered_support_bound():
    """The existing decision must not often support savings from equal-distribution noise."""
    report = calibration_report()
    null = report["scenarios"]["null"][0]
    assert null["taskCount"] == 6
    assert null["verdictCounts"]["supported"] <= 4  # pre-registered: at most 4 of 40
    assert len(null["seeds"]) == report["cohorts"] == 40
    assert set(null["verdictCounts"]) == {
        "supported", "inconclusive", "quality_regression", "not_computable"
    }


def test_calibration_large_effect_is_supported_often():
    """The same rule should detect the declared large reduction with unchanged quality."""
    report = calibration_report()
    effect = report["scenarios"]["effect"][0]
    assert effect["taskCount"] == 6
    assert effect["verdictCounts"]["supported"] >= 30  # pre-registered: at least 30 of 40
    assert len(effect["seeds"]) == report["cohorts"] == 40
    assert report["thresholds"]["effectCostFactor"] < 1


def test_calibration_is_byte_deterministic_for_a_fixed_seed():
    args = ("--seed", "183", "--cohorts", "40", "--task-counts", "6")
    first = run_calibration(*args)
    second = run_calibration(*args)
    assert first.returncode == second.returncode == 0
    assert first.stdout.encode() == second.stdout.encode()


def test_calibration_refuses_more_than_two_hundred_cohorts():
    result = run_calibration("--cohorts", "201")
    assert result.returncode == 1
    assert result.stdout == ""
    assert "200" in result.stderr


def test_calibration_refuses_too_many_or_duplicate_task_counts():
    too_many = run_calibration("--task-counts", *("1" for _ in range(13)))
    assert too_many.returncode == 1
    assert too_many.stdout == ""
    assert "at most 12 entries" in too_many.stderr

    duplicate = run_calibration("--task-counts", "6", "6")
    assert duplicate.returncode == 1
    assert duplicate.stdout == ""
    assert "must be unique" in duplicate.stderr
