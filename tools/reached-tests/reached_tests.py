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
- emitted Rust test commands use `-- --test-threads=2`, and Studio related tests use
  `--maxWorkers=1`, so the selector does not amplify load on a shared lane;
Nothing else is reached by a docs-only diff. When `graphhelm` is on PATH, the Keel plan's class and
proof for the same paths are printed too. Standard library only.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

TOOLCHAIN = "+1.97.1"
CLI, CLI_BIN = "graphhelm-cli", "graphhelm"
TEST_THREADS = "-- --test-threads=2"
GUARD = f"cargo {TOOLCHAIN} test --locked -p graphhelm-protocols --test authored_strings_across_the_workspace {TEST_THREADS}"
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
                     "cargo +1.97.1 test --locked -p graphhelm-cli --test cli "
                     "-- --ignored journey_replay_browser:: journey_explore_browser:: journey_live_browser::")
BROWSER_ARGV = ["cargo", TOOLCHAIN, "test", "--locked", "-p", CLI, "--test", "cli", "--", "--ignored",
                "journey_replay_browser::", "journey_explore_browser::", "journey_live_browser::",
                "--test-threads=2"]
BROWSER_REACHERS = ("tools/journey-driver/", "apps/cli/src/commands/journey_replay.rs",
                    "apps/cli/src/commands/journey_explore.rs", "apps/cli/src/commands/journey_live.rs",
                    "apps/cli/tests/journey_replay_browser.rs", "apps/cli/tests/journey_explore_browser.rs",
                    "apps/cli/tests/journey_live_browser.rs")
INCLUDE = re.compile(r'include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)')
DISPATCH = {"src/main.rs", "src/commands/mod.rs"}
SOURCE_READ = re.compile(r'"src/|join\("src"\)')
# Audited exceptions for CLI integration tests that mention `src` while reading a fixture or a
# deliberately bounded production surface. `None` means the test reads the whole CLI source tree;
# an empty tuple means its `src` strings are fixture-only. Unknown readers remain conservative.
SOURCE_READER_SCOPES = {
    "source_invariants": None,
    "attention_inputs_one_feed": None,
    "surface_completeness": ("src/commands/serve/mod.rs", "src/commands/mcp/tools.rs"),
    "api_http": (),
    "runtime_http": (),
}
BUNDLE_MOD = re.compile(r'#\[path\s*=\s*"([^"]+)\.rs"\]\s*mod\s+(\w+)\s*;')
PRIVATE_TEST_PATH = (
    re.compile(r'#\[cfg\s*\(\s*test\s*\)\]\s*#\[path\s*=\s*"([^"]+\.rs)"\]\s*mod\s+\w+\s*;', re.S),
    re.compile(r'#\[path\s*=\s*"([^"]+\.rs)"\]\s*#\[cfg\s*\(\s*test\s*\)\]\s*mod\s+\w+\s*;', re.S),
)


def run(cmd, cwd):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=True).stdout


def workspace(repo):
    """Packages plus Cargo target metadata needed to expand whole-package plans safely."""
    meta = json.loads(run(["cargo", TOOLCHAIN, "metadata", "--format-version", "1", "--no-deps", "--offline"], repo))
    root = Path(meta["workspace_root"]).resolve()
    packages = []
    for p in meta["packages"]:
        pdir = Path(p["manifest_path"]).resolve().parent
        manifest_ok = True
        try:
            manifest = tomllib.loads(Path(p["manifest_path"]).read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError):
            manifest = {}
            manifest_ok = False
        manifest_targets = []
        for kind, key in (("lib", "lib"), ("bin", "bin"), ("test", "test"),
                          ("example", "example"), ("bench", "bench")):
            values = manifest.get(key, {}) if key == "lib" else manifest.get(key, [])
            if isinstance(values, dict):
                values = [values]
            if isinstance(values, list):
                manifest_targets.extend((kind, value) for value in values if isinstance(value, dict))

        def target_config(target):
            kind = next(iter(target["kind"]), "")
            if kind == "proc-macro":
                kind = "lib"
            src = Path(target["src_path"]).resolve()
            for config_kind, config in manifest_targets:
                if config_kind != kind:
                    continue
                if kind == "lib":
                    return config
                config_name = config.get("name")
                config_path = config.get("path")
                if config_name == target["name"]:
                    return config
                if config_path and (pdir / config_path).resolve() == src:
                    return config
            return {}

        targets = []
        for t in p["targets"]:
            config = target_config(t)
            targets.append({"name": t["name"], "kind": next(iter(t["kind"]), ""),
                            "src": Path(t["src_path"]).resolve(), "test": bool(t.get("test")),
                            "doctest": bool(t.get("doctest")),
                            "harness": config.get("harness", True), "harnessKnown": manifest_ok,
                            "requiredFeatures": list(t.get("required-features", []))})
        tests = [{"name": t["name"], "src": t["src"]} for t in targets if t["kind"] == "test"]
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
            "targets": [{**t, "src": t["src"].relative_to(root).as_posix()} for t in targets],
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
                if target == source.resolve():
                    continue
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


