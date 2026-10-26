"""Archive only the native sessions belonging to a captured benchmark phase."""
from __future__ import annotations

from datetime import datetime, timedelta
import json
import os
from pathlib import Path
import uuid

import methodology as base
import native_session_evidence as evidence


def _uuid(value: object) -> str:
    if not isinstance(value, str) or str(uuid.UUID(value)) != value:
        raise ValueError("noncanonical native session UUID")
    return value


def _source(source: Path, native_root: Path, project: Path) -> Path:
    resolved = source.resolve()
    if (source.is_symlink() or not resolved.is_relative_to(native_root)
            or resolved.is_relative_to(project)):
        raise ValueError("native evidence source escapes approved state or enters subject")
    return resolved


def _copy_owned(source: Path, destination: Path, *, archive: Path,
                native_root: Path, project: Path) -> Path:
    source = _source(source, native_root, project)
    if destination.resolve().parent != archive:
        raise ValueError("native archive destination escapes private archive")
    if source.is_symlink() or not source.is_file() or source.stat().st_size > 128 * 1024 * 1024:
        raise ValueError("native owned log missing, linked or oversized")
    before = source.stat()
    data = source.read_bytes()
    after = source.stat()
    if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
        raise ValueError("native session log is still changing")
    if source.is_relative_to(archive):
        raise ValueError("native log source cannot be the receipt archive")
    with destination.open("xb") as output:
        output.write(data)
    return destination


def archive_phase(cli: str, stdout: Path, *, project: Path, archive: Path,
                  root_counts: dict, environment: dict[str, str] | None = None) -> dict:
    """Derive identities from native output and archive their owned journals.

    The controller, not the subject, calls this after native capture and cleanup.
    The archive must be outside the subject project. Authentication files are
    neither read nor copied. Unknown, missing and changing journals fail closed.
    """
    try:
        project = project.resolve()
        archive = archive.resolve()
        if archive.is_relative_to(project) or project.is_relative_to(archive):
            raise ValueError("private native evidence must be disjoint from the subject")
        archive.mkdir(parents=True, exist_ok=False)
        env = os.environ if environment is None else environment
        rows = evidence.records(stdout)
        if cli == "codex":
            starts = [r for r in rows if r.get("type") == "thread.started"]
            if len(starts) != 1:
                raise ValueError("native root thread identity missing or duplicated")
            thread = _uuid(starts[0].get("thread_id"))
            home = Path(env.get("CODEX_HOME", str(Path.home() / ".codex"))).resolve()
            if home.is_relative_to(project):
                raise ValueError("subject-writable Codex state is not native evidence")
            session_root = (home / "sessions").resolve()
            if (not session_root.is_relative_to(home) or session_root.is_relative_to(project)
                    or project.is_relative_to(session_root)):
                raise ValueError("native session directory redirects outside approved home or enters subject")
            matching = list(session_root.rglob("*" + thread + ".jsonl"))
            if len(matching) != 1:
                raise ValueError("owned native Codex root journal missing or duplicated")
            root_source = _source(matching[0], session_root, project)
            with root_source.open(encoding="utf-8") as stream:
                root_meta = json.loads(stream.readline(2 * 1024 * 1024))["payload"]
            if root_meta.get("id") != thread or Path(root_meta.get("cwd", "")).resolve() != project:
                raise ValueError("native root journal belongs to another capture")
            # A session may cross midnight; inspect first-line metadata only in
            # the root day and its adjacent days, never unrelated session bodies.
            day = datetime.fromisoformat(root_meta["timestamp"].replace("Z", "+00:00")).date()
            possible = {}
            for offset in (-1, 0, 1):
                date = day + timedelta(days=offset)
                directory = session_root / date.strftime("%Y/%m/%d")
                for path in directory.glob("*.jsonl"):
                    if path == root_source or path.is_symlink():
                        continue
                    safe_path = _source(path, session_root, project)
                    with safe_path.open(encoding="utf-8") as stream:
                        first = stream.readline(2 * 1024 * 1024)
                    try:
                        metadata = json.loads(first)
                    except ValueError:
                        continue
                    if metadata.get("type") != "session_meta":
                        continue
                    meta = metadata.get("payload", {})
                    if Path(meta.get("cwd", "")).resolve() == project and isinstance(meta.get("parent_thread_id"), str):
                        identifier = _uuid(meta.get("id"))
                        _uuid(meta["parent_thread_id"])
                        if identifier in possible:
                            raise ValueError("duplicate native child journal")
                        possible[identifier] = (meta, safe_path)
            owned = {thread}
            children = []
            while True:
                found = [(identifier, value) for identifier, value in possible.items()
                         if identifier not in owned and value[0]["parent_thread_id"] in owned]
                if not found:
                    break
                for identifier, (_, path) in found:
                    owned.add(identifier)
                    children.append(_copy_owned(path, archive / ("child-" + identifier + ".jsonl"),
                                                archive=archive, native_root=session_root, project=project))
            root = _copy_owned(root_source, archive / "root.jsonl", archive=archive,
                               native_root=session_root, project=project)
            result = evidence.codex_tree(root, children, project=project, root_thread=thread,
                                         stdout_usage=evidence.counters(root_counts))
            result["rootSession"] = thread
        elif cli == "claude":
            finals = [r for r in rows if r.get("type") == "result"]
            if len(finals) != 1:
                raise ValueError("native final session identity missing or duplicated")
            session = _uuid(finals[0].get("session_id"))
            home = Path(env.get("CLAUDE_CONFIG_DIR", str(Path.home() / ".claude"))).resolve()
            if home.is_relative_to(project):
                raise ValueError("subject-writable Claude state is not native evidence")
            projects = (home / "projects").resolve()
            if (not projects.is_relative_to(home) or projects.is_relative_to(project)
                    or project.is_relative_to(projects)):
                raise ValueError("native projects directory redirects outside approved home or enters subject")
            roots = list(projects.glob("*/" + session + ".jsonl"))
            if len(roots) != 1:
                raise ValueError("owned native Claude root transcript missing or duplicated")
            root_source = _source(roots[0], projects, project)
            native_rows = evidence.records(root_source)
            metadata = [row for row in native_rows if row.get("type") in {"user", "assistant"}]
            if not metadata or any(row.get("sessionId") != session or row.get("version") != "2.1.310"
                                   or row.get("isSidechain") is not False or row.get("agentId") is not None
                                   or Path(row.get("cwd", "")).resolve() != project for row in metadata):
                raise ValueError("native Claude root transcript provenance mismatch")
            if any(row.get("session_id") != session for row in rows if row.get("type") in {"user", "assistant"}):
                raise ValueError("native Claude stream linkage session mismatch")
            root = _copy_owned(root_source, archive / "root.jsonl", archive=archive,
                               native_root=projects, project=project)
            owned_directory = roots[0].parent / session / "subagents"
            children = [_copy_owned(path, archive / path.name, archive=archive,
                                    native_root=projects, project=project)
                        for path in sorted(owned_directory.glob("agent-*.jsonl"))]
            if children:
                result = evidence.claude_owned_tree(stdout, children, project=project, session_id=session)
            else:
                result = evidence.claude_pipeline(stdout)
            result["rootSession"] = session
        else:
            raise ValueError("unsupported native session provider")
        result["archive"] = {str(path.relative_to(archive)): base.digest(path)
                             for path in sorted(archive.glob("*.jsonl"))}
        result["archivedRoot"] = str(root)
        return result
    except (OSError, ValueError, KeyError, TypeError) as error:
        return {"status": "unknown", "reason": str(error), "total": None}
