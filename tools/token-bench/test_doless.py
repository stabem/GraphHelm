"""Focused tests for the do-less scorer: each names the defect it would catch."""
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("doless_under_test", HERE / "doless.py")
doless = importlib.util.module_from_spec(spec)
spec.loader.exec_module(doless)


def check(findings=(), surface=None, proofs=None, exit_code=0):
    full = {"newTests": 0, "newPublicSymbols": 0, "undeclaredPublicSymbols": []}
    full.update(surface or {})
    data = {"findings": list(findings), "surface": full}
    if proofs is not None:
        data["testProof"] = {"proofs": proofs}
    return {"exit": exit_code, "data": data}


def green_row(**surface):
    cols = {"keelCheck": "PASS", "pathsOutsideCard": 0, "unrequestedNewFiles": [], "unrequestedNewTests": 0,
            "undeclaredPublicSymbols": [], "newTestsGreenOnParent": 0}
    cols.update(surface)
    return {"oracle": "PASS", "regression": "PASS", "surface": cols, "agentError": None}


def test_path_outside_card_is_counted_from_the_keel_finding():
    """Catches reading the outside-card count from the wrong field (a silent zero)."""
    finding = {"rule": "keel.scope.path_outside_card", "path": "docs/extra.md", "blocking": True}
    cols = doless.surface_columns(check([finding], exit_code=2), [], {})
    assert cols["pathsOutsideCard"] == 1 and cols["pathsOutsideCardList"] == ["docs/extra.md"]
    assert cols["keelCheck"] == "REFUSED"


def test_only_tests_beyond_the_tasks_allowance_are_unrequested():
    """Catches charging a task for the one regression test it asked for, or not charging a second."""
    cols = doless.surface_columns(check(surface={"newTests": 3, "undeclaredPublicSymbols": []}), [], {"newTests": 1})
    assert cols["unrequestedNewTests"] == 2


def test_declared_symbols_and_files_are_not_unrequested():
    """Catches penalising the helper the historical fix itself introduced."""
    surface = {"newTests": 0, "undeclaredPublicSymbols": ["compose_arm_c_prompt", "PromptBuilder"]}
    cols = doless.surface_columns(check(surface=surface), ["a.py", "new_helper.py"],
                                  {"publicSymbols": ["compose_arm_c_prompt"], "newFiles": ["a.py"]})
    assert cols["undeclaredPublicSymbols"] == ["PromptBuilder"]
    assert cols["unrequestedNewFiles"] == ["new_helper.py"]


def test_green_on_parent_tests_are_counted():
    proofs = [{"verdict": "green_on_parent"}, {"verdict": "earned"}]
    cols = doless.surface_columns(check(proofs=proofs), [], {})
    assert cols["newTestsGreenOnParent"] == 1 and cols["newTestsProved"] == 2


def test_a_clean_run_passes_and_any_extra_surface_fails():
    assert doless.do_less_verdict(green_row()) == ("PASS", [])
    verdict, reasons = doless.do_less_verdict(green_row(unrequestedNewFiles=["x.py"]))
    assert verdict == "FAIL" and reasons == ["unrequestedNewFiles"]


def test_a_broken_instrument_is_incomplete_never_a_pass():
    """Catches a keel-check error or a missing regression observer scoring as do-less."""
    row = green_row()
    row["surface"] = {"keelCheck": "ERROR", "keelCheckError": "no executable"}
    assert doless.do_less_verdict(row)[0] == "INCOMPLETE"
    row = green_row()
    row["regression"] = "UNOBSERVED"
    assert doless.do_less_verdict(row)[0] == "INCOMPLETE"
    row = green_row()
    row["surface"] = {}
    assert doless.do_less_verdict(row)[0] == "INCOMPLETE"


def scored(check_result, prove=False):
    row = green_row()
    row["surface"] = doless.surface_columns(check_result, [], {}, prove) if prove else         doless.surface_columns(check_result, [], {})
    return doless.do_less_verdict(row)


