#!/usr/bin/env python3
"""Install or remove the warn-only reference-transaction hook (#2669).

    python tools/git-hooks/install.py [--git-dir <common .git dir>] [--dry-run]
    python tools/git-hooks/install.py --uninstall [--git-dir <dir>]

Idempotent: a second install leaves the same bytes. A hook already there that is not ours is kept as
`reference-transaction.bak-graphhelm` (never overwritten); `--uninstall` restores it, or removes
ours. The default git dir is the common dir of the current checkout. Standard library only.
"""
import argparse
import subprocess
import sys
from pathlib import Path

MARKER = b"# graphhelm-ref-guard v1"
SOURCE = Path(__file__).with_name("reference-transaction")


def shared_git_dir(cwd):
    out = subprocess.run(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], cwd=cwd,
                         capture_output=True, text=True, timeout=30, check=True,
                         creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).stdout.strip()
    return Path(out)


def install(git_dir, dry_run=False):
    hooks = git_dir / "hooks"
    target = hooks / "reference-transaction"
    backup = hooks / "reference-transaction.bak-graphhelm"
    wanted = SOURCE.read_bytes().replace(b"\r\n", b"\n")
    if MARKER not in wanted:
        raise SystemExit("install: the shipped hook lost its marker")
    existing = target.read_bytes() if target.is_file() else None
    if existing == wanted:
        return "unchanged"
    if dry_run:
        return "would install" + (" (backing up the existing hook)" if existing and MARKER not in existing else "")
    hooks.mkdir(parents=True, exist_ok=True)
    if existing is not None and MARKER not in existing and not backup.exists():
        backup.write_bytes(existing)
    target.write_bytes(wanted)
    target.chmod(0o755)
    return "installed"


def uninstall(git_dir):
    hooks = git_dir / "hooks"
    target = hooks / "reference-transaction"
    backup = hooks / "reference-transaction.bak-graphhelm"
    if target.is_file() and MARKER not in target.read_bytes():
        return "not ours: left alone"
    if backup.is_file():
        target.write_bytes(backup.read_bytes())
        target.chmod(0o755)
        backup.unlink()
        return "restored the original hook"
    if target.is_file():
        target.unlink()
        return "removed"
    return "nothing to remove"


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--git-dir", type=Path)
    p.add_argument("--uninstall", action="store_true")
    p.add_argument("--dry-run", action="store_true")
    args = p.parse_args(argv)
    git_dir = args.git_dir or shared_git_dir(Path.cwd())
    print(uninstall(git_dir) if args.uninstall else install(git_dir, args.dry_run))
    return 0


if __name__ == "__main__":
    sys.exit(main())
