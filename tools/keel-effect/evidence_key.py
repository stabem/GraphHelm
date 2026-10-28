"""Key the #1476 lost-ack evidence to the append path's dependency closure, and decide reuse.

The guarantee: an append whose ack was lost, retried with the same idempotency key, returns the
committed record and never writes a second one. Its observer is `append_effect_contract.rs` against
the real `LocalEventRepository`. A run of that observer is evidence for one set of bytes only: the
files that can change what the append path does.

That set is computed at a git revision from what the repository already declares, never from the
working tree and never from a list typed by hand:

- the workspace members in the root `Cargo.toml`;
- from `graphhelm-events`, every member reached through `path` dependencies in `[dependencies]`,
  `[build-dependencies]` and their `[target.*]` forms; plus the dev-dependencies of
  `graphhelm-events` itself (they link into the observer) and their normal closure;
- every file under those members' directories, plus `Cargo.toml`, `Cargo.lock`,
  `rust-toolchain.toml` and `.cargo/config.toml`;
- every file a Rust source in that set embeds through a literal `include!`, `include_str!` or
  `include_bytes!`, resolved from the source's directory and followed through embedded `.rs`.
  A non-literal include, or one naming a file absent at the revision, is listed as `unknown`:
  `check` answers `invalidate` and `record` refuses. No crate in the closure has a `build.rs`
  today; one would sit inside its member directory and be keyed, but its outputs would not.

The key is SHA-256 over the sorted `path NUL blob-id` lines of those files at the revision. The
record (`docs/keel/effect-evidence/append-lost-ack.json`) holds the key of the commit whose
observer run it vouches for. `check` recomputes the key at a revision: equal means `reuse`, any
difference means `invalidate`, with the closure paths that differ. A change outside the closure
(Studio, docs, another crate the append path does not link) cannot move the key.

Usage:
  python tools/keel-effect/evidence_key.py check [--rev HEAD]     # exit 0 reuse, 1 invalidate
  python tools/keel-effect/evidence_key.py record --rev <sha> --observed "<test command and result>"
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT_PACKAGE = "graphhelm-events"
GUARANTEE = "idempotent under lost_ack"
OBSERVER = "cargo +1.97.1 test --locked -p graphhelm-events --test append_effect_contract"
RECORD = "docs/keel/effect-evidence/append-lost-ack.json"
SHARED_FILES = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml")
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)
NORMAL = ("dependencies", "build-dependencies")
MAX_SCAN_FILES = 5000
MAX_SCAN_BYTES = 4 * 1024 * 1024
INCLUDE_SITE = re.compile(r"\binclude(?:_str|_bytes)?!\s*\(")
INCLUDE_LITERAL = re.compile(r'include(?:_str|_bytes)?!\s*\(\s*"([^"\\]+)"\s*,?\s*\)')


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, text=True,
                          encoding="utf-8", creationflags=NO_WINDOW).stdout


def _toml_at(repo: Path, rev: str, path: str) -> dict:
    return tomllib.loads(git(repo, "show", f"{rev}:{path}"))


def _path_deps(manifest: dict, sections: tuple[str, ...]) -> list[str]:
    tables = [manifest.get(s, {}) for s in sections]
    for target in manifest.get("target", {}).values():
        tables.extend(target.get(s, {}) for s in sections)
    return [spec["path"] for table in tables for spec in table.values()
            if isinstance(spec, dict) and "path" in spec]


def closure(repo: Path, rev: str) -> list[str]:
    """Member directories the append path and its observer link, at `rev`, sorted."""
    members = _toml_at(repo, rev, "Cargo.toml")["workspace"]["members"]
    manifests = {m: _toml_at(repo, rev, f"{m}/Cargo.toml") for m in members}
    by_name = {man["package"]["name"]: m for m, man in manifests.items()}

    def resolve(member: str, rel: str) -> str:
        return (Path(member) / rel).as_posix() if not Path(rel).is_absolute() else rel

    def normal_from(start: set[str]) -> set[str]:
        reached, frontier = set(start), set(start)
        while frontier:
            nxt = set()
            for m in frontier:
                for rel in _path_deps(manifests[m], NORMAL):
                    dep = _normalise(resolve(m, rel))
                    if dep in manifests and dep not in reached:
                        nxt.add(dep)
            reached |= nxt
            frontier = nxt
        return reached

    root = by_name[ROOT_PACKAGE]
    dev = {_normalise(resolve(root, rel)) for rel in _path_deps(manifests[root], ("dev-dependencies",))}
    return sorted(normal_from({root}) | normal_from({d for d in dev if d in manifests}))


def _normalise(path: str) -> str:
    parts: list[str] = []
    for part in path.split("/"):
        if part == "..":
            parts.pop()
        elif part not in ("", "."):
            parts.append(part)
    return "/".join(parts)


def _ls_tree(repo: Path, rev: str, paths: list[str]) -> dict[str, str]:
    listing = git(repo, "ls-tree", "-r", "-z", rev, "--", *paths) if paths else ""
    blobs = {}
    for entry in listing.split("\0"):
        if entry:
            meta, path = entry.split("\t", 1)
            blobs[path] = meta.split()[2]
    return blobs


def _blob_texts(repo: Path, blob_ids: list[str]) -> list[str]:
    """Contents of `blob_ids`, in order, through one `git cat-file --batch`."""
    if not blob_ids:
        return []
    out = subprocess.run(["git", "cat-file", "--batch"], cwd=repo, check=True, capture_output=True,
                         input="\n".join(blob_ids).encode() + b"\n", creationflags=NO_WINDOW).stdout
    texts, at = [], 0
    for _ in blob_ids:
        header_end = out.index(b"\n", at)
        size = int(out[at:header_end].split()[2])
        if size > MAX_SCAN_BYTES:
            raise ValueError(f"source over {MAX_SCAN_BYTES} bytes")
        texts.append(out[header_end + 1:header_end + 1 + size].decode("utf-8", errors="replace"))
        at = header_end + 1 + size + 1
    return texts


def includes(source_path: str, text: str) -> tuple[list[str], list[str]]:
    """Files `text` embeds via a literal `include!`/`include_str!`/`include_bytes!`, resolved from
    the source file's directory; and the sites whose argument is not one plain string literal."""
    found, unknown = [], []
    for site in INCLUDE_SITE.finditer(text):
        literal = INCLUDE_LITERAL.match(text, site.start())
        line = text.count("\n", 0, site.start()) + 1
        if not literal:
            unknown.append(f"{source_path}:{line}")
            continue
        target = _normalise(f"{Path(source_path).parent.as_posix()}/{literal.group(1)}")
        found.append(target) if target else unknown.append(f"{source_path}:{line}")
    return found, unknown