def test_a_refusal_for_another_rule_is_incomplete_never_a_pass():
    """Catches keel check exit 2 for a rule other than path_outside_card scoring PASS (PR #1336 review)."""
    other = {"rule": "keel.body.oversized_change", "path": "a.py", "blocking": True}
    assert scored(check([other], exit_code=2))[0] == "INCOMPLETE"
    outside = {"rule": "keel.scope.path_outside_card", "path": "docs/x.md", "blocking": True}
    assert scored(check([outside, other], exit_code=2))[0] == "INCOMPLETE"
    assert scored(check([], exit_code=2))[0] == "INCOMPLETE"
    # A hand-built row that says REFUSED with nothing outside the card is not a pass either.
    assert doless.do_less_verdict(green_row(keelCheck="REFUSED"))[0] == "INCOMPLETE"


def test_an_unexpected_exit_or_blocking_finding_on_exit_zero_is_incomplete():
    assert scored({"exit": 1, "data": {"findings": [], "surface": {}}})[0] == "INCOMPLETE"
    blocking = {"rule": "keel.card.missing", "path": "", "blocking": True}
    assert scored(check([blocking], exit_code=0))[0] == "INCOMPLETE"


def test_a_missing_surface_or_field_is_incomplete_never_zeros():
    """Catches an envelope without `surface` (or one of its fields) reading as zeros and passing."""
    assert scored({"exit": 0, "data": {"findings": []}})[0] == "INCOMPLETE"
    assert scored({"exit": 0, "data": {"findings": [], "surface": {"newTests": 0}}})[0] == "INCOMPLETE"
    assert scored({"exit": 0, "data": {"surface": {"newTests": 0, "newPublicSymbols": 0,
                                                  "undeclaredPublicSymbols": []}}})[0] == "INCOMPLETE"
    assert doless.do_less_verdict(green_row() | {"surface": {"keelCheck": "PASS"}})[0] == "INCOMPLETE"


def test_a_prove_run_without_proofs_is_incomplete():
    assert scored(check(), prove=True)[0] == "INCOMPLETE"
    assert scored(check(proofs=[]), prove=True) == ("PASS", [])


def test_the_one_explained_refusal_still_fails():
    outside = {"rule": "keel.scope.path_outside_card", "path": "docs/x.md", "blocking": True}
    assert scored(check([outside], exit_code=2)) == ("FAIL", ["pathsOutsideCard"])


def test_a_red_oracle_fails_even_with_a_minimal_diff():
    row = green_row()
    row["oracle"] = "FAIL"
    assert doless.do_less_verdict(row) == ("FAIL", ["oracle_fail"])


def test_agents_keel_section_stops_at_the_next_heading():
    text = "# A\n### Keel: how code is written here\nbody\n\n## Next\nother"
    assert doless.agents_keel_section(text) == "### Keel: how code is written here\nbody\n"


def test_the_frozen_split_matches_its_rule_and_every_category_has_train_cases():
    """Catches a hand-edited split (tasks moved after results were read) or a category the
    hillclimber can never see."""
    tasks = doless.load_tasks()
    assert all(task["split"] == doless.split_of(task_id, tasks) for task_id, task in tasks.items())
    for category in {task["category"] for task in tasks.values()}:
        splits = {task["split"] for task in tasks.values() if task["category"] == category}
        assert "train" in splits


def test_every_task_names_files_that_exist():
    tasks = doless.load_tasks()
    assert 6 <= len(tasks) <= 30
    for task_id, task in tasks.items():
        for key in ("prompt", "promptIssue", "card", "oracle"):
            assert (doless.DOLESS / task[key]).is_file(), (task_id, key)
        card = json.loads((doless.DOLESS / task["card"]).read_text(encoding="utf-8"))
        assert card["scopePaths"] and card["promise"] and card["proof"], task_id
        assert task.get("fixSha") or task.get("referenceAnswer"), task_id


