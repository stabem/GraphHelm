"""Fail-closed checks for archived test-proof replay evidence."""

import importlib.util
import sys
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).with_name("proof_sensitivity.py")
spec = importlib.util.spec_from_file_location("proof_sensitivity", MODULE_PATH)
assert spec and spec.loader
replay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(replay)


def valid_rows():
    return [
        {
            "arm": arm,
            "parent_exit": 1,
            "parent_failed_for_expected_reason": True,
            "patched_exit": 0,
            **({"parent_exit_with_python_utf8_mode": 0} if arm == "a" else {}),
        }
        for arm in ("a", "b", "c")
    ]


@pytest.mark.parametrize(
    ("arm", "field", "value", "message"),
    [
        ("a", "parent_exit", 0, "parent test did not fail"),
        ("b", "parent_failed_for_expected_reason", False, "parent failed for the wrong reason"),
        ("c", "patched_exit", 1, "patched test did not pass"),
        ("a", "parent_exit_with_python_utf8_mode", 1, "false green was not reproduced"),
    ],
)
def test_invalid_observation_cannot_become_valid(arm, field, value, message):
    rows = valid_rows()
    next(row for row in rows if row["arm"] == arm)[field] = value
    assert message in " ".join(replay.proof_issues(rows, windows=True))


def test_invalid_replay_exits_nonzero(monkeypatch, capsys):
    monkeypatch.setattr(sys, "argv", ["proof_sensitivity.py"])
    monkeypatch.setattr(replay, "replay", lambda _scratch: {"status": "INVALID"})
    assert replay.main() == 2
    assert '"status": "INVALID"' in capsys.readouterr().out
