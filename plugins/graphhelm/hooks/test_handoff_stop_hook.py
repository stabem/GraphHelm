"""Contract tests for the verification handoff Stop gate."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import handoff_stop_hook as hook

SCRIPT = Path(__file__).with_name("handoff_stop_hook.py")

# The ml-saas reply that started this gate: the fix was deployed, then the user was sent to test.
ML_SAAS = ("The fix passed a blind review, was merged, and is deployed on multi.\nWhat you do now:\n"
           "1. Try it in the app: paste the same link again in “Adicionar item”.\n"
           "2. E-mail design: look at design/emails/email-link-de-acesso.html and tell me “aprovado”.")


def run(payload):
    out = subprocess.run([sys.executable, str(SCRIPT)], input=json.dumps(payload),
                         capture_output=True, text=True, timeout=10)
    return out.returncode, out.stdout


class HandoffStopHook(unittest.TestCase):
    def test_ml_saas_handoff_blocks(self):
        code, out = run({"last_assistant_message": ML_SAAS})
        self.assertEqual(code, 0)
        verdict = json.loads(out)
        self.assertEqual(verdict["decision"], "block")
        self.assertIn("observer", verdict["reason"])

    def test_portuguese_handoffs_block(self):
        for text in ("Corrigi. Vai lá e testa no app.", "Pronto, me avisa se funcionou.", "Testa aí no app.", "Pode testar aí?", "Abre o Studio e testa.",
                     "Teste você no site."):
            self.assertIsNotNone(hook.decide(text), text)

    def test_product_decision_and_proof_pass(self):
        self.assertIsNone(hook.decide("Look at the e-mail design and tell me “aprovado”."))
        self.assertIsNone(hook.decide("Playwright ran the add-item journey on the live site: green."))
        for text in ("O Codex vai testar a jornada no site.", "O observer vai testar a jornada depois do deploy.",
                     "Can you check that the new design matches the brief? Approve A or B.",
                     "Pode conferir o layout e escolher entre A e B?"):
            self.assertIsNone(hook.decide(text), text)
        self.assertIsNone(hook.decide("OBSERVER_MISSING: no browser here. Try it in the app."))

    def test_reentry_and_bad_input_let_turn_end(self):
        self.assertEqual(run({"last_assistant_message": ML_SAAS, "stop_hook_active": True}), (0, ""))
        out = subprocess.run([sys.executable, str(SCRIPT)], input="not json",
                             capture_output=True, text=True, timeout=10)
        self.assertEqual((out.returncode, out.stdout), (0, ""))

    def test_reads_transcript_tail(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "t.jsonl"
            rows = [{"message": {"role": "user", "content": "fix it"}},
                    {"message": {"role": "assistant", "content": [{"type": "text", "text": ML_SAAS}]}}]
            path.write_text("\n".join(json.dumps(r) for r in rows))
            self.assertIsNotNone(hook.decide(hook.last_message({"transcript_path": str(path)})))


if __name__ == "__main__":
    unittest.main()