def test_no_test_task_shares_a_parent_or_fix_with_a_train_task():
    """Catches the leak #1343 found: py-utf8-leave-timeout (test) had tb-utf8-prompt's parent and
    fix, so a hillclimber reading train transcripts saw a test answer."""
    tasks = doless.load_tasks()
    assert doless.lineage_overlaps(tasks) == []
    leaky = {"t": {"split": "test", "parentSha": "p", "fixSha": "f"},
             "u": {"split": "train", "parentSha": "q", "fixSha": "f"},
             "om": {"split": "train", "parentSha": "p", "fixSha": None}}
    assert doless.lineage_overlaps(leaky) == [("t", "u")]


def test_issue_prompts_do_not_name_the_answer_format():
    """Catches an issue-style prompt that hints the observer-missing answer (the harness prefix
    states it once for every arm; the issue must not repeat it)."""
    for task_id, task in doless.load_tasks().items():
        text = (doless.DOLESS / task["promptIssue"]).read_text(encoding="utf-8")
        assert "OBSERVER_MISSING" not in text, task_id
        assert "must stay green" not in text, task_id


def test_arm_d_carries_the_digest_and_the_skill_path_but_not_the_skill(tmp_path):
    """Catches arm d silently pasting the full surfaces (no cost difference to measure) or losing
    the path to the full skill."""
    task = next(iter(doless.load_tasks().values()))
    for key, rel in doless.SURFACES.items():
        (tmp_path / rel).parent.mkdir(parents=True, exist_ok=True)
        body = (doless.REPO / rel).read_text(encoding="utf-8")
        (tmp_path / rel).write_text(body, encoding="utf-8")
    prompt, digests = doless.compose_prompt(task, "d", tmp_path, "issue")
    assert set(digests) == {"digest"}
    assert "BEGIN KEEL DIGEST" in prompt and "BEGIN KEEL SKILL" not in prompt
    assert str(doless.SURFACES["skill"].parent).replace("\\", "/") in prompt
    assert (doless.DOLESS / task["promptIssue"]).read_text(encoding="utf-8") in prompt
    assert len((tmp_path / doless.SURFACES["digest"]).read_text(encoding="utf-8").splitlines()) <= 20


def test_arm_k_pastes_the_full_keel_reference_not_the_digest_skill():
    """Catches #1349: after #1348 the keel SKILL.md became the digest, so arm k pasting it measured
    the digest twice and full Keel never."""
    assert doless.SURFACES["skill"].name == "REFERENCE.md"
    task = next(iter(doless.load_tasks().values()))
    prompt, digests = doless.compose_prompt(task, "k", doless.REPO, "issue")
    reference = (doless.REPO / doless.SURFACES["skill"]).read_text(encoding="utf-8")
    assert reference in prompt and "Keel, in brief" not in prompt
    assert set(digests) == {"skill", "agentsKeel", "keelYaml"}


def test_a_linker_or_file_lock_fault_is_infra_not_fail(tmp_path):
    """Catches #1346: `LNK1104` on a test .exe another run held scored as a code FAIL."""
    for message in ("LINK : fatal error LNK1104: cannot open file 'catalog_integrity-1.exe'",
                    "failed to remove file: The process cannot access the file because it is being "
                    "used by another process. (os error 32)"):
        code = f"import sys; print({message!r}); sys.exit(101)"
        status, exit_code, _ = doless.run_command(["{python}", "-c", code], tmp_path)
        assert (status, exit_code) == ("INFRA", 101)
    status, _, _ = doless.run_command(["{python}", "-c", "import sys; print('assertion failed'); sys.exit(101)"],
                                      tmp_path)
    assert status == "FAIL"


def test_an_infra_fault_in_either_observer_is_incomplete():
    for key in ("regression", "oracle"):
        row = green_row()
        row[key] = "INFRA"
        verdict, reasons = doless.do_less_verdict(row)
        assert verdict == "INCOMPLETE" and f"{key}_infra" in reasons


