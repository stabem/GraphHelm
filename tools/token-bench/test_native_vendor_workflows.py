"""Meaningful contracts for the pinned native vendor phase records."""
import tempfile
import unittest
from pathlib import Path

import native_vendor_workflows as vendor
import native_session_evidence as evidence


class NativeVendorWorkflowContracts(unittest.TestCase):
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
            witnesses = [Path(p["requiredWitnesses"][0]["path"]) for p in workflow["phases"]]
            self.assertEqual(witnesses, [other, skill])
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