def python_test_tool(root, path):
    """Choose the module's declared Python test framework without running it."""
    text = (root / path).read_text(encoding="utf-8", errors="replace")
    if re.search(r"^(?:import pytest|from pytest)", text, re.M) or re.search(r"^def\s+test_\w+\s*\(", text, re.M):
        return f"python -m pytest {path}"
    return f"python -m unittest {path}"


def source_reader_targets(texts, packages, package, changed_source):
    """Map audited source readers; unknown readers stay broad until reviewed."""
    readers = set()
    for f, text in texts.items():
        if "CARGO_MANIFEST_DIR" not in text or not SOURCE_READ.search(text):
            continue
        scope = SOURCE_READER_SCOPES.get(f.stem, "unknown")
        if scope == () or (scope not in (None, "unknown") and changed_source not in scope):
            continue
        target = target_of(packages, package, f.stem)
        if target is not None:
            readers.add(target)
    return readers


def private_test_parent(sources, changed_path):
    """Return the unique production module that includes a cfg(test) path child."""
    parents = set()
    for source_path, text in sources.items():
        for pattern in PRIVATE_TEST_PATH:
            for child in pattern.findall(text):
                candidate = (Path(source_path).parent / child).as_posix()
                if candidate == changed_path:
                    parents.add(source_path[:-len(".rs")])
    return next(iter(parents)) if len(parents) == 1 else None


def private_test_children(sources, parent):
    """Return cfg(test) path children belonging to one production source."""
    children = set()
    parent_source = parent + ".rs"
    text = sources.get(parent_source, "")
    for pattern in PRIVATE_TEST_PATH:
        for child in pattern.findall(text):
            children.add((Path(parent_source).parent / child).as_posix())
    return children


def cli_module(packages, p, rel, root):
    """#361: a change to one `apps/cli/src/commands/<module>` reaches that module's unit tests, the
    integration tests that read the crate's own source, and the ones that name its command (every
    word of the module path as a quoted literal: `journey_explore` -> "journey" and "explore").
    Modules that import it directly (`<name>::` in their source; one level, not transitively) add their own unit
    and command tests (#530 review); `main.rs` and the `mod.rs` files only dispatch, so they are not
    importers. None (the whole package) for a change to a shared file (`main.rs`, any `mod.rs`,
    anything outside `commands/`), for a module that shared code outside `commands/` uses, for a
    changed module no test names, and when the tree cannot be read."""
    if p["name"] != CLI or root is None or not rel.startswith("src/commands/") or rel.endswith("/mod.rs"):
        return None
    crate = root / p["dir"]
    tests, src = crate / "tests", crate / "src"
    if not tests.is_dir():
        return None
    texts = {f: f.read_text(encoding="utf-8", errors="replace") for f in sorted(tests.glob("*.rs"))}
    sources = {f.relative_to(crate).as_posix(): f.read_text(encoding="utf-8", errors="replace")
               for f in sorted(src.rglob("*.rs"))} if src.is_dir() else {}
    start = rel[len("src/commands/"):-len(".rs")]
    parent = private_test_parent(sources, rel)
    audited_parent = "src/commands/workspace_slot" if start == "workspace_slot" else parent
    if parent is not None and parent.startswith("src/commands/"):
        start = parent[len("src/commands/"):]
    changed_source = f"src/commands/{start}.rs"
    readers = source_reader_targets(texts, packages, p["name"], changed_source)

    if audited_parent == "src/commands/workspace_slot":
        cli_bin = any(t.get("kind") == "bin" and t.get("name") == CLI_BIN for t in p.get("targets", []))
        workspace_target = target_of(packages, p["name"], "workspace_cli")
        allowed = {audited_parent + ".rs", *private_test_children(sources, audited_parent), "src/main.rs", "src/commands/mod.rs",
                   "src/commands/workspace.rs", "src/commands/serve/routes.rs"}
        unknown_caller = any(
            path not in allowed and re.search(r"\bworkspace_slot\b", text)
            for path, text in sources.items()
        )
        if not cli_bin or workspace_target is None or unknown_caller:
            return None
        return {(p["name"], f"bin:{CLI_BIN}", "commands::" + start.replace("/", "::")),
                workspace_target, *readers}

    def named(module):
        words = [w for part in module.split("/") for w in part.split("_") if w]
        return {t for f, text in texts.items() if words and all(f'"{w}"' in text for w in words)
                for t in [target_of(packages, p["name"], f.stem)] if t is not None}

    if not named(start):
        return None
    reached, queue, out = {start}, [start], set(readers)
    while queue:
        module = queue.pop()
        out |= {(p["name"], f"bin:{CLI_BIN}", "commands::" + module.replace("/", "::"))} | named(module)
        use = re.compile(r"\b" + re.escape(module.split("/")[-1]) + r"::")
        for path, text in sources.items():
            if path == f"src/commands/{module}.rs" or path in DISPATCH or path.endswith("/mod.rs") or not use.search(text):
                continue  # the dispatchers call every module to run it; they do not build on it
            if module != start:
                continue  # direct importers only: an importer's own importers are not followed
            if not path.startswith("src/commands/"):
                return None  # used from shared code (args, output, ...): anything behind it can be reached
            importer = path[len("src/commands/"):-len(".rs")]
            if importer not in reached:
                reached.add(importer)
                queue.append(importer)  # one level: past it, hub modules (journey.rs, serve) reach nearly every test
    return out


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
            tools |= {python_test_tool(root, f) for f in py} | ({f"node --test {' '.join(js)}"} if js else set())
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


