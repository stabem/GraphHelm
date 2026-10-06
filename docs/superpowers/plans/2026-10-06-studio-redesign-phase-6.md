# Studio Redesign Phase 6 (Producers and Guidance) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the spec §6.3 producers and guidance: a Keel card may name its `journeys`, `keel check` warns (never blocks) when the card touches a screen with no fresh capture at the head, the Playwright observer records captures and walked transitions with `--journey`, and DELIVERY.md asks for before/after captures.

**Architecture:** The card field and the pure finding live in `core/policy/src/keel.rs` (no IO). The CLI `keel check` does the IO: it reads each named contract from `<repo>/.graphhelm/journeys/<id>.json`, optionally reads the run's captures with the keyring (`commands::journeys::records`), folds them with `graphhelm_execution::fold_journeys` pinned to the range's head, and hands the per-screen state to the pure function. The observer stays a stdlib-only Python script that shells out to `graphhelm journey capture|walked`.

**Tech Stack:** Rust 1.97.1 (core/policy, apps/cli), JSON Schema 2020-12, Python 3 stdlib + unittest.

**Spec:** `docs/specs/2026-10-05-studio-live-team-and-proven-journeys-design.md` §6.1–6.4. Issue: #321.

## Global Constraints

- Branch `issue-321-journey-producers`; every commit message ends with a blank line then `Refs #321` (last task: `Closes #321`) and `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Cargo target dir: `F:/ghp6t` (`CARGO_TARGET_DIR=F:/ghp6t`). Toolchain `cargo +1.97.1`. Never write to `F:\github\ml-saas\.graphhelm`.
- Documentation in English only.
- Advisory means `blocking: false`: a journey finding never changes the exit code (0 stays 0) and never sets `refused`.
- Old cards stay valid: `journeys` is optional; `Card` keeps `deny_unknown_fields`.
- Journey ids follow Ruling 1 of phase 4: `^[a-z0-9][a-z0-9._-]{0,127}$`, no `..` (`graphhelm_execution::valid_journey_id`).
- A new test must fail on the parent (without the change). State each new test's cost in the report.
- After each green task: `git commit` then `git push`.
- Lints for touched crates: `cargo +1.97.1 fmt -p <crate> -- --check` (the `--all` form hits Windows os error 206 here), `cargo +1.97.1 clippy --locked -p <crate> --all-targets --all-features -- -D warnings`, and the source guard `cargo +1.97.1 test --locked -p graphhelm-protocols --test authored_strings_across_the_workspace`.
- Tests may not need internet, Docker, credentials or a real browser.

## Review Focus

1. A card with `journeys` but no `--events`: the user must still get a warning per touched screen, worded so they know captures were not read (not "no capture exists").
2. A capture taken from uncommitted code (`dirty: true`) is never fresh, even if the code did not move.
3. A card whose `journeys` names a contract file that is missing or invalid: a warning naming the contract, never exit 3 and never a panic.
4. Observer `--journey` when Playwright failed a step test: that step is not captured and no walked transition touches it.
5. Observer `--journey` when `graphhelm` is not on PATH or a capture call fails: the verdict of the test run stays what Playwright said; the journey part reports what was not recorded.

---

### Task 1: `journeys` on the Keel card

**Files:**
- Modify: `extensions/builtin/graphhelm-development-contracts/schemas/keel-card.schema.json`
- Modify: `extensions/builtin/graphhelm-development-contracts/extension.json` (recompute the `schema/keel-card` sha256; add the new fixture contribution if fixtures are registered there, mirroring `fixture/keel-card-valid-full`)
- Create: `extensions/builtin/graphhelm-development-contracts/fixtures/keel-card/valid/card-journeys.json`
- Modify: `core/policy/src/keel.rs` (`Card`)
- Modify: `core/policy/tests/keel.rs` (`card()` helper gains `journeys: Vec::new()`; new test)
- Modify: `apps/cli/src/commands/keel.rs` (`card_from_markdown` reads `Journeys:`)
- Modify: `apps/cli/tests/keel_check.rs` (new tests)
- Any other construction site of `graphhelm_policy::keel::Card { .. }` that stops compiling (search `Card {`).

**Interfaces:**
- Produces: `pub journeys: Vec<String>` on `graphhelm_policy::keel::Card`, `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.

- [ ] **Step 1: Failing tests.**

In `core/policy/tests/keel.rs` add:

```rust
#[test]
fn a_card_may_name_its_journeys_and_an_old_card_still_reads() {
    let old = std::fs::read(package_root().join("fixtures/keel-card/valid/card-full.json")).unwrap();
    let old: graphhelm_policy::keel::Card = serde_json::from_slice(&old).unwrap();
    assert!(old.journeys.is_empty());
    let new = std::fs::read(package_root().join("fixtures/keel-card/valid/card-journeys.json")).unwrap();
    let new: graphhelm_policy::keel::Card = serde_json::from_slice(&new).unwrap();
    assert_eq!(new.journeys, ["checkout"]);
}
```

Fixture `card-journeys.json`:

```json
{
  "promise": "the cart shows the line total",
  "scopePaths": ["web/src/routes/cart/"],
  "proof": "npx playwright test cart",
  "journeys": ["checkout"]
}
```

In `apps/cli/tests/keel_check.rs` add (reusing `repository`, `run`, `codes`):

```rust
#[test]
fn a_card_naming_journeys_validates_and_a_bad_journey_id_is_refused_at_its_path() {
    let repo = repository(&[]);
    let path = repo.path().join("card.json");
    let mut card = serde_json::json!({
        "promise": "added() exists", "scopePaths": ["src"], "proof": "cargo test",
        "exportedSymbols": ["added"], "journeys": ["checkout"],
    });
    fs::write(&path, serde_json::to_vec(&card).unwrap()).unwrap();
    let (code, reply) = run(repo.path(), Some(&path));
    assert_eq!(code, 0, "{reply}");
    card["journeys"] = serde_json::json!(["../etc"]);
    fs::write(&path, serde_json::to_vec(&card).unwrap()).unwrap();
    let (code, reply) = run(repo.path(), Some(&path));
    assert_eq!(code, 3, "{reply}");
    assert_eq!(reply["diagnostics"][0]["path"], "/journeys/0", "{reply}");
}

#[test]
fn a_card_written_in_a_pr_body_reads_its_journeys_line() {
    let repo = repository(&[]);
    let body = repo.path().join("body.md");
    fs::write(&body, "Promise: added() exists\nScope: `src`\nProof: `cargo test`\nExported: `added`\nJourneys: `checkout`\n").unwrap();
    let (code, reply) = run(repo.path(), Some(&body));
    assert_eq!(code, 0, "{reply}");
    assert!(codes(&reply).iter().any(|c| c == "keel.journey.no_fresh_capture" || c == "keel.journey.contract_unreadable"), "{reply}");
}
```

The second test only goes green after Task 2 (it proves the markdown field reaches the check). Mark it `#[ignore = "green after Task 2"]` in this task and remove the attribute in Task 2.

Note: the `/journeys/0` pointer is what `graphhelm_schema::validate_inline_value` reports for an item that fails `pattern`; if the shipped validator reports a different pointer shape for array items, assert the one it really produces and say so in the report.

- [ ] **Step 2: Run, expect FAIL** (`no field journeys`, fixture missing, schema `additionalProperties: false` refuses `journeys`).

```
CARGO_TARGET_DIR=F:/ghp6t cargo +1.97.1 test --locked -p graphhelm-policy --test keel a_card_may_name
CARGO_TARGET_DIR=F:/ghp6t cargo +1.97.1 test --locked -p graphhelm-cli --test keel_check a_card_naming_journeys
```

- [ ] **Step 3: Implement.**

Schema, add under `properties`:

```json
"journeys": {
  "type": "array",
  "uniqueItems": true,
  "items": {
    "type": "string",
    "pattern": "^[a-z0-9][a-z0-9._-]{0,127}$",
    "not": { "pattern": "\\.\\." }
  },
  "description": "Journey contracts (`.graphhelm/journeys/<contractId>.json`) that prove the promise. keel check reports, without blocking, a screen the card's scope touches whose journey has no fresh capture at the head (keel.journey.no_fresh_capture)."
}
```

`Card`:

```rust
    /// Journey contract ids whose screens prove the promise (spec §6.3). Advisory only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub journeys: Vec<String>,
```

`card_from_markdown`: add the arm `"journeys" => ("journeys", list()),`.

Recompute the schema sha256 in `extension.json` from the file's bytes as git stores them (LF): `git hash-object` is NOT sha256; use `python -c "import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],'rb').read().replace(b'\r\n',b'\n')).hexdigest())" <file>` and check how the existing pin was computed by recomputing it for the unchanged `card-full.json` first (it must equal the pinned value; if it only matches without the CRLF replacement, use that form).

- [ ] **Step 4: Run, expect PASS**, plus every test that reads the package or its digests:

```
CARGO_TARGET_DIR=F:/ghp6t cargo +1.97.1 test --locked -p graphhelm-policy
CARGO_TARGET_DIR=F:/ghp6t cargo +1.97.1 test --locked -p graphhelm-cli --test keel_check --test development_plugin --test development_package_inventory --test development_contract_schemas --test jpd_plugin --test extension_cli
```

Then fmt/clippy for `graphhelm-policy` and `graphhelm-cli`, and the source guard.

- [ ] **Step 5: Commit and push** — `feat(keel): optional journeys on the Keel card (#321)`.

---

### Task 2: advisory `keel check` finding for screens without a fresh capture

**Files:**
- Modify: `core/policy/src/keel.rs` (new `JourneyScreen`, `check_journeys`)
- Modify: `core/policy/tests/keel.rs` (unit test)
- Modify: `apps/cli/src/args.rs` (`KeelCommand::Check` gains four optional args)
- Modify: `apps/cli/src/commands/mod.rs` (pass them)
- Modify: `apps/cli/src/commands/keel.rs` (IO + wiring)
- Modify: `apps/cli/src/commands/journeys.rs` only if `records`/`contract` need `pub(crate)` (they already are)
- Modify: `apps/cli/tests/journey_producers_cli.rs` (integration test with a temp git repo, using its `Harness`)
- Modify: `apps/cli/tests/keel_check.rs` (remove the Task 1 `#[ignore]`)
- Modify: `docs/keel/KEEL_CHECK.md` (rules table and flags)

**Interfaces:**
- Consumes: `Card::journeys` (Task 1); `commands::journeys::{records, contract}`; `graphhelm_execution::{fold_journeys, GitHistory, ScopeHistory, ChangedSince, Freshness, UnknownCause, valid_revision}`.
- Produces:

```rust
/// One screen of a journey a card names, with whether its newest capture is fresh at the head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JourneyScreen {
    pub contract_id: String,
    pub step_id: String,
    pub scope_paths: Vec<String>,
    /// `None` when the newest capture is fresh at the head; otherwise why not, in words.
    pub not_fresh: Option<String>,
}

/// Spec §6.3, advisory: one `keel.journey.no_fresh_capture` warning per screen whose scope the
/// card's scope touches (a path equal to, under, or containing the other, Keel's prefix meaning)
/// and whose newest capture is not fresh. Never blocking.
#[must_use]
pub fn check_journeys(card_scope: &[String], screens: &[JourneyScreen]) -> Vec<Finding>
```

Finding shape: `rule: "keel.journey.no_fresh_capture"`, `blocking: false`, `path: Some(<the first card scope path that touches the screen>)`, `detail: format!("{contract_id}/{step_id}: {reason}")`.

Touch test (both directions, so a card naming `web/` touches screen `web/cart/` and a card naming `web/cart/Line.tsx` touches screen `web/cart/`):

```rust
fn under(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    path == root || path.trim_end_matches('/') == root || path.starts_with(&format!("{root}/"))
}
fn touches(a: &str, b: &str) -> bool { under(a, b) || under(b, a) }
```

- [ ] **Step 1: Failing tests.**

`core/policy/tests/keel.rs`:

```rust
#[test]
fn a_touched_screen_without_a_fresh_capture_is_a_warning_and_never_blocks() {
    use graphhelm_policy::keel::{JourneyScreen, check_journeys};
    let screen = |step: &str, scope: &str, not_fresh: Option<&str>| JourneyScreen {
        contract_id: "checkout".into(), step_id: step.into(),
        scope_paths: vec![scope.into()], not_fresh: not_fresh.map(str::to_owned),
    };
    let screens = [
        screen("cart", "web/cart/", Some("no capture of this step was read")),
        screen("pay", "web/pay/", Some("stale")),
        screen("home", "web/home/", None),
    ];
    let findings = check_journeys(&["web/cart/Line.tsx".into(), "web/home".into()], &screens);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, "keel.journey.no_fresh_capture");
    assert!(!findings[0].blocking);
    assert_eq!(findings[0].path.as_deref(), Some("web/cart/Line.tsx"));
    assert_eq!(findings[0].detail, "checkout/cart: no capture of this step was read");
    assert_eq!(check_journeys(&["web".into()], &screens).len(), 2);
}
```

`apps/cli/tests/journey_producers_cli.rs` (it already has `Harness`, `prepared()`, `git()`, `capture()`; its contract `cart` has steps `open-cart` → `web/cart/`, `review` → `web/review/`, `pay` → `web/pay/`, and `web/cart/Line.tsx` is committed):

```rust
impl Harness {
    /// `keel check` over `HEAD~1..HEAD` of the project with a card naming the `cart` journey.
    fn keel_check(&self, scope: &[&str], with_records: bool) -> (i32, Value) {
        let card = self.scratch.path().join("card.json");
        std::fs::write(&card, serde_json::to_vec(&json!({
            "promise": "the cart renders", "scopePaths": scope,
            "proof": "npx playwright test", "journeys": ["cart"],
        })).unwrap()).unwrap();
        let mut command = graphhelm();
        command.args(["--json", "keel", "check", "--diff", "HEAD~1..HEAD", "--repo"])
            .arg(&self.project).arg("--card").arg(&card);
        if with_records {
            command.arg("--events").arg(&self.events).args(["--execution", RUN, "--keyring"])
                .arg(&self.keyring).args(["--key-id", KEY_ID]);
        }
        let output = command.output().unwrap();
        (output.status.code().unwrap(), envelope(&output))
    }
}

fn journey_warnings(reply: &Value) -> Vec<String> {
    reply["diagnostics"].as_array().unwrap().iter()
        .filter(|d| d["code"] == "keel.journey.no_fresh_capture")
        .map(|d| d["message"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn keel_check_warns_for_a_touched_screen_until_it_has_a_fresh_capture_at_the_head() {
    if !git_available() { return; }
    let harness = prepared();
    std::fs::write(harness.project.join("web/cart/Line.tsx"), "line 2").unwrap();
    git(&harness.project, &["commit", "-qam", "edit cart"]);

    let (code, reply) = harness.keel_check(&["web/cart/Line.tsx"], false);
    assert_eq!(code, 0, "{reply}");
    let warnings = journey_warnings(&reply);
    assert_eq!(warnings.len(), 1, "{reply}");
    assert!(warnings[0].contains("cart/open-cart"), "{reply}");
    assert!(warnings[0].contains("--events"), "the warning says captures were not read: {reply}");

    let (_, reply) = harness.keel_check(&["web/cart/Line.tsx"], true);
    assert!(journey_warnings(&reply)[0].contains("no capture"), "{reply}");

    harness.capture("open-cart", &[]);
    let (code, reply) = harness.keel_check(&["web/cart/Line.tsx"], true);
    assert_eq!(code, 0, "{reply}");
    assert!(journey_warnings(&reply).is_empty(), "a fresh capture at the head silences it: {reply}");

    std::fs::write(harness.project.join("web/cart/Line.tsx"), "line 3").unwrap();
    git(&harness.project, &["commit", "-qam", "edit cart again"]);
    let (code, reply) = harness.keel_check(&["web/cart/Line.tsx"], true);
    assert_eq!(code, 0, "{reply}");
    let warnings = journey_warnings(&reply);
    assert!(warnings[0].contains("web/cart/Line.tsx"), "a stale capture names the changed file: {reply}");
}

#[test]
fn keel_check_warns_on_a_missing_contract_and_ignores_untouched_screens() {
    if !git_available() { return; }
    let harness = prepared();
    std::fs::write(harness.project.join("README.md"), "x").unwrap();
    git(&harness.project, &["add", "-A"]);
    git(&harness.project, &["commit", "-qm", "readme"]);
    let (code, reply) = harness.keel_check(&["README.md"], false);
    assert_eq!(code, 0, "{reply}");
    assert!(journey_warnings(&reply).is_empty(), "README.md touches no screen: {reply}");
    std::fs::remove_file(harness.project.join(".graphhelm/journeys/cart.json")).unwrap();
    let (code, reply) = harness.keel_check(&["README.md"], false);
    assert_eq!(code, 0, "{reply}");
    assert!(reply["diagnostics"].as_array().unwrap().iter()
        .any(|d| d["code"] == "keel.journey.contract_unreadable" && d["severity"] == "warning"), "{reply}");
}
```

(If `capture()` in the harness asserts fields that Task 2 does not change, reuse it as is. If `diagnostics[].severity` is spelled differently in this envelope, use the spelling `keel_check.rs` already asserts on and say so in the report.)

- [ ] **Step 2: Run, expect FAIL** (unknown args `--events`; `check_journeys` missing).

- [ ] **Step 3: Implement.**

`core/policy/src/keel.rs`: `JourneyScreen`, `check_journeys` as in Interfaces. Iterate screens in input order; for each with `not_fresh = Some(reason)`, find the first card scope path `p` and screen scope path `s` with `touches(p, s)`; push the finding.

`apps/cli/src/args.rs`, inside `KeelCommand::Check`:

```rust
        /// With a card that names `journeys`: the run whose screen captures `keel check` reads
        /// (with `--execution`, `--keyring`, `--key-id`). Without them every touched screen is
        /// reported as having no capture read.
        #[arg(long, requires_all = ["execution", "keyring", "key_id"])]
        events: Option<PathBuf>,
        #[arg(long, requires = "events")]
        execution: Option<String>,
        #[arg(long, requires = "events")]
        keyring: Option<PathBuf>,
        #[arg(long, requires = "events")]
        key_id: Option<String>,
```

`apps/cli/src/commands/keel.rs`:
- `check(..)` gains `records: Option<JourneyRecords>` where `pub(super) struct JourneyRecords { pub(super) events: PathBuf, pub(super) execution: String, pub(super) keyring: PathBuf, pub(super) key_id: String }`; `mod.rs` builds it when `events` is `Some` (clap guarantees the other three).
- After `let report = policy_keel::check(..)` and before the diagnostics are built, when the card names journeys:

```rust
    let mut report = report;
    if let Some((card, _)) = card.as_ref().filter(|(card, _)| !card.journeys.is_empty()) {
        match journey_findings(repo, head, card, records.as_ref()) {
            Ok(findings) => report.findings.extend(findings),
            Err(outcome) => return outcome,
        }
    }
```

- `journey_findings`:
  1. `head_sha = git -C repo rev-parse --verify <head>^{commit}` (fixed args; failure → `input_error(.., "/diff")`).
  2. For each id in `card.journeys` (schema already validated the shape): `crate::commands::journeys::contract(&repo.join(".graphhelm/journeys").join(format!("{id}.json")), id)`; on `Err(reason)` push `Finding { rule: "keel.journey.contract_unreadable", blocking: false, path: None, detail: format!("{id}: {reason}") }` and skip it.
  3. Captures/transitions: when `records` is given, `journeys::records(&events, &execution, &SignalKeyring { directory: keyring, key_id })`; its `Err(failure)` becomes `failure.into_outcome("keel")` (exit 3; the user asked for records explicitly). Otherwise empty vectors.
  4. Fold with a history pinned to the range head (the working tree may be elsewhere):

```rust
struct AtHead { git: GitHistory, head: String }
impl ScopeHistory for AtHead {
    fn head(&self) -> Option<String> { Some(self.head.clone()) }
    fn changed_since(&self, revision: &str, head: &str) -> ChangedSince { self.git.changed_since(revision, head) }
}
```

  5. For every step with a screen, build `JourneyScreen { contract_id, step_id, scope_paths: screen.scope_paths, not_fresh }` where `not_fresh` is:
     - no capture: `"no capture of this step was read"` plus, when `records` is `None`, `" (pass --events, --execution, --keyring and --key-id to read the run's captures)"`;
     - `Freshness::Fresh` → `None`;
     - `Freshness::Stale` → `format!("code changed after the capture at {}: {}", &revision[..8.min(len)], changed_files.join(", "))`;
     - `Freshness::Unknown` → `format!("freshness unknown ({cause})")` with cause words `taken from uncommitted code` / `screen has no scope paths` / `no git history` / `revision not in the repository`.
  6. Return contract findings followed by `policy_keel::check_journeys(&card.scope_paths, &screens)`.
- Keep `refused` computed from the findings as today (journey findings are non-blocking, so it cannot flip).

`docs/keel/KEEL_CHECK.md`: add the two rules to the findings table (Blocks: "No (warning)") and document `--events/--execution/--keyring/--key-id` and the `journeys` card field / `Journeys:` markdown line in two or three sentences.

- [ ] **Step 4: Run, expect PASS**:

```
CARGO_TARGET_DIR=F:/ghp6t cargo +1.97.1 test --locked -p graphhelm-policy
CARGO_TARGET_DIR=F:/ghp6t cargo +1.97.1 test --locked -p graphhelm-cli --test keel_check --test journey_producers_cli --test journeys_surfaces
```

then fmt/clippy (`graphhelm-policy`, `graphhelm-cli`) and the source guard.

- [ ] **Step 5: Commit and push** — `feat(keel): advisory keel check finding for touched screens without a fresh capture (#321)`.

---

### Task 3: Playwright observer `--journey`

**Files:**
- Modify: `tools/playwright-observer/playwright_observe.py`
- Modify: `tools/playwright-observer/test_playwright_observe.py`
- Modify: `tools/playwright-observer/README.md`

**Interfaces:** shells out to the CLI of Task 0 (phase 4): `graphhelm journey capture --events E --execution X --keyring K --key-id I --project P --contract C --step S --image F` and `graphhelm journey walked ... --contract C --from A --to B`. Both print the JSON envelope (`{"ok": true, "data": {...}}`) when piped.

**Rule (how a step maps to a screenshot):** one Playwright test per journey step, titled exactly with the step id, with `use: { screenshot: 'on' }` so Playwright takes a screenshot after each test. A test may instead attach its own PNG named after the step id (`testInfo.attach('<stepId>', { path, contentType: 'image/png' })`); an attachment named after the step wins over the automatic `screenshot`. The observer reads the attachments from the JSON report: `suites[].specs[].tests[].results[].attachments[] = {name, contentType, path}`; the spec's `title` is the test title.

- [ ] **Step 1: Failing tests.** Add to `test_playwright_observe.py`:

```python
CONTRACT = {"contractId": "checkout", "steps": [{"stepId": "home"}, {"stepId": "cart"}, {"stepId": "pay"}]}


def journey_report(shots, statuses):
    """One spec per step; `shots[step]` is a PNG path or None; `statuses[step]` expected|unexpected."""
    specs = []
    for step, path in shots.items():
        attachments = [] if path is None else [{"name": "screenshot", "contentType": "image/png", "path": str(path)}]
        specs.append({"title": step, "tests": [{"status": statuses.get(step, "expected"),
                                                "results": [{"attachments": attachments}]}]})
    unexpected = sum(1 for s in statuses.values() if s == "unexpected")
    return {"stats": {"expected": len(specs) - unexpected, "unexpected": unexpected, "flaky": 0},
            "suites": [{"title": "checkout.spec.ts", "specs": specs, "suites": []}]}


class JourneyMode(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.project = Path(self.tmp.name)
        (self.project / ".graphhelm" / "journeys").mkdir(parents=True)
        (self.project / ".graphhelm" / "journeys" / "checkout.json").write_text(json.dumps(CONTRACT))
        self.calls = self.project / "calls.jsonl"
        self.fake_cli = self.project / "fake_graphhelm.py"
        self.fake_cli.write_text(
            "import json, sys\n"
            f"open({str(self.calls)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            "fail = 'FAIL_STEP' in open(sys.argv[0]).read() and '--step' in sys.argv and sys.argv[sys.argv.index('--step') + 1] == 'cart'\n"
            "print(json.dumps({'ok': not fail, 'data': {'signalId': 'sig', 'outcome': 'recorded'}}))\n"
            "sys.exit(1 if fail else 0)\n")

    def tearDown(self):
        self.tmp.cleanup()

    def run_journey(self, shots, statuses, exit_code=0, extra=()):
        for step, path in shots.items():
            if path is not None:
                path.write_bytes(b"\x89PNG\r\n\x1a\n")
        (self.project / "next.json").write_text(json.dumps(journey_report(shots, statuses)))
        fake = self.project / "fake_playwright.py"
        fake.write_text("import os, shutil, sys\n"
                        "shutil.copy('next.json', os.environ['PLAYWRIGHT_JSON_OUTPUT_NAME'])\n"
                        f"sys.exit({exit_code})\n")
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project),
                                 "--command", f"{sys.executable} {fake}",
                                 "--journey", "checkout", "--events", "ev", "--execution", "run-1",
                                 "--keyring", "kr", "--key-id", "key",
                                 "--graphhelm", f"{sys.executable} {self.fake_cli}", *extra],
                                capture_output=True, text=True)
        calls = [json.loads(line) for line in self.calls.read_text().splitlines()] if self.calls.exists() else []
        return result.returncode, json.loads(result.stdout), calls

    def test_each_step_is_captured_in_contract_order_then_each_pair_walked(self):
        shots = {s: self.project / f"{s}.png" for s in ("home", "cart", "pay")}
        code, out, calls = self.run_journey(shots, {})
        self.assertEqual((code, out["verdict"]), (0, "passed"))
        self.assertEqual([c[:2] for c in calls], [["journey", "capture"]] * 3 + [["journey", "walked"]] * 2)
        self.assertEqual([c[c.index("--step") + 1] for c in calls[:3]], ["home", "cart", "pay"])
        self.assertEqual(calls[0][calls[0].index("--image") + 1], str(shots["home"]))
        for flag, value in (("--events", "ev"), ("--execution", "run-1"), ("--keyring", "kr"),
                            ("--key-id", "key"), ("--contract", "checkout"), ("--project", str(self.project))):
            self.assertEqual(calls[0][calls[0].index(flag) + 1], value, flag)
        self.assertEqual([(c[c.index("--from") + 1], c[c.index("--to") + 1]) for c in calls[3:]],
                         [("home", "cart"), ("cart", "pay")])
        self.assertEqual(out["journey"]["captured"], ["home", "cart", "pay"])
        self.assertEqual(out["journey"]["walked"], [["home", "cart"], ["cart", "pay"]])

    def test_a_failed_or_unshot_step_is_not_captured_and_breaks_its_arrows(self):
        shots = {"home": self.project / "home.png", "cart": self.project / "cart.png", "pay": None}
        code, out, calls = self.run_journey(shots, {"cart": "unexpected"}, exit_code=1)
        self.assertEqual((code, out["verdict"]), (1, "failed"))
        self.assertEqual([c[c.index("--step") + 1] for c in calls if c[1] == "capture"], ["home"])
        self.assertEqual([c for c in calls if c[1] == "walked"], [])
        self.assertEqual(out["journey"]["missing"], {"cart": "test unexpected", "pay": "no screenshot"})

    def test_a_refused_capture_is_reported_and_the_test_verdict_stands(self):
        self.fake_cli.write_text(self.fake_cli.read_text() + "# FAIL_STEP\n")
        shots = {s: self.project / f"{s}.png" for s in ("home", "cart", "pay")}
        code, out, calls = self.run_journey(shots, {})
        self.assertEqual((code, out["verdict"]), (0, "passed"))
        self.assertEqual(out["journey"]["captured"], ["home", "pay"])
        self.assertIn("cart", out["journey"]["missing"])
        self.assertEqual(out["journey"]["walked"], [])

    def test_journey_without_its_record_flags_is_a_usage_error(self):
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project), "--journey", "checkout"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--events", result.stderr)

    def test_a_path_like_journey_id_is_refused_before_anything_runs(self):
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project), "--journey", "../x",
                                 "--events", "e", "--execution", "x", "--keyring", "k", "--key-id", "i"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertFalse(self.calls.exists())
```

- [ ] **Step 2: Run, expect FAIL**: `python -m unittest tools/playwright-observer/test_playwright_observe.py -v` (unknown argument `--journey`). The eight existing tests keep passing.

- [ ] **Step 3: Implement** in `playwright_observe.py` (stdlib only):
  - argparse: `--journey`, `--events`, `--execution`, `--keyring`, `--key-id`, `--graphhelm` (default `os.environ.get("GRAPHHELM_BIN", "graphhelm")`). With `--journey`: `parser.error(...)` (exit 2, message names the missing flags) unless all four record flags are given; `parser.error` too when the id fails `re.fullmatch(r"[a-z0-9][a-z0-9._-]{0,127}", id)` or contains `..`.
  - Read `<project>/.graphhelm/journeys/<id>.json` before running Playwright; unreadable or no `steps` list → `parser.error`.
  - `observe(...)` keeps its result; parse the report a second time only when `--journey` is set (`json.loads(report_path.read_bytes())`, guarded; a report that fails to parse means no step shots).
  - `_shots(suites, out)`: walk suites recursively; for each spec, for each test, the last result's attachments: per spec title record `{"status": test["status"], "path": <attachment named == spec title with image/png, else the 'screenshot' image/png one, else None>}`.
  - For each contract step in order: missing entry → `missing[step] = "no test titled with the step id"`; status not `expected` → `missing[step] = f"test {status}"`; path None → `"no screenshot"`; else run `shlex.split(graphhelm) + ["journey", "capture", "--events", .., "--execution", .., "--keyring", .., "--key-id", .., "--project", str(project), "--contract", id, "--step", step, "--image", path]` with `capture_output=True, text=True, timeout=120`; success = exit 0 and the stdout JSON has `ok: true`; else `missing[step] = "capture refused: <first diagnostics message or exit code>"`. `FileNotFoundError` → `missing[step] = "graphhelm not found"`.
  - Then for each consecutive pair of contract steps both in `captured`: run `journey walked` with `--from/--to`; on success append `[a, b]` to `walked`.
  - Add `outcome["journey"] = {"contractId": id, "captured": [...], "walked": [[a, b], ...], "missing": {...}}` and one evidence line per capture/walk (`journey checkout: captured home`, `journey checkout: walked home -> cart`). The exit code still follows the Playwright verdict only.

`README.md`: a "Journey captures" section: the flags, the one-test-per-step rule with `screenshot: 'on'` or a step-named attachment, that failed or unshot steps are not captured and break their arrows, and that it needs the Runtime's events dir and keyring because each capture is a sealed signal.

- [ ] **Step 4: Run, expect PASS** (all 13 tests). Also `CARGO_TARGET_DIR=F:/ghp6t cargo +1.97.1 test --locked -p graphhelm-cli observers` (the CLI embeds this script with `include_str!`).

- [ ] **Step 5: Commit and push** — `feat(observer): playwright --journey records captures and walked transitions (#321)`.

---

### Task 4: DELIVERY.md before/after guidance

**Files:**
- Modify: `docs/process/DELIVERY.md` (§3 Work)
- Modify: `docs/harness/JOURNEY_PROVEN_DEVELOPMENT.md` or `docs/keel/RECORDS.md`: one sentence pointing at the producers if either describes the capture records (check with grep `screen_captured`).

- [ ] **Step 1: Write** a short subsection at the end of §3 "Work", titled "Before/after captures for screens (guidance)":

```markdown
**Before/after captures for screens (guidance, not a gate).** When the card's scope touches a
journey screen's `scopePaths` (`.graphhelm/journeys/<contractId>.json`), record, for each touched
screen, a `before` capture at the base and an `after` capture at the head, and link both in the PR
body:

    git checkout <base>   # then run the app
    graphhelm journey capture --events <dir> --execution <id> --keyring <dir> --key-id <id> \
      --contract <contractId> --step <stepId> --image before.png --pr <N> --phase before
    git checkout <head>   # then run the app
    graphhelm journey capture ... --image after.png --pr <N> --phase after

`tools/playwright-observer/playwright_observe.py --journey <contractId>` records the head captures and
the walked transitions in one run. Name the journeys in the card (`journeys: [contractId]`, or a
`Journeys:` line in the PR body); `graphhelm keel check` then warns, without blocking, about a
touched screen with no fresh capture at the head. Gates are off since 2026-09-24: a PR without
captures is reviewed and merged like any other, and the review says what was not observed.
```

- [ ] **Step 2: Verify** `git diff --check` and that no closing keyword appears in the docs.
- [ ] **Step 3: Commit and push** — `docs(delivery): before/after captures for PRs touching journey screens (#321)` with `Closes #321`.

## Self-review

- Spec §6.3 bullets: observer (Task 3), DELIVERY (Task 4), card `journeys` (Task 1), advisory check (Task 2). The two CLI producers already exist (phase 4).
- Types: `Card::journeys: Vec<String>` used in Tasks 1–2; `JourneyScreen` / `check_journeys` defined and consumed in Task 2; observer calls match the CLI flags in `apps/cli/src/args.rs` (`JourneyRecordArgs`).
- Review Focus 1 → Task 2 test (`--events` in the message); 2 → fold already picks the newest non-dirty capture, and an unknown cause maps to a warning; 3 → Task 2 second test; 4 → Task 3 second test; 5 → Task 3 third test.
