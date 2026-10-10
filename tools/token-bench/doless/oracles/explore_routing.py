"""Frozen structured assessment: compare conclusions, not prose or keyword presence.
Arrays are sets of claims (order irrelevant); extra claims and missing facts fail.
"""
import json
from _lib import answer, fail, git_changed_paths, ok

EXPECTED = {'initial': {'tier': 'small', 'effort': 'low'}, 'oneRed': {'tier': 'standard', 'effort': 'high'}, 'nineRed': {'tier': 'large', 'effort': 'high'}, 'callsModel': False}

if git_changed_paths():
    fail("read-only assessment changed the checkout")
try:
    actual = json.loads(answer())
except (ValueError, TypeError):
    fail("answer must be a JSON assessment")
if not isinstance(actual, dict) or actual.keys() != EXPECTED.keys():
    fail("assessment has missing or additional claims")
for key, expected in EXPECTED.items():
    value = actual[key]
    if type(value) is not type(expected):
        fail(f"wrong conclusion type: {key}")
    if isinstance(expected, list):
        if any(not isinstance(item, str) for item in value) or sorted(value) != sorted(expected):
            fail(f"wrong claims: {key}")
    elif value != expected:
        fail(f"wrong conclusion: {key}")
ok()
