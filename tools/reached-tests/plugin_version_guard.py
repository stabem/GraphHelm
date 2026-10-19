#!/usr/bin/env python3
"""Require a coherent plugin version increase when shipped plugin content changes.

`--write` regenerates the chain a skill change drags in (#1459 item 14), from the working tree:
the version of each changed release group one patch above `--base` (unless it is already above
it), every builtin extension's contribution digests in `extension.json`, and each package's
canonical goldens under `core/protocols/tests/fixtures/canonical-goldens/` (#1459 item 16; the
Rust cell `canonical_golden` stays the authority over those bytes). The source release file
`extensions/releases/adoption-*.json` pins no version or digest, so it needs no rewrite. A
second run changes nothing."""
import argparse
import hashlib
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
KEEL_MANIFESTS = (
    "plugins/keel/.claude-plugin/plugin.json",
    "plugins/keel/.codex-plugin/plugin.json",
)
JOURNEY_REVIEW_MANIFESTS = ("plugins/journey-review/.claude-plugin/plugin.json",)
# Each release group versions its own roots: shipping one never forces a bump of another.
GROUPS = (
    (PLUGIN_ROOTS, MANIFESTS),
    (("plugins/keel/",), KEEL_MANIFESTS),
    (("plugins/journey-review/",), JOURNEY_REVIEW_MANIFESTS),
)
EXTENSIONS = "extensions/builtin/"
GOLDENS = "core/protocols/tests/fixtures/canonical-goldens/"
ALL_ROOTS = tuple(root for roots, _ in GROUPS for root in roots) + (EXTENSIONS,)
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
        if status.startswith(("R", "C")):
            source, destination = fields[index:index + 2]
            paths.append(destination)
            if status.startswith("R") and not destination.startswith(ALL_ROOTS):
                paths.append(source)
            index += 2
        else:
            paths.append(fields[index])
            index += 1
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
    version = _version_field(manifest)
    match = VERSION.fullmatch(version) if isinstance(version, str) else None
    if match is None:
        raise ValueError(f"invalid release version in manifest: {path}")
    major, minor, patch = (int(part) for part in match.group(1, 2, 3))
    prerelease = match.group(4)
    identifiers = tuple(prerelease.split(".")) if prerelease is not None else None
    return version, (major, minor, patch, identifiers)


def _version_field(manifest):
    if not isinstance(manifest, dict):
        return None
    if manifest.get("kind") == "Extension":
        metadata = manifest.get("metadata")
        return metadata.get("version") if isinstance(metadata, dict) else None
    return manifest.get("version")


def _extension_groups(repo, commit):
    """Each builtin extension package is its own release group: extension.json and both host manifests."""
    try:
        names = _git(repo, "ls-tree", "-d", "--name-only", f"{commit}:{EXTENSIONS.rstrip('/')}").split()
    except subprocess.CalledProcessError:
        return []
    return [((f"{EXTENSIONS}{name}/",), tuple(f"{EXTENSIONS}{name}/{manifest}" for manifest in
             ("extension.json", ".claude-plugin/plugin.json", ".codex-plugin/plugin.json")))
            for name in sorted(names)]


def _parsed_version(version):
    match = VERSION.fullmatch(version) if isinstance(version, str) else None
    if match is None:
        return None
    major, minor, patch = (int(part) for part in match.group(1, 2, 3))
    return major, minor, patch, tuple(match.group(4).split(".")) if match.group(4) is not None else None


def _version_precedence(version):
    major, minor, patch, prerelease = version
    if prerelease is None:
        return major, minor, patch, (1,)
    # A matching shorter prerelease sorts below every possible next identifier.
    return major, minor, patch, (0, *(
        (0, int(identifier)) if identifier.isdigit() else (1, identifier)
        for identifier in prerelease
    ), (-1,))


def _group_check(repo, base, head, paths, roots, manifests):
    shipped = [path for path in paths if path.startswith(roots) and not _is_docs_or_tests(path)]
    if not shipped:
        return True, None
    try:
        after = [_manifest_version(repo, head, path) for path in manifests]
    except ValueError as exc:
        return False, str(exc)
    after_versions = [item[0] for item in after]
    if len(set(after_versions)) != 1:
        path = manifests[next(i for i, version in enumerate(after_versions) if version != after_versions[0])]
        return False, f"mismatched release manifests: {path} has {after_versions[manifests.index(path)]}"
    try:
        before = [_manifest_version(repo, base, path) for path in manifests]
    except ValueError as exc:
        # A group that did not exist at the base is a new plugin: its first version needs no bump.
        if all(_absent(repo, base, path) for path in manifests):
            return True, f"PASS: new plugin release {after_versions[0]} in {manifests[0]}"
        return False, str(exc)
    before_versions = [item[0] for item in before]
    if len(set(before_versions)) != 1:
        path = manifests[next(i for i, version in enumerate(before_versions) if version != before_versions[0])]
        return False, f"mismatched base release manifests: {path} has {before_versions[manifests.index(path)]}"
    if _version_precedence(after[0][1]) <= _version_precedence(before[0][1]):
        return False, f"bump the plugin version in {manifests[0]} (shipped plugin content changed)"
    return True, f"PASS: shipped plugin content changed; release version {after_versions[0]} exceeds {before_versions[0]}"


