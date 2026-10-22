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
import ast
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

TOOLCHAIN = "+1.97.1"
CLI, CLI_BIN = "graphhelm-cli", "graphhelm"
TEST_THREADS = "-- --test-threads=2"
GUARD = f"cargo {TOOLCHAIN} test --locked -p graphhelm-protocols --test authored_strings_across_the_workspace {TEST_THREADS}"
# Files some tests read at run time (not embedded, so the include scan cannot see them): a changed
# path under the prefix reaches these test files, as (package, test file stem).
RUNTIME_READERS = {
    "extensions/": [("graphhelm-host-adoption", "release_packages"), ("graphhelm-cli", "development_plugin"),
                    ("graphhelm-cli", "jpd_plugin"), ("graphhelm-cli", "extension_cli"),
                    # The canonical golden pins the extension manifests and the digests they carry.
                    ("graphhelm-protocols", "canonical_golden")],
    # The committed journey flows: the scope guard reads them, and `journey validate --all` checks them.
    ".graphhelm/journeys/": [("graphhelm-cli", "journey_scope_guard")],
    # The MCP tool catalog: the stdio tests serve it, and the Studio's WebMCP adapter imports it.
    "schemas/mcp-tools.json": [("graphhelm-cli", "mcp_stdio")],
    # #1608: a schema change can move the canonical form the golden pins (#1403 turned main red
    # unseen); the golden is one cheap target, so every schema path reaches it.
    "schemas/": [("graphhelm-protocols", "canonical_golden")],
}
# #1608: the CLI help golden (`args_help_golden`, a bin unit-test module) pins `--help` for the whole
# command tree. Its fixture and its own source reach only it; a source file that derives clap
# argument types reaches it too (#1468 changed help text and turned main red unseen).
HELP_GOLDEN = (CLI, f"bin:{CLI_BIN}", "args_help_golden")
HELP_GOLDEN_PATHS = ("apps/cli/tests/fixtures/help/", "apps/cli/src/args_help_golden.rs")
CLAP_DERIVE = re.compile(r"derive\([^)]*\b(?:Parser|Args|Subcommand|ValueEnum)\b")
# #1608: the CLI words of the `commands` modules whose file name is not the command they run, as
# alternatives (any one word list, every word of it a quoted literal in the test). Everything else
# is derived from the module path. Deliberately absent, so they keep the whole package:
# - `atomic`, `hex_key`, `remediation`: helpers no command is named after; many modules build on
#   them, so no word list would bound what they reach.
# - `main.rs`, `args.rs`, `commands/mod.rs`, `output.rs`, `human.rs`, `palette.rs`, `progress.rs`,
#   `error_codes.rs`: shared by every command (parsing, dispatch, envelope, rendering, codes), so
#   they truly reach every test; `args.rs` also holds every command's help text.
CLI_COMMAND_WORDS = {
    "adoption": (("setup",), ("backup",), ("restore",)),
    "architect": (("synthesize",),),
    "development_index": (("code-index",),),
    "wake_wait": (("wake-wait",),),
    "workspace_merged": (("--merged-branch",),),
}
LAUNCHER_TESTS = f"cargo {TOOLCHAIN} test --locked -p graphhelm-cli --bin graphhelm {TEST_THREADS} commands::studio::tests::the_launcher_"
JOURNEY_REVIEW_CHECK = "python tools/reached-tests/claude_plugin_check.py plugins/journey-review"
# #1818: the Studio v2 app (#1790) beside the old one until cutover (C1 #1808) renames it.
STUDIO_NEXT = "apps/studio-next/"
STUDIO_NEXT_VITEST = ["npx", "vitest", "run", "--maxWorkers=4"]
MCP_CATALOG_STUDIO_TEST = "apps/studio/src/webmcp/mcp-tools.test.ts"
# #1521: starts the Studio dev server the way the journey launcher does (`fixture.sh up`).
STUDIO_START_SMOKE = "tools/studio-journey-fixture/test_studio_starts.py"
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


# Root files that no offline test reaches. Listing one here is the honest answer ("nothing observes
# this"), not a pass: the reason says why, so the entry is deleted the day a test covers the file.
NO_TEST_ROOT_FILES = {
    "Dockerfile": "the release image build needs Docker, which offline tests may not require",
    "docker-compose.yml": "compose needs Docker, which offline tests may not require",
    ".gitattributes": "line-ending rules; no test reads them",
    "LICENSE": "license text; no test reads it",
    "VERSION": "release number; the plugin version guard reads the plugin manifests, not this file",
    "rust-toolchain.toml": "toolchain pin; every cargo step runs under it, no test reads it by name",
    "rustfmt.toml": "format rules; the fmt step reads them, no test targets them",
}


