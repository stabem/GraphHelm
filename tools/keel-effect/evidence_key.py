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
    return {"closure": dirs, "blobs": blobs, "unknown": unknown, "key": key_of(blobs),
            "effects": effect_set(repo, blobs)}


# Effect kinds the closure's production sources can perform, each a regex over one Rust source
# lexed by `strip_noise` (comments removed, literals emptied). `ffi` marks code the scan cannot see
# inside (`unsafe`, `extern "C"`, `asm!`): it is a kind of its own, so a new opaque site reports `+ffi` instead of passing unseen.
# An in-process `Mutex::lock()` is not a file lock and is deliberately not matched.
EFFECT_KINDS = {
    "file_write": re.compile(r"\bFile::create\b|\bOpenOptions\b|\bfs::(?:write|rename|remove_file|"
                             r"remove_dir(?:_all)?|create_dir(?:_all)?|copy|hard_link)\b|\.set_len\("),
    "fsync": re.compile(r"\.sync_all\(|\.sync_data\(|\bFlushFileBuffers\b"),
    "file_lock": re.compile(r"\b(?:try_)?lock_(?:exclusive|shared)\b|\b(?:Unlock|Lock)FileEx\b|\bflock\b"),
    "network": re.compile(r"\bstd::net\b|\bTcpStream\b|\bTcpListener\b|\bUdpSocket\b|\bToSocketAddrs\b|"
                          r"\breqwest\b|\bhyper::|\btokio::net\b|\bureq\b"),
    "process_spawn": re.compile(r"\bCommand::new\b|\bprocess::Command\b|\bgraphhelm_process::command\b|"
                                r"\bCreateProcess[AW]?\b"),
    "ffi": re.compile(r"\bunsafe\b|\bextern\s+\"\"|\basm!"),
}
NOT_PRODUCTION = ("/tests/", "/benches/", "/examples/")
MOD_DECL = re.compile(r"((?:#\s*\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*([;{])")
CFG_ATTR = re.compile(r"#\s*\[\s*cfg\s*\((.*?)\)\s*\]", re.S)
PATH_RAW = re.compile(r"#\s*\[\s*path\s*=\s*\"([^\"\\]+)\"\s*\]")
PATH_LEXED = re.compile(r"#\s*\[\s*path\s*=\s*\"\"\s*\]")
IDENT = re.compile(r"[A-Za-z0-9_]")


class Unlexable(ValueError):
    """The source holds a construct the lexer cannot tokenize with confidence."""


def strip_noise(text: str) -> str:
    """`text` with comments removed and every string, byte string, raw string and char literal
    replaced by `""`, so data such as `"reqwest::get("` or `'"'` is never read as code. Anything the
    lexer cannot close with certainty (an unterminated literal or comment, a quote it cannot place)
    raises `Unlexable`: the caller answers `unknown`, never a guess."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            depth, i = 1, i + 2
            while depth:
                if i >= n:
                    raise Unlexable("unterminated block comment")
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
            out.append(" ")
        elif (raw := re.compile(r"(?:b|c)?r(#*)\"").match(text, i)) and not (i and IDENT.match(text[i - 1])):
            end = text.find('"' + raw.group(1), raw.end())
            if end < 0:
                raise Unlexable("unterminated raw string")
            out.append('""')
            i = end + 1 + len(raw.group(1))
        elif c == '"' or (c in "bc" and text.startswith('"', i + 1) and not (i and IDENT.match(text[i - 1]))):
            i += 1 if c == '"' else 2
            while True:
                if i >= n:
                    raise Unlexable("unterminated string")
                if text[i] == "\\":
                    i += 2
                elif text[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
            out.append('""')
        elif c == "'" or (c == "b" and text.startswith("'", i + 1) and not (i and IDENT.match(text[i - 1]))):
            q = i if c == "'" else i + 1
            if text.startswith("\\", q + 1):
                close = text.find("'", q + 2 if text[q + 2:q + 3] != "'" else q + 3)
                if close < 0 or close - q > 12:
                    raise Unlexable("char escape not closed")
                out.append('""')
                i = close + 1
            elif q + 2 < n and text[q + 2] == "'":
                out.append('""')
                i = q + 3
            elif q + 1 < n and IDENT.match(text[q + 1]):
                out.append(text[i:q + 2])  # a lifetime or loop label
                i = q + 2
            else:
                raise Unlexable(f"quote at offset {q} is neither a char literal nor a lifetime")
        else:
            out.append(c)
            i += 1
    return "".join(out)


def _is_test_only(attrs: str) -> bool:
    """True only for exactly one `#[cfg(test)]` and no `cfg_attr`: `not(test)`, `any(..)`, `all(..)`,
    feature gates or a second cfg all compile the module into some non-test build."""
    cfgs = CFG_ATTR.findall(attrs)
    return len(cfgs) == 1 and cfgs[0].strip() == "test" and "cfg_attr" not in attrs


def analyse(path: str, text: str) -> tuple[str, list[tuple[set[str], bool]]]:
    """Lexed non-test code of one source, and each external module declaration in it as
    (files it may load, test-only). Inline `#[cfg(test)] mod x { ... }` bodies are cut, and the
    declarations inside them dropped. `#[path]` literals are read from the raw text in order; a
    count that does not match the lexed text, or a `cfg_attr`-computed path, raises `Unlexable`."""
    code = strip_noise(text)
    paths = PATH_RAW.findall(text)
    if len(paths) != len(PATH_LEXED.findall(code)):
        raise Unlexable("#[path] attributes not attributable")
    path_at = {m.start(): paths[i] for i, m in enumerate(PATH_LEXED.finditer(code))}
    cuts, decls = [], []
    for m in MOD_DECL.finditer(code):
        attrs, name, end = m.group(1), m.group(2), m.group(3)
        if "cfg_attr" in attrs and "path" in attrs:
            raise Unlexable("cfg_attr path")
        test_only = _is_test_only(attrs)
        if end == "{":
            if test_only:
                depth, j = 1, m.end()
                while depth and j < len(code):
                    depth += {"{": 1, "}": -1}.get(code[j], 0)
                    j += 1
                if depth:
                    raise Unlexable("unbalanced test module")
                cuts.append((m.start(), j))
            continue
        explicit = [p for at, p in path_at.items() if m.start() <= at < m.end()]
        here = Path(path)
        if explicit:
            files = {_normalise(f"{here.parent.as_posix()}/{explicit[0]}")}
        else:
            base = here.parent if here.name in ("lib.rs", "main.rs", "mod.rs") else here.parent / here.stem
            files = {(base / f).as_posix() for f in (f"{name}.rs", f"{name}/mod.rs")}
        decls.append((m.start(), files, test_only))
    inside = lambda at: any(a <= at < b for a, b in cuts)
    kept = [(files, test_only) for at, files, test_only in decls if not inside(at)]
    for a, b in reversed(cuts):
        code = code[:a] + code[b:]
    return code, kept


def effects_of(text: str, path: str = "src/lib.rs") -> set[str]:
    """Effect kinds one Rust source's non-test code can perform; raises `Unlexable`."""
    code, _ = analyse(path, text)
    return {kind for kind, pattern in EFFECT_KINDS.items() if pattern.search(code)}