def test_each_checkout_builds_into_its_own_cargo_target(tmp_path, monkeypatch):
    """Catches #1346: runs inherited one CARGO_TARGET_DIR and locked each other's test .exe."""
    monkeypatch.setenv("TOKEN_BENCH_SCRATCH", str(tmp_path / "scratch"))
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "shared"))
    task = {"regression": {"command": ["{python}", "-c", "import os; print(os.environ['CARGO_TARGET_DIR'])"]}}
    seen = []
    for name in ("tb-one", "tb-two"):
        wt = tmp_path / name
        wt.mkdir()
        status, _, out = doless.run_regression(wt, task)
        assert status == "PASS"
        seen.append(out.strip())
    assert len(set(seen)) == 2 and str(tmp_path / "shared") not in seen


def test_every_task_cites_public_history_and_a_disjoint_split():
    """Catches a task copied from the private archive (its commits cannot be checked out here) and
    a split with no test task."""
    tasks = doless.load_tasks()
    for task_id, task in tasks.items():
        for sha in filter(None, (task["parentSha"], task.get("fixSha"))):
            probe = subprocess.run(["git", "merge-base", "--is-ancestor", sha, "HEAD"], cwd=doless.REPO,
                                   capture_output=True)
            assert probe.returncode == 0, (task_id, sha)
    assert {t["split"] for t in tasks.values()} == {"train", "test"}


def row(arm, verdict, cost=0.10):
    return {"arm": arm, "verdict": verdict, "costUsd": cost}


def test_incomplete_rows_are_excluded_from_score_but_counted_in_cost():
    """Catches an INCOMPLETE run scoring as a failure (or a pass), or its spend vanishing."""
    arms = doless.arm_scores([row("a", "PASS", 0.1), row("a", "FAIL", 0.1), row("a", "INCOMPLETE", 0.4)])
    a = arms["a"]
    assert (a["runs"], a["scored"], a["passes"], a["incomplete"]) == (3, 2, 1, 1)
    assert a["passRate"] == 0.5
    assert abs(a["costTotal"] - 0.6) < 1e-9 and abs(a["costMean"] - 0.2) < 1e-9
    assert abs(a["costMedian"] - 0.1) < 1e-9 and abs(a["costPerPass"] - 0.6) < 1e-9


def test_the_frontier_drops_only_dominated_arms():
    arms = doless.arm_scores(
        [row("cheap", "PASS", 0.05), row("cheap", "FAIL", 0.05)]          # 50%, 0.05
        + [row("best", "PASS", 0.20), row("best", "PASS", 0.20)]          # 100%, 0.20
        + [row("worse", "PASS", 0.30), row("worse", "FAIL", 0.30)])       # 50%, 0.30: dominated
    assert {a: c["frontier"] for a, c in arms.items()} == {"cheap": True, "best": True, "worse": False}


def test_ties_on_one_axis_are_dominated_and_exact_ties_are_not():
    """Catches strict-on-both dominance (an arm with the same score at a higher cost staying on the
    frontier) and an exact tie knocking both arms off."""
    same_score = doless.arm_scores([row("a", "PASS", 0.1), row("b", "PASS", 0.2)])
    assert same_score["a"]["frontier"] is True and same_score["b"]["frontier"] is False
    same_cost = doless.arm_scores([row("a", "PASS", 0.1), row("b", "FAIL", 0.1)])
    assert same_cost["a"]["frontier"] is True and same_cost["b"]["frontier"] is False
    exact = doless.arm_scores([row("a", "PASS", 0.1), row("b", "PASS", 0.1)])
    assert exact["a"]["frontier"] is True and exact["b"]["frontier"] is True


def test_an_arm_that_cannot_be_placed_is_unplaced_and_dominates_nothing():
    """Catches an all-INCOMPLETE arm (score undefined) or an arm with a missing cost reading as
    free and knocking real arms off the frontier."""
    arms = doless.arm_scores([row("a", "PASS", 0.2), row("x", "INCOMPLETE", 0.0),
                              row("y", "PASS", None)])
    assert arms["a"]["frontier"] is True
    assert arms["x"]["frontier"] is None and arms["x"]["passRate"] is None
    assert arms["y"]["frontier"] is None and arms["y"]["costMean"] is None and arms["y"]["costMissing"] == 1


