import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import wake_webhook_check as check

URL = "https://signal.openbot.run/v1/webhooks/0fe3b54f-c2c9-433d-a2b1-1bc1004b79dc"


def project(urls, secrets=None):
    root = Path(tempfile.mkdtemp())
    (root / "wake-webhooks.json").write_text(json.dumps(urls), encoding="utf-8")
    (root / "wake-secrets").mkdir()
    for lane, value in (secrets or {}).items():
        (root / "wake-secrets" / f"{lane}.secret").write_text(value, encoding="utf-8")
    return root


def run(argv):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = check.main(argv)
    return code, out.getvalue()


class SignatureTest(unittest.TestCase):
    def test_it_is_the_rfc_4231_hmac_the_runtime_uses(self):
        # The Runtime's own test vector (RFC 4231 case 1) through the same message layout.
        digest = check.hmac.new(b"\x0b" * 20, b"Hi There", check.hashlib.sha256).hexdigest()
        self.assertEqual(digest, "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
        self.assertEqual(check.signature("k", 1, "id", "{}"),
                         "sha256=" + check.hmac.new(b"k", b"1.id.{}", check.hashlib.sha256).hexdigest())


class DryRunTest(unittest.TestCase):
    def test_a_ready_lane_says_so_and_sends_nothing_and_never_prints_the_secret_or_path(self):
        root = project({"openbot-neji": URL}, {"openbot-neji": "hunter2\n"})
        with mock.patch.object(check.urllib.request, "OpenerDirector") as opener:
            code, out = run(["--dir", str(root), "--lane", "openbot-neji"])
        self.assertEqual(code, 0, out)
        self.assertIn("dry run: nothing was sent", out)
        self.assertNotIn("hunter2", out)
        self.assertNotIn("0fe3b54f", out)
        opener.assert_not_called()

    def test_every_missing_piece_is_named(self):
        root = project({"someone-else": URL})
        code, out = run(["--dir", str(root), "--lane", "openbot-neji"])
        self.assertEqual(code, 1)
        self.assertIn("has no url for openbot-neji", out)
        self.assertIn("openbot-neji.secret does not exist", out)
        self.assertIn("NOT READY", out)

    def test_one_bad_url_turns_the_whole_file_off_as_the_runtime_does(self):
        root = project({"openbot-neji": URL, "openbot-sai": "http://evil.example/x"}, {"openbot-neji": "s"})
        code, out = run(["--dir", str(root), "--lane", "openbot-neji"])
        self.assertEqual(code, 1)
        self.assertIn("openbot-sai are not https urls", out)
        self.assertIn("turns the whole file off", out)

    def test_a_lane_name_that_is_a_path_is_refused(self):
        root = project({"x": URL}, {"x": "s"})
        code, out = run(["--dir", str(root), "--lane", "../x"])
        self.assertEqual(code, 1)
        self.assertIn("not a file-name-safe lane name", out)

    def test_url_pin(self):
        for bad in ("http://signal.openbot.run/x", "https://signal.openbot.run.evil.example/x",
                    "https://u:p@signal.openbot.run/x", "https://signal.openbot.run:8443/x", "https://signal.openbot.run"):
            self.assertIsNotNone(check.url_problem(bad), bad)
        self.assertIsNone(check.url_problem(URL))


class SendTest(unittest.TestCase):
    def test_send_posts_one_signed_request_and_prints_only_the_status(self):
        root = project({"openbot-neji": URL}, {"openbot-neji": "hunter2"})
        seen = {}

        class Reply:
            status = 202

        class Opener:
            def open(self, request, timeout):
                seen["url"] = request.full_url
                seen["headers"] = {k.lower(): v for k, v in request.header_items()}
                seen["body"] = request.data.decode()
                return Reply()

        with mock.patch.object(check.urllib.request, "build_opener", return_value=Opener()):
            code, out = run(["--dir", str(root), "--lane", "openbot-neji", "--send"])
        self.assertEqual(code, 0, out)
        self.assertIn("HTTP 202", out)
        self.assertNotIn("hunter2", out)
        self.assertNotIn(seen["headers"]["x-openbot-signature"], out)
        stamp, delivery_id = int(seen["headers"]["x-openbot-timestamp"]), seen["headers"]["x-openbot-delivery-id"]
        self.assertEqual(seen["headers"]["x-openbot-signature"],
                         check.signature("hunter2", stamp, delivery_id, seen["body"]))
        self.assertTrue(delivery_id.startswith("openbot-neji:selftest:"))
        self.assertEqual(json.loads(seen["body"])["type"], "graphhelm.step.ready")


if __name__ == "__main__":
    unittest.main()
