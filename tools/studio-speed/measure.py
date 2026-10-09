"""Opt-in Studio latency observer; one Chromium, ten sequential fresh contexts.

Run from any directory. See docs/studio-speed.md for boundaries and limitations.
Only synthetic fixture data is sent; no tracing, screenshots or response bodies
are exported. Browser/fixture failures fail the run, never produce partial stats.
"""
import argparse
from datetime import datetime, timezone
import importlib.metadata
import json
import math
import os
from pathlib import Path
import platform
import socket
import statistics
import subprocess
import sys


def summarize(samples):
    if len(samples) != 10 or any(not math.isfinite(x) or x < 0 for x in samples):
        raise ValueError("Expected ten finite, nonnegative samples")
    ordered = sorted(samples)
    return {"count": 10, "medianMs": round(statistics.median(ordered), 2),
            "p95Ms": round(ordered[math.ceil(0.95 * len(ordered)) - 1], 2),
            "samplesMs": [round(x, 2) for x in samples]}


def painted(page, locator):
    # DOM correctness is independent of elapsed time. Two rAF callbacks allow
    # a paint opportunity between them; this is not a physical-display probe.
    locator.wait_for(state="visible")
    from playwright.sync_api import expect
    expect(locator).to_be_in_viewport()
    return page.evaluate("""() => new Promise(resolve =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve(performance.now()))))""")


