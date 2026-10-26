"""The native pilot ledger must retain failed retries and refuse missing usage."""
import hashlib
import json
from pathlib import Path
import tempfile
import uuid
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

import native_pilot_ledger as ledger
import native_session_evidence as evidence


def _write_attempt(root: Path, name: str, session: str) -> None:
    stage = root / name
    stage.mkdir()
    stdout = stage / "native.stdout.jsonl"
    stderr = stage / "native.stderr.log"
    session_id = str(uuid.uuid5(uuid.NAMESPACE_DNS, session))
    fixture = Path(__file__).parent / "fixtures/native-sessions/claude-child.jsonl"
    rows = [json.loads(line) for line in fixture.read_text(encoding="utf-8").splitlines()]
    next(row for row in rows if row.get("type") == "result")["session_id"] = session_id
    stdout.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
    stderr.write_text("", encoding="utf-8")
    archive = stage / "archive"
    archive.mkdir()
    (archive / "root.jsonl").write_text(json.dumps({"type": "user", "sessionId": session_id,
        "cwd": str(root), "isSidechain": False, "agentId": None}) + "\n", encoding="utf-8")
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    (stage / "receipt.json").write_text(json.dumps({
        "status": "incomplete" if "retry" not in name else "observed",
        "stdoutSha256": digest(stdout), "stderrSha256": digest(stderr),
        "archive": {"status": "observed", "rootSession": session_id,
                    "archivedRoot": str(archive / "root.jsonl"),
                    "archive": {"root.jsonl": digest(archive / "root.jsonl")},
                    "total": evidence.claude_pipeline(stdout)["total"]}}), encoding="utf-8")


def test_failed_attempt_and_retry_both_contribute_tokens():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        _write_attempt(root, "propose", "one")
        _write_attempt(root, "propose-retry1", "two")
        result = ledger.summarize(root, prefix="propose", cli="claude", project=root)
        assert result["accountingStatus"] == "observed"
        assert len(result["attempts"]) == 2
        assert result["totalProcessedTokens"] == 26860
        (root / "propose-retry1/receipt.json").unlink()
        incomplete = ledger.summarize(root, prefix="propose", cli="claude", project=root)
        assert incomplete["accountingStatus"] == "unknown"
        assert incomplete["totalProcessedTokens"] is None


def test_forged_archive_total_cannot_be_counted():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        _write_attempt(root, "review", "one")
        receipt = root / "review/receipt.json"
        data = json.loads(receipt.read_text(encoding="utf-8"))
        data["archive"]["total"]["output"] += 1
        receipt.write_text(json.dumps(data), encoding="utf-8")
        result = ledger.summarize(root, prefix="review", cli="claude", project=root)
        assert result["accountingStatus"] == "unknown"
        assert result["totalProcessedTokens"] is None


def test_copied_native_session_cannot_be_reidentified_as_a_retry():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        _write_attempt(root, "propose", "one")
        _write_attempt(root, "propose-retry1", "one")
        forged = root / "propose-retry1/receipt.json"
        data = json.loads(forged.read_text(encoding="utf-8"))
        data["archive"]["rootSession"] = str(uuid.uuid5(uuid.NAMESPACE_DNS, "two"))
        forged.write_text(json.dumps(data), encoding="utf-8")
        result = ledger.summarize(root, prefix="propose", cli="claude", project=root)
        assert result["accountingStatus"] == "unknown"
        assert result["totalProcessedTokens"] is None


def test_codex_cache_is_a_subset_of_input():
    counts = {"input": 100, "output": 20, "cacheRead": 80, "cacheWrite": 0}
    assert ledger._processed(counts, cli="codex") == 120
    assert ledger._processed(counts, cli="claude") == 200
