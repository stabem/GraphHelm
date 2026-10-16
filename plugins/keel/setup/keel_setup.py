"""Installs Keel into a project that has none: the bundled KEEL files, and one pointer line in CLAUDE.md and AGENTS.md.

Offline and idempotent: a second run changes no bytes. An existing KEEL.md or keel/ file is never
overwritten; when it differs from the bundled copy the run says so and keeps the project's copy.
Usage: python keel_setup.py [project-root]   (default: the current directory)
"""
import sys
from pathlib import Path

BUNDLED = Path(__file__).resolve().parent / "files"
FILES = ("KEEL.md", "keel/KEEL_PLAN.md", "keel/KEEL_EXEC.md", "keel/KEEL_REVIEW.md", "keel/KEEL_MERGE.md")
POINTER_FILES = ("CLAUDE.md", "AGENTS.md")
START, END = "<!-- keel:pointer start -->", "<!-- keel:pointer end -->"
POINTER = "Read KEEL.md before any code work, then the file for your step in keel/."
BLOCK = f"{START}\n{POINTER}\n{END}"


def copy_files(root, out):
    for name in FILES:
        source, target = (BUNDLED / name).read_bytes(), root / name
        if not target.exists():
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(source)
            out.append(f"created {name}")
        elif target.read_bytes() != source:
            out.append(f"kept the project's {name}: it differs from the bundled copy")


def with_pointer(current):
    # The marked block is found and replaced in place; without one it is appended once.
    if START in current and END in current.split(START, 1)[1]:
        head, rest = current.split(START, 1)
        return head + BLOCK + rest.split(END, 1)[1]
    if not current:
        return BLOCK + "\n"
    return current + ("\n" if current.endswith("\n") else "\n\n") + BLOCK + "\n"


def write_pointer(root, out):
    for name in POINTER_FILES:
        path = root / name
        current = path.read_bytes().decode("utf-8") if path.exists() else ""
        updated = with_pointer(current)
        if updated != current:
            path.write_bytes(updated.encode("utf-8"))
            out.append(f"wrote the Keel pointer in {name}")


def main(argv):
    root = Path(argv[1] if len(argv) > 1 else ".").resolve()
    out = []
    copy_files(root, out)
    write_pointer(root, out)
    print("\n".join(out) or "Keel is already installed; nothing changed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