def effect_set(repo: Path, blobs: dict[str, str]) -> dict:
    """`{"by_kind": {kind: [sources]}, "unlexed": [sources]}` over the closure's production `.rs`
    files: not under tests/, benches/ or examples/, and not a file whose every declaration in the
    closure is exactly `#[cfg(test)] mod`. One non-test declaration (plain, `not(test)`, `any(..)`,
    a feature gate, a `#[path]` to the same file) keeps it production. Bounded by the include
    scan's file and byte limits."""
    paths = sorted(p for p in blobs if p.endswith(".rs") and not any(d in f"/{p}" for d in NOT_PRODUCTION))
    if len(paths) > MAX_SCAN_FILES:
        raise ValueError(f"effect scan over {MAX_SCAN_FILES} files")
    kinds: dict[str, set[str]] = {}
    declared: dict[str, list[bool]] = {}
    unlexed: list[str] = []
    for path, text in zip(paths, _blob_texts(repo, [blobs[p] for p in paths])):
        try:
            code, decls = analyse(path, text)
        except Unlexable:
            unlexed.append(path)
            continue
        for files, test_only in decls:
            for f in files:
                declared.setdefault(f, []).append(test_only)
        kinds[path] = {k for k, pattern in EFFECT_KINDS.items() if pattern.search(code)}
    tests = {f for f, flags in declared.items() if all(flags)}
    by_kind: dict[str, list[str]] = {}
    for path in sorted(kinds.keys() - tests):
        for kind in sorted(kinds[path]):
            by_kind.setdefault(kind, []).append(path)
    # An unlexed source may declare any module, so no exclusion is trusted: it stays listed and
    # makes the answer `unknown`.
    return {"by_kind": dict(sorted(by_kind.items())), "unlexed": sorted(unlexed)}



def _sites(effects: dict) -> set[str]:
    return {f"{kind}@{path}" for kind, paths in effects["by_kind"].items() for path in paths}


def effects_changed(before: dict | None, now: dict) -> dict:
    """Effect sites (`kind@source`) added and removed since the record, so a new spawn in the append
    file shows even when another closure file already spawns. A record with no effect set, or a
    source either side could not lex, answers `unknown`, never `unchanged`."""
    if before is None or "by_kind" not in before:
        return {"added": [], "removed": [], "unknown": ["record has no effect set"],
                "summary": "effects unknown: the record has no effect set"}
    unlexed = sorted(set(now["unlexed"]) | set(before.get("unlexed", [])))
    if unlexed:
        return {"added": [], "removed": [], "unknown": unlexed,
                "summary": "effects unknown: cannot lex " + ", ".join(unlexed)}
    added, removed = sorted(_sites(now) - _sites(before)), sorted(_sites(before) - _sites(now))
    summary = ("effects changed: " + " ".join([*(f"+{s}" for s in added), *(f"-{s}" for s in removed)])
               if added or removed else "effects unchanged")
    return {"added": added, "removed": removed, "summary": summary}


def decide(repo: Path, record: dict, rev: str) -> dict:
    """`reuse` when the closure bytes at `rev` equal the recorded ones and every include resolved;
    otherwise `invalidate`. An include the scan cannot resolve never reads as reuse. Beside the
    decision, `effects_changed` says whether the effect sites moved: a `+kind@source` means the
    path gained an effect site the recorded guarantee never covered."""
    now = state(repo, rev)
    base = {"key": now["key"], "evidence_commit": record["commit"],
            "effects_changed": effects_changed(record.get("effects"), now["effects"])}
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
    if now["effects"]["unlexed"]:
        raise ValueError(f"cannot record effects of unlexable sources: {now['effects']['unlexed']}")
    return {"guarantee": GUARANTEE, "observer": OBSERVER, "observed": observed, "commit": commit,
            "closure": now["closure"], "key": now["key"], "effects": now["effects"]}


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
