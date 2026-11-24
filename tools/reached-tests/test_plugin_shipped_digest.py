"""A plugin file that changes under an unchanged manifest version fails here (#3006).

Cost: reads the plugin files once and builds a throwaway git repository; offline.
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import plugin_shipped_digest as shipped  # noqa: E402


NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


class ShippedDigests(unittest.TestCase):
    def test_checked_in_record_matches_the_plugin_files(self):
        self.assertEqual(shipped.problems(shipped.REPO, shipped.load()), [])


class ChangeWithoutBump(unittest.TestCase):
    """Red when a file changes under the same version, green after the bump and re-record."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        for roots, manifests in shipped.guard.GROUPS:
            for manifest in manifests:
                self.put(manifest, json.dumps({"version": "1.0.0"}))
            self.put(roots[0] + "skills/x/SKILL.md", "one\n")
        subprocess.run(["git", "init", "-q"], cwd=self.repo, check=True, timeout=30, creationflags=NO_WINDOW)
        self.record = {g: {v: d} for g, (v, d) in shipped.current(self.repo).items()}

    def put(self, name, text):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(text.encode())

    def test_unchanged_tree_is_clean(self):
        self.assertEqual(shipped.problems(self.repo, self.record), [])

    def test_changed_file_with_same_version_is_red_and_bump_with_record_is_green(self):
        group = shipped.guard.GROUPS[1][0][0]
        self.put(group + "skills/x/SKILL.md", "two\n")
        found = shipped.problems(self.repo, self.record)
        self.assertEqual(len(found), 1)
        self.assertIn("stayed 1.0.0", found[0])
        for manifest in shipped.guard.GROUPS[1][1]:
            self.put(manifest, json.dumps({"version": "1.0.1"}))
        self.assertIn("no recorded digest", shipped.problems(self.repo, self.record)[0])
        self.assertEqual(shipped.record_current(self.repo, self.record), [])
        self.assertEqual(shipped.problems(self.repo, self.record), [])

    def test_write_refuses_to_overwrite_the_digest_of_an_unchanged_version(self):
        group = shipped.guard.GROUPS[1][0][0]
        self.put(group + "skills/x/SKILL.md", "two\n")
        before = json.dumps(self.record, sort_keys=True)
        refused = shipped.record_current(self.repo, self.record)
        self.assertEqual(len(refused), 1)
        self.assertIn("already has a different recorded digest", refused[0])
        self.assertEqual(json.dumps(self.record, sort_keys=True), before)

    def test_a_test_file_change_needs_no_bump(self):
        group = shipped.guard.GROUPS[1][0][0]
        self.put(group + "hooks/test_x.py", "assert True\n")
        self.assertEqual(shipped.problems(self.repo, self.record), [])


if __name__ == "__main__":
    unittest.main()
