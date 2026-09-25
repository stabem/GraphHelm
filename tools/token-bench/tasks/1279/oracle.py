"""Acceptance oracle for the Windows legacy-code-page prompt regression."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile


RUNNER_PATH = Path(__file__).resolve().parents[2] / "run.py"
spec = importlib.util.spec_from_file_location("token_bench_runner", RUNNER_PATH)
if spec is None or spec.loader is None:
    raise RuntimeError(f"cannot load runner from {RUNNER_PATH}")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


def main() -> int:
    # The child is real: the runner must encode input and pass it through an actual
    # subprocess boundary. The patched locale models a Windows host whose default
    # text encoding is cp1252, which cannot represent the arrow in this prompt.
    with tempfile.TemporaryDirectory(prefix="graphhelm-unicode-oracle-") as raw_dir:
        directory = Path(raw_dir)
        helper = directory / "child.py"
        helper.write_text(
            "import json, sys\n"
            "text = sys.stdin.buffer.read().decode('utf-8')\n"
            "sys.stdout.buffer.write(json.dumps({'result': text}, ensure_ascii=False).encode('utf-8'))\n",
            encoding="utf-8",
        )
        command = directory / "child.cmd"
        command.write_text(
            f'@"{sys.executable}" "%~dp0child.py" %*\n',
            encoding="ascii",
        )

        original_encoding = subprocess.locale.getencoding
        subprocess.locale.getencoding = lambda: "cp1252"
        try:
            prompt = "Keel → GraphHelm: ação comprovada"
            result, stderr, _wall = runner.run_agent(
                directory,
                prompt,
                "a",
                None,
                1,
                {},
                None,
                None,
                {"path": str(command)},
            )
        except (UnicodeEncodeError, OSError) as exc:
            print(f"FAIL: prompt did not cross subprocess boundary: {exc}")
            return 1
        finally:
            subprocess.locale.getencoding = original_encoding

    if result.get("result") != prompt:
        print("FAIL: child received a different prompt")
        print(json.dumps({"result": result, "stderr": stderr}, ensure_ascii=False))
        return 1
    print("PASS: Unicode prompt crossed a real subprocess under simulated cp1252")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
