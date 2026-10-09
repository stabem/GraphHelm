#!/usr/bin/env python3
"""Which tests a diff reaches (#361): run those, not the whole battery.

    python tools/reached-tests/reached_tests.py                      # origin/main...HEAD
    python tools/reached-tests/reached_tests.py --base <rev> --head <rev> [--json]

From the changed paths and the workspace graph (`cargo metadata --no-deps`, no build):
- a changed integration-test file reaches only its own test target, or its module in a test bundle
  (`--test <bundle> <file>::`);
- a changed `tests/support/` or fixture file reaches its package's tests;
- a changed crate source reaches that crate's tests and every workspace crate that depends on it,
  directly or not;
- a changed file that some crate embeds (`include_str!` / `include_bytes!`, e.g. an extension's
  skill or schema) reaches the crates that embed it;
- a changed file some test reads at run time (`RUNTIME_READERS`: the extension packages) reaches
  those tests, and an extension package change adds `graphhelm extension validate <package>`; a
  committed journey flow reaches `journey_scope_guard` and `graphhelm journey validate --all`;
- a change to the journey driver, replay, explore or live play prints the `#[ignore]` browser
  observers' command (`BROWSER_OBSERVERS`) for a lane with a browser to run;
- a changed file under `tools/<tool>/` reaches that tool's `test_*.py` / `*.test.mjs` (not browser);
- changed Studio files reach `npx vitest related <files>` and `npx tsc -b`;
- a change to one `apps/cli/src/commands` module reaches its unit tests (`--bin graphhelm
  commands::<module>::`), the tests that read the crate's own source, and the integration tests
  that name its command; shared files and modules no test names reach the whole package;
- any reached Rust crate adds `cargo fmt --check`, clippy on the reached crates, and the
  workspace authored-strings guard (DELIVERY.md, "One review").
Nothing else is reached by a docs-only diff. When `graphhelm` is on PATH, the Keel plan's class and
proof for the same paths are printed too. Standard library only.
"""
import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

TOOLCHAIN = "+1.97.1"
GUARD = f"cargo {TOOLCHAIN} test --locked -p graphhelm-protocols --test authored_strings_across_the_workspace"
# Files some tests read at run time (not embedded, so the include scan cannot see them): a changed
# path under the prefix reaches these test files, as (package, test file stem).
RUNTIME_READERS = {
    "extensions/": [("graphhelm-host-adoption", "release_packages"), ("graphhelm-cli", "development_plugin"),
                    ("graphhelm-cli", "jpd_plugin"), ("graphhelm-cli", "extension_cli")],
    # The committed journey flows: the scope guard reads them, and `journey validate --all` checks them.
    ".graphhelm/journeys/": [("graphhelm-cli", "journey_scope_guard")],
}
# The `#[ignore]` browser observers (docs/guides/journeys.md): the real proof of a change to the
# journey driver, replay, explore or live play. Printed, never run for you: they need a browser.
BROWSER_OBSERVERS = ("GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT=<dir with node_modules/@playwright/test> "
                     "cargo +1.97.1 test --locked -p graphhelm-cli --test journey_replay_browser "
                     "--test journey_explore_browser --test journey_live_browser -- --ignored")
BROWSER_REACHERS = ("tools/journey-driver/", "apps/cli/src/commands/journey_replay.rs",
                    "apps/cli/src/commands/journey_explore.rs", "apps/cli/src/commands/journey_live.rs",
                    "apps/cli/tests/journey_replay_browser.rs", "apps/cli/tests/journey_explore_browser.rs",
                    "apps/cli/tests/journey_live_browser.rs")
INCLUDE = re.compile(r'include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)')
CLI, CLI_BIN = "graphhelm-cli", "graphhelm"
SOURCE_READ = re.compile(r'"src/|join\("src"\)')
BUNDLE_MOD = re.compile(r'#\[path\s*=\s*"([^"]+)\.rs"\]\s*mod\s+(\w+)\s*;')


def run(cmd, cwd):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=True).stdout


