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


def _processed(counts: dict[str, int], *, cli: str) -> int:
    if cli == "claude":
        return counts["input"] + counts["cacheRead"] + counts["cacheWrite"] + counts["output"]
    if cli == "codex":
        return counts["input"] + counts["output"]
    raise ValueError("unsupported native provider")


def _attempt(path: Path, *, cli: str, project: Path) -> dict:
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
        import native_session_evidence as evidence
        root_log = Path(archive["archivedRoot"])
        if cli == "claude":
            native_stream = evidence.records(stdout)
            finals = [row for row in native_stream if row.get("type") == "result"]
            if len(finals) != 1 or finals[0].get("session_id") != archive.get("rootSession"):
                raise ValueError("Claude root session does not match native stream")
            native_root = evidence.records(root_log)
            metadata = [row for row in native_root if row.get("type") in {"user", "assistant"}]
            if not metadata or any(row.get("sessionId") != archive["rootSession"]
                                   or not Path(row.get("cwd", "")).resolve().is_relative_to(project.resolve())
                                   or row.get("version") != "2.1.310"
                                   or row.get("isSidechain") is not False
                                   or row.get("agentId") is not None for row in metadata):
                raise ValueError("Claude archived root provenance differs")
            children = sorted(archive_root.glob("agent-*.jsonl"))
            replay = (evidence.claude_owned_tree(stdout, children, project=project,
                                                 session_id=archive["rootSession"])
                      if children else evidence.claude_pipeline(stdout))
        elif cli == "codex":
            import methodology as base
            native_stream = evidence.records(stdout)
            starts = [row for row in native_stream if row.get("type") == "thread.started"]
            if len(starts) != 1 or starts[0].get("thread_id") != archive.get("rootSession"):
                raise ValueError("Codex root thread does not match native stream")
            wire = receipt["wire"]["counts"]
            parsed_wire = base.usage("codex", stdout.read_text(encoding="utf-8"))
            if parsed_wire.get("accountingStatus") != "observed" or parsed_wire.get("counts") != {
                "input_tokens": wire["input"], "output_tokens": wire["output"],
                "cached_input_tokens": wire["cacheRead"]}:
                raise ValueError("Codex native stdout counters differ from receipt")
            replay = evidence.codex_tree(root_log, sorted(archive_root.glob("child-*.jsonl")),
                project=project, root_thread=archive["rootSession"],
                stdout_usage=evidence.counters({"input_tokens": wire["input"],
                    "output_tokens": wire["output"],
                    "cached_input_tokens": wire["cacheRead"],
                    "cache_write_input_tokens": wire["cacheWrite"]}))
        else:
            raise ValueError("unsupported native provider")
        if replay.get("status") != "observed" or _counts(replay.get("total")) != counts:
            raise ValueError("raw native counters do not reconcile with archive total")
        return {"attempt": path.name, "status": receipt.get("status", "unknown"),
                "rootSession": archive.get("rootSession"), "counts": counts,
                "receiptSha256": _digest(receipt_path)}
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
        return {"attempt": path.name, "status": "unknown", "reason": str(error)}


def summarize(runs_root: Path, *, prefix: str, cli: str, project: Path) -> dict:
    """Count all matching attempted sessions; never silently omit a retry or failed phase."""
    if not runs_root.is_dir() or not prefix or cli not in {"claude", "codex"} or not project.is_dir():
        raise ValueError("runs, attempt prefix, provider and existing project are required")
    prefixes = prefix.split(",")
    paths = sorted(path for path in runs_root.iterdir()
                   if path.is_dir() and (prefix == "*" or any(
                       path.name == item or path.name.startswith(item if item.endswith("-")
                                                                 else item + "-")
                       for item in prefixes)))
    if not paths:
        raise ValueError("no matching native attempts")
    rows = [_attempt(path, cli=cli, project=project) for path in paths]
    roots = [row.get("rootSession") for row in rows]
    known = (all(row.get("counts") is not None and isinstance(row.get("rootSession"), str)
                 and row["rootSession"] for row in rows) and len(roots) == len(set(roots)))
    total = {key: sum(row["counts"][key] for row in rows) for key in COUNTERS} if known else None
    processed = _processed(total, cli=cli) if total is not None else None
    return {"schema": "native-pilot-ledger/1", "runsRoot": str(runs_root.resolve()),
            "prefix": prefix, "cli": cli, "projectRoot": str(project.resolve()),
            "attempts": rows, "accountingStatus": "observed" if known else "unknown",
            "counts": total, "totalProcessedTokens": processed,
            "methodComplete": False, "winner": None}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runs_root", type=Path)
    parser.add_argument("--prefix", required=True)
    parser.add_argument("--cli", choices=("claude", "codex"), required=True)
    parser.add_argument("--project", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = summarize(args.runs_root, prefix=args.prefix, cli=args.cli,
                       project=args.project)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return 0 if result["accountingStatus"] == "observed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