def _target_argv(package, target):
    """One explicit Cargo invocation for a target that a package-wide test would cover."""
    kind, name = target["kind"], target["name"]
    base = ["cargo", TOOLCHAIN, "test", "--locked", "-p", package]
    if kind in ("lib", "proc-macro"):
        return base + ["--lib", *TEST_THREADS.split()]
    if kind == "bin":
        return base + ["--bin", name, *TEST_THREADS.split()]
    if kind == "test":
        return base + ["--test", name, *TEST_THREADS.split()]
    if kind == "example":
        return base + ["--example", name, "--no-run", *TEST_THREADS.split()]
    return None


def expand_whole_packages(whole, packages):
    """Expand package-wide Cargo test semantics into target steps and explicit unsupported items."""
    by_name = {p["name"]: p for p in packages}
    plans, unsupported = {}, []
    for name in sorted(whole):
        package = by_name.get(name)
        targets = package.get("targets") if package else None
        if not targets:
            unsupported.append({"package": name, "reason": "Cargo target metadata unavailable"})
            plans[name] = []
            continue
        package_steps = []
        doc_added = False
        for target in targets:
            required = target.get("requiredFeatures", [])
            if required:
                unsupported.append({"package": name, "target": target["name"],
                                    "kind": target["kind"],
                                    "reason": "target requires features: " + ", ".join(required)})
                continue
            if not target.get("harnessKnown", True):
                unsupported.append({"package": name, "target": target["name"],
                                    "kind": target["kind"],
                                    "reason": "Cargo manifest unavailable; harness setting unresolved"})
                continue
            if not target.get("harness", True):
                unsupported.append({"package": name, "target": target["name"],
                                    "kind": target["kind"],
                                    "reason": "custom harness target cannot inherit bounded test arguments"})
                continue
            if target["kind"] == "example" and target.get("test"):
                unsupported.append({"package": name, "target": target["name"],
                                    "kind": target["kind"],
                                    "reason": "test-enabled example execution is not represented by compile-only step"})
                continue
            if target["kind"] in ("lib", "proc-macro", "bin", "test") and not target.get("test") \
                    and not (target["kind"] in ("lib", "proc-macro") and target.get("doctest")):
                continue
            argv = _target_argv(name, target)
            if argv is None:
                if target["kind"] not in ("custom-build", "bench"):
                    unsupported.append({"package": name, "target": target["name"],
                                        "kind": target["kind"], "reason": "unsupported Cargo target kind"})
                continue
            if target["kind"] in ("lib", "proc-macro") and target.get("doctest") and target.get("test"):
                package_steps.append({"argv": argv, "cwd": ".", "slot": True})
            if target["kind"] in ("lib", "proc-macro") and target.get("doctest") and not doc_added:
                package_steps.append({"argv": ["cargo", TOOLCHAIN, "test", "--locked", "-p", name,
                                                "--doc", *TEST_THREADS.split()], "cwd": ".", "slot": True})
                doc_added = True
            elif not (target["kind"] in ("lib", "proc-macro") and target.get("doctest")):
                package_steps.append({"argv": argv, "cwd": ".", "slot": True})
        plans[name] = package_steps
    return plans, unsupported