def workspace(repo):
    """Packages as {name, dir, deps, tests: [{name, src}], bundles: {file stem: bundle}}."""
    meta = json.loads(run(["cargo", TOOLCHAIN, "metadata", "--format-version", "1", "--no-deps", "--offline"], repo))
    root = Path(meta["workspace_root"]).resolve()
    packages = []
    for p in meta["packages"]:
        pdir = Path(p["manifest_path"]).resolve().parent
        tests = [{"name": t["name"], "src": Path(t["src_path"]).resolve()} for t in p["targets"] if "test" in t["kind"]]
        bundles = {}
        for t in tests:
            text = t["src"].read_text(encoding="utf-8", errors="replace") if t["src"].is_file() else ""
            for path, module in BUNDLE_MOD.findall(text):
                bundles[module] = t["name"]
        packages.append({
            "name": p["name"],
            "dir": pdir.relative_to(root).as_posix(),
            "deps": sorted(d["name"] for d in p["dependencies"] if d.get("path")),
            "tests": [{"name": t["name"], "src": t["src"].relative_to(root).as_posix()} for t in tests],
            "bundles": bundles,
        })
    return packages


def embeds(repo, packages):
    """{embedded repo path: {package names}} from every include_str!/include_bytes! in the workspace."""
    out = {}
    root = Path(repo).resolve()
    for p in packages:
        for source in (root / p["dir"]).rglob("*.rs"):
            if "target" in source.relative_to(root).parts:
                continue
            for rel in INCLUDE.findall(source.read_text(encoding="utf-8", errors="replace")):
                if "{" in rel or rel.startswith("/"):
                    continue
                target = (source.parent / rel).resolve()
                try:
                    out.setdefault(target.relative_to(root).as_posix(), set()).add(p["name"])
                except ValueError:
                    pass
    return out


def dependents(packages, names):
    """`names` and every package that depends on one of them, transitively."""
    reached, frontier = set(names), set(names)
    while frontier:
        frontier = {p["name"] for p in packages if p["name"] not in reached and set(p["deps"]) & frontier}
        reached |= frontier
    return reached


def owner(packages, path):
    """The package whose directory holds `path` (the deepest one)."""
    best = None
    for p in packages:
        if path == p["dir"] or path.startswith(p["dir"] + "/"):
            if best is None or len(p["dir"]) > len(best["dir"]):
                best = p
    return best


def target_of(packages, package, stem):
    """(package, test target, bundle module or None) for one integration-test file stem."""
    p = next((q for q in packages if q["name"] == package), None)
    if p is None:
        return None
    if stem in p["bundles"]:
        return (package, p["bundles"][stem], stem)
    if any(t["name"] == stem for t in p["tests"]):
        return (package, stem, None)
    return None


def cli_module(packages, p, rel, root):
    """#361: a change to one `apps/cli/src/commands/<module>` reaches that module's unit tests, the
    integration tests that read the crate's own source, and the ones that name its command (every
    word of the module path as a quoted literal: `journey_explore` -> "journey" and "explore").
    None (the whole package) for shared files (`main.rs`, any `mod.rs`, anything outside
    `commands/`), for a module no test names, and when the tree cannot be read."""
    if p["name"] != CLI or root is None or not rel.startswith("src/commands/") or rel.endswith("/mod.rs"):
        return None
    module = rel[len("src/commands/"):-len(".rs")]
    words = [w for part in module.split("/") for w in part.split("_") if w]
    tests = root / p["dir"] / "tests"
    if not words or not tests.is_dir():
        return None
    named, readers = set(), set()
    for f in sorted(tests.glob("*.rs")):
        text = f.read_text(encoding="utf-8", errors="replace")
        target = target_of(packages, p["name"], f.stem)
        if target is None:
            continue
        if "CARGO_MANIFEST_DIR" in text and SOURCE_READ.search(text):
            readers.add(target)
        if all(f'"{w}"' in text for w in words):
            named.add(target)
    if not named:
        return None
    return {(p["name"], f"bin:{CLI_BIN}", "commands::" + module.replace("/", "::"))} | named | readers


