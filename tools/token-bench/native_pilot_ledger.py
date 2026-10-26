"""Reconcile every native subscription pilot attempt, including failed retries."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


COUNTERS = ("input", "output", "cacheRead", "cacheWrite")


def _digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _counts(value: object) -> dict[str, int] | None:
    if not isinstance(value, dict) or any(type(value.get(key)) is not int or value[key] < 0
                                          for key in COUNTERS):
        return None
    return {key: value[key] for key in COUNTERS}


def _attempt(path: Path) -> dict:
    receipt_path = path / "receipt.json"
    stdout = path / "native.stdout.jsonl"
    stderr = path / "native.stderr.log"
    if not all(item.is_file() and not item.is_symlink()
               for item in (receipt_path, stdout, stderr)):
        return {"attempt": path.name, "status": "unknown", "reason": "missing raw attempt evidence"}
    try:
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        archive = receipt["archive"]
        counts = _counts(archive.get("total"))
        if counts is None or archive.get("status") != "observed":
            raise ValueError("archived token accounting is incomplete")
        if receipt.get("stdoutSha256") != _digest(stdout) or receipt.get("stderrSha256") != _digest(stderr):
            raise ValueError("raw attempt hash differs from receipt")
        archive_root = Path(archive["archivedRoot"]).parent
        if not isinstance(archive.get("archive"), dict) or not archive["archive"]:
            raise ValueError("owned session archive is missing")
        if any(not (archive_root / name).is_file() or _digest(archive_root / name) != digest
               for name, digest in archive["archive"].items()):
            raise ValueError("owned session archive hash differs")
        return {"attempt": path.name, "status": receipt.get("status", "unknown"),
                "rootSession": archive.get("rootSession"), "counts": counts,
                "receiptSha256": _digest(receipt_path)}
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
        return {"attempt": path.name, "status": "unknown", "reason": str(error)}


def summarize(runs_root: Path, *, prefix: str) -> dict:
    """Count all matching attempted sessions; never silently omit a retry or failed phase."""
    if not runs_root.is_dir() or not prefix:
        raise ValueError("a runs directory and nonempty attempt prefix are required")
    prefixes = prefix.split(",")
    paths = sorted(path for path in runs_root.iterdir()
                   if path.is_dir() and (prefix == "*" or any(
                       path.name == item or path.name.startswith(item if item.endswith("-")
                                                                 else item + "-")
                       for item in prefixes)))
    if not paths:
        raise ValueError("no matching native attempts")
    rows = [_attempt(path) for path in paths]
    roots = [row.get("rootSession") for row in rows]
    known = (all(row.get("counts") is not None and isinstance(row.get("rootSession"), str)
                 and row["rootSession"] for row in rows) and len(roots) == len(set(roots)))
    total = {key: sum(row["counts"][key] for row in rows) for key in COUNTERS} if known else None
    processed = (total["input"] + total["cacheRead"] + total["cacheWrite"] + total["output"]
                 if total is not None else None)
    return {"schema": "native-pilot-ledger/1", "runsRoot": str(runs_root.resolve()),
            "prefix": prefix, "attempts": rows, "accountingStatus": "observed" if known else "unknown",
            "counts": total, "totalProcessedTokens": processed,
            "methodComplete": False, "winner": None}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runs_root", type=Path)
    parser.add_argument("--prefix", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = summarize(args.runs_root, prefix=args.prefix)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return 0 if result["accountingStatus"] == "observed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
