"""Package a built GraphHelm CLI as a release archive and smoke-test it from outside the tree.

Usage:
  python package_release.py --binary <built graphhelm[.exe]> --repo <checkout at the release head>
      --target windows-x86_64|ubuntu-24.04-x86_64 --out <new directory> [--smoke-only <archive>]

Archive layout (as 0.1.1): graphhelm[.exe], extensions/ (builtin packages and releases/, which the
adoption loader resolves next to the executable), LICENSE, README.md. Writes the archive and
SHA256SUMS.txt. The smoke test extracts into a fresh scratch directory and runs --version, --help,
`extension validate` on each builtin package and an offline graph validation; any failure fails
the run. Every child runs hidden (CREATE_NO_WINDOW on Windows).
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def stage(binary: Path, repo: Path, root: Path) -> None:
    root.mkdir(parents=True)
    shutil.copy2(binary, root / binary.name)
    for sub in ("builtin", "releases"):
        shutil.copytree(repo / "extensions" / sub, root / "extensions" / sub)
    shutil.copy2(repo / "LICENSE", root / "LICENSE")
    (root / "README.md").write_text(
        "GraphHelm CLI release archive. Run `graphhelm --version`, then follow\n"
        "docs/install/GETTING_STARTED.md in the tagged source for the Runtime and the Studio\n"
        "(the Studio ships in source and runs from source).\n",
        encoding="utf-8",
    )


def archive(root: Path, out: Path, target: str) -> Path:
    name = f"graphhelm-{target}"
    if target.startswith("windows"):
        path = out / f"{name}.zip"
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zipped:
            for item in sorted(root.rglob("*")):
                if item.is_file():
                    zipped.write(item, Path(name) / item.relative_to(root))
    else:
        path = out / f"{name}.tar.gz"
        with tarfile.open(path, "w:gz") as tarred:
            tarred.add(root, arcname=name)
    return path


def extract(path: Path, into: Path) -> Path:
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as zipped:
            zipped.extractall(into)
    else:
        with tarfile.open(path) as tarred:
            tarred.extractall(into, filter="data")
    return next(into.iterdir())


def smoke(path: Path, example_graph: Path) -> list[str]:
    scratch = Path(tempfile.mkdtemp(prefix="graphhelm-smoke-"))
    try:
        root = extract(path, scratch)
        exe = next(p for p in root.iterdir() if p.name in ("graphhelm", "graphhelm.exe"))
        if os.name != "nt":
            exe.chmod(0o755)
        work = scratch / "work"
        work.mkdir()
        # A graph from the release head's own examples, copied OUT of the tree: offline validation.
        graph = work / example_graph.name
        shutil.copy2(example_graph, graph)
        steps = [
            [str(exe), "--version"],
            [str(exe), "--help"],
            *([str(exe), "--json", "extension", "validate", str(pkg)]
              for pkg in sorted((root / "extensions" / "builtin").iterdir()) if pkg.is_dir()),
            [str(exe), "--json", "graph", "validate", str(graph)],
        ]
        lines = []
        for argv in steps:
            done = subprocess.run(argv, cwd=work, capture_output=True, text=True, timeout=120,
                                  creationflags=NO_WINDOW)
            label = " ".join(Path(a).name if i == 0 else a for i, a in enumerate(argv))
            label = label.replace(str(root), "<archive>").replace(str(work), "<work>")
            if done.returncode != 0:
                raise SystemExit(f"SMOKE FAILED: {label} -> {done.returncode}\n{done.stdout}{done.stderr}")
            first = (done.stdout.strip().splitlines() or [""])[0][:120]
            lines.append(f"ok  {label}  | {first}")
        return lines
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--repo", type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--smoke-only", type=Path)
    parser.add_argument("--graph", type=Path, required=True,
                        help="a valid example graph from the release head (copied out of the tree)")
    args = parser.parse_args()
    if args.smoke_only:
        print("\n".join(smoke(args.smoke_only, args.graph)))
        return 0
    if args.out.exists():
        raise SystemExit(f"{args.out} exists; choose a new directory")
    args.out.mkdir(parents=True)
    staging = Path(tempfile.mkdtemp(prefix="graphhelm-stage-", dir=args.out.parent))
    try:
        stage(args.binary, args.repo, staging / "root")
        path = archive(staging / "root", args.out, args.target)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    (args.out / "SHA256SUMS.txt").write_text(f"{sha256(path)}  {path.name}\n", encoding="utf-8")
    manifest = {"target": args.target, "archive": path.name, "sha256": sha256(path),
                "binarySha256": sha256(args.binary), "bytes": path.stat().st_size}
    (args.out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest))
    print("\n".join(smoke(path, args.graph)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
