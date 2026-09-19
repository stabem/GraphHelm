#!/usr/bin/env python3
"""Copy one untrusted regular file into a root-controlled destination safely."""

from __future__ import annotations

import argparse
import os
import stat
import sys


BUFFER_BYTES = 1024 * 1024
OPEN_DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW


class UnsafeFileError(Exception):
    """The untrusted pathname or inode did not satisfy the sealing boundary."""


def open_parent_without_symlinks(path: str) -> tuple[int, str]:
    normalized = os.path.abspath(path)
    if not os.path.isabs(path) or normalized != path:
        raise UnsafeFileError("path must be absolute and normalized")
    components = normalized.split(os.sep)
    leaf = components.pop()
    if not leaf or leaf in {".", ".."}:
        raise UnsafeFileError("path has no safe final component")

    descriptor = os.open(os.sep, OPEN_DIRECTORY_FLAGS)
    try:
        for component in components[1:]:
            if not component or component in {".", ".."}:
                raise UnsafeFileError("path contains an unsafe component")
            next_descriptor = os.open(
                component,
                OPEN_DIRECTORY_FLAGS,
                dir_fd=descriptor,
            )
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor, leaf
    except Exception:
        os.close(descriptor)
        raise


def inode_identity(metadata: os.stat_result) -> tuple[int, ...]:
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_uid,
        metadata.st_gid,
        metadata.st_nlink,
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def seal_file(
    source: str, destination: str, mode: int, max_bytes: "int | None" = None
) -> None:
    # VOLUME IS A SEPARATE THREAT FROM SUBSTITUTION, and this helper was built against the second.
    # `O_NOFOLLOW`, the root-controlled parent and the inode-identity re-check all answer "is this
    # still the same file"; not one of them answers "how much of it is there". The callers measured
    # the size AFTER the copy -- `backup-vps.sh:187` seals and only rejects at `:194` -- so a
    # service-owned file grown to fill the disk had already filled it by the time the limit spoke.
    #
    # THREE PARTS, and the first alone is not enough. `st_size` is a snapshot: a source that grows
    # WHILE the copy runs defeats a pre-flight check on its own, and the pre-flight-only version is
    # the one that looks sufficient and is the most likely to be written. So the budget is also
    # enforced against bytes actually read, and the identity re-check that already existed stays.
    #
    # `max_bytes = None` keeps the helper usable where no caller has a limit to state, rather than
    # inventing one here: a default ceiling in a shared helper is a policy decision belonging to the
    # script that knows what it is sealing.
    source_parent, source_leaf = open_parent_without_symlinks(source)
    destination_parent = -1
    source_descriptor = -1
    destination_descriptor = -1
    destination_created = False
    destination_leaf = ""
    try:
        source_descriptor = os.open(
            source_leaf,
            # O_NONBLOCK makes a FIFO (or another special file) fail closed at the descriptor
            # boundary instead of waiting forever for a writer before the regular-file check.
            os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC | os.O_NOFOLLOW,
            dir_fd=source_parent,
        )
        before = os.fstat(source_descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise UnsafeFileError("source is not a regular file")
        # PART ONE: refuse before the destination is CREATED, so an oversized source costs no
        # bytes on the destination filesystem at all. The number was already in hand ten lines
        # above the copy and was never read.
        if max_bytes is not None and before.st_size > max_bytes:
            raise UnsafeFileError("source exceeds the caller's byte budget")

        destination_parent, destination_leaf = open_parent_without_symlinks(destination)
        parent_metadata = os.fstat(destination_parent)
        if parent_metadata.st_uid != 0 or parent_metadata.st_mode & 0o022:
            raise UnsafeFileError("destination parent is not root-controlled")

        destination_descriptor = os.open(
            destination_leaf,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW,
            mode,
            dir_fd=destination_parent,
        )
        destination_created = True

        copied = 0
        while True:
            chunk = os.read(source_descriptor, BUFFER_BYTES)
            if not chunk:
                break
            # PART TWO: the budget is enforced against bytes actually read, because `st_size` was a
            # snapshot taken before `O_CREAT` and a source that grows during the copy walks straight
            # past it. This is the half a pre-flight-only fix leaves open.
            copied += len(chunk)
            if max_bytes is not None and copied > max_bytes:
                raise UnsafeFileError("source grew past the caller's byte budget while sealing")
            view = memoryview(chunk)
            while view:
                written = os.write(destination_descriptor, view)
                if written <= 0:
                    raise OSError("short write while sealing file")
                view = view[written:]

        os.fchmod(destination_descriptor, mode)
        os.fsync(destination_descriptor)
        after = os.fstat(source_descriptor)
        if inode_identity(after) != inode_identity(before):
            raise UnsafeFileError("source changed while it was being sealed")
        os.fsync(destination_parent)
    except (OSError, UnsafeFileError) as error:
        if destination_created and destination_parent >= 0 and destination_leaf:
            try:
                os.unlink(destination_leaf, dir_fd=destination_parent)
                os.fsync(destination_parent)
            except OSError:
                pass
        raise UnsafeFileError(str(error)) from error
    finally:
        for descriptor in (
            destination_descriptor,
            destination_parent,
            source_descriptor,
            source_parent,
        ):
            if descriptor >= 0:
                os.close(descriptor)


def parse_mode(value: str) -> int:
    try:
        mode = int(value, 8)
    except ValueError as error:
        raise argparse.ArgumentTypeError("mode must be octal") from error
    if mode < 0 or mode > 0o777:
        raise argparse.ArgumentTypeError("mode must be between 0000 and 0777")
    return mode


def parse_max_bytes(value: str) -> int:
    try:
        limit = int(value, 10)
    except ValueError as error:
        raise argparse.ArgumentTypeError("max-bytes must be a decimal integer") from error
    # ZERO IS REFUSED RATHER THAN TREATED AS "NO LIMIT". A caller whose variable expanded to empty
    # or to 0 would otherwise get the unlimited behaviour it was trying to leave behind, which is
    # the failure direction that produces a silent regression instead of a loud one.
    if limit <= 0:
        raise argparse.ArgumentTypeError("max-bytes must be greater than zero")
    return limit


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True)
    parser.add_argument("--destination", required=True)
    parser.add_argument("--mode", required=True, type=parse_mode)
    # PART THREE: without this the limit is unreachable from the outside. The parser accepted only
    # source, destination and mode, so no caller could impose a budget even when it had one sitting
    # in a constant -- which `backup-vps.sh` did, and spent after the copy instead of before it.
    parser.add_argument("--max-bytes", type=parse_max_bytes, default=None)
    arguments = parser.parse_args()
    try:
        seal_file(
            arguments.source, arguments.destination, arguments.mode, arguments.max_bytes
        )
    except (OSError, UnsafeFileError):
        print("graphhelm file seal: unsafe untrusted file", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
