"""Installs Keel into a project: the bundled KEEL files, one pointer line in CLAUDE.md and AGENTS.md, and the advisor setting.

Offline and idempotent: a second run changes no bytes. A missing KEEL.md or keel/ file is created.
An existing one that equals the bundled copy (line endings aside) is left alone; one that equals a
past bundled version listed in known_digests.json was never edited and is replaced by the bundled
copy ("updated <name>"), keeping its line endings; any other content is the project's own and is
kept, with a note that it differs. The advisor step runs on every invocation: it adds `advisorModel` to
<project>/.claude/settings.json, keeps every other key, and leaves an existing `advisorModel` alone.
Usage: python keel_setup.py [project-root] [--advisor MODEL | --no-advisor]   (default root: the current directory)
Exit 1 when the settings file cannot be used or a Keel file could not be replaced; the other steps have still run.
"""
import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

BUNDLED = Path(__file__).resolve().parent / "files"
DIGESTS = Path(__file__).resolve().parent / "known_digests.json"
FILES = ("KEEL.md", "keel/KEEL_PLAN.md", "keel/KEEL_EXEC.md", "keel/KEEL_REVIEW.md", "keel/KEEL_MERGE.md")
POINTER_FILES = ("CLAUDE.md", "AGENTS.md")
START, END = "<!-- keel:pointer start -->", "<!-- keel:pointer end -->"
POINTER = "Read KEEL.md before any code work, then the file for your step in keel/."
BLOCK = f"{START}\n{POINTER}\n{END}"

ADVISOR_KEY = "advisorModel"
DEFAULT_ADVISOR = "fable"
SETTINGS = ".claude/settings.json"


CRLF, LF = b"\r\n", b"\n"


def same_text(a, b):
    # A checkout that converts line endings still holds the same file.
    return a.replace(CRLF, LF) == b.replace(CRLF, LF)


def digest(data):
    return hashlib.sha256(data.replace(CRLF, LF)).hexdigest()


def replace_file(target, data):
    temp = target.with_name(target.name + ".keel-tmp")
    try:
        temp.write_bytes(data)
        os.replace(temp, target)  # a crash or refusal leaves the old file, never half of the new one
    except OSError:
        temp.unlink(missing_ok=True)
        raise


def copy_files(root, out):
    """Returns True when every file is as it should be; False when a replacement was refused."""
    known, ok = json.loads(DIGESTS.read_bytes().decode("utf-8")), True
    for name in FILES:
        source, target = (BUNDLED / name).read_bytes(), root / name
        if not target.exists():
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(source)
            out.append(f"created {name}")
            continue
        current = target.read_bytes()
        if same_text(current, source):
            continue
        if digest(current) not in known.get(name, []):
            out.append(f"kept the project's {name}: it differs from the bundled copy")
            continue
        fresh = source.replace(CRLF, LF)
        if CRLF in current:
            fresh = fresh.replace(LF, CRLF)  # the file keeps its line-ending style
        try:
            replace_file(target, fresh)
            out.append(f"updated {name}")
        except OSError as error:
            out.append(f"left {name} alone: {error}; it is an unedited older Keel copy, fix the file and run again")
            ok = False
    return ok


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


def write_advisor(root, model, out):
    """Returns True when the settings file is usable (written, or already decided)."""
    path = root / SETTINGS
    try:
        raw = path.read_bytes() if path.exists() else None
        if raw is None:
            settings, newline = {}, "\n"
        else:
            settings = json.loads(raw.decode("utf-8-sig"))
            newline = "\r\n" if b"\r\n" in raw else "\n"
        if not isinstance(settings, dict):
            out.append(f"left {SETTINGS} alone: it is not a JSON object; add \"{ADVISOR_KEY}\": \"{model}\" by hand")
            return False
        if ADVISOR_KEY in settings:
            out.append(f"kept {ADVISOR_KEY} in {SETTINGS}: the project already sets it")
            return True
        settings[ADVISOR_KEY] = model
        body = json.dumps(settings, indent=2, ensure_ascii=False).replace("\n", newline) + newline
        path.parent.mkdir(parents=True, exist_ok=True)
        replace_file(path, body.encode("utf-8"))
        out.append(f"set {ADVISOR_KEY} to {model} in {SETTINGS}")
        return True
    except (OSError, ValueError) as error:
        out.append(f"left {SETTINGS} alone: {error}; fix the file and run again")
        return False


def main(argv):
    parser = argparse.ArgumentParser(prog="keel_setup.py")
    parser.add_argument("root", nargs="?", default=".")
    choice = parser.add_mutually_exclusive_group()
    choice.add_argument("--advisor", metavar="MODEL", default=DEFAULT_ADVISOR)
    choice.add_argument("--no-advisor", action="store_true")
    args = parser.parse_args(argv[1:])
    root = Path(args.root).resolve()
    out = []
    copied = copy_files(root, out)
    write_pointer(root, out)
    usable = args.no_advisor or write_advisor(root, args.advisor, out)
    print("\n".join(out) or "Keel is already installed; nothing changed")
    return 0 if usable and copied else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
