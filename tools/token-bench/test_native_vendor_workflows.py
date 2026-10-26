"""Meaningful contracts for the pinned native vendor phase records."""
import tempfile
import unittest
from pathlib import Path

import native_vendor_workflows as vendor


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

    def test_phase_operation_metadata_uses_machine_key_output_json(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            skill = root / ".agents" / "skills" / "speckit-plan" / "SKILL.md"
            skill.parent.mkdir(parents=True)
            skill.write_text("pinned skill\n", encoding="utf-8")
            phase = vendor._phase(phase_id="plan", skill="speckit-plan", cli="codex",
                                  task_ref="task-record-binding", task_brief="bounded task",
                                  approval_policy="fixture approval", project_root=root,
                                  operations=[], output_globs=[])
            operation = phase["requiredOperations"][0]
            self.assertEqual(operation["outputJson"], True)
            self.assertNotIn("outputJSON", operation)
            self.assertIn("setup-plan", operation["commandPattern"])


if __name__ == "__main__":
    unittest.main()
