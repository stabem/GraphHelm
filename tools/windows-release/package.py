#!/usr/bin/env python3
"""Package a prebuilt Windows x86_64 GraphHelm executable for local review."""
import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import struct
import sys
import tempfile
import zipfile
from pathlib import Path

MAX_BINARY_BYTES = 512 * 1024 * 1024
CHUNK_BYTES = 1024 * 1024
TARGET = "x86_64-pc-windows-msvc"


class PackageError(Exception):
    """A safe, user-facing packaging refusal."""


def fail(message):
    raise PackageError(message)


def sha256_file(path):
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while True:
            chunk = stream.read(CHUNK_BYTES)
            if not chunk:
                break
            size += len(chunk)
            if size > MAX_BINARY_BYTES:
                fail(f"binary exceeds the {MAX_BINARY_BYTES}-byte limit")
            digest.update(chunk)
    return digest.hexdigest(), size


def validate_pe(path):
    owns_output = False
    try:
        with path.open("rb") as stream:
            head = stream.read(64)
            if len(head) < 64 or head[:2] != b"MZ":
                fail("input is not a valid PE executable")
            pe_offset = struct.unpack_from("<I", head, 0x3C)[0]
            if pe_offset < 64 or pe_offset > MAX_BINARY_BYTES - 6:
                fail("input has an invalid PE header offset")
            stream.seek(pe_offset)
            header = stream.read(6)
    except OSError as error:
        fail(f"cannot read binary: {error}")
    if len(header) != 6 or header[:4] != b"PE\0\0":
        fail("input has a missing or truncated PE header")
    machine = struct.unpack_from("<H", header, 4)[0]
    if machine != 0x8664:
        fail("input PE machine is not x86_64")


def load_binary(path):
    try:
        info = path.lstat()
    except OSError as error:
        fail(f"cannot inspect binary: {error}")
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode):
        fail("binary must be a regular, non-linked file")
    if info.st_size > MAX_BINARY_BYTES:
        fail(f"binary exceeds the {MAX_BINARY_BYTES}-byte limit")
    validate_pe(path)
    digest, size = sha256_file(path)
    if size != info.st_size:
        fail("binary changed while it was being read")
    return digest, size


def package(binary, output, source_sha):
    if not re.fullmatch(r"[0-9a-fA-F]{40}", source_sha):
        fail("source SHA must be exactly 40 hexadecimal characters")
    binary_hash, binary_size = load_binary(binary)
    if output.exists() or output.is_symlink():
        fail("output destination already exists")
    parent = output.parent
    if not parent.is_dir():
        fail("output parent directory must already exist")

    staging = Path(tempfile.mkdtemp(prefix=f".{output.name}.staging-", dir=parent))
    try:
        archive = staging / "graphhelm-windows-x86_64.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            bundle.write(binary, "bin/graphhelm.exe")
        archive_hash, archive_size = sha256_file(archive)
        manifest = {
            "format_version": 1,
            "target": TARGET,
            "source_sha": source_sha.lower(),
            "binary_sha256": binary_hash,
            "binary_size_bytes": binary_size,
            "archive_sha256": archive_hash,
            "archive_size_bytes": archive_size,
        }
        (staging / "manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n"
        )
        (staging / "SHA256SUMS").write_text(
            f"{archive_hash}  {archive.name}\n", encoding="ascii", newline="\n"
        )
        output.mkdir()
        owns_output = True
        for name in (archive.name, "manifest.json", "SHA256SUMS"):
            os.replace(staging / name, output / name)
    except Exception:
        if owns_output and output.exists():
            shutil.rmtree(output)
        raise
    finally:
        shutil.rmtree(staging, ignore_errors=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="prebuilt graphhelm.exe")
    parser.add_argument("--output", type=Path, required=True, help="new package directory")
    parser.add_argument("--source-sha", required=True, help="declared full 40-character source commit SHA")
    args = parser.parse_args(argv)
    try:
        package(args.binary, args.output, args.source_sha)
    except (PackageError, OSError, zipfile.BadZipFile) as error:
        print(f"package: {error}", file=sys.stderr)
        return 1
    print(f"created local package: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
