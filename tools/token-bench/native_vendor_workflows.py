"""Pinned native vendor workflow registry factory (planning only)."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

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

def _phase(*, phase_id: str, skill: str, cli: str, task_ref: str,
           task_brief: str, approval_policy: str, project_root: Path,
           operations: list[str], output_globs: list[str]) -> dict:
    prefix = "$" if cli == "codex" else "/"
    prompt = (f"{prefix}{skill} {task_brief}\n"
              f"Task ref: {task_ref}. Phase boundary: execute only {phase_id}; "
              f"stop after its declared artifacts. Approval policy: {approval_policy}.")
    return {"id": phase_id, "command": ["--add-dir", str(project_root.resolve())],
            "prompt": prompt, "frameworkStage": skill,
            "requiredOperations": [{"name": operation, "commandPattern": operation,
                                     "outputJSON": False} for operation in operations],
            "requiredArtifactGlobs": output_globs,
            "requiredArtifacts": [], "requiredWitnessKinds": ["native_stream", "native_skill"],
            "costKinds": ["native_cli", skill]}

def _workflow(*, method_id: str, framework: str, project_root: Path,
              task_ref: str, skill_root: str, cli: str, executable: Path,
              stages: list[tuple[str, str, list[str], list[str]]],
              task_brief: str, approval_policy: str,
              task_context_root: Path, public_files: list[str]) -> dict:
    if not project_root.is_dir() or not executable.is_file():
        raise ValueError("native project fixture or executable is missing")
    phases = [_phase(phase_id=phase_id, skill=skill, cli=cli, task_ref=task_ref,
                     task_brief=task_brief, approval_policy=approval_policy,
                     project_root=project_root, operations=ops, output_globs=globs)
              for phase_id, skill, ops, globs in stages]
    first = Path(skill_root) / stages[0][1] / "SKILL.md"
    first_abs = project_root / first
    return {"id": f"native-{method_id}", "version": f"pinned-{PINNED[method_id]}",
            "framework": framework, "maxFixAttempts": 1,
            "projectRoot": str(project_root.resolve()), "taskRefs": [task_ref],
            "pinnedPublicFiles": [str(first).replace("\\", "/")],
            "methodAssetDigests": _asset_digests(project_root / skill_root),
            "native": {"cli": cli, "executable": str(executable.resolve()),
                        "integration": "native-project-skills",
                        "taskContextRoot": str(task_context_root.resolve()),
                        "publicTaskFiles": list(public_files), "skillRoot": skill_root,
                        "assetRoot": str((project_root / skill_root).resolve()),
                        "pin": PINNED[method_id]}, "phases": [
                            {**phase, "requiredWitnesses": [{"kind": "native_skill", "path": str(first),
                                                              "sha256": _digest(first_abs)}]}
                            for phase in phases]}

def build_vendor_registry(*, project_root: Path, task_ref: str, asset_root: Path,
                          project_roots: dict[str, Path],
                          cli: str, executable: Path, public_files: list[str],
                          task_brief: str = "Implement issue-2074 while preserving repository gates.",
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
        ("clarify", "speckit-clarify", ["read spec; record bounded clarifications"], ["specs/*/spec.md", "specs/*/checklists/requirements.md"]),
        ("plan", "speckit-plan", ["run setup-plan.ps1 -Json; read spec and constitution; write design artifacts"], ["specs/*/plan.md", "specs/*/research.md", "specs/*/data-model.md", "specs/*/contracts/**"]),
        ("tasks", "speckit-tasks", ["run check-prerequisites.ps1 -Json -RequireTasks -IncludeTasks; write tasks.md"], ["specs/*/tasks.md"]),
        ("implement", "speckit-implement", ["read tasks, plan, contracts and checklists; implement and run declared tests"], ["**/*"]),
        ("converge", "speckit-converge", ["assess implementation against spec, plan and tasks; append remaining tasks"], ["specs/*/tasks.md"]),
        ("review", "speckit-checklist", ["scan checklists; never mark unchecked items"], ["specs/*/checklists/*"]),]
    openspec = [
        ("explore", "openspec-explore", ["run openspec list --json; keep exploration read-only"], ["openspec/changes/*/**"]),
        ("propose", "openspec-propose", ["confirm local root; create proposal, specs, design and tasks"], ["openspec/changes/*/proposal.md", "openspec/changes/*/tasks.md"]),
        ("apply", "openspec-apply-change", ["read apply instructions and every context file; implement and validate"], ["**/*"]),
        ("sync", "openspec-sync-specs", ["read delta specs; edit main specs only after approval"], ["openspec/specs/**"]),
        ("review", "openspec-explore", ["inspect status and artifacts without writing"], ["openspec/changes/*/**"]),
        ("archive", "openspec-archive-change", ["validate; archive only a completed change"], ["openspec/changes/archive/**"]),]
    bmad = [
        ("setup", "bmad", ["run installed knowledge command; do not infer workflow"], ["_bmad/**"]),
        ("build", "bmad-build", ["run uv render_skill.py exactly once; stop on HALT or failure"], ["_bmad-output/**"]),
        ("review", "bmad-code-review", ["follow rendered review route; preserve independent reviewer"], ["_bmad-output/**"]),
        ("verify", "bmad-qa-generate-e2e-tests", ["generate or run only declared QA evidence"], ["_bmad-output/**"]),]
    superpowers = [
        ("entry", "using-superpowers", ["invoke applicable skills before response or action"], [".agents/skills/**"]),
        ("brainstorm", "brainstorming", ["brainstorm before implementation; record decisions"], ["docs/superpowers/**"]),
        ("plan", "writing-plans", ["write a bite-sized plan before code"], ["docs/superpowers/plans/**"]),
        ("implement", "test-driven-development", ["write failing test; observe red; implement minimal green"], ["**/*"]),
        ("review", "requesting-code-review", ["request independent review with exact diff and tests"], ["docs/superpowers/**"]),
        ("verify", "verification-before-completion", ["run fresh verification before any completion claim"], ["**/*"]),]
    common = dict(cli=cli, executable=executable, task_brief=task_brief,
                  approval_policy=approval_policy, task_context_root=project_root,
                  public_files=public_files)
    workflows = [
        _workflow(method_id="speckit", framework="SpecKit", project_root=roots["speckit"], task_ref=task_ref, skill_root=skill_root, stages=speckit, **common),
        _workflow(method_id="openspec", framework="OpenSpec", project_root=roots["openspec"], task_ref=task_ref, skill_root=skill_root, stages=openspec, **common),
        _workflow(method_id="bmad", framework="BMAD V7 preview (bmod-method 6.13.0-next)", project_root=roots["bmad"], task_ref=task_ref, skill_root=skill_root, stages=bmad, **common),
        _workflow(method_id="superpowers", framework="Superpowers", project_root=roots["superpowers"], task_ref=task_ref, skill_root=skill_root, stages=superpowers, **common),]
    return {"schema": SCHEMA, "version": "native-v2", "workflows": workflows}

def write_registry(path: Path, **kwargs) -> dict:
    registry = build_vendor_registry(**kwargs)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(registry, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return registry
