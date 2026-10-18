"""Contracts of the #1476 planted-defect runner, without building anything.

The anchors are checked against the real `core/events/src/local.rs`: when that file moves, the
runner must report the defect UNOBSERVED, and this test says so first. The runner itself is driven
with a fake cargo, so these cells cost milliseconds.
"""
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
SPEC = importlib.util.spec_from_file_location("plant_defects", HERE / "plant_defects.py")
pd = importlib.util.module_from_spec(SPEC)
sys.modules["plant_defects"] = pd
SPEC.loader.exec_module(pd)

GREEN = (0, "test result: ok. 3 passed; 0 failed")
RED = (101, "test a ... FAILED\ntest result: FAILED. 2 passed; 1 failed")
BROKEN = (101, "error[E0308]: mismatched types\nerror: could not compile `graphhelm-events`")


class Anchors(unittest.TestCase):
    def test_every_anchor_matches_the_event_store_exactly_once(self):
        source = (REPO / pd.TARGET).read_text(encoding="utf-8")
        self.assertEqual(len(pd.DEFECTS), 5)
        for defect in pd.DEFECTS:
            with self.subTest(defect=defect.id):
                self.assertEqual(source.count(defect.anchor), 1)
                mutated = pd.apply(source, defect)
                self.assertNotEqual(mutated, source)
                self.assertEqual(len(mutated) - len(source), len(defect.replacement) - len(defect.anchor))

    def test_a_drifted_anchor_is_not_applied(self):
        defect = pd.DEFECTS[0]
        self.assertIsNone(pd.apply("no such code", defect))
        self.assertIsNone(pd.apply(defect.anchor * 2, defect))


class Runner(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        target = self.repo / pd.TARGET
        target.parent.mkdir(parents=True)
        self.original = ("// header\r\n" + "\n".join(d.anchor for d in pd.DEFECTS) + "\n").encode()
        target.write_bytes(self.original)

    def tearDown(self):
        self.tmp.cleanup()

    def drive(self, results):
        seen = []
        answers = iter(results)

        def runner(repo):
            seen.append((repo / pd.TARGET).read_bytes())
            return next(answers)
        report = pd.plant(self.repo, runner)
        self.assertEqual((self.repo / pd.TARGET).read_bytes(), self.original)
        return report, seen

    def test_red_is_caught_green_is_missed_and_broken_is_unobserved(self):
        report, seen = self.drive([GREEN, RED, GREEN, BROKEN, RED, RED])
        self.assertEqual([row["verdict"] for row in report["defects"]],
                         ["caught", "missed", "UNOBSERVED", "caught", "caught"])
        self.assertEqual(report["caught"], 3)
        self.assertEqual(report["status"], "UNOBSERVED")
        self.assertEqual(seen[0], self.original)
        for mutated, defect in zip(seen[1:], pd.DEFECTS):
            self.assertIn(defect.replacement.encode(), mutated)

    def test_all_caught_passes_and_three_caught_kills(self):
        self.assertEqual(self.drive([GREEN] + [RED] * 5)[0]["status"], "pass")
        report = self.drive([GREEN, RED, RED, RED, GREEN, GREEN])[0]
        self.assertEqual((report["caught"], report["status"]), (3, "kill"))

    def test_a_red_baseline_observes_nothing(self):
        report, seen = self.drive([RED])
        self.assertEqual(len(seen), 1)
        self.assertEqual({row["verdict"] for row in report["defects"]}, {"UNOBSERVED"})

    def test_the_file_is_restored_when_the_runner_raises(self):
        def runner(repo):
            if (repo / pd.TARGET).read_bytes() != self.original:
                raise KeyboardInterrupt
            return GREEN
        with self.assertRaises(KeyboardInterrupt):
            pd.plant(self.repo, runner)
        self.assertEqual((self.repo / pd.TARGET).read_bytes(), self.original)

    def test_a_drifted_anchor_is_unobserved_not_caught(self):
        (self.repo / pd.TARGET).write_bytes(b"nothing matches\n")
        self.original = b"nothing matches\n"
        report, seen = self.drive([GREEN])
        self.assertEqual(len(seen), 1)
        self.assertEqual({row["verdict"] for row in report["defects"]}, {"UNOBSERVED"})


if __name__ == "__main__":
    unittest.main()
