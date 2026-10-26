"""The native pilot ledger must retain failed retries and refuse missing usage."""
import hashlib
import json
from pathlib import Path
import tempfile

import native_pilot_ledger as ledger


def _write_attempt(root: Path, name: str, session: str, output: int) -> None:
    stage = root / name
    stage.mkdir()
    stdout = stage / "native.stdout.jsonl"
    stderr = stage / "native.stderr.log"
    stdout.write_text("native stream\n", encoding="utf-8")
    stderr.write_text("", encoding="utf-8")
    archive = stage / "archive"
    archive.mkdir()
    (archive / "root.jsonl").write_text("owned session\n", encoding="utf-8")
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    (stage / "receipt.json").write_text(json.dumps({
        "status": "incomplete" if "retry" not in name else "observed",
        "stdoutSha256": digest(stdout), "stderrSha256": digest(stderr),
        "archive": {"status": "observed", "rootSession": session,
                    "archivedRoot": str(archive / "root.jsonl"),
                    "archive": {"root.jsonl": digest(archive / "root.jsonl")},
                    "total": {"input": 1, "output": output, "cacheRead": 3,
                              "cacheWrite": 4}}}), encoding="utf-8")


def test_failed_attempt_and_retry_both_contribute_tokens():
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        _write_attempt(root, "propose", "one", 5)
        _write_attempt(root, "propose-retry1", "two", 7)
        result = ledger.summarize(root, prefix="propose")
        assert result["accountingStatus"] == "observed"
        assert len(result["attempts"]) == 2
        assert result["totalProcessedTokens"] == 28
        (root / "propose-retry1/receipt.json").unlink()
        incomplete = ledger.summarize(root, prefix="propose")
        assert incomplete["accountingStatus"] == "unknown"
        assert incomplete["totalProcessedTokens"] is None