def run(cmd, cwd):
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=True, creationflags=NO_WINDOW).stdout


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
        bundle_paths = {}
        for t in tests:
            text = t["src"].read_text(encoding="utf-8", errors="replace") if t["src"].is_file() else ""
            for path, module in BUNDLE_MOD.findall(text):
                bundles[module] = t["name"]
                source = (t["src"].parent / (path + ".rs")).resolve()
                if source.is_relative_to(root):
                    bundle_paths.setdefault(source.relative_to(root).as_posix(), set()).add((t["name"], module))
        packages.append({
            "name": p["name"],
            "dir": pdir.relative_to(root).as_posix(),
            # #1873: a dev-dependency links the crate into this package's tests only, never into its
            # library, so it reaches this package and stops; normal and build dependencies spread.
            "deps": sorted({d["name"] for d in p["dependencies"] if d.get("path") and d.get("kind") != "dev"}),
            "dev_deps": sorted({d["name"] for d in p["dependencies"] if d.get("path") and d.get("kind") == "dev"}),
            "tests": [{"name": t["name"], "src": t["src"].relative_to(root).as_posix()} for t in tests],
            "bundles": bundles,
            "bundle_paths": bundle_paths,
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


def rust_path_readers(repo, packages):
    """{repo path or basename: {(package name, Rust source path)}} for file references."""
    readers = {}
    root = Path(repo).resolve()
    for package in packages:
        package_root = root / package["dir"]
        for source in package_root.rglob("*.rs"):
            if "target" in source.relative_to(root).parts:
                continue
            text = source.read_text(encoding="utf-8", errors="replace")
            references = [(reference, source.parent) for reference in INCLUDE.findall(text)]
            references.extend(
                (match.group(2), source.parent)
                for match in re.finditer(r'''\b(?:read|read_to_string|File::open|open)\s*\(\s*(r#*)?["']([^"']+)["']''', text)
            )
            references.extend(
                (match.group(1), package_root if 'CARGO_MANIFEST_DIR' in match.group(0) else root)
                for match in re.finditer(
                    r'''\b(?:read|read_to_string|File::open|open)\s*\(\s*.*?\.join\s*\(\s*(?:r#*)?["']([^"']+)["']''',
                    text,
                )
            )
            for reference, base in references:
                if "{" in reference or reference.startswith("/"):
                    continue
                target = (base / reference).resolve()
                try:
                    path = target.relative_to(root).as_posix()
                except ValueError:
                    continue
                reader = (package["name"], source.relative_to(root).as_posix())
                readers.setdefault(path, set()).add(reader)
                readers.setdefault(Path(path).name, set()).add(reader)
    return readers


def rust_reader_target(packages, package_name, source_path):
    """Select the executable that owns a Rust file reference, if metadata identifies it."""
    package = next((p for p in packages if p["name"] == package_name), None)
    if package is None:
        return None
    rel = source_path[len(package["dir"]) + 1:]
    if rel.startswith("tests/"):
        registered = package.get("bundle_paths", {}).get(source_path, set())
        if registered:
            target, module = sorted(registered)[0]
            return (package_name, target, module)
        stem = Path(rel).stem
        return target_of(packages, package_name, stem)
    if rel.startswith("src/"):
        targets = package.get("targets", [])
        owning = [t for t in targets if t["kind"] in ("lib", "bin")]
        exact = [t for t in owning if t.get("src") == source_path]
        if len(exact) == 1:
            target = exact[0]
        elif len(owning) == 1:
            target = owning[0]
        else:
            return None
        return (package_name, "lib:" + target["name"] if target["kind"] == "lib"
                else "bin:" + target["name"], None)
    return None


def dependents(packages, names):
    """`names`, every package that depends on one of them through normal or build dependencies,
    transitively, and every package whose tests take one of those as a dev-dependency (#1873).

    A dev-dependency is linked into that package's tests only, never into its library, so a package
    reached through one has nothing a further dependent compiles against: it is not followed on."""
    reached, frontier = set(names), set(names)
    while frontier:
        frontier = {p["name"] for p in packages if p["name"] not in reached and set(p["deps"]) & frontier}
        reached |= frontier
    return reached | {p["name"] for p in packages if set(p.get("dev_deps", ())) & reached}


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
    if p["name"] != CLI or root is None or not rel.startswith("src/commands/") or rel == "src/commands/mod.rs":
        return None
    crate = root / p["dir"]
    tests, src = crate / "tests", crate / "src"
    if not tests.is_dir():
        return None
    texts = {f: f.read_text(encoding="utf-8", errors="replace") for f in sorted(tests.glob("*.rs"))}
    sources = {f.relative_to(crate).as_posix(): f.read_text(encoding="utf-8", errors="replace")
               for f in sorted(src.rglob("*.rs"))} if src.is_dir() else {}
    start = rel[len("src/commands/"):-len(".rs")]
    directory = rel.endswith("/mod.rs")
    if directory:
        start = start[:-len("/mod")]  # #1608: `serve/mod.rs` is the `serve` module
    parent = private_test_parent(sources, rel)
    audited_parent = "src/commands/workspace_slot" if start == "workspace_slot" else parent
    if parent is not None and parent.startswith("src/commands/"):
        start = parent[len("src/commands/"):]
    changed_source = rel if directory else f"src/commands/{start}.rs"
    readers = source_reader_targets(texts, packages, p["name"], changed_source)
    if CLAP_DERIVE.search(sources.get(rel, "")):
        readers.add(HELP_GOLDEN)

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

    def named_by(words):
        return {t for f, text in texts.items() if words and all(f'"{w}"' in text for w in words)
                for t in [target_of(packages, p["name"], f.stem)] if t is not None}

    def named(module):
        if module in CLI_COMMAND_WORDS:
            return set().union(*(named_by(list(words)) for words in CLI_COMMAND_WORDS[module]))
        found = named_by([w for part in module.split("/") for w in part.split("_") if w])
        if not found and "/" in module:
            # #1608: a submodule no test names by its own words is reached through its command
            # (`serve/ports` -> the tests that name "serve").
            found = named(module.split("/")[0])
        return found

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


def _crate_sources(root, package):
    src = root / package["dir"] / "src"
    return {f.relative_to(root / package["dir"]).as_posix(): f.read_text(encoding="utf-8", errors="replace")
            for f in sorted(src.rglob("*.rs"))} if src.is_dir() else {}


def _normal_closure(packages, start):
    """`start` and every package that reaches it through normal or build dependencies."""
    reached, frontier = {start}, {start}
    while frontier:
        frontier = {p["name"] for p in packages if p["name"] not in reached and set(p["deps"]) & frontier}
        reached |= frontier
    return reached


def core_module_cli(packages, p, rel, root):
    """#1875: the CLI part of the plan for a change to one module of a core crate `p`: the union of
    the narrow plans (`cli_module`) of the CLI files that name an affected path, plus the CLI test
    targets that name one. Derived from source text; None (the whole CLI package) whenever any step
    cannot be proved:
    - the change is to `lib.rs`, `main.rs`, a `mod.rs`, or not a module under `src/`;
    - `p`'s `lib.rs` mentions an affected module outside its `mod` lines (a re-export, a root
      function), an affected module exports a macro or has an `impl` on a type it does not declare,
      or anything imports `p` by glob or alias, or with `self` inside a `{…}` group;
    - a crate between `p` and the CLI (on a normal-dependency path) names an affected path;
    - a CLI file that names one has no narrow plan (shared code, a dispatcher, an unnamed module).
    Affected modules are the changed top-level module and, transitively, `p`'s own top-level modules
    whose source mentions one's name as a word."""
    if root is None or p["name"] == CLI or not rel.startswith("src/") or not rel.endswith(".rs"):
        return None
    if rel in ("src/lib.rs", "src/main.rs") or rel.endswith("/mod.rs"):
        return None
    cli = next((q for q in packages if q["name"] == CLI), None)
    if cli is None or CLI not in _normal_closure(packages, p["name"]):
        return None
    sources = _crate_sources(root, p)
    if not sources or "src/lib.rs" not in sources:
        return None

    def top(path):
        part = path[len("src/"):].split("/")[0]
        return part[:-len(".rs")] if part.endswith(".rs") else part

    # Affected modules: the changed one, then every top-level module of `p` whose source mentions
    # an affected module's name as a word at all. A word match over-approximates every way Rust
    # can name it (`crate::m`, `super::{m, …}`, `use crate::{x, m}`, `self::m`), so none is missed
    # (#1877 review, case A).
    affected, changed = {top(rel)}, True
    while changed:
        changed = False
        word = re.compile(r"\b(?:" + "|".join(re.escape(m) for m in sorted(affected)) + r")\b")
        for path, text in sources.items():
            module = top(path)
            if path != "src/lib.rs" and module not in affected and word.search(text):
                affected.add(module)
                changed = True
    word = re.compile(r"\b(?:" + "|".join(re.escape(m) for m in sorted(affected)) + r")\b")
    if any(top(path) in affected and "macro_export" in text for path, text in sources.items()):
        return None  # an exported macro is reached at the crate root, under no module path
    # Cases B and D: any other mention in `lib.rs` (a root function calling it, a re-export in any
    # form) puts affected code behind a root path, which no module path names.
    root_code = "\n".join(line for line in sources["src/lib.rs"].splitlines()
                          if not re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*;", line))
    if word.search(root_code):
        return None
    # Case C: an `impl` in an affected module changes a type declared elsewhere (an inherent
    # method, a trait impl, a blanket impl) for callers that never name the affected module. A
    # `type` alias declares no new type (`type L = crate::n::S; impl L` adds methods to `n::S`), so
    # it is not a declaration here (#1877 review, case Y).
    declared = {match for path, text in sources.items() if top(path) in affected
                for match in re.findall(r"\b(?:struct|enum|union|trait)\s+([A-Za-z_]\w*)", text)}
    for path, text in sources.items():
        if top(path) not in affected:
            continue
        for header in re.findall(r"\bimpl\b([^{;]*)\{", text):
            # #1877 review (cases V, W): narrow only for `impl [<params>] [Trait for] Name[<args>]`
            # where Name is one bare identifier the affected modules declare and not a generic
            # parameter of the impl; a path (`crate::n::T`), a `where` clause, a reference, a
            # trait object or anything else is the whole package.
            header = header.strip()
            params = set()
            if header.startswith("<"):
                depth, end = 0, None
                for i, ch in enumerate(header):
                    depth += {"<": 1, ">": -1}.get(ch, 0)
                    if depth == 0:
                        end = i
                        break
                if end is None:
                    return None
                params = set(re.findall(r"(?:^|[<,])\s*(?:const\s+)?([A-Za-z_]\w*)", header[:end + 1]))
                header = header[end + 1:].strip()
            if re.search(r"\bwhere\b", header):
                return None
            target = re.split(r"\s+for\s+", header)[-1].strip()
            shape = re.fullmatch(r"([A-Za-z_]\w*)\s*(?:<[^:{}]*>)?", target)
            if shape is None or shape.group(1) not in declared or shape.group(1) in params:
                return None
    ident = p["name"].replace("-", "_")
    path_use = re.compile(r"\b" + ident + r"::(?:" + "|".join(re.escape(m) for m in sorted(affected)) + r")\b")
    alias = re.compile(r"\b" + ident + r"::\*|\b" + ident + r"\s+as\b|\buse\s+" + ident + r"\s*;")

    def groups(text):
        """Each `<ident>::{…}` group, braces balanced."""
        for start in re.finditer(r"\b" + ident + r"::\{", text):
            depth, i = 0, start.end() - 1
            while i < len(text):
                depth += {"{": 1, "}": -1}.get(text[i], 0)
                if depth == 0:
                    yield text[start.end():i]
                    break
                i += 1
            else:
                yield text[start.end():]

    def named(text):
        return bool(path_use.search(text)) or any(word.search(g) for g in groups(text))

    def opaque(text):
        # Case G: `{self as k}`, `{self}` or a glob inside a group reaches modules unnamed.
        return bool(alias.search(text)) or any(re.search(r"\bself\b|\*", g) for g in groups(text))

    between = (_normal_closure(packages, p["name"]) - {p["name"], CLI}) & {
        q["name"] for q in packages if CLI in _normal_closure(packages, q["name"])}
    for name in sorted(between):
        package = next(q for q in packages if q["name"] == name)
        if any(named(t) or opaque(t) for t in _crate_sources(root, package).values()):
            return None
    out = set()
    for path, text in _crate_sources(root, cli).items():
        if opaque(text):
            return None
        if not named(text):
            continue
        narrow = cli_module(packages, cli, path, root)
        if narrow is None:
            return None
        out |= narrow
    tests = root / cli["dir"] / "tests"
    for f in sorted(tests.rglob("*.rs")) if tests.is_dir() else []:
        text = f.read_text(encoding="utf-8", errors="replace")
        if opaque(text):
            return None
        if not named(text):
            continue
        path = f.relative_to(root).as_posix()
        registered = cli.get("bundle_paths", {}).get(path, set())
        target = target_of(packages, CLI, f.stem) if f.parent == tests else None
        if registered:
            out |= {(CLI, t, m) for t, m in registered}
        elif target is not None:
            out.add(target)
        else:
            return None  # support code or an unregistered module: every test may build on it
    return out


def reach(changed, packages, embedded, repo=None):
    """The plan for one diff: whole packages, single test targets/modules, Studio files, tool
    tests, extension packages to validate, and what no rule maps."""
    whole, single, studio, tools, validate, other = set(), set(), [], set(), set(), []
    root = Path(repo).resolve() if repo else None
    path_readers = rust_path_readers(root, packages) if root is not None else {}
    for path in changed:
        readers = set()
        if root is not None and not path.endswith(".rs"):
            readers = path_readers.get(path, set()) | path_readers.get(Path(path).name, set())
            for package_name, source_path in readers:
                target = rust_reader_target(packages, package_name, source_path)
                if target is None:
                    whole.add(package_name)
                else:
                    single.add(target)
            if readers:
                # The path itself may be deleted, so derive package ownership from source refs.
                hit = True
            else:
                hit = False
        else:
            hit = False
        if path.startswith("apps/studio/tools/"):
            # #1571: the launcher scripts are run by the CLI's `the_launcher_*` tests, not by vitest.
            tools.add(LAUNCHER_TESTS)
            continue
        if path.startswith(STUDIO_NEXT):
            # #1818: Studio v2 has no related-test map yet; any change runs its whole suite.
            studio.append(path)
            continue
        if path.startswith("apps/studio/"):
            studio.append(path)
            # #1521: Vitest and tsc never start Vite; the journey launcher does.
            if root is not None and (root / STUDIO_START_SMOKE).is_file():
                tools.add(f"python -m unittest {STUDIO_START_SMOKE}")
            continue
        for prefix, runtime_readers in RUNTIME_READERS.items():
            if path.startswith(prefix):
                single |= {t for t in (target_of(packages, pkg, stem) for pkg, stem in runtime_readers) if t}
                parts = path.split("/")
                if prefix == "extensions/" and len(parts) > 2 and parts[1] in ("builtin",):
                    validate.add("/".join(parts[:3]))
                if prefix == ".graphhelm/journeys/":
                    tools.add("graphhelm --json journey validate --all")
                hit = True
        if path == "schemas/mcp-tools.json" and root is not None and (root / MCP_CATALOG_STUDIO_TEST).is_file():
            studio.append(MCP_CATALOG_STUDIO_TEST)
        if path.startswith(BROWSER_REACHERS):
            tools.add(BROWSER_OBSERVERS)
        handoff_scenario = ".graphhelm/fixtures/keel-task-handoff/scenario.py"
        handoff_readme = ".graphhelm/fixtures/keel-task-handoff/README.md"
        handoff_test = ".graphhelm/fixtures/keel-task-handoff/test_scenario.py"
        if path in {handoff_scenario, handoff_readme, handoff_test}:
            if root is not None and (root / handoff_scenario).is_file():
                wrong_store = " --wrong-store" if os.environ.get("GRAPHHELM_HANDOFF_WRONG_STORE") == "1" else ""
                tools.add(f"python {handoff_scenario}{wrong_store}")
                hit = True
            if root is not None and (root / handoff_test).is_file():
                tools.add(f"python {handoff_test}")
                hit = True
            elif root is not None and (root / handoff_scenario).is_file():
                other.append({"path": handoff_test, "reason": "handoff adapter-selection observer is missing"})
        if path.startswith(".graphhelm/fixtures/keel-pinned-verdict-merge/"):
            target = target_of(packages, "graphhelm-cli", "journey_backend_observer")
            if target:
                single.add(target)
            hit = True
        proportional_fixture = ".graphhelm/fixtures/keel-proportional-rows"
        proportional_script = f"{proportional_fixture}/run.py"
        proportional_test = f"{proportional_fixture}/test_run.py"
        if path in {proportional_script, proportional_test}:
            if root is not None and (root / proportional_test).is_file():
                tools.add(f"python {proportional_test}")
                hit = True
            elif root is not None:
                other.append({"path": proportional_test,
                              "reason": "proportional-rows fixture observer is missing"})
        # Fixture behavior is observed by previews of the changed flows. #1521: the one part a
        # plan can observe offline is that the Studio the launcher starts does start.
        if path.startswith("tools/studio-journey-fixture/"):
            if root is not None and (root / STUDIO_START_SMOKE).is_file():
                tools.add(f"python -m unittest {STUDIO_START_SMOKE}")
            hit = True
        # #1248: a Markdown change under docs/ or to an entry point can orphan a document or leave a
        # dead link; the reachability guard is the observer. Without this rule `.md` maps nothing.
        if path.endswith(".md") and (path.startswith("docs/") or path in ("README.md", "AGENTS.md")):
            observer = "tools/docs-guard/test_docs_reachable.py"
            if root is not None and (root / observer).is_file():
                tools.add(python_test_tool(root, observer))
                hit = True
        # The keel plugin's skills carry the KEEL files' text verbatim: an edit to either side runs
        # the guard that compares them. The same guard reads both marketplaces.
        if (path in ("KEEL.md", ".claude-plugin/marketplace.json", ".agents/plugins/marketplace.json")
                or path.startswith(("keel/", "plugins/keel/"))):
            observer = "tools/keel-plugin/test_keel_plugin.py"
            if root is not None and (root / observer).is_file():
                tools.add(python_test_tool(root, observer))
                hit = True
        # #1476: the planted defects anchor on exact text in the event store append; an edit there
        # runs the anchor check so a moved anchor is seen before a defect turns unobserved.
        if path == "core/events/src/local.rs":
            observer = "tools/keel-effect/test_plant_defects.py"
            if root is not None and (root / observer).is_file():
                tools.add(python_test_tool(root, observer))
        qualification_packet = "evals/graphhelm-development-contracts/judge-qualification.json"
        if path == qualification_packet:
            observer = "tools/evals/test_skill_twins.py"
            if root is not None and (root / observer).is_file():
                tools.add(python_test_tool(root, observer))
                hit = True
        if path.startswith("tools/") and path.count("/") >= 2 and root is not None:
            tool = root / "/".join(path.split("/")[:2])
            py = sorted(f.relative_to(root).as_posix() for f in tool.glob("test_*.py"))
            js = sorted(f.relative_to(root).as_posix() for f in tool.glob("*.test.mjs") if ".browser." not in f.name)
            tools |= {python_test_tool(root, f) for f in py} | ({f"node --test {' '.join(js)}"} if js else set())
            hit = hit or bool(py or js)
        if path in ("tools/no-window/observe.ps1", "tools/no-window/test_observe.ps1",
                    "tools/no-window/fixture/fixture.sh", "tools/no-window/fixture/server.mjs"):
            observer = "tools/no-window/test_observe.ps1"
            if root is not None and (root / observer).is_file():
                tools.add(f"powershell.exe -NoProfile -ExecutionPolicy Bypass -File {observer}")
                hit = True
            elif root is not None:
                other.append({"path": observer, "reason": "no-window observer test is missing"})
                hit = True
        if path.endswith(".py") and root is not None:
            observer = "tools/reached-tests/test_no_console_window.py"
            if (root / observer).is_file():
                tools.add(python_test_tool(root, observer))
                hit = True
        # The journey-review mod is not a skill plugin: `claude plugin validate` and `claude plugin test`
        # are its observers (run through a wrapper that reports a missing CLI as UNOBSERVED).
        if path.startswith("plugins/journey-review/") and root is not None:
            if (root / "tools/reached-tests/claude_plugin_check.py").is_file():
                tools.add(JOURNEY_REVIEW_CHECK)
            hooks_dir = "plugins/journey-review/hooks/"
            if path.startswith(hooks_dir) and path.endswith(".py") and "/" not in path[len(hooks_dir):]:
                for test in sorted((root / hooks_dir).glob("test_*.py")):
                    tools.add(python_test_tool(root, hooks_dir + test.name))
            hit = True
        plugin_parts = path.split("/")
        plugin_path = path.startswith("plugins/") and not path.startswith("plugins/journey-review/")
        hook_index = next((index for index, part in enumerate(plugin_parts)
                           if part == "hooks" and index >= 2), None) if plugin_path else None
        plugin_name = plugin_parts[1] if plugin_path and len(plugin_parts) > 1 else None
        hook_prefix = ("/".join(plugin_parts[:hook_index + 1]) + "/"
                       if hook_index is not None else None)
        companion_prefix = "plugins/graphhelm-codex-hooks/hooks/"
        if plugin_path and root is not None:
            manifest_path = (plugin_name is not None and
                             ((len(plugin_parts) >= 3 and plugin_parts[2] in {".mcp.json", "plugin.json"}) or
                             (len(plugin_parts) >= 4 and plugin_parts[2] in {".claude-plugin", ".codex-plugin"}
                               and plugin_parts[-1].endswith(".json"))))
            if manifest_path:
                # Plugin manifest and version contracts are exercised by the existing
                # plugin evaluators; validation runs only for builtin packages.
                test_paths = ["tools/evals/test_skill_twins.py"]
                test_paths.append("plugins/graphhelm/hooks/test_session_hook.py")
                for test_path in test_paths:
                    if (root / test_path).is_file():
                        tools.add(python_test_tool(root, test_path))
                if plugin_parts[1] == "builtin" and len(plugin_parts) >= 3:
                    validate.add("/".join(plugin_parts[:3]))
                hit = True
            elif any(part.lower() in {"skills", "agents"} for part in plugin_parts[2:]) or path.lower().endswith(("skill.md", "agent.md")):
                test_path = "tools/evals/test_skill_twins.py"
                if (root / test_path).is_file():
                    tools.add(python_test_tool(root, test_path))
                hit = True
            elif hook_prefix is None:
                # Plugin assets without a matching observer are docs-like: classify
                # them without stopping the selector as unmapped.
                hit = True

        if hook_prefix and root is not None:
            observed_hook_prefix = "plugins/graphhelm/hooks/" if path.startswith(companion_prefix) else hook_prefix
            hook_path = Path(observed_hook_prefix) / Path(path).name
            hook_dir = root / observed_hook_prefix
            changed_file = root / path
            if hook_path.name.startswith("test_") and hook_path.suffix == ".py" and not changed_file.is_file():
                # Git's name-only diff includes the missing side of a rename. Never
                # read or schedule that path; retain every surviving hook observer.
                survivors = sorted(f.relative_to(root).as_posix()
                                   for f in hook_dir.glob("test_*.py") if f.is_file())
                if survivors:
                    tools |= {python_test_tool(root, test) for test in survivors}
                else:
                    other.append({"path": path, "reason": "deleted hook test has no surviving observer"})
                hit = True
            if path.startswith(companion_prefix):
                # The companion ships the same hooks; observe behavior and byte parity.
                parity = root / "plugins/graphhelm/hooks/test_session_hook.py"
                if parity.is_file():
                    tools.add(python_test_tool(root, parity.relative_to(root).as_posix()))
                    hit = True
            if hook_path.name.startswith("test_") and hook_path.suffix == ".py" and changed_file.is_file():
                tools.add(python_test_tool(root, path))
                hit = True
            elif hook_path.name.startswith("test_") and changed_file.is_file():
                tools.add(python_test_tool(root, path))
                hit = True
            else:
                specific = hook_path.with_name("test_" + hook_path.name)
                tests = [specific.as_posix()] if (root / specific).is_file() else []
                # A changed helper reaches the tests of its actual local importers.
                # Parse imports instead of guessing from matching filenames.
                if changed_file.is_file() and hook_path.suffix == ".py":
                    module = hook_path.stem
                    source_dir = changed_file.parent
                    for source in sorted(source_dir.glob("*.py")):
                        if source == changed_file or source.name.startswith("test_"):
                            continue
                        try:
                            tree = ast.parse(source.read_text(encoding="utf-8", errors="replace"))
                        except SyntaxError:
                            # An unreadable edge makes the local graph uncertain; the
                            # existing all-hook observer fallback below is conservative.
                            tests = []
                            break
                        imports_module = any(
                            (isinstance(node, ast.ImportFrom) and node.module == module)
                            or (isinstance(node, ast.Import) and any(alias.name == module for alias in node.names))
                            for node in ast.walk(tree)
                        )
                        if imports_module:
                            consumer_test = source.with_name("test_" + source.name)
                            if (root / consumer_test).is_file():
                                tests.append(consumer_test.relative_to(root).as_posix())
                # Keep the parity test when its source actually reads this file.
                parity = root / "plugins/graphhelm/hooks/test_session_hook.py"
                if changed_file.is_file() and parity.is_file():
                    parity_text = parity.read_text(encoding="utf-8", errors="replace")
                    parity_names = set(re.findall(
                        r'\(SCRIPT\.parent\s*/\s*["\']([^"\']+)["\']\)\.read_bytes\(\)', parity_text
                    ))
                    script_name = re.search(
                        r'SCRIPT\s*=.*?with_name\(["\']([^"\']+)["\']\)', parity_text
                    )
                    if script_name and "SCRIPT.read_bytes()" in parity_text:
                        parity_names.add(script_name.group(1))
                    if hook_path.name in parity_names:
                        tests.append(parity.relative_to(root).as_posix())
                tests = sorted(set(tests))
                if not tests:
                    tests = sorted(f.relative_to(root).as_posix() for f in hook_dir.glob("test_*.py") if f.is_file())
                if not tests and changed_file.is_file():
                    other.append({"path": path, "reason": "hook change has no surviving observer"})
                    hit = True
                tools |= {python_test_tool(root, test) for test in tests}
                hit = hit or bool(tests)
        if path in embedded:
            if not readers:
                # #1608: a Rust source a test embeds (attention_tag_domain embeds
                # commands/execution/mod.rs) reaches that embedding target, not its whole package,
                # when every embedder resolves to a known target; the source's own rules follow below.
                embedders = path_readers.get(path, set()) if path.endswith(".rs") else set()
                targets = {rust_reader_target(packages, name, source) for name, source in embedders}
                if embedders and None not in targets and {name for name, _ in embedders} >= embedded[path]:
                    single |= targets
                else:
                    whole |= embedded[path]
            hit = True
        if path.startswith(HELP_GOLDEN_PATHS):
            single.add(HELP_GOLDEN)
            continue
        p = owner(packages, path)
        if p is not None:
            rel = path[len(p["dir"]) + 1:] if p["dir"] else path
            stem = Path(rel).stem
            test = next((t for t in p["tests"] if t["src"] == path), None)
            registered = p.get("bundle_paths", {}).get(path, set())
            if registered:
                single |= {(p["name"], target, module) for target, module in registered}
                hit = True
            elif rel.startswith("tests/") and rel.count("/") == 1 and rel.endswith(".rs"):
                if stem in p["bundles"] and not p.get("bundle_paths"):
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
                core = core_module_cli(packages, p, rel, root) if narrow is None else None
                if core is not None:
                    # #1875: the CLI runs only the tests of the modules that use the changed one.
                    single |= core
                    whole |= dependents(packages, {p["name"]}) - {CLI}
                elif narrow is None:
                    whole |= dependents(packages, {p["name"]})
                else:
                    single |= narrow
                    whole |= dependents(packages, {p["name"]}) - {p["name"]}
                hit = True
        if path in NO_TEST_ROOT_FILES:
            hit = True
        if not hit and not path.endswith(".md") and not path.startswith("docs/"):
            other.append(path)
    single = {s for s in single if s[0] not in whole}
    return whole, single, studio, sorted(tools), sorted(validate), other


def _plugin_release_content(path):
    """Shipped files need a release bump; dedicated docs and test files do not."""
    if not path.startswith(("plugins/graphhelm/", "plugins/graphhelm-codex-hooks/", "plugins/keel/",
                            "plugins/journey-review/")):
        return False
    parts = path.split("/")
    name = parts[-1].lower()
    return not (any(part.lower() in {"docs", "tests"} for part in parts[2:])
                or name.startswith("test_") or name.endswith("_test.py") or ".test." in name)


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


def _cli_bundle_steps(package, target):
    """Partition the CLI integration target without losing inline/root tests."""
    if package["name"] != CLI or target.get("kind") != "test" or target.get("name") != "cli":
        return None
    modules = sorted(module for module, target_name in package.get("bundles", {}).items()
                     if target_name == "cli")
    if not modules:
        return []
    base = ["cargo", TOOLCHAIN, "test", "--locked", "-p", package["name"], "--test", "cli",
            "--", "--test-threads=2"]
    steps = []
    for index, module in enumerate(modules):
        prior = sum((["--skip", previous + "::"] for previous in modules[:index]), [])
        steps.append({"argv": [*base, *prior, module + "::"], "cwd": ".", "slot": True})
    steps.append({"argv": [*base, *sum((["--skip", module + "::"] for module in modules), [])],
                  "cwd": ".", "slot": True})
    return steps


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
            if required:
                # #1280: run a gated target with its features rather than refusing the plan.
                # Cargo reads them before libtest's `--`.
                cut = argv.index("--") if "--" in argv else len(argv)
                argv = argv[:cut] + ["--features", ",".join(required)] + argv[cut:]
            if name == CLI and target.get("kind") == "test" and target.get("name") == "cli":
                bundle_steps = _cli_bundle_steps(package, target)
                if not bundle_steps:
                    unsupported.append({"package": name, "target": target["name"],
                                        "kind": target["kind"],
                                        "reason": "CLI test bundle metadata is missing or empty"})
                    continue
                package_steps.extend(bundle_steps)
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
    grouped_items = [(package, target, None if entry["unfiltered"] else tuple(sorted(entry["modules"])))
                     for (package, target), entry in sorted(grouped.items())]
    by_key = {(package, target): (package, target, modules)
              for package, target, modules in grouped_items}
    pair = ("bin:graphhelm", "cli")
    if all(("graphhelm-cli", target) in by_key for target in pair):
        left = by_key[("graphhelm-cli", pair[0])]
        right = by_key[("graphhelm-cli", pair[1])]
        if left[2] is not None and right[2] is not None:
            grouped_items = [item for item in grouped_items
                             if item[0:2] not in {("graphhelm-cli", pair[0]),
                                                  ("graphhelm-cli", pair[1])}]
            grouped_items.append(("graphhelm-cli", pair,
                                  tuple(sorted(set(left[2]) | set(right[2])))))
    return sorted(grouped_items, key=lambda item: (item[0], str(item[1])))


def _single_features(package_metadata, package, target):
    """#1280: `--features` for a single-target step whose test or bin target declares them."""
    if not isinstance(target, str) or target.startswith("lib:"):
        return []
    name, kind = (target[4:], "bin") if target.startswith("bin:") else (target, "test")
    meta = next((item for item in (package_metadata or []) if item["name"] == package), None)
    for candidate in (meta or {}).get("targets", []):
        if candidate.get("name") == name and candidate.get("kind") == kind:
            required = candidate.get("requiredFeatures", [])
            return ["--features", ",".join(required)] if required else []
    return []


def _target_flags(target):
    if isinstance(target, tuple):
        return ["--bin", target[0][4:], "--test", target[1]]
    if target.startswith("lib:"):
        return ["--lib"]
    return ["--bin", target[4:]] if target.startswith("bin:") else ["--test", target]


def commands(whole, single, studio, tools=(), validate=(), lint_packages=None, package_plans=None,
             package_metadata=None, plugin_version_refs=None):
    if whole and package_plans is not None:
        missing = sorted(set(whole) - set(package_plans))
        if missing:
            raise ValueError("whole-package target metadata missing for: " + ", ".join(missing))
    cmds = list(tools) + [f"graphhelm --json extension validate {v}" for v in validate]
    if plugin_version_refs is not None:
        cmds.insert(0, _argv_text(["python", "tools/reached-tests/plugin_version_guard.py",
                                   "--base", plugin_version_refs[0], "--head", plugin_version_refs[1]]))
    for name in sorted(whole):
        if package_plans is not None and name in package_plans:
            cmds.extend(_argv_text(step["argv"]) for step in package_plans[name])
        else:
            cmds.append(f"cargo {TOOLCHAIN} test --locked -p {name} {TEST_THREADS}")
    for package, target, modules in _group_single(single):
        if package == CLI and target == "cli" and modules is None:
            metadata = next((item for item in (package_metadata or []) if item["name"] == CLI), None)
            if metadata is None:
                raise ValueError("CLI bundle metadata is required for an unfiltered cli target")
            bundle_steps = _cli_bundle_steps(metadata, {"kind": "test", "name": "cli"})
            if not bundle_steps:
                raise ValueError("CLI bundle metadata is missing or empty")
            cmds.extend(_argv_text(step["argv"]) for step in bundle_steps)
            continue
        kind = " ".join(_target_flags(target) + _single_features(package_metadata, package, target))
        filters = "" if modules is None else " " + " ".join(modules)
        cmds.append(f"cargo {TOOLCHAIN} test --locked -p {package} {kind} {TEST_THREADS}{filters}")
    rust = sorted(whole | {s[0] for s in single})
    lint = rust if lint_packages is None else sorted(lint_packages)
    if rust:
        cmds.append(f"cargo {TOOLCHAIN} fmt --all -- --check")
        if lint:
            cmds.append(f"cargo {TOOLCHAIN} clippy --locked " + " ".join(f"-p {n}" for n in lint) + " --all-targets --all-features -- -D warnings")
        cmds.append(GUARD)
    old = [p for p in studio if not p.startswith(STUDIO_NEXT)]
    if old:
        if "apps/studio/vite.config.ts" in old:
            vitest = "npx vitest run --maxWorkers=1"
        else:
            files = " ".join(p[len("apps/studio/"):] for p in old)
            vitest = f"npx vitest related --run --maxWorkers=1 {files}"
        cmds.append(f"(cd apps/studio && {vitest} && npx tsc -b)")
    if len(old) < len(studio):
        cmds.append(f"(cd {STUDIO_NEXT.rstrip('/')} && {' '.join(STUDIO_NEXT_VITEST)} && npx tsc -b)")
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


def steps(whole, single, studio, tools=(), validate=(), lint_packages=None, repo=None, package_plans=None,
          package_metadata=None, plugin_version_refs=None):
    """Return executable argv records without interpreting the legacy shell command strings."""
    if whole and package_plans is None:
        raise ValueError("whole-package steps require Cargo target metadata")
    if whole:
        missing = sorted(set(whole) - set(package_plans))
        if missing:
            raise ValueError("whole-package target metadata missing for: " + ", ".join(missing))
    out = []
    if plugin_version_refs is not None:
        out.append({"argv": ["python", "tools/reached-tests/plugin_version_guard.py",
                              "--base", plugin_version_refs[0], "--head", plugin_version_refs[1]],
                    "cwd": ".", "slot": False})
    for tool in tools:
        if tool == BROWSER_OBSERVERS:
            out.append({"argv": BROWSER_ARGV[:], "cwd": ".", "slot": True, "observer": "browser"})
        elif tool == "graphhelm --json journey validate --all":
            out.append({"argv": ["graphhelm", "--json", "journey", "validate", "--all"], "cwd": ".", "slot": False})
        elif tool == LAUNCHER_TESTS:
            out.append({"argv": ["cargo", TOOLCHAIN, "test", "--locked", "-p", CLI, "--bin", "graphhelm",
                                  *TEST_THREADS.split(), "commands::studio::tests::the_launcher_"],
                        "cwd": ".", "slot": True})
        elif tool == LAUNCHER_TESTS:
            out.append({"argv": ["cargo", TOOLCHAIN, "test", "--locked", "-p", "graphhelm-cli", "--bin", "graphhelm",
                                  *TEST_THREADS.split(), "commands::studio::tests::the_launcher_"],
                        "cwd": ".", "slot": True})
        elif tool.startswith("python -m unittest "):
            test_path = tool[len("python -m unittest "):]
            out.append({"argv": ["python", "-m", "unittest", test_path], "cwd": ".", "slot": False})
        elif tool.startswith("python -m pytest "):
            test_path = tool[len("python -m pytest "):]
            out.append({"argv": ["python", "-m", "pytest", test_path], "cwd": ".", "slot": False})
        elif tool == "powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools/no-window/test_observe.ps1":
            observer = "tools/no-window/test_observe.ps1"
            if repo is None or not (Path(repo) / observer).is_file():
                raise ValueError(f"PowerShell observer script is missing: {observer}")
            out.append({"argv": ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
                                  "-File", observer], "cwd": ".", "slot": False})
        elif tool.startswith("python "):
            parts = tool[len("python "):].split(" ")
            script, args = parts[0], parts[1:]
            if repo is None or not (Path(repo) / script).is_file():
                raise ValueError(f"python observer script is missing: {script}")
            allowed = {
                ".graphhelm/fixtures/keel-task-handoff/scenario.py": {(), ("--wrong-store",)},
                ".graphhelm/fixtures/keel-task-handoff/test_scenario.py": {()},
                ".graphhelm/fixtures/keel-proportional-rows/test_run.py": {()},
                "tools/reached-tests/claude_plugin_check.py": {("plugins/journey-review",)},
            }
            if script not in allowed or tuple(args) not in allowed[script]:
                raise ValueError(f"unrecognised python observer arguments: {tool}")
            out.append({"argv": ["python", script, *args], "cwd": ".", "slot": False})
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
        if package == CLI and target == "cli" and modules is None:
            metadata = next((item for item in (package_metadata or []) if item["name"] == CLI), None)
            if metadata is None:
                raise ValueError("CLI bundle metadata is required for an unfiltered cli target")
            bundle_steps = _cli_bundle_steps(metadata, {"kind": "test", "name": "cli"})
            if not bundle_steps:
                raise ValueError("CLI bundle metadata is missing or empty")
            out.extend(bundle_steps)
            continue
        kind = _target_flags(target) + _single_features(package_metadata, package, target)
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
    old = [p for p in studio if not p.startswith(STUDIO_NEXT)]
    if old:
        if "apps/studio/vite.config.ts" in old:
            vitest = ["npx", "vitest", "run", "--maxWorkers=1"]
        else:
            files = [p[len("apps/studio/"):] for p in old]
            vitest = ["npx", "vitest", "related", "--run", "--maxWorkers=1", *files]
        out.append({"argv": vitest,
                    "cwd": "apps/studio", "slot": False})
        out.append({"argv": ["npx", "tsc", "-b"], "cwd": "apps/studio", "slot": False})
    if len(old) < len(studio):
        next_root = STUDIO_NEXT.rstrip("/")
        out.append({"argv": list(STUDIO_NEXT_VITEST), "cwd": next_root, "slot": False})
        out.append({"argv": ["npx", "tsc", "-b"], "cwd": next_root, "slot": False})
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
    changed = [line for line in run(["git", "diff", "--name-only", "--no-renames",
                                    f"{args.base}...{args.head}"], repo).splitlines() if line]
    plugin_version_refs = (args.base, args.head) if any(_plugin_release_content(path) for path in changed) else None
    packages = workspace(repo)
    embedded = embeds(repo, packages)
    whole, single, studio, tools, validate, other = reach(changed, packages, embedded, repo)
    package_plans, unsupported = expand_whole_packages(whole, packages)
    lint = lint_scope(changed, packages, embedded)
    unsupported_hook_observers = [item for item in other if isinstance(item, dict)]
    unmapped = [item for item in other if isinstance(item, str)]
    result = {"changed": changed, "packages": sorted(whole), "broadPackages": sorted(whole),
              "targets": [{"package": p, "test": t, "module": m} for p, t, m in sorted(single, key=lambda s: (s[0], s[1], s[2] or ""))],
              "studio": studio, "tools": tools, "validate": validate, "unmapped": unmapped,
              "commands": commands(whole, single, studio, tools, validate, lint, package_plans, packages, plugin_version_refs),
              "steps": steps(whole, single, studio, tools, validate, lint, repo, package_plans, packages, plugin_version_refs),
              "lintPackages": sorted(lint),
              "unsupported": unsupported + unsupported_hook_observers,
              "keelPlan": keel_plan(repo, changed)}
    if args.json:
        print(json.dumps(result, indent=1))
        return 0
    print(f"{len(changed)} changed paths; {len(whole)} whole packages, {len(single)} single targets, {len(studio)} Studio files")
    if result["keelPlan"]:
        print(f"keel plan: {result['keelPlan']}")
    if unmapped:
        print("not mapped to any test (check by hand): " + ", ".join(unmapped))
    for cmd in result["commands"]:
        print(cmd)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