def _argv_text(argv):
    return " ".join(argv)


def _group_single(single):
    """Group selected modules by package and test executable.

    A None module means the executable must run unfiltered, which dominates any
    narrower module selections for the same executable.
    """
    grouped = {}
    for package, target, module in single:
        key = (package, target)
        entry = grouped.setdefault(key, {"unfiltered": False, "modules": set()})
        if module is None:
            entry["unfiltered"] = True
        else:
            entry["modules"].add(module + "::")
    return [(package, target, None if entry["unfiltered"] else tuple(sorted(entry["modules"])))
            for (package, target), entry in sorted(grouped.items())]


def commands(whole, single, studio, tools=(), validate=(), lint_packages=None, package_plans=None):
    if whole and package_plans is not None:
        missing = sorted(set(whole) - set(package_plans))
        if missing:
            raise ValueError("whole-package target metadata missing for: " + ", ".join(missing))
    cmds = list(tools) + [f"graphhelm --json extension validate {v}" for v in validate]
    for name in sorted(whole):
        if package_plans is not None and name in package_plans:
            cmds.extend(_argv_text(step["argv"]) for step in package_plans[name])
        else:
            cmds.append(f"cargo {TOOLCHAIN} test --locked -p {name} {TEST_THREADS}")
    for package, target, modules in _group_single(single):
        kind = f"--bin {target[4:]}" if target.startswith("bin:") else f"--test {target}"
        filters = "" if modules is None else " " + " ".join(modules)
        cmds.append(f"cargo {TOOLCHAIN} test --locked -p {package} {kind} {TEST_THREADS}{filters}")
    rust = sorted(whole | {s[0] for s in single})
    lint = rust if lint_packages is None else sorted(lint_packages)
    if rust:
        cmds.append(f"cargo {TOOLCHAIN} fmt --all -- --check")
        if lint:
            cmds.append(f"cargo {TOOLCHAIN} clippy --locked " + " ".join(f"-p {n}" for n in lint) + " --all-targets --all-features -- -D warnings")
        cmds.append(GUARD)
    if studio:
        files = " ".join(p[len("apps/studio/"):] for p in studio)
        cmds.append(f"(cd apps/studio && npx vitest related --run --maxWorkers=1 {files} && npx tsc -b)")
    return cmds


def _node_test_paths(command, repo):
    """Recover paths from our generated node command, refusing ambiguous names."""
    tail = command[len("node --test "):]
    tokens = tail.split(" ")
    paths = []
    current = []
    for token in tokens:
        current.append(token)
        candidate = " ".join(current)
        if (repo / candidate).is_file():
            paths.append(candidate)
            current = []
    if current:
        raise ValueError(f"ambiguous node test path: {' '.join(current)}")
    return paths


