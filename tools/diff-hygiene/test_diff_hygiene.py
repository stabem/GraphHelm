"""diff_hygiene.py on scratch repos: each rule red on a branch with its defect, green on a clean one."""
from __future__ import annotations

import contextlib
import importlib.util
import io
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

SPEC = importlib.util.spec_from_file_location("diff_hygiene", Path(__file__).with_name("diff_hygiene.py"))
dh = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dh)

CARD = "## Keel card\n- **Paths:** `src/{a.txt,c.txt}`, `docs`\n- Promise: x\n"


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                          text=True, creationflags=NO_WINDOW).stdout


def write(repo: Path, rel: str, data: bytes) -> None:
    (repo / rel).parent.mkdir(parents=True, exist_ok=True)
    (repo / rel).write_bytes(data)


def commit(repo: Path, message: str) -> None:
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", message)


class DiffHygiene(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.origin, self.repo = root / "origin.git", root / "work"
        git(root, "init", "-q", "--bare", "-b", "main", str(self.origin))
        git(root, "clone", "-q", str(self.origin), str(self.repo))
        for key, value in (("user.name", "t"), ("user.email", "t@t"), ("core.autocrlf", "false")):
            git(self.repo, "config", key, value)
        write(self.repo, "src/a.txt", b"one\ntwo\n")
        write(self.repo, "src/b.txt", b"b\n")
        commit(self.repo, "base")
        git(self.repo, "push", "-q", "origin", "HEAD:main")
        git(self.repo, "checkout", "-q", "-b", "topic")
        self.card = root / "card.md"
        self.card.write_text(CARD, encoding="utf-8")

    def tearDown(self):
        self.tmp.cleanup()

    def run_check(self, *extra: str) -> tuple[int, str]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = dh.main(["--repo", str(self.repo), "--card", str(self.card), *extra])
        return code, out.getvalue()

    def test_clean_branch_passes(self):
        write(self.repo, "src/a.txt", b"one\ntwo\nthree\n")
        commit(self.repo, "clean")
        code, out = self.run_check()
        self.assertEqual(code, 0, out)
        self.assertIn("diff-hygiene: passed", out)

    def test_crlf_churn_fails(self):
        write(self.repo, "src/a.txt", b"one\r\ntwo\r\nthree\r\n")
        commit(self.repo, "crlf")
        code, out = self.run_check()
        self.assertEqual(code, 1, out)
        self.assertIn("line endings: src/a.txt", out)

    def test_stale_base_with_overlap_fails_and_without_overlap_passes(self):
        write(self.repo, "src/a.txt", b"one\ntwo\nmine\n")
        commit(self.repo, "topic change")
        git(self.repo, "checkout", "-q", "main")
        write(self.repo, "docs/other.md", b"x\n")
        commit(self.repo, "unrelated main change")
        git(self.repo, "push", "-q", "origin", "main")
        git(self.repo, "checkout", "-q", "topic")
        code, out = self.run_check()
        self.assertEqual(code, 0, out)
        git(self.repo, "checkout", "-q", "main")
        write(self.repo, "src/a.txt", b"one\ntwo\nfrom main\n")
        commit(self.repo, "main touches a")
        git(self.repo, "push", "-q", "origin", "main")
        git(self.repo, "checkout", "-q", "topic")
        code, out = self.run_check()
        self.assertEqual(code, 1, out)
        self.assertIn("stale base: src/a.txt", out)

    def test_path_outside_card_fails_unless_allowed_or_summary_only(self):
        write(self.repo, "src/b.txt", b"b changed\n")
        write(self.repo, "Cargo.lock", b"lock\n")
        commit(self.repo, "out of scope")
        code, out = self.run_check()
        self.assertEqual(code, 1, out)
        self.assertIn("scope: src/b.txt outside", out)
        self.assertNotIn("Cargo.lock", out)
        self.card.write_text("Summary only: docs tweak.\n", encoding="utf-8")
        code, out = self.run_check()
        self.assertEqual(code, 0, out)

    def test_unreadable_card_or_unfetchable_base_is_unobserved(self):
        write(self.repo, "src/a.txt", b"one\ntwo\nthree\n")
        commit(self.repo, "clean")
        code, out = self.run_check("--card", str(self.card) + ".missing")
        self.assertEqual((code, out.split(" |")[0]), (2, "diff-hygiene: unobserved"))
        git(self.repo, "remote", "set-url", "origin", str(self.origin) + ".gone")
        code, out = self.run_check()
        self.assertEqual((code, out.split(" |")[0]), (2, "diff-hygiene: unobserved"))

    def test_a_rename_counts_as_its_old_and_new_name(self):
        git(self.repo, "mv", "src/a.txt", "src/z.txt")
        commit(self.repo, "rename a")
        git(self.repo, "checkout", "-q", "main")
        write(self.repo, "src/a.txt", b"one\ntwo\nfrom main\n")
        commit(self.repo, "main edits a")
        git(self.repo, "push", "-q", "origin", "main")
        git(self.repo, "checkout", "-q", "topic")
        code, out = self.run_check()
        self.assertEqual(code, 1, out)
        self.assertIn("stale base: src/a.txt", out)
        self.assertIn("scope: src/z.txt outside", out)
        self.assertNotIn("=>", out)

    def test_a_bare_base_is_fetched_from_origin_or_unobserved(self):
        write(self.repo, "src/a.txt", b"one\ntwo\nmine\n")
        commit(self.repo, "topic change")
        other = Path(self.tmp.name) / "other"
        git(Path(self.tmp.name), "clone", "-q", str(self.origin), str(other))
        for key, value in (("user.name", "t"), ("user.email", "t@t")):
            git(other, "config", key, value)
        write(other, "src/a.txt", b"one\ntwo\nfrom main\n")
        commit(other, "main edits a elsewhere")
        git(other, "push", "-q", "origin", "main")
        code, out = self.run_check("--base", "main")
        self.assertEqual(code, 1, out)
        self.assertIn("stale base: src/a.txt", out)
        git(self.repo, "remote", "set-url", "origin", str(self.origin) + ".gone")
        code, out = self.run_check("--base", "main")
        self.assertEqual((code, out.split(" |")[0]), (2, "diff-hygiene: unobserved"))

    def test_the_card_scope_is_read_like_keel_check(self):
        write(self.repo, "src/b.txt", b"b changed\n")
        commit(self.repo, "touch b")
        self.card.write_text("- **Scope:** `src/b.txt`\n- Promise: x\n", encoding="utf-8")
        self.assertEqual(self.run_check()[0], 0)
        self.card.write_text("Scope paths:\n- `src/a.txt`\n\nProof: x\n", encoding="utf-8")
        code, out = self.run_check()
        self.assertEqual(code, 1, out)
        self.assertIn("scope: src/b.txt outside", out)


if __name__ == "__main__":
    unittest.main()