def _absent(repo, commit, path):
    return subprocess.run(["git", "cat-file", "-e", f"{commit}:{path}"], cwd=repo, capture_output=True,
                          creationflags=NO_WINDOW).returncode != 0


def check(repo, base_ref, head_ref):
    """Return (passed, message), reading only committed blobs at the supplied refs."""
    repo = Path(repo).resolve()
    base, head = _commit(repo, base_ref), _commit(repo, head_ref)
    paths = _changed_paths(repo, base, head)
    messages = []
    for roots, manifests in (*GROUPS, *_extension_groups(repo, head)):
        passed, message = _group_check(repo, base, head, paths, roots, manifests)
        if not passed:
            return False, message
        if message is not None:
            messages.append(message)
    if not messages:
        return True, "PASS: no shipped plugin content changed; version bump not required"
    return True, "; ".join(messages)


_VERSION_FIELD = re.compile(r'("version"\s*:\s*)"[^"]*"')
_DIGEST_FIELD = re.compile(r'("sha256"\s*:\s*)"sha256:[0-9a-f]{64}"')


def _rewrite(path, text):
    """Write `text` only when it differs from the file, so a current file keeps its bytes and mtime."""
    if path.is_file() and path.read_bytes().decode("utf-8").replace("\r\n", "\n") == text:
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))
    return True


def _set_version(path, version):
    text = path.read_bytes().decode("utf-8")
    new, count = _VERSION_FIELD.subn(lambda m: f'{m.group(1)}"{version}"', text, count=1)
    if count != 1 or _version_field(json.loads(new)) != version:
        raise ValueError(f"cannot set the version in {path}")
    return _rewrite(path, new)


def _set_digests(package):
    path = package / "extension.json"
    text = path.read_bytes().decode("utf-8")
    contributions = json.loads(text)["spec"]["contracts"]["contributions"]
    digests = iter(["sha256:" + hashlib.sha256((package / item["path"]).read_bytes()).hexdigest()
                    for item in contributions])
    new, count = _DIGEST_FIELD.subn(lambda m: f'{m.group(1)}"{next(digests)}"', text)
    if count != len(contributions):
        raise ValueError(f"{path}: {count} sha256 fields for {len(contributions)} contributions")
    return _rewrite(path, new)


def _reject_constant(name):
    raise ValueError(name)


def _goldens(repo, package):
    """Canonical JSON digests of every parseable JSON file of one package: the corpus and bytes
    core/protocols/tests/canonical_golden.rs checks."""
    out = {}
    for path in package.rglob("*.json"):
        try:
            value = json.loads(path.read_bytes().decode("utf-8"), parse_constant=_reject_constant)
        except ValueError:
            continue  # a deliberately malformed fixture is not a canonical input
        canonical = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        out[path.relative_to(repo).as_posix()] = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    return json.dumps(dict(sorted(out.items())), indent=2, ensure_ascii=False) + "\n"


def _worktree_changes(repo, base):
    """Paths that differ between the merge base with `base` and the working tree, untracked included."""
    fork = _git(repo, "merge-base", base, "HEAD").strip()
    changed = _git(repo, "diff", "--name-only", "-z", fork, "--").split("\0")
    untracked = _git(repo, "ls-files", "--others", "--exclude-standard", "-z").split("\0")
    return [path for path in changed + untracked if path]


def write(repo, base_ref):
    """Regenerate versions, contribution digests and goldens; return the files written."""
    repo = Path(repo).resolve()
    base = _commit(repo, base_ref)
    changed = _worktree_changes(repo, base)
    written = []
    for roots, manifests in (*GROUPS, *_extension_groups(repo, "HEAD")):
        if not any(path.startswith(roots) and not _is_docs_or_tests(path) for path in changed):
            continue
        if all(_absent(repo, base, path) for path in manifests):
            continue  # a new release group starts at its own first version
        _, before = _manifest_version(repo, base, manifests[0])
        current = {_version_field(json.loads((repo / path).read_bytes())) for path in manifests}
        parsed = _parsed_version(next(iter(current))) if len(current) == 1 else None
        if parsed is not None and _version_precedence(parsed) > _version_precedence(before):
            continue
        version = f"{before[0]}.{before[1]}.{before[2] + 1}"
        written += [path for path in manifests if _set_version(repo / path, version)]
    for package in sorted((repo / EXTENSIONS).iterdir()):
        if not (package / "extension.json").is_file():
            continue
        if _set_digests(package):
            written.append(f"{EXTENSIONS}{package.name}/extension.json")
        golden = repo / GOLDENS / f"{package.name}.json"
        if _rewrite(golden, _goldens(repo, package)):
            written.append(golden.relative_to(repo).as_posix())
    return list(dict.fromkeys(written))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=".")
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", help="the ref to check (required without --write)")
    parser.add_argument("--write", action="store_true",
                        help="regenerate versions, extension digests and canonical goldens in the working tree")
    args = parser.parse_args(argv)
    if args.write:
        try:
            written = write(args.repo, args.base)
        except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as exc:
            print(f"plugin version guard --write failed: {exc}")
            return 1
        print("wrote " + ", ".join(written) if written else "nothing to write: the chain is current")
        return 0
    if args.head is None:
        parser.error("--head is required without --write")
    try:
        passed, message = check(args.repo, args.base, args.head)
    except (OSError, subprocess.CalledProcessError) as exc:
        message = f"plugin version guard could not read committed Git refs: {exc}"
        passed = False
    print(message)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