def closure_blobs(repo: Path, rev: str, dirs: list[str]) -> tuple[dict[str, str], list[str]]:
    """`path -> blob id` for every file in the closure at `rev`, including the files its Rust sources
    embed (followed through `include!` of `.rs`, bounded), and the include sites it cannot resolve."""
    blobs = _ls_tree(repo, rev, [*dirs, *SHARED_FILES])
    unknown: list[str] = []
    scan = sorted(p for p in blobs if p.endswith(".rs"))
    scanned: set[str] = set()
    while scan:
        if len(scanned) + len(scan) > MAX_SCAN_FILES:
            raise ValueError(f"include scan over {MAX_SCAN_FILES} files")
        scanned.update(scan)
        embedded: set[str] = set()
        for path, text in zip(scan, _blob_texts(repo, [blobs[p] for p in scan])):
            found, sites = includes(path, text)
            embedded.update(found)
            unknown.extend(sites)
        missing = sorted(embedded - blobs.keys())
        resolved = _ls_tree(repo, rev, missing)
        unknown.extend(f"missing:{p}" for p in missing if p not in resolved)
        blobs.update(resolved)
        scan = sorted(p for p in resolved if p.endswith(".rs") and p not in scanned)
    return blobs, sorted(unknown)


def key_of(blobs: dict[str, str]) -> str:
    digest = hashlib.sha256()
    for path in sorted(blobs):
        digest.update(f"{path}\0{blobs[path]}\n".encode())
    return "sha256:" + digest.hexdigest()


def state(repo: Path, rev: str) -> dict:
    dirs = closure(repo, rev)
    blobs, unknown = closure_blobs(repo, rev, dirs)
    return {"closure": dirs, "blobs": blobs, "unknown": unknown, "key": key_of(blobs)}


def decide(repo: Path, record: dict, rev: str) -> dict:
    """`reuse` when the closure bytes at `rev` equal the recorded ones and every include resolved;
    otherwise `invalidate`. An include the scan cannot resolve never reads as reuse."""
    now = state(repo, rev)
    base = {"key": now["key"], "evidence_commit": record["commit"]}
    if now["unknown"]:
        return {"decision": "invalidate", **base, "changed": [], "unknown": now["unknown"],
                "rerun": record["observer"]}
    if now["key"] == record["key"]:
        return {"decision": "reuse", **base, "changed": []}
    before = state(repo, record["commit"])["blobs"]
    changed = sorted(p for p in before.keys() | now["blobs"].keys() if before.get(p) != now["blobs"].get(p))
    return {"decision": "invalidate", **base, "changed": changed, "rerun": record["observer"]}


def make_record(repo: Path, rev: str, observed: str) -> dict:
    commit = git(repo, "rev-parse", f"{rev}^{{commit}}").strip()
    now = state(repo, commit)
    if now["unknown"]:
        raise ValueError(f"cannot vouch for unresolved includes: {now['unknown']}")
    return {"guarantee": GUARANTEE, "observer": OBSERVER, "observed": observed, "commit": commit,
            "closure": now["closure"], "key": now["key"]}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("action", choices=("check", "record"))
    parser.add_argument("--repo", default=".")
    parser.add_argument("--rev", default="HEAD")
    parser.add_argument("--observed", help="record: the observer command's result line")
    args = parser.parse_args(argv)
    repo = Path(args.repo)
    path = repo / RECORD
    if args.action == "record":
        if not args.observed:
            parser.error("record needs --observed")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(make_record(repo, args.rev, args.observed), indent=2) + "\n",
                        encoding="utf-8", newline="\n")
        return 0
    result = decide(repo, json.loads(path.read_text(encoding="utf-8")), args.rev)
    print(json.dumps(result, indent=2))
    return 0 if result["decision"] == "reuse" else 1


if __name__ == "__main__":
    sys.exit(main())
