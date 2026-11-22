#!/usr/bin/env python3
"""Pin each plugin release group's shipped files to its manifest version (#3006).

A host caches a plugin by version, so content that changed under an unchanged version never
reaches it. `plugin-shipped-digests.json` records, per release group and version, the digest of
the group's shipped files (tests and docs excluded, as in plugin_version_guard). `check` fails
when the digest recorded for the manifest's current version differs from the files; `--write`
records the current digest after the version is bumped. Offline; reads `git ls-files` and files.

`--write` refuses to replace the digest of a version that is already recorded."""
import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import plugin_version_guard as guard  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
RECORD = Path(__file__).resolve().parent / "plugin-shipped-digests.json"
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def _files(repo, roots):
    out = subprocess.run(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", *roots],
                         cwd=repo, capture_output=True, check=True, creationflags=NO_WINDOW).stdout
    names = sorted(set(out.decode("utf-8", "surrogateescape").split("\0")) - {""})
    return [n for n in names if (repo / n).is_file() and not guard._is_docs_or_tests(n)]


def group_digest(repo, roots):
    digest = hashlib.sha256()
    for name in _files(repo, roots):
        data = (repo / name).read_bytes().replace(b"\r\n", b"\n")
        digest.update(name.encode() + b"\0" + hashlib.sha256(data).digest())
    return digest.hexdigest()


def group_version(repo, manifests):
    versions = {json.loads((repo / m).read_bytes())["version"] for m in manifests}
    if len(versions) != 1:
        raise ValueError(f"release manifests disagree for {manifests[0]}: {sorted(versions)}")
    return versions.pop()


def current(repo):
    """{group root: (version, digest)} for every plugin release group, read from the working tree."""
    return {roots[0]: (group_version(repo, manifests), group_digest(repo, roots))
            for roots, manifests in guard.GROUPS}


def problems(repo, record):
    found = []
    for group, (version, digest) in current(repo).items():
        recorded = record.get(group, {}).get(version)
        if recorded is None:
            found.append(f"{group}: version {version} has no recorded digest; run "
                         f"`python tools/reached-tests/plugin_shipped_digest.py --write`")
        elif recorded != digest:
            found.append(f"{group}: shipped files changed while the version stayed {version}; bump the "
                         f"version in every manifest of the group, then run "
                         f"`python tools/reached-tests/plugin_shipped_digest.py --write`")
    return found


def load(path=RECORD):
    return json.loads(path.read_bytes().decode("utf-8"))


def record_current(repo, record):
    """Add the current digests to `record`; refuse (and change nothing) to overwrite a different
    digest recorded for the same version, which is a change under an unchanged version."""
    now = current(repo)
    refused = [f"{group}: version {version} already has a different recorded digest; bump the version "
               f"in every manifest of the group before `--write`"
               for group, (version, digest) in now.items()
               if record.get(group, {}).get(version, digest) != digest]
    if not refused:
        for group, (version, digest) in now.items():
            record.setdefault(group, {})[version] = digest
    return refused


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="record the current digests")
    args = parser.parse_args(argv)
    if args.write:
        record = load() if RECORD.is_file() else {}
        refused = record_current(REPO, record)
        if refused:
            print("\n".join(refused))
            return 1
        RECORD.write_bytes((json.dumps(record, indent=2, sort_keys=True) + "\n").encode("utf-8"))
        return 0
    found = problems(REPO, load())
    print("\n".join(found) if found else "plugin shipped digests current")
    return 1 if found else 0


if __name__ == "__main__":
    raise SystemExit(main())
