"""Release archive layout and checksum (#2376).

Cost: a temp directory, a fixture "binary" (a few bytes) and a fixture repo tree; no Cargo, no
network, no process. The smoke test needs a real GraphHelm build and is exercised at release.
"""
import hashlib
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import package_release as pr  # noqa: E402


def fixture_repo(root: Path) -> Path:
    repo = root / "repo"
    for path, text in {
        "LICENSE": "license\n",
        "extensions/builtin/pkg/extension.json": "{}\n",
        "extensions/releases/adoption.json": '{"version": "1.0.0"}\n',
    }.items():
        (repo / path).parent.mkdir(parents=True, exist_ok=True)
        (repo / path).write_text(text, encoding="utf-8")
    return repo


class ReleaseArchive(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = fixture_repo(self.root)

    def build(self, name: str, target: str) -> Path:
        binary = self.root / name
        binary.write_bytes(b"fixture binary")
        staged = self.root / f"stage-{target}"
        pr.stage(binary, self.repo, staged)
        out = self.root / f"out-{target}"
        out.mkdir()
        return pr.archive(staged, out, target)

    def test_each_archive_holds_the_binary_extensions_beside_it_license_and_readme(self):
        """The adoption loader resolves extensions/releases next to the executable, so the archive
        must carry both extension trees beside the binary."""
        expected = {"LICENSE", "README.md", "extensions/builtin/pkg/extension.json",
                    "extensions/releases/adoption.json"}
        for name, target in (("graphhelm.exe", "windows-x86_64"), ("graphhelm", "ubuntu-24.04-x86_64")):
            with self.subTest(target=target):
                path = self.build(name, target)
                if path.suffix == ".zip":
                    with zipfile.ZipFile(path) as zipped:
                        names = {n for n in zipped.namelist() if not n.endswith("/")}
                else:
                    with tarfile.open(path) as tarred:
                        names = {m.name for m in tarred.getmembers() if m.isfile()}
                prefix = f"graphhelm-{target}/"
                self.assertTrue(all(n.startswith(prefix) for n in names), names)
                self.assertEqual({n[len(prefix):] for n in names}, expected | {name})

    def test_the_checksum_is_the_archives_sha256(self):
        path = self.build("graphhelm", "ubuntu-24.04-x86_64")
        self.assertEqual(pr.sha256(path), hashlib.sha256(path.read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