def test_the_wilson_interval_is_bounded_and_known():
    assert doless.wilson_interval(0, 0) is None
    lo, hi = doless.wilson_interval(10, 10)
    assert abs(hi - 1.0) < 1e-9 and hi <= 1.0 and abs(lo - 0.7225) < 1e-3
    lo, hi = doless.wilson_interval(5, 10)
    assert abs(lo - 0.2366) < 1e-3 and abs(hi - 0.7634) < 1e-3


def test_the_report_and_chart_name_every_arm_and_the_frontier(tmp_path, monkeypatch):
    rows = [dict(row("a", "PASS", 0.1), split="train"), dict(row("k", "FAIL", 0.3), split="train"),
            dict(row("d", "INCOMPLETE", 0.2), split="test")]
    results = tmp_path / "results.jsonl"
    results.write_text("".join(json.dumps(r) + "\n" for r in rows), encoding="utf-8")
    monkeypatch.setattr(doless, "RESULTS", results)
    chart = tmp_path / "chart.svg"
    monkeypatch.setattr(sys, "argv", ["doless.py", "table", "--pareto", "--split", "train", "--svg", str(chart)])
    doless.main()
    svg = chart.read_text(encoding="utf-8")
    assert svg.startswith("<svg") and 'class="on"' in svg and 'class="off"' in svg
    assert ">a<" in svg and ">k<" in svg and ">d<" not in svg


