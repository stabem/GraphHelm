"""The native pilot ledger must retain failed retries and refuse missing usage."""
import hashlib
import json
from pathlib import Path
import tempfile

import native_pilot_ledger as ledger
import native_session_evidence as evidence


def _write_attempt(root: Path, name: str, session: str) -> None:
    stage = root / name
    stage.mkdir()
    stdout = stage / "native.stdout.jsonl"
    stderr = stage / "native.stderr.log"
    fixture = Path(__file__).parent / "fixtures/native-sessions/claude-child.jsonl"
    stdout.write_bytes(fixture.read_bytes())
    stderr.write_text("", encoding="utf-8")
    archive = stage / "archive"
    archive.mkdir()
    (archive / "root.jsonl").write_bytes(fixture.read_bytes())
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    (stage / "receipt.json").write_text(json.dumps({
        "status": "incomplete" if "retry" not in name else "observed",
        "stdoutSha256": digest(stdout), "stderrSha256": digest(stderr),
        "archive": {"status": "observed", "rootSession": session,
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


def test_codex_cache_is_a_subset_of_input():
    counts = {"input": 100, "output": 20, "cacheRead": 80, "cacheWrite": 0}
    assert ledger._processed(counts, cli="codex") == 120
    assert ledger._processed(counts, cli="claude") == 200
