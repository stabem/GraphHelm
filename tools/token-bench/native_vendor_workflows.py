"""Pinned native vendor workflow registry factory (planning only)."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess

SCHEMA = "native-methodology-workflow/1"
PINNED = {
    "speckit": "67e0f7294e6e728ab60bf7b4590deb470448bc2d",
    "openspec": "6d04a1b58dedc95729d34e4f223d9d483aae210a",
    "bmad": "61e01f82644c4bb533750cfa0d6d61ab61d2cc9b",
    "superpowers": "2ce8aa15d05bd57213e6f746f953aafb7ded7a8b",
}

def _digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

def _asset_digests(root: Path) -> dict[str, str]:
    return {str(path): _digest(path) for path in sorted(root.rglob("*"))
            if path.is_file() and not path.is_symlink()}


def _baseline_head(root: Path) -> str | None:
    try:
        result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True,
                                text=True, timeout=15,
                                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0), check=False)
    except (OSError, subprocess.TimeoutExpired):
        return None
    head = result.stdout.strip()
    return head if result.returncode == 0 and len(head) == 40 else None

def _phase(*, phase_id: str, skill: str, cli: str, task_ref: str,
           task_brief: str, approval_policy: str, project_root: Path,
           operations: list[str], output_globs: list[str]) -> dict:
    prefix = "$" if cli == "codex" else "/"
    prompt = (f"{prefix}{skill} {task_brief}\n"
              f"Task ref: {task_ref}. Phase boundary: execute only {phase_id}; "
              f"stop after its declared artifacts. Approval policy: {approval_policy}.")
    patterns = {"constitution": r"resolve-template\.ps1\s+constitution-template\s+-Json",
                "specify": r"(?:resolve-template|create-new-feature)\.ps1",
                "plan": r"setup-plan\.ps1\s+-Json",
                "tasks": r"setup-tasks\.ps1\s+-Json",
                "explore": r"openspec(?:\.cmd)?\s+list\s+--json",
                "propose": r"openspec(?:\.cmd)?\s+(?:new\s+change|instructions)",
                "sync": r"openspec(?:\.cmd)?\s+sync\s+specs",
                "archive": r"openspec(?:\.cmd)?\s+archive",
                "setup": r"uv\s+run\s+--no-cache.*bmad/scripts/setup\.py.*--project-root",
                "build": r"uv\s+run\s+--no-cache.*render_skill\.py"}
    if phase_id == "method_review":
        patterns[phase_id] = r"uv\s+run\s+--no-cache.*render_skill\.py"
    required = [] if phase_id not in patterns else [{"name": phase_id,
        "commandPattern": patterns[phase_id]}]
    skill_path = project_root / (".claude/skills" if cli == "claude" else ".agents/skills") / skill / "SKILL.md"
    return {"id": phase_id, "command": [], "nativeEntrypoint": f"{prefix}{skill}",
            "prompt": prompt, "frameworkStage": skill,
            "requiredOperations": required,
            "requiredArtifactGlobs": output_globs,
            "requiredArtifacts": [], "requiredWitnessKinds": ["native_stream", "native_skill"],
            "requiredWitnesses": [{"kind": "native_skill", "path": str(skill_path),
                                    "sha256": _digest(skill_path)}],
            "costKinds": ["native_cli", skill]}

def _workflow(*, method_id: str, framework: str, project_root: Path,
              task_ref: str, skill_root: str, cli: str, executable: Path,
              stages: list[tuple[str, str, list[str], list[str]]],
              task_brief: str, approval_policy: str,
              task_context_root: Path, public_files: list[str], public_tests: list[str], scope: list[str]) -> dict:
    if not project_root.is_dir() or not executable.is_file():
        raise ValueError("native project fixture or executable is missing")
    phases = [_phase(phase_id=phase_id, skill=skill, cli=cli, task_ref=task_ref,
                     task_brief=task_brief, approval_policy=approval_policy,
                     project_root=project_root, operations=ops, output_globs=globs)
              for phase_id, skill, ops, globs in stages]
    phases.append({"id": "review", "command": [], "nativeEntrypoint": None,
                   "prompt": ("Fresh independent review of task " + task_ref + ". Read the public task, "
                              "method artifacts, actual Git diff and focused source. Run the public "
                              "and submitted observers. Do not edit any project file. Review the "
                              "committed candidate at the Git head in "
                              ".native-observations/review-binding.json, which the benchmark controller "
                              "creates before this phase. "
                              "Return only a JSON object with reviewer, taskId, gitHead, patchSha256, "
                              "verdict (APPROVE, APPROVE-WITH-RISK, or BLOCK), and findings. "
                              "Use reviewer=native-reviewer and taskId=" + task_ref + ". "
                              "The controller verifies the patch digest against Git independently; "
                              "BLOCK if the candidate cannot be inspected or a finding prevents approval. "
                              "No author conversation or expected verdict is available."),
                   "frameworkStage": "independent-review", "requiredOperations": [],
                   "requiredArtifactGlobs": [], "requiredArtifacts": [],
                   "requiredWitnessKinds": ["native_stream"], "requiredWitnesses": [],
                   "costKinds": ["native_cli", "independent_review"]})
    artifact_prefixes = {"speckit": [".native-artifacts", ".specify/memory", "specs"],
                         "openspec": [".native-artifacts", "openspec/changes"],
                         "bmad": [".native-artifacts", "_bmad-output"],
                         "superpowers": [".native-artifacts", "docs/superpowers"]}
    return {"id": f"native-{method_id}", "version": f"pinned-{PINNED[method_id]}",
            "framework": framework, "maxFixAttempts": 1,
            "projectRoot": str(project_root.resolve()), "taskRefs": [task_ref],
            "baselineHead": _baseline_head(project_root),
            "methodArtifactPrefixes": artifact_prefixes[method_id],
            "pinnedPublicFiles": list(dict.fromkeys(public_tests + public_files)),
            "methodAssetDigests": _asset_digests(project_root / skill_root),
            "taskContext": {"task": task_ref, "scope": list(scope),
                             "publicTests": list(public_tests), "request": task_brief},
            "native": {"cli": cli, "executable": str(executable.resolve()),
                        "integration": "native-project-skills",
                        "taskContextRoot": str(task_context_root.resolve()),
                        "publicTaskFiles": list(public_files), "skillRoot": skill_root,
                        "assetRoot": str((project_root / skill_root).resolve()),
                        "pin": PINNED[method_id]}, "phases": phases}

def build_vendor_registry(*, project_root: Path, task_ref: str, asset_root: Path,
                          project_roots: dict[str, Path],
                          cli: str, executable: Path, public_files: list[str], public_tests: list[str], scope: list[str],
                          task_brief: str,
                          approval_policy: str = "read-only inspection is automatic; writes require fixture approval") -> dict:
    """Build four independent project-local method fixtures; never execute them."""
    if not task_ref or "<" in task_ref or ">" in task_ref:
        raise ValueError("task_ref must be concrete")
    roots = {name: Path(project_roots[name]) for name in PINNED}
    if any(not root.is_dir() for root in roots.values()):
        raise ValueError("every method project root must be an existing fixture")
    skill_root = ".claude/skills" if cli == "claude" else ".agents/skills"
    speckit = [
        ("constitution", "speckit-constitution", ["read constitution; preserve gates"], [".specify/memory/constitution.md"]),
        ("specify", "speckit-specify", ["run setup checks; resolve template; write spec.md and feature.json"], ["specs/*/spec.md", ".specify/feature.json"]),
        ("plan", "speckit-plan", ["run setup-plan.ps1 -Json; read spec and constitution; write design artifacts"], ["specs/*/plan.md", "specs/*/research.md", "specs/*/data-model.md", "specs/*/contracts/**"]),
        ("tasks", "speckit-tasks", ["run setup-tasks.ps1 -Json; read the returned feature directory and task template; write tasks.md"], ["specs/*/tasks.md"]),
        ("implement", "speckit-implement", ["read tasks, plan, contracts and checklists; implement and run declared tests"], ["**/*"]),
        ]
    openspec = [
        ("explore", "openspec-explore", ["run openspec list --json; keep exploration read-only"], []),
        ("propose", "openspec-propose", ["confirm local root; create proposal, specs, design and tasks; stop at the native planning boundary"], ["openspec/changes/*/proposal.md", "openspec/changes/*/tasks.md"]),
        ("apply", "openspec-apply-change", ["start a new native request after proposal review; read apply instructions and every context file; implement and validate"], ["**/*"]),]
    bmad = [
        ("setup", "bmad", ["run installed knowledge command; do not infer workflow"], ["_bmad/**"]),
        ("build", "bmad-build", ["run uv render_skill.py exactly once; stop on HALT or failure"], ["_bmad-output/**"]),
        ("method_review", "bmad-code-review", ["follow rendered review route; preserve independent reviewer"], ["_bmad-output/**"]),]
    superpowers = [
        ("entry", "using-superpowers", ["invoke applicable skills before response or action"], [".agents/skills/**"]),
        ("debug", "systematic-debugging", ["investigate the root cause before proposing a fix"], ["tools/task-record/task_record.py"]),
        ("implement", "test-driven-development", ["write failing test; observe red; implement minimal green"], ["**/*"]),
        ("request_review", "requesting-code-review", ["request independent review with exact diff and tests"], []),
        ("verify", "verification-before-completion", ["run fresh verification before any completion claim"], ["**/*"]),]
    common = dict(cli=cli, executable=executable, task_brief=task_brief,
                  approval_policy=approval_policy, task_context_root=project_root,
                  public_files=public_files, public_tests=public_tests, scope=scope)
    workflows = [
        _workflow(method_id="speckit", framework="SpecKit", project_root=roots["speckit"], task_ref=task_ref, skill_root=skill_root, stages=speckit, **common),
        _workflow(method_id="openspec", framework="OpenSpec", project_root=roots["openspec"], task_ref=task_ref, skill_root=skill_root, stages=openspec, **common),
        _workflow(method_id="bmad", framework="BMAD V7 preview (bmod-method 6.13.0-next)", project_root=roots["bmad"], task_ref=task_ref, skill_root=skill_root, stages=bmad, **common),
        _workflow(method_id="superpowers", framework="Superpowers", project_root=roots["superpowers"], task_ref=task_ref, skill_root=skill_root, stages=superpowers, **common),]
    next(phase for phase in workflows[2]["phases"] if phase["id"] == "method_review")["minNativeReviewerAgents"] = 2
    return {"schema": SCHEMA, "version": "native-v2", "workflows": workflows}

def write_registry(path: Path, **kwargs) -> dict:
    registry = build_vendor_registry(**kwargs)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(registry, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return registry