def steps(whole, single, studio, tools=(), validate=(), lint_packages=None, repo=None, package_plans=None):
    """Return executable argv records without interpreting the legacy shell command strings."""
    if whole and package_plans is None:
        raise ValueError("whole-package steps require Cargo target metadata")
    if whole:
        missing = sorted(set(whole) - set(package_plans))
        if missing:
            raise ValueError("whole-package target metadata missing for: " + ", ".join(missing))
    out = []
    for tool in tools:
        if tool == BROWSER_OBSERVERS:
            out.append({"argv": BROWSER_ARGV[:], "cwd": ".", "slot": True, "observer": "browser"})
        elif tool == "graphhelm --json journey validate --all":
            out.append({"argv": ["graphhelm", "--json", "journey", "validate", "--all"], "cwd": ".", "slot": False})
        elif tool.startswith("python -m unittest "):
            test_path = tool[len("python -m unittest "):]
            out.append({"argv": ["python", "-m", "unittest", test_path], "cwd": ".", "slot": False})
        elif tool.startswith("python -m pytest "):
            test_path = tool[len("python -m pytest "):]
            out.append({"argv": ["python", "-m", "pytest", test_path], "cwd": ".", "slot": False})
        elif tool.startswith("node --test "):
            if repo is None:
                raise ValueError("node test paths require the repository root")
            test_paths = _node_test_paths(tool, Path(repo))
            out.append({"argv": ["node", "--test", *test_paths], "cwd": ".", "slot": False})
        else:
            raise ValueError(f"unrecognised reached-test tool: {tool}")
    for path in validate:
        out.append({"argv": ["graphhelm", "--json", "extension", "validate", path], "cwd": ".", "slot": False})
    for name in sorted(whole):
        if package_plans is not None and name in package_plans:
            out.extend(package_plans[name])
        else:
            out.append({"argv": ["cargo", TOOLCHAIN, "test", "--locked", "-p", name, *TEST_THREADS.split()],
                        "cwd": ".", "slot": True})
    for package, target, modules in _group_single(single):
        kind = ["--bin", target[4:]] if target.startswith("bin:") else ["--test", target]
        argv = ["cargo", TOOLCHAIN, "test", "--locked", "-p", package, *kind]
        argv.extend(TEST_THREADS.split())
        if modules is not None:
            argv.extend(modules)
        out.append({"argv": argv, "cwd": ".", "slot": True})
    rust = sorted(whole | {s[0] for s in single})
    lint = rust if lint_packages is None else sorted(lint_packages)
    if rust:
        out.append({"argv": ["cargo", TOOLCHAIN, "fmt", "--all", "--", "--check"], "cwd": ".", "slot": False})
        if lint:
            out.append({"argv": ["cargo", TOOLCHAIN, "clippy", "--locked", *sum((["-p", n] for n in lint), []),
                                   "--all-targets", "--all-features", "--", "-D", "warnings"],
                        "cwd": ".", "slot": True})
        out.append({"argv": ["cargo", TOOLCHAIN, "test", "--locked", "-p", "graphhelm-protocols",
                               "--test", "authored_strings_across_the_workspace", *TEST_THREADS.split()],
                    "cwd": ".", "slot": True})
    if studio:
        files = [p[len("apps/studio/"):] for p in studio]
        out.append({"argv": ["npx", "vitest", "related", "--run", "--maxWorkers=1", *files],
                    "cwd": "apps/studio", "slot": False})
        out.append({"argv": ["npx", "tsc", "-b"], "cwd": "apps/studio", "slot": False})
    return out


def keel_plan(repo, changed):
    configured = os.environ.get("GRAPHHELM_CLI")
    exe = str(Path(configured).resolve()) if configured else shutil.which("graphhelm")
    if configured and not Path(exe).is_file():
        raise ValueError("GRAPHHELM_CLI does not name an existing executable")
    if exe is None or not changed:
        return None
    try:
        reply = json.loads(run([exe, "--json", "keel", "plan", "--task", "reached-tests", "--repo", ".", "--paths", *changed], repo))
        plan = reply["data"]["plan"]
        return {"classes": plan["classes"], "proof": plan["proof"], "decidedBy": plan["decidedBy"]}
    except Exception:
        return None


def lint_scope(changed, packages, embedded):
    """Crates whose source or manifest was touched; do not include test-only dependents."""
    out = set()
    for path in changed:
        package = owner(packages, path)
        if package is not None:
            out.add(package["name"])
        else:
            out |= embedded.get(path, set())
    return out


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
    embedded = embeds(repo, packages)
    whole, single, studio, tools, validate, other = reach(changed, packages, embedded, repo)
    package_plans, unsupported = expand_whole_packages(whole, packages)
    lint = lint_scope(changed, packages, embedded)
    result = {"changed": changed, "packages": sorted(whole), "broadPackages": sorted(whole),
              "targets": [{"package": p, "test": t, "module": m} for p, t, m in sorted(single, key=lambda s: (s[0], s[1], s[2] or ""))],
              "studio": studio, "tools": tools, "validate": validate, "unmapped": other,
              "commands": commands(whole, single, studio, tools, validate, lint, package_plans),
              "steps": steps(whole, single, studio, tools, validate, lint, repo, package_plans),
              "lintPackages": sorted(lint),
              "unsupported": unsupported,
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
