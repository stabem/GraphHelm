#!/usr/bin/env python3
"""Require a coherent plugin version increase when shipped plugin content changes."""
import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

MANIFESTS = (
    "plugins/graphhelm/.claude-plugin/plugin.json",
    "plugins/graphhelm/.codex-plugin/plugin.json",
    "plugins/graphhelm/plugin.json",
    "plugins/graphhelm-codex-hooks/.codex-plugin/plugin.json",
)
PLUGIN_ROOTS = ("plugins/graphhelm/", "plugins/graphhelm-codex-hooks/")
VERSION = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-((?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9]\d*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$")
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def _git(repo, *args):
    return subprocess.run(["git", *args], cwd=repo, capture_output=True, text=True,
                          encoding="utf-8", errors="replace", check=True,
                          creationflags=NO_WINDOW).stdout


def _commit(repo, ref):
    return _git(repo, "rev-parse", "--verify", f"{ref}^{{commit}}").strip()


def _changed_paths(repo, base, head):
    data = subprocess.run(["git", "diff", "--name-status", "-z", "--find-renames", f"{base}...{head}", "--"],
                          cwd=repo, capture_output=True, check=True,
                          creationflags=NO_WINDOW).stdout.decode("utf-8", "surrogateescape")
    fields = data.split("\0")
    paths = []
    index = 0
    while index < len(fields) and fields[index]:
        status = fields[index]
        index += 1
        count = 2 if status.startswith(("R", "C")) else 1
        paths.extend(fields[index:index + count])
        index += count
    return paths


def _is_docs_or_tests(path):
    parts = path.split("/")
    name = parts[-1].lower()
    return (any(part.lower() in {"docs", "tests"} for part in parts[2:])
            or name.startswith("test_") or name.endswith("_test.py")
            or ".test." in name)


def _manifest_version(repo, commit, path):
    try:
        raw = _git(repo, "show", f"{commit}:{path}")
    except subprocess.CalledProcessError:
        raise ValueError(f"missing release manifest: {path}") from None
    try:
        manifest = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ValueError(f"invalid release manifest {path}: {exc.msg}") from None
    version = manifest.get("version") if isinstance(manifest, dict) else None
    match = VERSION.fullmatch(version) if isinstance(version, str) else None
    if match is None:
        raise ValueError(f"invalid release version in manifest: {path}")
    major, minor, patch = (int(part) for part in match.group(1, 2, 3))
    prerelease = match.group(4)
    identifiers = tuple(prerelease.split(".")) if prerelease is not None else None
    return version, (major, minor, patch, identifiers)


def _version_precedence(version):
    major, minor, patch, prerelease = version
    if prerelease is None:
        return major, minor, patch, (1,)
    return major, minor, patch, (0, *(
        (0, int(identifier)) if identifier.isdigit() else (1, identifier)
        for identifier in prerelease
    ), (2,))


def check(repo, base_ref, head_ref):
    """Return (passed, message), reading only committed blobs at the supplied refs."""
    repo = Path(repo).resolve()
    base, head = _commit(repo, base_ref), _commit(repo, head_ref)
    paths = _changed_paths(repo, base, head)
    shipped = [path for path in paths if path.startswith(PLUGIN_ROOTS) and not _is_docs_or_tests(path)]
    if not shipped:
        return True, "PASS: no shipped plugin content changed; version bump not required"

    try:
        before = [_manifest_version(repo, base, path) for path in MANIFESTS]
        after = [_manifest_version(repo, head, path) for path in MANIFESTS]
    except ValueError as exc:
        return False, str(exc)
    before_versions = [item[0] for item in before]
    after_versions = [item[0] for item in after]
    if len(set(before_versions)) != 1:
        path = MANIFESTS[next(i for i, version in enumerate(before_versions) if version != before_versions[0])]
        return False, f"mismatched base release manifests: {path} has {before_versions[MANIFESTS.index(path)]}"
    if len(set(after_versions)) != 1:
        path = MANIFESTS[next(i for i, version in enumerate(after_versions) if version != after_versions[0])]
        return False, f"mismatched release manifests: {path} has {after_versions[MANIFESTS.index(path)]}"
    if _version_precedence(after[0][1]) <= _version_precedence(before[0][1]):
        return False, f"bump the plugin version in {MANIFESTS[0]} (shipped plugin content changed)"
    return True, f"PASS: shipped plugin content changed; release version {after_versions[0]} exceeds {before_versions[0]}"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=".")
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    args = parser.parse_args(argv)
    try:
        passed, message = check(args.repo, args.base, args.head)
    except (OSError, subprocess.CalledProcessError) as exc:
        message = f"plugin version guard could not read committed Git refs: {exc}"
        passed = False
    print(message)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
