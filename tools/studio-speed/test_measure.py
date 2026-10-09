"""Report contract: reject incomplete/non-finite samples and retain the slow tail.

No browser or Runtime needed; about 1 second, Python and Node. Existing journey
observers do not check timing-state isolation or the report's distributions.
"""
import importlib.util
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("measure", Path(__file__).with_name("measure.py"))
measure = importlib.util.module_from_spec(spec)
spec.loader.exec_module(measure)


class ReportTests(unittest.TestCase):
    def test_click_without_own_start_rejects_stale_previous_start(self):
        # Execute the actual observer JS, with browser I/O and clock substituted.
        # Node only, <1 second; covers timing state absent from report tests.
        for fires in (True, False):
            with self.subTest(click_reaches_listener=fires):
                scripts = ["global.window = {__speedStart: 7};",
                           "global.performance = {now: () => 0};",
                           "const control = new EventTarget();"]

                def evaluate(expression):
                    script = "\n".join(scripts + [
                        f"console.log(JSON.stringify(({expression}) ?? null));"])
                    value = subprocess.check_output(["node", "-e", script], text=True)
                    scripts.append(f"{expression};")
                    return json.loads(value)

                page = Mock(evaluate=evaluate)
                control = Mock()
                control.evaluate.side_effect = lambda js: evaluate(f"({js})(control)")
                control.click.side_effect = lambda: evaluate(
                    "control.dispatchEvent(new Event('click'))" if fires else "null")
                with patch.object(measure, "painted", return_value=100):
                    if fires:
                        # Zero is a valid fresh timestamp, not a missing start.
                        self.assertEqual(measure.click_time(page, control, Mock()), 100)
                    else:
                        with self.assertRaisesRegex(RuntimeError, "start"):
                            measure.click_time(page, control, Mock())

    def test_even_median_and_nearest_rank_p95_keep_slow_tail(self):
        result = measure.summarize([100, 1, 9, 2, 8, 3, 7, 4, 6, 5])
        self.assertEqual(result["medianMs"], 5.5)
        self.assertEqual(result["p95Ms"], 100)
        self.assertEqual(result["count"], 10)

    def test_partial_or_invalid_measurements_cannot_be_reported_as_success(self):
        for values in ([1] * 9, [1] * 11, [1] * 9 + [float("nan")],
                       [1] * 9 + [float("inf")], [1] * 9 + [-1]):
            with self.subTest(values=values), self.assertRaises(ValueError):
                measure.summarize(values)


if __name__ == "__main__":
    unittest.main()