def _oracle(name: str):
    sys.path.insert(0, str(HERE / "doless" / "oracles"))
    spec = importlib.util.spec_from_file_location(f"oracle_{name}", HERE / "doless" / "oracles" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


PARENT_LIGHT = "| Docs, comments, config values, a one-line fix, a test-only fix | Nothing beyond the summary. |"
PARENT_HEAVY = "| Persistence, permissions, compatibility, security, external effects | The full card and the JPD flow. |"


def test_config_risk_accepts_behavioural_evidence_on_the_expanded_row():
    """Catches #139: the old grader demanded the evidence wording inside the light row, failing a
    correct edit that put it on the expanded route, as the issue prompt reads."""
    oracle = _oracle("docs_config_risk")
    table = oracle.rows("\n".join([
        "| Docs, comments, inert config (a display label), a one-line fix, a test-only fix | Nothing beyond the summary. |",
        "| Persistence, permissions, compatibility, security, external effects, config that changes runtime or"
        " security behaviour (a timeout, a permission) | The full card and the JPD flow, with behavioural evidence. |"]))
    assert oracle.problems(table) == []


def test_config_risk_still_fails_the_parent_table_and_a_missing_evidence_rule():
    oracle = _oracle("docs_config_risk")
    assert len(oracle.problems(oracle.rows(PARENT_LIGHT + "\n" + PARENT_HEAVY))) == 3
    no_evidence = oracle.rows("\n".join([
        "| Docs, comments, inert config values, a one-line fix | Nothing beyond the summary. |",
        "| Persistence, permissions, security, runtime-affecting config | The full card and the JPD flow. |"]))
    assert oracle.problems(no_evidence) == ["no routing row says runtime-affecting config needs behavioural evidence"]


END_IMPL = '''
{prelude}
def request(url, token, method, body=None, headers=None, timeout: float = 1.0):
    pass


def _end_impl(payload: dict, host: str) -> None:
    signal_id = "x"
    reply = request(f"{{url}}/v1/executions/{{execution}}/signal", token, "POST", {{}}, {{"Idempotency-Key": signal_id}}{timeout})
'''


def test_sessionend_resolves_a_named_constant_on_the_codex_host():
    """Catches #139: the old grader read only a literal `timeout=` and saw no wait at all."""
    oracle = _oracle("py_sessionend_cap")
    source = END_IMPL.format(prelude="CODEX_END_WAIT = 2.0",
                             timeout=', timeout=CODEX_END_WAIT if host == "codex" else 3.0')
    assert oracle.end_signal_waits(source) == [2.0]
    assert oracle.end_signal_waits(source, host="claude") == [3.0]


def test_sessionend_still_reads_literals_defaults_and_refuses_what_it_cannot_resolve():
    oracle = _oracle("py_sessionend_cap")
    assert oracle.end_signal_waits(END_IMPL.format(prelude="", timeout=", timeout=3.0")) == [3.0]
    assert oracle.end_signal_waits(END_IMPL.format(prelude="", timeout=", timeout=2.5")) == [2.5]
    assert oracle.end_signal_waits(END_IMPL.format(prelude="", timeout="")) == [1.0]
    try:
        oracle.end_signal_waits(END_IMPL.format(prelude="", timeout=", timeout=os_wait()"))
    except oracle.Unresolved:
        pass
    else:
        raise AssertionError("an unresolvable timeout must not pass silently")


END_BODY = '''
CODEX_END_WAIT = 2.0


def request(url, token, method, body=None, headers=None, timeout: float = 5.0):
    pass


def _end_impl(payload: dict, host: str) -> None:
{body}
    reply = request(f"{{url}}/v1/executions/{{execution}}/signal", token, "POST", {{}}, {{}}, timeout={timeout})
'''


def _waits(body: str, timeout: str):
    oracle = _oracle("py_sessionend_cap")
    return oracle, lambda: oracle.end_signal_waits(END_BODY.format(body=body, timeout=timeout))


def test_sessionend_fails_closed_on_ambiguous_bindings_and_unknown_operators():
    """Catches the #140 BLOCK: flow-insensitive binding and an unknown operator read as False
    accepted a Codex wait of 5 or 6 seconds as 1."""
    cases = {
        "branch assignment on host": ('    if host == "codex":\n        w = 5.0\n    else:\n        w = 1.0', "w"),
        "augmented assignment": ("    w = 1.0\n    w += 5", "w"),
        "host rebound": ('    host = "claude"', '1.0 if host == "codex" else 5.0'),
        "is comparison": ("    pass", '5.0 if host is "codex" else 1.0'),
        "rebinding": ("    w = 1.0\n    w = 5.0", "w"),
    }
    for label, (body, timeout) in cases.items():
        oracle, run = _waits(body, timeout)
        try:
            waits = run()
        except oracle.Unresolved:
            continue
        raise AssertionError(f"{label}: resolved to {waits} instead of failing closed")


def test_sessionend_still_accepts_a_straight_line_local_and_a_module_constant():
    _, run = _waits("    w = CODEX_END_WAIT if host == \"codex\" else 3.0", "w")
    assert run() == [2.0]
    _, run = _waits("    pass", "min(CODEX_END_WAIT, 2.5)")
    assert run() == [2.0]


def inference_config():
    return {"version": 1, "host": "claude_code", "provider": "anthropic",
            "model": "claude-test-exact", "effort": "high",
            "supportedEfforts": ["low", "high"],
            "capabilitySource": "https://code.claude.com/docs/en/model-config"}


def test_inference_configuration_is_frozen_and_refuses_unsupported_controls(tmp_path):
    """Catches aliases, unsupported effort or conflicting CLI model reaching a paid run; <1s, files only."""
    import pytest
    path = tmp_path / "inference.json"
    config = inference_config()
    path.write_text(json.dumps(config), encoding="utf-8")
    first = doless.load_inference_config(path, None)
    path.write_text(json.dumps(config, indent=4), encoding="utf-8")
    assert doless.load_inference_config(path, None) == first
    assert first["configuration"] == config and first["digest"].startswith("sha256:")
    for changed in ({"effort": "max"}, {"model": "sonnet"}, {"host": "unknown"},
                    {"supportedEfforts": []}, {"version": 2}, {"capabilitySource": ""}):
        path.write_text(json.dumps(config | changed), encoding="utf-8")
        with pytest.raises(ValueError):
            doless.load_inference_config(path, None)
    path.write_text(json.dumps(config), encoding="utf-8")
    with pytest.raises(ValueError):
        doless.load_inference_config(path, "another-model")


def test_cli_effort_capability_probe_refuses_before_agent_session(monkeypatch):
    """Catches passing a flag unsupported by the pinned executable; local --help I/O fixture, <1s."""
    import pytest
    seen = []
    def help_text(cmd, **kw):
        seen.append(cmd)
        assert kw["timeout"] == 10
        return subprocess.CompletedProcess(cmd, 0, stdout="  --effort <level>  Reasoning effort", stderr="")
    monkeypatch.setattr(doless.subprocess, "run", help_text)
    observation = doless.observe_effort_flag({"path": "pinned-claude"})
    assert observation["flag"] == "--effort" and observation["source"] == "cli_help"
    assert seen == [["pinned-claude", "--help"]]
    monkeypatch.setattr(doless.subprocess, "run", lambda *a, **kw: subprocess.CompletedProcess(a, 0, "", ""))
    with pytest.raises(RuntimeError):
        doless.observe_effort_flag({"path": "pinned-claude"})


def test_reports_separate_inference_configurations_and_keep_unknown_cost_unknown():
    """Catches cheaper model/effort rows being pooled with a methodology arm; no I/O, <1s."""
    rows = [row("a", "PASS", 0.2) | {"task": "t", "inferenceConfig": {"digest": "sha256:one"}},
            row("a", "FAIL", 0.1) | {"task": "t", "inferenceConfig": {"digest": "sha256:two"}},
            row("a", "INCOMPLETE", None) | {"task": "t"}]
    scores = doless.arm_scores(rows)
    assert set(scores) == {"a@sha256:one", "a@sha256:two", "a"}
    assert scores["a@sha256:one"]["passRate"] == 1
    assert scores["a@sha256:two"]["passRate"] == 0
    assert scores["a"]["costMean"] is None
    summary = doless.summarise(rows)
    assert summary[("t", "a")]["cost"] is None
    assert summary[("t", "a@sha256:one")]["cost"] == 0.2


def test_configured_run_records_requested_effort_and_incomplete_usage(tmp_path, monkeypatch):
    """Catches unobserved usage/model being published as a configured PASS; mocked process/file I/O, <1s."""
    import argparse
    config = tmp_path / "config.json"
    config.write_text(json.dumps(inference_config()), encoding="utf-8")
    result_path = tmp_path / "rows.jsonl"
    cli = tmp_path / "claude"
    cli.write_bytes(b"pinned")
    cli_identity = {"path": str(cli), "sha256": doless.runner.digest_file(cli), "version": "fixture"}
    monkeypatch.setattr(doless, "RESULTS", result_path)
    monkeypatch.setattr(doless.runner, "validate_prerequisites", lambda _arm: cli_identity)
    monkeypatch.setattr(doless, "observe_effort_flag", lambda _cli: {"flag": "--effort", "source": "cli_help"})
    monkeypatch.setattr(doless.runner, "make_worktree", lambda *_a: tmp_path)
    monkeypatch.setattr(doless, "score_checkout", lambda *_a: green_row())
    monkeypatch.setattr(doless.runner, "transcript_usage", lambda _id: {"models": ["claude-test-exact"]})
    seen = []
    def agent(*args, **kw):
        seen.append((args[3], kw))
        return {"session_id": "fixture", "result": "done", "total_cost_usd": 0.3}, "", 1.0
    monkeypatch.setattr(doless.runner, "run_agent", agent)
    args = argparse.Namespace(task=next(iter(doless.load_tasks())), inference_config=str(config),
                              model=None, surface_dir=None, arm="a", prompt_style="issue", runs=1,
                              timeout_min=1, max_budget_usd=1.0, prove=False, keep=True)
    doless.cmd_run(args)
    recorded = json.loads(result_path.read_text())
    assert seen == [("claude-test-exact", {"effort": "high"})]
    assert recorded["requestedEffort"] == "high" and recorded["observedEffort"] is None
    assert recorded["inferenceConfig"]["configuration"] == inference_config()
    assert recorded["costProvenance"] == "cli_estimate"
    assert recorded["verdict"] == "INCOMPLETE"
    assert "usage_incomplete" in recorded["verdictReasons"]
