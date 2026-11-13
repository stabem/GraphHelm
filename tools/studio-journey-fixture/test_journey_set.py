"""#2863: the journeys the Studio fixture lists are the ids in journeys.list, not every flow in the
repository, so the count on screen changes only when that list is edited on purpose.

Runs journey_set.sh on scratch directories; needs bash only (no Node, Docker or Runtime).
"""
import os
import pathlib
import re
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SCRIPT = HERE / "journey_set.sh"
LIST = HERE / "journeys.list"
SHELL = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else "bash"
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)
# #2869: a studio2 flow whose own journey is never shown in Proof is not listed, so the total that
# flows pin ("1 of 24 journeys") does not move for it. Each flow below is exempt for its own reason.
UNLISTED = {
    "studio2-proof-run-progress": "it presses Run on report-slow, which fixture.sh seeds for this flow alone",
    "studio2-add-refusal": "it is HTTP-only and needs no journey card seeded in Proof; listing it would move the pinned total",
}


def run(*args):
    return subprocess.run([SHELL, str(SCRIPT), *map(str, args)], capture_output=True, text=True,
                          timeout=60, check=False, creationflags=NO_WINDOW)


def listed(path):
    return [re.sub(r"#.*", "", line).strip() for line in path.read_text(encoding="utf-8").splitlines()
            if re.sub(r"#.*", "", line).strip()]


class JourneySet(unittest.TestCase):
    def scratch(self, parent, ids, extra=()):
        journeys = pathlib.Path(parent) / "journeys"
        journeys.mkdir()
        for name in (*ids, *extra):
            (journeys / f"{name}.journey.yaml").write_text(f"id: {name}\n", encoding="utf-8")
            (journeys / f"{name}.json").write_text("{}", encoding="utf-8")
        return journeys

    def test_a_flow_added_to_the_repository_does_not_change_what_the_fixture_lists(self):
        with tempfile.TemporaryDirectory() as parent:
            journeys = self.scratch(parent, ["alpha", "beta"], extra=["gamma"])
            names = pathlib.Path(parent) / "names"
            names.write_text("# why\nalpha\nbeta  # kept\n", encoding="utf-8")
            first = pathlib.Path(parent) / "first"
            self.assertEqual(run("copy", journeys, names, first).returncode, 0)
            expected = {"alpha.journey.yaml", "alpha.json", "beta.journey.yaml", "beta.json"}
            self.assertEqual({p.name for p in first.iterdir()}, expected, "gamma is in the repository, not on the list")
            (journeys / "delta.journey.yaml").write_text("id: delta\n", encoding="utf-8")
            second = pathlib.Path(parent) / "second"
            self.assertEqual(run("copy", journeys, names, second).returncode, 0)
            self.assertEqual({p.name for p in second.iterdir()}, expected, "a new flow file changes nothing")

    def test_a_listed_id_with_no_flow_fails_by_name(self):
        with tempfile.TemporaryDirectory() as parent:
            journeys = self.scratch(parent, ["alpha"])
            names = pathlib.Path(parent) / "names"
            names.write_text("alpha\nbeta\n", encoding="utf-8")
            for mode in (["check"], ["copy"]):
                args = [*mode, journeys, names] + ([pathlib.Path(parent) / "out"] if mode == ["copy"] else [])
                result = run(*args)
                self.assertEqual(result.returncode, 3, result.stderr)
                self.assertIn("names the journey 'beta' but .graphhelm/journeys/beta.journey.yaml does not exist",
                              result.stderr)
            self.assertFalse((pathlib.Path(parent) / "out").exists(), "nothing is copied from a list that fails")

    def test_the_real_list_resolves_against_the_repository_and_has_no_duplicates(self):
        ids = listed(LIST)
        self.assertEqual(len(ids), len(set(ids)), "an id is listed twice")
        result = run("check", ROOT / ".graphhelm" / "journeys", LIST)
        self.assertEqual(result.returncode, 0, result.stderr)
        for flow in sorted((ROOT / ".graphhelm" / "journeys").glob("studio2-*.journey.yaml")):
            flow_id = flow.name.removesuffix(".journey.yaml")
            if flow_id in UNLISTED:
                self.assertNotIn(flow_id, ids, f"{flow_id} is exempt from the list; {UNLISTED[flow_id]}")
                continue
            self.assertIn(flow_id, ids, "every studio2 flow is played on this fixture, so it is listed")


if __name__ == "__main__":
    unittest.main()