def reach(changed, packages, embedded, repo=None):
    """The plan for one diff: whole packages, single test targets/modules, Studio files, tool
    tests, extension packages to validate, and what no rule maps."""
    whole, single, studio, tools, validate, other = set(), set(), [], set(), set(), []
    root = Path(repo).resolve() if repo else None
    for path in changed:
        if path.startswith("apps/studio/"):
            studio.append(path)
            continue
        hit = False
        for prefix, readers in RUNTIME_READERS.items():
            if path.startswith(prefix):
                single |= {t for t in (target_of(packages, pkg, stem) for pkg, stem in readers) if t}
                parts = path.split("/")
                if prefix == "extensions/" and len(parts) > 2 and parts[1] in ("builtin",):
                    validate.add("/".join(parts[:3]))
                if prefix == ".graphhelm/journeys/":
                    tools.add("graphhelm --json journey validate --all")
                hit = True
        if path.startswith(BROWSER_REACHERS):
            tools.add(BROWSER_OBSERVERS)
        if path.startswith("tools/") and path.count("/") >= 2 and root is not None:
            tool = root / "/".join(path.split("/")[:2])
            py = sorted(f.relative_to(root).as_posix() for f in tool.glob("test_*.py"))
            js = sorted(f.relative_to(root).as_posix() for f in tool.glob("*.test.mjs") if ".browser." not in f.name)
            tools |= {f"python -m unittest {f}" for f in py} | ({f"node --test {' '.join(js)}"} if js else set())
            hit = hit or bool(py or js)
        if path in embedded:
            whole |= embedded[path]
            hit = True
        p = owner(packages, path)
        if p is not None:
            rel = path[len(p["dir"]) + 1:] if p["dir"] else path
            stem = Path(rel).stem
            test = next((t for t in p["tests"] if t["src"] == path), None)
            if rel.startswith("tests/") and rel.count("/") == 1 and rel.endswith(".rs"):
                if stem in p["bundles"]:
                    single.add((p["name"], p["bundles"][stem], stem))
                elif test is not None:
                    single.add((p["name"], test["name"], None))
                else:
                    whole.add(p["name"])
                hit = True
            elif rel.startswith("tests/"):
                whole.add(p["name"])  # support modules and fixtures serve the package's tests
                hit = True
            elif rel.startswith(("src/", "build.rs", "Cargo.toml")) or rel.endswith(".rs"):
                narrow = cli_module(packages, p, rel, root)
                if narrow is None:
                    whole |= dependents(packages, {p["name"]})
                else:
                    single |= narrow
                    whole |= dependents(packages, {p["name"]}) - {p["name"]}
                hit = True
        if not hit and not path.endswith(".md") and not path.startswith("docs/"):
            other.append(path)
    single = {s for s in single if s[0] not in whole}
    return whole, single, studio, sorted(tools), sorted(validate), other


def commands(whole, single, studio, tools=(), validate=()):
    cmds = list(tools) + [f"graphhelm --json extension validate {v}" for v in validate]
    for name in sorted(whole):
        cmds.append(f"cargo {TOOLCHAIN} test --locked -p {name}")
    for package, target, module in sorted(single, key=lambda s: (s[0], s[1], s[2] or "")):
        kind = f"--bin {target[4:]}" if target.startswith("bin:") else f"--test {target}"
        cmds.append(f"cargo {TOOLCHAIN} test --locked -p {package} {kind}" + (f" {module}::" if module else ""))
    rust = sorted(whole | {s[0] for s in single})
    if rust:
        cmds.append(f"cargo {TOOLCHAIN} fmt --all -- --check")
        cmds.append(f"cargo {TOOLCHAIN} clippy --locked " + " ".join(f"-p {n}" for n in rust) + " --all-targets --all-features -- -D warnings")
        cmds.append(GUARD)
    if studio:
        files = " ".join(p[len("apps/studio/"):] for p in studio)
        cmds.append(f"(cd apps/studio && npx vitest related --run {files} && npx tsc -b)")
    return cmds


def keel_plan(repo, changed):
    exe = shutil.which("graphhelm")
    if exe is None or not changed:
        return None
    try:
        reply = json.loads(run([exe, "--json", "keel", "plan", "--task", "reached-tests", "--repo", ".", "--paths", *changed], repo))
        plan = reply["data"]["plan"]
        return {"classes": plan["classes"], "proof": plan["proof"], "decidedBy": plan["decidedBy"]}
    except Exception:
        return None


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--repo", default=".")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args(argv)
    repo = Path(args.repo).resolve()
    changed = [line for line in run(["git", "diff", "--name-only", f"{args.base}...{args.head}"], repo).splitlines() if line]
    packages = workspace(repo)
    whole, single, studio, tools, validate, other = reach(changed, packages, embeds(repo, packages), repo)
    result = {"changed": changed, "packages": sorted(whole),
              "targets": [{"package": p, "test": t, "module": m} for p, t, m in sorted(single, key=lambda s: (s[0], s[1], s[2] or ""))],
              "studio": studio, "tools": tools, "validate": validate, "unmapped": other,
              "commands": commands(whole, single, studio, tools, validate),
              "keelPlan": keel_plan(repo, changed)}
    if args.json:
        print(json.dumps(result, indent=1))
        return 0
    print(f"{len(changed)} changed paths; {len(whole)} whole packages, {len(single)} single targets, {len(studio)} Studio files")
    if result["keelPlan"]:
        print(f"keel plan: {result['keelPlan']}")
    if other:
        print("not mapped to any test (check by hand): " + ", ".join(other))
    for cmd in result["commands"]:
        print(cmd)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