def click_time(page, control, result):
    control.scroll_into_view_if_needed()
    # Start at the browser's input event, excluding Playwright auto-wait/IPC.
    control.evaluate("el => el.addEventListener('click', () => { window.__speedStart = performance.now(); }, {capture: true, once: true})")
    control.click()
    end = painted(page, result)
    return end - page.evaluate("window.__speedStart")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture-dir", required=True, type=Path, help="New disposable directory (must not exist)")
    parser.add_argument("--output", required=True, type=Path, help="New JSON report (must not exist)")
    parser.add_argument("--runtime-port", type=int, default=8897)
    parser.add_argument("--studio-port", type=int, default=5284)
    args = parser.parse_args()
    from playwright.sync_api import sync_playwright, expect

    repo = Path(__file__).resolve().parents[2]
    fixture_dir = args.fixture_dir.resolve()
    output = args.output.resolve()
    if fixture_dir.exists() or output.exists():
        parser.error("Use new fixture and output paths; existing data is never overwritten")
    if args.runtime_port == args.studio_port:
        parser.error("Runtime and Studio ports must differ")
    for port in (args.runtime_port, args.studio_port):
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", port))
    bash = "C:/Program Files/Git/bin/bash.exe" if os.name == "nt" else "bash"
    fixture = [bash, str(repo / "tools/studio-journey-fixture/fixture.sh")]
    samples = {name: [] for name in ("connectFirstUsablePaint", "tabTeam", "tabJourney", "tabGraph", "openJourney", "sendChat")}
    title = "Read the team chat and message everyone"
    stage = "fixture startup"
    try:
        subprocess.run(fixture + ["up", fixture_dir.as_posix(), str(args.runtime_port), str(args.studio_port)],
                       cwd=repo, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=180, check=True)
        token = (fixture_dir / ".graphhelm/events.token").read_text().splitlines()[0]
        with sync_playwright() as pw:
            browser = pw.chromium.launch()
            try:
                for run in range(10):
                    stage = f"run {run + 1}"
                    print(stage, file=sys.stderr, flush=True)
                    context = browser.new_context(viewport={"width": 1440, "height": 1000}, locale="en-US")
                    try:
                        # Opening a flow normally starts another browser. Model the
                        # documented older Runtime (404) for this route only.
                        context.route("**/v1/journey-flows/*/preview", lambda route: route.fulfill(
                            status=404, content_type="application/json", body='{"ok":false,"diagnostics":[]}'))
                        page = context.new_page()
                        page.set_default_timeout(15000)
                        page.goto(f"http://127.0.0.1:{args.studio_port}/")
                        page.get_by_label("Bearer token", exact=True).fill(token)
                        stage = f"run {run + 1}: connect"
                        samples["connectFirstUsablePaint"].append(click_time(page,
                            page.get_by_role("button", name="Connect", exact=True),
                            page.locator("#studio-panel-graph").get_by_role("tab", name="Team", exact=True)))
                        expect(page.get_by_label("Selected run identity")).to_contain_text("Run: demo")
                        stage = f"run {run + 1}: tabs"
                        samples["tabTeam"].append(click_time(page,
                            page.locator("#studio-panel-graph").get_by_role("tab", name="Team", exact=True),
                            page.locator("#studio-panel-team").get_by_text("planner", exact=True).first))
                        samples["tabJourney"].append(click_time(page,
                            page.get_by_role("tab", name="Journey", exact=True),
                            page.get_by_role("list", name="Journeys", exact=True).get_by_role("button").first))
                        stage = f"run {run + 1}: journey"
                        detail = page.get_by_role("article", name=f"Journey {title}", exact=True)
                        expect(detail).to_have_count(0)
                        samples["openJourney"].append(click_time(page,
                            page.get_by_role("list", name="Journeys", exact=True).get_by_role("button", name=title, exact=False),
                            detail.get_by_role("list", name="Steps", exact=True).get_by_role("button").first))
                        expect(detail.get_by_role("heading", name=title, exact=True)).to_be_visible()
                        expect(detail.get_by_role("list", name="Steps").locator(":scope > li")).to_have_count(2)
                        samples["tabGraph"].append(click_time(page,
                            page.get_by_role("tab", name="Graph", exact=True),
                            page.locator("#studio-panel-graph").get_by_text("No journeys in this project yet", exact=True)))
                        page.locator("#studio-panel-graph").get_by_role("tab", name="Team", exact=True).click()
                        page.get_by_role("tab", name="Everyone", exact=True).click()
                        message = f"Studio speed sample {run + 1}"
                        panel = page.get_by_role("tabpanel", name="Everyone", exact=True)
                        expect(panel.get_by_text(message, exact=True)).to_have_count(0)
                        page.get_by_role("textbox", name="Message", exact=True).fill(message)
                        stage = f"run {run + 1}: chat"
                        samples["sendChat"].append(click_time(page,
                            page.get_by_role("button", name="Send", exact=True),
                            panel.get_by_text(message, exact=True)))
                        expect(page.get_by_role("textbox", name="Message", exact=True)).to_have_value("")
                        # A fresh page must read the sent record back, not an optimistic echo.
                        page.reload()
                        page.get_by_label("Bearer token", exact=True).fill(token)
                        page.get_by_role("button", name="Connect", exact=True).click()
                        page.get_by_role("tab", name="Everyone", exact=True).click()
                        expect(panel.get_by_text(message, exact=True)).to_be_visible()
                    finally:
                        context.close()
                report = {
                    "schema": "graphhelm.studio-speed/1", "measuredAt": datetime.now(timezone.utc).isoformat(),
                    "sourceHead": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
                    "sourceDirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repo, text=True).strip()),
                    "platform": platform.system(), "browser": browser.version,
                    "playwright": importlib.metadata.version("playwright"),
                    "runtime": subprocess.check_output(["graphhelm", "--version"], text=True).strip(),
                    "runs": 10, "parallelism": 1, "viewport": {"width": 1440, "height": 1000},
                    "mode": "Vite dev; fresh context per run; shared warm server; no untimed warmup",
                    "fixture": "manual-override-deploy; one run; one question; adds one synthetic chat record per run",
                    "preview": "404 stub: journey details only, no replay or approval",
                    "clock": "browser click event to visible DOM plus two animation frames",
                    "p95Method": "nearest rank (maximum for ten samples)",
                    "correctness": "selected demo; planner visible; two journey steps; chat reread after reload",
                    "metrics": {key: summarize(values) for key, values in samples.items()},
                }
            finally:
                browser.close()
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report, indent=2))
    except Exception as error:
        # Playwright errors can include input values. Do not export their text.
        print(f"FAILED at {stage}: {type(error).__name__}; no report written", file=sys.stderr)
        return 1
    finally:
        subprocess.run(fixture + ["down", fixture_dir.as_posix()], cwd=repo,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30, check=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
