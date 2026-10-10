# Read-only assessment

Review hook_input at this frozen revision. The native host sends one complete small JSON object but keeps stdin open while awaiting the hook response. Report verdict (BLOCK or APPROVE), function, waitsForEof, rejectsOversize, rejectsNonObject, and findings as an array of defect codes chosen from EOF_WAIT, NO_SIZE_LIMIT, ACCEPTS_NON_OBJECT. Report only defects supported by the source; do not fix them.

Read only the named source files: `plugins/graphhelm/hooks/session_hook.py`. Leave the checkout unchanged. Return only one JSON object, without Markdown fences. Object keys are {"verdict": "string", "function": "string", "waitsForEof": "boolean", "rejectsOversize": "boolean", "rejectsNonObject": "boolean", "findings": "array"}. Array order does not matter. Do not include other findings or fields.
