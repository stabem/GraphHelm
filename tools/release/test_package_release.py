"""Release archive layout and the SHA256SUMS.txt bytes.

Cost: a temp directory, a fixture "binary" (a few bytes) and a fixture repo tree; no Cargo, no
network, no process. The smoke test needs a real GraphHelm build and is exercised at release.
"""
import contextlib
import hashlib
import io
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

    def run_main(self, target: str, name: str, **extra) -> Path:
        """`main()` on the fixture, the smoke step replaced by a no-op (it needs a real build)."""
        binary = self.root / name
        binary.write_bytes(b"fixture binary")
        out = self.root / f"release-{target}"
        argv = ["--binary", str(binary), "--repo", str(self.repo), "--graph", str(binary),
                "--target", target, "--out", str(out)]
        original = pr.smoke
        pr.smoke = lambda *_: ["smoke skipped in this unit test"]
        self.addCleanup(setattr, pr, "smoke", original)
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(pr.main(argv), 0)
        return out

    def test_sha256sums_txt_is_the_archives_hash_with_lf_endings_for_sha256sum_c(self):
        """Exact bytes: `<sha256 of the ARCHIVE>  <archive name>\\n`, LF only, so `sha256sum -c`
        works on Windows output too. Red if the file holds the binary's hash or CRLF."""
        for target, name in (("windows-x86_64", "graphhelm.exe"), ("ubuntu-24.04-x86_64", "graphhelm")):
            with self.subTest(target=target):
                out = self.run_main(target, name)
                archive_path = next(p for p in out.iterdir() if p.name.startswith("graphhelm-"))
                expected = f"{hashlib.sha256(archive_path.read_bytes()).hexdigest()}  {archive_path.name}\n"
                self.assertEqual((out / "SHA256SUMS.txt").read_bytes(), expected.encode("utf-8"))
                self.assertNotIn(b"\r", (out / "SHA256SUMS.txt").read_bytes())
                self.assertNotEqual(
                    expected.split()[0], hashlib.sha256(b"fixture binary").hexdigest(),
                    "the checksum must cover the archive, not the binary")

    def test_a_failed_run_leaves_no_output_directory_behind(self):
        out = self.root / "failed"
        with self.assertRaises(SystemExit):
            pr.main(["--binary", str(self.root / "missing"), "--repo", str(self.repo),
                     "--graph", str(self.root), "--target", "windows-x86_64", "--out", str(out)])
        self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
