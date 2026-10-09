"""Report contract: reject incomplete/non-finite samples and retain the slow tail.

No browser or Runtime needed; <1 second. Existing journey observers do not check
the measurement report's percentile convention or reject partial distributions.
"""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("measure", Path(__file__).with_name("measure.py"))
measure = importlib.util.module_from_spec(spec)
spec.loader.exec_module(measure)


class ReportTests(unittest.TestCase):
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
