import hashlib
import json
import os
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
COMMAND = ROOT / "tools" / "windows-release" / "package.py"


def pe_fixture(machine=0x8664, truncated=False):
    data = bytearray(128)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<HH", data, 68, machine, 0)
    if truncated:
        return bytes(data[:69])
    return bytes(data)


class PackageCommandTests(unittest.TestCase):
    def invoke(self, binary, output, revision="a" * 40):
        return subprocess.run(
            [sys.executable, str(COMMAND), "--binary", str(binary), "--output", str(output), "--source-sha", revision],
            capture_output=True,
            text=True,
            check=False,
        )

    def test_creates_zip_manifest_and_checksum_for_exact_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "graphhelm.exe"
            content = pe_fixture()
            binary.write_bytes(content)
            output = root / "release"

            result = self.invoke(binary, output)

            self.assertEqual(result.returncode, 0, result.stderr)
            archive = output / "graphhelm-windows-x86_64.zip"
            manifest_path = output / "manifest.json"
            checksums = output / "SHA256SUMS"
            with zipfile.ZipFile(archive) as bundle:
                self.assertEqual(bundle.namelist(), ["bin/graphhelm.exe"])
                self.assertEqual(bundle.read("bin/graphhelm.exe"), content)
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(manifest["format_version"], 1)
            self.assertEqual(manifest["target"], "x86_64-pc-windows-msvc")
            self.assertEqual(manifest["source_sha"], "a" * 40)
            self.assertEqual(manifest["binary_sha256"], hashlib.sha256(content).hexdigest())
            self.assertEqual(manifest["binary_size_bytes"], len(content))
            archive_bytes = archive.read_bytes()
            self.assertEqual(manifest["archive_sha256"], hashlib.sha256(archive_bytes).hexdigest())
            self.assertEqual(manifest["archive_size_bytes"], len(archive_bytes))
            self.assertEqual(checksums.read_text(encoding="ascii"),
                             f"{hashlib.sha256(archive_bytes).hexdigest()}  {archive.name}\n")

    def test_rejects_invalid_source_revision_without_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "graphhelm.exe"
            binary.write_bytes(pe_fixture())
            output = root / "release"
            result = self.invoke(binary, output, "not-a-full-sha")
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())

    def test_rejects_missing_non_regular_linked_and_non_pe_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "release"
            result = self.invoke(root / "missing.exe", output)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())

            folder = root / "folder.exe"
            folder.mkdir()
            self.assertNotEqual(self.invoke(folder, output).returncode, 0)

            binary = root / "plain.exe"
            binary.write_bytes(b"not a PE")
            self.assertNotEqual(self.invoke(binary, output).returncode, 0)
            self.assertFalse(output.exists())

            linked = root / "linked.exe"
            try:
                linked.symlink_to(binary)
            except (OSError, NotImplementedError):
                self.skipTest("symlink creation is unavailable")
            self.assertNotEqual(self.invoke(linked, output).returncode, 0)
            self.assertFalse(output.exists())

    def test_rejects_truncated_and_wrong_machine_pe(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, data in (("short.exe", pe_fixture(truncated=True)),
                               ("x86.exe", pe_fixture(machine=0x14C))):
                binary = root / name
                binary.write_bytes(data)
                output = root / (name + ".package")
                self.assertNotEqual(self.invoke(binary, output).returncode, 0)
                self.assertFalse(output.exists())

    def test_existing_destination_is_preserved_and_no_partial_package_is_advertised(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "graphhelm.exe"
            binary.write_bytes(pe_fixture())
            output = root / "release"
            output.mkdir()
            marker = output / "keep.txt"
            marker.write_text("keep", encoding="utf-8")
            result = self.invoke(binary, output)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(marker.read_text(encoding="utf-8"), "keep")
            self.assertEqual(list(output.iterdir()), [marker])


if __name__ == "__main__":
    unittest.main()
