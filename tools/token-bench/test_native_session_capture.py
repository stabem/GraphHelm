"""Exercise private journal discovery and refusal of subject-controlled evidence."""
import json
from pathlib import Path

import native_session_capture as capture

FIXTURES = Path(__file__).parent / "fixtures/native-sessions"
ROOT_ID = "01a17672-2f62-7c41-b135-b94ea4c887c0"
CHILD_ID = "01a17672-46f5-7aa3-92fd-9aeab59a9f0c"
COUNTS = {"input_tokens": 65656, "cached_input_tokens": 43392, "output_tokens": 72}


def test_archive_discovers_real_native_parent_child_linkage(tmp_path):
    native = tmp_path / "native"
    day = native / "sessions/2026/10/26"
    day.mkdir(parents=True)
    project = Path("D:/gh/codex-scope-cleanup/native-2074-probes/codex-child-001")
    for name, identifier in [("codex-root", ROOT_ID), ("codex-child", CHILD_ID)]:
        rows = [json.loads(line) for line in (FIXTURES / (name + ".jsonl")).read_text().splitlines()]
        rows[0]["payload"]["timestamp"] = "2026-10-26T04:22:37Z"
        (day / ("rollout-" + identifier + ".jsonl")).write_text(
            "".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    stdout = tmp_path / "stdout.jsonl"
    stdout.write_text(json.dumps({"type": "thread.started", "thread_id": ROOT_ID}) + "\n", encoding="utf-8")
    result = capture.archive_phase("codex", stdout, project=project, archive=tmp_path / "private",
                                   root_counts=COUNTS, environment={"CODEX_HOME": str(native)})
    assert result["status"] == "observed", result
    assert result["total"]["input"] == 87212
    assert set(result["archive"]) == {"root.jsonl", "child-" + CHILD_ID + ".jsonl"}
    assert (day / ("rollout-" + CHILD_ID + ".jsonl")).is_file()


def test_subject_cannot_supply_native_home_or_receipt_archive(tmp_path):
    subject = tmp_path / "subject"
    subject.mkdir()
    stdout = tmp_path / "stdout.jsonl"
    stdout.write_text(json.dumps({"type": "thread.started", "thread_id": ROOT_ID}) + "\n", encoding="utf-8")
    result = capture.archive_phase("codex", stdout, project=subject, archive=tmp_path / "private",
                                   root_counts=COUNTS, environment={"CODEX_HOME": str(subject)})
    assert result["status"] == "unknown"
    result = capture.archive_phase("codex", stdout, project=subject, archive=subject / "forged-receipts",
                                   root_counts=COUNTS)
    assert result["status"] == "unknown"


def test_native_session_identity_cannot_be_an_output_path(tmp_path):
    stdout = tmp_path / "stdout.jsonl"
    stdout.write_text(json.dumps({"type": "thread.started", "thread_id": "../escaped"}) + "\n", encoding="utf-8")
    result = capture.archive_phase("codex", stdout, project=tmp_path / "subject", archive=tmp_path / "private",
                                   root_counts=COUNTS)
    assert result["status"] == "unknown"
    assert not (tmp_path / "escaped").exists()
