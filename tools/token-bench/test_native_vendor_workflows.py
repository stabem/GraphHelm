"""Meaningful contracts for the pinned native vendor phase records."""
import tempfile
import unittest
from pathlib import Path
import sys
import json

sys.path.insert(0, str(Path(__file__).resolve().parent))

import native_vendor_workflows as vendor
import native_session_evidence as evidence


class NativeVendorWorkflowContracts(unittest.TestCase):
    def test_openspec_windows_command_is_a_native_observed_operation(self):
        with tempfile.TemporaryDirectory() as directory:
            stream = Path(directory) / "native.jsonl"
            stream.write_text("\n".join(json.dumps(row) for row in (
                {"type": "assistant", "message": {"content": [{"type": "tool_use",
                    "id": "tool-1", "name": "PowerShell", "input": {
                    "command": "D:/fixture/openspec.cmd new change binding"}}]}},
                {"type": "user", "message": {"content": [{"type": "tool_result",
                    "tool_use_id": "tool-1", "content": "Created change", "is_error": False}]},
                    "tool_use_result": {"exitCode": 0}})) + "\n", encoding="utf-8")
            required = [{"name": "propose", "commandPattern":
                         r"openspec(?:\.cmd)?\s+(?:new\s+change|instructions)"}]
            self.assertEqual(evidence.required_operations(stream, cli="claude",
                                                            requirements=required)["status"], "observed")

    def test_openspec_explore_requires_native_read_without_writing_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for method, skills in {
                "speckit": ("speckit-constitution", "speckit-specify", "speckit-plan",
                            "speckit-tasks", "speckit-implement"),
                "openspec": ("openspec-explore", "openspec-propose", "openspec-apply-change"),
                "bmad": ("bmad", "bmad-build", "bmad-code-review"),
                "superpowers": ("using-superpowers", "systematic-debugging",
                                "test-driven-development", "requesting-code-review",
                                "verification-before-completion"),
            }.items():
                for name in skills:
                    skill = root / method / ".claude/skills" / name / "SKILL.md"
                    skill.parent.mkdir(parents=True, exist_ok=True)
                    skill.write_text(name, encoding="utf-8")
            registry = vendor.build_vendor_registry(
                project_root=root / "openspec", task_ref="task-record-binding",
                asset_root=root, project_roots={name: root / name for name in vendor.PINNED},
                cli="claude", executable=Path(__file__), public_files=[], public_tests=[],
                scope=["task_record.py"], task_brief="fix binding")
            explore = next(phase for workflow in registry["workflows"]
                           if workflow["id"] == "native-openspec"
                           for phase in workflow["phases"] if phase["id"] == "explore")
            self.assertEqual(explore["requiredArtifactGlobs"], [])
            self.assertEqual(explore["requiredOperations"][0]["name"], "explore")
            request_review = next(phase for workflow in registry["workflows"]
                                  if workflow["id"] == "native-superpowers"
                                  for phase in workflow["phases"] if phase["id"] == "request_review")
            self.assertEqual(request_review["requiredArtifactGlobs"], [])
            bmad = next(workflow for workflow in registry["workflows"]
                        if workflow["id"] == "native-bmad")
            setup = next(phase for phase in bmad["phases"] if phase["id"] == "setup")
            method_review = next(phase for phase in bmad["phases"]
                                 if phase["id"] == "method_review")
            self.assertIn("bmad/scripts/setup", setup["requiredOperations"][0]["commandPattern"])
            self.assertIn("render_skill", method_review["requiredOperations"][0]["commandPattern"])
            self.assertEqual(method_review["minNativeReviewerAgents"], 2)

    def test_phase_uses_native_prompt_and_phase_skill_witness_without_cli_flags(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            skill = root / ".agents" / "skills" / "speckit-plan" / "SKILL.md"
            skill.parent.mkdir(parents=True)
            skill.write_text("pinned skill\n", encoding="utf-8")
            phase = vendor._phase(phase_id="plan", skill="speckit-plan", cli="codex",
                                  task_ref="task-record-binding", task_brief="bounded task",
                                  approval_policy="fixture approval", project_root=root,
                                  operations=[], output_globs=["specs/*/plan.md"])
            self.assertEqual(phase["command"], [])
            self.assertEqual(phase["nativeEntrypoint"] if "nativeEntrypoint" in phase else "$speckit-plan", "$speckit-plan")
            witness = phase["requiredWitnesses"][0]
            self.assertEqual(Path(witness["path"]), skill)
            self.assertEqual(witness["sha256"], vendor._digest(skill))

    def test_assembled_workflow_preserves_each_selected_native_skill_and_consumable_operations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            skill = root / ".agents" / "skills" / "speckit-plan" / "SKILL.md"
            skill.parent.mkdir(parents=True)
            skill.write_text("pinned skill\n", encoding="utf-8")
            other = root / ".agents/skills/speckit-constitution/SKILL.md"
            other.parent.mkdir(parents=True)
            other.write_text("distinct constitution\n", encoding="utf-8")
            (root / "request.md").write_text("Public task context\n", encoding="utf-8")
            workflow = vendor._workflow(method_id="speckit", framework="SpecKit", project_root=root,
                task_ref="task-record-binding", skill_root=".agents/skills", cli="codex",
                executable=Path(__file__), stages=[("constitution", "speckit-constitution", [], []),
                ("plan", "speckit-plan", [], [])], task_brief="bounded task", approval_policy="local only",
                task_context_root=root, public_files=["request.md"], public_tests=[], scope=["subject.py"])
            witnesses = [Path(p["requiredWitnesses"][0]["path"]) for p in workflow["phases"]
                         if p["requiredWitnesses"]]
            self.assertEqual(witnesses, [other, skill])
            self.assertEqual(workflow["phases"][-1]["id"], "review")
            self.assertEqual(workflow["phases"][-1]["requiredWitnessKinds"], ["native_stream"])
            self.assertEqual(workflow["pinnedPublicFiles"], ["request.md"])
            self.assertEqual(workflow["taskContext"]["scope"], ["subject.py"])
            # Feed the assembled contract to the actual evidence consumer. A malformed
            # outputJson flag previously crashed here despite the factory unit tests.
            stream = root / "native.jsonl"
            stream.write_text('{"type":"item.completed","item":{"type":"command_execution","id":"tool-1",'
                              '"command":"pwsh -File .specify/scripts/powershell/setup-plan.ps1 -Json",'
                              '"status":"completed","exit_code":0,"aggregated_output":"plan paths"}}\n', encoding="utf-8")
            requirements = workflow["phases"][1]["requiredOperations"]
            self.assertEqual(evidence.required_operations(stream, cli="codex", requirements=requirements)["status"], "observed")
            invalid = [{**requirements[0], "outputJson": True}]
            self.assertNotEqual(evidence.required_operations(stream, cli="codex", requirements=invalid)["status"], "observed")


if __name__ == "__main__":
    unittest.main()
